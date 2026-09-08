//! TypeScript contract and Zod schema generation.
//!
//! Produces a TypeScript file containing:
//! - Zod schema constants for each input/output type (from `ZodTs::zod_ts()`)
//! - An oRPC `contract` object matching the Rust handler structure
//! - Type exports (`export type X = z.infer<typeof XSchema>`)

pub mod contract;
pub mod ir;
pub mod typescript;

use std::path::Path;

/// Metadata for a single handler used during contract generation.
#[derive(Debug, Clone)]
pub struct HandlerInfo {
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub input_type_name: &'static str,
    pub query_type_name: Option<&'static str>,
    pub output_type_name: &'static str,
    pub module_path: &'static str,
    pub error_type_name: Option<&'static str>,
    pub stream_event_type_name: Option<&'static str>,
    /// Ordered comma-separated Rust type names for `Path<T>` parameters.
    /// E.g., `"i32"` for one path param, `"i32,String"` for two.
    /// Empty string when no `Path<T>` extractors are present.
    pub path_param_types: &'static str,
}

/// A collected Zod schema string for a single Rust type.
#[derive(Debug, Clone)]
pub struct SchemaEntry {
    pub type_name: &'static str,
    pub ts_schema_name: String, // e.g., "SessionSchema", "EntitiesSessionSchema"
}

/// A single error variant for TypeScript `.errors({...})` generation.
#[derive(Debug, Clone)]
pub struct ErrorVariantInfo {
    pub name: &'static str,
    pub data_schema: Option<&'static str>,
}

/// Registration entry for an error enum type.
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub type_name: &'static str,
    pub variants: Vec<ErrorVariantInfo>,
}

/// Builder for TypeScript contract generation.
///
/// # Example
///
/// ```no_run
/// rorpc::generate_contract()
///     .output("../client/src/rpc/index.ts")
///     .unwrap();
/// ```
pub struct ContractBuilder {
    handlers: Vec<HandlerInfo>,
    schemas: Vec<SchemaEntry>,
    errors: Vec<ErrorInfo>,
    /// Fully resolved schemas ready for one-pass TypeScript emission.
    resolved_schemas: Vec<ir::ResolvedSchema>,
}

impl ContractBuilder {
    pub fn new(
        handlers: Vec<HandlerInfo>,
        schemas: Vec<SchemaEntry>,
        resolved_schemas: Vec<ir::ResolvedSchema>,
    ) -> Self {
        Self {
            handlers,
            schemas,
            errors: Vec::new(),
            resolved_schemas,
        }
    }

    pub fn with_errors(mut self, errors: Vec<ErrorInfo>) -> Self {
        self.errors = errors;
        self
    }

