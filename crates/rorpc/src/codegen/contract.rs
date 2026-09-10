//! oRPC contract object generation.
//!
//! Produces the `export const contract = { ... } as const` TypeScript object
//! from collected handler metadata, grouped by path prefix (namespace).

use super::HandlerInfo;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

/// Compute a content-based hash for an error variant set.
///
/// Hash is based on the sorted list of (variant_name, data_schema) tuples,
/// making it order-independent for comparison.
fn error_variant_set_hash(variants: &[super::ErrorVariantInfo]) -> u64 {
    let mut hasher = DefaultHasher::new();

    // Sort variants by name for consistent hashing
    let mut sorted: Vec<_> = variants.iter().collect();
    sorted.sort_by_key(|v| v.name);

    for variant in sorted {
        variant.name.hash(&mut hasher);
        variant.data_schema.hash(&mut hasher);
    }

    hasher.finish()
}

/// Generate a TypeScript constant name for an error set.
///
/// Returns `"StandardApiErrors"` if this is the only error set in the project,
/// otherwise returns `"{TypeName}Errors"` based on the most common (or first
/// alphabetically) Rust error type that uses this variant set.
fn generate_error_constant_name(type_names: &[&str], is_only_set: bool) -> String {
    if is_only_set {
        "StandardApiErrors".to_string()
    } else if let Some(&type_name) = type_names.first() {
        // Remove "Error" suffix if present to avoid "AppErrorErrors"
        let base = type_name.strip_suffix("Error").unwrap_or(type_name);
        format!("{}Errors", base)
    } else {
        "StandardApiErrors".to_string()
    }
}

/// Emit TypeScript constant definitions for error variant sets.
///
/// Returns a vector of lines forming the error constants section, including
/// header comment and `as const` assertions.
fn emit_error_constants(error_sets: &[(String, &super::ErrorInfo)]) -> Vec<String> {
    if error_sets.is_empty() {
        return Vec::new();
    }

    let mut lines = vec![
        "// ============================================================================"
            .to_string(),
        "// Error Schemas".to_string(),
        "// ============================================================================"
            .to_string(),
        String::new(),
    ];

    for (const_name, error_info) in error_sets {
        let entries: Vec<String> = error_info
            .variants
            .iter()
            .map(|v| match v.data_schema {
                Some(schema) => format!("  {}: {{\n    data: {}\n  }}", v.name, schema),
                None => format!("  {}: {{}}", v.name),
            })
            .collect();

        if !entries.is_empty() {
            lines.push(format!("const {} = {{", const_name));
            lines.extend(entries.iter().map(|e| format!("{},", e)));
            lines.push("} as const;".to_string());
            lines.push(String::new());
        }
    }

    lines
}

