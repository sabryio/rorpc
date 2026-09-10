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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SchemaCategory {
    Enum,
    Domain,
    Input,
    Request,
    SseEvent,
}

/// Categorize a schema based on its characteristics.
fn categorize_schema(schema: &ir::ResolvedSchema) -> SchemaCategory {
    // Enums go in Enum Types section
    if matches!(schema.def, ir::ResolvedDef::Enum { .. }) {
        return SchemaCategory::Enum;
    }

    let type_name = &schema.ts_type_name;
    let lower = type_name.to_lowercase();

    // Check for SSE/Event patterns
    if lower.contains("event") || lower.contains("sse") || type_name.ends_with("Data") {
        return SchemaCategory::SseEvent;
    }

    // Check for Input/Request/Query patterns
    if type_name.ends_with("Input")
        || type_name.ends_with("Request")
        || type_name.ends_with("Query")
    {
        // Further distinguish Request from Input
        if type_name.ends_with("Request") {
            return SchemaCategory::Request;
        }
        return SchemaCategory::Input;
    }

    // Default to Domain
    SchemaCategory::Domain
}

/// Extract module name from module_path for grouping.
///
/// Examples:
/// - `"crate::entities::Session"` → `"entities"`
/// - `"crate::handlers::stream::EventData"` → `"handlers_stream"`
/// - `""` → `"_ungrouped"`
fn extract_module_name(module_path: &str) -> String {
    if module_path.is_empty() {
        return "_ungrouped".to_string();
    }

    // Split by :: and collect segments
    let segments: Vec<&str> = module_path.split("::").collect();

    if segments.len() <= 1 {
        return "_ungrouped".to_string();
    }

    // Skip "crate" prefix and last segment (type name)
    let module_segments: Vec<&str> = segments
        .iter()
        .copied()
        .skip_while(|&s| s == "crate")
        .collect();

    // Remove the last segment (type name)
    let module_segments = &module_segments[..module_segments.len().saturating_sub(1)];

    if module_segments.is_empty() {
        return "_ungrouped".to_string();
    }

    // Join nested modules with underscore
    module_segments.join("_")
}

/// Generate a section header with 76-character separator lines.
fn section_header(title: &str) -> String {
    format!(
        "// ============================================================================\n\
         // {}\n\
         // ============================================================================",
        title
    )
}