    /// Generate the TypeScript contract and write it to `path`.
    ///
    /// Accepts both absolute and relative paths. Relative paths are resolved against
    /// the current working directory (typically `CARGO_MANIFEST_DIR` at build time).
    pub fn output(self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();

        let content = self.generate();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)
    }

    /// Generate the full TypeScript content as a string.
    pub fn generate(self) -> String {
        let imports = typescript::generate_imports();

        // Collect type names that have real schemas — used by placeholder generator.
        let real_schema_types: std::collections::HashSet<&str> =
            self.schemas.iter().map(|s| s.type_name).collect();

        // Emit resolved schemas — one pass over structured data, no string scanning.
        let real_schemas = typescript::emit_resolved_schemas(&self.resolved_schemas);

        // Build unambiguous rename map for the contract block.
        // Collision cases (2+ targets mapping to the same bare name) are left as-is;
        // the schema block will already use the correct disambiguated names.
        let mut rename_groups: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for s in &self.resolved_schemas {
            let default = format!("{}Schema", s.ts_type_name);
            if s.ts_schema_name != default {
                rename_groups
                    .entry(default)
                    .or_default()
                    .push(s.ts_schema_name.clone());
            }
        }
        let unambiguous_renames: Vec<(String, String)> = rename_groups
            .into_iter()
            .filter_map(|(old, targets)| {
                if targets.len() == 1 {
                    Some((old, targets.into_iter().next().unwrap()))
                } else {
                    None
                }
            })
            .collect();

        let placeholder_schemas = generate_missing_placeholders(&self.handlers, &real_schema_types);

        // Build schema_name_map: both bare type name AND full module path → ts_schema_name.
        // Full path entries (e.g. "crate::entities::Session" → "EntitiesSessionSchema") allow
        // contract resolution to pick the correct schema when the handler's output_type_name
        // contains a qualified path. Bare name entries serve as fallbacks.
        let mut schema_name_map: std::collections::HashMap<String, String> = self
            .schemas
            .iter()
            .map(|s| (s.type_name.to_string(), s.ts_schema_name.clone()))
            .collect();
        // Add full module path entries from resolved schemas (module_path is e.g.
        // "issue1_duplicate_schemas::types::Session" → "TypesSessionSchema").
        for s in &self.resolved_schemas {
            if !s.module_path.is_empty() {
                schema_name_map.insert(s.module_path.to_string(), s.ts_schema_name.clone());
                // Also add path without crate prefix ("types::Session" → same)
                // so handlers using "crate::types::Session" hit the right entry.
                // "crate::types::Session" → strip "crate::" prefix variant
                if let Some(without_first) =
                    s.module_path.find("::").map(|i| &s.module_path[i + 2..])
                {
                    schema_name_map.insert(without_first.to_string(), s.ts_schema_name.clone());
                }
            }
        }

        let contract_raw = contract::generate_contract(
            &self.handlers,
            &self.errors,
            &self.schemas,
            &schema_name_map,
        );

        // Apply unambiguous renames to the contract string.
        let contract = unambiguous_renames
            .iter()
            .fold(contract_raw, |text, (old, new)| {
                [(")", ")"), (".", "."), (",", ","), ("\n", "\n")]
                    .iter()
                    .fold(text, |t, (suffix, new_suffix)| {
                        t.replace(
                            &format!("{}{}", old, suffix),
                            &format!("{}{}", new, new_suffix),
                        )
                    })
            });

        let schema_block = match (real_schemas.is_empty(), placeholder_schemas.is_empty()) {
            (true, true) => String::new(),
            (true, false) => placeholder_schemas,
            (false, true) => real_schemas,
            (false, false) => format!("{}\n\n{}", real_schemas, placeholder_schemas),
        };

        format!(
            "// AUTO-GENERATED by rorpc — do not edit manually.\n// Re-generate: rorpc::generate_contract().output(path)\n\n{imports}\n\n{schema_block}\n\n{contract}"
        )
    }
}

