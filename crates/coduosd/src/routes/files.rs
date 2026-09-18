use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::error::ApiError;
use crate::jail;
use crate::state::AppState;
use crate::storage;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/files", get(list))
        .route("/files/search", get(search))
        .route("/files/mkdir", post(mkdir))
        .route("/files/rename", post(rename))
        .route("/files/copy", post(copy))
        .route("/files/delete", post(delete))
        .route("/files/upload", post(upload))
        .route("/files/download", get(download))
}

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    pub root: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub inline: bool,
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
    if !storage::file_root_available(&root.path) {
        return Err(ApiError::BadRequest("this location is not mounted".into()));
    }
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
        .filter(|r| storage::file_root_available(&r.path))
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
    if !storage::file_root_available(&root.path) {
        return Ok(Json(ListOut {
            root: String::new(),
            path: String::new(),
            roots,
            entries: vec![],
            favorites,
            space: None,
        }));
    }
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

const SEARCH_LIMIT: usize = 250;
const SEARCH_VISIT: usize = 40_000;

#[derive(Debug, Deserialize)]
struct SearchQuery {
    pub root: String,
    #[serde(default)]
    pub path: String,
    pub q: String,
}

#[derive(Serialize)]
struct SearchOut {
    root: String,
    path: String,
    query: String,
    entries: Vec<Entry>,
    truncated: bool,
}

async fn search(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(q): Query<SearchQuery>,
) -> Result<Json<SearchOut>, ApiError> {
    current_user(&state, &jar).await?;
    let needle = q.q.trim().to_lowercase();
    if needle.chars().count() < 2 {
        return Err(ApiError::BadRequest(
            "type at least 2 characters to search subfolders".into(),
        ));
    }
    let dir = resolve(&state, &q.root, &q.path).await?;
    if !dir.is_dir() {
        return Err(ApiError::BadRequest("not a directory".into()));
    }
    let start_rel = q.path.trim_end_matches('/').to_string();
    let needle_out = needle.clone();
    let (entries, truncated) = tokio::task::spawn_blocking(move || {
        search_tree(&dir, &start_rel, &needle, SEARCH_LIMIT, SEARCH_VISIT)
    })
    .await
    .map_err(|e| ApiError::internal(e))?;
    Ok(Json(SearchOut {
        root: q.root,
        path: q.path,
        query: needle_out,
        entries,
        truncated,
    }))
}

fn skip_search_dir(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "node_modules" | "__pycache__" | "lost+found"
    )
}

