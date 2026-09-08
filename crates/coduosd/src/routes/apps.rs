use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};

use crate::config::{slugify, valid_id};
use crate::db::AppRow;
use crate::docker::{self, ComposeStatus};
use crate::error::ApiError;
use crate::state::AppState;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/apps", get(list).post(create))
        .route("/apps/{id}", get(get_one).put(update).delete(remove))
        .route("/apps/{id}/start", post(start))
        .route("/apps/{id}/stop", post(stop))
        .route("/apps/{id}/restart", post(restart))
        .route("/apps/{id}/logs", get(logs))
}

#[derive(Serialize)]
struct AppOut {
    id: String,
    name: String,
    compose_yaml: String,
    icon_url: Option<String>,
    web_port: Option<i64>,
    created_at: String,
    status: ComposeStatus,
}

#[derive(Deserialize)]
struct AppIn {
    id: Option<String>,
    name: String,
    compose_yaml: String,
    icon_url: Option<String>,
    web_port: Option<i64>,
}

fn to_out(row: AppRow, status: ComposeStatus) -> AppOut {
    AppOut {
        id: row.id,
        name: row.name,
        compose_yaml: row.compose_yaml,
        icon_url: row.icon_url,
        web_port: row.web_port,
        created_at: row.created_at,
        status,
    }
}

async fn list(State(state): State<AppState>, jar: CookieJar) -> Result<Json<Vec<AppOut>>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let apps_dir = cfg.apps_dir();
    drop(cfg);
    let mut out = Vec::new();
    for row in state.db.list_apps()? {
        let status = docker::compose_ps(&apps_dir, &row.id).await;
        out.push(to_out(row, status));
    }
    Ok(Json(out))
}

async fn get_one(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    let row = state.db.get_app(&id)?.ok_or(ApiError::NotFound)?;
    let cfg = state.config.read().await;
    let status = docker::compose_ps(&cfg.apps_dir(), &id).await;
    Ok(Json(to_out(row, status)))
}

async fn create(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<AppIn>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    if body.compose_yaml.trim().is_empty() {
        return Err(ApiError::BadRequest("compose YAML is required".into()));
    }
    serde_yaml::from_str::<serde_yaml::Value>(&body.compose_yaml)
        .map_err(|e| ApiError::BadRequest(format!("invalid YAML: {e}")))?;

    let mut id = body
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| slugify(&body.name));
    if !valid_id(&id) {
        id = slugify(&id);
    }
    if !valid_id(&id) {
        return Err(ApiError::BadRequest("invalid app id".into()));
    }
    if state.db.get_app(&id)?.is_some() {
        return Err(ApiError::Conflict(format!("app {id} already exists")));
    }

    let port = body
        .web_port
        .or_else(|| docker::first_host_port(&body.compose_yaml).map(|p| p as i64));
    let row = AppRow {
        id: id.clone(),
        name: body.name,
        compose_yaml: body.compose_yaml,
        icon_url: body.icon_url,
        web_port: port,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let cfg = state.config.read().await;
    std::fs::create_dir_all(cfg.apps_dir())?;
    docker::write_compose(&cfg.apps_dir(), &id, &row.compose_yaml)?;
    state.db.upsert_app(&row)?;
    let status = docker::compose_ps(&cfg.apps_dir(), &id).await;
    Ok(Json(to_out(row, status)))
}

async fn update(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<AppIn>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    let mut row = state.db.get_app(&id)?.ok_or(ApiError::NotFound)?;
    serde_yaml::from_str::<serde_yaml::Value>(&body.compose_yaml)
        .map_err(|e| ApiError::BadRequest(format!("invalid YAML: {e}")))?;
    row.name = body.name;
    row.compose_yaml = body.compose_yaml;
    row.icon_url = body.icon_url;
    row.web_port = body
        .web_port
        .or_else(|| docker::first_host_port(&row.compose_yaml).map(|p| p as i64));
    let cfg = state.config.read().await;
    docker::write_compose(&cfg.apps_dir(), &id, &row.compose_yaml)?;
    state.db.upsert_app(&row)?;
    let status = docker::compose_ps(&cfg.apps_dir(), &id).await;
    Ok(Json(to_out(row, status)))
}

async fn remove(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let _ = state.db.get_app(&id)?.ok_or(ApiError::NotFound)?;
    let cfg = state.config.read().await;
    let _ = docker::compose(&cfg.apps_dir(), &id, &["down"]).await;
    docker::remove_app_dir(&cfg.apps_dir(), &id)?;
    state.db.delete_app(&id)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn start(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    action(&state, &jar, &id, &["up", "-d"]).await
}

async fn stop(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    action(&state, &jar, &id, &["stop"]).await
}

async fn restart(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    action(&state, &jar, &id, &["restart"]).await
}

async fn action(
    state: &AppState,
    jar: &CookieJar,
    id: &str,
    args: &[&str],
) -> Result<Json<AppOut>, ApiError> {
    current_user(state, jar).await?;
    let row = state.db.get_app(id)?.ok_or(ApiError::NotFound)?;
    if !docker::docker_available() {
        return Err(ApiError::BadRequest(
            "Docker is not available on this host".into(),
        ));
    }
    let cfg = state.config.read().await;
    docker::write_compose(&cfg.apps_dir(), id, &row.compose_yaml)?;
    let (ok, stdout, stderr) = docker::compose(&cfg.apps_dir(), id, args).await?;
    if !ok {
        return Err(ApiError::BadRequest(if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        }));
    }
    let status = docker::compose_ps(&cfg.apps_dir(), id).await;
    Ok(Json(to_out(row, status)))
}

#[derive(Deserialize)]
struct LogQuery {
    tail: Option<u32>,
}

async fn logs(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let _ = state.db.get_app(&id)?.ok_or(ApiError::NotFound)?;
    let tail = q.tail.unwrap_or(200).clamp(1, 2000).to_string();
    let cfg = state.config.read().await;
    let (ok, stdout, stderr) = docker::compose(
        &cfg.apps_dir(),
        &id,
        &["logs", "--no-color", "--tail", &tail],
    )
    .await?;
    if !ok && stdout.is_empty() {
        return Err(ApiError::BadRequest(stderr));
    }
    Ok(Json(serde_json::json!({ "logs": stdout, "stderr": stderr })))
}
