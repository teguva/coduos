use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::WatchStream;
use tokio_stream::StreamExt;

use crate::config::{slugify, valid_id};
use crate::db::AppRow;
use crate::docker::{self, AppJob, AppPhase, ComposeStatus, PullProgress};
use crate::error::ApiError;
use crate::state::AppState;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/apps", get(list).post(create))
        .route("/apps/stream", get(stream))
        .route("/apps/{id}", get(get_one).put(update).delete(remove))
        .route("/apps/{id}/install", post(install))
        .route("/apps/{id}/update", post(update_images))
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

async fn load_app(state: &AppState, id: &str) -> Result<AppOut, ApiError> {
    let row = state.db.get_app(id)?.ok_or(ApiError::NotFound)?;
    let cfg = state.config.read().await;
    let mut status =
        docker::inspect_status(&cfg.apps_dir(), id, row.last_error.as_deref()).await;
    drop(cfg);
    if let Some(job) = state.job(id) {
        status = job.overlay(status);
    }
    Ok(to_out(row, status))
}

fn busy_conflict() -> ApiError {
    ApiError::Conflict("This app is already starting or updating".into())
}

fn require_idle(state: &AppState, id: &str) -> Result<(), ApiError> {
    if state.job(id).is_some() {
        Err(busy_conflict())
    } else {
        Ok(())
    }
}

fn require_docker() -> Result<(), ApiError> {
    if docker::docker_available() {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "Docker is not available on this host".into(),
        ))
    }
}

async fn list(State(state): State<AppState>, jar: CookieJar) -> Result<Json<Vec<AppOut>>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let apps_dir = cfg.apps_dir();
    drop(cfg);
    let mut out = Vec::new();
    for row in state.db.list_apps()? {
        let mut status =
            docker::inspect_status(&apps_dir, &row.id, row.last_error.as_deref()).await;
        if let Some(job) = state.job(&row.id) {
            status = job.overlay(status);
        }
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
    Ok(Json(load_app(&state, &id).await?))
}

async fn stream(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    current_user(&state, &jar).await?;
    let rx = state.jobs_tx.subscribe();
    let s = WatchStream::new(rx).map(|jobs: HashMap<String, AppJob>| {
        match Event::default().json_data(jobs) {
            Ok(ev) => Ok(ev),
            Err(_) => Ok(Event::default().data("{}")),
        }
    });
    Ok(Sse::new(s).keep_alive(KeepAlive::default()))
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
        last_error: None,
    };
    let cfg = state.config.read().await;
    std::fs::create_dir_all(cfg.apps_dir())?;
    docker::write_compose(&cfg.apps_dir(), &id, &row.compose_yaml)?;
    state.db.upsert_app(&row)?;
    drop(cfg);
    Ok(Json(load_app(&state, &id).await?))
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
    drop(cfg);
    Ok(Json(load_app(&state, &id).await?))
}

