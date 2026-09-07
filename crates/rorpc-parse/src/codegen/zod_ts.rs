//! Code generation for `#[derive(ZodTs)]`.
//!
//! Generates a `fn zod_ts() -> String` method that returns a complete
//! TypeScript block with a Zod schema and a `z.infer` type alias,
//! plus an `inventory::submit!` for `SchemaRegistration`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

use crate::{
    attributes::{ZodAttrs, apply_rename_rule, parse_serde_attrs, parse_zod_attrs},
    errors::Result,
    types::{OPTION, VEC, is_primitive, try_extract_wrapper},
};

const ZOD_IMPORT: &str = "import * as z from \"zod\";";

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate the `#[derive(ZodTs)]` expansion.
pub fn derive_zod_ts(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let name_str = name.to_string();

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => expand_named_struct(name, &name_str, fields, &input),
            Fields::Unnamed(_) | Fields::Unit => Err(syn::Error::new_spanned(
                name,
                "ZodTs: only structs with named fields are supported",
            )
            .into()),
        },
        Data::Enum(data) => expand_enum(name, &name_str, data, &input),
        Data::Union(_) => {
            Err(syn::Error::new_spanned(name, "ZodTs cannot be derived for unions").into())
        }
    }
}

// ---------------------------------------------------------------------------
// Struct expansion
// ---------------------------------------------------------------------------

fn expand_named_struct(
    name: &syn::Ident,
    name_str: &str,
    fields: &syn::FieldsNamed,
    _input: &DeriveInput,
) -> Result<TokenStream> {
    let mut field_lines: Vec<String> = Vec::new();
    let mut dep_type_names: Vec<String> = Vec::new();

    for field in &fields.named {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let serde = parse_serde_attrs(&field.attrs)?;

        if serde.skip {
            continue;
        }

        let ts_key = serde.rename.as_deref().unwrap_or(&field_name);
        let zod = parse_zod_attrs(&field.attrs)?;
        let is_opt = is_option_type(&field.ty);

        let base_ty = if is_opt {
            option_inner(&field.ty).unwrap_or(&field.ty)
        } else {
            &field.ty
        };

        let zod_expr = rust_type_to_zod(base_ty, &zod);

        let final_expr = if is_opt {
            format!("{}.optional()", zod_expr)
        } else {
            zod_expr
        };

        field_lines.push(format!("  {}: {}", ts_key, final_expr));

        // Collect non-primitive custom types for dependency tracking
        if let Some(custom) = innermost_custom_name(base_ty) {
            dep_type_names.push(custom);
        }
    }

    let schema_name = format!("{}Schema", name_str);
    let ts_code = format!(
        "{}\n\nexport const {} = z.object({{\n{}\n}});\n\nexport type {} = z.infer<typeof {}>;",
        ZOD_IMPORT,
        schema_name,
        field_lines.join(",\n"),
        name_str,
        schema_name,
    );

    Ok(emit_registration(name, name_str, &ts_code, &dep_type_names))
}

// ---------------------------------------------------------------------------
// Enum expansion
// ---------------------------------------------------------------------------

fn expand_enum(
    name: &syn::Ident,
    name_str: &str,
    data: &syn::DataEnum,
    input: &DeriveInput,
) -> Result<TokenStream> {
    let serde_container = parse_serde_attrs(&input.attrs)?;
    let rename_all = serde_container.rename_all.as_deref();

    let mut variant_schemas: Vec<String> = Vec::new();

    for variant in &data.variants {
        let serde_variant = parse_serde_attrs(&variant.attrs)?;
        if serde_variant.skip {
            continue;
        }

        let raw_name = variant.ident.to_string();
        let variant_name = serde_variant
            .rename
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| {
                rename_all
                    .map(|rule| apply_rename_rule(rule, &raw_name))
                    .unwrap_or(raw_name)
            });

        variant_schemas.push(generate_variant_ts(&variant_name, &variant.fields)?);
    }

    let schema_name = format!("{}Schema", name_str);
    let variants_str = variant_schemas.join(",\n  ");
    let ts_code = format!(
        "{}\n\nexport const {} = z.union([\n  {}\n]);\n\nexport type {} = z.infer<typeof {}>;",
        ZOD_IMPORT, schema_name, variants_str, name_str, schema_name,
    );

    Ok(emit_registration(name, name_str, &ts_code, &[]))
}

