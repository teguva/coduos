use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::ApiError;
use crate::util::{self, privileged, valid_hostname, valid_hostport_host};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyState {
    pub dashboard_tls: String,
    pub hosts: Vec<ProxyHost>,
}

impl Default for ProxyState {
    fn default() -> Self {
        Self {
            dashboard_tls: "off".into(),
            hosts: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyHost {
    pub id: String,
    pub hostname: String,
    pub target_host: String,
    pub target_port: u16,
    /// off | lan | acme
    pub tls: String,
    pub websocket: bool,
    pub enabled: bool,
    #[serde(default)]
    pub app_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyStatus {
    pub privileged: bool,
    pub nginx_available: bool,
    pub certbot_available: bool,
    pub dashboard_upstream: String,
    pub dashboard_tls: String,
    pub hosts: Vec<ProxyHost>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HostIn {
    pub hostname: String,
    pub target_host: Option<String>,
    pub target_port: u16,
    pub tls: Option<String>,
    pub websocket: Option<bool>,
    pub enabled: Option<bool>,
    pub app_id: Option<String>,
}

fn dir(cfg: &Config) -> PathBuf {
    cfg.data_dir.join("nginx")
}

fn conf_d(cfg: &Config) -> PathBuf {
    dir(cfg).join("conf.d")
}

fn state_path(cfg: &Config) -> PathBuf {
    dir(cfg).join("proxy.json")
}

fn load_state(cfg: &Config) -> ProxyState {
    std::fs::read_to_string(state_path(cfg))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(cfg: &Config, state: &ProxyState) -> Result<(), ApiError> {
    std::fs::create_dir_all(dir(cfg))?;
    let path = state_path(cfg);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(state).map_err(ApiError::internal)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn tls_mode(s: &str) -> Result<&'static str, ApiError> {
    match s {
        "off" => Ok("off"),
        "lan" => Ok("lan"),
        "acme" => Ok("acme"),
        _ => Err(ApiError::BadRequest("tls must be off, lan, or acme".into())),
    }
}

fn nginx_escape_host(host: &str) -> Result<&str, ApiError> {
    if valid_hostname(host) {
        Ok(host)
    } else {
        Err(ApiError::BadRequest(format!("invalid hostname {host}")))
    }
}

fn proxy_headers(websocket: bool) -> &'static str {
    if websocket {
        "    proxy_http_version 1.1;\n    proxy_set_header Upgrade $http_upgrade;\n    proxy_set_header Connection \"upgrade\";\n"
    } else {
        "    proxy_http_version 1.1;\n    proxy_set_header Connection \"\";\n"
    }
}

fn location_block(upstream: &str, websocket: bool) -> String {
    format!(
        "  location / {{\n    proxy_pass {upstream};\n    proxy_set_header Host $host;\n    proxy_set_header X-Real-IP $remote_addr;\n    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n    proxy_set_header X-Forwarded-Proto $scheme;\n{headers}  }}\n",
        upstream = upstream,
        headers = proxy_headers(websocket)
    )
}

fn acme_location(cfg: &Config) -> String {
    let root = dir(cfg).join("acme");
    format!(
        "  location /.well-known/acme-challenge/ {{\n    root {};\n  }}\n",
        root.display()
    )
}

fn ssl_listen(host: &str, crt: &Path, key: &Path, extra: &str) -> String {
    format!(
        "server {{\n  listen 443 ssl;\n  listen [::]:443 ssl;\n  server_name {host};\n  ssl_certificate {};\n  ssl_certificate_key {};\n{extra}}}\n",
        crt.display(),
        key.display()
    )
}

fn lan_certs(cfg: &Config) -> Result<(PathBuf, PathBuf), ApiError> {
    let certs = dir(cfg).join("certs");
    std::fs::create_dir_all(&certs)?;
    let crt = certs.join("lan.crt");
    let key = certs.join("lan.key");
    if crt.exists() && key.exists() {
        return Ok((crt, key));
    }
    util::run_ok(
        "openssl",
        &[
            "req",
            "-x509",
            "-nodes",
            "-newkey",
            "rsa:2048",
            "-keyout",
            &key.display().to_string(),
            "-out",
            &crt.display().to_string(),
            "-days",
            "825",
            "-subj",
            "/CN=coduos.local",
        ],
    )?;
    Ok((crt, key))
}

fn acme_certs(hostname: &str) -> Option<(PathBuf, PathBuf)> {
    let live = PathBuf::from("/etc/letsencrypt/live").join(hostname);
    let crt = live.join("fullchain.pem");
    let key = live.join("privkey.pem");
    if crt.exists() && key.exists() {
        Some((crt, key))
    } else {
        None
    }
}

fn dashboard_upstream(cfg: &Config) -> String {
    let bind = cfg.bind.clone();
    let port = bind
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(13209);
    format!("http://127.0.0.1:{port}")
}

fn write_nginx(cfg: &Config, state: &ProxyState) -> Result<(), ApiError> {
    let conf_d = conf_d(cfg);
    std::fs::create_dir_all(&conf_d)?;
    std::fs::create_dir_all(dir(cfg).join("acme"))?;
    for ent in std::fs::read_dir(&conf_d).into_iter().flatten().flatten() {
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) == Some("conf") {
            let _ = std::fs::remove_file(p);
        }
    }

    let upstream = dashboard_upstream(cfg);
    let mut dash = format!(
        "server {{\n  listen 80 default_server;\n  listen [::]:80 default_server;\n  server_name _;\n{}\n{}\n}}\n",
        acme_location(cfg),
        location_block(&upstream, true)
    );
    if state.dashboard_tls == "lan" {
        let (crt, key) = lan_certs(cfg)?;
        dash.push_str(&ssl_listen(
            "_",
            &crt,
            &key,
            &location_block(&upstream, true),
        ));
    }
    std::fs::write(conf_d.join("00-dashboard.conf"), dash)?;

    for host in state.hosts.iter().filter(|h| h.enabled) {
        let name = nginx_escape_host(&host.hostname)?;
        if !valid_hostport_host(&host.target_host) {
            return Err(ApiError::BadRequest(format!(
                "invalid upstream host {}",
                host.target_host
            )));
        }
        if host.target_port == 0 {
            return Err(ApiError::BadRequest("invalid upstream port".into()));
        }
        let up = format!("http://{}:{}", host.target_host, host.target_port);
        let mut body = format!(
            "server {{\n  listen 80;\n  listen [::]:80;\n  server_name {name};\n{}\n{}\n}}\n",
            acme_location(cfg),
            location_block(&up, host.websocket)
        );
        match host.tls.as_str() {
            "lan" => {
                let (crt, key) = lan_certs(cfg)?;
                body.push_str(&ssl_listen(
                    name,
                    &crt,
                    &key,
                    &location_block(&up, host.websocket),
                ));
            }
            "acme" => {
                if let Some((crt, key)) = acme_certs(name) {
                    body.push_str(&ssl_listen(
                        name,
                        &crt,
                        &key,
                        &location_block(&up, host.websocket),
                    ));
                }
            }
            _ => {}
        }
        let fname = format!("10-{}.conf", crate::config::slugify(name));
        std::fs::write(conf_d.join(fname), body)?;
    }
    Ok(())
}

fn reload_nginx() -> Result<(), ApiError> {
    if !privileged() {
        return Ok(());
    }
    let test = util::run("nginx", &["-t"])?;
    if !test.status.success() {
        return Err(ApiError::BadRequest(format!(
            "nginx -t: {}",
            String::from_utf8_lossy(&test.stderr).trim()
        )));
    }
    let out = util::run("systemctl", &["reload", "nginx.service"])?;
    if !out.status.success() {
        let start = util::run("systemctl", &["restart", "nginx.service"])?;
        if !start.status.success() {
            return Err(ApiError::BadRequest(format!(
                "nginx reload: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    Ok(())
}

fn issue_acme(cfg: &Config, hostname: &str) -> Result<(), ApiError> {
    if !util::which("certbot") {
        return Err(ApiError::BadRequest("certbot is not installed".into()));
    }
    let webroot = dir(cfg).join("acme");
    std::fs::create_dir_all(&webroot)?;
    let email_host = hostname.split('.').rev().take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(".");
    let email = format!("admin@{email_host}");
    util::run_ok(
        "certbot",
        &[
            "certonly",
            "--webroot",
            "-w",
            &webroot.display().to_string(),
            "-d",
            hostname,
            "--non-interactive",
            "--agree-tos",
            "-m",
            &email,
            "--keep-until-expiring",
        ],
    )?;
    Ok(())
}

pub fn status(cfg: &Config) -> ProxyStatus {
    let state = load_state(cfg);
    let nginx_available = util::which("nginx");
    ProxyStatus {
        privileged: privileged(),
        nginx_available,
        certbot_available: util::which("certbot"),
        dashboard_upstream: dashboard_upstream(cfg),
        dashboard_tls: state.dashboard_tls,
        hosts: state.hosts,
        error: if nginx_available {
            None
        } else {
            Some("nginx is not installed".into())
        },
    }
}

pub fn apply_now(cfg: &Config) -> Result<ProxyStatus, ApiError> {
    let state = load_state(cfg);
    write_nginx(cfg, &state)?;
    if privileged() {
        reload_nginx()?;
    }
    Ok(status(cfg))
}

#[derive(Deserialize)]
pub struct DashboardIn {
    pub tls: String,
}

pub fn set_dashboard_tls(cfg: &Config, tls: &str) -> Result<ProxyStatus, ApiError> {
    util::require_privileged()?;
    let mode = tls_mode(tls)?;
    let mut state = load_state(cfg);
    state.dashboard_tls = mode.to_string();
    save_state(cfg, &state)?;
    write_nginx(cfg, &state)?;
    reload_nginx()?;
    Ok(status(cfg))
}

pub fn upsert_host(cfg: &Config, body: HostIn, id: Option<String>) -> Result<ProxyStatus, ApiError> {
    util::require_privileged()?;
    let hostname = body.hostname.trim().to_ascii_lowercase();
    nginx_escape_host(&hostname)?;
    let tls = tls_mode(body.tls.as_deref().unwrap_or("off"))?;
    let target_host = body
        .target_host
        .as_deref()
        .unwrap_or("127.0.0.1")
        .trim()
        .to_string();
    if !valid_hostport_host(&target_host) {
        return Err(ApiError::BadRequest("invalid upstream host".into()));
    }
    if body.target_port == 0 {
        return Err(ApiError::BadRequest("invalid upstream port".into()));
    }
    let mut state = load_state(cfg);
    let id = id.unwrap_or_else(|| crate::config::slugify(&hostname));
    let host = ProxyHost {
        id: id.clone(),
        hostname,
        target_host,
        target_port: body.target_port,
        tls: tls.to_string(),
        websocket: body.websocket.unwrap_or(true),
        enabled: body.enabled.unwrap_or(true),
        app_id: body.app_id,
    };
    let need_acme = host.tls == "acme";
    let acme_host = host.hostname.clone();
    if let Some(existing) = state.hosts.iter_mut().find(|h| h.id == id) {
        *existing = host;
    } else {
        if state.hosts.iter().any(|h| h.hostname == host.hostname) {
            return Err(ApiError::Conflict("a host with that name already exists".into()));
        }
        state.hosts.push(host);
    }
    if need_acme {
        write_nginx(cfg, &state)?;
        reload_nginx()?;
        issue_acme(cfg, &acme_host)?;
    }
    save_state(cfg, &state)?;
    write_nginx(cfg, &state)?;
    reload_nginx()?;
    Ok(status(cfg))
}

pub fn delete_host(cfg: &Config, id: &str) -> Result<ProxyStatus, ApiError> {
    util::require_privileged()?;
    let mut state = load_state(cfg);
    let before = state.hosts.len();
    state.hosts.retain(|h| h.id != id);
    if state.hosts.len() == before {
        return Err(ApiError::NotFound);
    }
    save_state(cfg, &state)?;
    write_nginx(cfg, &state)?;
    reload_nginx()?;
    Ok(status(cfg))
}

pub fn ensure_dirs(cfg: &Config) {
    let _ = std::fs::create_dir_all(conf_d(cfg));
    let _ = std::fs::create_dir_all(dir(cfg).join("acme"));
    let state = load_state(cfg);
    let _ = write_nginx(cfg, &state);
}
