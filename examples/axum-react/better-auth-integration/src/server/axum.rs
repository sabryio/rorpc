#[cfg(feature = "better-auth-integration")]
use crate::infrastructure::auth::schema::AppAuthSchema;
use crate::infrastructure::context::AppState;
use axum::Router;
#[cfg(feature = "better-auth-integration")]
use better_auth::{integrations::axum::AxumIntegration, BetterAuth};
#[cfg(feature = "better-auth-integration")]
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[cfg(feature = "better-auth-integration")]
pub async fn run_server(
    state: AppState,
    auth: Arc<BetterAuth<AppAuthSchema>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Nest both routers (both already have their state applied)
    let app = Router::new()
        .nest("/rpc", rorpc::router!(state))
        .nest("/api/auth", auth.clone().axum_router().with_state(auth))
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:5173"
                        .parse::<axum::http::HeaderValue>()
                        .unwrap(),
                    "http://localhost:3000"
                        .parse::<axum::http::HeaderValue>()
                        .unwrap(),
                ])
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::ACCEPT,
                ])
                .allow_credentials(true),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await?;

    println!("🚀 Server running on http://127.0.0.1:3001");
    println!("📚 Better Auth Endpoints (under /api/auth)");
    println!("🔧 orpc Endpoints (under /rpc)");

    axum::serve(listener, app)
        .with_graceful_shutdown(crate::server::shutdown::shutdown_signal())
        .await?;

    println!("\n✨ Server shutdown complete");
    Ok(())
}

#[cfg(not(feature = "better-auth-integration"))]
pub async fn run_server(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    // Simplified router without Better Auth
    let app = Router::new()
        .nest("/rpc", rorpc::router!(state))
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:5173"
                        .parse::<axum::http::HeaderValue>()
                        .unwrap(),
                    "http://localhost:3000"
                        .parse::<axum::http::HeaderValue>()
                        .unwrap(),
                ])
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::ACCEPT,
                ])
                .allow_credentials(true),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await?;

    println!("🚀 Server running on http://127.0.0.1:3001");
    println!("🔧 orpc Endpoints (under /rpc)");

    axum::serve(listener, app)
        .with_graceful_shutdown(crate::server::shutdown::shutdown_signal())
        .await?;

    println!("\n✨ Server shutdown complete");
    Ok(())
}