// ---------------------------------------------------------------------------
// Variant code generation
// ---------------------------------------------------------------------------

fn generate_variant_ts(variant_name: &str, fields: &Fields) -> Result<String> {
    match fields {
        Fields::Unit => Ok(format!("z.literal(\"{}\")", escape_str(variant_name))),

        Fields::Unnamed(fields_unnamed) => {
            let count = fields_unnamed.unnamed.len();
            if count == 1 {
                let field = fields_unnamed.unnamed.first().unwrap();
                let zod = parse_zod_attrs(&field.attrs)?;
                let schema = rust_type_to_zod(&field.ty, &zod);
                Ok(format!(
                    "z.object({{ {}: {} }})",
                    ts_object_key(variant_name),
                    schema
                ))
            } else {
                let elements: Vec<String> = fields_unnamed
                    .unnamed
                    .iter()
                    .map(|f| {
                        let zod = parse_zod_attrs(&f.attrs)?;
                        Ok(rust_type_to_zod(&f.ty, &zod))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(format!(
                    "z.object({{ {}: z.tuple([{}]) }})",
                    ts_object_key(variant_name),
                    elements.join(", ")
                ))
            }
        }

        Fields::Named(fields_named) => {
            let field_schemas: Vec<String> = fields_named
                .named
                .iter()
                .map(|field| {
                    let field_name = field.ident.as_ref().unwrap().to_string();
                    let serde = parse_serde_attrs(&field.attrs)?;
                    if serde.skip {
                        return Ok(String::new());
                    }
                    let ts_key = serde.rename.as_deref().unwrap_or(&field_name);
                    let zod_attrs = parse_zod_attrs(&field.attrs)?;
                    let is_opt = is_option_type(&field.ty);
                    let base_ty = if is_opt {
                        option_inner(&field.ty).unwrap_or(&field.ty)
                    } else {
                        &field.ty
                    };
                    let schema = rust_type_to_zod(base_ty, &zod_attrs);
                    let final_schema = if is_opt {
                        format!("{}.optional()", schema)
                    } else {
                        schema
                    };
                    Ok(format!("{}: {}", ts_key, final_schema))
                })
                .filter(|r| r.as_deref().map(|s| !s.is_empty()).unwrap_or(true))
                .collect::<Result<Vec<_>>>()?;

            Ok(format!(
                "z.object({{ {}: z.object({{ {} }}) }})",
                ts_object_key(variant_name),
                field_schemas.join(", ")
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// inventory::submit! emission
// ---------------------------------------------------------------------------

fn emit_registration(
    name: &syn::Ident,
    name_str: &str,
    ts_code: &str,
    dep_type_names: &[String],
) -> TokenStream {
    let dep_strs: Vec<&str> = dep_type_names.iter().map(String::as_str).collect();

    quote! {
        impl #name {
            pub fn zod_ts() -> String {
                #ts_code.to_string()
            }

            pub fn dependent_types() -> Vec<&'static str> {
                vec![#(#dep_strs),*]
            }
        }

        const _: () = {
            ::rorpc::inventory::submit! {
                ::rorpc::SchemaRegistration {
                    type_name: #name_str,
                    zod_ts: #name::zod_ts,
                    dependent_types: #name::dependent_types,
                }
            }
        };
    }
}

// ---------------------------------------------------------------------------
// Type → Zod expression
// ---------------------------------------------------------------------------

/// Map a `syn::Type` to a Zod schema expression string.
///
/// Uses AST-based wrapper detection for `Option<T>` and `Vec<T>` —
/// never string prefix matching.
pub fn rust_type_to_zod(ty: &syn::Type, attrs: &ZodAttrs) -> String {
    // Option<T> — recurse on inner, then .optional()
    if is_option_type(ty)
        && let Some(inner) = option_inner(ty)
    {
        let inner_schema = rust_type_to_zod(inner, &ZodAttrs::default());
        return format!("{}.optional()", inner_schema);
    }

    // Vec<T>
    if let Some(m) = try_extract_wrapper(ty, VEC)
        && let Some(inner) = m.first_type()
    {
        let inner_schema = rust_type_to_zod(inner, &ZodAttrs::default());
        let mut chain = format!("z.array({})", inner_schema);
        if let Some(n) = attrs.length {
            chain.push_str(&format!(".length({})", n));
        }
        if let Some(n) = attrs.min_length {
            chain.push_str(&format!(".min({})", n));
        }
        if let Some(n) = attrs.max_length {
            chain.push_str(&format!(".max({})", n));
        }
        return chain;
    }

    // Primitives — match on the final path segment ident
    if let syn::Type::Path(type_path) = ty
        && let Some(seg) = type_path.path.segments.last()
    {
        let name = seg.ident.to_string();
        return match name.as_str() {
            "String" | "str" => build_string_schema(attrs),
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" => build_integer_schema(attrs),
            "f32" | "f64" => build_float_schema(attrs),
            "bool" => "z.boolean()".to_string(),
            // uuid::Uuid → z.uuid()
            "Uuid" => "z.uuid()".to_string(),
            // chrono::DateTime<Utc> → z.iso.datetime()
            "DateTime" => "z.iso.datetime({ offset: true })".to_string(),
            // serde_json::Value → z.any()
            "Value" => "z.any()".to_string(),
            // Custom type — reference its schema by name
            other => format!("{}Schema", other),
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
// Schema builders
// ---------------------------------------------------------------------------

fn build_string_schema(attrs: &ZodAttrs) -> String {
    let mut chain = String::from("z.string()");
    if let Some(n) = attrs.length {
        chain.push_str(&format!(".length({})", n));
    }
    if let Some(n) = attrs.min_length {
        chain.push_str(&format!(".min({})", n));
    }
    if let Some(n) = attrs.max_length {
        chain.push_str(&format!(".max({})", n));
    }
    if attrs.email {
        chain.push_str(".email()");
    }
    if attrs.url {
        chain.push_str(".url()");
    }
    if let Some(ref p) = attrs.regex {
        chain.push_str(&format!(".regex(/{}/)", p));
    }
    if let Some(ref p) = attrs.starts_with {
        chain.push_str(&format!(".startsWith(\"{}\")", p));
    }
    if let Some(ref p) = attrs.ends_with {
        chain.push_str(&format!(".endsWith(\"{}\")", p));
    }
    if let Some(ref p) = attrs.includes {
        chain.push_str(&format!(".includes(\"{}\")", p));
    }
    chain
}

fn build_integer_schema(attrs: &ZodAttrs) -> String {
    let mut chain = String::from("z.number().int()");
    append_number_validators(&mut chain, attrs);
    chain
}

fn build_float_schema(attrs: &ZodAttrs) -> String {
    let mut chain = String::from("z.number()");
    if attrs.int {
        chain.push_str(".int()");
    }
    append_number_validators(&mut chain, attrs);
    chain
}

fn append_number_validators(chain: &mut String, attrs: &ZodAttrs) {
    if let Some(n) = attrs.min {
        chain.push_str(&format!(".min({})", n));
    }
    if let Some(n) = attrs.max {
        chain.push_str(&format!(".max({})", n));
    }
    if attrs.positive {
        chain.push_str(".positive()");
    }
    if attrs.negative {
        chain.push_str(".negative()");
    }
    if attrs.nonnegative {
        chain.push_str(".nonnegative()");
    }
    if attrs.nonpositive {
        chain.push_str(".nonpositive()");
    }
    if attrs.finite {
        chain.push_str(".finite()");
    }
}

// ---------------------------------------------------------------------------
// Type helpers — all AST-based, no string matching on type names
// ---------------------------------------------------------------------------

fn is_option_type(ty: &syn::Type) -> bool {
    try_extract_wrapper(ty, OPTION).is_some()
}

fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    try_extract_wrapper(ty, OPTION)?.first_type()
}

/// Return the simple name of the innermost non-primitive, non-wrapper type,
/// for dependency tracking in `dependent_types()`.
fn innermost_custom_name(ty: &syn::Type) -> Option<String> {
    // Strip Vec<T>
    if let Some(m) = try_extract_wrapper(ty, VEC) {
        return m.first_type().and_then(innermost_custom_name);
    }
    if is_primitive(ty) {
        return None;
    }
    if let syn::Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
    {
        let name = seg.ident.to_string();
        // Exclude Value (serde_json) from dependency tracking
        if name == "Value" {
            return None;
        }
        return Some(name);
    }
    None
}

fn ts_object_key(name: &str) -> String {
    let valid = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if valid {
        name.to_string()
    } else {
        format!("\"{}\"", escape_str(name))
    }
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Runtime type-to-zod conversion (for contract generation)
// ---------------------------------------------------------------------------

/// Convert a Rust type name string to a TypeScript Zod schema reference.
///
/// This is for runtime contract generation when you have type names as strings
/// from handler metadata, not `syn::Type` ASTs. For compile-time AST-based
/// conversion, use [`rust_type_to_zod`] instead.
///
/// # String-based parsing
///
/// This function uses string prefix/suffix matching because it operates on
/// type name strings collected at link time via `inventory`. It handles:
///
/// - Wrapper unwrapping: `"Json<Planet>"` → `"PlanetSchema"`
/// - Result unwrapping: `"Result<Json<Planet>, E>"` → `"PlanetSchema"`
/// - Vec mapping: `"Json<Vec<Planet>>"` → `"z.array(PlanetSchema)"`
/// - Primitive mapping: `"String"` → `"z.string()"`
/// - SSE streams: `"Sse<...>"` → `"asyncIteratorObject(z.unknown())"`
///
/// # Examples
///
/// ```
/// use rorpc_parse::codegen::rust_type_to_ts_schema;
///
/// assert_eq!(rust_type_to_ts_schema("Json<Planet>"), "PlanetSchema");
/// assert_eq!(rust_type_to_ts_schema("Json<Vec<Planet>>"), "z.array(PlanetSchema)");
/// assert_eq!(rust_type_to_ts_schema("Result<Json<Planet>, E>"), "PlanetSchema");
/// assert_eq!(rust_type_to_ts_schema("String"), "z.string()");
/// assert_eq!(rust_type_to_ts_schema("()"), "");
/// ```
pub fn rust_type_to_ts_schema(raw: &str) -> String {
    let raw = raw.replace(' ', "");

    if raw.starts_with("Sse<") {
        return "asyncIteratorObject(z.unknown() /* TODO: add #[derive(ZodTs)] to your stream event type */)".to_string();
    }

    // Unwrap Result<T, E> → T
    let inner = if raw.starts_with("Result<") {
        extract_first_generic_arg_string(&raw).unwrap_or(raw.clone())
    } else {
        raw.clone()
    };

    // Unwrap Json<T> → T
    let inner = if inner.starts_with("Json<") && inner.ends_with('>') {
        inner[5..inner.len() - 1].to_string()
    } else {
        inner
    };

    type_name_to_zod_ref(&inner)
}

/// Map a bare type name to its Zod schema reference.
fn type_name_to_zod_ref(type_name: &str) -> String {
    match type_name {
        "()" | "" => String::new(),
        "String" | "str" => "z.string()".to_string(),
        "bool" => "z.boolean()".to_string(),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" => "z.number().int()".to_string(),
        "f32" | "f64" => "z.number()".to_string(),
        "Uuid" => "z.uuid()".to_string(),
        "DateTime" => "z.iso.datetime({ offset: true })".to_string(),
        "serde_json::Value" | "Value" => "z.any()".to_string(),
        _ if type_name.starts_with("Vec<") && type_name.ends_with('>') => {
            let inner = &type_name[4..type_name.len() - 1];
            format!("z.array({})", type_name_to_zod_ref(inner))
        }
        _ if type_name.starts_with("Option<") && type_name.ends_with('>') => {
            let inner = &type_name[7..type_name.len() - 1];
            format!("{}.optional()", type_name_to_zod_ref(inner))
        }
        _ => {
            let base = type_name.rsplit("::").next().unwrap_or(type_name);
            format!("{}Schema", base)
        }
    }
}

/// Extract the first generic argument from a type string.
///
/// `"Result<Json<Planet>, E>"` → `Some("Json<Planet>")`
fn extract_first_generic_arg_string(type_str: &str) -> Option<String> {
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
        Some(type_str[start..end].to_string())
    } else {
        None
    }
}

/// Convert type name to schema constant name: `"Planet"` → `"PlanetSchema"`
pub fn to_schema_name(rust_type: &str) -> String {
    format!("{}Schema", base_type_name(rust_type))
}

/// Extract the base type name, stripping all wrappers.
///
/// `"Result<Json<Vec<Planet>>, E>"` → `"Planet"`
pub fn base_type_name(rust_type: &str) -> String {
    let mut base = rust_type.trim();

    if base.starts_with("Result<")
        && let Some(inner) = extract_first_generic_arg_string(base)
    {
        base = Box::leak(inner.into_boxed_str());
    }
    if base.starts_with("Json<") && base.ends_with('>') {
        base = &base[5..base.len() - 1];
    }
    if base.starts_with("Vec<") && base.ends_with('>') {
        base = &base[4..base.len() - 1];
    }
    if base.starts_with("Option<") && base.ends_with('>') {
        base = &base[7..base.len() - 1];
    }

    base.rsplit("::").next().unwrap_or(base).to_string()
}

#[cfg(test)]
mod runtime_conversion_tests {
    use super::*;

    #[test]
    fn json_planet() {
        assert_eq!(rust_type_to_ts_schema("Json<Planet>"), "PlanetSchema");
    }

    #[test]
    fn json_vec_planet() {
        assert_eq!(
            rust_type_to_ts_schema("Json<Vec<Planet>>"),
            "z.array(PlanetSchema)"
        );
    }

    #[test]
    fn result_json_planet() {
        assert_eq!(
            rust_type_to_ts_schema("Result<Json<Planet>, StatusCode>"),
            "PlanetSchema"
        );
    }

    #[test]
    fn json_string() {
        assert_eq!(rust_type_to_ts_schema("Json<String>"), "z.string()");
    }

    #[test]
    fn unit_type() {
        assert_eq!(rust_type_to_ts_schema("()"), "");
    }

    #[test]
    fn serde_json_value() {
        assert_eq!(rust_type_to_ts_schema("Json<serde_json::Value>"), "z.any()");
    }

    #[test]
    fn schema_name_simple() {
        assert_eq!(to_schema_name("Planet"), "PlanetSchema");
    }

    #[test]
    fn schema_name_vec() {
        assert_eq!(to_schema_name("Vec<Planet>"), "PlanetSchema");
    }

    #[test]
    fn base_type_unwraps_wrappers() {
        assert_eq!(base_type_name("Result<Json<Vec<Planet>>, E>"), "Planet");
        assert_eq!(base_type_name("Json<Planet>"), "Planet");
        assert_eq!(base_type_name("Vec<Planet>"), "Planet");
        assert_eq!(base_type_name("Option<Planet>"), "Planet");
    }

    #[test]
    fn base_type_strips_module_path() {
        assert_eq!(base_type_name("models::Planet"), "Planet");
        assert_eq!(base_type_name("crate::domain::Planet"), "Planet");
    }
}
