//! Handler function signature analysis.
//!
//! Extracts a fully-typed [`HandlerSignature`] from a `syn::ItemFn` using
//! AST-based type inspection via [`crate::types`]. All type matching is done
//! on path segment idents — never on string representations.

use proc_macro2::Span;
use syn::{FnArg, ItemFn, ReturnType, Type, spanned::Spanned};

use crate::{
    errors::{Error, Result},
    types::{JSON, PATH, QUERY, RESULT, SSE, STATE, STATUSCODE, last_ident_matches, try_extract_wrapper},
};

// ---------------------------------------------------------------------------
// HandlerSignature
// ---------------------------------------------------------------------------

/// Fully analysed handler function signature.
///
/// Produced by [`extract_handler_signature`]. All fields are resolved against
/// the actual AST — no string-based type inference.
#[derive(Debug)]
pub struct HandlerSignature {
    /// The function's identifier, e.g. `"list_planets"`.
    pub fn_name: String,
    /// Span of the function identifier for error reporting.
    pub fn_span: Span,
    /// The `S` in a `State<S>` parameter, if present.
    pub state_type: Option<Type>,
    /// The `T` in a `Json<T>` parameter, if present.
    pub input_type: Option<Type>,
    /// The `T` in a `Query<T>` parameter, if present.
    pub query_type: Option<Type>,
    /// Ordered list of `(binding_name, rust_type)` pairs from `Path<T>` parameters.
    ///
    /// E.g., `Path(id): Path<i32>` → `[("id", Type::i32)]`
    /// Multiple params: `Path(id): Path<i32>, Path(slug): Path<String>` → `[("id", i32), ("slug", String)]`
    pub path_params: Vec<(String, Type)>,
    /// The resolved output type:
    /// - `Json<T>` return → `T`
    /// - `Result<Json<T>, E>` return → `T`
    /// - `Sse<...>` return → unit `()` (output type comes from `data` attribute)
    pub output_type: Type,
    /// The `E` in `Result<_, E>`, if present.
    pub error_type: Option<Type>,
    /// Whether the handler returns `Sse<...>` (an SSE streaming response).
    pub is_streaming: bool,
    /// Whether the function is declared `async`.
    pub is_async: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract a [`HandlerSignature`] from a handler function.
///
/// Validates:
/// - The return type is `Json<T>` or `Result<Json<T>, E>` (not a bare type)
/// - Collects `State<S>`, `Json<T>`, and `Query<T>` parameters when present
///
/// Handlers that return neither `Json<T>` nor `Result<Json<T>, E>` are
/// rejected with an error pointing at the return type token.
pub fn extract_handler_signature(func: &ItemFn) -> Result<HandlerSignature> {
    let fn_name = func.sig.ident.to_string();
    let fn_span = func.sig.ident.span();
    let is_async = func.sig.asyncness.is_some();

    let (output_type, error_type, is_streaming) = extract_return_types(&func.sig.output, &fn_name)?;
    let state_type = extract_state_param(&func.sig.inputs);
    let input_type = extract_json_param(&func.sig.inputs);
    let query_type = extract_query_param(&func.sig.inputs);
    let path_params = extract_path_params(&func.sig.inputs);

    Ok(HandlerSignature {
        fn_name,
        fn_span,
        state_type,
        input_type,
        query_type,
        path_params,
        output_type,
        error_type,
        is_streaming,
        is_async,
    })
}

// ---------------------------------------------------------------------------
// Internal extraction helpers
// ---------------------------------------------------------------------------

/// Extract the unwrapped output type, optional error type, and streaming flag
/// from a return type.
///
/// Accepts:
/// - `-> Json<T>` → (T, None, false)
/// - `-> Result<Json<T>, E>` → (T, Some(E), false)
/// - `-> Sse<...>` → ((), None, true)
fn extract_return_types(
    return_type: &ReturnType,
    fn_name: &str,
) -> Result<(Type, Option<Type>, bool)> {
    let ty = match return_type {
        ReturnType::Default => {
            return Err(Error::missing_return_type(
                proc_macro2::Span::call_site(),
                fn_name,
            ));
        }
        ReturnType::Type(_, ty) => ty.as_ref(),
    };

    // Case 1: Sse<...> — streaming handler; output type comes from data attribute
    if try_extract_wrapper(ty, SSE).is_some() {
        let unit: Type = syn::parse_quote! { () };
        return Ok((unit, None, true));
    }

    // Case 2: StatusCode — DELETE/PUT handlers with no response body
    if last_ident_matches(ty, STATUSCODE) {
        let unit: Type = syn::parse_quote! { () };
        return Ok((unit, None, false));
    }

    // Case 3: Json<T>
    if let Some(m) = try_extract_wrapper(ty, JSON) {
        let output = m
            .first_type()
            .ok_or_else(|| Error::empty_generic_args(ty.span(), JSON))?
            .clone();
        return Ok((output, None, false));
    }

    // Case 4: Result<Json<T>, E>
    if let Some(result_match) = try_extract_wrapper(ty, RESULT) {
        let first = result_match
            .first_type()
            .ok_or_else(|| Error::empty_generic_args(ty.span(), RESULT))?;

        let json_match = try_extract_wrapper(first, JSON).ok_or_else(|| {
            Error::invalid_handler_sig(
                first.span(),
                fn_name,
                "Result's first type argument must be Json<T>",
            )
        })?;

        let output = json_match
            .first_type()
            .ok_or_else(|| Error::empty_generic_args(first.span(), JSON))?
            .clone();

        let error_type = result_match.second_type().cloned();
        return Ok((output, error_type, false));
    }

    Err(Error::invalid_handler_sig(
        ty.span(),
        fn_name,
        "return type must be Json<T>, Result<Json<T>, E>, StatusCode, or Sse<impl Stream<...>>",
    ))
}

/// Find a `State<S>` parameter and return the inner `S`.
fn extract_state_param(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>,
) -> Option<Type> {
    for arg in inputs {
        if let FnArg::Typed(pat_type) = arg
            && let Some(m) = try_extract_wrapper(&pat_type.ty, STATE)
        {
            return m.first_type().cloned();
        }
    }
    None
}

/// Find the first `Json<T>` parameter and return the inner `T`.
fn extract_json_param(inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>) -> Option<Type> {
    for arg in inputs {
        if let FnArg::Typed(pat_type) = arg
            && let Some(m) = try_extract_wrapper(&pat_type.ty, JSON)
        {
            return m.first_type().cloned();
        }
    }
    None
}

/// Find the first `Query<T>` parameter and return the inner `T`.
fn extract_query_param(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>,
) -> Option<Type> {
    for arg in inputs {
        if let FnArg::Typed(pat_type) = arg
            && let Some(m) = try_extract_wrapper(&pat_type.ty, QUERY)
        {
            return m.first_type().cloned();
        }
    }
    None
}

/// Collect all `Path<T>` parameters, returning `(binding_name, inner_type)` pairs.
///
/// `Path(id): Path<i32>` → `("id", Type::i32)`
/// Preserves declaration order so types can be zipped with path template params.
fn extract_path_params(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::Token![,]>,
) -> Vec<(String, Type)> {
    let mut params = Vec::new();
    for arg in inputs {
        if let FnArg::Typed(pat_type) = arg
            && let Some(m) = try_extract_wrapper(&pat_type.ty, PATH)
            && let Some(inner) = m.first_type()
        {
            let name = extract_path_binding_name(&pat_type.pat);
            params.push((name, inner.clone()));
        }
    }
    params
}

/// Extract the binding name from a `Path(name)` or `Path { name }` pattern.
///
/// `Path(id)` → `"id"`, falls back to `"param"` for unrecognised patterns.
fn extract_path_binding_name(pat: &syn::Pat) -> String {
    // Most common form: Path(id) — a TupleStruct pattern
    if let syn::Pat::TupleStruct(ts) = pat
        && let Some(first) = ts.elems.first()
        && let syn::Pat::Ident(id) = first
    {
        return id.ident.to_string();
    }
    // Plain ident (uncommon but valid)
    if let syn::Pat::Ident(id) = pat {
        return id.ident.to_string();
    }
    "param".to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::type_display;
    use syn::parse_quote;

    fn sig(func: ItemFn) -> HandlerSignature {
        extract_handler_signature(&func).unwrap()
    }

    fn sig_err(func: ItemFn) -> Error {
        extract_handler_signature(&func).unwrap_err()
    }

    // --- valid signatures ---

    #[test]
    fn json_return_only() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> Json<Planet> {}
        };
        let s = sig(f);
        assert_eq!(s.fn_name, "handler");
        assert_eq!(type_display(&s.output_type), "Planet");
        assert!(s.error_type.is_none());
        assert!(s.input_type.is_none());
        assert!(s.state_type.is_none());
        assert!(s.is_async);
        assert!(!s.is_streaming);
    }

    #[test]
    fn result_json_return() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> Result<Json<Planet>, AppError> {}
        };
        let s = sig(f);
        assert_eq!(type_display(&s.output_type), "Planet");
        assert_eq!(type_display(s.error_type.as_ref().unwrap()), "AppError");
    }

    #[test]
    fn qualified_result_return() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> std::result::Result<Json<Planet>, AppError> {}
        };
        let s = sig(f);
        assert_eq!(type_display(&s.output_type), "Planet");
    }

    #[test]
    fn qualified_json_return() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> axum::extract::Json<Planet> {}
        };
        let s = sig(f);
        assert_eq!(type_display(&s.output_type), "Planet");
    }

    #[test]
    fn state_param_extracted() {
        let f: ItemFn = parse_quote! {
            async fn handler(State(db): State<Db>) -> Json<Planet> {}
        };
        let s = sig(f);
        assert_eq!(type_display(s.state_type.as_ref().unwrap()), "Db");
    }

    #[test]
    fn json_param_extracted() {
        let f: ItemFn = parse_quote! {
            async fn handler(Json(body): Json<CreatePlanet>) -> Json<Planet> {}
        };
        let s = sig(f);
        assert_eq!(type_display(s.input_type.as_ref().unwrap()), "CreatePlanet");
    }

    #[test]
    fn both_state_and_json_params() {
        let f: ItemFn = parse_quote! {
            async fn handler(State(db): State<Db>, Json(body): Json<CreatePlanet>) -> Result<Json<Planet>, AppError> {}
        };
        let s = sig(f);
        assert_eq!(type_display(s.state_type.as_ref().unwrap()), "Db");
        assert_eq!(type_display(s.input_type.as_ref().unwrap()), "CreatePlanet");
        assert_eq!(type_display(&s.output_type), "Planet");
        assert!(s.error_type.is_some());
    }

    #[test]
    fn path_param_extracted() {
        let f: ItemFn = parse_quote! {
            async fn handler(Path(id): Path<i32>) -> Json<Planet> {}
        };
        let s = sig(f);
        assert_eq!(s.path_params.len(), 1);
        assert_eq!(s.path_params[0].0, "id");
        assert_eq!(type_display(&s.path_params[0].1), "i32");
    }

    #[test]
    fn multiple_path_params_extracted() {
        let f: ItemFn = parse_quote! {
            async fn handler(Path(ws_id): Path<i32>, Path(proj_id): Path<String>) -> Json<Planet> {}
        };
        let s = sig(f);
        assert_eq!(s.path_params.len(), 2);
        assert_eq!(s.path_params[0].0, "ws_id");
        assert_eq!(type_display(&s.path_params[0].1), "i32");
        assert_eq!(s.path_params[1].0, "proj_id");
        assert_eq!(type_display(&s.path_params[1].1), "String");
    }

    #[test]
    fn path_and_query_params_both_extracted() {
        let f: ItemFn = parse_quote! {
            async fn handler(Path(id): Path<i32>, Query(q): Query<SearchQuery>) -> Json<Planet> {}
        };
        let s = sig(f);
        assert_eq!(s.path_params.len(), 1);
        assert_eq!(s.path_params[0].0, "id");
        assert_eq!(type_display(s.query_type.as_ref().unwrap()), "SearchQuery");
    }

    #[test]
    fn no_path_params_gives_empty_vec() {
        let f: ItemFn = parse_quote! {
            async fn handler(State(db): State<Db>) -> Json<Planet> {}
        };
        let s = sig(f);
        assert!(s.path_params.is_empty());
    }

    #[test]
    fn sync_function_allowed() {
        let f: ItemFn = parse_quote! {
            fn handler() -> Json<Planet> {}
        };
        let s = sig(f);
        assert!(!s.is_async);
    }

    #[test]
    fn sse_return_is_streaming() {
        let f: ItemFn = parse_quote! {
            async fn stream_events(State(_state): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {}
        };
        let s = sig(f);
        assert!(s.is_streaming);
        assert!(s.error_type.is_none());
        // output_type is unit () for SSE handlers
        assert_eq!(type_display(&s.output_type), "()");
    }

    #[test]
    fn qualified_sse_return_is_streaming() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> axum::response::Sse<SomeStream> {}
        };
        let s = sig(f);
        assert!(s.is_streaming);
    }

    #[test]
    fn statuscode_return_maps_to_unit() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> StatusCode {}
        };
        let s = sig(f);
        assert_eq!(type_display(&s.output_type), "()");
        assert!(!s.is_streaming);
    }

    #[test]
    fn qualified_statuscode_works() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> http::StatusCode {}
        };
        let s = sig(f);
        assert_eq!(type_display(&s.output_type), "()");
    }

    // --- invalid signatures ---

    #[test]
    fn no_return_type_error() {
        let f: ItemFn = parse_quote! {
            async fn handler() {}
        };
        let err = sig_err(f);
        assert!(err.to_string().contains("has no return type"));
    }

    #[test]
    fn bare_type_return_error() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> Vec<Planet> {}
        };
        let err = sig_err(f);
        assert!(err.to_string().contains("return type must be Json<T>"));
    }

    #[test]
    fn result_without_json_inner_error() {
        let f: ItemFn = parse_quote! {
            async fn handler() -> Result<Vec<Planet>, AppError> {}
        };
        let err = sig_err(f);
        assert!(
            err.to_string()
                .contains("Result's first type argument must be Json<T>")
        );
    }
}