async fn remove(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let _ = state.db.get_app(&id)?.ok_or(ApiError::NotFound)?;
    state.clear_job(&id);
    let cfg = state.config.read().await;
    let _ = docker::compose(&cfg.apps_dir(), &id, &["down"]).await;
    docker::remove_app_dir(&cfg.apps_dir(), &id)?;
    state.db.delete_app(&id)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn install(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    spawn_job(&state, &id, JobKind::Install).await
}

async fn update_images(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    spawn_job(&state, &id, JobKind::Update).await
}

async fn start(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    let row = state.db.get_app(&id)?.ok_or(ApiError::NotFound)?;
    let cfg = state.config.read().await;
    let status = docker::inspect_status(&cfg.apps_dir(), &id, row.last_error.as_deref()).await;
    drop(cfg);
    let kind = if status.installed {
        JobKind::Start
    } else {
        JobKind::Install
    };
    spawn_job(&state, &id, kind).await
}

async fn stop(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    require_idle(&state, &id)?;
    require_docker()?;
    blocking_action(&state, &id, &["stop"]).await
}

async fn restart(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<AppOut>, ApiError> {
    current_user(&state, &jar).await?;
    spawn_job(&state, &id, JobKind::Restart).await
}

async fn blocking_action(
    state: &AppState,
    id: &str,
    args: &[&str],
) -> Result<Json<AppOut>, ApiError> {
    let row = state.db.get_app(id)?.ok_or(ApiError::NotFound)?;
    let cfg = state.config.read().await;
    docker::write_compose(&cfg.apps_dir(), id, &row.compose_yaml)?;
    let (ok, stdout, stderr) = docker::compose(&cfg.apps_dir(), id, args).await?;
    drop(cfg);
    if !ok {
        let err = docker::short_error(if !stderr.trim().is_empty() {
            &stderr
        } else {
            &stdout
        });
        let _ = state.db.set_last_error(id, Some(&err));
        return Err(ApiError::BadRequest(err));
    }
    if args != ["stop"] {
        let _ = state.db.set_last_error(id, None);
    }
    Ok(Json(load_app(state, id).await?))
}

#[derive(Clone, Copy)]
enum JobKind {
    Install,
    Update,
    Start,
    Restart,
}

impl JobKind {
    fn phase(self) -> AppPhase {
        match self {
            Self::Install => AppPhase::Installing,
            Self::Update => AppPhase::Updating,
            Self::Start | Self::Restart => AppPhase::Starting,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Install => "Downloading images…",
            Self::Update => "Downloading newer images…",
            Self::Start => "Starting…",
            Self::Restart => "Restarting…",
        }
    }

    fn pull(self) -> bool {
        matches!(self, Self::Install | Self::Update)
    }

    fn compose_args(self) -> &'static [&'static str] {
        match self {
            Self::Restart => &["restart"],
            _ => &["up", "-d"],
        }
    }
}

async fn spawn_job(state: &AppState, id: &str, kind: JobKind) -> Result<Json<AppOut>, ApiError> {
    let row = state.db.get_app(id)?.ok_or(ApiError::NotFound)?;
    require_idle(state, id)?;
    require_docker()?;
    let cfg = state.config.read().await;
    docker::write_compose(&cfg.apps_dir(), id, &row.compose_yaml)?;
    drop(cfg);
    state.set_job(AppJob::new(id, kind.phase(), kind.message()));
    let worker = state.clone();
    let job_id = id.to_string();
    tokio::spawn(async move {
        run_job(worker, job_id, kind).await;
    });
    Ok(Json(load_app(state, id).await?))
}

async fn run_job(state: AppState, id: String, kind: JobKind) {
    let cfg = state.config.read().await;
    let apps_dir = cfg.apps_dir();
    drop(cfg);

    let mut progress = PullProgress::default();
    let publish = |phase: AppPhase, progress: &PullProgress, fallback: &str| {
        let mut job = AppJob::new(&id, phase, fallback);
        if !progress.message.is_empty() {
            job.message = Some(progress.message.clone());
        }
        job.percent = progress.percent();
        state.set_job(job);
    };

    if kind.pull() {
        publish(kind.phase(), &progress, kind.message());
        match docker::compose_stream(&apps_dir, &id, &["pull"], |line| {
            progress.ingest(line);
            publish(kind.phase(), &progress, kind.message());
        })
        .await
        {
            Ok((true, _)) => {}
            Ok((false, log)) => {
                fail_job(&state, &id, &log);
                return;
            }
            Err(err) => {
                fail_job(&state, &id, &err.to_string());
                return;
            }
        }
    }

    progress = PullProgress::default();
    let start_msg = if matches!(kind, JobKind::Restart) {
        "Restarting…"
    } else {
        "Starting…"
    };
    publish(AppPhase::Starting, &progress, start_msg);
    match docker::compose_stream(&apps_dir, &id, kind.compose_args(), |line| {
        progress.ingest(line);
        publish(AppPhase::Starting, &progress, start_msg);
    })
    .await
    {
        Ok((true, _)) => {
            let _ = state.db.set_last_error(&id, None);
            wait_for_app_ready(&state, &id).await;
            state.clear_job(&id);
        }
        Ok((false, log)) => fail_job(&state, &id, &log),
        Err(err) => fail_job(&state, &id, &err.to_string()),
    }
}

async fn wait_for_app_ready(state: &AppState, id: &str) {
    let port = match state.db.get_app(id) {
        Ok(Some(row)) => row
            .web_port
            .and_then(|p| u16::try_from(p).ok())
            .filter(|&p| p > 0),
        _ => None,
    };
    let Some(port) = port else {
        return;
    };
    state.set_job(AppJob::new(
        id,
        AppPhase::Starting,
        "Waiting for the web UI…",
    ));
    let _ = docker::wait_for_tcp(port, Duration::from_secs(90)).await;
}

fn fail_job(state: &AppState, id: &str, raw: &str) {
    let err = docker::short_error(raw);
    tracing::warn!(app = %id, error = %err, "app job failed");
    let _ = state.db.set_last_error(id, Some(&err));
    state.clear_job(id);
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
        return Err(ApiError::BadRequest(docker::short_error(&stderr)));
    }
    Ok(Json(serde_json::json!({ "logs": stdout, "stderr": stderr })))
}
