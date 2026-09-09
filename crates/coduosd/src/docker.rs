use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Serialize;
use tokio::process::Command;

use crate::error::ApiError;

#[derive(Debug, Clone, Serialize)]
pub struct ComposeStatus {
    pub running: bool,
    pub containers: Vec<ContainerStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerStatus {
    pub name: String,
    pub state: String,
    pub status: String,
}

pub fn compose_file(apps_dir: &Path, id: &str) -> PathBuf {
    apps_dir.join(id).join("compose.yml")
}

pub fn write_compose(apps_dir: &Path, id: &str, yaml: &str) -> Result<PathBuf, ApiError> {
    let dir = apps_dir.join(id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("compose.yml");
    std::fs::write(&path, yaml)?;
    write_dotenv(&dir, yaml)?;
    Ok(path)
}

fn write_dotenv(dir: &Path, yaml: &str) -> Result<(), ApiError> {
    let mut env = BTreeMap::new();
    collect_interpolations(yaml, &mut env);
    if let Ok(parsed) = serde_yaml::from_str::<serde_yaml::Value>(yaml) {
        if let Some(map) = parsed
            .get("x-coduos")
            .and_then(|v| v.get("env"))
            .and_then(|v| v.as_mapping())
        {
            for (k, v) in map {
                let Some(key) = k.as_str() else { continue };
                if let Some(val) = yaml_scalar(v) {
                    env.insert(key.to_string(), val);
                }
            }
        }
    }
    let path = dir.join(".env");
    if env.is_empty() {
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        return Ok(());
    }
    let mut body = String::new();
    for (key, value) in &env {
        body.push_str(&dotenv_line(key, value));
    }
    std::fs::write(&path, body)?;
    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&path, perms)?;
    Ok(())
}

fn collect_interpolations(yaml: &str, env: &mut BTreeMap<String, String>) {
    let bytes = yaml.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            if let Some(rel) = yaml[start..].find('}') {
                let inner = &yaml[start..start + rel];
                if let Some((key, def)) = split_interp(inner) {
                    env.entry(key.to_string()).or_insert_with(|| def.to_string());
                }
                i = start + rel + 1;
                continue;
            }
        }
        i += 1;
    }
}

fn split_interp(inner: &str) -> Option<(&str, &str)> {
    let (key, def) = inner
        .split_once(":-")
        .map(|(k, d)| (k, d))
        .unwrap_or((inner, ""));
    if key.is_empty() {
        return None;
    }
    let mut chars = key.chars();
    let first = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    if !chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((key, def))
}

fn yaml_scalar(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        serde_yaml::Value::Null => Some(String::new()),
        _ => None,
    }
}

fn dotenv_line(key: &str, value: &str) -> String {
    if value.is_empty() {
        return format!("{key}=\n");
    }
    if value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\''))
    {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("{key}=\"{escaped}\"\n");
    }
    format!("{key}={value}\n")
}

pub fn remove_app_dir(apps_dir: &Path, id: &str) -> Result<(), ApiError> {
    let dir = apps_dir.join(id);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

pub async fn compose(
    apps_dir: &Path,
    id: &str,
    args: &[&str],
) -> Result<(bool, String, String), ApiError> {
    let file = compose_file(apps_dir, id);
    if !file.exists() {
        return Err(ApiError::NotFound);
    }
    let dir = file.parent().unwrap_or(apps_dir);
    let mut cmd = Command::new("docker");
    cmd.current_dir(dir)
        .arg("compose")
        .arg("--project-directory")
        .arg(dir)
        .arg("-f")
        .arg(&file)
        .arg("-p")
        .arg(id)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().await.map_err(|err| {
        ApiError::BadRequest(format!("docker compose failed to start: {err}"))
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((out.status.success(), stdout, stderr))
}

pub async fn compose_ps(apps_dir: &Path, id: &str) -> ComposeStatus {
    match compose(apps_dir, id, &["ps", "--format", "json"]).await {
        Ok((true, stdout, _)) => {
            let mut containers = Vec::new();
            for line in stdout.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    containers.push(ContainerStatus {
                        name: v
                            .get("Name")
                            .or_else(|| v.get("name"))
                            .and_then(|x| x.as_str())
                            .unwrap_or(id)
                            .to_string(),
                        state: v
                            .get("State")
                            .or_else(|| v.get("state"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        status: v
                            .get("Status")
                            .or_else(|| v.get("status"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    });
                } else if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                    for v in arr {
                        containers.push(ContainerStatus {
                            name: v
                                .get("Name")
                                .and_then(|x| x.as_str())
                                .unwrap_or(id)
                                .to_string(),
                            state: v
                                .get("State")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string(),
                            status: v
                                .get("Status")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string(),
                        });
                    }
                    break;
                }
            }
            let running = containers.iter().any(|c| {
                let s = c.state.to_ascii_lowercase();
                s == "running" || s == "up"
            });
            ComposeStatus {
                running,
                containers,
                error: None,
            }
        }
        Ok((false, _, stderr)) => ComposeStatus {
            running: false,
            containers: vec![],
            error: Some(stderr),
        },
        Err(err) => ComposeStatus {
            running: false,
            containers: vec![],
            error: Some(err.to_string()),
        },
    }
}

pub fn first_host_port(yaml: &str) -> Option<u16> {
    let parsed: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
    let services = parsed.get("services")?.as_mapping()?;
    for (_, svc) in services {
        if let Some(ports) = svc.get("ports").and_then(|p| p.as_sequence()) {
            for port in ports {
                if let Some(s) = port.as_str() {
                    if let Some(p) = parse_port_map(s) {
                        return Some(p);
                    }
                } else if let Some(n) = port.as_u64() {
                    return u16::try_from(n).ok();
                } else if let Some(published) = port.get("published") {
                    if let Some(n) = published.as_u64() {
                        return u16::try_from(n).ok();
                    }
                    if let Some(s) = published.as_str() {
                        return s.parse().ok();
                    }
                }
            }
        }
    }
    None
}

fn parse_port_map(s: &str) -> Option<u16> {
    // "8080:80" or "127.0.0.1:8080:80" or "8080"
    let parts: Vec<&str> = s.split('/').next().unwrap_or(s).split(':').collect();
    match parts.as_slice() {
        [host] => host.parse().ok(),
        [host, _cont] => host.parse().ok(),
        [_, host, _cont] => host.parse().ok(),
        _ => None,
    }
}

pub fn docker_available() -> bool {
    std::process::Command::new("docker")
        .args(["info"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

