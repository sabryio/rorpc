//! Attribute parsing for `#[serde(...)]` and `#[zod(...)]` annotations.
//!
//! Both parsers use `syn`'s `parse_nested_meta` API so they work correctly
//! with the full attribute syntax including `serde(rename(serialize = "...",
//! deserialize = "..."))` and can skip unknown keys without panicking.

use syn::{Attribute, Meta, spanned::Spanned};

use crate::errors::{Error, Result};

// ---------------------------------------------------------------------------
// SerdeAttrs
// ---------------------------------------------------------------------------

/// Parsed `#[serde(...)]` attributes relevant to orpc schema generation.
#[derive(Debug, Default, PartialEq)]
pub struct SerdeAttrs {
    /// `#[serde(rename = "name")]` or `#[serde(rename(deserialize = "name"))]`
    pub rename: Option<String>,
    /// `#[serde(rename_all = "camelCase")]` on containers
    pub rename_all: Option<String>,
    /// `#[serde(skip)]` or `#[serde(skip_serializing)]`
    pub skip: bool,
    /// `#[serde(default)]`
    pub default: bool,
    /// `#[serde(tag = "type")]` on enum containers
    pub tag: Option<String>,
    /// `#[serde(content = "data")]` on enum containers
    pub content: Option<String>,
    /// `#[serde(untagged)]` on enum containers
    pub untagged: bool,
}

/// Parse all `#[serde(...)]` attributes from a slice, merging results.
///
/// Unknown serde keys are silently skipped — orpc only cares about rename/skip.
pub fn parse_serde_attrs(attrs: &[Attribute]) -> Result<SerdeAttrs> {
    let mut out = SerdeAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                out.rename = Some(parse_string_or_nested(&meta, "deserialize")?);
            } else if meta.path.is_ident("rename_all") {
                out.rename_all = Some(parse_lit_str_value(&meta)?);
            } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
                out.skip = true;
                // skip has no value
            } else if meta.path.is_ident("default") {
                out.default = true;
                // default may have an optional path value — consume and ignore
                if meta.input.peek(syn::Token![=]) {
                    let _: syn::Expr = meta.value()?.parse()?;
                }
            } else if meta.path.is_ident("tag") {
                out.tag = Some(parse_lit_str_value(&meta)?);
            } else if meta.path.is_ident("content") {
                out.content = Some(parse_lit_str_value(&meta)?);
            } else if meta.path.is_ident("untagged") {
                out.untagged = true;
            } else {
                // Unknown serde key — consume any value so the parser doesn't stall
                skip_meta_value(&meta)?;
            }
            Ok(())
        })
        .map_err(Error::from)?;
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// ZodAttrs
// ---------------------------------------------------------------------------

/// Parsed `#[zod(...)]` attributes for a single struct field.
#[derive(Debug, Default, PartialEq)]
pub struct ZodAttrs {
    // Numbers
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub int: bool,
    pub positive: bool,
    pub negative: bool,
    pub nonnegative: bool,
    pub nonpositive: bool,
    pub finite: bool,
    // Strings / arrays
    pub length: Option<usize>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub starts_with: Option<String>,
    pub ends_with: Option<String>,
    pub includes: Option<String>,
    pub email: bool,
    pub url: bool,
    pub regex: Option<String>,
}

