use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::util;

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: Option<String>,
    pub html_url: Option<String>,
    pub up_to_date: bool,
    pub can_apply: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApplyResult {
    pub ok: bool,
    pub version: String,
    pub restarting: bool,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

pub fn running_installed_bin() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let exe = exe.canonicalize().unwrap_or(exe);
    let installed = PathBuf::from("/usr/bin/coduosd");
    let installed = installed.canonicalize().unwrap_or(installed);
    exe == installed
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn target_arch() -> Result<&'static str, ApiError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        "arm" => Ok("arm-7"),
        other => Err(ApiError::BadRequest(format!("unsupported architecture: {other}"))),
    }
}

fn parse_latest(tag: &str) -> String {
    tag.trim_start_matches('v').to_string()
}

fn up_to_date(current: &str, tag: &str) -> bool {
    let latest = parse_latest(tag);
    latest == current || tag == format!("v{current}")
}

pub async fn check(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
) -> Result<UpdateInfo, ApiError> {
    let current = current_version();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let res = client
        .get(&url)
        .header("user-agent", format!("CoduOS/{current}"))
        .header("accept", "application/vnd.github+json")
        .send()
        .await;
    match res {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.json::<GhRelease>().await.map_err(ApiError::internal)?;
            let latest = parse_latest(&body.tag_name);
            let current_is_latest = up_to_date(&current, &body.tag_name);
            Ok(UpdateInfo {
                can_apply: util::privileged()
                    && running_installed_bin()
                    && !current_is_latest
                    && !latest.is_empty(),
                up_to_date: current_is_latest,
                latest: Some(latest),
                html_url: Some(body.html_url),
                current,
                error: None,
            })
        }
        Ok(resp) => Ok(UpdateInfo {
            current,
            latest: None,
            html_url: None,
            up_to_date: true,
            can_apply: false,
            error: Some(format!("GitHub HTTP {}", resp.status())),
        }),
        Err(err) => Ok(UpdateInfo {
            current,
            latest: None,
            html_url: None,
            up_to_date: true,
            can_apply: false,
            error: Some(err.to_string()),
        }),
    }
}

async fn fetch_latest(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
) -> Result<GhRelease, ApiError> {
    let current = current_version();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let resp = client
        .get(&url)
        .header("user-agent", format!("CoduOS/{current}"))
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("GitHub: {e}")))?;
    if !resp.status().is_success() {
        return Err(ApiError::BadRequest(format!(
            "GitHub HTTP {}",
            resp.status()
        )));
    }
    resp.json::<GhRelease>()
        .await
        .map_err(|e| ApiError::BadRequest(format!("GitHub release json: {e}")))
}

fn pick_asset<'a>(rel: &'a GhRelease, arch: &str) -> Result<&'a GhAsset, ApiError> {
    let ver = parse_latest(&rel.tag_name);
    let want = format!("linux-{arch}-coduos-v{ver}.tar.gz");
    rel.assets
        .iter()
        .find(|a| a.name == want)
        .ok_or_else(|| {
            ApiError::BadRequest(format!("release has no asset {want}"))
        })
}

fn allowed_url(owner: &str, repo: &str, url: &str) -> bool {
    let prefix = format!("https://github.com/{owner}/{repo}/releases/download/");
    url.starts_with(&prefix)
}

fn extract_root(tmp: &Path) -> Result<PathBuf, ApiError> {
    let direct = tmp.join("usr/bin/coduosd");
    if direct.is_file() {
        return Ok(tmp.to_path_buf());
    }
    let rd = std::fs::read_dir(tmp)?;
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() && p.join("usr/bin/coduosd").is_file() {
            return Ok(p);
        }
    }
    Err(ApiError::BadRequest("tarball is missing usr/bin/coduosd".into()))
}

