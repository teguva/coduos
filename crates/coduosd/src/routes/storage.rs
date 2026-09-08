use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use crate::storage::{self, Inventory, Partition, UnmountResult};

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/storage", get(list))
        .route("/storage/mount", post(mount))
        .route("/storage/unmount", post(unmount))
        .route("/storage/files", post(add_files))
}

async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Inventory>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(storage::inventory(&cfg)?))
}

#[derive(Deserialize)]
struct DeviceIn {
    device: String,
}

async fn mount(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<DeviceIn>,
) -> Result<Json<Partition>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(storage::mount_device(
        &mut cfg,
        &state.config_path,
        &body.device,
    )?))
}

#[derive(Deserialize)]
struct UnmountIn {
    device: Option<String>,
    mountpoint: Option<String>,
    force: Option<bool>,
}

async fn unmount(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<UnmountIn>,
) -> Result<Json<UnmountResult>, ApiError> {
    current_user(&state, &jar).await?;
    let target = body
        .device
        .or(body.mountpoint)
        .ok_or_else(|| ApiError::BadRequest("device or mountpoint required".into()))?;
    let mut cfg = state.config.write().await;
    Ok(Json(storage::unmount_device(
        &mut cfg,
        &state.config_path,
        &target,
        body.force.unwrap_or(false),
    )?))
}

async fn add_files(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<DeviceIn>,
) -> Result<Json<crate::config::FileRoot>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(storage::add_to_files(
        &mut cfg,
        &state.config_path,
        &body.device,
    )?))
}
