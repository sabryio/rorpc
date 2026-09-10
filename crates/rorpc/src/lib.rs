//! # rorpc
//!
//! Unified facade for handler metadata collection, auto-router construction,
//! and TypeScript contract generation.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use axum::{extract::State, Json};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Planet { id: i32, name: String }
//!
//! #[rorpc::get("/planet/list")]
//! async fn list_planets(State(db): State<Db>) -> Json<Vec<Planet>> {
//!     Json(db.list().await)
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let app = rorpc::router!(db);
//!
//!     rorpc::generate_contract()
//!         .output("../client/src/rpc/index.ts")
//!         .unwrap();
//!
//!     axum::serve(listener, app).await.unwrap();
//! }
//! ```

pub mod codegen;
pub mod error_registry;
pub mod metadata;
pub mod registration;
pub mod schema_registry;

pub use codegen::ContractBuilder;
pub use error_registry::{ErrorRegistration, ErrorVariant};
pub use metadata::{HandlerMetadata, NamespaceMetadata};
pub use registration::HandlerRegistration;
pub use schema_registry::{
    EnumRepr, FieldDef, SchemaDef, SchemaRegistration, VariantDef, VariantKind,
};

// Re-export inventory so users don't need to depend on it directly
pub use inventory;

pub use rorpc_macros::{OrpcError, ZodTs};

// Re-export the attribute macros and router! proc macro
pub use rorpc_macros::contract;
pub use rorpc_macros::delete;
pub use rorpc_macros::get;
pub use rorpc_macros::namespace;
pub use rorpc_macros::patch;
pub use rorpc_macros::post;
pub use rorpc_macros::put;
pub use rorpc_macros::route;
pub use rorpc_macros::router;