/// Parse all `#[zod(...)]` attributes from a slice, merging results.
///
/// Returns an error for unrecognised keys so users get an actionable message
/// rather than silently ignored constraints.
pub fn parse_zod_attrs(attrs: &[Attribute]) -> Result<ZodAttrs> {
    let mut out = ZodAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("zod") {
            continue;
        }

        let Meta::List(ref list) = attr.meta else {
            continue;
        };

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("min") {
                out.min = Some(parse_f64_value(&meta)?);
            } else if meta.path.is_ident("max") {
                out.max = Some(parse_f64_value(&meta)?);
            } else if meta.path.is_ident("length") {
                out.length = Some(parse_usize_value(&meta)?);
            } else if meta.path.is_ident("min_length") {
                out.min_length = Some(parse_usize_value(&meta)?);
            } else if meta.path.is_ident("max_length") {
                out.max_length = Some(parse_usize_value(&meta)?);
            } else if meta.path.is_ident("starts_with") {
                out.starts_with = Some(parse_lit_str_value(&meta)?);
            } else if meta.path.is_ident("ends_with") {
                out.ends_with = Some(parse_lit_str_value(&meta)?);
            } else if meta.path.is_ident("includes") {
                out.includes = Some(parse_lit_str_value(&meta)?);
            } else if meta.path.is_ident("regex") {
                out.regex = Some(parse_lit_str_value(&meta)?);
            } else if meta.path.is_ident("email") {
                out.email = true;
            } else if meta.path.is_ident("url") {
                out.url = true;
            } else if meta.path.is_ident("int") {
                out.int = true;
            } else if meta.path.is_ident("positive") {
                out.positive = true;
            } else if meta.path.is_ident("negative") {
                out.negative = true;
            } else if meta.path.is_ident("nonnegative") {
                out.nonnegative = true;
            } else if meta.path.is_ident("nonpositive") {
                out.nonpositive = true;
            } else if meta.path.is_ident("finite") {
                out.finite = true;
            } else {
                let key = meta
                    .path
                    .get_ident()
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                return Err(syn::Error::new(
                    meta.path.span(),
                    Error::unknown_key(
                        meta.path.span(),
                        &key,
                        &[
                            "min",
                            "max",
                            "length",
                            "min_length",
                            "max_length",
                            "starts_with",
                            "ends_with",
                            "includes",
                            "regex",
                            "email",
                            "url",
                            "int",
                            "positive",
                            "negative",
                            "nonnegative",
                            "nonpositive",
                            "finite",
                        ],
                    )
                    .to_string(),
                ));
            }
            let _ = list; // suppress unused warning — list is accessed above for Meta::List check
            Ok(())
        })
        .map_err(Error::from)?;
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// serde rename_all rule application
// ---------------------------------------------------------------------------

/// Apply a `#[serde(rename_all = "...")]` rule to a variant or field name.
pub fn apply_rename_rule(rule: &str, name: &str) -> String {
    match rule {
        "lowercase" => name.to_ascii_lowercase(),
        "UPPERCASE" => name.to_ascii_uppercase(),
        "camelCase" => {
            let mut chars = name.chars();
            match chars.next() {
                Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        }
        "snake_case" => to_snake_case(name),
        "SCREAMING_SNAKE_CASE" => to_snake_case(name).to_ascii_uppercase(),
        "kebab-case" => to_snake_case(name).replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => to_snake_case(name).to_ascii_uppercase().replace('_', "-"),
        // Unknown rules pass through unchanged — serde would reject them at compile time
        _ => name.to_string(),
    }
}

fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.char_indices() {
        if i > 0 && ch.is_uppercase() {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

// ---------------------------------------------------------------------------
// Internal parsing helpers
// ---------------------------------------------------------------------------

/// Parse `= "string"` or `("string")` value from a meta item.
fn parse_lit_str_value(meta: &syn::meta::ParseNestedMeta) -> syn::Result<String> {
    if meta.input.peek(syn::Token![=]) {
        let lit: syn::LitStr = meta.value()?.parse()?;
        return Ok(lit.value());
    }
    // Parenthesised form: key("value")
    if meta.input.peek(syn::token::Paren) {
        let mut result = None;
        meta.parse_nested_meta(|inner| {
            // The inner content is a bare string literal, not a key=value pair.
            // We parse it by reading the literal directly from the token stream.
            let lit: syn::LitStr = inner.input.parse()?;
            result = Some(lit.value());
            Ok(())
        })?;
        if let Some(v) = result {
            return Ok(v);
        }
    }
    Err(syn::Error::new(
        meta.input.span(),
        "expected `= \"value\"` or `(\"value\")`",
    ))
}

/// Parse `= 3.14` / `= 42` or `(3.14)` / `(42)` as f64.
fn parse_f64_value(meta: &syn::meta::ParseNestedMeta) -> syn::Result<f64> {
    let expr: syn::Expr = if meta.input.peek(syn::Token![=]) {
        meta.value()?.parse()?
    } else if meta.input.peek(syn::token::Paren) {
        // Parenthesised form: key(42) or key(3.14)
        let content;
        syn::parenthesized!(content in meta.input);
        content.parse()?
    } else {
        return Err(syn::Error::new(
            meta.input.span(),
            "expected `= <number>` or `(<number>)`",
        ));
    };

    match &expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Float(f),
            ..
        }) => f
            .base10_parse::<f64>()
            .map_err(|e| syn::Error::new(f.span(), e)),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(i),
            ..
        }) => i
            .base10_parse::<f64>()
            .map_err(|e| syn::Error::new(i.span(), e)),
        _ => Err(syn::Error::new(expr.span(), "expected a numeric literal")),
    }
}