/// Generate the `export const contract = { ... } as const` TypeScript block.
///
/// `schema_name_map` maps bare Rust type names (e.g. `"Session"`) to their
/// resolved TypeScript schema names (e.g. `"TypesSessionSchema"`). Built from
/// `ResolvedSchema` in the caller. Used to resolve handler input/output types
/// that reference disambiguated schemas.
pub fn generate_contract(
    handlers: &[HandlerInfo],
    errors: &[super::ErrorInfo],
    schemas: &[super::SchemaEntry],
    schema_name_map: &std::collections::HashMap<String, String>,
) -> String {
    let error_map: std::collections::HashMap<&str, &super::ErrorInfo> =
        errors.iter().map(|e| (e.type_name, e)).collect();

    // Pass 1: Collect unique error variant sets
    let mut error_set_map: HashMap<u64, (Vec<&str>, &super::ErrorInfo)> = HashMap::new();

    for handler in handlers {
        if let Some(error_type_name) = handler.error_type_name
            && let Some(&error_info) = error_map.get(error_type_name)
        {
            let hash = error_variant_set_hash(&error_info.variants);
            error_set_map
                .entry(hash)
                .or_insert_with(|| (Vec::new(), error_info))
                .0
                .push(error_type_name);
        }
    }

    // Pass 2: Assign constant names to each unique error set
    let is_only_set = error_set_map.len() == 1;
    let mut error_constants: Vec<(String, &super::ErrorInfo)> = error_set_map
        .values()
        .map(|(type_names, error_info)| {
            let const_name = generate_error_constant_name(type_names, is_only_set);
            (const_name, *error_info)
        })
        .collect();

    // Sort by constant name for deterministic output
    error_constants.sort_by(|a, b| a.0.cmp(&b.0));

    // Build error type name → constant name lookup map
    let error_constant_map: HashMap<&str, String> = error_set_map
        .iter()
        .flat_map(|(hash, (type_names, _))| {
            // Find the constant name for this hash
            let const_name = error_constants
                .iter()
                .find(|(_, info)| error_variant_set_hash(&info.variants) == *hash)
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| "StandardApiErrors".to_string());

            type_names.iter().map(move |&tn| (tn, const_name.clone()))
        })
        .collect();

    let mut namespaces: BTreeMap<String, Vec<&HandlerInfo>> = BTreeMap::new();

    for handler in handlers {
        let namespace = extract_namespace(handler.path);
        namespaces.entry(namespace).or_default().push(handler);
    }

    // Emit error constants section
    let mut lines = emit_error_constants(&error_constants);

    // Add separator if error constants were emitted
    if !lines.is_empty() {
        lines.push(
            "// ============================================================================"
                .to_string(),
        );
        lines.push("// API Contract".to_string());
        lines.push(
            "// ============================================================================"
                .to_string(),
        );
        lines.push(String::new());
    }

    lines.push("export const contract = {".to_string());

    for (namespace, handlers) in &namespaces {
        if namespace.is_empty() {
            for h in handlers {
                lines.push(format!(
                    "  {},",
                    generate_procedure_entry(
                        h,
                        &error_map,
                        &error_constant_map,
                        schemas,
                        schema_name_map
                    )
                ));
            }
        } else {
            lines.push(format!("  {}: {{", namespace));
            for h in handlers {
                lines.push(format!(
                    "    {},",
                    generate_procedure_entry(
                        h,
                        &error_map,
                        &error_constant_map,
                        schemas,
                        schema_name_map
                    )
                ));
            }
            lines.push("  },".to_string());
        }
    }

    lines.push("} as const;".to_string());
    lines.push(String::new());
    lines.push("export type Contract = typeof contract;".to_string());

    lines.join("\n")
}

/// Resolve a Rust type string to a TypeScript schema expression.
///
/// For custom types, checks `schema_name_map` by full path then bare name so
/// disambiguated names (e.g. `"TypesSessionSchema"`) are used when available.
/// Falls back to `rust_type_to_ts_schema` for primitives and unknown types.
fn resolve_schema(
    type_name: &str,
    schema_name_map: &std::collections::HashMap<String, String>,
) -> String {
    let cleaned = type_name.replace(' ', "");

    // Fully unwrap all wrappers to reach the inner type string,
    // tracking whether we need to wrap in z.array() at the end.
    let mut s = cleaned.as_str();
    let mut array = false;

    // Result<T, E> → T
    if s.starts_with("Result<")
        && let Some(comma) = find_first_generic_comma(s)
    {
        s = &s[7..comma];
    }

    // Json<T> → T
    if s.starts_with("Json<") && s.ends_with('>') {
        s = &s[5..s.len() - 1];
    }

    // Vec<T> → T  (with array flag)
    if s.starts_with("Vec<") && s.ends_with('>') {
        s = &s[4..s.len() - 1];
        array = true;
    }

    // Try to resolve custom type using progressively shorter path prefixes.
    // Handler metadata uses "crate::mod::Type", registry uses "crate_name::mod::Type".
    // Strip segments from the left until we find a match or reach bare name.
    let bare = s.rsplit("::").next().unwrap_or(s);

    // Build a list of candidate keys to try in order: full path, path without
    // first segment (strips "crate" or crate name), bare name.
    let without_first = s.find("::").map(|i| &s[i + 2..]);
    let resolved = schema_name_map
        .get(s)
        .or_else(|| without_first.and_then(|p| schema_name_map.get(p)))
        .or_else(|| schema_name_map.get(bare))
        .cloned();

    if let Some(ts_name) = resolved {
        if array {
            format!("z.array({})", ts_name)
        } else {
            ts_name
        }
    } else {
        // Primitive or unknown — delegate to the string-based converter
        super::typescript::rust_type_to_ts_schema(type_name)
    }
}

