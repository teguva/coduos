use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::wrappers::WatchStream;
use tokio_stream::StreamExt;

use crate::battery::{self, BatteryPack, ChargeLimitIn};
use crate::display::{self, DisplayIn, DisplayOut};
use crate::error::ApiError;
use crate::kernel::{self, KernelIn, KernelOut};
use crate::state::AppState;
use crate::stats::{self, DockerStat, ProcInfo, SystemSummary};
use crate::util;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/system/summary", get(summary))
        .route("/system/summary/stream", get(stream))
        .route("/system/tasks", get(tasks))
        .route("/system/process/{pid}/signal", post(signal))
        .route("/system/battery", get(battery).post(set_battery))
        .route("/system/display", get(get_display).post(set_display))
        .route("/system/kernel", get(get_kernel).post(set_kernel))
        .route("/system/power", post(power))
}

#[derive(Serialize)]
struct TasksOut {
    processes: Vec<ProcInfo>,
    containers: Vec<DockerStat>,
}

async fn tasks(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<TasksOut>, ApiError> {
    current_user(&state, &jar).await?;
    let processes = tokio::task::spawn_blocking(stats::list_processes)
        .await
        .map_err(ApiError::internal)?;
    let containers = tokio::task::spawn_blocking(stats::docker_stats)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(TasksOut {
        processes,
        containers,
    }))
}

async fn signal(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(pid): Path<u32>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    tokio::task::spawn_blocking(move || stats::signal_process(pid))
        .await
        .map_err(ApiError::internal)??;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn summary(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<SystemSummary>, ApiError> {
    current_user(&state, &jar).await?;
    Ok(Json(state.summary_rx.borrow().clone()))
}

async fn stream(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    current_user(&state, &jar).await?;
    let rx = state.summary_tx.subscribe();
    let s = WatchStream::new(rx).map(|sum| match Event::default().json_data(sum) {
        Ok(ev) => Ok(ev),
        Err(_) => Ok(Event::default().data("{}")),
    });
    Ok(Sse::new(s).keep_alive(KeepAlive::default()))
}

async fn battery(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<BatteryPack>, ApiError> {
    current_user(&state, &jar).await?;
    Ok(Json(battery::snapshot()))
}

async fn set_battery(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<ChargeLimitIn>,
) -> Result<Json<BatteryPack>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(battery::apply_and_save(
        &mut cfg,
        &state.config_path,
        body.limit_pct,
        body.start_pct,
    )?))
}

async fn get_display(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<DisplayOut>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(display::snapshot(&cfg)))
}

async fn set_display(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<DisplayIn>,
) -> Result<Json<DisplayOut>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(display::apply_and_save(
        &mut cfg,
        &state.config_path,
        body.off,
        body.ignore_lid,
    )?))
}

async fn get_kernel(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<KernelOut>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(kernel::snapshot(&cfg)))
}

async fn set_kernel(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<KernelIn>,
) -> Result<Json<KernelOut>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    Ok(Json(kernel::apply_and_save(
        &mut cfg,
        &state.config_path,
        body.memory_overcommit,
    )?))
}

#[derive(Deserialize)]
struct PowerIn {
    action: String,
}

async fn power(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<PowerIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    util::require_privileged()?;
    let unit = match body.action.as_str() {
        "reboot" => "reboot",
        "shutdown" => "poweroff",
        _ => {
            return Err(ApiError::BadRequest(
                "action must be reboot or shutdown".into(),
            ))
        }
    };
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let _ = tokio::process::Command::new("systemctl")
            .args([unit, "--no-block"])
            .status()
            .await;
    });
    Ok(Json(serde_json::json!({"ok": true, "action": body.action})))
}
