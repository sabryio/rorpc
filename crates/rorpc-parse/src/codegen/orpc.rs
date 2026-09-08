//! Code generation for the `#[rorpc(method, path)]` attribute macro.
//!
//! Parses the attribute arguments, analyses the handler signature, and emits:
//! - The original function unchanged
//! - An `inventory::submit!` for `HandlerMetadata`
//! - An `inventory::submit!` for `HandlerRegistration` (Axum router factory)
//! - `inventory::submit!` blocks for `SchemaRegistration` fallback schemas

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Expr, ExprLit, ItemFn, Lit, MetaNameValue, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

use crate::{
    errors::{Error, Result, type_display},
    functions::extract_handler_signature,
    types::{JSON, QUERY, RESULT, innermost_custom_type, is_primitive, try_extract_wrapper},
};

// ---------------------------------------------------------------------------
// Attribute name constants — centralized for easy renaming
// ---------------------------------------------------------------------------

const ATTR_METHOD: &str = "method";
const ATTR_PATH: &str = "path";
const ATTR_DATA: &str = "data";

// ---------------------------------------------------------------------------
// OrpcArgs — parsed from #[orpc(method = "...", path = "...", data = TypePath)]
// ---------------------------------------------------------------------------

/// Parsed arguments for the `#[orpc(...)]` attribute.
pub struct OrpcArgs {
    pub method: String,
    pub path: String,
    pub stream_event: Option<String>,
}

// ---------------------------------------------------------------------------
// MethodShorthandArgs — parsed from #[orpc::get("/path")] or #[orpc::post("/path", data = "Type")]
// ---------------------------------------------------------------------------

/// Parsed arguments for method-specific shorthand macros like `#[orpc::get("/path")]`.
///
/// Syntax: `#[orpc::get("/path")]` or `#[orpc::post("/path", data = "StreamEvent")]`
pub struct MethodShorthandArgs {
    pub path: String,
    pub data: Option<String>,
}

impl Parse for MethodShorthandArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // First token must be a string literal (the path)
        let path_lit: syn::LitStr = input.parse()?;
        let path = path_lit.value();

        // Optional: comma + data = "Type"
        let mut data = None;

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;

            let pairs = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;

            for pair in &pairs {
                let key = pair
                    .path
                    .get_ident()
                    .map(|i| i.to_string())
                    .unwrap_or_default();

                let span = pair
                    .path
                    .get_ident()
                    .map(|i| i.span())
                    .unwrap_or_else(proc_macro2::Span::call_site);

                match key.as_str() {
                    ATTR_DATA => match &pair.value {
                        Expr::Lit(ExprLit {
                            lit: Lit::Str(s), ..
                        }) => {
                            data = Some(s.value());
                        }
                        Expr::Path(expr_path) => {
                            let type_path = syn::TypePath {
                                attrs: vec![],
                                qself: expr_path.qself.clone(),
                                path: expr_path.path.clone(),
                            };
                            data = Some(type_display(&syn::Type::Path(type_path)));
                        }
                        _ => {
                            return Err(syn::Error::new(
                                span,
                                format!(
                                    "{} must be a string literal (\"StreamEvent\") or type path (StreamEvent)",
                                    ATTR_DATA
                                ),
                            ));
                        }
                    },
                    _ => {
                        return Err(syn::Error::new(
                            span,
                            Error::unknown_key(span, &key, &[ATTR_DATA]).to_string(),
                        ));
                    }
                }
            }
        }

        Ok(MethodShorthandArgs { path, data })
    }
}

/// Convert method shorthand args to standard OrpcArgs.
///
/// This allows method-specific macros like `#[orpc::get("/path")]` to reuse
/// all the existing codegen logic without duplication.
impl MethodShorthandArgs {
    pub fn into_orpc_args(self, method: &str) -> OrpcArgs {
        OrpcArgs {
            method: method.to_uppercase(),
            path: self.path,
            stream_event: self.data,
        }
    }
}

const VALID_KEYS: &[&str] = &[ATTR_METHOD, ATTR_PATH, ATTR_DATA];

