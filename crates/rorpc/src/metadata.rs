//! Handler metadata collected at link time via the `inventory` crate.
//!
//! Every `#[orpc(method, path)]`-annotated handler emits an `inventory::submit!`
//! call that registers a `HandlerMetadata` entry. `inventory::iter::<HandlerMetadata>`
//! then provides access to all registered entries from any file in the binary.

/// Metadata registered by the `#[orpc(method, path)]` attribute macro.
///
/// One instance is created per annotated handler function and registered
/// globally at link time — no central list required.
pub struct HandlerMetadata {
    /// Handler function name (e.g. `"list_planets"`)
    pub name: &'static str,
    /// HTTP method in uppercase (e.g. `"GET"`, `"POST"`)
    pub method: &'static str,
    /// HTTP path (e.g. `"/planet/list"`)
    pub path: &'static str,
    /// Fully qualified Rust type name of the input type (from Json<T>)
    pub input_type_name: &'static str,
    /// Fully qualified Rust type name of the query type (from Query<T>)
    pub query_type_name: Option<&'static str>,
    /// Fully qualified Rust type name of the output type
    pub output_type_name: &'static str,
    /// Rust module path of the handler (e.g. `"axum_native::handlers::planet"`)
    pub module_path: &'static str,
    /// Namespace prefix applied to this handler (if any, e.g. `"/planet"`)
    pub namespace: Option<&'static str>,
    /// Error type name extracted from `Result<T, E>` return type (if present)
    pub error_type_name: Option<&'static str>,
    /// Stream event type name for SSE endpoints (if specified with `data` attribute)
    pub stream_event_type_name: Option<&'static str>,
    /// Ordered comma-separated Rust type names for `Path<T>` parameters.
    ///
    /// E.g., `Path(id): Path<i32>` on `/planet/{id}` → `"i32"`
    /// Multiple params on `/ws/{wsId}/planet/{id}` → `"i32,i32"`
    /// Empty string when no `Path<T>` extractors are present.
    pub path_param_types: &'static str,
}

inventory::collect!(HandlerMetadata);

/// Namespace metadata registered by the `#[rorpc::namespace("/prefix")]` attribute macro.
///
/// Applied to modules to automatically prefix all handler routes within that module.
/// One instance is created per annotated module and registered globally at link time.
pub struct NamespaceMetadata {
    /// Rust module path where the namespace was declared (e.g. `"crate::handlers::planet"`)
    pub module_path: &'static str,
    /// HTTP path prefix to prepend to all handlers in this module (e.g. `"/planet"`)
    pub prefix: &'static str,
}

inventory::collect!(NamespaceMetadata);