/// Generate placeholder schemas only for types that don't have real schemas.
fn generate_missing_placeholders(
    handlers: &[HandlerInfo],
    real_schema_types: &std::collections::HashSet<&str>,
) -> String {
    use std::collections::{BTreeSet, HashMap};

    // Track all type paths that normalize to the same name
    let mut normalized_to_paths: HashMap<String, Vec<String>> = HashMap::new();

    // Helper to normalize type names: strip module paths
    fn normalize_type_name(s: &str) -> String {
        s.rsplit("::").next().unwrap_or(s).to_string()
    }

    // First pass: collect all types and detect collisions
    for handler in handlers {
        let mut type_names = vec![handler.input_type_name, handler.output_type_name];
        if let Some(query_type) = handler.query_type_name {
            type_names.push(query_type);
        }
        for type_str in type_names {
            if !typescript::is_primitive_type_name(type_str) {
                for full_path in extract_base_types(type_str) {
                    let normalized = normalize_type_name(&full_path);
                    normalized_to_paths
                        .entry(normalized)
                        .or_default()
                        .push(full_path);
                }
            }
        }
    }

    // Second pass: decide which types need placeholders
    let mut unique_types: BTreeSet<String> = BTreeSet::new();
    let mut collision_warnings: Vec<String> = Vec::new();

    for (normalized, paths) in &normalized_to_paths {
        // Skip if there's already a real schema for this normalized name
        if real_schema_types.contains(normalized.as_str()) {
            continue;
        }

        // Detect collision: multiple different paths normalize to same name
        if paths.len() > 1 {
            // Sort and deduplicate paths
            let unique_paths: BTreeSet<_> = paths.iter().collect();
            if unique_paths.len() > 1 {
                collision_warnings.push(format!(
                    "// ⚠️  WARNING: Multiple types resolve to '{normalized}Schema': {}",
                    unique_paths
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                collision_warnings.push(
                    "//    Add #[derive(ZodTs)] to one of them, or rename them to avoid collision."
                        .to_string(),
                );
            }
        }

        unique_types.insert(normalized.clone());
    }

    if unique_types.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "// ⚠️  Placeholder schemas — add #[derive(ZodTs)] to your types for real schemas"
            .to_string(),
        String::new(),
    ];

    // Add collision warnings at the top
    if !collision_warnings.is_empty() {
        lines.extend(collision_warnings);
        lines.push(String::new());
    }

    for type_name in unique_types {
        let schema_name = typescript::to_schema_name(&type_name);
        let base_name = typescript::base_type_name(&type_name);
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

/// Extract all concrete type names from a potentially complex type signature.
/// `"Result<Json<Vec<Planet>>, E>"` → `["Planet"]`
/// Skips `Sse<...>` streaming types — handled separately in contract generation.
fn extract_base_types(type_str: &str) -> Vec<String> {
    let cleaned = type_str.replace(' ', "");

    if cleaned.starts_with("Sse<") {
        return vec![];
    }

    // Unwrap Result<T, E> → T
    let inner = if cleaned.starts_with("Result<") {
        rorpc_parse::types::extract_first_generic_arg_string(&cleaned)
            .map(|s| s.to_string())
            .unwrap_or(cleaned.clone())
    } else {
        cleaned.clone()
    };

    // Unwrap Json<T> → T
    let inner = if inner.starts_with("Json<") && inner.ends_with('>') {
        inner[5..inner.len() - 1].to_string()
    } else {
        inner
    };

    // Helper function to normalize type names by stripping Rust module paths
    // BUT preserve the original if it contains "::" for collision detection
    let normalize_type = |s: String| -> String {
        // If it has a path separator, keep only the final component
        // "crate::types::Session" → "Session"
        // "entities::Session" → "Session"
        // But we return the original string so caller can check
        s
    };

    if inner.starts_with("Vec<") && inner.ends_with('>') {
        let elem = &inner[4..inner.len() - 1];
        if !typescript::is_primitive_type_name(elem) {
            vec![normalize_type(elem.to_string())]
        } else {
            vec![]
        }
    } else if inner.starts_with("Option<") && inner.ends_with('>') {
        let elem = &inner[7..inner.len() - 1];
        if !typescript::is_primitive_type_name(elem) {
            vec![normalize_type(elem.to_string())]
        } else {
            vec![]
        }
    } else if !typescript::is_primitive_type_name(&inner) && !inner.is_empty() && inner != "()" {
        vec![normalize_type(inner)]
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handler(name: &'static str, method: &'static str, path: &'static str) -> HandlerInfo {
        HandlerInfo {
            name,
            method,
            path,
            input_type_name: "()",
            query_type_name: None,
            output_type_name: "String",
            module_path: "test::handlers",
            error_type_name: None,
            stream_event_type_name: None,
            path_param_types: "",
        }
    }

    #[test]
    fn accepts_relative_paths_with_parent_dirs() {
        let builder =
            ContractBuilder::new(vec![make_handler("ping", "GET", "/ping")], vec![], vec![]);
        // Should not panic - relative paths with .. are allowed for legitimate use cases
        // like frontend directories outside the Rust workspace
        let temp_dir = std::env::temp_dir();
        let result = builder.output(temp_dir.join("test-output.ts"));
        // We only care that it doesn't reject the path - actual write may fail in test env
        assert!(result.is_ok() || result.unwrap_err().kind() != std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn accepts_multiple_parent_dirs_in_path() {
        // Test that paths with multiple .. components work (e.g., frontend outside workspace)
        let builder =
            ContractBuilder::new(vec![make_handler("test", "GET", "/test")], vec![], vec![]);

        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("rorpc-test-multiple-parents.ts");

        // Create a nested directory structure to test ../../.. navigation
        let nested = temp_dir.join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).ok();

        // Use relative path with multiple .. to write outside nested dir
        let relative_from_nested = "../../../rorpc-test-multiple-parents.ts";
        let full_path = nested.join(relative_from_nested);

        let result = builder.output(&full_path);

        // Verify it works (doesn't reject with InvalidInput) - use as_ref to avoid move
        let is_valid = result
            .as_ref()
            .map(|_| true)
            .unwrap_or_else(|e| e.kind() != std::io::ErrorKind::InvalidInput);
        assert!(is_valid, "Path with multiple .. should be accepted");

        // If write succeeded, verify file was created in correct location
        if result.is_ok() {
            assert!(test_file.exists(), "File should exist at resolved path");
            std::fs::remove_file(test_file).ok();
        }

        // Cleanup
        std::fs::remove_dir_all(temp_dir.join("a")).ok();
    }

    #[test]
    fn accepts_deep_parent_navigation() {
        // Test extreme case: ../../../../../../../../ (many levels up)
        let builder = ContractBuilder::new(
            vec![make_handler("extreme", "GET", "/extreme")],
            vec![],
            vec![],
        );

        let temp_dir = std::env::temp_dir();
        let test_output = temp_dir.join("rorpc-deep-navigation-test.ts");

        // Create very nested structure
        let deeply_nested = temp_dir
            .join("level1")
            .join("level2")
            .join("level3")
            .join("level4")
            .join("level5");
        std::fs::create_dir_all(&deeply_nested).ok();

        // Navigate all the way back up
        let relative = "../../../../../rorpc-deep-navigation-test.ts";
        let path_from_deep = deeply_nested.join(relative);

        let result = builder.output(&path_from_deep);

        // Use as_ref to avoid move
        let is_valid = result
            .as_ref()
            .map(|_| true)
            .unwrap_or_else(|e| e.kind() != std::io::ErrorKind::InvalidInput);
        assert!(
            is_valid,
            "Deep parent navigation (../../../../..) should be accepted"
        );

        if result.is_ok() && test_output.exists() {
            std::fs::remove_file(test_output).ok();
        }

        std::fs::remove_dir_all(temp_dir.join("level1")).ok();
    }

    #[test]
    fn generates_with_no_schemas() {
        let builder =
            ContractBuilder::new(vec![make_handler("ping", "GET", "/ping")], vec![], vec![]);
        let output = builder.generate();
        assert!(output.contains("AUTO-GENERATED"));
        assert!(output.contains("ping"));
    }

    #[test]
    fn generates_with_real_schemas() {
        let builder = ContractBuilder::new(
            vec![make_handler("list_planets", "POST", "/planet/list")],
            vec![SchemaEntry {
                type_name: "Planet",
                ts_schema_name: "PlanetSchema".to_string(),
            }],
            vec![ir::ResolvedSchema {
                ts_schema_name: "PlanetSchema".to_string(),
                ts_type_name: "Planet".to_string(),
                module_path: "",
                def: ir::ResolvedDef::Object {
                    fields: vec![ir::ResolvedField {
                        ts_key: "id".to_string(),
                        zod_expr: "z.number().int()".to_string(),
                    }],
                },
            }],
        );
        let output = builder.generate();
        assert!(output.contains("PlanetSchema"));
        assert!(output.contains("z.object"));
        assert!(output.contains("listPlanets"));
    }

    // =========================================================================
    // Collision Detection Tests
    // =========================================================================

    #[test]
    fn test_no_duplicate_when_real_schema_exists() {
        // Scenario: Session has #[derive(ZodTs)], handler uses it
        // Expected: Only one SessionSchema (the real one), no placeholder
        let handlers = vec![HandlerInfo {
            name: "get_session",
            method: "GET",
            path: "/api/sessions/{id}",
            input_type_name: "()",
            query_type_name: None,
            output_type_name: "Json<Session>",
            module_path: "test",
            error_type_name: None,
            stream_event_type_name: None,
            path_param_types: "Uuid",
        }];

        let schemas = vec![SchemaEntry {
            type_name: "Session",
            ts_schema_name: "SessionSchema".to_string(),
        }];

        let resolved = vec![ir::ResolvedSchema {
            ts_schema_name: "SessionSchema".to_string(),
            ts_type_name: "Session".to_string(),
            module_path: "",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.uuid()".to_string(),
                }],
            },
        }];

        let builder = ContractBuilder::new(handlers, schemas, resolved);
        let output = builder.generate();

        // Should have exactly one SessionSchema definition
        let session_schema_count = output.matches("export const SessionSchema").count();
        assert_eq!(
            session_schema_count, 1,
            "Should have exactly one SessionSchema definition"
        );

        // Should NOT have placeholder/TODO comment
        assert!(
            !output.contains("TODO: add #[derive(ZodTs)] to Session"),
            "Should not have placeholder comment for Session"
        );

        // Should have the real schema
        assert!(output.contains("z.uuid()"));
    }

    #[test]
    fn test_module_path_normalized() {
        // Scenario: crate::types::Session with #[derive(ZodTs)]
        // Handler uses types::Session
        // Expected: No duplicate, both resolve to same schema
        let handlers = vec![HandlerInfo {
            name: "admin_list",
            method: "GET",
            path: "/api/admin/sessions",
            input_type_name: "()",
            query_type_name: None,
            output_type_name: "Json<Vec<crate::types::Session>>",
            module_path: "test",
            error_type_name: None,
            stream_event_type_name: None,
            path_param_types: "",
        }];

        let schemas = vec![SchemaEntry {
            type_name: "Session",
            ts_schema_name: "SessionSchema".to_string(),
        }];

        let resolved = vec![ir::ResolvedSchema {
            ts_schema_name: "SessionSchema".to_string(),
            ts_type_name: "Session".to_string(),
            module_path: "",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.uuid()".to_string(),
                }],
            },
        }];

        let builder = ContractBuilder::new(handlers, schemas, resolved);
        let output = builder.generate();

        // Should have exactly one SessionSchema
        let session_schema_count = output.matches("export const SessionSchema").count();
        assert_eq!(
            session_schema_count, 1,
            "Module paths should normalize to prevent duplicates"
        );

        // Should not have placeholder
        assert!(!output.contains("TODO: add #[derive(ZodTs)] to Session"));
    }

    #[test]
    fn test_collision_warning_for_different_types() {
        // Scenario: crate::entities::Session and crate::types::Session
        // Both have NO #[derive(ZodTs)]
        // Expected: Warning about collision
        let handlers = vec![
            HandlerInfo {
                name: "get_entity_session",
                method: "GET",
                path: "/api/entities/sessions/{id}",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "Json<crate::entities::Session>",
                module_path: "test",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "Uuid",
            },
            HandlerInfo {
                name: "get_type_session",
                method: "GET",
                path: "/api/types/sessions/{id}",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "Json<crate::types::Session>",
                module_path: "test",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "Uuid",
            },
        ];

        let schemas = vec![]; // No real schemas

        let builder = ContractBuilder::new(handlers, schemas, vec![]);
        let output = builder.generate();

        // Should have collision warning
        assert!(
            output.contains("⚠️  WARNING: Multiple types resolve to 'SessionSchema'"),
            "Should warn about collision"
        );

        assert!(
            output.contains("crate::entities::Session") && output.contains("crate::types::Session"),
            "Should list both conflicting types"
        );

        // Should still generate one placeholder
        let session_schema_count = output.matches("export const SessionSchema").count();
        assert_eq!(
            session_schema_count, 1,
            "Should generate one placeholder despite collision"
        );
    }

    #[test]
    fn test_collision_resolved_by_real_schema() {
        // Scenario: crate::entities::Session (no ZodTs) and crate::types::Session (has ZodTs)
        // Expected: No collision warning, uses real schema
        let handlers = vec![
            HandlerInfo {
                name: "get_entity_session",
                method: "GET",
                path: "/api/entities/sessions/{id}",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "Json<crate::entities::Session>",
                module_path: "test",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "Uuid",
            },
            HandlerInfo {
                name: "get_type_session",
                method: "GET",
                path: "/api/types/sessions/{id}",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "Json<crate::types::Session>",
                module_path: "test",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "Uuid",
            },
        ];

        let schemas = vec![SchemaEntry {
            type_name: "Session",
            ts_schema_name: "SessionSchema".to_string(),
        }];

        let resolved = vec![ir::ResolvedSchema {
            ts_schema_name: "SessionSchema".to_string(),
            ts_type_name: "Session".to_string(),
            module_path: "",
            def: ir::ResolvedDef::Object {
                fields: vec![ir::ResolvedField {
                    ts_key: "id".to_string(),
                    zod_expr: "z.uuid()".to_string(),
                }],
            },
        }];

        let builder = ContractBuilder::new(handlers, schemas, resolved);
        let output = builder.generate();

        // Should NOT have collision warning (resolved by real schema)
        assert!(
            !output.contains("⚠️  WARNING: Multiple types resolve to 'SessionSchema'"),
            "Should not warn when real schema exists"
        );

        // Should have exactly one schema (the real one)
        let session_schema_count = output.matches("export const SessionSchema").count();
        assert_eq!(session_schema_count, 1);

        // Should be the real schema, not placeholder
        assert!(output.contains("z.uuid()"));
        assert!(!output.contains("z.unknown()"));
    }

    #[test]
    fn test_vec_and_option_extract_inner_types() {
        // Scenario: Handler returns Vec<Session> and Option<User>
        // Expected: Both Session and User extracted for placeholder check
        let handlers = vec![HandlerInfo {
            name: "test",
            method: "GET",
            path: "/test",
            input_type_name: "Json<Option<User>>",
            query_type_name: None,
            output_type_name: "Json<Vec<Session>>",
            module_path: "test",
            error_type_name: None,
            stream_event_type_name: None,
            path_param_types: "",
        }];

        let schemas = vec![]; // No real schemas

        let builder = ContractBuilder::new(handlers, schemas, vec![]);
        let output = builder.generate();

        // Should generate placeholders for both
        assert!(output.contains("SessionSchema = z.unknown()"));
        assert!(output.contains("UserSchema = z.unknown()"));
    }
}
