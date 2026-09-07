//! Better Auth + orpc Integration Example
//!
//! Demonstrates plain Axum handlers with `#[orpc]` annotations alongside
//! Better Auth session management. No orpc-axum wrapper needed.
//!
//! ## Features
//! - `better-auth-integration`: Enables Better Auth session management (disabled by default)
//!
//! To enable Better Auth, run with:
//! ```bash
//! cargo run --features better-auth-integration
//! ```

mod application;
mod domain;
mod infrastructure;
mod server;

#[cfg(feature = "better-auth-integration")]
use better_auth::{
    plugins::{EmailPasswordPlugin, SessionManagementPlugin},
    seaorm::SeaOrmStore,
    AuthConfig, BetterAuth,
};
use infrastructure::{
    context::AppState,
    repositories::in_memory_planet_repo::{sample_planets, InMemoryPlanetRepository},
};
use std::sync::Arc;

#[cfg(feature = "better-auth-integration")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting Better Auth + orpc integration example...");

    // Generate TypeScript contract before starting the server
    #[cfg(debug_assertions)]
    {
        println!("📝 Generating TypeScript contract...");
        rorpc::generate_contract()
            .output(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../client/src/rpc/bindings.ts"
            ))
            .expect("contract generation failed");
        println!("✅ Generated examples/axum-react/client/src/rpc/bindings.ts");
    }

    // 1. Database & auth setup
    let database = infrastructure::db::seaorm::connect("sqlite::memory:").await?;
    infrastructure::auth::schema::run_app_migrations(&database).await?;

    let secret = "your-very-secure-secret-key-at-least-32-chars-long-for-production-use-only";
    let config = AuthConfig::new(secret)
        .base_url("http://localhost:3000")
        .password_min_length(8);

    let store = SeaOrmStore::<infrastructure::auth::schema::AppAuthSchema>::new(config.clone(), database);

    let auth = Arc::new(
        BetterAuth::<infrastructure::auth::schema::AppAuthSchema>::new(config)
            .store(store)
            .plugin(EmailPasswordPlugin::new().enable_signup(true))
            .plugin(SessionManagementPlugin::new())
            .build()
            .await?,
    );

    // 2. Build shared state — repo + auth in one place
    let state = AppState {
        planet_repo: Arc::new(InMemoryPlanetRepository::new(sample_planets())),
        auth: auth.clone(),
    };

    // 3. Start server
    server::axum::run_server(state, auth).await
}

#[cfg(not(feature = "better-auth-integration"))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting orpc example (without Better Auth integration)...");
    println!("💡 To enable Better Auth, run with: cargo run --features better-auth-integration");

    // Generate TypeScript contract before starting the server
    #[cfg(debug_assertions)]
    {
        println!("📝 Generating TypeScript contract...");
        rorpc::generate_contract()
            .output(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../client/src/rpc/bindings.ts"
            ))
            .expect("contract generation failed");
        println!("✅ Generated examples/axum-react/client/src/rpc/bindings.ts");
    }

    // Build shared state — just the repository
    let state = AppState {
        planet_repo: Arc::new(InMemoryPlanetRepository::new(sample_planets())),
    };

    // Start server without auth
    server::axum::run_server(state).await
}
