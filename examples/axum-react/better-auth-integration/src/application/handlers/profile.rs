#[cfg(feature = "better-auth-integration")]
use axum::{extract::State, Json};
#[cfg(feature = "better-auth-integration")]
use better_auth::prelude::AuthUser;
#[cfg(feature = "better-auth-integration")]
use serde_json::json;

#[cfg(feature = "better-auth-integration")]
use crate::infrastructure::{
    auth::extractors::{Session, SessionExt},
    context::AppState,
};

#[cfg(feature = "better-auth-integration")]
#[rorpc::get("/profile")]
pub async fn get_profile(
    State(_state): State<AppState>,
    session: Session,
) -> Json<serde_json::Value> {
    Json(json!({
        "id": session.user_id(),
        "email": session.user_email(),
        "name": session.user.name(),
    }))
}
