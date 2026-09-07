//! Code generation for the `router!(...)` proc macro.
//!
//! Parses 0–2 arguments (state expression and/or module path pattern) in any
//! order, then emits an Axum `Router` that merges all matching registered
//! handlers from the `inventory`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Expr, ExprArray, LitStr, Token,
    parse::{Parse, ParseStream},
};

// ---------------------------------------------------------------------------
// RouterArgs — parsed from router!(state?, pattern?)
// ---------------------------------------------------------------------------

/// Parsed arguments for the `router!(...)` macro.
///
/// Both arguments are optional and may appear in any order.
pub struct RouterArgs {
    pub state: Option<Expr>,
    pub pattern: Option<Pattern>,
}

impl std::fmt::Debug for RouterArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouterArgs")
            .field("has_state", &self.state.is_some())
            .field("pattern", &self.pattern)
            .finish()
    }
}

/// Module path filter pattern.
#[derive(Clone, Debug)]
pub enum Pattern {
    /// `"handlers::planet"` — single pattern
    Single(String),
    /// `["handlers::planet", "handlers::user"]` — multiple patterns
    Multiple(Vec<String>),
}

impl Parse for RouterArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut state: Option<Expr> = None;
        let mut pattern: Option<Pattern> = None;

        if !input.is_empty() {
            match parse_one_arg(input)? {
                Arg::Pattern(p) => pattern = Some(p),
                Arg::State(s) => state = Some(s),
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
                if !input.is_empty() {
                    match parse_one_arg(input)? {
                        Arg::Pattern(p) => {
                            if pattern.is_some() {
                                return Err(syn::Error::new(
                                    input.span(),
                                    "router!(): pattern specified twice",
                                ));
                            }
                            pattern = Some(p);
                        }
                        Arg::State(s) => {
                            if state.is_some() {
                                return Err(syn::Error::new(
                                    input.span(),
                                    "router!(): state specified twice",
                                ));
                            }
                            state = Some(s);
                        }
                    }
                }
            }
        }

        Ok(RouterArgs { state, pattern })
    }
}

enum Arg {
    Pattern(Pattern),
    State(Expr),
}

fn parse_one_arg(input: ParseStream) -> syn::Result<Arg> {
    if let Ok(lit) = input.parse::<LitStr>() {
        return Ok(Arg::Pattern(Pattern::Single(lit.value())));
    }
    if let Ok(array) = input.parse::<ExprArray>() {
        let patterns: syn::Result<Vec<String>> = array
            .elems
            .iter()
            .map(|elem| match elem {
                Expr::Lit(expr_lit) => {
                    if let syn::Lit::Str(s) = &expr_lit.lit {
                        Ok(s.value())
                    } else {
                        Err(syn::Error::new_spanned(
                            elem,
                            "router!(): array elements must be string literals",
                        ))
                    }
                }
                _ => Err(syn::Error::new_spanned(
                    elem,
                    "router!(): array elements must be string literals",
                )),
            })
            .collect();
        return Ok(Arg::Pattern(Pattern::Multiple(patterns?)));
    }
    Ok(Arg::State(input.parse::<Expr>()?))
}

// ---------------------------------------------------------------------------
// expand_router
// ---------------------------------------------------------------------------

