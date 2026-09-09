use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use crate::storage::{self, Inventory, MountOpts, Partition, UnmountResult};

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/storage", get(list))
        .route("/storage/mount", post(mount))
        .route("/storage/unmount", post(unmount))
        .route("/storage/files", post(add_files))
        .route("/storage/format", post(format_dev))
        .route("/storage/auto-mount", post(auto_mount))
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

#[derive(Deserialize)]
struct MountIn {
    device: String,
    folder: Option<String>,
    files_name: Option<String>,
    add_to_files: Option<bool>,
    auto_mount: Option<bool>,
    read_only: Option<bool>,
}

async fn mount(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<MountIn>,
) -> Result<Json<Partition>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(storage::mount_device_with(
        &mut cfg,
        &state.config_path,
        &body.device,
        &MountOpts {
            folder: body.folder,
            files_label: body.files_name,
            add_to_files: body.add_to_files.unwrap_or(true),
            auto_mount: body.auto_mount.unwrap_or(true),
            read_only: body.read_only.unwrap_or(false),
        },
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

#[derive(Deserialize)]
struct FormatIn {
    device: String,
    fstype: Option<String>,
    label: Option<String>,
}

async fn format_dev(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<FormatIn>,
) -> Result<Json<Partition>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(storage::format_and_mount(
        &mut cfg,
        &state.config_path,
        &body.device,
        body.fstype.as_deref().unwrap_or("ext4"),
        body.label.as_deref().unwrap_or(""),
    )?))
}

#[derive(Deserialize)]
struct AutoMountIn {
    device: String,
    enabled: bool,
}

async fn auto_mount(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<AutoMountIn>,
) -> Result<Json<Partition>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(storage::set_auto_mount(
        &mut cfg,
        &state.config_path,
        &body.device,
        body.enabled,
    )?))
}
