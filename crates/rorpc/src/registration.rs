//! Handler registration — pairs route metadata with a type-erased Axum handler.
//!
//! The `#[orpc]` macro emits two `inventory::submit!` calls per handler:
//! 1. `HandlerMetadata` — for codegen (TypeScript contract)
//! 2. `HandlerRegistration` — for runtime (Axum router construction)
//!
//! ## State erasure
//!
//! Handlers that extract `State<T>` produce `MethodRouter<T>` which can't be
//! stored in a `static` alongside handlers with different state types.
//!
//! Solution: the factory receives the concrete state value as `Box<dyn Any + Send + Sync>`
//! and downcasts it internally, returning a fully state-erased `Router<()>`.
//! Each handler's factory knows its own state type `S` at macro expansion time.

use axum::Router;
use std::any::Any;
use std::sync::Arc;

/// Factory that builds a single-route `Router<()>` given the app state and path.
///
/// Receives state as type-erased `Arc<dyn Any + Send + Sync>` and the final
/// route path (which may have a namespace prefix applied).
/// The macro emits a concrete factory that downcasts to the known `S` type.
pub type RouteFactory = fn(state: Arc<dyn Any + Send + Sync>, path: &str) -> Router;

/// Pairs a route path + method with its type-erased Axum route factory.
///
/// Registered globally by the `#[orpc]` macro via `inventory::submit!`.
pub struct HandlerRegistration {
    /// The HTTP path without namespace (e.g. `"/list"`)
    pub path: &'static str,
    /// The HTTP method in uppercase (e.g. `"POST"`)
    pub method: &'static str,
    /// Factory that builds a single-route `Router<()>` with state and path applied
    pub factory: RouteFactory,
}

inventory::collect!(HandlerRegistration);

/// Build an Axum `Router<()>` from all registered handlers.
///
/// `state` is passed type-erased; each handler's factory downcasts it to its
/// own expected state type.
///
/// # Example
///
/// ```no_run
/// let app = rorpc::registration::build_router(std::sync::Arc::new(()));
/// ```
pub fn build_router(state: Arc<dyn Any + Send + Sync>) -> Router {
    let mut app: Router = Router::new();

    for reg in inventory::iter::<HandlerRegistration>.into_iter() {
        let route = (reg.factory)(Arc::clone(&state), reg.path);
        app = app.merge(route);
    }

    app
}
