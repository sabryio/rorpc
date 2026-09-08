//! AST-based type analysis utilities.
//!
//! All wrapper extraction operates on `syn::Type` AST nodes by checking the
//! **final path segment ident** — never on a string representation of the type.
//! This means `Json<T>`, `axum::Json<T>`, and `axum::extract::Json<T>` are
//! all matched identically.

use syn::{GenericArgument, PathArguments, Token, Type, TypePath, punctuated::Punctuated};

use crate::errors::{Error, Result};

// ---------------------------------------------------------------------------
// Well-known wrapper names
// ---------------------------------------------------------------------------

pub const JSON: &str = "Json";
pub const QUERY: &str = "Query";
pub const PATH: &str = "Path";
pub const RESULT: &str = "Result";
pub const OPTION: &str = "Option";
pub const VEC: &str = "Vec";
pub const STATE: &str = "State";
pub const SSE: &str = "Sse";

// ---------------------------------------------------------------------------
// WrapperMatch — result of a successful wrapper extraction
// ---------------------------------------------------------------------------

/// The generic argument list of a matched wrapper type.
///
/// Returned by [`try_extract_wrapper`] and [`extract_wrapper`].
// WrapperMatch holds references into the syn AST — Debug is derived for test ergonomics.
pub struct WrapperMatch<'a> {
    pub generic_args: &'a Punctuated<GenericArgument, Token![,]>,
}

impl std::fmt::Debug for WrapperMatch<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WrapperMatch")
            .field("generic_args_len", &self.generic_args.len())
            .finish()
    }
}

impl<'a> WrapperMatch<'a> {
    /// The first type argument — `T` in `Wrapper<T, ...>`.
    pub fn first_type(&self) -> Option<&'a Type> {
        self.generic_args.iter().find_map(as_type_arg)
    }

    /// The second type argument — `E` in `Result<T, E>`.
    pub fn second_type(&self) -> Option<&'a Type> {
        self.generic_args.iter().filter_map(as_type_arg).nth(1)
    }

    /// The nth type argument (0-indexed).
    pub fn nth_type(&self, n: usize) -> Option<&'a Type> {
        self.generic_args.iter().filter_map(as_type_arg).nth(n)
    }
}

fn as_type_arg(arg: &GenericArgument) -> Option<&Type> {
    if let GenericArgument::Type(ty) = arg {
        Some(ty)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Extraction — try (Option) and strict (Result)
// ---------------------------------------------------------------------------

/// Extract the generic arguments of `wrapper_name<...>` from `ty`.
///
/// Checks the **final path segment ident** so qualified paths like
/// `std::result::Result` and `axum::extract::Json` are handled identically
/// to bare `Result` and `Json`.
///
/// Returns `None` when `ty` is not the named wrapper — use this when a
/// wrapper is optional (e.g. a parameter may or may not be `Json<T>`).
pub fn try_extract_wrapper<'a>(ty: &'a Type, wrapper_name: &str) -> Option<WrapperMatch<'a>> {
    let last = last_segment(ty)?;
    if last.ident != wrapper_name {
        return None;
    }
    match &last.arguments {
        PathArguments::AngleBracketed(args) if !args.args.is_empty() => Some(WrapperMatch {
            generic_args: &args.args,
        }),
        _ => None,
    }
}

/// Extract the generic arguments of `wrapper_name<...>` from `ty`, or return
/// a structured error if the wrapper is not present or has no type arguments.
///
/// Use this when the wrapper is required (e.g. a handler return type must be
/// `Json<T>` or `Result<Json<T>, E>`).
pub fn extract_wrapper<'a>(ty: &'a Type, wrapper_name: &'static str) -> Result<WrapperMatch<'a>> {
    let span = span_of(ty);

    let last = last_segment(ty)
        .ok_or_else(|| Error::unsupported_type(span, ty, "empty type path", "use a named type"))?;

    if last.ident != wrapper_name {
        return Err(Error::missing_wrapper(span, wrapper_name, ty));
    }

    match &last.arguments {
        PathArguments::AngleBracketed(args) if !args.args.is_empty() => Ok(WrapperMatch {
            generic_args: &args.args,
        }),
        PathArguments::AngleBracketed(_) | PathArguments::None => {
            Err(Error::empty_generic_args(span, wrapper_name))
        }
        PathArguments::Parenthesized(_) => Err(Error::unsupported_type(
            span,
            ty,
            "parenthesised arguments are not supported",
            "use angle-bracketed generics: `Wrapper<T>`",
        )),
    }
}