fn install_file(src: &Path, dest: &Path, mode: u32) -> Result<(), ApiError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_file_name({
        let mut name = dest
            .file_name()
            .unwrap_or_default()
            .to_os_string();
        name.push(".new");
        name
    });
    std::fs::copy(src, &tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

fn copy_www(src: &Path, dest: &Path) -> Result<(), ApiError> {
    if !src.is_dir() {
        return Err(ApiError::BadRequest("tarball is missing web files".into()));
    }
    let backup = dest.with_file_name({
        let mut name = dest.file_name().unwrap_or_default().to_os_string();
        name.push(".bak");
        name
    });
    if dest.exists() {
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
        std::fs::rename(dest, &backup)?;
    }
    match copy_dir(src, dest) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::remove_dir_all(dest);
            if backup.exists() {
                let _ = std::fs::rename(&backup, dest);
            }
            Err(err)
        }
    }
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), ApiError> {
    std::fs::create_dir_all(dest)?;
    for ent in std::fs::read_dir(src)? {
        let ent = ent?;
        let from = ent.path();
        let to = dest.join(ent.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub async fn apply(
    api: &reqwest::Client,
    owner: &str,
    repo: &str,
    www_dir: &Path,
) -> Result<ApplyResult, ApiError> {
    util::require_privileged()?;
    if !running_installed_bin() {
        return Err(ApiError::BadRequest(
            "only the installed daemon at /usr/bin/coduosd can update itself".into(),
        ));
    }
    if !util::which("tar") {
        return Err(ApiError::BadRequest("tar is required to apply updates".into()));
    }
    let rel = fetch_latest(api, owner, repo).await?;
    let current = current_version();
    if up_to_date(&current, &rel.tag_name) {
        return Err(ApiError::BadRequest("already on the latest release".into()));
    }
    let arch = target_arch()?;
    let asset = pick_asset(&rel, arch)?;
    if !allowed_url(owner, repo, &asset.browser_download_url) {
        return Err(ApiError::Forbidden);
    }
    let version = parse_latest(&rel.tag_name);
    let dl = reqwest::Client::builder()
        .user_agent(format!("CoduOS/{current}"))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(ApiError::internal)?;
    let resp = dl
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("download: {e}")))?;
    if !resp.status().is_success() {
        return Err(ApiError::BadRequest(format!(
            "download HTTP {}",
            resp.status()
        )));
    }
    if resp
        .content_length()
        .is_some_and(|n| n > 120 * 1024 * 1024)
    {
        return Err(ApiError::BadRequest("update tarball is too large".into()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ApiError::BadRequest(format!("download: {e}")))?;
    if bytes.len() > 120 * 1024 * 1024 {
        return Err(ApiError::BadRequest("update tarball is too large".into()));
    }

    let tmp = std::env::temp_dir().join(format!("coduos-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let tar_path = tmp.join("coduos.tar.gz");
    let install = async {
        std::fs::write(&tar_path, &bytes)?;
        let tar_s = tar_path.display().to_string();
        let tmp_s = tmp.display().to_string();
        util::run_ok("tar", &["-xzf", &tar_s, "-C", &tmp_s])?;
        let root = extract_root(&tmp)?;
        let bin = root.join("usr/bin/coduosd");
        let www = root.join("usr/share/coduos/www");
        copy_www(&www, www_dir)?;
        install_file(&bin, Path::new("/usr/bin/coduosd"), 0o755)?;
        let unit = root.join("usr/lib/systemd/system/coduosd.service");
        if unit.is_file() {
            install_file(&unit, Path::new("/usr/lib/systemd/system/coduosd.service"), 0o644)?;
        }
        let wg = root.join("usr/lib/systemd/system/coduos-wg.service");
        if wg.is_file() {
            install_file(
                &wg,
                Path::new("/usr/lib/systemd/system/coduos-wg.service"),
                0o644,
            )?;
        }
        let _ = util::run("systemctl", &["daemon-reload"]);
        Ok::<(), ApiError>(())
    }
    .await;
    let _ = std::fs::remove_dir_all(&tmp);
    install?;
    Ok(ApplyResult {
        ok: true,
        version,
        restarting: true,
    })
}
