use serde::Serialize;

use crate::error::ApiError;
use crate::util;

/// Embedded copy of `packaging/deps`.
const DEPS_TEXT: &str = include_str!("../../../packaging/deps");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub name: String,
    pub command: String,
    pub reason: String,
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
        let Some(name) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(command) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let reason = parts.next().unwrap_or("").trim();
        out.push(Dep {
            name: name.to_string(),
            command: command.to_string(),
            reason: reason.to_string(),
        });
    }
    out
}

pub fn status() -> Vec<PackageStatus> {
    deps()
        .into_iter()
        .map(|d| PackageStatus {
            installed: util::which(&d.command),
            name: d.name,
            command: d.command,
            reason: d.reason,
        })
        .collect()
}

pub fn missing_names() -> Vec<String> {
    status()
        .into_iter()
        .filter(|p| !p.installed)
        .map(|p| p.name)
        .collect()
}

pub fn install_missing() -> Result<InstallOut, ApiError> {
    util::require_privileged()?;
    let wanted = missing_names();
    if wanted.is_empty() {
        return Ok(InstallOut {
            ok: true,
            installed: Vec::new(),
            packages: status(),
            error: None,
        });
    }
    tracing::info!(packages = %wanted.join(", "), "installing missing CoduOS packages");
    if let Err(err) = install_packages(&wanted) {
        return Ok(InstallOut {
            ok: false,
            installed: Vec::new(),
            packages: status(),
            error: Some(err),
        });
    }
    let after = status();
    let still: Vec<String> = after
        .iter()
        .filter(|p| !p.installed && wanted.iter().any(|n| n == &p.name))
        .map(|p| p.name.clone())
        .collect();
    let installed: Vec<String> = wanted
        .iter()
        .filter(|n| after.iter().any(|p| p.name == **n && p.installed))
        .cloned()
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
            error: Some(format!(
                "could not install {}",
                still.join(", ")
            )),
        })
    }
}

fn install_packages(packages: &[String]) -> Result<(), String> {
    if packages.is_empty() {
        return Ok(());
    }
    if util::which("apt-get") {
        run_pm(
            "apt-get",
            &["update", "-qq"],
            &[("DEBIAN_FRONTEND", "noninteractive")],
        )?;
        let mut args = vec!["install".into(), "-y".into()];
        args.extend(packages.iter().cloned());
        return run_pm("apt-get", &str_refs(&args), &[("DEBIAN_FRONTEND", "noninteractive")]);
    }
    if util::which("pacman") {
        let mut args = vec!["-Sy".into(), "--noconfirm".into(), "--needed".into()];
        args.extend(packages.iter().cloned());
        return run_pm("pacman", &str_refs(&args), &[]);
    }
    if util::which("dnf") {
        let mut args = vec!["install".into(), "-y".into()];
        args.extend(packages.iter().cloned());
        return run_pm("dnf", &str_refs(&args), &[]);
    }
    if util::which("apk") {
        let mut args = vec!["add".into(), "--no-cache".into()];
        args.extend(packages.iter().cloned());
        return run_pm("apk", &str_refs(&args), &[]);
    }
    Err("no supported package manager".into())
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
    let out = c
        .output()
        .map_err(|err| format!("{cmd}: {err}"))?;
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
        let list = deps();
        let names: Vec<&str> = list.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"parted"));
        assert!(names.contains(&"e2fsprogs"));
        assert!(names.contains(&"openssl"));
        assert!(list.iter().all(|d| !d.command.is_empty() && !d.reason.is_empty()));
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let list = parse_deps("# hi\n\nparted|parted|Format a whole disk\n");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "parted");
    }
}