// ---------------------------------------------------------------------------
// Primitive check
// ---------------------------------------------------------------------------

/// Returns `true` for Rust primitive types and the unit type `()`.
///
/// Uses AST ident comparison — never string matching.
pub fn is_primitive(ty: &Type) -> bool {
    match ty {
        // Unit type ()
        Type::Tuple(t) if t.elems.is_empty() => true,
        Type::Path(_) => matches!(
            last_ident_str(ty).as_deref(),
            Some(
                "String"
                    | "str"
                    | "bool"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "f32"
                    | "f64"
                    // Well-known non-schema types that map to inline Zod expressions
                    | "Uuid"
                    | "DateTime"
                    | "Value" // serde_json::Value
            )
        ),
        _ => false,
    }
}

/// Check if a type name string represents a Rust primitive or standard type.
///
/// This is for runtime use when you have a type name from metadata as a string,
/// not a `syn::Type` AST. For compile-time AST checking, use [`is_primitive`] instead.
///
/// # Examples
///
/// ```
/// use rorpc_parse::types::is_primitive_type_name;
///
/// assert!(is_primitive_type_name("String"));
/// assert!(is_primitive_type_name("i32"));
/// assert!(is_primitive_type_name("()"));
/// assert!(!is_primitive_type_name("Planet"));
/// ```
pub fn is_primitive_type_name(type_name: &str) -> bool {
    matches!(
        type_name,
        "()" | "String"
            | "str"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "f32"
            | "f64"
            | "bool"
            | "usize"
            | "isize"
            | "serde_json::Value"
            | "Json<serde_json::Value>"
    )
}

// ---------------------------------------------------------------------------
// Innermost custom type
// ---------------------------------------------------------------------------

