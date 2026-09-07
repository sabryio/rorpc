//! oRPC contract object generation.
//!
//! Produces the `export const contract = { ... } as const` TypeScript object
//! from collected handler metadata, grouped by path prefix (namespace).

use super::HandlerInfo;
use std::collections::BTreeMap;

/// Generate the `export const contract = { ... } as const` TypeScript block.
pub fn generate_contract(
    handlers: &[HandlerInfo],
    errors: &[super::ErrorInfo],
    schemas: &[super::SchemaEntry],
) -> String {
    let error_map: std::collections::HashMap<&str, &super::ErrorInfo> =
        errors.iter().map(|e| (e.type_name, e)).collect();

    let mut namespaces: BTreeMap<String, Vec<&HandlerInfo>> = BTreeMap::new();

    for handler in handlers {
        let namespace = extract_namespace(handler.path);
        namespaces.entry(namespace).or_default().push(handler);
    }

    let mut lines = vec!["export const contract = {".to_string()];

    for (namespace, handlers) in &namespaces {
        if namespace.is_empty() {
            for h in handlers {
                lines.push(format!(
                    "  {},",
                    generate_procedure_entry(h, &error_map, schemas)
                ));
            }
        } else {
            lines.push(format!("  {}: {{", namespace));
            for h in handlers {
                lines.push(format!(
                    "    {},",
                    generate_procedure_entry(h, &error_map, schemas)
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

fn generate_procedure_entry(
    handler: &HandlerInfo,
    error_map: &std::collections::HashMap<&str, &super::ErrorInfo>,
    schemas: &[super::SchemaEntry],
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
        let schema = super::typescript::rust_type_to_ts_schema(type_name);
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
            format!("asyncIteratorObject({}Schema)", event_type)
        } else {
            super::typescript::rust_type_to_ts_schema(handler.output_type_name)
        };
        if schema.is_empty() {
            String::new()
        } else {
            format!("\n      .output({})", schema)
        }
    };

    let errors_block = if let Some(error_type_name) = handler.error_type_name {
        if let Some(error_info) = error_map.get(error_type_name) {
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
    _schemas: &[super::SchemaEntry],
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
    } else {
        // Use Zod's .extend() with .shape to merge path params with query schema
        format!("{}.extend({}.shape)", path_object, query_schema)
    }
}

/// `"/planet/list"` → `"planet"`, `"/ping"` → `""`
fn extract_namespace(path: &str) -> String {
    let segments: Vec<&str> = path.trim_start_matches('/').splitn(3, '/').collect();
    if segments.len() >= 2 {
        segments[0].to_string()
    } else {
        String::new()
    }
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
    fn namespace_extraction() {
        assert_eq!(extract_namespace("/planet/list"), "planet");
        assert_eq!(extract_namespace("/ping"), "");
        assert_eq!(extract_namespace("/user/profile"), "user");
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
    fn no_path_params() {
        assert_eq!(extract_path_params("/planet/list"), Vec::<String>::new());
    }

    #[test]
    fn merges_path_and_query_params() {
        use super::super::SchemaEntry;

        let schemas = vec![SchemaEntry {
            type_name: "FindPlanetQuery",
            zod_ts: "export const FindPlanetQuerySchema = z.object({ id: z.number().int(), q: z.string().optional() });".to_string(),
        }];

        let merged =
            merge_path_and_query_schema("/planet/{id}", "FindPlanetQuerySchema", "i32", &schemas);

        // Should use .extend() with .shape to merge path params with query schema
        assert!(merged.contains("z.object({"));
        assert!(merged.contains("id: z.number().int()")); // i32 → z.number().int()
        assert!(merged.contains(".extend(FindPlanetQuerySchema.shape)"));
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
        let output = generate_contract(&handlers, &[], &[]);
        assert!(output.contains("listPlanets"));
        assert!(output.contains("ping"));
        assert!(output.contains("/planet/list"));
        assert!(output.contains("as const"));
    }
}
