use axum::extract::{Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use crate::vpn::{self, PeerIn, PeerView, VpnSettingsIn, VpnStatus};

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/vpn", get(status).put(update))
        .route("/vpn/peers", post(add_peer))
        .route("/vpn/peers/{id}", post(peer_act).delete(revoke))
        .route("/vpn/peers/{id}/config", get(config))
        .route("/vpn/peers/{id}/qr", get(qr))
}

async fn status(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<VpnStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(vpn::status(&cfg)))
}

async fn update(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<VpnSettingsIn>,
) -> Result<Json<VpnStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(vpn::update_settings(&cfg, body)?))
}

async fn add_peer(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<PeerIn>,
) -> Result<Json<PeerView>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(vpn::add_peer(&cfg, body)?))
}

#[derive(Deserialize)]
struct PeerAct {
    enabled: Option<bool>,
}

async fn peer_act(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<PeerAct>,
) -> Result<Json<VpnStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(vpn::set_peer_enabled(
        &cfg,
        &id,
        body.enabled.unwrap_or(true),
    )?))
}

async fn revoke(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<VpnStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(vpn::revoke_peer(&cfg, &id)?))
}

async fn config(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let body = vpn::peer_config(&cfg, &id)?;
    let name = vpn::conf_file_name(&cfg, &id)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(
            CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        )
        .body(body.into())
        .map_err(ApiError::internal)
}

async fn qr(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let svg = vpn::peer_qr(&cfg, &id)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "image/svg+xml")
        .body(svg.into())
        .map_err(ApiError::internal)
}
