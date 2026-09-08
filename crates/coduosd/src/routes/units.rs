use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use crate::systemd::{self, UnitStatus};

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/units", get(list))
        .route("/units/{id}/{action}", post(act))
        .route("/units/{id}/journal", get(journal))
}

async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<UnitStatus>>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let mut out = Vec::new();
    for spec in &cfg.units {
        match systemd::status(spec).await {
            Ok(s) => out.push(s),
            Err(err) => out.push(UnitStatus {
                id: spec.id.clone(),
                unit: spec.unit.clone(),
                label: spec.label.clone(),
                active: "unknown".into(),
                enabled: "unknown".into(),
                description: err.to_string(),
            }),
        }
    }
    Ok(Json(out))
}

async fn act(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((id, action)): Path<(String, String)>,
) -> Result<Json<UnitStatus>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let spec = cfg
        .unit(&id)
        .cloned()
        .ok_or(ApiError::Forbidden)?;
    drop(cfg);
    systemd::action(&spec, &action).await?;
    Ok(Json(systemd::status(&spec).await?))
}

#[derive(Deserialize)]
struct JournalQuery {
    lines: Option<u32>,
}

async fn journal(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(q): Query<JournalQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let spec = cfg
        .unit(&id)
        .cloned()
        .ok_or(ApiError::Forbidden)?;
    drop(cfg);
    let text = systemd::journal(&spec, q.lines.unwrap_or(100)).await?;
    Ok(Json(serde_json::json!({ "journal": text })))
}
