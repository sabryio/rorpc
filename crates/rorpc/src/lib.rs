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
pub use metadata::HandlerMetadata;
pub use registration::HandlerRegistration;
pub use schema_registry::SchemaRegistration;

// Re-export inventory so users don't need to depend on it directly
pub use inventory;

pub use rorpc_macros::{OrpcError, ZodTs};

// Re-export the attribute macros and router! proc macro
pub use rorpc_macros::contract;
pub use rorpc_macros::delete;
pub use rorpc_macros::get;
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
    let handlers: Vec<codegen::HandlerInfo> = inventory::iter::<HandlerMetadata>
        .into_iter()
        .map(|m| codegen::HandlerInfo {
            name: m.name,
            method: m.method,
            path: m.path,
            input_type_name: m.input_type_name,
            query_type_name: m.query_type_name,
            output_type_name: m.output_type_name,
            module_path: m.module_path,
            error_type_name: m.error_type_name,
            stream_event_type_name: m.stream_event_type_name,
            path_param_types: m.path_param_types,
        })
        .collect();

    // Build schema registry — prefer real schemas over z.unknown() fallbacks
    let mut registry: std::collections::HashMap<&'static str, &SchemaRegistration> =
        std::collections::HashMap::new();

    for reg in inventory::iter::<SchemaRegistration>.into_iter() {
        let is_fallback = (reg.zod_ts)().contains("z.unknown()");
        match registry.get(reg.type_name) {
            None => {
                registry.insert(reg.type_name, reg);
            }
            Some(existing) if (existing.zod_ts)().contains("z.unknown()") && !is_fallback => {
                registry.insert(reg.type_name, reg);
            }
            _ => {}
        }
    }

    // Topological sort: emit dependency schemas before the types that use them
    let mut ordered: Vec<codegen::SchemaEntry> = Vec::new();
    let mut visited: std::collections::HashSet<&'static str> = std::collections::HashSet::new();

    fn visit(
        type_name: &'static str,
        registry: &std::collections::HashMap<&'static str, &SchemaRegistration>,
        visited: &mut std::collections::HashSet<&'static str>,
        ordered: &mut Vec<codegen::SchemaEntry>,
    ) {
        if visited.contains(type_name) {
            return;
        }
        visited.insert(type_name);

        if let Some(reg) = registry.get(type_name) {
            for dep in (reg.dependent_types)() {
                visit(dep, registry, visited, ordered);
            }
            ordered.push(codegen::SchemaEntry {
                type_name: reg.type_name,
                zod_ts: (reg.zod_ts)(),
            });
        }
    }

    for type_name in registry.keys().copied() {
        visit(type_name, &registry, &mut visited, &mut ordered);
    }

    // Collect error registrations
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

    ContractBuilder::new(handlers, ordered).with_errors(errors)
}