impl Parse for OrpcArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let pairs = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;

        let mut method = None;
        let mut path = None;
        let mut stream_event = None;

        for pair in &pairs {
            let key = pair
                .path
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_default();

            let span = pair
                .path
                .get_ident()
                .map(|i| i.span())
                .unwrap_or_else(proc_macro2::Span::call_site);

            match key.as_str() {
                ATTR_METHOD => {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &pair.value
                    {
                        method = Some(s.value().to_uppercase());
                    } else {
                        return Err(syn::Error::new(
                            span,
                            Error::invalid_attr_value(
                                span,
                                &key,
                                "a string literal",
                                "non-string expression",
                            )
                            .to_string(),
                        ));
                    }
                }
                ATTR_PATH => {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &pair.value
                    {
                        path = Some(s.value());
                    } else {
                        return Err(syn::Error::new(
                            span,
                            Error::invalid_attr_value(
                                span,
                                &key,
                                "a string literal",
                                "non-string expression",
                            )
                            .to_string(),
                        ));
                    }
                }
                ATTR_DATA => {
                    match &pair.value {
                        // String literal: data = "StreamEvent" or data = "module::StreamEvent"
                        // Preferred syntax for IDE support
                        Expr::Lit(ExprLit {
                            lit: Lit::Str(s), ..
                        }) => {
                            stream_event = Some(s.value());
                        }
                        // Type path: data = StreamEvent (backward compatibility)
                        Expr::Path(expr_path) => {
                            let type_path = syn::TypePath {
                                attrs: vec![],
                                qself: expr_path.qself.clone(),
                                path: expr_path.path.clone(),
                            };
                            stream_event = Some(type_display(&syn::Type::Path(type_path)));
                        }
                        _ => {
                            return Err(syn::Error::new(
                                span,
                                format!(
                                    "{} must be a string literal (\"StreamEvent\") or type path (StreamEvent)",
                                    ATTR_DATA
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new(
                        span,
                        Error::unknown_key(span, &key, VALID_KEYS).to_string(),
                    ));
                }
            }
        }

        let method = method.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                Error::missing_required_attr(
                    proc_macro2::Span::call_site(),
                    ATTR_METHOD,
                    "add `method = \"GET\"` to #[orpc]",
                )
                .to_string(),
            )
        })?;

        let path = path.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                Error::missing_required_attr(
                    proc_macro2::Span::call_site(),
                    ATTR_PATH,
                    "add `path = \"/your/route\"` to #[orpc]",
                )
                .to_string(),
            )
        })?;

        Ok(OrpcArgs {
            method,
            path,
            stream_event,
        })
    }
}

// ---------------------------------------------------------------------------
// expand_orpc
// ---------------------------------------------------------------------------

/// Generate the full expansion for `#[rorpc::route(method, path)]` or shorthand macros.
///
/// Returns the original function unchanged plus all inventory registrations.
pub fn expand_orpc(args: OrpcArgs, func: ItemFn) -> TokenStream {
    match try_expand_orpc(args, func) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error(),
    }
}

