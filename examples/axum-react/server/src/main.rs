//! Axum server example with optional Better Auth integration
//!
//! By default, runs without authentication. Enable with:
//! ```bash
//! cargo run --bin server --features better-auth-integration
//! ```

#[cfg(feature = "better-auth-integration")]
mod auth_schema;

#[cfg(feature = "better-auth-integration")]
use auth_schema::AppAuthSchema;
use axum::{
    http::StatusCode,
    response::sse::Event,
    response::{IntoResponse, Json, Response, Sse},
    routing::post,
    Router,
};
#[cfg(feature = "better-auth-integration")]
use better_auth::{
    integrations::axum::{AxumIntegration, CurrentSession, OptionalSession},
    plugins::{EmailPasswordPlugin, SessionManagementPlugin},
    prelude::AuthUser,
    seaorm::{sea_orm::Database, SeaOrmStore},
    AuthConfig, BetterAuth,
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;

// ===== State =====

#[cfg(feature = "better-auth-integration")]
#[derive(Clone)]
struct AppState {
    planets: Arc<Vec<Planet>>,
    auth: Arc<BetterAuth<AppAuthSchema>>,
}

#[cfg(not(feature = "better-auth-integration"))]
#[derive(Clone)]
struct AppState {
    planets: Arc<Vec<Planet>>,
}

#[cfg(feature = "better-auth-integration")]
use axum::extract::FromRef;

#[cfg(feature = "better-auth-integration")]
impl FromRef<AppState> for Arc<BetterAuth<AppAuthSchema>> {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Planet {
    id: i32,
    name: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FindPlanetInput {
    id: i32,
}

#[derive(Debug, Deserialize)]
struct CreatePlanetInput {
    name: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListPlanetsPaginatedInput {
    limit: usize,
    offset: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ListPlanetsPaginatedOutput {
    items: Vec<Planet>,
    #[serde(rename = "nextPageParam")]
    next_page_param: Option<usize>,
}

// ===== Error =====

#[derive(Debug, Serialize)]
struct RpcError {
    code: String,
    message: String,
}

impl RpcError {
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: "NOT_FOUND".into(),
            message: message.into(),
        }
    }
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            code: "BAD_REQUEST".into(),
            message: message.into(),
        }
    }
    fn internal_error(message: impl Into<String>) -> Self {
        Self {
            code: "INTERNAL_ERROR".into(),
            message: message.into(),
        }
    }
}

impl IntoResponse for RpcError {
    fn into_response(self) -> Response {
        let status = match self.code.as_str() {
            "NOT_FOUND" => StatusCode::NOT_FOUND,
            "BAD_REQUEST" => StatusCode::BAD_REQUEST,
            "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}

// ===== Handlers =====

#[cfg(feature = "better-auth-integration")]
async fn ping(session: OptionalSession<AppAuthSchema>) -> Json<String> {
    match session.0 {
        Some(s) => Json(format!(
            "pong (authenticated as {})",
            s.user.email().unwrap_or("unknown")
        )),
        None => Json("pong (anonymous)".to_string()),
    }
}

#[cfg(not(feature = "better-auth-integration"))]
async fn ping() -> Json<String> {
    Json("pong".to_string())
}

#[cfg(feature = "better-auth-integration")]
async fn list_planets(
    axum::extract::State(state): axum::extract::State<AppState>,
    _session: OptionalSession<AppAuthSchema>,
) -> Json<Vec<Planet>> {
    Json(state.planets.to_vec())
}

#[cfg(not(feature = "better-auth-integration"))]
async fn list_planets(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<Vec<Planet>> {
    Json(state.planets.to_vec())
}

#[cfg(feature = "better-auth-integration")]
async fn list_planets_paginated(
    axum::extract::State(state): axum::extract::State<AppState>,
    _session: OptionalSession<AppAuthSchema>,
    Json(input): Json<ListPlanetsPaginatedInput>,
) -> Json<ListPlanetsPaginatedOutput> {
    let offset = input.offset.unwrap_or(0);
    let items: Vec<Planet> = state
        .planets
        .iter()
        .skip(offset)
        .take(input.limit)
        .cloned()
        .collect();
    let next_page_param = if offset + input.limit < state.planets.len() {
        Some(offset + input.limit)
    } else {
        None
    };
    Json(ListPlanetsPaginatedOutput {
        items,
        next_page_param,
    })
}

#[cfg(not(feature = "better-auth-integration"))]
async fn list_planets_paginated(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(input): Json<ListPlanetsPaginatedInput>,
) -> Json<ListPlanetsPaginatedOutput> {
    let offset = input.offset.unwrap_or(0);
    let items: Vec<Planet> = state
        .planets
        .iter()
        .skip(offset)
        .take(input.limit)
        .cloned()
        .collect();
    let next_page_param = if offset + input.limit < state.planets.len() {
        Some(offset + input.limit)
    } else {
        None
    };
    Json(ListPlanetsPaginatedOutput {
        items,
        next_page_param,
    })
}

#[cfg(feature = "better-auth-integration")]
async fn find_planet(
    axum::extract::State(state): axum::extract::State<AppState>,
    _session: OptionalSession<AppAuthSchema>,
    Json(input): Json<FindPlanetInput>,
) -> Result<Json<Planet>, RpcError> {
    state
        .planets
        .iter()
        .find(|p| p.id == input.id)
        .cloned()
        .map(Json)
        .ok_or_else(|| RpcError::not_found(format!("Planet with id {} not found", input.id)))
}

#[cfg(not(feature = "better-auth-integration"))]
async fn find_planet(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(input): Json<FindPlanetInput>,
) -> Result<Json<Planet>, RpcError> {
    state
        .planets
        .iter()
        .find(|p| p.id == input.id)
        .cloned()
        .map(Json)
        .ok_or_else(|| RpcError::not_found(format!("Planet with id {} not found", input.id)))
}

#[cfg(feature = "better-auth-integration")]
async fn create_planet(
    axum::extract::State(state): axum::extract::State<AppState>,
    // CurrentSession rejects the request automatically if not signed in
    session: CurrentSession<AppAuthSchema>,
    Json(input): Json<CreatePlanetInput>,
) -> Result<Json<Planet>, RpcError> {
    let _user = session.user;

    if input.name.trim().is_empty() {
        return Err(RpcError::bad_request("Planet name cannot be empty"));
    }
    if input.name.len() > 100 {
        return Err(RpcError::internal_error(
            "Planet name too long (max 100 characters)",
        ));
    }

    Ok(Json(Planet {
        id: state.planets.len() as i32 + 1,
        name: input.name,
        description: input.description,
    }))
}

#[cfg(not(feature = "better-auth-integration"))]
async fn create_planet(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(input): Json<CreatePlanetInput>,
) -> Result<Json<Planet>, RpcError> {
    if input.name.trim().is_empty() {
        return Err(RpcError::bad_request("Planet name cannot be empty"));
    }
    if input.name.len() > 100 {
        return Err(RpcError::internal_error(
            "Planet name too long (max 100 characters)",
        ));
    }

    Ok(Json(Planet {
        id: state.planets.len() as i32 + 1,
        name: input.name,
        description: input.description,
    }))
}

// ===== Streaming =====

#[derive(Debug, Serialize)]
struct StreamEvent {
    message: String,
    count: u32,
}

async fn stream_events() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let initial = tokio_stream::iter([Ok(Event::default().comment(""))]);

    let events = tokio_stream::iter(0u32..)
        .throttle(Duration::from_secs(1))
        .take(10)
        .map(|count| {
            let payload = serde_json::to_string(&StreamEvent {
                message: format!("Event #{count}"),
                count,
            })
            .unwrap();
            Ok(Event::default()
                .event("message")
                .id(count.to_string())
                .retry(Duration::from_secs(5))
                .data(payload))
        })
        .chain(tokio_stream::iter([Ok(Event::default()
            .event("close")
            .data(""))]));

    Sse::new(initial.chain(events)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text(""),
    )
}

async fn stream_async() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    use async_stream::stream;

    let s = stream! {
        yield Ok(Event::default().comment(""));
        for i in 0u32..15 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let payload = serde_json::to_string(&StreamEvent {
                message: format!("Async Stream Event #{i}"),
                count: i,
            })
            .unwrap();
            yield Ok(Event::default()
                .event("message")
                .id(i.to_string())
                .retry(Duration::from_secs(5))
                .data(payload));
        }
        yield Ok(Event::default().event("close").data(""));
    };

    Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text(""),
    )
}

// ===== Main =====

fn sample_planets() -> Vec<Planet> {
    vec![
        Planet {
            id: 1,
            name: "Mercury".into(),
            description: Some("The smallest planet".into()),
        },
        Planet {
            id: 2,
            name: "Venus".into(),
            description: Some("The hottest planet".into()),
        },
        Planet {
            id: 3,
            name: "Earth".into(),
            description: Some("The blue planet".into()),
        },
        Planet {
            id: 4,
            name: "Mars".into(),
            description: Some("The red planet".into()),
        },
        Planet {
            id: 5,
            name: "Jupiter".into(),
            description: Some("The largest planet".into()),
        },
        Planet {
            id: 6,
            name: "Saturn".into(),
            description: Some("The ringed planet".into()),
        },
        Planet {
            id: 7,
            name: "Uranus".into(),
            description: Some("The ice giant".into()),
        },
        Planet {
            id: 8,
            name: "Neptune".into(),
            description: Some("The windiest planet".into()),
        },
        Planet {
            id: 9,
            name: "Pluto".into(),
            description: Some("The dwarf planet".into()),
        },
        Planet {
            id: 10,
            name: "Ceres".into(),
            description: Some("Dwarf planet in asteroid belt".into()),
        },
        Planet {
            id: 11,
            name: "Eris".into(),
            description: Some("Distant dwarf planet".into()),
        },
        Planet {
            id: 12,
            name: "Haumea".into(),
            description: Some("Egg-shaped dwarf planet".into()),
        },
    ]
}

#[cfg(feature = "better-auth-integration")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting server with Better Auth integration...");
    
    let database = Database::connect("sqlite::memory:").await?;
    auth_schema::run_app_migrations(&database).await?;

    let secret = "your-very-secure-secret-key-at-least-32-chars-long";
    let config = AuthConfig::new(secret)
        .base_url("http://localhost:3000")
        .password_min_length(8);

    let store = SeaOrmStore::<AppAuthSchema>::new(config.clone(), database);

    let auth = Arc::new(
        BetterAuth::<AppAuthSchema>::new(config)
            .store(store)
            .plugin(EmailPasswordPlugin::new().enable_signup(true))
            .plugin(SessionManagementPlugin::new())
            .build()
            .await?,
    );

    let state = AppState {
        planets: Arc::new(sample_planets()),
        auth: auth.clone(),
    };

    // Resolve auth_router's state before nesting into AppState router
    let auth_router = auth.clone().axum_router().with_state(auth.clone());

    let rpc_router = Router::new()
        .route("/ping", post(ping))
        .route("/planet/list", post(list_planets))
        .route("/planet/list-paginated", post(list_planets_paginated))
        .route("/planet/find", post(find_planet))
        .route("/planet/create", post(create_planet))
        .route("/stream", post(stream_events))
        .route("/stream-async", post(stream_async));

    let app = Router::new()
        .nest("/api/auth", auth_router)
        .nest("/rpc", rpc_router)
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:3000".parse::<axum::http::HeaderValue>()?,
                    "http://127.0.0.1:3000".parse::<axum::http::HeaderValue>()?,
                    "http://localhost:5173".parse::<axum::http::HeaderValue>()?,
                    "http://127.0.0.1:5173".parse::<axum::http::HeaderValue>()?,
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
    println!();
    println!("📚 Better Auth  (under /api/auth):");
    println!("   POST /api/auth/sign-up/email");
    println!("   POST /api/auth/sign-in/email");
    println!("   POST /api/auth/sign-out");
    println!("   GET  /api/auth/get-session");
    println!();
    println!("🔧 oRPC  (under /rpc):");
    println!("   POST /rpc/ping                  (public)");
    println!("   POST /rpc/planet/list           (public)");
    println!("   POST /rpc/planet/list-paginated (public)");
    println!("   POST /rpc/planet/find           (public)");
    println!("   POST /rpc/planet/create         (requires auth)");
    println!("   POST /rpc/stream                (public)");
    println!("   POST /rpc/stream-async          (public)");

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(not(feature = "better-auth-integration"))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting server without authentication");
    println!("💡 To enable Better Auth, run: cargo run --bin server --features better-auth-integration");
    
    let state = AppState {
        planets: Arc::new(sample_planets()),
    };

    let rpc_router = Router::new()
        .route("/ping", post(ping))
        .route("/planet/list", post(list_planets))
        .route("/planet/list-paginated", post(list_planets_paginated))
        .route("/planet/find", post(find_planet))
        .route("/planet/create", post(create_planet))
        .route("/stream", post(stream_events))
        .route("/stream-async", post(stream_async));

    let app = Router::new()
        .nest("/rpc", rpc_router)
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:3000".parse::<axum::http::HeaderValue>()?,
                    "http://127.0.0.1:3000".parse::<axum::http::HeaderValue>()?,
                    "http://localhost:5173".parse::<axum::http::HeaderValue>()?,
                    "http://127.0.0.1:5173".parse::<axum::http::HeaderValue>()?,
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

    println!();
    println!("🚀 Server running on http://127.0.0.1:3001");
    println!();
    println!("🔧 oRPC  (under /rpc):");
    println!("   POST /rpc/ping                  (public)");
    println!("   POST /rpc/planet/list           (public)");
    println!("   POST /rpc/planet/list-paginated (public)");
    println!("   POST /rpc/planet/find           (public)");
    println!("   POST /rpc/planet/create         (public - no auth)");
    println!("   POST /rpc/stream                (public)");
    println!("   POST /rpc/stream-async          (public)");

    axum::serve(listener, app).await?;
    Ok(())
}