fn search_tree(
    start: &Path,
    start_rel: &str,
    needle: &str,
    limit: usize,
    visit_cap: usize,
) -> (Vec<Entry>, bool) {
    let mut out = Vec::new();
    let mut visits = 0usize;
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((start.to_path_buf(), start_rel.to_string()));
    while let Some((dir, rel)) = queue.pop_front() {
        if out.len() >= limit || visits >= visit_cap {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd {
            if visits >= visit_cap || out.len() >= limit {
                break;
            }
            visits += 1;
            let Ok(ent) = ent else { continue };
            let name = ent.file_name().to_string_lossy().into_owned();
            if name == "." || name == ".." {
                continue;
            }
            let Ok(meta) = ent.metadata() else { continue };
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let is_dir = meta.is_dir();
            let is_link = meta.file_type().is_symlink();
            if name.to_lowercase().contains(needle) {
                out.push(file_entry(name.clone(), child_rel.clone(), &meta));
            }
            if is_dir && !is_link && !skip_search_dir(&name) {
                queue.push_back((ent.path(), child_rel));
            }
        }
    }
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then(a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
    let truncated = out.len() >= limit || visits >= visit_cap;
    if out.len() > limit {
        out.truncate(limit);
    }
    (out, truncated)
}

fn file_entry(name: String, rel: String, meta: &std::fs::Metadata) -> Entry {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    Entry {
        name,
        path: rel,
        dir: meta.is_dir(),
        size: meta.len(),
        modified,
    }
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
    #[serde(default)]
    unique: bool,
}

async fn rename(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<RenameIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let from = resolve(&state, &body.root, &body.from).await?;
    let mut to = resolve(&state, &body.root, &body.to).await?;
    if to.exists() {
        if body.unique {
            to = unique_path(&to);
        } else {
            return Err(ApiError::Conflict("destination already exists".into()));
        }
    }
    if from.is_dir() && to.starts_with(&from) {
        return Err(ApiError::BadRequest("cannot move a folder into itself".into()));
    }
    std::fs::rename(from, to)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
struct CopyIn {
    root: String,
    from: String,
    to: String,
}

async fn copy(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<CopyIn>,
) -> Result<Json<serde_json::Value>, ApiError> {
    current_user(&state, &jar).await?;
    let from = resolve(&state, &body.root, &body.from).await?;
    let mut to = resolve(&state, &body.root, &body.to).await?;
    if to.exists() {
        to = unique_path(&to);
    }
    if from.is_dir() && to.starts_with(&from) {
        return Err(ApiError::BadRequest("cannot copy a folder into itself".into()));
    }
    tokio::task::spawn_blocking(move || copy_entry(&from, &to))
        .await
        .map_err(ApiError::internal)??;
    Ok(Json(serde_json::json!({"ok": true})))
}

fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for i in 1..10_000 {
        let name = match ext {
            Some(ext) => format!("{stem} ({i}).{ext}"),
            None => format!("{stem} ({i})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem} copy"))
}

fn copy_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = from.symlink_metadata()?;
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for ent in std::fs::read_dir(from)? {
            let ent = ent?;
            copy_entry(&ent.path(), &to.join(ent.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
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
    headers: HeaderMap,
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
    stream_file(&path, q.inline, headers.get(RANGE)).await
}

async fn stream_file(
    path: &Path,
    inline: bool,
    range_header: Option<&HeaderValue>,
) -> Result<Response, ApiError> {
    let meta = tokio::fs::metadata(path).await?;
    let len = meta.len();
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download")
        .replace(['"', '\\'], "_");
    let disp = if inline {
        format!("inline; filename=\"{filename}\"")
    } else {
        format!("attachment; filename=\"{filename}\"")
    };

    let range = if len == 0 {
        None
    } else {
        parse_byte_range(range_header, len)?
    };

    let mut file = tokio::fs::File::open(path).await?;
    let (status, start, take) = if let Some((start, end)) = range {
        (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
    } else {
        (StatusCode::OK, 0, len)
    };
    file.seek(SeekFrom::Start(start)).await?;
    let stream = ReaderStream::new(file.take(take));
    let mut builder = Response::builder()
        .status(status)
        .header(CONTENT_TYPE, mime)
        .header(CONTENT_DISPOSITION, disp)
        .header(ACCEPT_RANGES, "bytes")
        .header(CONTENT_LENGTH, take.to_string());
    if let Some((start, end)) = range {
        builder = builder.header(CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(ApiError::internal)
}

fn parse_byte_range(
    header: Option<&HeaderValue>,
    len: u64,
) -> Result<Option<(u64, u64)>, ApiError> {
    let Some(val) = header else {
        return Ok(None);
    };
    let s = val
        .to_str()
        .map_err(|_| ApiError::BadRequest("invalid range".into()))?;
    let Some(spec) = s.strip_prefix("bytes=") else {
        return Err(ApiError::BadRequest("invalid range".into()));
    };
    let first = spec.split(',').next().unwrap_or(spec).trim();
    if first.is_empty() || len == 0 {
        return Ok(None);
    }
    let (start, end) = if let Some(suffix) = first.strip_prefix('-') {
        let n: u64 = suffix
            .parse()
            .map_err(|_| ApiError::BadRequest("invalid range".into()))?;
        if n == 0 {
            return Ok(None);
        }
        let n = n.min(len);
        (len - n, len - 1)
    } else {
        let mut parts = first.splitn(2, '-');
        let start: u64 = parts
            .next()
            .unwrap_or("")
            .parse()
            .map_err(|_| ApiError::BadRequest("invalid range".into()))?;
        let end = match parts.next() {
            Some("") | None => len.saturating_sub(1),
            Some(e) => e
                .parse()
                .map_err(|_| ApiError::BadRequest("invalid range".into()))?,
        };
        if start >= len || start > end {
            return Err(ApiError::BadRequest("invalid range".into()));
        }
        (start, end.min(len.saturating_sub(1)))
    };
    Ok(Some((start, end)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn unique_path_appends_number() {
        let dir = std::env::temp_dir().join(format!("coduos-unique-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("photo.jpg");
        fs::write(&file, b"a").unwrap();
        let next = unique_path(&file);
        assert_eq!(next.file_name().unwrap(), "photo (1).jpg");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_range_suffix_and_span() {
        let val = HeaderValue::from_static("bytes=2-5");
        assert_eq!(parse_byte_range(Some(&val), 10).unwrap(), Some((2, 5)));
        let suffix = HeaderValue::from_static("bytes=-3");
        assert_eq!(parse_byte_range(Some(&suffix), 10).unwrap(), Some((7, 9)));
        let open = HeaderValue::from_static("bytes=8-");
        assert_eq!(parse_byte_range(Some(&open), 10).unwrap(), Some((8, 9)));
        assert_eq!(parse_byte_range(None, 10).unwrap(), None);
    }

    #[test]
    fn search_tree_finds_nested_file_and_folder() {
        let dir = std::env::temp_dir().join(format!(
            "coduos-search-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("photos/2024")).unwrap();
        fs::write(dir.join("photos/2024/beach.jpg"), b"a").unwrap();
        fs::write(dir.join("readme.txt"), b"b").unwrap();
        fs::create_dir_all(dir.join("photos/library")).unwrap();
        let (hits, truncated) = search_tree(&dir, "", "beach", 50, 1000);
        let (folders, _) = search_tree(&dir, "", "libr", 50, 1000);
        let _ = fs::remove_dir_all(&dir);
        assert!(!truncated);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "photos/2024/beach.jpg");
        assert!(!hits[0].dir);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, "photos/library");
        assert!(folders[0].dir);
    }

    #[test]
    fn search_tree_from_subdir_stays_scoped() {
        let dir = std::env::temp_dir().join(format!(
            "coduos-search-scope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("keep/nested")).unwrap();
        fs::create_dir_all(dir.join("skip")).unwrap();
        fs::write(dir.join("keep/nested/note.txt"), b"a").unwrap();
        fs::write(dir.join("skip/note.txt"), b"b").unwrap();
        let (hits, _) = search_tree(&dir.join("keep"), "keep", "note", 50, 1000);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "keep/nested/note.txt");
    }

    #[test]
    fn search_tree_skips_node_modules() {
        let dir = std::env::temp_dir().join(format!(
            "coduos-search-skip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::write(dir.join("node_modules/pkg/hit.txt"), b"a").unwrap();
        fs::create_dir_all(dir.join("photos")).unwrap();
        fs::write(dir.join("photos/hit.txt"), b"b").unwrap();
        let (hits, _) = search_tree(&dir, "", "hit", 50, 1000);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "photos/hit.txt");
    }
}
