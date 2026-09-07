use axum::{extract::State, Json};
use uuid::Uuid;

#[cfg(feature = "better-auth-integration")]
use crate::infrastructure::auth::extractors::OptionalSession;
use crate::infrastructure::context::AppState;
#[cfg(feature = "better-auth-integration")]
use better_auth::prelude::AuthUser;

use crate::domain::models::ping::PingResponse;

#[cfg(feature = "better-auth-integration")]
#[rorpc::get("/ping")]
pub async fn ping(State(_state): State<AppState>, session: OptionalSession) -> Json<PingResponse> {
    let msg = match session.0 {
        Some(s) => format!(
            "pong (authenticated as {})",
            s.user.email().unwrap_or("unknown")
        ),
        None => "pong (anonymous)".to_string(),
    };

    Json(PingResponse {
        id: Uuid::new_v4(),
        message: msg,
    })
}

#[cfg(not(feature = "better-auth-integration"))]
#[rorpc::get("/ping")]
pub async fn ping(State(_state): State<AppState>) -> Json<PingResponse> {
    Json(PingResponse {
        id: Uuid::new_v4(),
        message: "pong".to_string(),
    })
}