/// Find the comma index separating `Result<T, E>` at depth 0.
fn find_first_generic_comma(s: &str) -> Option<usize> {
    let start = s.find('<')? + 1;
    let mut depth = 0usize;
    for (i, ch) in s[start..].char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if depth == 0 => return None,
            '>' => depth -= 1,
            ',' if depth == 0 => return Some(start + i),
            _ => {}
        }
    }
    None
}

fn generate_procedure_entry(
    handler: &HandlerInfo,
    error_map: &std::collections::HashMap<&str, &super::ErrorInfo>,
    error_constant_map: &HashMap<&str, String>,
    schemas: &[super::SchemaEntry],
    schema_name_map: &std::collections::HashMap<String, String>,
) -> String {
    let key = handler_key(handler.name);
    let method = handler.method;
    let path = handler.path;

    // Use query_type_name for GET params (Query<T>), input_type_name for POST body (Json<T>)
    // Both render as .input() in the TypeScript contract
    let input_schema = {
        let type_name = if let Some(query_type) = handler.query_type_name {
            query_type
        } else {
            handler.input_type_name
        };
        let schema = resolve_schema(type_name, schema_name_map);
        if schema.is_empty() {
            // No body/query schema — but still need .input() if path params exist
            let path_only =
                merge_path_and_query_schema(path, "", handler.path_param_types, schemas);
            if path_only.is_empty() {
                String::new()
            } else {
                format!("\n      .input({})", path_only)
            }
        } else {
            // Merge path params into body/query schema if path has parameters
            let merged_schema =
                merge_path_and_query_schema(path, &schema, handler.path_param_types, schemas);
            format!("\n      .input({})", merged_schema)
        }
    };

    let output_schema = {
        let schema = if let Some(event_type) = handler.stream_event_type_name {
            // SSE streaming handler — output is an async iterator of the event type
            // Use resolve_schema to handle path normalization and schema name mapping
            let resolved = resolve_schema(event_type, schema_name_map);
            format!("asyncIteratorObject({})", resolved)
        } else {
            resolve_schema(handler.output_type_name, schema_name_map)
        };
        if schema.is_empty() {
            String::new()
        } else {
            format!("\n      .output({})", schema)
        }
    };

    let errors_block = if let Some(error_type_name) = handler.error_type_name {
        // Check if this error type has a constant defined
        if let Some(const_name) = error_constant_map.get(error_type_name) {
            // Reference the shared constant
            format!("\n      .errors({})", const_name)
        } else if let Some(error_info) = error_map.get(error_type_name) {
            // Fall back to inline generation (shouldn't happen if deduplication worked)
            let entries: Vec<String> = error_info
                .variants
                .iter()
                .map(|v| match v.data_schema {
                    Some(schema) => format!(
                        "        {}: {{\n          data: {}\n        }}",
                        v.name, schema
                    ),
                    None => format!("        {}: {{}}", v.name),
                })
                .collect();

            if !entries.is_empty() {
                format!("\n      .errors({{\n{}\n      }})", entries.join(",\n"))
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    format!(
        r#"{key}: oc
      .meta(openapi({{ method: "{method}", path: "{path}" }})){input_schema}{output_schema}{errors_block}"#
    )
}

/// Extract path parameter names from a path template.
///
/// # Examples
///
/// ```
/// # fn extract_path_params(path: &str) -> Vec<String> {
/// #     let mut params = Vec::new();
/// #     let mut chars = path.chars().peekable();
/// #     while let Some(ch) = chars.next() {
/// #         if ch == '{' {
/// #             let mut param_name = String::new();
/// #             while let Some(&next_ch) = chars.peek() {
/// #                 if next_ch == '}' {
/// #                     chars.next();
/// #                     break;
/// #                 }
/// #                 param_name.push(chars.next().unwrap());
/// #             }
/// #             if !param_name.is_empty() {
/// #                 let clean_name = param_name.trim_start_matches('+');
/// #                 params.push(clean_name.to_string());
/// #             }
/// #         }
/// #     }
/// #     params
/// # }
/// assert_eq!(extract_path_params("/planet/{id}"), vec!["id"]);
/// assert_eq!(extract_path_params("/workspace/{wsId}/project/{projId}"),
///            vec!["wsId", "projId"]);
/// assert_eq!(extract_path_params("/files/{+path}"), vec!["path"]); // Catch-all
/// assert_eq!(extract_path_params("/planet/list"), Vec::<String>::new());
/// ```
fn extract_path_params(path: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut chars = path.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut param_name = String::new();
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '}' {
                    chars.next(); // Consume '}'
                    break;
                }
                param_name.push(chars.next().unwrap());
            }
            if !param_name.is_empty() {
                // Handle catch-all syntax: {+path} → path
                let clean_name = param_name.trim_start_matches('+');
                params.push(clean_name.to_string());
            }
        }
    }

    params
}