/// Emit TypeScript for a slice of fully resolved schemas, organized into sections.
///
/// Schemas are grouped by category (Enum, Domain, Input, Request, SSE Events) with
/// clear section headers. Domain and Input types are further grouped by module.
pub fn emit_resolved_schemas(schemas: &[ir::ResolvedSchema]) -> String {
    if schemas.is_empty() {
        return String::new();
    }

    // Emit schemas in topological dependency order (as provided), inserting
    // section headers when category/module changes but never reordering.
    let mut lines = Vec::new();
    let mut last_category: Option<SchemaCategory> = None;
    let mut last_module: Option<String> = None;

    for schema in schemas {
        let category = categorize_schema(schema);
        let module = extract_module_name(schema.module_path);

        // Determine if we need a new section header
        let needs_header = match last_category {
            None => true,                                        // First schema
            Some(ref last_cat) if *last_cat != category => true, // Category changed
            Some(SchemaCategory::Domain) | Some(SchemaCategory::Input) => {
                // For Domain/Input, also check module change
                last_module.as_ref() != Some(&module)
            }
            _ => false,
        };

        if needs_header {
            // Add blank line before new section (except first)
            if last_category.is_some() {
                lines.push(String::new());
            }

            // Generate section header
            let header_title = match category {
                SchemaCategory::Enum => "Enum Types".to_string(),
                SchemaCategory::Domain => {
                    if module == "_ungrouped" {
                        "Domain Types".to_string()
                    } else {
                        format!(
                            "Domain Types - {}",
                            module
                                .split('_')
                                .map(|s| {
                                    let mut c = s.chars();
                                    match c.next() {
                                        None => String::new(),
                                        Some(first) => {
                                            first.to_uppercase().collect::<String>() + c.as_str()
                                        }
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    }
                }
                SchemaCategory::Input => {
                    if module == "_ungrouped" {
                        "Input Types".to_string()
                    } else {
                        format!(
                            "Input Types - {}",
                            module
                                .split('_')
                                .map(|s| {
                                    let mut c = s.chars();
                                    match c.next() {
                                        None => String::new(),
                                        Some(first) => {
                                            first.to_uppercase().collect::<String>() + c.as_str()
                                        }
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    }
                }
                SchemaCategory::Request => "Request Types".to_string(),
                SchemaCategory::SseEvent => "SSE Event Types".to_string(),
            };

            lines.push(section_header(&header_title));
            lines.push(String::new());
        }

        // Emit the schema
        lines.push(emit_one_resolved(schema));
        lines.push(String::new());

        // Update tracking state
        last_category = Some(category);
        last_module = Some(module);
    }

    lines.join("\n").trim_end().to_string()
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

    // --- Option serialization tests (nullable vs optional) ---

    #[test]
    fn option_string_without_skip_generates_nullable() {
        // Option<String> without skip_serializing_if → .nullable()
        let field = ir::ResolvedField {
            ts_key: "name".to_string(),
            zod_expr: "z.string().nullable()".to_string(),
        };
        let schema = ir::ResolvedSchema {
            ts_schema_name: "TestSchema".to_string(),
            ts_type_name: "Test".to_string(),
            module_path: "test::Test",
            def: ir::ResolvedDef::Object {
                fields: vec![field],
            },
        };
        let output = emit_one_resolved(&schema);
        assert!(output.contains("name: z.string().nullable()"));
    }

    #[test]
    fn option_string_with_skip_generates_optional() {
        // Option<String> with skip_serializing_if → .optional()
        let field = ir::ResolvedField {
            ts_key: "name".to_string(),
            zod_expr: "z.string().optional()".to_string(),
        };
        let schema = ir::ResolvedSchema {
            ts_schema_name: "TestSchema".to_string(),
            ts_type_name: "Test".to_string(),
            module_path: "test::Test",
            def: ir::ResolvedDef::Object {
                fields: vec![field],
            },
        };
        let output = emit_one_resolved(&schema);
        assert!(output.contains("name: z.string().optional()"));
    }

    #[test]
    fn option_custom_type_without_skip_generates_nullable() {
        // Option<CustomType> without skip_serializing_if → CustomTypeSchema.nullable()
        let field = ir::ResolvedField {
            ts_key: "session".to_string(),
            zod_expr: "SessionSchema.nullable()".to_string(),
        };
        let schema = ir::ResolvedSchema {
            ts_schema_name: "TestSchema".to_string(),
            ts_type_name: "Test".to_string(),
            module_path: "test::Test",
            def: ir::ResolvedDef::Object {
                fields: vec![field],
            },
        };
        let output = emit_one_resolved(&schema);
        assert!(output.contains("session: SessionSchema.nullable()"));
    }

    #[test]
    fn option_custom_type_with_skip_generates_optional() {
        // Option<CustomType> with skip_serializing_if → CustomTypeSchema.optional()
        let field = ir::ResolvedField {
            ts_key: "session".to_string(),
            zod_expr: "SessionSchema.optional()".to_string(),
        };
        let schema = ir::ResolvedSchema {
            ts_schema_name: "TestSchema".to_string(),
            ts_type_name: "Test".to_string(),
            module_path: "test::Test",
            def: ir::ResolvedDef::Object {
                fields: vec![field],
            },
        };
        let output = emit_one_resolved(&schema);
        assert!(output.contains("session: SessionSchema.optional()"));
    }

    // --- Schema organization tests ---

    #[test]
    fn categorizes_enum_correctly() {
        let schema = ir::ResolvedSchema {
            ts_schema_name: "StatusSchema".to_string(),
            ts_type_name: "Status".to_string(),
            module_path: "app::Status",
            def: ir::ResolvedDef::Enum {
                repr: ir::ResolvedEnumRepr::External,
                variants: vec![],
            },
        };
        assert_eq!(categorize_schema(&schema), SchemaCategory::Enum);
    }

    #[test]
    fn categorizes_input_types_correctly() {
        let schema = ir::ResolvedSchema {
            ts_schema_name: "CreateUserInputSchema".to_string(),
            ts_type_name: "CreateUserInput".to_string(),
            module_path: "app::CreateUserInput",
            def: ir::ResolvedDef::Object { fields: vec![] },
        };
        assert_eq!(categorize_schema(&schema), SchemaCategory::Input);

        let query_schema = ir::ResolvedSchema {
            ts_schema_name: "SearchQuerySchema".to_string(),
            ts_type_name: "SearchQuery".to_string(),
            module_path: "app::SearchQuery",
            def: ir::ResolvedDef::Object { fields: vec![] },
        };
        assert_eq!(categorize_schema(&query_schema), SchemaCategory::Input);
    }

    #[test]
    fn categorizes_request_types_correctly() {
        let schema = ir::ResolvedSchema {
            ts_schema_name: "UpdateUserRequestSchema".to_string(),
            ts_type_name: "UpdateUserRequest".to_string(),
            module_path: "app::UpdateUserRequest",
            def: ir::ResolvedDef::Object { fields: vec![] },
        };
        assert_eq!(categorize_schema(&schema), SchemaCategory::Request);
    }

    #[test]
    fn categorizes_sse_event_types_correctly() {
        let event_schema = ir::ResolvedSchema {
            ts_schema_name: "UserEventSchema".to_string(),
            ts_type_name: "UserEvent".to_string(),
            module_path: "app::UserEvent",
            def: ir::ResolvedDef::Object { fields: vec![] },
        };
        assert_eq!(categorize_schema(&event_schema), SchemaCategory::SseEvent);

        let data_schema = ir::ResolvedSchema {
            ts_schema_name: "StreamDataSchema".to_string(),
            ts_type_name: "StreamData".to_string(),
            module_path: "app::StreamData",
            def: ir::ResolvedDef::Object { fields: vec![] },
        };
        assert_eq!(categorize_schema(&data_schema), SchemaCategory::SseEvent);
    }

    #[test]
    fn categorizes_domain_types_by_default() {
        let schema = ir::ResolvedSchema {
            ts_schema_name: "UserSchema".to_string(),
            ts_type_name: "User".to_string(),
            module_path: "app::User",
            def: ir::ResolvedDef::Object { fields: vec![] },
        };
        assert_eq!(categorize_schema(&schema), SchemaCategory::Domain);
    }

    #[test]
    fn extracts_module_name_simple() {
        assert_eq!(extract_module_name("crate::entities::Session"), "entities");
    }

    #[test]
    fn extracts_module_name_nested() {
        assert_eq!(
            extract_module_name("crate::handlers::stream::EventData"),
            "handlers_stream"
        );
    }

    #[test]
    fn extracts_module_name_ungrouped_when_empty() {
        assert_eq!(extract_module_name(""), "_ungrouped");
    }

    #[test]
    fn extracts_module_name_ungrouped_when_no_path() {
        assert_eq!(extract_module_name("Session"), "_ungrouped");
    }

    #[test]
    fn section_header_format() {
        let header = section_header("Enum Types");
        assert!(header.contains("// ===="));
        assert!(header.contains("// Enum Types"));
        assert_eq!(header.lines().count(), 3);
    }

    #[test]
    fn emit_resolved_schemas_organizes_by_category() {
        let enum_schema = ir::ResolvedSchema {
            ts_schema_name: "StatusSchema".to_string(),
            ts_type_name: "Status".to_string(),
            module_path: "app::Status",
            def: ir::ResolvedDef::Enum {
                repr: ir::ResolvedEnumRepr::External,
                variants: vec![ir::ResolvedVariant {
                    serialized_name: "active".to_string(),
                    kind: ir::ResolvedVariantKind::Unit,
                }],
            },
        };

        let domain_schema = ir::ResolvedSchema {
            ts_schema_name: "UserSchema".to_string(),
            ts_type_name: "User".to_string(),
            module_path: "crate::entities::User",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.string()".to_string(),
                }],
            },
        };

        let input_schema = ir::ResolvedSchema {
            ts_schema_name: "CreateUserInputSchema".to_string(),
            ts_type_name: "CreateUserInput".to_string(),
            module_path: "crate::handlers::CreateUserInput",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "name".to_string(),
                    zod_expr: "z.string()".to_string(),
                }],
            },
        };

        let schemas = vec![enum_schema, domain_schema, input_schema];
        let output = emit_resolved_schemas(&schemas);

        // Check section headers appear in correct order
        let enum_pos = output.find("// Enum Types").expect("Enum Types section");
        let domain_pos = output
            .find("// Domain Types - Entities")
            .expect("Domain Types section");
        let input_pos = output
            .find("// Input Types - Handlers")
            .expect("Input Types section");

        assert!(enum_pos < domain_pos);
        assert!(domain_pos < input_pos);

        // Check schemas appear in their sections
        assert!(output.contains("StatusSchema"));
        assert!(output.contains("UserSchema"));
        assert!(output.contains("CreateUserInputSchema"));
    }

    #[test]
    fn preserves_dependency_order_within_same_module() {
        // Regression test for: CampaignSchema references ContactSchema before it's declared
        // Both schemas are in the same module, and alphabetically Campaign comes before Contact,
        // but Contact must be declared first because Campaign depends on it.

        let contact_schema = ir::ResolvedSchema {
            ts_schema_name: "ContactSchema".to_string(),
            ts_type_name: "Contact".to_string(),
            module_path: "app::core_types::Contact",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.string()".to_string(),
                }],
            },
        };

        let campaign_schema = ir::ResolvedSchema {
            ts_schema_name: "CampaignSchema".to_string(),
            ts_type_name: "Campaign".to_string(),
            module_path: "app::core_types::Campaign",
            def: ir::ResolvedDef::Object {
                fields: vec![
                    ir::ResolvedField {
                        ts_key: "id".to_string(),
                        zod_expr: "z.string()".to_string(),
                    },
                    ir::ResolvedField {
                        ts_key: "contacts".to_string(),
                        zod_expr: "z.array(ContactSchema)".to_string(), // References ContactSchema
                    },
                ],
            },
        };

        // Pass schemas in dependency order: Contact before Campaign
        let schemas = vec![contact_schema, campaign_schema];
        let output = emit_resolved_schemas(&schemas);

        // Find positions of schema declarations
        let contact_pos = output
            .find("export const ContactSchema")
            .expect("ContactSchema declaration");
        let campaign_pos = output
            .find("export const CampaignSchema")
            .expect("CampaignSchema declaration");

        // Contact must be declared before Campaign (dependency order preserved)
        assert!(
            contact_pos < campaign_pos,
            "ContactSchema must be declared before CampaignSchema (found at {} and {} respectively)",
            contact_pos,
            campaign_pos
        );

        // Verify the reference is present in CampaignSchema
        assert!(
            output.contains("z.array(ContactSchema)"),
            "Campaign should reference ContactSchema"
        );
    }
    
    #[test]
    fn preserves_cross_category_dependency_order() {
        // Regression test for: TickResponseSchema (Domain) references ProcessedItemSchema (SSE Event)
        // ProcessedItemSchema must be declared first even though it's in a different category.
    
        let processed_item_schema = ir::ResolvedSchema {
            ts_schema_name: "ProcessedItemSchema".to_string(),
            ts_type_name: "ProcessedItem".to_string(),
            module_path: "app::events::ProcessedItem",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "item_id".to_string(),
                    zod_expr: "z.string()".to_string(),
                }],
            },
        };
    
        let tick_response_schema = ir::ResolvedSchema {
            ts_schema_name: "TickResponseSchema".to_string(),
            ts_type_name: "TickResponse".to_string(),
            module_path: "app::handlers::scheduler::TickResponse",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "processed".to_string(),
                    zod_expr: "z.array(ProcessedItemSchema)".to_string(), // References ProcessedItemSchema
                }],
            },
        };
    
        // Pass schemas in dependency order: ProcessedItem before TickResponse
        let schemas = vec![processed_item_schema, tick_response_schema];
        let output = emit_resolved_schemas(&schemas);
    
        // Find positions of schema declarations
        let processed_pos = output
            .find("export const ProcessedItemSchema")
            .expect("ProcessedItemSchema declaration");
        let tick_pos = output
            .find("export const TickResponseSchema")
            .expect("TickResponseSchema declaration");
    
        // ProcessedItem must be declared before TickResponse (cross-category dependency preserved)
        assert!(
            processed_pos < tick_pos,
            "ProcessedItemSchema must be declared before TickResponseSchema (found at {} and {} respectively)",
            processed_pos,
            tick_pos
        );
    
        // Verify the reference is present in TickResponseSchema
        assert!(
            output.contains("z.array(ProcessedItemSchema)"),
            "TickResponse should reference ProcessedItemSchema"
        );
    }
}
