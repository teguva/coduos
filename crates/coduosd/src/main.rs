mod auth;
mod battery;
mod config;
mod db;
mod ddns;
mod docker;
mod error;
mod github;
mod jail;
mod proxy;
mod routes;
mod state;
mod stats;
mod storage;
mod systemd;
mod util;
mod vpn;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use clap::Parser;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::state::AppState;

#[derive(Parser, Debug)]
#[command(name = "coduosd", about = "CoduOS dashboard daemon", version)]
struct Cli {
    #[arg(short, long, env = "CODUOS_CONFIG")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let (cfg, config_path) = config::Config::load(cli.config.as_deref())?;
    std::fs::create_dir_all(&cfg.data_dir)
        .with_context(|| format!("create data dir {}", cfg.data_dir.display()))?;
    std::fs::create_dir_all(cfg.apps_dir())?;
    std::fs::create_dir_all(cfg.icons_dir())?;
    vpn::ensure_dirs(&cfg);
    proxy::ensure_dirs(&cfg);
    ddns::ensure_dirs(&cfg);
    seed_icons_readme(&cfg.icons_dir());
    storage::remount_persisted(&cfg);
    battery::apply_persisted(&cfg);

    if !config_path.exists() {
        if let Err(err) = cfg.save(&config_path) {
            tracing::warn!("could not write {}: {err}", config_path.display());
        }
    }

    let db = Arc::new(db::Db::open(&cfg.db_path())?);
    let (summary_tx, summary_rx) = stats::channel();
    stats::spawn_collector(summary_tx.clone());

    let http = reqwest::Client::builder()
        .user_agent(format!("CoduOS/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let bind = cfg.bind.clone();
    let state = AppState {
        config: Arc::new(RwLock::new(cfg)),
        config_path,
        db,
        summary_tx,
        summary_rx,
        http,
    };
    ddns::spawn_updater(state.clone());

    let app = Router::new()
        .nest("/api", routes::router())
        .fallback(spa)
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = bind.parse().context("invalid bind address")?;
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(err) if addr.port() == 80 => {
            tracing::warn!("bind {addr} failed ({err}); falling back to 0.0.0.0:13209");
            tokio::net::TcpListener::bind("0.0.0.0:13209").await?
        }
        Err(err) => return Err(err.into()),
    };
    tracing::info!("CoduOS listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn spa(State(state): State<AppState>, uri: Uri, req: Request) -> Response {
    if uri.path().starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"error":"not found","code":"not_found"})),
        )
            .into_response();
    }
    if uri.path().starts_with("/icons/") {
        return serve_icon(&state, uri.path()).await;
    }
    let www = resolve_www(&state).await;
    let rel = uri.path().trim_start_matches('/');
    if !rel.is_empty() {
        if let Some(file) = safe_join(&www, rel) {
            if file.is_file() {
                return send_file(file).await;
            }
        }
    }
    let index = www.join("index.html");
    if index.is_file() {
        return send_file(index).await;
    }
    let _ = req;
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!doctype html><html><body style=\"font-family:sans-serif;padding:2rem\"><h1>CoduOS</h1><p>API is running. Build the UI with <code>npm run build</code> in <code>web/</code>.</p></body></html>",
    )
        .into_response()
}

async fn resolve_www(state: &AppState) -> PathBuf {
    let cfg_www = state.config.read().await.www_dir.clone();
    if cfg_www.join("index.html").is_file() {
        return cfg_www;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("www");
            if cand.join("index.html").is_file() {
                return cand;
            }
        }
    }
    let local = PathBuf::from("web/dist");
    if local.join("index.html").is_file() {
        return local;
    }
    cfg_www
}

async fn serve_icon(state: &AppState, path: &str) -> Response {
    let rel = path.trim_start_matches("/icons/");
    if rel.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let cfg = state.config.read().await;
    if let Some(file) = safe_join(&cfg.icons_dir(), rel) {
        if file.is_file() {
            return send_file_with_cache(file, "no-cache").await;
        }
    }
    drop(cfg);
    let www = resolve_www(state).await;
    if let Some(file) = safe_join(&www.join("icons"), rel) {
        if file.is_file() {
            return send_file_with_cache(file, "no-cache").await;
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

fn seed_icons_readme(dir: &PathBuf) {
    let readme = dir.join("README.txt");
    if readme.exists() {
        return;
    }
    let body = "\
CoduOS icons (runtime overlay)
==============================
Files here are served at /icons/ and override bundled icons in the UI.

Examples:
  jellyfin.svg     desktop icon for an app whose id is jellyfin
  mkv.svg          .mkv files in the file explorer
  folder-media.svg folder named Media
  files.svg        replace the Files app icon

SVG, PNG, or WebP. Refresh the browser after adding or replacing a file.
";
    let _ = std::fs::write(readme, body);
}

fn safe_join(root: &PathBuf, rel: &str) -> Option<PathBuf> {
    let mut out = root.clone();
    for comp in std::path::Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(c) => out.push(c),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    if out.starts_with(root) {
        Some(out)
    } else {
        None
    }
}

async fn send_file(path: PathBuf) -> Response {
    send_file_with_cache(path, "public, max-age=60").await
}

async fn send_file_with_cache(path: PathBuf, cache: &str) -> Response {
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, cache)
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
