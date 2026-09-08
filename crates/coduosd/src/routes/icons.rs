use std::collections::BTreeMap;
use std::path::Path;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/icons", get(list))
}

#[derive(Serialize)]
struct IconsOut {
    files: Vec<String>,
}

async fn list(State(state): State<AppState>) -> Json<IconsOut> {
    let cfg = state.config.read().await;
    let mut by_stem: BTreeMap<String, String> = BTreeMap::new();
    collect_dir(&Path::new("web/public/icons"), &mut by_stem);
    collect_dir(&cfg.www_dir.join("icons"), &mut by_stem);
    collect_dir(&Path::new("web/dist/icons"), &mut by_stem);
    collect_dir(&cfg.icons_dir(), &mut by_stem);
    Json(IconsOut {
        files: by_stem.into_values().collect(),
    })
}

fn collect_dir(dir: &Path, out: &mut BTreeMap<String, String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(
            ext.as_str(),
            "svg" | "png" | "webp" | "jpg" | "jpeg" | "gif" | "ico"
        ) {
            continue;
        }
        let key = stem.to_ascii_lowercase();
        if let Some(prev) = out.get(&key) {
            if prev.ends_with(".svg") && ext != "svg" {
                continue;
            }
        }
        out.insert(key, name.to_string());
    }
}