/// Recursively unwrap well-known wrappers and return the innermost type.
///
/// `Result<Json<Vec<Planet>>, E>` → `Planet` (a `Type::Path` for `Planet`)
///
/// Returns `None` when the innermost resolved type is a primitive (no schema
/// registration needed) or when the type cannot be unwrapped further.
pub fn innermost_custom_type(ty: &Type) -> Option<&Type> {
    for wrapper in &[RESULT, JSON, QUERY, OPTION, VEC, STATE, SSE] {
        if let Some(m) = try_extract_wrapper(ty, wrapper)
            && let Some(inner) = m.first_type()
        {
            return innermost_custom_type(inner);
        }
    }
    // Base case: no wrapper matched — this is the innermost type
    if is_primitive(ty) { None } else { Some(ty) }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn last_segment(ty: &Type) -> Option<&syn::PathSegment> {
    if let Type::Path(TypePath { path, .. }) = ty {
        path.segments.last()
    } else {
        None
    }
}

fn last_ident_str(ty: &Type) -> Option<String> {
    last_segment(ty).map(|seg| seg.ident.to_string())
}

fn span_of(ty: &Type) -> proc_macro2::Span {
    use syn::spanned::Spanned;
    ty.span()
}

/// Extract the first generic argument from a type string.
///
/// String-based parsing for runtime use. Returns the first type argument
/// from generic type syntax, handling nested generics correctly.
///
/// # Examples
///
/// ```
/// use rorpc_parse::types::extract_first_generic_arg_string;
///
/// assert_eq!(extract_first_generic_arg_string("Result<T, E>"), Some("T".to_string()));
/// assert_eq!(extract_first_generic_arg_string("Vec<Planet>"), Some("Planet".to_string()));
/// assert_eq!(extract_first_generic_arg_string("Result<Json<Planet>, E>"), Some("Json<Planet>".to_string()));
/// assert_eq!(extract_first_generic_arg_string("NoGenerics"), None);
/// ```
pub fn extract_first_generic_arg_string(type_str: &str) -> Option<String> {
    let start = type_str.find('<')? + 1;
    let mut depth = 0;
    let mut end = start;

    for (i, ch) in type_str[start..].char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if depth == 0 => {
                end = start + i;
                break;
            }
            '>' => depth -= 1,
            ',' if depth == 0 => {
                end = start + i;
                break;
            }
            _ => {}
        }
    }

    if end > start {
        Some(type_str[start..end].trim().to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::type_display;

    fn parse_type(s: &str) -> Type {
        syn::parse_str(s).unwrap()
    }

    // --- try_extract_wrapper ---

    #[test]
    fn bare_json() {
        let ty = parse_type("Json<Planet>");
        let m = try_extract_wrapper(&ty, JSON).unwrap();
        assert!(m.first_type().is_some());
    }

    #[test]
    fn qualified_json() {
        let ty = parse_type("axum::extract::Json<Planet>");
        let m = try_extract_wrapper(&ty, JSON).unwrap();
        assert!(m.first_type().is_some());
    }

    #[test]
    fn qualified_result() {
        let ty = parse_type("std::result::Result<Json<Planet>, AppError>");
        let m = try_extract_wrapper(&ty, RESULT).unwrap();
        assert!(m.first_type().is_some());
        assert!(m.second_type().is_some());
    }

    #[test]
    fn wrong_wrapper_returns_none() {
        let ty = parse_type("Option<String>");
        assert!(try_extract_wrapper(&ty, JSON).is_none());
    }

    #[test]
    fn wrapper_without_args_returns_none() {
        // `Json` with no generics — syn parses this as a bare path, not angle-bracketed
        let ty = parse_type("Json");
        assert!(try_extract_wrapper(&ty, JSON).is_none());
    }

    // --- extract_wrapper (strict) ---

    #[test]
    fn extract_wrong_wrapper_gives_error() {
        let ty = parse_type("Vec<Planet>");
        let err = extract_wrapper(&ty, JSON).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("expected `Json` wrapper"));
        assert!(msg.contains("Vec<Planet>"));
    }

    // --- is_primitive ---

    #[test]
    fn primitives_identified() {
        for s in &["String", "i32", "u64", "f64", "bool", "usize"] {
            assert!(is_primitive(&parse_type(s)), "{} should be primitive", s);
        }
    }

    #[test]
    fn unit_type_is_primitive() {
        let ty: Type = syn::parse_str("()").unwrap();
        assert!(is_primitive(&ty));
    }

    #[test]
    fn custom_type_not_primitive() {
        assert!(!is_primitive(&parse_type("Planet")));
        assert!(!is_primitive(&parse_type("AppError")));
    }

    // --- innermost_custom_type ---

    #[test]
    fn unwraps_result_json_vec() {
        let ty = parse_type("Result<Json<Vec<Planet>>, AppError>");
        let inner = innermost_custom_type(&ty).unwrap();
        assert_eq!(type_display(inner), "Planet");
    }

    #[test]
    fn primitive_innermost_returns_none() {
        let ty = parse_type("Json<String>");
        assert!(innermost_custom_type(&ty).is_none());
    }

    #[test]
    fn custom_type_at_root_returned_as_is() {
        let ty = parse_type("Planet");
        let inner = innermost_custom_type(&ty).unwrap();
        assert_eq!(type_display(inner), "Planet");
    }

    // --- WrapperMatch ---

    #[test]
    fn second_type_on_result() {
        let ty = parse_type("Result<Json<Planet>, AppError>");
        let m = try_extract_wrapper(&ty, RESULT).unwrap();
        let second = m.second_type().unwrap();
        assert_eq!(type_display(second), "AppError");
    }

    #[test]
    fn nth_type_indexing() {
        let ty = parse_type("Result<Json<Planet>, AppError>");
        let m = try_extract_wrapper(&ty, RESULT).unwrap();
        assert!(m.nth_type(0).is_some());
        assert!(m.nth_type(1).is_some());
        assert!(m.nth_type(2).is_none());
    }
}