fn try_expand_orpc(args: OrpcArgs, func: ItemFn) -> Result<TokenStream> {
    let sig = extract_handler_signature(&func)?;

    let fn_name = &func.sig.ident;
    let fn_name_str = sig.fn_name.as_str();
    let method = &args.method;
    let path = &args.path;

    let output_type_str = type_display(&sig.output_type);

    let error_type_token = match &sig.error_type {
        Some(ty) => {
            let s = type_display(ty);
            quote! { Some(#s) }
        }
        None => quote! { None },
    };

    let stream_event_token = match &args.stream_event {
        Some(type_name) => {
            let s = type_name.as_str();
            quote! { Some(#s) }
        }
        None => quote! { None },
    };

    let input_type_str = match &sig.input_type {
        Some(ty) => type_display(ty),
        None => "()".to_string(),
    };

    let query_type_token = match &sig.query_type {
        Some(ty) => {
            let s = type_display(ty);
            quote! { Some(#s) }
        }
        None => quote! { None },
    };

    // Encode path param types as comma-separated string: "i32,String"
    // Order matches the path template param order (declaration order in signature)
    let path_param_types_str = sig
        .path_params
        .iter()
        .map(|(_, ty)| type_display(ty))
        .collect::<Vec<_>>()
        .join(",");

    let registration = emit_handler_registration(fn_name, method, path, &sig.state_type);
    let schema_registrations = emit_schema_registrations(&func);

    Ok(quote! {
        #func

        ::rorpc::inventory::submit! {
            ::rorpc::HandlerMetadata {
                name: #fn_name_str,
                method: #method,
                path: #path,
                input_type_name: #input_type_str,
                query_type_name: #query_type_token,
                output_type_name: #output_type_str,
                module_path: ::std::module_path!(),
                namespace: None,
                error_type_name: #error_type_token,
                stream_event_type_name: #stream_event_token,
                path_param_types: #path_param_types_str,
            }
        }

        #registration
        #schema_registrations
    })
}

// ---------------------------------------------------------------------------
// Handler registration factory
// ---------------------------------------------------------------------------

fn emit_handler_registration(
    fn_name: &syn::Ident,
    method: &str,
    path: &str,
    state_type: &Option<syn::Type>,
) -> TokenStream {
    if let Some(state_ty) = state_type {
        quote! {
            ::rorpc::inventory::submit! {
                ::rorpc::HandlerRegistration {
                    path: #path,
                    method: #method,
                    factory: |state: ::std::sync::Arc<dyn ::std::any::Any + Send + Sync>, final_path: &str| {
                        use ::axum::routing::{delete, get, patch, post, put};
                        let method_router = match #method {
                            "GET"    => get(#fn_name),
                            "POST"   => post(#fn_name),
                            "PUT"    => put(#fn_name),
                            "PATCH"  => patch(#fn_name),
                            "DELETE" => delete(#fn_name),
                            _        => post(#fn_name),
                        };
                        if let Some(typed_state) = state.downcast_ref::<#state_ty>() {
                            ::axum::Router::new()
                                .route(final_path, method_router)
                                .with_state(typed_state.clone())
                        } else {
                            ::axum::Router::new()
                        }
                    },
                }
            }
        }
    } else {
        quote! {
            ::rorpc::inventory::submit! {
                ::rorpc::HandlerRegistration {
                    path: #path,
                    method: #method,
                    factory: |_state: ::std::sync::Arc<dyn ::std::any::Any + Send + Sync>, final_path: &str| {
                        use ::axum::routing::{delete, get, patch, post, put};
                        let method_router = match #method {
                            "GET"    => get(#fn_name),
                            "POST"   => post(#fn_name),
                            "PUT"    => put(#fn_name),
                            "PATCH"  => patch(#fn_name),
                            "DELETE" => delete(#fn_name),
                            _        => post(#fn_name),
                        };
                        ::axum::Router::new().route(final_path, method_router)
                    },
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Schema registrations — z.unknown() fallback for types without #[derive(ZodTs)]
// ---------------------------------------------------------------------------

fn emit_schema_registrations(func: &ItemFn) -> TokenStream {
    let mut seen = std::collections::HashSet::new();
    let mut registrations = Vec::new();

    // Collect candidate types from Json<T> and Query<T> params and return type
    let mut candidates: Vec<&syn::Type> = Vec::new();

    for arg in &func.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = arg {
            // Check for Json<T>
            if let Some(m) = try_extract_wrapper(&pat_type.ty, JSON)
                && let Some(inner) = m.first_type()
            {
                candidates.push(inner);
            }
            // Check for Query<T>
            if let Some(m) = try_extract_wrapper(&pat_type.ty, QUERY)
                && let Some(inner) = m.first_type()
            {
                candidates.push(inner);
            }
        }
    }

    if let syn::ReturnType::Type(_, ty) = &func.sig.output {
        // Handle both Json<T> and Result<Json<T>, E>
        if let Some(m) = try_extract_wrapper(ty, JSON) {
            if let Some(inner) = m.first_type() {
                candidates.push(inner);
            }
        } else if let Some(result_m) = try_extract_wrapper(ty, RESULT)
            && let Some(first) = result_m.first_type()
            && let Some(json_m) = try_extract_wrapper(first, JSON)
            && let Some(inner) = json_m.first_type()
        {
            candidates.push(inner);
        }
    }

    for ty in candidates {
        if let Some(custom_ty) = innermost_custom_type(ty) {
            if is_primitive(custom_ty) {
                continue;
            }
            let name = type_display(custom_ty);
            if !seen.insert(name.clone()) {
                continue;
            }
            registrations.push(quote! {
                ::rorpc::inventory::submit! {
                    ::rorpc::SchemaRegistration {
                        type_name: #name,
                        module_path: "",
                        schema_def: ::rorpc::SchemaDef::Unknown,
                        dependent_types: || vec![],
                    }
                }
            });
        }
    }

    quote! { #(#registrations)* }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn parse_data_type_string() {
        // Test that data = "StreamEvent" (string literal) parses correctly
        let args: OrpcArgs = syn::parse_quote! {
            method = "GET", path = "/stream", data = "StreamEvent"
        };

        assert_eq!(args.method, "GET");
        assert_eq!(args.path, "/stream");
        assert_eq!(args.stream_event, Some("StreamEvent".to_string()));
    }

    #[test]
    fn parse_data_qualified_path_string() {
        // Test that data = "crate::models::StreamEvent" works
        let args: OrpcArgs = syn::parse_quote! {
            method = "GET", path = "/stream", data = "crate::models::StreamEvent"
        };

        assert_eq!(
            args.stream_event,
            Some("crate::models::StreamEvent".to_string())
        );
    }

    #[test]
    fn parse_data_type_path_backward_compat() {
        // Test backward compatibility: data = StreamEvent (bare path)
        let args: OrpcArgs = syn::parse_quote! {
            method = "GET", path = "/stream", data = StreamEvent
        };

        assert_eq!(args.method, "GET");
        assert_eq!(args.path, "/stream");
        assert_eq!(args.stream_event, Some("StreamEvent".to_string()));
    }

    #[test]
    fn parse_without_data() {
        // Test that data is optional
        let args: OrpcArgs = syn::parse_quote! {
            method = "POST", path = "/create"
        };

        assert_eq!(args.method, "POST");
        assert_eq!(args.path, "/create");
        assert_eq!(args.stream_event, None);
    }

    #[test]
    fn data_type_converts_to_string_literal() {
        // Verify that when we generate the metadata, data becomes a string literal
        let args: OrpcArgs = syn::parse_quote! {
            method = "GET", path = "/stream", data = "StreamEvent"
        };

        let func: syn::ItemFn = parse_quote! {
            async fn stream_test() -> Sse<impl Stream<Item = Event>> {
                todo!()
            }
        };

        let result = try_expand_orpc(args, func);
        assert!(result.is_ok(), "expand_orpc should succeed");

        // Check that the generated code contains Some("StreamEvent") as a string literal
        let tokens = result.unwrap().to_string();

        // quote! serialises with spaces between tokens, so `Some("StreamEvent")` becomes
        // `Some ("StreamEvent")`. Check the field name and the quoted value separately.
        assert!(
            tokens.contains("stream_event_type_name") && tokens.contains(r#""StreamEvent""#),
            "Generated code should contain stream_event_type_name: Some(\"StreamEvent\"), got: {}",
            tokens
        );
        // Also assert it is NOT a bare identifier (which would be a type error at compile time)
        assert!(
            !tokens.contains("Some (StreamEvent)") && !tokens.contains("Some(StreamEvent)"),
            "stream_event_type_name must be a string literal, not a bare identifier"
        );
    }
}

// ---------------------------------------------------------------------------
// MethodShorthandArgs tests
// ---------------------------------------------------------------------------

#[test]
fn parse_shorthand_path_only() {
    // Test: #[rorpc::get("/planet/list")]
    let args: MethodShorthandArgs = syn::parse_quote! { "/planet/list" };

    assert_eq!(args.path, "/planet/list");
    assert_eq!(args.data, None);
}

#[test]
fn parse_shorthand_with_data_string() {
    // Test: #[rorpc::get("/stream", data = "EventData")]
    let args: MethodShorthandArgs = syn::parse_quote! { "/stream", data = "EventData" };

    assert_eq!(args.path, "/stream");
    assert_eq!(args.data, Some("EventData".to_string()));
}

#[test]
fn parse_shorthand_with_qualified_data() {
    // Test: #[rorpc::get("/stream", data = "models::EventData")]
    let args: MethodShorthandArgs = syn::parse_quote! { "/stream", data = "models::EventData" };

    assert_eq!(args.path, "/stream");
    assert_eq!(args.data, Some("models::EventData".to_string()));
}

#[test]
fn parse_shorthand_with_data_type_path() {
    // Test backward compat: #[rorpc::get("/stream", data = EventData)]
    let args: MethodShorthandArgs = syn::parse_quote! { "/stream", data = EventData };

    assert_eq!(args.path, "/stream");
    assert_eq!(args.data, Some("EventData".to_string()));
}

#[test]
fn shorthand_converts_to_orpc_args() {
    let shorthand: MethodShorthandArgs = syn::parse_quote! { "/planet/list" };
    let args = shorthand.into_orpc_args("GET");

    assert_eq!(args.method, "GET");
    assert_eq!(args.path, "/planet/list");
    assert_eq!(args.stream_event, None);
}

#[test]
fn shorthand_with_data_converts_to_orpc_args() {
    let shorthand: MethodShorthandArgs = syn::parse_quote! { "/stream", data = "EventData" };
    let args = shorthand.into_orpc_args("GET");

    assert_eq!(args.method, "GET");
    assert_eq!(args.path, "/stream");
    assert_eq!(args.stream_event, Some("EventData".to_string()));
}

#[test]
fn shorthand_method_normalized_to_uppercase() {
    let shorthand: MethodShorthandArgs = syn::parse_quote! { "/test" };
    let args = shorthand.into_orpc_args("get"); // lowercase input

    assert_eq!(args.method, "GET"); // should be uppercase
}
