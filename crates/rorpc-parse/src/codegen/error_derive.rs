//! Code generation for `#[derive(OrpcError)]`.
//!
//! Registers error enum variants with rorpc via `inventory::submit!` so
//! `generate_contract()` can emit TypeScript `.errors({...})` entries.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

use crate::{
    errors::Result,
    types::{OPTION, VEC, try_extract_wrapper},
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate the `#[derive(OrpcError)]` expansion.
pub fn expand_orpc_errors(input: DeriveInput) -> Result<TokenStream> {
    let enum_name = &input.ident;
    let enum_name_str = enum_name.to_string();

    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "#[derive(OrpcError)] can only be used on enums",
            )
            .into());
        }
    };

    let mut variant_tokens: Vec<TokenStream> = Vec::new();

    for variant in variants {
        let variant_name_screaming = to_screaming_snake_case(&variant.ident.to_string());

        let data_schema = match &variant.fields {
            Fields::Unit => quote! { None },
            Fields::Unnamed(fields) => {
                let schemas: Vec<String> = fields
                    .unnamed
                    .iter()
                    .map(|f| zod_schema_for_type(&f.ty))
                    .collect();
                let schema = if schemas.len() == 1 {
                    schemas.into_iter().next().unwrap()
                } else {
                    format!("z.tuple([{}])", schemas.join(", "))
                };
                quote! { Some(#schema) }
            }
            Fields::Named(fields) => {
                let field_schemas: Vec<String> = fields
                    .named
                    .iter()
                    .map(|f| {
                        let name = f.ident.as_ref().unwrap().to_string();
                        let schema = zod_schema_for_type(&f.ty);
                        format!("{}: {}", name, schema)
                    })
                    .collect();
                let schema = format!("z.object({{ {} }})", field_schemas.join(", "));
                quote! { Some(#schema) }
            }
        };

        variant_tokens.push(quote! {
            ::rorpc::ErrorVariant {
                name: #variant_name_screaming,
                data_schema: #data_schema,
            }
        });
    }

    Ok(quote! {
        const _: () = {
            ::rorpc::inventory::submit! {
                ::rorpc::ErrorRegistration {
                    type_name: #enum_name_str,
                    variants: &[
                        #(#variant_tokens),*
                    ],
                }
            }
        };
    })
}

// ---------------------------------------------------------------------------
// Type → Zod schema string
//
// Uses AST-based wrapper detection for Option<T> and Vec<T>.
// ---------------------------------------------------------------------------

fn zod_schema_for_type(ty: &syn::Type) -> String {
    // Option<T>
    if let Some(m) = try_extract_wrapper(ty, OPTION) {
        if let Some(inner) = m.first_type() {
            return format!("{}.optional()", zod_schema_for_type(inner));
        }
        return "z.unknown().optional()".to_string();
    }

    // Vec<T>
    if let Some(m) = try_extract_wrapper(ty, VEC) {
        if let Some(inner) = m.first_type() {
            return format!("z.array({})", zod_schema_for_type(inner));
        }
        return "z.array(z.unknown())".to_string();
    }

    // Path types — check final segment ident
    if let syn::Type::Path(type_path) = ty
        && let Some(seg) = type_path.path.segments.last()
    {
        return match seg.ident.to_string().as_str() {
            "String" | "str" => "z.string()".to_string(),
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" => "z.number().int()".to_string(),
            "f32" | "f64" => "z.number()".to_string(),
            "bool" => "z.boolean()".to_string(),
            "Value" => "z.any()".to_string(),
            // Custom types without #[derive(ZodTs)] cannot be introspected here —
            // the macro only sees the field's type name, not its internal fields.
            // Emit z.unknown() so the contract is always valid TypeScript.
            // To get a precise schema, add #[derive(ZodTs)] to the inner type.
            _other => "z.unknown()".to_string(),
        };
    }

    // Unit type ()
    if let syn::Type::Tuple(t) = ty
        && t.elems.is_empty()
    {
        return "z.void()".to_string();
    }

    "z.unknown()".to_string()
}

// ---------------------------------------------------------------------------
// Naming helpers
// ---------------------------------------------------------------------------

/// Convert `PascalCase` to `SCREAMING_SNAKE_CASE`.
fn to_screaming_snake_case(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 && prev_lower {
            out.push('_');
        }
        out.push(ch.to_ascii_uppercase());
        prev_lower = ch.is_ascii_lowercase();
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screaming_snake_case() {
        assert_eq!(to_screaming_snake_case("NotFound"), "NOT_FOUND");
        assert_eq!(to_screaming_snake_case("DatabaseError"), "DATABASE_ERROR");
        assert_eq!(
            to_screaming_snake_case("RateLimitExceeded"),
            "RATE_LIMIT_EXCEEDED"
        );
        assert_eq!(to_screaming_snake_case("OK"), "OK");
        assert_eq!(to_screaming_snake_case("NotFoundError"), "NOT_FOUND_ERROR");
    }

    #[test]
    fn zod_schema_primitives() {
        let string_ty: syn::Type = syn::parse_str("String").unwrap();
        assert_eq!(zod_schema_for_type(&string_ty), "z.string()");

        let i32_ty: syn::Type = syn::parse_str("i32").unwrap();
        assert_eq!(zod_schema_for_type(&i32_ty), "z.number().int()");

        let f64_ty: syn::Type = syn::parse_str("f64").unwrap();
        assert_eq!(zod_schema_for_type(&f64_ty), "z.number()");

        let bool_ty: syn::Type = syn::parse_str("bool").unwrap();
        assert_eq!(zod_schema_for_type(&bool_ty), "z.boolean()");
    }

    #[test]
    fn zod_schema_option() {
        let ty: syn::Type = syn::parse_str("Option<String>").unwrap();
        assert_eq!(zod_schema_for_type(&ty), "z.string().optional()");
    }

    #[test]
    fn zod_schema_qualified_option() {
        // core::option::Option — matches on final segment
        let ty: syn::Type = syn::parse_str("core::option::Option<i32>").unwrap();
        assert_eq!(zod_schema_for_type(&ty), "z.number().int().optional()");
    }

    #[test]
    fn zod_schema_vec() {
        let ty: syn::Type = syn::parse_str("Vec<String>").unwrap();
        assert_eq!(zod_schema_for_type(&ty), "z.array(z.string())");
    }

    #[test]
    fn zod_schema_custom_type() {
        // Custom types without #[derive(ZodTs)] cannot be introspected at macro time —
        // the macro only sees the type name, not its internal fields.
        // Falls back to z.unknown() so generated contracts always compile.
        let ty: syn::Type = syn::parse_str("Planet").unwrap();
        assert_eq!(zod_schema_for_type(&ty), "z.unknown()");
    }

    #[test]
    fn zod_schema_value() {
        let ty: syn::Type = syn::parse_str("serde_json::Value").unwrap();
        assert_eq!(zod_schema_for_type(&ty), "z.any()");
    }

    #[test]
    fn expand_unit_enum() {
        let input: DeriveInput = syn::parse_quote! {
            enum AppError { NotFound, Conflict }
        };
        let ts = expand_orpc_errors(input).unwrap();
        let code = ts.to_string();
        assert!(code.contains("NOT_FOUND"));
        assert!(code.contains("CONFLICT"));
        assert!(code.contains("None"));
    }

    #[test]
    fn expand_rejects_struct() {
        let input: DeriveInput = syn::parse_quote! {
            struct NotAnEnum { field: String }
        };
        assert!(expand_orpc_errors(input).is_err());
    }
}
