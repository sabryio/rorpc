//! Code generation layer — converts parsed AST into `proc_macro2::TokenStream`.
//!
//! Each sub-module corresponds to one proc macro in `rorpc-macros`.
//! `rorpc-macros/src/lib.rs` calls these functions directly; the only logic
//! that lives there is the thin `proc_macro::TokenStream` ↔ `proc_macro2::TokenStream`
//! conversion.

pub mod contract_attr;
pub mod error_derive;
pub mod namespace;
pub mod orpc;
pub mod router;
pub mod zod_ts;

// Re-export the types that orpc-macros needs at the call site
pub use contract_attr::{ContractArgs, expand_contract};
pub use error_derive::expand_orpc_errors;
pub use namespace::{NamespaceArgs, expand_namespace};
pub use orpc::{MethodShorthandArgs, OrpcArgs, expand_orpc};
pub use router::{RouterArgs, expand_router};
pub use zod_ts::derive_zod_ts;

// Re-export runtime conversion utilities for use by the orpc runtime crate
pub use zod_ts::{base_type_name, rust_type_to_ts_schema, to_schema_name};
