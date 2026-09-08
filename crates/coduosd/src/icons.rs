//! Freedesktop icon theme lookup for any installed Linux icon theme.
//!
//! Reversal (bundled with the UI) is always available as a fallback so CoduOS
//! still has colorful icons when the host has only symbolic/hicolor themes.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use serde::Serialize;

const BUNDLED_ID: &str = "Reversal";
const SYSTEM_ID: &str = "system";

#[derive(Clone, Debug, Serialize)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub bundled: bool,
}

#[derive(Clone, Debug)]
pub struct Lookup<'a> {
    pub theme: &'a str,
    pub context: &'a str,
    pub name: &'a str,
    pub size: u32,
    pub www_dir: &'a Path,
    pub extra_dirs: &'a [PathBuf],
}

struct ThemeIndex {
    name: String,
    comment: String,
    inherits: Vec<String>,
    hidden: bool,
    dirs: Vec<IconDir>,
}

struct IconDir {
    rel: String,
    size: u32,
    context: String,
    scalable: bool,
    symbolic: bool,
}

struct CachedIndex {
    mtime: Option<SystemTime>,
    index: ThemeIndex,
}

struct Cache {
    indexes: HashMap<PathBuf, CachedIndex>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache {
    indexes: HashMap::new(),
}));

pub fn valid_theme_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 64 {
        return false;
    }
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

pub fn valid_icon_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

pub fn valid_context(ctx: &str) -> bool {
    matches!(
        ctx,
        "places"
            | "mimes"
            | "mimetypes"
            | "apps"
            | "applications"
            | "devices"
            | "actions"
            | "categories"
            | "emblems"
            | "status"
            | "preferences"
    )
}

pub fn search_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        push_unique(&mut out, PathBuf::from(format!("{home}/.local/share/icons")));
        push_unique(&mut out, PathBuf::from(format!("{home}/.icons")));
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        push_unique(&mut out, PathBuf::from(xdg).join("icons"));
    }
    if let Ok(dirs) = std::env::var("XDG_DATA_DIRS") {
        for p in dirs.split(':').filter(|s| !s.is_empty()) {
            push_unique(&mut out, PathBuf::from(p).join("icons"));
        }
    }
    for p in extra {
        push_unique(&mut out, p.clone());
    }
    push_unique(&mut out, PathBuf::from("/usr/local/share/icons"));
    push_unique(&mut out, PathBuf::from("/usr/share/icons"));
    out.retain(|p| p.is_dir());
    out
}

pub fn bundled_dir(www_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        www_dir.join("icons/reversal"),
        PathBuf::from("web/dist/icons/reversal"),
        PathBuf::from("web/public/icons/reversal"),
    ];
    candidates.into_iter().find(|p| p.join("places/folder.svg").is_file())
}

pub fn list_themes(www_dir: &Path, extra: &[PathBuf]) -> Vec<ThemeInfo> {
    let mut themes = Vec::new();
    themes.push(ThemeInfo {
        id: SYSTEM_ID.into(),
        name: "System default".into(),
        comment: "Follows the host GTK or KDE icon theme".into(),
        bundled: false,
    });

    let mut seen = vec![SYSTEM_ID.to_string()];
    for root in search_roots(extra) {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let mut ids: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        ids.sort_by_key(|e| e.file_name());
        for ent in ids {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let id = match ent.file_name().into_string() {
                Ok(s) if valid_theme_id(&s) => s,
                _ => continue,
            };
            if seen.iter().any(|s| s.eq_ignore_ascii_case(&id)) {
                continue;
            }
            let Some(index) = load_index(&path) else {
                continue;
            };
            if index.hidden || index.dirs.is_empty() {
                continue;
            }
            seen.push(id.clone());
            themes.push(ThemeInfo {
                id,
                name: index.name,
                comment: index.comment,
                bundled: false,
            });
        }
    }

    if !seen.iter().any(|s| s.eq_ignore_ascii_case(BUNDLED_ID)) {
        if bundled_dir(www_dir).is_some() {
            themes.insert(
                1,
                ThemeInfo {
                    id: BUNDLED_ID.into(),
                    name: "Reversal".into(),
                    comment: "Bundled colorful icon theme".into(),
                    bundled: true,
                },
            );
        }
    }
    themes
}

