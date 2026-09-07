use axum::{
    extract::{Path, Query, State},
    Json,
};

use crate::{
    application::errors::AppError,
    domain::models::planet::*,
    infrastructure::context::AppState,
};

#[cfg(feature = "better-auth-integration")]
use crate::infrastructure::auth::extractors::Session;

#[rorpc::get("/planet/list")]
pub async fn list_planets(State(state): State<AppState>) -> Result<Json<Vec<Planet>>, AppError> {
    state
        .planet_repo
        .list()
        .await
        .map(Json)
        .map_err(|e| AppError::Internal { msg: e.to_string() })
}

#[rorpc::get("/planet/list-paginated")]
pub async fn list_planets_paginated(
    State(state): State<AppState>,
    Query(input): Query<ListPlanetsPaginatedInput>,
) -> Result<Json<ListPlanetsPaginatedOutput>, AppError> {
    state
        .planet_repo
        .list_paginated(input)
        .await
        .map(Json)
        .map_err(|e| AppError::Internal { msg: e.to_string() })
}

#[rorpc::get("/planet/{id}")]
pub async fn find_planet(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(query): Query<FindPlanetQuery>,
) -> Result<Json<Planet>, AppError> {
    let input = FindPlanetInput { id, q: query.q };
    state
        .planet_repo
        .find(input)
        .await
        .map(Json)
        .map_err(|_| AppError::NotFound)
}

/// Protected — `CurrentSession` returns 401 automatically if not authenticated.
#[cfg(feature = "better-auth-integration")]
#[rorpc::post("/planet")]
pub async fn create_planet(
    State(state): State<AppState>,
    _session: Session,
    Json(input): Json<CreatePlanetInput>,
) -> Result<Json<Planet>, AppError> {
    state
        .planet_repo
        .create(input)
        .await
        .map(Json)
        .map_err(|e| AppError::Internal { msg: e.to_string() })
}

/// Unprotected version — no authentication required
#[cfg(not(feature = "better-auth-integration"))]
#[rorpc::post("/planet")]
pub async fn create_planet(
    State(state): State<AppState>,
    Json(input): Json<CreatePlanetInput>,
) -> Result<Json<Planet>, AppError> {
    state
        .planet_repo
        .create(input)
        .await
        .map(Json)
        .map_err(|e| AppError::Internal { msg: e.to_string() })
}

/// Protected — `CurrentSession` returns 401 automatically if not authenticated.
#[cfg(feature = "better-auth-integration")]
#[rorpc::delete("/planet/{id}")]
pub async fn delete_planet(
    State(state): State<AppState>,
    _session: Session,
    Path(id): Path<i32>,
) -> Result<Json<()>, AppError> {
    state
        .planet_repo
        .delete(DeletePlanetInput { id })
        .await
        .map(Json)
        .map_err(|_| AppError::NotFound)
}

/// Unprotected version — no authentication required
#[cfg(not(feature = "better-auth-integration"))]
#[rorpc::delete("/planet/{id}")]
pub async fn delete_planet(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<()>, AppError> {
    state
        .planet_repo
        .delete(DeletePlanetInput { id })
        .await
        .map(Json)
        .map_err(|_| AppError::NotFound)
}
