use axum::extract::{Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;

use crate::error::ApiError;
use crate::proxy::{self, DashboardIn, HostIn, ProxyStatus};
use crate::state::AppState;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/proxy", get(status).post(apply))
        .route("/proxy/ca.crt", get(ca_crt))
        .route("/proxy/dashboard", put(dashboard))
        .route("/proxy/hosts", post(create_host))
        .route("/proxy/hosts/{id}", put(update_host).delete(delete_host))
}

async fn status(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<ProxyStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(proxy::status(&cfg)))
}

async fn ca_crt(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let body = proxy::ca_pem(&cfg)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/x-x509-ca-cert")
        .header(
            CONTENT_DISPOSITION,
            "attachment; filename=\"coduos-lan-ca.crt\"",
        )
        .body(body.into())
        .map_err(ApiError::internal)
}

async fn apply(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<ProxyStatus>, ApiError> {
    current_user(&state, &jar).await?;
    crate::util::require_privileged()?;
    let cfg = state.config.read().await;
    Ok(Json(proxy::apply_now(&cfg)?))
}

async fn dashboard(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<DashboardIn>,
) -> Result<Json<ProxyStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(proxy::set_dashboard_tls(&cfg, &body.tls)?))
}

async fn create_host(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<HostIn>,
) -> Result<Json<ProxyStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(proxy::upsert_host(&cfg, body, None)?))
}

async fn update_host(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<HostIn>,
) -> Result<Json<ProxyStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(proxy::upsert_host(&cfg, body, Some(id))?))
}

async fn delete_host(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<ProxyStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(proxy::delete_host(&cfg, &id)?))
}