pub fn theme_allowed(id: &str, www_dir: &Path, extra: &[PathBuf]) -> bool {
    if id.eq_ignore_ascii_case(SYSTEM_ID) || id.eq_ignore_ascii_case(BUNDLED_ID) {
        return true;
    }
    list_themes(www_dir, extra)
        .iter()
        .any(|t| t.id == id)
}

pub fn resolve(req: &Lookup<'_>) -> Option<PathBuf> {
    if !valid_icon_name(req.name) || !valid_context(req.context) {
        return None;
    }
    let theme = if req.theme.is_empty() {
        BUNDLED_ID
    } else {
        req.theme
    };
    let roots = search_roots(req.extra_dirs);
    let bundled = bundled_dir(req.www_dir);
    let effective = effective_theme(theme, &roots);
    let names = icon_aliases(req.name);
    let contexts = context_aliases(req.context);

    let mut chain = inherit_chain(&effective, &roots, bundled.as_deref());
    if !chain.iter().any(|c| c.id == BUNDLED_ID) {
        if let Some(dir) = bundled.clone() {
            chain.push(ThemeRef {
                id: BUNDLED_ID.into(),
                dir,
                bundled: true,
            });
        }
    }

    for theme_ref in &chain {
        for ctx in &contexts {
            for name in &names {
                if let Some(path) = lookup_in_theme(theme_ref, ctx, name, req.size) {
                    if path_allowed(&path, &roots, bundled.as_deref()) {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

struct ThemeRef {
    id: String,
    dir: PathBuf,
    bundled: bool,
}

fn effective_theme(requested: &str, roots: &[PathBuf]) -> String {
    if requested.eq_ignore_ascii_case(SYSTEM_ID) {
        host_icon_theme(roots).unwrap_or_else(|| BUNDLED_ID.to_string())
    } else {
        requested.to_string()
    }
}

fn inherit_chain(id: &str, roots: &[PathBuf], bundled: Option<&Path>) -> Vec<ThemeRef> {
    let mut out = Vec::new();
    let mut pending = vec![id.to_string()];
    let mut seen = Vec::new();
    while let Some(next) = pending.pop() {
        if seen.iter().any(|s: &String| s.eq_ignore_ascii_case(&next)) {
            continue;
        }
        seen.push(next.clone());
        let Some(dir) = theme_dir(&next, roots, bundled) else {
            continue;
        };
        let bundled_theme = bundled.is_some() && bundled.unwrap() == dir;
        let inherits = load_index(&dir)
            .map(|i| i.inherits)
            .unwrap_or_default();
        out.push(ThemeRef {
            id: next,
            dir: dir.to_path_buf(),
            bundled: bundled_theme,
        });
        for parent in inherits.into_iter().rev() {
            pending.push(parent);
        }
    }
    if !seen.iter().any(|s| s.eq_ignore_ascii_case("hicolor")) {
        if let Some(dir) = theme_dir("hicolor", roots, None) {
            out.push(ThemeRef {
                id: "hicolor".into(),
                dir: dir.to_path_buf(),
                bundled: false,
            });
        }
    }
    out
}

fn theme_dir(id: &str, roots: &[PathBuf], bundled: Option<&Path>) -> Option<PathBuf> {
    for root in roots {
        let p = root.join(id);
        if p.is_dir() {
            return Some(p);
        }
    }
    if id.eq_ignore_ascii_case(BUNDLED_ID) {
        return bundled.map(Path::to_path_buf);
    }
    None
}

fn lookup_in_theme(theme: &ThemeRef, context: &str, name: &str, size: u32) -> Option<PathBuf> {
    if theme.bundled || is_bundled_layout(&theme.dir) {
        if let Some(p) = lookup_flat(&theme.dir, context, name) {
            return Some(p);
        }
    }
    let index = load_index(&theme.dir);
    if let Some(index) = index {
        let mut best: Option<(i32, PathBuf)> = None;
        for dir in &index.dirs {
            if !context_matches(context, &dir.context, &dir.rel) {
                continue;
            }
            for ext in ["svg", "png"] {
                let path = theme.dir.join(&dir.rel).join(format!("{name}.{ext}"));
                if !path.is_file() {
                    continue;
                }
                let score = icon_score(dir, ext, size);
                match &best {
                    Some((best_score, _)) if *best_score >= score => {}
                    _ => best = Some((score, path)),
                }
            }
        }
        if let Some((_, path)) = best {
            return Some(path);
        }
    }
    lookup_common_paths(&theme.dir, context, name)
}

fn is_bundled_layout(dir: &Path) -> bool {
    dir.join("places/folder.svg").is_file() && !dir.join("index.theme").is_file()
}

fn lookup_flat(root: &Path, context: &str, name: &str) -> Option<PathBuf> {
    let ctx = fs_context(context);
    let p = root.join(ctx).join(format!("{name}.svg"));
    if p.is_file() {
        return Some(p);
    }
    for dir in ["places", "mimes", "apps", "devices", "actions", "categories"] {
        let p = root.join(dir).join(format!("{name}.svg"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn lookup_common_paths(root: &Path, context: &str, name: &str) -> Option<PathBuf> {
    let ctx = fs_context(context);
    let alt = match ctx {
        "mimes" => "mimetypes",
        "apps" => "applications",
        other => other,
    };
    let sizes = ["scalable", "64", "48", "32", "24", "22", "16", "64x64", "48x48", "32x32", "24x24"];
    let mut cands = Vec::new();
    for c in [ctx, alt] {
        for s in sizes {
            cands.push(root.join(c).join(s).join(format!("{name}.svg")));
            cands.push(root.join(s).join(c).join(format!("{name}.svg")));
            cands.push(root.join(c).join(s).join(format!("{name}.png")));
            cands.push(root.join(s).join(c).join(format!("{name}.png")));
        }
        cands.push(root.join(c).join(format!("{name}.svg")));
    }
    cands.into_iter().find(|p| p.is_file())
}

fn icon_score(dir: &IconDir, ext: &str, size: u32) -> i32 {
    let mut score = 0;
    if ext == "svg" {
        score += 1000;
    }
    if dir.symbolic {
        score -= 4000;
    }
    if dir.scalable {
        score += 200;
    }
    let diff = (dir.size as i32 - size as i32).abs();
    score -= diff;
    score
}

fn context_matches(want: &str, dir_ctx: &str, rel: &str) -> bool {
    let want_l = want.to_ascii_lowercase();
    let ctx_l = dir_ctx.to_ascii_lowercase();
    let rel_l = rel.to_ascii_lowercase();
    if ctx_l.is_empty() {
        return rel_l.contains(&fs_context(&want_l).to_string()) || rel_l.contains(&want_l);
    }
    let want_fs = fs_context(&want_l);
    spec_context(want_fs) == ctx_l
        || ctx_l == want_l
        || ctx_l == want_fs
        || (want_l == "mimes" && (ctx_l == "mimetypes" || ctx_l.contains("mime")))
        || (want_l == "apps" && ctx_l.contains("application"))
}

fn spec_context(ctx: &str) -> &'static str {
    match ctx {
        "places" => "places",
        "mimes" => "mimetypes",
        "apps" => "applications",
        "devices" => "devices",
        "actions" => "actions",
        "categories" => "categories",
        "emblems" => "emblems",
        "status" => "status",
        "preferences" => "preferences",
        _ => "",
    }
}

fn fs_context(ctx: &str) -> &str {
    match ctx {
        "mimetypes" => "mimes",
        "applications" => "apps",
        other => other,
    }
}

fn context_aliases(ctx: &str) -> Vec<String> {
    let base = fs_context(ctx).to_string();
    match base.as_str() {
        "mimes" => vec!["mimes".into(), "mimetypes".into()],
        "apps" => vec!["apps".into(), "applications".into(), "preferences".into()],
        other => vec![other.to_string()],
    }
}

fn icon_aliases(name: &str) -> Vec<String> {
    let mut out = vec![name.to_string()];
    let extra: &[&str] = match name {
        "folder" => &["inode-directory"],
        "folder-images" => &["folder-pictures", "folder-image", "folder-photo"],
        "folder-download" => &["folder-downloads"],
        "folder-videos" => &["folder-video", "folder-movies"],
        "folder-music" => &["folder-sound"],
        "file-manager" => &["system-file-manager", "org.gnome.Nautilus"],
        "preferences-system" => &["gnome-settings", "org.gnome.Settings", "preferences-desktop"],
        "applications-other" => &["applications-system", "applications-all"],
        "applications-system" => &["applications-utilities", "preferences-system"],
        "system-software-install" => &["system-software-update", "softwarecenter"],
        "docker" => &["folder-docker"],
        "unknown" => &["application-octet-stream", "application-x-generic", "text-x-generic"],
        "computer" => &["computer-laptop", "video-display"],
        "drive-harddisk" => &["drive-harddisk-system", "gnome-dev-harddisk"],
        "text-x-generic" => &["text-plain", "text-x-generic-template"],
        _ => &[],
    };
    for e in extra {
        out.push((*e).to_string());
    }
    out
}

fn load_index(dir: &Path) -> Option<ThemeIndex> {
    let path = dir.join("index.theme");
    if !path.is_file() {
        return None;
    }
    let mtime = fs::metadata(&path).ok().and_then(|m| m.modified().ok());
    if let Ok(cache) = CACHE.lock() {
        if let Some(hit) = cache.indexes.get(&path) {
            if hit.mtime == mtime {
                return Some(clone_index(&hit.index));
            }
        }
    }
    let raw = fs::read_to_string(&path).ok()?;
    let index = parse_index(&raw)?;
    if let Ok(mut cache) = CACHE.lock() {
        cache.indexes.insert(
            path,
            CachedIndex {
                mtime,
                index: clone_index(&index),
            },
        );
    }
    Some(index)
}

fn clone_index(i: &ThemeIndex) -> ThemeIndex {
    ThemeIndex {
        name: i.name.clone(),
        comment: i.comment.clone(),
        inherits: i.inherits.clone(),
        hidden: i.hidden,
        dirs: i
            .dirs
            .iter()
            .map(|d| IconDir {
                rel: d.rel.clone(),
                size: d.size,
                context: d.context.clone(),
                scalable: d.scalable,
                symbolic: d.symbolic,
            })
            .collect(),
    }
}

fn parse_index(raw: &str) -> Option<ThemeIndex> {
    let sections = parse_ini(raw);
    let theme = sections.iter().find(|(n, _)| n.eq_ignore_ascii_case("Icon Theme"))?;
    let map = &theme.1;
    let name = map.get("Name").cloned().unwrap_or_default();
    let comment = map.get("Comment").cloned().unwrap_or_default();
    let hidden = map
        .get("Hidden")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let inherits = map
        .get("Inherits")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let listed = map
        .get("Directories")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    if listed.is_empty() && name.is_empty() {
        return None;
    }
    let mut dirs = Vec::new();
    for rel in listed {
        if rel.to_ascii_lowercase().contains("cursor") {
            continue;
        }
        let sec = sections
            .iter()
            .find(|(n, _)| n == &rel)
            .map(|(_, kvs)| kvs);
        let size = sec
            .and_then(|m| m.get("Size"))
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| infer_size(&rel));
        let context = sec
            .and_then(|m| m.get("Context"))
            .cloned()
            .unwrap_or_else(|| infer_context(&rel));
        let type_s = sec
            .and_then(|m| m.get("Type"))
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let scalable = type_s == "scalable" || rel.to_ascii_lowercase().contains("scalable");
        let symbolic = rel.to_ascii_lowercase().contains("symbolic");
        dirs.push(IconDir {
            rel,
            size,
            context,
            scalable,
            symbolic,
        });
    }
    if dirs.is_empty() {
        return None;
    }
    Some(ThemeIndex {
        name: if name.is_empty() { "Icon theme".into() } else { name },
        comment,
        inherits,
        hidden,
        dirs,
    })
}

fn infer_size(rel: &str) -> u32 {
    for part in rel.split(['/', '@']) {
        if part == "scalable" || part == "symbolic" {
            return 64;
        }
        if let Some(n) = part.strip_suffix("x") {
            if let Ok(v) = n.parse() {
                return v;
            }
        }
        if let Some((w, _)) = part.split_once('x') {
            if let Ok(v) = w.parse() {
                return v;
            }
        }
        if let Ok(v) = part.parse() {
            return v;
        }
    }
    48
}

fn infer_context(rel: &str) -> String {
    let l = rel.to_ascii_lowercase();
    for (key, ctx) in [
        ("places", "Places"),
        ("mimetypes", "MimeTypes"),
        ("mimes", "MimeTypes"),
        ("apps", "Applications"),
        ("applications", "Applications"),
        ("devices", "Devices"),
        ("actions", "Actions"),
        ("categories", "Categories"),
        ("emblems", "Emblems"),
        ("status", "Status"),
        ("preferences", "Preferences"),
    ] {
        if l.split('/').any(|p| p == key) {
            return ctx.into();
        }
    }
    String::new()
}

fn parse_ini(raw: &str) -> Vec<(String, HashMap<String, String>)> {
    let mut sections = Vec::new();
    let mut current: Option<(String, HashMap<String, String>)> = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            if let Some(name) = rest.strip_suffix(']') {
                if let Some(prev) = current.take() {
                    sections.push(prev);
                }
                current = Some((name.to_string(), HashMap::new()));
                continue;
            }
        }
        if let Some((k, v)) = line.split_once('=') {
            if let Some((_, map)) = current.as_mut() {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    if let Some(prev) = current {
        sections.push(prev);
    }
    sections
}

fn host_icon_theme(roots: &[PathBuf]) -> Option<String> {
    let mut files = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        files.push(PathBuf::from(format!("{home}/.config/gtk-3.0/settings.ini")));
        files.push(PathBuf::from(format!("{home}/.config/gtk-4.0/settings.ini")));
        files.push(PathBuf::from(format!("{home}/.config/kdeglobals")));
    }
    files.push(PathBuf::from("/etc/gtk-3.0/settings.ini"));
    files.push(PathBuf::from("/etc/gtk-4.0/settings.ini"));
    for file in files {
        let Ok(raw) = fs::read_to_string(&file) else {
            continue;
        };
        if let Some(name) = gtk_icon_name(&raw).or_else(|| kde_icon_name(&raw)) {
            if theme_dir(&name, roots, None).is_some() {
                return Some(name);
            }
        }
    }
    None
}

fn gtk_icon_name(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("gtk-icon-theme-name=") {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if valid_theme_id(v) {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn kde_icon_name(raw: &str) -> Option<String> {
    let mut in_icons = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.eq_ignore_ascii_case("[Icons]") {
            in_icons = true;
            continue;
        }
        if line.starts_with('[') {
            in_icons = false;
            continue;
        }
        if in_icons {
            if let Some(v) = line.strip_prefix("Theme=") {
                let v = v.trim();
                if valid_theme_id(v) {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn path_allowed(path: &Path, roots: &[PathBuf], bundled: Option<&Path>) -> bool {
    let Ok(canon) = fs::canonicalize(path) else {
        return false;
    };
    if let Some(b) = bundled {
        if let Ok(b) = fs::canonicalize(b) {
            if canon.starts_with(&b) {
                return true;
            }
        }
    }
    roots.iter().any(|root| {
        fs::canonicalize(root)
            .map(|r| canon.starts_with(&r))
            .unwrap_or(false)
    })
}

fn push_unique(out: &mut Vec<PathBuf>, path: PathBuf) {
    if !out.iter().any(|p| p == &path) {
        out.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reversal() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/public/icons/reversal")
    }

    #[test]
    fn rejects_traversal() {
        assert!(!valid_icon_name("../etc/passwd"));
        assert!(!valid_icon_name("foo/bar"));
        assert!(!valid_theme_id("../Adwaita"));
        assert!(valid_icon_name("image-svg+xml"));
        assert!(valid_icon_name("application-vnd.ms-excel"));
    }

    #[test]
    fn bundled_folder() {
        let www = reversal().parent().unwrap().parent().unwrap().to_path_buf();
        let extra: [PathBuf; 0] = [];
        let path = resolve(&Lookup {
            theme: "Reversal",
            context: "places",
            name: "folder",
            size: 48,
            www_dir: &www,
            extra_dirs: &extra,
        })
        .expect("bundled folder icon");
        assert!(path.ends_with("places/folder.svg"));
    }

    #[test]
    fn parse_whitesur_like_index() {
        let raw = r#"
[Icon Theme]
Name=Demo
Comment=Test
Inherits=hicolor
Directories=places/scalable,places/symbolic,mimes/48

[places/scalable]
Size=64
Context=Places
Type=Scalable

[places/symbolic]
Size=16
Context=Places
Type=Fixed

[mimes/48]
Size=48
Context=MimeTypes
Type=Fixed
"#;
        let idx = parse_index(raw).unwrap();
        assert_eq!(idx.name, "Demo");
        assert_eq!(idx.inherits, vec!["hicolor"]);
        assert_eq!(idx.dirs.len(), 3);
        assert!(idx.dirs[0].scalable);
        assert!(idx.dirs[1].symbolic);
    }

    #[test]
    fn list_includes_bundled() {
        let www = reversal().parent().unwrap().parent().unwrap().to_path_buf();
        let extra: [PathBuf; 0] = [];
        let themes = list_themes(&www, &extra);
        assert!(themes.iter().any(|t| t.id == "system"));
        assert!(themes.iter().any(|t| t.id == "Reversal" && t.bundled));
    }
}
