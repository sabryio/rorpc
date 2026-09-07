use axum::{extract::State, Json};

#[cfg(feature = "better-auth-integration")]
use better_auth::prelude::AuthUser;
#[cfg(feature = "better-auth-integration")]
use crate::infrastructure::auth::extractors::OptionalSession;
use crate::infrastructure::context::AppState;

#[cfg(feature = "better-auth-integration")]
#[rorpc::get("/ping")]
pub async fn ping(State(_state): State<AppState>, session: OptionalSession) -> Json<String> {
    let msg = match session.0 {
        Some(s) => format!(
            "pong (authenticated as {})",
            s.user.email().unwrap_or("unknown")
        ),
        None => "pong (anonymous)".to_string(),
    };
    Json(msg)
}

#[cfg(not(feature = "better-auth-integration"))]
#[rorpc::get("/ping")]
pub async fn ping(State(_state): State<AppState>) -> Json<String> {
    Json("pong".to_string())
}