/// Parse `= 42` or `(42)` integer value as usize.
fn parse_usize_value(meta: &syn::meta::ParseNestedMeta) -> syn::Result<usize> {
    let lit: syn::LitInt = if meta.input.peek(syn::Token![=]) {
        meta.value()?.parse()?
    } else if meta.input.peek(syn::token::Paren) {
        // Parenthesised form: key(42)
        let content;
        syn::parenthesized!(content in meta.input);
        content.parse()?
    } else {
        return Err(syn::Error::new(
            meta.input.span(),
            "expected `= <integer>` or `(<integer>)`",
        ));
    };
    lit.base10_parse::<usize>()
        .map_err(|e| syn::Error::new(lit.span(), e))
}

/// Parse either `= "value"` or `(serialize = "...", deserialize = "...")`,
/// preferring the `prefer_key` variant when both are present.
fn parse_string_or_nested(
    meta: &syn::meta::ParseNestedMeta,
    prefer_key: &str,
) -> syn::Result<String> {
    if meta.input.peek(syn::Token![=]) {
        return parse_lit_str_value(meta);
    }
    // Nested form: rename(serialize = "a", deserialize = "b")
    let mut serialize = None;
    let mut deserialize = None;
    meta.parse_nested_meta(|inner| {
        let lit: syn::LitStr = inner.value()?.parse()?;
        if inner.path.is_ident("serialize") {
            serialize = Some(lit.value());
        } else if inner.path.is_ident("deserialize") {
            deserialize = Some(lit.value());
        }
        Ok(())
    })?;
    // Prefer the requested key (deserialize drives the wire format we care about)
    if prefer_key == "deserialize" {
        Ok(deserialize.or(serialize).unwrap_or_default())
    } else {
        Ok(serialize.or(deserialize).unwrap_or_default())
    }
}

