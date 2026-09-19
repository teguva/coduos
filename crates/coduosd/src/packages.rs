use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;

use serde::Serialize;

use crate::error::ApiError;
use crate::util;

/// Embedded copy of `packaging/deps`. `/usr/share/coduos/deps` from a newer
/// tarball wins so an update can install packages the old binary did not know.
const DEPS_TEXT: &str = include_str!("../../../packaging/deps");
const DEPS_PATH: &str = "/usr/share/coduos/deps";
const COMPOSE_MIN: (u32, u32, u32) = (2, 29, 0);
const COMPOSE_PLUGIN_VERSION: &str = "v2.40.3";
const COMPOSE_PLUGIN_DIR: &str = "/usr/local/lib/docker/cli-plugins";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub names: Vec<String>,
    pub command: String,
    pub reason: String,
}

impl Dep {
    pub fn name(&self) -> &str {
        self.names.first().map(String::as_str).unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageStatus {
    pub name: String,
    pub command: String,
    pub reason: String,
    pub installed: bool,
}

#[derive(Debug, Serialize)]
pub struct InstallOut {
    pub ok: bool,
    pub installed: Vec<String>,
    pub packages: Vec<PackageStatus>,
    pub error: Option<String>,
}

pub fn deps() -> Vec<Dep> {
    if let Ok(text) = std::fs::read_to_string(DEPS_PATH) {
        let list = parse_deps(&text);
        if !list.is_empty() {
            return list;
        }
    }
    parse_deps(DEPS_TEXT)
}

pub fn parse_deps(text: &str) -> Vec<Dep> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '|');
        let Some(names_raw) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(command) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let reason = parts.next().unwrap_or("").trim();
        let names: Vec<String> = names_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if names.is_empty() {
            continue;
        }
        out.push(Dep {
            names,
            command: command.to_string(),
            reason: reason.to_string(),
        });
    }
    out
}

pub fn command_present(command: &str) -> bool {
    if is_compose_check(command) {
        return compose_meets_min();
    }
    let mut parts = command.split_whitespace();
    let Some(bin) = parts.next() else {
        return false;
    };
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        return util::which(bin);
    }
    std::process::Command::new(bin)
        .args(&rest)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_compose_check(command: &str) -> bool {
    command == "docker compose version"
}

fn compose_meets_min() -> bool {
    compose_short_version()
        .as_deref()
        .is_some_and(compose_version_ok)
}

