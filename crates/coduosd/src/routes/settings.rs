use axum::extract::State;
use axum::routing::{get, put};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};

use crate::auth;
use crate::config::{valid_id, FileRoot, UnitSpec};
use crate::error::ApiError;
use crate::github;
use crate::state::AppState;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(get_settings).put(put_settings))
        .route("/settings/password", put(password))
        .route("/update", get(update).post(apply_update))
}

#[derive(Serialize)]
struct SettingsOut {
    bind: String,
    data_dir: String,
    www_dir: String,
    github_owner: String,
    github_repo: String,
    file_roots: Vec<FileRoot>,
    units: Vec<UnitSpec>,
    version: &'static str,
    hostname: String,
    privileged: bool,
}

async fn get_settings(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<SettingsOut>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    Ok(Json(settings_out(&cfg)))
}

fn settings_out(cfg: &crate::config::Config) -> SettingsOut {
    SettingsOut {
        bind: cfg.bind.clone(),
        data_dir: cfg.data_dir.display().to_string(),
        www_dir: cfg.www_dir.display().to_string(),
        github_owner: cfg.github_owner.clone(),
        github_repo: cfg.github_repo.clone(),
        file_roots: cfg.file_roots.clone(),
        units: cfg.units.clone(),
        version: env!("CARGO_PKG_VERSION"),
        hostname: hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "coduos".into()),
        privileged: crate::config::running_as_root(),
    }
}

#[derive(Deserialize)]
struct SettingsIn {
    file_roots: Option<Vec<FileRoot>>,
    units: Option<Vec<UnitSpec>>,
    github_owner: Option<String>,
    github_repo: Option<String>,
}

async fn put_settings(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<SettingsIn>,
) -> Result<Json<SettingsOut>, ApiError> {
    current_user(&state, &jar).await?;
    let mut cfg = state.config.write().await;
    let mut next = cfg.clone();
    if let Some(roots) = body.file_roots {
        for r in &roots {
            if !valid_id(&r.id) {
                return Err(ApiError::BadRequest(format!("invalid root id {}", r.id)));
            }
            if r.path.as_os_str().is_empty() {
                return Err(ApiError::BadRequest("root path required".into()));
            }
        }
        next.file_roots = roots;
    }
    if let Some(units) = body.units {
        for u in &units {
            if !valid_id(&u.id) {
                return Err(ApiError::BadRequest(format!("invalid unit id {}", u.id)));
            }
            if u.unit.contains('/') || u.unit.contains("..") {
                return Err(ApiError::Forbidden);
            }
        }
        next.units = units;
    }
    if let Some(o) = body.github_owner {
        next.github_owner = o;
    }
    if let Some(r) = body.github_repo {
        next.github_repo = r;
    }
    next.save(&state.config_path)
        .map_err(|e| ApiError::BadRequest(format!("could not write config: {e}")))?;
    *cfg = next;
    Ok(Json(settings_out(&cfg)))
}

#[derive(Deserialize)]
struct PasswordIn {
    current: String,
    new_password: String,
}

async fn password(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<PasswordIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = current_user(&state, &jar).await?;
    if !auth::verify_password(&body.current, &user.password_hash)? {
        return Err(ApiError::Unauthorized);
    }
    auth::validate_password(&body.new_password)?;
    let hash = auth::hash_password(&body.new_password)?;
    state.db.set_password_hash(user.id, &hash)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn update(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<github::UpdateInfo>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let info = github::check(&state.http, &cfg.github_owner, &cfg.github_repo).await?;
    Ok(Json(info))
}

async fn apply_update(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<github::ApplyResult>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let result = github::apply(
        &state.http,
        &cfg.github_owner,
        &cfg.github_repo,
        &cfg.www_dir,
    )
    .await?;
    drop(cfg);
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let _ = tokio::process::Command::new("systemctl")
            .args(["restart", "coduosd.service"])
            .status()
            .await;
    });
    Ok(Json(result))
}