/// Begin building a TypeScript contract from all discovered handlers.
///
/// # Example
///
/// ```no_run
/// rorpc::generate_contract()
///     .output("../client/src/rpc/index.ts")
///     .unwrap();
/// ```
pub fn generate_contract() -> ContractBuilder {
    // Build namespace lookup map: module_path -> prefix
    let mut namespace_map: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    for ns in inventory::iter::<NamespaceMetadata> {
        namespace_map.insert(ns.module_path, ns.prefix);
    }

    let handlers: Vec<codegen::HandlerInfo> = inventory::iter::<HandlerMetadata>
        .into_iter()
        .map(|m| {
            // Look up namespace for this handler's module or any parent module
            let mut final_path = m.path.to_string();
            let module_parts: Vec<&str> = m.module_path.split("::").collect();
            for i in (0..=module_parts.len()).rev() {
                let parent_path = module_parts[..i].join("::");
                if let Some(prefix) = namespace_map.get(parent_path.as_str()) {
                    // Compose namespace + handler path
                    final_path = format!("{}{}", prefix, m.path);
                    break;
                }
            }

            codegen::HandlerInfo {
                name: m.name,
                method: m.method,
                path: Box::leak(final_path.into_boxed_str()),
                input_type_name: m.input_type_name,
                query_type_name: m.query_type_name,
                output_type_name: m.output_type_name,
                module_path: m.module_path,
                error_type_name: m.error_type_name,
                stream_event_type_name: m.stream_event_type_name,
                path_param_types: m.path_param_types,
            }
        })
        .collect();

    // Helper: convert module path to PascalCase TypeScript name
    // "issue1_duplicate_schemas::entities::Session" → "EntitiesSession"
    // "issue1_duplicate_schemas::types::Session" → "TypesSession"
    fn module_path_to_ts_name(module_path: &str, type_name: &str) -> String {
        let parts: Vec<&str> = module_path.split("::").collect();

        // Strip the crate root (first segment) and the type name (last segment)
        // Everything in between becomes the disambiguating prefix
        let middle: Vec<&str> = parts
            .iter()
            .skip(1) // skip crate root (e.g. "issue1_duplicate_schemas")
            .copied()
            .filter(|p| *p != type_name) // skip the type name itself
            .collect();

        if middle.is_empty() {
            // No distinguishing module segments — fall back to full crate name prefix
            let crate_root = parts.first().copied().unwrap_or("");
            let pascal_crate = crate_root
                .split('_')
                .map(|word| {
                    let mut c = word.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    }
                })
                .collect::<String>();
            return format!("{}{}", pascal_crate, type_name);
        }

        // Convert each middle segment to PascalCase: "entities" → "Entities"
        let mut name_parts: Vec<String> = middle
            .iter()
            .map(|p| {
                let mut c = p.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect();

        name_parts.push(type_name.to_string());
        name_parts.join("")
    }

    // Build schema registry and track ALL registrations (including collisions)
    let mut registry: std::collections::HashMap<&'static str, Vec<&SchemaRegistration>> =
        std::collections::HashMap::new();

    for reg in inventory::iter::<SchemaRegistration>.into_iter() {
        // Normalize type_name: strip module paths so "crate::types::Session" → "Session"
        // This groups handler-emitted fallbacks together with real ZodTs registrations
        let normalized = reg.type_name.rsplit("::").next().unwrap_or(reg.type_name);
        registry.entry(normalized).or_default().push(reg);
    }

    // Process registrations: prefer real schemas, warn on collisions
    let mut final_registry: std::collections::HashMap<
        &'static str,
        (&'static SchemaRegistration, String),
    > = std::collections::HashMap::new();
    let mut type_to_keys: std::collections::HashMap<&'static str, Vec<&'static str>> =
        std::collections::HashMap::new();

    for (type_name, regs) in registry {
        let real_schemas: Vec<_> = regs
            .iter()
            .filter(|r| !matches!(r.schema_def, SchemaDef::Unknown))
            .copied()
            .collect();

        if real_schemas.is_empty() {
            if let Some(reg) = regs.first() {
                let ts_name = format!("{}Schema", type_name);
                final_registry.insert(type_name, (reg, ts_name));
                type_to_keys.entry(type_name).or_default().push(type_name);
            }
        } else if real_schemas.len() == 1 {
            let reg = real_schemas[0];
            let ts_name = format!("{}Schema", type_name);
            final_registry.insert(type_name, (reg, ts_name));
            type_to_keys.entry(type_name).or_default().push(type_name);
        } else {
            eprintln!(
                "⚠️  WARNING: Schema name collision detected for '{}'",
                type_name
            );
            eprintln!("   Multiple types with #[derive(ZodTs)] have the same name:");
            for reg in &real_schemas {
                let disambiguated = module_path_to_ts_name(reg.module_path, type_name);
                let ts_name = format!("{}Schema", disambiguated);
                eprintln!("     {} → {}", reg.module_path, ts_name);
                final_registry.insert(reg.module_path, (reg, ts_name));
                type_to_keys
                    .entry(type_name)
                    .or_default()
                    .push(reg.module_path);
            }
            eprintln!();
        }
    }

    // Topological sort: emit dependency schemas before the types that use them
    let mut ordered: Vec<codegen::SchemaEntry> = Vec::new();
    let mut registered: Vec<(String, &'static SchemaRegistration)> = Vec::new();
    let mut visited: std::collections::HashSet<&'static str> = std::collections::HashSet::new();

    fn visit(
        type_name: &'static str,
        final_registry: &std::collections::HashMap<
            &'static str,
            (&'static SchemaRegistration, String),
        >,
        type_to_keys: &std::collections::HashMap<&'static str, Vec<&'static str>>,
        visited: &mut std::collections::HashSet<&'static str>,
        ordered: &mut Vec<codegen::SchemaEntry>,
        registered: &mut Vec<(String, &'static SchemaRegistration)>,
    ) {
        if visited.contains(type_name) {
            return;
        }
        visited.insert(type_name);

        // Get all keys for this type (could be multiple if collision)
        let keys = type_to_keys
            .get(type_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        for key in keys {
            if let Some((reg, ts_schema_name)) = final_registry.get(key) {
                // Visit dependencies first
                for dep in (reg.dependent_types)() {
                    // Normalize dependency: strip to bare name for lookup
                    // Handles both full paths ("crate::types::Session") and bare names ("Session")
                    let normalized = dep.rsplit("::").next().unwrap_or(dep);
                    visit(
                        normalized,
                        final_registry,
                        type_to_keys,
                        visited,
                        ordered,
                        registered,
                    );
                }
                ordered.push(codegen::SchemaEntry {
                    type_name: reg.type_name,
                    ts_schema_name: ts_schema_name.clone(),
                });
                registered.push((ts_schema_name.clone(), reg));
            }
        }
    }

    // Seed from all type names (not keys) to ensure correct dep ordering
    let all_type_names: Vec<&'static str> = type_to_keys.keys().copied().collect();
    for type_name in all_type_names {
        visit(
            type_name,
            &final_registry,
            &type_to_keys,
            &mut visited,
            &mut ordered,
            &mut registered,
        );
    } // Collect error registrations
    let errors: Vec<codegen::ErrorInfo> = inventory::iter::<ErrorRegistration>
        .into_iter()
        .map(|e| codegen::ErrorInfo {
            type_name: e.type_name,
            variants: e
                .variants
                .iter()
                .map(|v| codegen::ErrorVariantInfo {
                    name: v.name,
                    data_schema: v.data_schema,
                })
                .collect(),
        })
        .collect();

    // -----------------------------------------------------------------------
    // Resolution pass: SchemaDef → Vec<ResolvedSchema>
    // -----------------------------------------------------------------------
    //
    // Build a lookup map: bare type name → all (ts_schema_name, module_path)
    // candidates. Used by `resolve_type_ref` to pick the right disambiguated
    // name for each field's type_ref.
    let mut candidates_by_name: std::collections::HashMap<
        &'static str,
        Vec<(String, &'static str)>,
    > = std::collections::HashMap::new();
    for (ts_schema_name, reg) in &registered {
        candidates_by_name
            .entry(reg.type_name)
            .or_default()
            .push((ts_schema_name.clone(), reg.module_path));
    }

    /// Pick the best `ts_schema_name` for `type_ref` given the module path of
    /// the schema that contains the reference.
    ///
    /// Strategy:
    /// 1. Exactly one candidate → unambiguous, use it directly.
    /// 2. Multiple candidates → prefer the one whose `module_path` shares the
    ///    most leading path segments with `referencing_module`.
    /// 3. Still tied → pick the first alphabetically (deterministic).
    fn resolve_type_ref(
        type_ref: &str,
        referencing_module: &str,
        candidates_by_name: &std::collections::HashMap<&'static str, Vec<(String, &'static str)>>,
    ) -> Option<String> {
        let candidates = candidates_by_name.get(type_ref)?;

        if candidates.len() == 1 {
            return Some(candidates[0].0.clone());
        }

        // Count shared leading segments between two module paths
        fn shared_prefix_len(a: &str, b: &str) -> usize {
            a.split("::")
                .zip(b.split("::"))
                .take_while(|(x, y)| x == y)
                .count()
        }

        candidates
            .iter()
            .max_by_key(|(_, mod_path)| {
                (
                    shared_prefix_len(referencing_module, mod_path),
                    mod_path.to_string(),
                )
            })
            .map(|(ts_name, _)| ts_name.clone())
    }

    /// Resolve a single `FieldDef` into a `ResolvedField`.
    fn resolve_field(
        field: &crate::schema_registry::FieldDef,
        referencing_module: &str,
        candidates_by_name: &std::collections::HashMap<&'static str, Vec<(String, &'static str)>>,
    ) -> codegen::ir::ResolvedField {
        let zod_expr = if field.type_ref.is_empty() {
            // Primitive — already a complete expression
            field.zod_expr.to_string()
        } else {
            // Custom type — resolve to ts_schema_name, apply optional/nullable wrapper
            let base = resolve_type_ref(field.type_ref, referencing_module, candidates_by_name)
                .unwrap_or_else(|| format!("{}Schema", field.type_ref));
            if field.optional {
                if field.skip_if_none {
                    format!("{}.optional()", base)
                } else {
                    format!("{}.nullable()", base)
                }
            } else {
                base
            }
        };
        codegen::ir::ResolvedField {
            ts_key: field.ts_name.to_string(),
            zod_expr,
        }
    }

    /// Resolve a single `VariantDef` into a `ResolvedVariant`.
    fn resolve_variant(
        variant: &crate::schema_registry::VariantDef,
        referencing_module: &str,
        candidates_by_name: &std::collections::HashMap<&'static str, Vec<(String, &'static str)>>,
    ) -> codegen::ir::ResolvedVariant {
        use crate::schema_registry::VariantKind;
        let kind = match variant.kind {
            VariantKind::Unit => codegen::ir::ResolvedVariantKind::Unit,
            VariantKind::NewtypeZod { zod_expr } => codegen::ir::ResolvedVariantKind::Newtype {
                zod_expr: zod_expr.to_string(),
            },
            VariantKind::NewtypeRef { type_ref } => {
                let resolved = resolve_type_ref(type_ref, referencing_module, candidates_by_name)
                    .unwrap_or_else(|| format!("{}Schema", type_ref));
                codegen::ir::ResolvedVariantKind::Newtype { zod_expr: resolved }
            }
            VariantKind::Struct { fields } => {
                let resolved_fields = fields
                    .iter()
                    .map(|f| resolve_field(f, referencing_module, candidates_by_name))
                    .collect();
                codegen::ir::ResolvedVariantKind::Struct {
                    fields: resolved_fields,
                }
            }
        };
        codegen::ir::ResolvedVariant {
            serialized_name: variant.serialized_name.to_string(),
            kind,
        }
    }

    let resolved_schemas: Vec<codegen::ir::ResolvedSchema> = registered
        .iter()
        .filter_map(|(ts_schema_name, reg)| {
            let ts_type_name = ts_schema_name
                .strip_suffix("Schema")
                .unwrap_or(ts_schema_name)
                .to_string();

            let def = match &reg.schema_def {
                SchemaDef::Unknown => return None, // skip fallbacks
                SchemaDef::Object { fields } => {
                    let resolved_fields = fields
                        .iter()
                        .map(|f| resolve_field(f, reg.module_path, &candidates_by_name))
                        .collect();
                    codegen::ir::ResolvedDef::Object {
                        fields: resolved_fields,
                    }
                }
                SchemaDef::Enum { repr, variants } => {
                    // Convert EnumRepr → ResolvedEnumRepr
                    let resolved_repr = match repr {
                        schema_registry::EnumRepr::External => {
                            codegen::ir::ResolvedEnumRepr::External
                        }
                        schema_registry::EnumRepr::Internal { tag } => {
                            codegen::ir::ResolvedEnumRepr::Internal {
                                tag: tag.to_string(),
                            }
                        }
                        schema_registry::EnumRepr::Adjacent { tag, content } => {
                            codegen::ir::ResolvedEnumRepr::Adjacent {
                                tag: tag.to_string(),
                                content: content.to_string(),
                            }
                        }
                        schema_registry::EnumRepr::Untagged => {
                            codegen::ir::ResolvedEnumRepr::Untagged
                        }
                    };

                    let resolved_variants = variants
                        .iter()
                        .map(|v| resolve_variant(v, reg.module_path, &candidates_by_name))
                        .collect();
                    codegen::ir::ResolvedDef::Enum {
                        repr: resolved_repr,
                        variants: resolved_variants,
                    }
                }
            };

            Some(codegen::ir::ResolvedSchema {
                ts_schema_name: ts_schema_name.clone(),
                ts_type_name,
                module_path: reg.module_path,
                def,
            })
        })
        .collect();

    ContractBuilder::new(handlers, ordered, resolved_schemas).with_errors(errors)
}