fn compose_short_version() -> Option<String> {
    let out = std::process::Command::new("docker")
        .args(["compose", "version", "--short"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn parse_compose_version(raw: &str) -> Option<(u32, u32, u32)> {
    let s = raw.trim().trim_start_matches('v');
    let s = s.split(['-', '+']).next().unwrap_or(s);
    let mut parts = s.split('.');
    let major = parts.next()?.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    let minor = parts
        .next()
        .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .filter(|p| !p.is_empty())
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let patch = parts
        .next()
        .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .filter(|p| !p.is_empty())
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

fn compose_version_ok(raw: &str) -> bool {
    parse_compose_version(raw).is_some_and(|v| v >= COMPOSE_MIN)
}

fn compose_download_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x86_64"),
        "aarch64" => Some("aarch64"),
        "arm" => Some("armv7"),
        _ => None,
    }
}

fn install_compose_plugin() -> Result<(), String> {
    let ver = std::env::var("CODUOS_COMPOSE_VERSION")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| COMPOSE_PLUGIN_VERSION.to_string());
    let arch = compose_download_arch().ok_or_else(|| {
        format!("no Docker Compose plugin for {}", std::env::consts::ARCH)
    })?;
    let url = format!(
        "https://github.com/docker/compose/releases/download/{ver}/docker-compose-linux-{arch}"
    );
    tracing::info!("installing Docker Compose {ver} ({arch})");
    fs::create_dir_all(COMPOSE_PLUGIN_DIR).map_err(|err| format!("{COMPOSE_PLUGIN_DIR}: {err}"))?;
    let dest = Path::new(COMPOSE_PLUGIN_DIR).join("docker-compose");
    let tmp = dest.with_extension("new");
    let out = std::process::Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(&tmp)
        .arg(&url)
        .output()
        .map_err(|err| format!("curl: {err}"))?;
    if !out.status.success() {
        let _ = fs::remove_file(&tmp);
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.trim();
        return Err(if msg.is_empty() {
            format!("could not download {url}")
        } else {
            format!("could not download {url}: {msg}")
        });
    }
    let mut perms = fs::metadata(&tmp)
        .map_err(|err| format!("{}: {err}", tmp.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&tmp, perms).map_err(|err| format!("{}: {err}", tmp.display()))?;
    fs::rename(&tmp, &dest).map_err(|err| format!("{}: {err}", dest.display()))?;
    Ok(())
}

pub fn status() -> Vec<PackageStatus> {
    deps()
        .into_iter()
        .map(|d| PackageStatus {
            installed: command_present(&d.command),
            name: d.name().to_string(),
            command: d.command,
            reason: d.reason,
        })
        .collect()
}

pub fn install_missing() -> Result<InstallOut, ApiError> {
    sync()
}

/// Refresh the package index, install any missing CoduOS deps, and upgrade
/// those packages if the manager already has them.
pub fn sync() -> Result<InstallOut, ApiError> {
    util::require_privileged()?;
    let wanted: Vec<Dep> = deps();
    tracing::info!("syncing CoduOS packages");
    if let Err(err) = refresh_index() {
        return Ok(InstallOut {
            ok: false,
            installed: Vec::new(),
            packages: status(),
            error: Some(err),
        });
    }
    let mut installed = Vec::new();
    for dep in &wanted {
        if command_present(&dep.command) {
            continue;
        }
        match install_dep(dep) {
            Ok(()) if command_present(&dep.command) => installed.push(dep.name().to_string()),
            Ok(()) => {
                return Ok(InstallOut {
                    ok: false,
                    installed,
                    packages: status(),
                    error: Some(format!("could not install {}", dep.name())),
                });
            }
            Err(err) => {
                return Ok(InstallOut {
                    ok: false,
                    installed,
                    packages: status(),
                    error: Some(err),
                });
            }
        }
    }
    if let Err(err) = upgrade_listed(&wanted) {
        return Ok(InstallOut {
            ok: false,
            installed,
            packages: status(),
            error: Some(err),
        });
    }
    let after = status();
    let still: Vec<String> = after
        .iter()
        .filter(|p| !p.installed)
        .map(|p| p.name.clone())
        .collect();
    if still.is_empty() {
        Ok(InstallOut {
            ok: true,
            installed,
            packages: after,
            error: None,
        })
    } else {
        Ok(InstallOut {
            ok: false,
            installed,
            packages: after,
            error: Some(format!("could not install {}", still.join(", "))),
        })
    }
}

fn install_dep(dep: &Dep) -> Result<(), String> {
    let mut last = "no package name".to_string();
    for name in &dep.names {
        match install_one(name) {
            Ok(()) => {
                if command_present(&dep.command) {
                    return Ok(());
                }
                last = format!("{name} installed but {} still missing", dep.command);
            }
            Err(err) => last = err,
        }
    }
    if is_compose_check(&dep.command) {
        match install_compose_plugin() {
            Ok(()) if command_present(&dep.command) => return Ok(()),
            Ok(()) => {
                last = "Docker Compose plugin is still older than 2.29".into();
            }
            Err(err) => last = err,
        }
    }
    Err(last)
}

fn install_one(package: &str) -> Result<(), String> {
    if util::which("apt-get") {
        return run_pm(
            "apt-get",
            &["install", "-y", package],
            &[("DEBIAN_FRONTEND", "noninteractive")],
        );
    }
    if util::which("pacman") {
        return run_pm("pacman", &["-Sy", "--noconfirm", "--needed", package], &[]);
    }
    if util::which("dnf") {
        return run_pm("dnf", &["install", "-y", package], &[]);
    }
    if util::which("apk") {
        return run_pm("apk", &["add", "--no-cache", package], &[]);
    }
    Err("no supported package manager".into())
}

fn refresh_index() -> Result<(), String> {
    if util::which("apt-get") {
        return run_pm(
            "apt-get",
            &["update", "-qq"],
            &[("DEBIAN_FRONTEND", "noninteractive")],
        );
    }
    Ok(())
}

fn upgrade_listed(wanted: &[Dep]) -> Result<(), String> {
    if util::which("apt-get") {
        let pkgs = apt_installed_names(wanted);
        if pkgs.is_empty() {
            return Ok(());
        }
        let mut args = vec!["install".into(), "-y".into()];
        args.extend(pkgs);
        return run_pm(
            "apt-get",
            &str_refs(&args),
            &[("DEBIAN_FRONTEND", "noninteractive")],
        );
    }
    if util::which("pacman") {
        let mut args = vec!["-Sy".into(), "--noconfirm".into(), "--needed".into()];
        args.extend(wanted.iter().map(|d| d.name().to_string()));
        return run_pm("pacman", &str_refs(&args), &[]);
    }
    Ok(())
}

fn apt_installed_names(wanted: &[Dep]) -> Vec<String> {
    let mut out = Vec::new();
    for dep in wanted {
        for name in &dep.names {
            if dpkg_installed(name) && !out.iter().any(|n| n == name) {
                out.push(name.clone());
            }
        }
    }
    out
}

fn dpkg_installed(name: &str) -> bool {
    let out = std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Status}", name])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else {
        return false;
    };
    out.status.success() && String::from_utf8_lossy(&out.stdout).contains("install ok installed")
}

fn str_refs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn run_pm(cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Result<(), String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().map_err(|err| format!("{cmd}: {err}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let msg = err.trim();
    let msg = if msg.is_empty() { stdout.trim() } else { msg };
    Err(if msg.is_empty() {
        format!("{cmd} failed")
    } else {
        format!("{cmd} failed: {msg}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_packaging_deps() {
        let list = parse_deps(DEPS_TEXT);
        let names: Vec<&str> = list.iter().map(|d| d.name()).collect();
        assert!(names.contains(&"parted"));
        assert!(names.contains(&"e2fsprogs"));
        assert!(names.contains(&"openssl"));
        assert!(names.contains(&"docker-compose-v2"));
        let compose = list.iter().find(|d| d.name() == "docker-compose-v2").unwrap();
        assert!(compose.names.contains(&"docker-compose-plugin".into()));
        assert_eq!(compose.command, "docker compose version");
        assert!(compose.reason.contains("2.29"));
        assert!(list.iter().all(|d| !d.command.is_empty() && !d.reason.is_empty()));
    }

    #[test]
    fn compose_version_requires_2_29() {
        assert!(!compose_version_ok("2.26.1-4"));
        assert!(!compose_version_ok("2.28.1"));
        assert!(compose_version_ok("2.29.0"));
        assert!(compose_version_ok("v2.40.3"));
        assert!(compose_version_ok("5.5.1"));
        assert_eq!(parse_compose_version("2.26.1-4"), Some((2, 26, 1)));
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let list = parse_deps("# hi\n\nparted|parted|Format a whole disk\n");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name(), "parted");
    }

    #[test]
    fn command_present_which_and_missing() {
        assert!(command_present("true"));
        assert!(!command_present("coduos-no-such-binary-xyz"));
    }
}
