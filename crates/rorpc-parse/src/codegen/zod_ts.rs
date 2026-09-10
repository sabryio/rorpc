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
    types::{HASHMAP, OPTION, VEC, is_primitive, try_extract_wrapper},
};

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
    let mut field_tokens: Vec<TokenStream> = Vec::new();
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

        // Check if this Option field has skip_serializing_if = "Option::is_none"
        let skip_if_none = is_opt && matches!(
            serde.skip_serializing_if.as_deref(),
            Some("Option::is_none") | Some("std::option::Option::is_none")
        );

        let base_ty = if is_opt {
            option_inner(&field.ty).unwrap_or(&field.ty)
        } else {
            &field.ty
        };

        // Collect non-primitive custom types for dependency tracking
        let custom = innermost_custom_name(base_ty);
        if let Some(ref c) = custom {
            // Store only the bare name for the dep list (last segment)
            let bare = c.rsplit("::").next().unwrap_or(c.as_str());
            dep_type_names.push(bare.to_string());
        }

        // Build FieldDef token: check containers with custom types first
        let field_tok =
            if let Some(container_expr) = try_generate_container_with_custom_types(base_ty) {
                // Vec<CustomType> or HashMap<K, V> with custom types
                // Emit complete Zod expression with wrapper, not bare type_ref
                let zod_expr = if is_opt {
                    if skip_if_none {
                        format!("{}.optional()", container_expr)
                    } else {
                        format!("{}.nullable()", container_expr)
                    }
                } else {
                    container_expr
                };
                quote! {
                    ::rorpc::FieldDef {
                        ts_name:   #ts_key,
                        zod_expr:  #zod_expr,
                        type_ref:  "",
                        optional:  #is_opt,
                        skip_if_none: #skip_if_none,
                    }
                }
            } else if let Some(ref type_ref) = custom {
                // Bare custom type — emit as type_ref for resolution pass
                let bare_ref = type_ref.rsplit("::").next().unwrap_or(type_ref.as_str());
                quote! {
                    ::rorpc::FieldDef {
                        ts_name:   #ts_key,
                        zod_expr:  "",
                        type_ref:  #bare_ref,
                        optional:  #is_opt,
                        skip_if_none: #skip_if_none,
                    }
                }
            } else {
                // Primitive — compute the full Zod expression now.
                let zod_expr = rust_type_to_zod(base_ty, &zod);
                let zod_expr = if is_opt {
                    if skip_if_none {
                        format!("{}.optional()", zod_expr)
                    } else {
                        format!("{}.nullable()", zod_expr)
                    }
                } else {
                    zod_expr
                };
                quote! {
                    ::rorpc::FieldDef {
                        ts_name:   #ts_key,
                        zod_expr:  #zod_expr,
                        type_ref:  "",
                        optional:  #is_opt,
                        skip_if_none: #skip_if_none,
                    }
                }
            };

        field_tokens.push(field_tok);
    }

    Ok(emit_registration(
        name,
        name_str,
        field_tokens,
        &dep_type_names,
    ))
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

    // Derive EnumRepr from serde container attributes
    let repr = if serde_container.untagged {
        quote! { ::rorpc::EnumRepr::Untagged }
    } else if let (Some(tag), Some(content)) = (&serde_container.tag, &serde_container.content) {
        // Leak to &'static str for static storage
        let tag_static: &'static str = Box::leak(tag.clone().into_boxed_str());
        let content_static: &'static str = Box::leak(content.clone().into_boxed_str());
        quote! { ::rorpc::EnumRepr::Adjacent { tag: #tag_static, content: #content_static } }
    } else if let Some(tag) = &serde_container.tag {
        let tag_static: &'static str = Box::leak(tag.clone().into_boxed_str());
        quote! { ::rorpc::EnumRepr::Internal { tag: #tag_static } }
    } else {
        quote! { ::rorpc::EnumRepr::External }
    };

    let mut variant_tokens: Vec<TokenStream> = Vec::new();

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

        let kind_tok = generate_variant_def(&variant.fields)?;
        variant_tokens.push(quote! {
            ::rorpc::VariantDef {
                serialized_name: #variant_name,
                kind: #kind_tok,
            }
        });
    }

    Ok(emit_enum_registration(name, name_str, repr, variant_tokens))
}

// ---------------------------------------------------------------------------
// Variant code generation — returns a VariantKind TokenStream
// ---------------------------------------------------------------------------

fn generate_variant_def(fields: &Fields) -> Result<TokenStream> {
    match fields {
        Fields::Unit => Ok(quote! { ::rorpc::VariantKind::Unit }),

        Fields::Unnamed(fields_unnamed) => {
            let count = fields_unnamed.unnamed.len();
            if count == 1 {
                let field = fields_unnamed.unnamed.first().unwrap();
                let zod = parse_zod_attrs(&field.attrs)?;
                if let Some(custom) = innermost_custom_name(&field.ty) {
                    let bare_ref = custom.rsplit("::").next().unwrap_or(custom.as_str());
                    Ok(quote! { ::rorpc::VariantKind::NewtypeRef { type_ref: #bare_ref } })
                } else {
                    let schema = rust_type_to_zod(&field.ty, &zod);
                    Ok(quote! { ::rorpc::VariantKind::NewtypeZod { zod_expr: #schema } })
                }
            } else {
                // Multi-field tuple variants not yet supported — emit Unknown
                Ok(quote! { ::rorpc::VariantKind::Unit })
            }
        }

        Fields::Named(fields_named) => {
            let mut field_tokens: Vec<TokenStream> = Vec::new();
            for field in &fields_named.named {
                let field_name = field.ident.as_ref().unwrap().to_string();
                let serde = parse_serde_attrs(&field.attrs)?;
                if serde.skip {
                    continue;
                }
                let ts_key = serde.rename.as_deref().unwrap_or(&field_name);
                let zod_attrs = parse_zod_attrs(&field.attrs)?;
                let is_opt = is_option_type(&field.ty);
                
                // Check if this Option field has skip_serializing_if = "Option::is_none"
                let skip_if_none = is_opt && matches!(
                    serde.skip_serializing_if.as_deref(),
                    Some("Option::is_none") | Some("std::option::Option::is_none")
                );
                
                let base_ty = if is_opt {
                    option_inner(&field.ty).unwrap_or(&field.ty)
                } else {
                    &field.ty
                };

                let field_tok = if let Some(custom) = innermost_custom_name(base_ty) {
                    let bare_ref = custom.rsplit("::").next().unwrap_or(custom.as_str());
                    quote! {
                        ::rorpc::FieldDef {
                            ts_name:  #ts_key,
                            zod_expr: "",
                            type_ref: #bare_ref,
                            optional: #is_opt,
                            skip_if_none: #skip_if_none,
                        }
                    }
                } else {
                    let zod_expr = rust_type_to_zod(base_ty, &zod_attrs);
                    let zod_expr = if is_opt {
                        if skip_if_none {
                            format!("{}.optional()", zod_expr)
                        } else {
                            format!("{}.nullable()", zod_expr)
                        }
                    } else {
                        zod_expr
                    };
                    quote! {
                        ::rorpc::FieldDef {
                            ts_name:  #ts_key,
                            zod_expr: #zod_expr,
                            type_ref: "",
                            optional: #is_opt,
                            skip_if_none: #skip_if_none,
                        }
                    }
                };
                field_tokens.push(field_tok);
            }
            Ok(quote! {
                ::rorpc::VariantKind::Struct {
                    fields: &[ #(#field_tokens),* ]
                }
            })
        }
    }
}

// ---------------------------------------------------------------------------
// inventory::submit! emission
// ---------------------------------------------------------------------------

/// Emit the `inventory::submit!` block for a type.
///
/// `items`    — `FieldDef` tokens for structs, `VariantDef` tokens for enums.
/// `is_enum`  — selects `SchemaDef::Enum` vs `SchemaDef::Object`.
fn emit_registration(
    name: &syn::Ident,
    name_str: &str,
    items: Vec<TokenStream>,
    dep_type_names: &[String],
) -> TokenStream {
    let dep_strs: Vec<&str> = dep_type_names.iter().map(String::as_str).collect();

    quote! {
        impl #name {
            pub fn dependent_types() -> Vec<&'static str> {
                vec![#(#dep_strs),*]
            }
        }

        const _: () = {
            static __FIELDS: &[::rorpc::FieldDef] = &[ #(#items),* ];
            ::rorpc::inventory::submit! {
                ::rorpc::SchemaRegistration {
                    type_name: #name_str,
                    module_path: concat!(module_path!(), "::", #name_str),
                    schema_def: ::rorpc::SchemaDef::Object { fields: __FIELDS },
                    dependent_types: #name::dependent_types,
                }
            }
        };
    }
}

/// Emit the `inventory::submit!` block for an enum type.
fn emit_enum_registration(
    name: &syn::Ident,
    name_str: &str,
    repr: TokenStream,
    items: Vec<TokenStream>,
) -> TokenStream {
    quote! {
        impl #name {
            pub fn dependent_types() -> Vec<&'static str> {
                vec![]
            }
        }

        const _: () = {
            static __VARIANTS: &[::rorpc::VariantDef] = &[ #(#items),* ];
            ::rorpc::inventory::submit! {
                ::rorpc::SchemaRegistration {
                    type_name: #name_str,
                    module_path: concat!(module_path!(), "::", #name_str),
                    schema_def: ::rorpc::SchemaDef::Enum { repr: #repr, variants: __VARIANTS },
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

    // HashMap<K, V>
    if let Some(m) = try_extract_wrapper(ty, HASHMAP) {
        let types = m.all_types();
        if types.len() == 2 {
            let key_schema = rust_type_to_zod(types[0], &ZodAttrs::default());
            let value_schema = rust_type_to_zod(types[1], &ZodAttrs::default());
            return format!("z.record({}, {})", key_schema, value_schema);
        } else {
            // Fallback for malformed HashMap
            return "z.record(z.string(), z.unknown())".to_string();
        }
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
            // serde_json::Value → z.record(z.string(), z.unknown())
            "Value" => "z.record(z.string(), z.unknown())".to_string(),
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

/// Generate a complete Zod expression for containers wrapping custom types.
///
/// Returns `Some("z.array(ItemSchema)")` for `Vec<Item>` where Item is custom.
/// Returns `Some("z.record(z.string(), ItemSchema)")` for `HashMap<String, Item>`.
/// Returns `None` for fully primitive containers (handled by rust_type_to_zod)
/// or bare types (handled by type_ref resolution).
fn try_generate_container_with_custom_types(ty: &syn::Type) -> Option<String> {
    // Vec<T> where T is custom
    if let Some(m) = try_extract_wrapper(ty, VEC)
        && let Some(inner) = m.first_type()
        && let Some(custom_name) = innermost_custom_name(inner)
    {
        let bare = custom_name.rsplit("::").next().unwrap_or(&custom_name);
        return Some(format!("z.array({}Schema)", bare));
    }

    // HashMap<K, V> where K or V is custom
    if let Some(m) = try_extract_wrapper(ty, HASHMAP) {
        let types = m.all_types();
        if types.len() == 2 {
            let key_has_custom = innermost_custom_name(types[0]).is_some();
            let val_has_custom = innermost_custom_name(types[1]).is_some();

            if key_has_custom || val_has_custom {
                let key_expr = type_to_zod_or_schema_ref(types[0]);
                let val_expr = type_to_zod_or_schema_ref(types[1]);
                return Some(format!("z.record({}, {})", key_expr, val_expr));
            }
        }
    }

    None
}

/// Convert a type to either a Zod primitive expression or a schema reference.
///
/// Used when building container expressions that mix primitives and custom types.
fn type_to_zod_or_schema_ref(ty: &syn::Type) -> String {
    if let Some(custom) = innermost_custom_name(ty) {
        let bare = custom.rsplit("::").next().unwrap_or(&custom);
        format!("{}Schema", bare)
    } else {
        rust_type_to_zod(ty, &ZodAttrs::default())
    }
}

/// Return the simple name of the innermost non-primitive, non-wrapper type,
/// for dependency tracking in `dependent_types()`.
fn innermost_custom_name(ty: &syn::Type) -> Option<String> {
    // Strip Vec<T>
    if let Some(m) = try_extract_wrapper(ty, VEC) {
        return m.first_type().and_then(innermost_custom_name);
    }
    // Strip HashMap<K, V> — check both K and V for custom types
    if let Some(m) = try_extract_wrapper(ty, HASHMAP) {
        // For HashMap, we need to check both key and value types
        // Return the first custom type found
        if let Some(key) = m.first_type()
            && let Some(name) = innermost_custom_name(key)
        {
            return Some(name);
        }
        if let Some(value) = m.nth_type(1) {
            return innermost_custom_name(value);
        }
        return None;
    }
    if is_primitive(ty) {
        return None;
    }
    if let syn::Type::Path(tp) = ty {
        // Extract full path by joining all segments
        let segments: Vec<String> = tp
            .path
            .segments
            .iter()
            .map(|seg| seg.ident.to_string())
            .collect();

        if segments.is_empty() {
            return None;
        }

        let last = segments.last().unwrap();
        // Exclude Value (serde_json) from dependency tracking
        if last == "Value" {
            return None;
        }

        // Return full path joined with ::
        return Some(segments.join("::"));
    }
    None
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
/// assert_eq!(rust_type_to_ts_schema("()"), "z.void()");
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
        "()" => "z.void()".to_string(),
        "" => String::new(),
        "String" | "str" => "z.string()".to_string(),
        "bool" => "z.boolean()".to_string(),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" => "z.number().int()".to_string(),
        "f32" | "f64" => "z.number()".to_string(),
        "Uuid" => "z.uuid()".to_string(),
        "DateTime" => "z.iso.datetime({ offset: true })".to_string(),
        "serde_json::Value" | "Value" => "z.record(z.string(), z.unknown())".to_string(),
        _ if type_name.starts_with("Vec<") && type_name.ends_with('>') => {
            let inner = &type_name[4..type_name.len() - 1];
            format!("z.array({})", type_name_to_zod_ref(inner))
        }
        _ if type_name.starts_with("HashMap<") && type_name.ends_with('>') => {
            let inner = &type_name[8..type_name.len() - 1];
            // Parse "K, V" from the HashMap generics
            let parts: Vec<&str> = inner.splitn(2, ',').collect();
            if parts.len() == 2 {
                let key_schema = type_name_to_zod_ref(parts[0].trim());
                let value_schema = type_name_to_zod_ref(parts[1].trim());
                format!("z.record({}, {})", key_schema, value_schema)
            } else {
                // Fallback if we can't parse the generics
                "z.record(z.string(), z.unknown())".to_string()
            }
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
        assert_eq!(rust_type_to_ts_schema("()"), "z.void()");
    }

    #[test]
    fn serde_json_value() {
        assert_eq!(
            rust_type_to_ts_schema("Json<serde_json::Value>"),
            "z.record(z.string(), z.unknown())"
        );
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

    #[test]
    fn hashmap_string_string() {
        assert_eq!(
            rust_type_to_ts_schema("HashMap<String, String>"),
            "z.record(z.string(), z.string())"
        );
    }

    #[test]
    fn hashmap_with_custom_value() {
        assert_eq!(
            rust_type_to_ts_schema("HashMap<String, Planet>"),
            "z.record(z.string(), PlanetSchema)"
        );
    }

    #[test]
    fn json_hashmap() {
        assert_eq!(
            rust_type_to_ts_schema("Json<HashMap<String, String>>"),
            "z.record(z.string(), z.string())"
        );
    }

    #[test]
    fn vec_of_custom_type() {
        let ty: syn::Type = syn::parse_str("Vec<Planet>").unwrap();
        let expr = try_generate_container_with_custom_types(&ty);
        assert_eq!(expr, Some("z.array(PlanetSchema)".to_string()));
    }

    #[test]
    fn vec_of_primitive_returns_none() {
        let ty: syn::Type = syn::parse_str("Vec<String>").unwrap();
        let expr = try_generate_container_with_custom_types(&ty);
        assert_eq!(expr, None);
    }

    #[test]
    fn hashmap_with_custom_key() {
        let ty: syn::Type = syn::parse_str("HashMap<Planet, String>").unwrap();
        let expr = try_generate_container_with_custom_types(&ty);
        assert_eq!(expr, Some("z.record(PlanetSchema, z.string())".to_string()));
    }

    #[test]
    fn hashmap_fully_primitive_returns_none() {
        let ty: syn::Type = syn::parse_str("HashMap<String, i32>").unwrap();
        let expr = try_generate_container_with_custom_types(&ty);
        assert_eq!(expr, None);
    }
}
