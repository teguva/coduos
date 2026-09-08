use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::error::ApiError;
use crate::jail;
use crate::state::AppState;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/files", get(list))
        .route("/files/mkdir", post(mkdir))
        .route("/files/rename", post(rename))
        .route("/files/delete", post(delete))
        .route("/files/upload", post(upload))
        .route("/files/download", get(download))
}

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
}

#[derive(Serialize)]
struct RootOut {
    id: String,
    label: String,
    path: String,
}

#[derive(Serialize)]
struct Entry {
    name: String,
    path: String,
    dir: bool,
    size: u64,
    modified: Option<i64>,
}

#[derive(Serialize)]
struct ListOut {
    root: String,
    path: String,
    roots: Vec<RootOut>,
    entries: Vec<Entry>,
    favorites: Vec<crate::config::FileFavorite>,
    space: Option<SpaceOut>,
}

#[derive(Serialize)]
struct SpaceOut {
    used: u64,
    total: u64,
}

async fn resolve(state: &AppState, root_id: &str, rel: &str) -> Result<PathBuf, ApiError> {
    let cfg = state.config.read().await;
    let root = cfg
        .file_root(root_id)
        .ok_or(ApiError::BadRequest("unknown file root".into()))?;
    jail::resolve_in_root(&root.path, rel)
}

async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(q): Query<PathQuery>,
) -> Result<Json<ListOut>, ApiError> {
    current_user(&state, &jar).await?;
    let cfg = state.config.read().await;
    let roots: Vec<RootOut> = cfg
        .file_roots
        .iter()
        .map(|r| RootOut {
            id: r.id.clone(),
            label: r.label.clone(),
            path: r.path.display().to_string(),
        })
        .collect();
    let favorites = cfg.file_favorites.clone();
    let root_id = match q.root.clone().filter(|s| !s.is_empty()) {
        Some(id) => id,
        None => {
            return Ok(Json(ListOut {
                root: String::new(),
                path: String::new(),
                roots,
                entries: vec![],
                favorites,
                space: None,
            }));
        }
    };
    let root = cfg
        .file_root(&root_id)
        .ok_or(ApiError::BadRequest("unknown file root".into()))?;
    let dir = jail::resolve_in_root(&root.path, &q.path)?;
    drop(cfg);
    if !dir.is_dir() {
        return Err(ApiError::BadRequest("not a directory".into()));
    }
    let mut entries = Vec::new();
    for ent in std::fs::read_dir(&dir)? {
        let ent = ent?;
        let meta = match ent.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = ent.file_name().to_string_lossy().into_owned();
        let rel = if q.path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", q.path.trim_end_matches('/'), name)
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        entries.push(Entry {
            name,
            path: rel,
            dir: meta.is_dir(),
            size: meta.len(),
            modified,
        });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let space = disk_space(&dir);
    Ok(Json(ListOut {
        root: root_id,
        path: q.path,
        roots,
        entries,
        favorites,
        space,
    }))
}

fn disk_space(path: &std::path::Path) -> Option<SpaceOut> {
    let cstr = std::ffi::CString::new(path.to_str()?).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(cstr.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let fr = s.f_frsize as u64;
    if fr == 0 {
        return None;
    }
    let total = s.f_blocks.saturating_mul(fr);
    let avail = s.f_bavail.saturating_mul(fr);
    if total == 0 {
        return None;
    }
    Some(SpaceOut {
        used: total.saturating_sub(avail),
        total,
    })
}

#[derive(Deserialize)]
struct MkdirIn {
    root: String,
    path: String,
    name: String,
}

async fn mkdir(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<MkdirIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    if body.name.contains('/') || body.name.contains('\0') || body.name == "." || body.name == ".."
    {
        return Err(ApiError::BadRequest("invalid folder name".into()));
    }
    let rel = if body.path.is_empty() {
        body.name.clone()
    } else {
        format!("{}/{}", body.path.trim_end_matches('/'), body.name)
    };
    let path = resolve(&state, &body.root, &rel).await?;
    std::fs::create_dir(&path)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct RenameIn {
    root: String,
    from: String,
    to: String,
}

async fn rename(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<RenameIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let from = resolve(&state, &body.root, &body.from).await?;
    let to = resolve(&state, &body.root, &body.to).await?;
    std::fs::rename(from, to)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct DeleteIn {
    root: String,
    path: String,
}

async fn delete(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<DeleteIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let path = resolve(&state, &body.root, &body.path).await?;
    let meta = std::fs::metadata(&path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn upload(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(q): Query<PathQuery>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let root = q
        .root
        .as_deref()
        .ok_or(ApiError::BadRequest("root required".into()))?;
    let dir = resolve(&state, root, &q.path).await?;
    if !dir.is_dir() {
        return Err(ApiError::BadRequest("not a directory".into()));
    }
    let mut saved = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
    {
        let name = field
            .file_name()
            .or(field.name())
            .unwrap_or("upload")
            .to_string();
        let name = std::path::Path::new(&name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("upload")
            .to_string();
        let dest_rel = if q.path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", q.path.trim_end_matches('/'), name)
        };
        let dest = resolve(&state, root, &dest_rel).await?;
        let data = field
            .bytes()
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
        let mut f = tokio::fs::File::create(&dest).await?;
        f.write_all(&data).await?;
        saved.push(name);
    }
    Ok(Json(serde_json::json!({ "ok": true, "files": saved })))
}

async fn download(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(q): Query<PathQuery>,
) -> Result<Response, ApiError> {
    current_user(&state, &jar).await?;
    let root = q
        .root
        .as_deref()
        .ok_or(ApiError::BadRequest("root required".into()))?;
    let path = resolve(&state, root, &q.path).await?;
    if !path.is_file() {
        return Err(ApiError::BadRequest("not a file".into()));
    }
    let data = tokio::fs::read(&path).await?;
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let disp = format!("attachment; filename=\"{filename}\"");
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime)
        .header(CONTENT_DISPOSITION, disp)
        .body(Body::from(data))
        .map_err(ApiError::internal)
}