/// Merge path parameters into query schema using Zod's .extend().
///
/// Returns the merged schema as a string. If no path parameters exist,
/// returns the original query schema unchanged.
fn merge_path_and_query_schema(
    path: &str,
    query_schema: &str,
    path_param_types: &str,
    schemas: &[super::SchemaEntry],
) -> String {
    let path_params = extract_path_params(path);

    if path_params.is_empty() {
        return query_schema.to_string();
    }

    // Decode comma-separated Rust types: "i32,String" → ["i32", "String"]
    let param_types: Vec<&str> = if path_param_types.is_empty() {
        vec![]
    } else {
        path_param_types.split(',').collect()
    };

    // Build path params object with correct Zod types
    let path_fields: Vec<String> = path_params
        .iter()
        .enumerate()
        .map(|(i, param_name)| {
            let rust_type = param_types.get(i).copied().unwrap_or("String");
            let zod_type = super::typescript::rust_type_to_ts_schema(rust_type);
            let zod_type = if zod_type.is_empty() {
                "z.string()".to_string()
            } else {
                zod_type
            };
            format!("{}: {}", param_name, zod_type)
        })
        .collect();

    let path_object = format!("z.object({{ {} }})", path_fields.join(", "));

    if query_schema.is_empty() {
        // Path params only — no query/body schema to extend
        path_object
    } else if schemas.iter().any(|s| s.ts_schema_name == query_schema) {
        // Known schema name from registry — it's a z.object() with .shape property
        // Use .extend() with .shape to merge path params with query/body schema
        format!("{}.extend({}.shape)", path_object, query_schema)
    } else {
        // Direct Zod expression (primitive, z.void(), z.record(), etc.) or unknown schema
        // These don't have .shape property, so we can't merge them
        // Return path params only as the input validation
        path_object
    }
}

/// Convert kebab-case to camelCase for valid TypeScript identifiers.
///
/// Ensures namespace names are valid unquoted TypeScript object keys by
/// converting hyphens to camelCase (e.g., `stream-async` → `streamAsync`).
fn kebab_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    
    result
}

/// Extract namespace from path for contract grouping.
///
/// Uses the second path segment when the first is "api", otherwise uses the first segment.
/// This handles the common REST pattern of `/api/resource` paths.
///
/// For paths with hyphens (e.g., `/stream-async`), extracts the base word before the
/// first hyphen to group related endpoints together (both `/stream` and `/stream-async`
/// map to the `stream` namespace).
///
/// # Examples
///
/// - `/api/sessions` → `"sessions"`
/// - `/api/campaigns/{id}` → `"campaigns"`
/// - `/api/v1/sessions` → `"v1"` (versioned APIs)
/// - `/sessions` → `"sessions"` (no api prefix)
/// - `/ping` → `""` (single segment = root namespace)
/// - `/stream` → `"stream"`
/// - `/stream-async` → `"stream"` (base word before hyphen)
fn extract_namespace(path: &str) -> String {
    let segments: Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .collect();

    let namespace = match segments.as_slice() {
        // /api/resource/... → "resource"
        ["api", resource, ..] => resource.to_string(),
        // /resource/... → "resource"
        [resource, ..] if !resource.is_empty() => resource.to_string(),
        // / or empty → ""
        _ => String::new(),
    };
    
    if namespace.is_empty() {
        return namespace;
    }
    
    // Extract base word before first hyphen to group related endpoints
    // e.g., "stream-async" → "stream"
    let base = namespace.split('-').next().unwrap_or(&namespace);
    
    // Sanitize to valid TypeScript identifier (convert any remaining hyphens to camelCase)
    kebab_to_camel(base)
}

