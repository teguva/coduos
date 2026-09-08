use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;

use crate::ddns::{self, DdnsSettingsIn, DdnsStatus};
use crate::error::ApiError;
use crate::state::AppState;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ddns", get(status).put(update))
        .route("/ddns/refresh", post(refresh))
}

async fn status(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<DdnsStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(ddns::status(&cfg)))
}

async fn update(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<DdnsSettingsIn>,
) -> Result<Json<DdnsStatus>, ApiError> {
    current_user(&state, &jar).await?;
    Ok(Json(ddns::update_settings(&state, body).await?))
}

async fn refresh(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<DdnsStatus>, ApiError> {
    current_user(&state, &jar).await?;
    Ok(Json(ddns::refresh(&state, true).await?))
}
