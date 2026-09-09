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
        ir::ResolvedDef::Enum { repr, variants } => {
            // Branch on repr to emit the correct Zod construct
            match repr {
                ir::ResolvedEnumRepr::External => {
                    // z.union([...]) — current behavior
                    let variant_exprs = variants
                        .iter()
                        .map(|v| emit_variant_for_repr(v, repr))
                        .collect::<Vec<_>>()
                        .join(",\n  ");
                    format!(
                        "export const {name} = z.union([\n  {variants}\n]);\n\nexport type {ty} = z.infer<typeof {name}>;",
                        name = s.ts_schema_name,
                        variants = variant_exprs,
                        ty = s.ts_type_name,
                    )
                }
                ir::ResolvedEnumRepr::Untagged => {
                    // z.union([...]) — no discriminant wrapper
                    let variant_exprs = variants
                        .iter()
                        .map(|v| emit_variant_for_repr(v, repr))
                        .collect::<Vec<_>>()
                        .join(",\n  ");
                    format!(
                        "export const {name} = z.union([\n  {variants}\n]);\n\nexport type {ty} = z.infer<typeof {name}>;",
                        name = s.ts_schema_name,
                        variants = variant_exprs,
                        ty = s.ts_type_name,
                    )
                }
                ir::ResolvedEnumRepr::Internal { tag } => {
                    // z.discriminatedUnion("<tag>", [...])
                    let variant_exprs = variants
                        .iter()
                        .map(|v| emit_variant_for_repr(v, repr))
                        .collect::<Vec<_>>()
                        .join(",\n  ");
                    format!(
                        "export const {name} = z.discriminatedUnion(\"{tag}\", [\n  {variants}\n]);\n\nexport type {ty} = z.infer<typeof {name}>;",
                        name = s.ts_schema_name,
                        tag = escape_str(tag),
                        variants = variant_exprs,
                        ty = s.ts_type_name,
                    )
                }
                ir::ResolvedEnumRepr::Adjacent { tag, content: _ } => {
                    // z.discriminatedUnion("<tag>", [...])
                    let variant_exprs = variants
                        .iter()
                        .map(|v| emit_variant_for_repr(v, repr))
                        .collect::<Vec<_>>()
                        .join(",\n  ");
                    format!(
                        "export const {name} = z.discriminatedUnion(\"{tag}\", [\n  {variants}\n]);\n\nexport type {ty} = z.infer<typeof {name}>;",
                        name = s.ts_schema_name,
                        tag = escape_str(tag),
                        variants = variant_exprs,
                        ty = s.ts_type_name,
                    )
                }
            }
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

/// Emit a variant according to its enum representation strategy.
fn emit_variant_for_repr(v: &ir::ResolvedVariant, repr: &ir::ResolvedEnumRepr) -> String {
    match repr {
        ir::ResolvedEnumRepr::External => {
            // External: current behavior via emit_one_variant
            emit_one_variant(v)
        }
        ir::ResolvedEnumRepr::Untagged => {
            // Untagged: no wrapper, emit the value directly
            match &v.kind {
                ir::ResolvedVariantKind::Unit => {
                    // Unit → z.literal("name")
                    format!("z.literal(\"{}\")", escape_str(&v.serialized_name))
                }
                ir::ResolvedVariantKind::Newtype { zod_expr } => {
                    // Newtype → zod_expr directly (unwrapped)
                    zod_expr.clone()
                }
                ir::ResolvedVariantKind::Struct { fields } => {
                    // Struct → z.object({ fields })
                    let field_exprs = fields
                        .iter()
                        .map(|f| format!("{}: {}", f.ts_key, f.zod_expr))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("z.object({{ {} }})", field_exprs)
                }
            }
        }
        ir::ResolvedEnumRepr::Internal { tag } => {
            // Internal: tag merged into fields
            let tag_key = ts_object_key(tag);
            match &v.kind {
                ir::ResolvedVariantKind::Unit => {
                    // Unit → z.object({ tag: z.literal("name") })
                    format!(
                        "z.object({{ {}: z.literal(\"{}\") }})",
                        tag_key,
                        escape_str(&v.serialized_name)
                    )
                }
                ir::ResolvedVariantKind::Newtype { zod_expr } => {
                    // Newtype → merge tag with inner fields if possible, else .and()
                    if let Some(field_map) = parse_zod_object_fields(zod_expr) {
                        // Inner is z.object({...}) — merge tag into it
                        let mut all_fields = vec![format!(
                            "{}: z.literal(\"{}\")",
                            tag_key,
                            escape_str(&v.serialized_name)
                        )];
                        for (k, v) in field_map.iter() {
                            all_fields.push(format!("{}: {}", k, v));
                        }
                        format!("z.object({{ {} }})", all_fields.join(", "))
                    } else {
                        // Not an object — use .and()
                        format!(
                            "z.object({{ {}: z.literal(\"{}\") }}).and({})",
                            tag_key,
                            escape_str(&v.serialized_name),
                            zod_expr
                        )
                    }
                }
                ir::ResolvedVariantKind::Struct { fields } => {
                    // Struct → z.object({ tag: z.literal("name"), ...fields })
                    let mut all_exprs = vec![format!(
                        "{}: z.literal(\"{}\")",
                        tag_key,
                        escape_str(&v.serialized_name)
                    )];
                    for f in fields {
                        all_exprs.push(format!("{}: {}", f.ts_key, f.zod_expr));
                    }
                    format!("z.object({{ {} }})", all_exprs.join(", "))
                }
            }
        }
        ir::ResolvedEnumRepr::Adjacent { tag, content } => {
            // Adjacent: { tag: "name", content: {...} } — unit variants omit content
            let tag_key = ts_object_key(tag);
            let content_key = ts_object_key(content);
            match &v.kind {
                ir::ResolvedVariantKind::Unit => {
                    // Unit → z.object({ tag: z.literal("name") }) — no content field
                    format!(
                        "z.object({{ {}: z.literal(\"{}\") }})",
                        tag_key,
                        escape_str(&v.serialized_name)
                    )
                }
                ir::ResolvedVariantKind::Newtype { zod_expr } => {
                    // Newtype → z.object({ tag: z.literal("name"), content: zod_expr })
                    format!(
                        "z.object({{ {}: z.literal(\"{}\"), {}: {} }})",
                        tag_key,
                        escape_str(&v.serialized_name),
                        content_key,
                        zod_expr
                    )
                }
                ir::ResolvedVariantKind::Struct { fields } => {
                    // Struct → z.object({ tag: z.literal("name"), content: z.object({ fields }) })
                    let field_exprs = fields
                        .iter()
                        .map(|f| format!("{}: {}", f.ts_key, f.zod_expr))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "z.object({{ {}: z.literal(\"{}\"), {}: z.object({{ {} }}) }})",
                        tag_key,
                        escape_str(&v.serialized_name),
                        content_key,
                        field_exprs
                    )
                }
            }
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
        assert_eq!(
            rust_type_to_ts_schema("Json<serde_json::Value>"),
            "z.record(z.string(), z.unknown())"
        );
    }

    #[test]
    fn schema_name_simple() {
        assert_eq!(to_schema_name("Planet"), "PlanetSchema");
    }

    // --- Enum representation tests (T006) ---

    #[test]
    fn external_unit_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "ping".to_string(),
            kind: ir::ResolvedVariantKind::Unit,
        };
        let repr = ir::ResolvedEnumRepr::External;
        assert_eq!(emit_variant_for_repr(&v, &repr), r#"z.literal("ping")"#);
    }

    #[test]
    fn external_newtype_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "value".to_string(),
            kind: ir::ResolvedVariantKind::Newtype {
                zod_expr: "z.string()".to_string(),
            },
        };
        let repr = ir::ResolvedEnumRepr::External;
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ value: z.string() })"#
        );
    }

    #[test]
    fn external_struct_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "data".to_string(),
            kind: ir::ResolvedVariantKind::Struct {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.number()".to_string(),
                }],
            },
        };
        let repr = ir::ResolvedEnumRepr::External;
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ data: z.object({ id: z.number() }) })"#
        );
    }

    #[test]
    fn untagged_unit_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "ping".to_string(),
            kind: ir::ResolvedVariantKind::Unit,
        };
        let repr = ir::ResolvedEnumRepr::Untagged;
        assert_eq!(emit_variant_for_repr(&v, &repr), r#"z.literal("ping")"#);
    }

    #[test]
    fn untagged_newtype_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "value".to_string(),
            kind: ir::ResolvedVariantKind::Newtype {
                zod_expr: "z.string()".to_string(),
            },
        };
        let repr = ir::ResolvedEnumRepr::Untagged;
        // Unwrapped — just the zod_expr
        assert_eq!(emit_variant_for_repr(&v, &repr), "z.string()");
    }

    #[test]
    fn untagged_struct_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "data".to_string(),
            kind: ir::ResolvedVariantKind::Struct {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.number()".to_string(),
                }],
            },
        };
        let repr = ir::ResolvedEnumRepr::Untagged;
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ id: z.number() })"#
        );
    }

    #[test]
    fn internal_unit_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "ping".to_string(),
            kind: ir::ResolvedVariantKind::Unit,
        };
        let repr = ir::ResolvedEnumRepr::Internal {
            tag: "type".to_string(),
        };
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ type: z.literal("ping") })"#
        );
    }

    #[test]
    fn internal_newtype_variant_with_object() {
        let v = ir::ResolvedVariant {
            serialized_name: "data".to_string(),
            kind: ir::ResolvedVariantKind::Newtype {
                zod_expr: "z.object({ id: z.number(), name: z.string() })".to_string(),
            },
        };
        let repr = ir::ResolvedEnumRepr::Internal {
            tag: "type".to_string(),
        };
        // Should merge tag into the object fields
        let result = emit_variant_for_repr(&v, &repr);
        assert!(result.contains(r#"type: z.literal("data")"#));
        assert!(result.contains("id: z.number()"));
        assert!(result.contains("name: z.string()"));
    }

    #[test]
    fn internal_newtype_variant_non_object() {
        let v = ir::ResolvedVariant {
            serialized_name: "count".to_string(),
            kind: ir::ResolvedVariantKind::Newtype {
                zod_expr: "z.number()".to_string(),
            },
        };
        let repr = ir::ResolvedEnumRepr::Internal {
            tag: "type".to_string(),
        };
        // Should use .and() fallback
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ type: z.literal("count") }).and(z.number())"#
        );
    }

    #[test]
    fn internal_struct_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "user".to_string(),
            kind: ir::ResolvedVariantKind::Struct {
                fields: vec![
                    ir::ResolvedField {
                        ts_key: "id".to_string(),
                        zod_expr: "z.number()".to_string(),
                    },
                    ir::ResolvedField {
                        ts_key: "name".to_string(),
                        zod_expr: "z.string()".to_string(),
                    },
                ],
            },
        };
        let repr = ir::ResolvedEnumRepr::Internal {
            tag: "type".to_string(),
        };
        let result = emit_variant_for_repr(&v, &repr);
        assert!(result.contains(r#"type: z.literal("user")"#));
        assert!(result.contains("id: z.number()"));
        assert!(result.contains("name: z.string()"));
    }

    #[test]
    fn adjacent_unit_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "ping".to_string(),
            kind: ir::ResolvedVariantKind::Unit,
        };
        let repr = ir::ResolvedEnumRepr::Adjacent {
            tag: "type".to_string(),
            content: "data".to_string(),
        };
        // Unit → no content field
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ type: z.literal("ping") })"#
        );
    }

    #[test]
    fn adjacent_newtype_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "value".to_string(),
            kind: ir::ResolvedVariantKind::Newtype {
                zod_expr: "z.string()".to_string(),
            },
        };
        let repr = ir::ResolvedEnumRepr::Adjacent {
            tag: "type".to_string(),
            content: "data".to_string(),
        };
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ type: z.literal("value"), data: z.string() })"#
        );
    }

    #[test]
    fn adjacent_struct_variant() {
        let v = ir::ResolvedVariant {
            serialized_name: "user".to_string(),
            kind: ir::ResolvedVariantKind::Struct {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.number()".to_string(),
                }],
            },
        };
        let repr = ir::ResolvedEnumRepr::Adjacent {
            tag: "type".to_string(),
            content: "data".to_string(),
        };
        assert_eq!(
            emit_variant_for_repr(&v, &repr),
            r#"z.object({ type: z.literal("user"), data: z.object({ id: z.number() }) })"#
        );
    }
}