/// `"list_planets"` → `"listPlanets"`
fn handler_key(name: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for ch in name.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_key_camel_case() {
        assert_eq!(handler_key("list_planets"), "listPlanets");
        assert_eq!(handler_key("get_profile"), "getProfile");
        assert_eq!(handler_key("ping"), "ping");
    }

    #[test]
    fn namespace_extraction_legacy_paths() {
        assert_eq!(extract_namespace("/planet/list"), "planet");
        assert_eq!(extract_namespace("/ping"), "ping");
        assert_eq!(extract_namespace("/user/profile"), "user");
    }

    #[test]
    fn namespace_extraction_api_prefix() {
        assert_eq!(extract_namespace("/api/sessions"), "sessions");
        assert_eq!(extract_namespace("/api/campaigns/{id}"), "campaigns");
        assert_eq!(extract_namespace("/api/users/profile"), "users");
    }

    #[test]
    fn namespace_extraction_versioned_api() {
        assert_eq!(extract_namespace("/api/v1/sessions"), "v1");
        assert_eq!(extract_namespace("/api/v2/users"), "v2");
    }

    #[test]
    fn namespace_extraction_without_api() {
        assert_eq!(extract_namespace("/sessions"), "sessions");
        assert_eq!(extract_namespace("/campaigns"), "campaigns");
    }

    #[test]
    fn namespace_extraction_root_handlers() {
        // Single segment paths: no sub-resource → treated as their own namespace
        assert_eq!(extract_namespace("/ping"), "ping");
        assert_eq!(extract_namespace("/health"), "health");
    }

    #[test]
    fn namespace_extraction_ignores_path_params() {
        assert_eq!(extract_namespace("/api/sessions/{id}"), "sessions");
        assert_eq!(extract_namespace("/{tenant}/sessions"), "sessions");
    }

    #[test]
    fn namespace_sanitizes_hyphens_to_camel_case() {
        // Both stream and stream-async map to "stream" namespace
        assert_eq!(extract_namespace("/stream"), "stream");
        assert_eq!(extract_namespace("/stream-async"), "stream");
        assert_eq!(extract_namespace("/api/stream-async"), "stream");
        
        // user-profile maps to "user" (base word)
        assert_eq!(extract_namespace("/api/user-profile"), "user");
        assert_eq!(extract_namespace("/user-profile"), "user");
        
        // Complex hyphens: takes first word only
        assert_eq!(extract_namespace("/my-complex-resource"), "my");
    }

    #[test]
    fn extract_single_path_param() {
        assert_eq!(extract_path_params("/planet/{id}"), vec!["id"]);
    }

    #[test]
    fn extract_multiple_path_params() {
        assert_eq!(
            extract_path_params("/workspace/{wsId}/project/{projId}"),
            vec!["wsId", "projId"]
        );
    }

    #[test]
    fn extract_catch_all_param() {
        assert_eq!(extract_path_params("/files/{+path}"), vec!["path"]);
    }

    #[test]
    fn merges_path_and_query_params() {
        use super::super::SchemaEntry;

        let schemas = vec![SchemaEntry {
            type_name: "FindPlanetQuery",
            ts_schema_name: "FindPlanetQuerySchema".to_string(),
        }];

        let merged =
            merge_path_and_query_schema("/planet/{id}", "FindPlanetQuerySchema", "i32", &schemas);

        // Should use .extend() with .shape to merge path params with query schema
        assert!(merged.contains("z.object({"));
        assert!(merged.contains("id: z.number().int()")); // i32 → z.number().int()
        assert!(merged.contains(".extend(FindPlanetQuerySchema.shape)"));
    }

    #[test]
    fn merges_path_params_with_direct_zod_expression() {
        let schemas = vec![];

        // Test with z.void() - happens when input type is ()
        // Not in schema registry, so path params should be the only input validation
        let merged_void =
            merge_path_and_query_schema("/user/{id}/ping", "z.void()", "i32", &schemas);
        assert_eq!(merged_void, "z.object({ id: z.number().int() })");

        // Test with z.record() - happens when input type is serde_json::Value
        // Not in schema registry, so path params should be the only input validation
        let merged_record = merge_path_and_query_schema(
            "/data/{id}",
            "z.record(z.string(), z.unknown())",
            "String",
            &schemas,
        );
        assert_eq!(merged_record, "z.object({ id: z.string() })");

        // Test with primitive type - happens when input is just a String or i32
        // Not in schema registry, so path params should be the only input validation
        let merged_primitive =
            merge_path_and_query_schema("/item/{id}", "z.string()", "i32", &schemas);
        assert_eq!(merged_primitive, "z.object({ id: z.number().int() })");

        // Test with unknown schema name that's not in registry
        // Should be treated as direct Zod expression and return path params only
        let merged_unknown =
            merge_path_and_query_schema("/item/{id}", "UnknownSchema", "i32", &schemas);
        assert_eq!(merged_unknown, "z.object({ id: z.number().int() })");
    }

    #[test]
    fn merges_path_params_with_registered_schema() {
        use super::super::SchemaEntry;

        // Schema IS in the registry, so it should use .extend() with .shape
        let schemas = vec![SchemaEntry {
            type_name: "UpdateUserBody",
            ts_schema_name: "UpdateUserBodySchema".to_string(),
        }];

        let merged =
            merge_path_and_query_schema("/user/{id}", "UpdateUserBodySchema", "i32", &schemas);

        assert!(merged.contains("z.object({ id: z.number().int() })"));
        assert!(merged.contains(".extend(UpdateUserBodySchema.shape)"));
    }

    #[test]
    fn contract_contains_handlers() {
        let handlers = vec![
            HandlerInfo {
                name: "list_planets",
                method: "POST",
                path: "/planet/list",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "Vec<Planet>",
                module_path: "handlers::planet",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "",
            },
            HandlerInfo {
                name: "ping",
                method: "GET",
                path: "/ping",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "String",
                module_path: "handlers",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "",
            },
        ];
        let output = generate_contract(&handlers, &[], &[], &HashMap::new());
        assert!(output.contains("listPlanets"));
        assert!(output.contains("ping"));
        assert!(output.contains("/planet/list"));
        assert!(output.contains("as const"));
    }

    #[test]
    fn contract_namespaces_by_resource() {
        // Bug-03 scenario: handlers with same names in different resource modules
        let handlers = vec![
            HandlerInfo {
                name: "list",
                method: "GET",
                path: "/api/sessions",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "Vec<Session>",
                module_path: "sessions",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "",
            },
            HandlerInfo {
                name: "create",
                method: "POST",
                path: "/api/sessions",
                input_type_name: "CreateSessionInput",
                query_type_name: None,
                output_type_name: "Session",
                module_path: "sessions",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "",
            },
            HandlerInfo {
                name: "list",
                method: "GET",
                path: "/api/campaigns",
                input_type_name: "()",
                query_type_name: None,
                output_type_name: "Vec<Campaign>",
                module_path: "campaigns",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "",
            },
            HandlerInfo {
                name: "create",
                method: "POST",
                path: "/api/campaigns",
                input_type_name: "CreateCampaignInput",
                query_type_name: None,
                output_type_name: "Campaign",
                module_path: "campaigns",
                error_type_name: None,
                stream_event_type_name: None,
                path_param_types: "",
            },
        ];
        let output = generate_contract(&handlers, &[], &[], &HashMap::new());

        // Should create separate namespaces
        assert!(output.contains("sessions: {"));
        assert!(output.contains("campaigns: {"));

        // Should NOT have duplicate keys in flat api namespace
        assert!(!output.contains("api: {"));

        // Both resources should have their own list/create handlers
        let sessions_idx = output.find("sessions: {").unwrap();
        let campaigns_idx = output.find("campaigns: {").unwrap();

        // BTreeMap sorts alphabetically: campaigns < sessions
        assert!(campaigns_idx < sessions_idx);

        // Verify both have list/create
        assert!(output.contains("list:"));
        assert!(output.contains("create:"));
    }

    #[test]
    fn error_deduplication_single_error_set() {
        use super::super::{ErrorInfo, ErrorVariantInfo};

        let error_info = ErrorInfo {
            type_name: "AppError",
            variants: vec![
                ErrorVariantInfo {
                    name: "NOT_FOUND",
                    data_schema: Some("z.string()"),
                },
                ErrorVariantInfo {
                    name: "UNAUTHORIZED",
                    data_schema: None,
                },
            ],
        };

        let handlers = vec![
            HandlerInfo {
                name: "create_session",
                method: "POST",
                path: "/api/sessions",
                input_type_name: "CreateSessionInput",
                query_type_name: None,
                output_type_name: "Session",
                module_path: "sessions",
                error_type_name: Some("AppError"),
                stream_event_type_name: None,
                path_param_types: "",
            },
            HandlerInfo {
                name: "update_session",
                method: "PATCH",
                path: "/api/sessions/{id}",
                input_type_name: "UpdateSessionInput",
                query_type_name: None,
                output_type_name: "Session",
                module_path: "sessions",
                error_type_name: Some("AppError"),
                stream_event_type_name: None,
                path_param_types: "String",
            },
        ];

        let output = generate_contract(&handlers, &[error_info], &[], &HashMap::new());

        // Should have error constant section
        assert!(output.contains("// Error Schemas"));
        assert!(output.contains("const StandardApiErrors = {"));
        assert!(output.contains("NOT_FOUND: {"));
        assert!(output.contains("data: z.string()"));
        assert!(output.contains("UNAUTHORIZED: {}"));
        assert!(output.contains("} as const;"));

        // Should reference constant instead of inline errors
        assert!(output.contains(".errors(StandardApiErrors)"));

        // Should NOT contain inline error definitions
        assert!(!output.contains(".errors({\n"));
    }

    #[test]
    fn error_deduplication_multiple_error_sets() {
        use super::super::{ErrorInfo, ErrorVariantInfo};

        let app_error = ErrorInfo {
            type_name: "AppError",
            variants: vec![
                ErrorVariantInfo {
                    name: "NOT_FOUND",
                    data_schema: Some("z.string()"),
                },
                ErrorVariantInfo {
                    name: "UNAUTHORIZED",
                    data_schema: None,
                },
            ],
        };

        let auth_error = ErrorInfo {
            type_name: "AuthError",
            variants: vec![
                ErrorVariantInfo {
                    name: "INVALID_TOKEN",
                    data_schema: Some("z.string()"),
                },
                ErrorVariantInfo {
                    name: "EXPIRED_SESSION",
                    data_schema: None,
                },
            ],
        };

        let handlers = vec![
            HandlerInfo {
                name: "create_session",
                method: "POST",
                path: "/api/sessions",
                input_type_name: "CreateSessionInput",
                query_type_name: None,
                output_type_name: "Session",
                module_path: "sessions",
                error_type_name: Some("AppError"),
                stream_event_type_name: None,
                path_param_types: "",
            },
            HandlerInfo {
                name: "login",
                method: "POST",
                path: "/api/auth/login",
                input_type_name: "LoginInput",
                query_type_name: None,
                output_type_name: "Session",
                module_path: "auth",
                error_type_name: Some("AuthError"),
                stream_event_type_name: None,
                path_param_types: "",
            },
        ];

        let output = generate_contract(&handlers, &[app_error, auth_error], &[], &HashMap::new());

        // Should have two error constants
        assert!(output.contains("const AppErrors = {"));
        assert!(output.contains("const AuthErrors = {"));

        // Should reference the correct constants
        assert!(output.contains(".errors(AppErrors)"));
        assert!(output.contains(".errors(AuthErrors)"));

        // Should NOT use StandardApiErrors when there are multiple sets
        assert!(!output.contains("StandardApiErrors"));
    }
}