/// Consume any value attached to a meta key without recording it.
fn skip_meta_value(meta: &syn::meta::ParseNestedMeta) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _: syn::Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|inner| skip_meta_value(&inner))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use syn::{DeriveInput, parse_quote};

    fn attrs_of(input: DeriveInput) -> Vec<Attribute> {
        input.attrs
    }

    fn field_attrs(input: &syn::ItemStruct, field_name: &str) -> Vec<Attribute> {
        if let syn::Fields::Named(fields) = &input.fields {
            for f in &fields.named {
                if f.ident.as_ref().map(|i| i == field_name).unwrap_or(false) {
                    return f.attrs.clone();
                }
            }
        }
        vec![]
    }

    // --- SerdeAttrs ---

    #[test]
    fn serde_rename_simple() {
        let input: DeriveInput = parse_quote! {
            #[serde(rename = "planet_name")]
            struct S;
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert_eq!(attrs.rename, Some("planet_name".to_string()));
    }

    #[test]
    fn serde_rename_all() {
        let input: DeriveInput = parse_quote! {
            #[serde(rename_all = "camelCase")]
            struct S;
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert_eq!(attrs.rename_all, Some("camelCase".to_string()));
    }

    #[test]
    fn serde_skip() {
        let input: DeriveInput = parse_quote! {
            #[serde(skip)]
            struct S;
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert!(attrs.skip);
    }

    #[test]
    fn serde_default() {
        let input: DeriveInput = parse_quote! {
            #[serde(default)]
            struct S;
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert!(attrs.default);
    }

    #[test]
    fn serde_unknown_key_ignored() {
        // Unknown keys are silently skipped
        let input: DeriveInput = parse_quote! {
            #[serde(some_future_key = "value")]
            struct S;
        };
        assert!(parse_serde_attrs(&attrs_of(input)).is_ok());
    }

    #[test]
    fn serde_tag_only() {
        let input: DeriveInput = parse_quote! {
            #[serde(tag = "type")]
            enum E {}
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert_eq!(attrs.tag, Some("type".to_string()));
        assert_eq!(attrs.content, None);
        assert!(!attrs.untagged);
    }

    #[test]
    fn serde_tag_and_content() {
        let input: DeriveInput = parse_quote! {
            #[serde(tag = "type", content = "data")]
            enum E {}
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert_eq!(attrs.tag, Some("type".to_string()));
        assert_eq!(attrs.content, Some("data".to_string()));
        assert!(!attrs.untagged);
    }

    #[test]
    fn serde_untagged() {
        let input: DeriveInput = parse_quote! {
            #[serde(untagged)]
            enum E {}
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert!(attrs.untagged);
        assert_eq!(attrs.tag, None);
        assert_eq!(attrs.content, None);
    }

    #[test]
    fn non_serde_attr_ignored() {
        let input: DeriveInput = parse_quote! {
            #[derive(Debug)]
            struct S;
        };
        let attrs = parse_serde_attrs(&attrs_of(input)).unwrap();
        assert_eq!(attrs, SerdeAttrs::default());
    }

    // --- ZodAttrs ---

    #[test]
    fn zod_string_constraints() {
        let s: syn::ItemStruct = parse_quote! {
            struct S {
                #[zod(min_length(3), max_length(100), email)]
                name: String,
            }
        };
        let attrs = parse_zod_attrs(&field_attrs(&s, "name")).unwrap();
        assert_eq!(attrs.min_length, Some(3));
        assert_eq!(attrs.max_length, Some(100));
        assert!(attrs.email);
    }

    #[test]
    fn zod_number_constraints() {
        let s: syn::ItemStruct = parse_quote! {
            struct S {
                #[zod(min(0), max(100), int, positive)]
                score: f64,
            }
        };
        let attrs = parse_zod_attrs(&field_attrs(&s, "score")).unwrap();
        assert_eq!(attrs.min, Some(0.0));
        assert_eq!(attrs.max, Some(100.0));
        assert!(attrs.int);
        assert!(attrs.positive);
    }

    #[test]
    fn zod_unknown_key_returns_error() {
        let s: syn::ItemStruct = parse_quote! {
            struct S {
                #[zod(unknown_key)]
                name: String,
            }
        };
        let err = parse_zod_attrs(&field_attrs(&s, "name")).unwrap_err();
        assert!(err.to_string().contains("unknown key"));
    }

    // --- apply_rename_rule ---

    #[test]
    fn rename_rules() {
        assert_eq!(apply_rename_rule("camelCase", "PlanetName"), "planetName");
        assert_eq!(apply_rename_rule("snake_case", "PlanetName"), "planet_name");
        assert_eq!(
            apply_rename_rule("SCREAMING_SNAKE_CASE", "PlanetName"),
            "PLANET_NAME"
        );
        assert_eq!(apply_rename_rule("kebab-case", "PlanetName"), "planet-name");
        assert_eq!(apply_rename_rule("lowercase", "PlanetName"), "planetname");
        assert_eq!(apply_rename_rule("UPPERCASE", "PlanetName"), "PLANETNAME");
    }

    #[test]
    fn unknown_rule_passthrough() {
        assert_eq!(
            apply_rename_rule("PascalCase", "planet_name"),
            "planet_name"
        );
    }
}