/// Generate the `router!(...)` expansion.
pub fn expand_router(args: RouterArgs) -> TokenStream {
    let state_expr = match &args.state {
        Some(expr) => quote! { ::std::sync::Arc::new(#expr) },
        None => quote! { ::std::sync::Arc::new(()) },
    };

    let filter = build_filter(&args.pattern);

    quote! {
        {
            let state: ::std::sync::Arc<dyn ::std::any::Any + Send + Sync> = #state_expr;
            let mut app: ::axum::Router = ::axum::Router::new();
            
            // Build namespace lookup map: module_path -> prefix
            let mut namespace_map: ::std::collections::HashMap<&'static str, &'static str> = 
                ::std::collections::HashMap::new();
            for ns in ::rorpc::inventory::iter::<::rorpc::NamespaceMetadata> {
                namespace_map.insert(ns.module_path, ns.prefix);
            }
            
            for reg in ::rorpc::inventory::iter::<::rorpc::HandlerRegistration> {
                let matches = #filter;
                if matches {
                    // Look up namespace for this handler and compose final path
                    let mut final_path: String = reg.path.to_string();
                    for metadata in ::rorpc::inventory::iter::<::rorpc::HandlerMetadata> {
                        if metadata.path == reg.path && metadata.method == reg.method {
                            // Check if handler's module or any parent has a namespace
                            let module_parts: Vec<&str> = metadata.module_path.split("::").collect();
                            for i in (0..=module_parts.len()).rev() {
                                let parent_path = module_parts[..i].join("::");
                                if let Some(prefix) = namespace_map.get(parent_path.as_str()) {
                                    // Compose namespace + handler path
                                    final_path = format!("{}{}", prefix, reg.path);
                                    break;
                                }
                            }
                            break;
                        }
                    }
                    
                    // Call factory with composed path
                    let route = (reg.factory)(::std::sync::Arc::clone(&state), &final_path);
                    app = app.merge(route);
                }
            }
            app
        }
    }
}

// ---------------------------------------------------------------------------
// Filter predicate generation
// ---------------------------------------------------------------------------

fn build_filter(pattern: &Option<Pattern>) -> TokenStream {
    let Some(p) = pattern else {
        return quote! { true };
    };

    let expanded: Vec<String> = match p {
        Pattern::Single(s) => expand_pattern(s),
        Pattern::Multiple(vec) => vec.iter().flat_map(|s| expand_pattern(s)).collect(),
    };

    if expanded.is_empty() {
        return quote! { true };
    }

    let conditions: Vec<TokenStream> = expanded
        .iter()
        .map(|pat| {
            let with_sep = format!("{}::", pat);
            quote! {
                (metadata.module_path == #pat || metadata.module_path.starts_with(#with_sep))
            }
        })
        .collect();

    quote! {
        {
            let mut matches = false;
            for metadata in ::rorpc::inventory::iter::<::rorpc::HandlerMetadata> {
                if metadata.path == reg.path && metadata.method == reg.method {
                    matches = #(#conditions)||*;
                    break;
                }
            }
            matches
        }
    }
}

/// Expand a pattern string, handling brace groups and wildcards.
///
/// - `"handlers::{planet,user}"` → `["handlers::planet", "handlers::user"]`
/// - `"handlers::*"` → `["handlers::"]` (prefix match)
fn expand_pattern(pattern: &str) -> Vec<String> {
    if let (Some(start), Some(end)) = (pattern.find('{'), pattern.find('}')) {
        let prefix = &pattern[..start];
        let group = &pattern[start + 1..end];
        let suffix = &pattern[end + 1..];
        return group
            .split(',')
            .map(|seg| normalise(format!("{}{}{}", prefix, seg.trim(), suffix)))
            .collect();
    }
    vec![normalise(pattern.to_string())]
}

/// Normalise a single pattern: strip trailing `*` after `::`.
fn normalise(pattern: String) -> String {
    if pattern.ends_with("::*") {
        pattern[..pattern.len() - 1].to_string()
    } else {
        pattern
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_simple() {
        assert_eq!(expand_pattern("handlers::planet"), vec!["handlers::planet"]);
    }

    #[test]
    fn expand_wildcard() {
        assert_eq!(expand_pattern("handlers::*"), vec!["handlers::"]);
    }

    #[test]
    fn expand_brace_group() {
        let mut result = expand_pattern("handlers::{planet,user}");
        result.sort();
        assert_eq!(result, vec!["handlers::planet", "handlers::user"]);
    }

    #[test]
    fn expand_brace_with_whitespace() {
        let mut result = expand_pattern("handlers::{ planet , user }");
        result.sort();
        assert_eq!(result, vec!["handlers::planet", "handlers::user"]);
    }

    #[test]
    fn parse_no_args() {
        let args: RouterArgs = syn::parse_str("").unwrap();
        assert!(args.state.is_none());
        assert!(args.pattern.is_none());
    }

    #[test]
    fn parse_pattern_only() {
        let args: RouterArgs = syn::parse_str("\"handlers::planet\"").unwrap();
        assert!(args.state.is_none());
        assert!(matches!(args.pattern, Some(Pattern::Single(_))));
    }

    #[test]
    fn parse_array_pattern() {
        let args: RouterArgs =
            syn::parse_str("[\"handlers::planet\", \"handlers::user\"]").unwrap();
        assert!(matches!(args.pattern, Some(Pattern::Multiple(_))));
    }

    #[test]
    fn parse_duplicate_pattern_errors() {
        let result: syn::Result<RouterArgs> =
            syn::parse_str("\"handlers::planet\", \"handlers::user\"");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("pattern specified twice")
        );
    }
}
