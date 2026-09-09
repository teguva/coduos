use std::collections::{BTreeMap, HashMap};
use std::net::Ipv4Addr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::error::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppPhase {
    NotInstalled,
    Installing,
    Starting,
    Running,
    Stopped,
    Updating,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComposeStatus {
    pub running: bool,
    pub installed: bool,
    pub phase: AppPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub containers: Vec<ContainerStatus>,
    pub error: Option<String>,
}

impl ComposeStatus {
    fn from_parts(
        running: bool,
        installed: bool,
        containers: Vec<ContainerStatus>,
        last_error: Option<String>,
    ) -> Self {
        let phase = derive_phase(running, installed, &containers, last_error.as_deref());
        let restarting = containers
            .iter()
            .any(|c| c.state.eq_ignore_ascii_case("restarting"));
        let message = match phase {
            AppPhase::NotInstalled => Some("Not installed".into()),
            AppPhase::Running => Some("Running".into()),
            AppPhase::Stopped => Some("Stopped".into()),
            AppPhase::Error if restarting => {
                Some("A container keeps restarting. Open logs.".into())
            }
            AppPhase::Error => last_error
                .as_deref()
                .map(short_error)
                .or_else(|| Some("A container failed".into())),
            _ => None,
        };
        Self {
            running,
            installed,
            phase,
            percent: None,
            message,
            containers,
            error: last_error.filter(|s| !s.is_empty()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppJob {
    pub id: String,
    pub phase: AppPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AppJob {
    pub fn new(id: impl Into<String>, phase: AppPhase, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            phase,
            percent: None,
            message: Some(message.into()),
            error: None,
        }
    }

    pub fn overlay(&self, mut status: ComposeStatus) -> ComposeStatus {
        status.phase = self.phase;
        status.percent = self.percent;
        if self.message.is_some() {
            status.message = self.message.clone();
        }
        if self.error.is_some() {
            status.error = self.error.clone();
        }
        status
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerStatus {
    pub name: String,
    pub state: String,
    pub status: String,
}

#[derive(Debug, Default)]
pub struct PullProgress {
    layers: HashMap<String, (u64, u64)>,
    last_percent: Option<u8>,
    pub message: String,
}

impl PullProgress {
    pub fn ingest(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
            let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("");
            let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let pd = v.get("progressDetail");
            let cur = pd
                .and_then(|p| p.get("current"))
                .or_else(|| v.get("current"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let tot = pd
                .and_then(|p| p.get("total"))
                .or_else(|| v.get("total"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let key = if id.is_empty() { "layer" } else { id };
            if tot > 0 {
                self.layers.insert(key.to_string(), (cur, tot));
            }
            if let Some(p) = v.get("percent").and_then(|x| x.as_u64()) {
                self.last_percent = Some(p.min(99) as u8);
            }
            let msg = if !status.is_empty() && !text.is_empty() && status != text {
                format!("{status} {text}")
            } else if !status.is_empty() {
                status.to_string()
            } else {
                text.to_string()
            };
            if !msg.is_empty() {
                let short = if id.len() > 12 { &id[..12] } else { id };
                self.message = if short.is_empty() {
                    msg
                } else {
                    format!("{msg} {short}")
                };
            }
            return;
        }
        self.message = line.chars().take(120).collect();
    }

    pub fn percent(&self) -> Option<u8> {
        if self.layers.is_empty() {
            return self.last_percent;
        }
        let (cur, tot) = self
            .layers
            .values()
            .fold((0u64, 0u64), |acc, (c, t)| (acc.0 + c, acc.1 + t));
        if tot == 0 {
            return self.last_percent;
        }
        Some(((cur.saturating_mul(100)) / tot).min(99) as u8)
    }
}

pub fn derive_phase(
    running: bool,
    installed: bool,
    containers: &[ContainerStatus],
    last_error: Option<&str>,
) -> AppPhase {
    if running {
        let mixed = containers.iter().any(|c| {
            let s = c.state.to_ascii_lowercase();
            s == "restarting" || s == "dead" || s == "exited"
        });
        return if mixed {
            AppPhase::Error
        } else {
            AppPhase::Running
        };
    }
    if containers
        .iter()
        .any(|c| c.state.eq_ignore_ascii_case("restarting"))
    {
        return AppPhase::Error;
    }
    if last_error.map(|e| !e.trim().is_empty()).unwrap_or(false) {
        return AppPhase::Error;
    }
    if installed {
        AppPhase::Stopped
    } else {
        AppPhase::NotInstalled
    }
}

pub fn short_error(raw: &str) -> String {
    let line = raw
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or(raw.trim());
    let t = line
        .trim_start_matches("Error:")
        .trim_start_matches("error:")
        .trim();
    t.chars().take(180).collect()
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

pub async fn inspect_status(apps_dir: &Path, id: &str, last_error: Option<&str>) -> ComposeStatus {
    let installed = images_present(apps_dir, id).await;
    match compose(apps_dir, id, &["ps", "-a", "--format", "json"]).await {
        Ok((true, stdout, _)) => {
            let containers = parse_ps_json(&stdout, id);
            let running = containers.iter().any(|c| {
                let s = c.state.to_ascii_lowercase();
                s == "running" || s == "up"
            });
            let installed = installed || !containers.is_empty();
            ComposeStatus::from_parts(
                running,
                installed,
                containers,
                last_error.map(str::to_string).filter(|s| !s.is_empty()),
            )
        }
        Ok((false, _, stderr)) => {
            let err = last_error
                .map(str::to_string)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    let t = stderr.trim();
                    if t.is_empty() {
                        None
                    } else {
                        Some(short_error(t))
                    }
                });
            ComposeStatus::from_parts(false, installed, vec![], err)
        }
        Err(err) => ComposeStatus::from_parts(
            false,
            installed,
            vec![],
            Some(short_error(&err.to_string())),
        ),
    }
}

fn parse_ps_json(stdout: &str, id: &str) -> Vec<ContainerStatus> {
    let mut containers = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            containers.push(container_from_value(&v, id));
        } else if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(stdout) {
            containers.extend(arr.iter().map(|v| container_from_value(v, id)));
            break;
        }
    }
    containers
}

fn container_from_value(v: &serde_json::Value, id: &str) -> ContainerStatus {
    ContainerStatus {
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
    }
}

pub async fn images_present(apps_dir: &Path, id: &str) -> bool {
    let Ok((ok, stdout, _)) = compose(apps_dir, id, &["config", "--images"]).await else {
        return false;
    };
    if !ok {
        return false;
    }
    let images: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if images.is_empty() {
        return false;
    }
    for img in images {
        let mut cmd = Command::new("docker");
        cmd.arg("image")
            .arg("inspect")
            .arg(img)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let ok = cmd
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return false;
        }
    }
    true
}

pub async fn compose_stream<F>(
    apps_dir: &Path,
    id: &str,
    args: &[&str],
    mut on_line: F,
) -> Result<(bool, String), ApiError>
where
    F: FnMut(&str),
{
    let file = compose_file(apps_dir, id);
    if !file.exists() {
        return Err(ApiError::NotFound);
    }
    let dir = file.parent().unwrap_or(apps_dir);
    let mut cmd = Command::new("docker");
    cmd.current_dir(dir)
        .arg("compose")
        .arg("--progress")
        .arg("json")
        .arg("--project-directory")
        .arg(dir)
        .arg("-f")
        .arg(&file)
        .arg("-p")
        .arg(id)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|err| {
        ApiError::BadRequest(format!("docker compose failed to start: {err}"))
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Some(out) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    if let Some(err) = stderr {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let mut log = String::new();
    while let Some(line) = rx.recv().await {
        if !log.is_empty() {
            log.push('\n');
        }
        log.push_str(&line);
        on_line(&line);
    }
    let status = child.wait().await.map_err(|err| {
        ApiError::BadRequest(format!("docker compose failed: {err}"))
    })?;
    Ok((status.success(), log))
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

pub async fn port_is_open(port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_millis(400),
        tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .is_some()
}

/// Poll localhost until `port` accepts a TCP connection, or `timeout` elapses.
pub async fn wait_for_tcp(port: u16, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if port_is_open(port).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_progress_aggregates_layers() {
        let mut p = PullProgress::default();
        p.ingest(r#"{"id":"abc","status":"Downloading","progressDetail":{"current":50,"total":100}}"#);
        p.ingest(r#"{"id":"def","status":"Downloading","progressDetail":{"current":25,"total":100}}"#);
        assert_eq!(p.percent(), Some(37));
        assert!(p.message.contains("Downloading"));
    }

    #[test]
    fn pull_progress_plaintext_and_percent_field() {
        let mut p = PullProgress::default();
        p.ingest("Pulling postgres… 120 MB");
        assert_eq!(p.percent(), None);
        assert!(p.message.contains("postgres"));
        p.ingest(r#"{"status":"Working","text":"Pulling","percent":42}"#);
        assert_eq!(p.percent(), Some(42));
    }

    #[test]
    fn derive_phase_table() {
        assert_eq!(derive_phase(false, false, &[], None), AppPhase::NotInstalled);
        assert_eq!(derive_phase(false, true, &[], None), AppPhase::Stopped);
        assert_eq!(derive_phase(true, true, &[], None), AppPhase::Running);
        assert_eq!(derive_phase(false, false, &[], Some("boom")), AppPhase::Error);
        let mixed = [
            ContainerStatus {
                name: "a".into(),
                state: "running".into(),
                status: String::new(),
            },
            ContainerStatus {
                name: "b".into(),
                state: "exited".into(),
                status: String::new(),
            },
        ];
        assert_eq!(derive_phase(true, true, &mixed, None), AppPhase::Error);
        let restarting = [ContainerStatus {
            name: "a".into(),
            state: "restarting".into(),
            status: String::new(),
        }];
        assert_eq!(derive_phase(false, true, &restarting, None), AppPhase::Error);
    }

    #[test]
    fn short_error_uses_last_line() {
        assert_eq!(short_error("warn\nError: no such image"), "no such image");
    }

    #[tokio::test]
    async fn wait_for_tcp_succeeds_when_listener_is_up() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        assert!(wait_for_tcp(port, Duration::from_secs(2)).await);
    }

    #[tokio::test]
    async fn wait_for_tcp_times_out_when_closed() {
        let port = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        assert!(!wait_for_tcp(port, Duration::from_millis(500)).await);
    }
}


