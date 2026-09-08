//! TypeScript import and Zod schema generation.
//!
//! Re-exports runtime string-based type conversion utilities from `orpc_parse`.
//! The actual implementations live in `orpc_parse::codegen::zod_ts` to avoid
//! duplication and keep all type-to-Zod logic in one place.

use super::{HandlerInfo, ir};
use std::collections::BTreeSet;

// Re-export runtime conversion utilities from rorpc-parse
pub use rorpc_parse::codegen::{base_type_name, rust_type_to_ts_schema, to_schema_name};
pub use rorpc_parse::types::is_primitive_type_name;

/// Generate standard TypeScript import block.
pub fn generate_imports() -> String {
    [
        r#"import { z } from "zod";"#,
        r#"import { oc } from "@orpc/contract";"#,
        r#"import { openapi } from "@orpc/openapi";"#,
        r#"import { asyncIteratorObject } from "@orpc/contract";"#,
    ]
    .join("\n")
}

// ---------------------------------------------------------------------------
// Data-oriented emission (replaces generate_real_schemas)
// ---------------------------------------------------------------------------

/// Emit TypeScript for a slice of fully resolved schemas.
///
/// One pass over structured data — no string scanning, no post-hoc replacement.
/// Each `ResolvedField.zod_expr` is already the complete, correct expression
/// (primitives inline, cross-references already disambiguated by the resolution
/// pass in `lib.rs`).
pub fn emit_resolved_schemas(schemas: &[ir::ResolvedSchema]) -> String {
    schemas
        .iter()
        .map(emit_one_resolved)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn emit_one_resolved(s: &ir::ResolvedSchema) -> String {
    match &s.def {
        ir::ResolvedDef::Object { fields } => {
            let field_lines = fields
                .iter()
                .map(|f| format!("  {}: {}", f.ts_key, f.zod_expr))
                .collect::<Vec<_>>()
                .join(",\n");
            format!(
                "export const {name} = z.object({{\n{fields}\n}});\n\nexport type {ty} = z.infer<typeof {name}>;",
                name = s.ts_schema_name,
                fields = field_lines,
                ty = s.ts_type_name,
            )
        }
        ir::ResolvedDef::Enum { variants } => {
            let variant_exprs = variants
                .iter()
                .map(emit_one_variant)
                .collect::<Vec<_>>()
                .join(",\n  ");
            format!(
                "export const {name} = z.union([\n  {variants}\n]);\n\nexport type {ty} = z.infer<typeof {name}>;",
                name = s.ts_schema_name,
                variants = variant_exprs,
                ty = s.ts_type_name,
            )
        }
    }
}

fn emit_one_variant(v: &ir::ResolvedVariant) -> String {
    let key = ts_object_key(&v.serialized_name);
    match &v.kind {
        ir::ResolvedVariantKind::Unit => {
            format!("z.literal(\"{}\")", escape_str(&v.serialized_name))
        }
        ir::ResolvedVariantKind::Newtype { zod_expr } => {
            format!("z.object({{ {}: {} }})", key, zod_expr)
        }
        ir::ResolvedVariantKind::Struct { fields } => {
            let field_exprs = fields
                .iter()
                .map(|f| format!("{}: {}", f.ts_key, f.zod_expr))
                .collect::<Vec<_>>()
                .join(", ");
            format!("z.object({{ {}: z.object({{ {} }}) }})", key, field_exprs)
        }
    }
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

/// Parse a Zod object schema string and extract field definitions.
///
/// Returns a map of field names to their Zod type definitions, or `None` if
/// the input is not a `z.object()` schema.
///
/// # Examples
///
/// ```
/// # use std::collections::HashMap;
/// # use rorpc::codegen::typescript::parse_zod_object_fields;
/// let schema = "z.object({ id: z.number().int(), name: z.string() })";
/// let fields = parse_zod_object_fields(schema).unwrap();
/// assert_eq!(fields.get("id"), Some(&"z.number().int()".to_string()));
/// assert_eq!(fields.get("name"), Some(&"z.string()".to_string()));
///
/// // Non-object schemas return None
/// assert!(parse_zod_object_fields("z.string()").is_none());
/// ```
pub fn parse_zod_object_fields(schema: &str) -> Option<std::collections::HashMap<String, String>> {
    let trimmed = schema.trim();

    if !trimmed.starts_with("z.object({") || !trimmed.ends_with("})") {
        return None;
    }

    // Extract inner content between { and }
    let start = trimmed.find('{')? + 1;
    let end = trimmed.rfind('}')?;
    let inner = &trimmed[start..end];

    let mut fields = std::collections::HashMap::new();
    let mut depth = 0;
    let mut current_field = String::new();
    let mut current_name = String::new();
    let mut in_field_name = true;

    for ch in inner.chars() {
        match ch {
            '{' | '(' => depth += 1,
            '}' | ')' => depth -= 1,
            ':' if depth == 0 && in_field_name => {
                current_name = current_field.trim().to_string();
                current_field.clear();
                in_field_name = false;
                continue;
            }
            ',' if depth == 0 => {
                if !current_name.is_empty() {
                    fields.insert(current_name.clone(), current_field.trim().to_string());
                }
                current_field.clear();
                current_name.clear();
                in_field_name = true;
                continue;
            }
            _ => {}
        }
        current_field.push(ch);
    }

    // Handle last field
    if !current_name.is_empty() {
        fields.insert(current_name, current_field.trim().to_string());
    }

    Some(fields)
}

/// Fallback: generate placeholder schemas from handler type names.
pub fn generate_placeholder_schemas(handlers: &[HandlerInfo]) -> String {
    let mut lines = Vec::new();

    let unique_types: BTreeSet<&str> = handlers
        .iter()
        .flat_map(|h| [h.input_type_name, h.output_type_name])
        .filter(|t| !is_primitive_type_name(t))
        .collect();

    if unique_types.is_empty() {
        return String::new();
    }

    lines.push(
        "// ⚠️  Placeholder schemas — add #[derive(ZodTs)] to your types for real schemas"
            .to_string(),
    );
    lines.push(String::new());

    for type_name in unique_types {
        let schema_name = to_schema_name(type_name);
        let base_name = base_type_name(type_name);
        lines.push(format!(
            "// TODO: add #[derive(ZodTs)] to {base_name} in Rust"
        ));
        lines.push(format!(
            "export const {schema_name} = z.unknown(); // {type_name}"
        ));
        lines.push(format!(
            "export type {base_name} = z.infer<typeof {schema_name}>;"
        ));
        lines.push(String::new());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_check() {
        assert!(is_primitive_type_name("String"));
        assert!(is_primitive_type_name("()"));
        assert!(!is_primitive_type_name("Planet"));
    }

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
            rust_type_to_ts_schema("Result<Json<Planet>,StatusCode>"),
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
        assert_eq!(rust_type_to_ts_schema("Json<serde_json::Value>"), "z.any()");
    }

    #[test]
    fn schema_name_simple() {
        assert_eq!(to_schema_name("Planet"), "PlanetSchema");
    }
}
