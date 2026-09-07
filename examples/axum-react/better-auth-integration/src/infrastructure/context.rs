#[cfg(feature = "better-auth-integration")]
use crate::{
    domain::ports::planet_repository::PlanetRepository, infrastructure::auth::schema::AppAuthSchema,
};
#[cfg(not(feature = "better-auth-integration"))]
use crate::domain::ports::planet_repository::PlanetRepository;

#[cfg(feature = "better-auth-integration")]
use axum::extract::FromRef;
#[cfg(feature = "better-auth-integration")]
use better_auth::BetterAuth;
use std::sync::Arc;

/// Shared Axum state for the application.
///
/// Holds the planet repository and optionally Better-Auth instance when the feature is enabled.
/// `FromRef` impls allow extractors to pull sub-state automatically —
/// `CurrentSession` / `OptionalSession` need `Arc<BetterAuth<Schema>>`.
#[derive(Clone)]
pub struct AppState {
    pub planet_repo: Arc<dyn PlanetRepository>,
    #[cfg(feature = "better-auth-integration")]
    pub auth: Arc<BetterAuth<AppAuthSchema>>,
}

#[cfg(feature = "better-auth-integration")]
impl FromRef<AppState> for Arc<BetterAuth<AppAuthSchema>> {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}
