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
    pub openssl_available: bool,
    pub dashboard_upstream: String,
    pub dashboard_tls: String,
    pub dashboard_names: Vec<String>,
    pub ca_ready: bool,
    pub ca_fingerprint: Option<String>,
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

fn certs_dir(cfg: &Config) -> PathBuf {
    dir(cfg).join("certs")
}

fn restrict_key(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn require_openssl() -> Result<(), ApiError> {
    if util::which("openssl") {
        Ok(())
    } else {
        Err(ApiError::BadRequest("openssl is not installed".into()))
    }
}

fn ensure_ca(cfg: &Config) -> Result<(PathBuf, PathBuf), ApiError> {
    let certs = certs_dir(cfg);
    std::fs::create_dir_all(&certs)?;
    let crt = certs.join("ca.crt");
    let key = certs.join("ca.key");
    if crt.exists() && key.exists() {
        return Ok((crt, key));
    }
    require_openssl()?;
    let crt_s = crt.display().to_string();
    let key_s = key.display().to_string();
    util::run_ok(
        "openssl",
        &[
            "req",
            "-x509",
            "-nodes",
            "-newkey",
            "rsa:4096",
            "-keyout",
            &key_s,
            "-out",
            &crt_s,
            "-days",
            "3650",
            "-subj",
            "/O=CoduOS/CN=CoduOS LAN CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE,pathlen:0",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
    )?;
    restrict_key(&key);
    Ok((crt, key))
}

fn leaf_paths(cfg: &Config, stem: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let certs = certs_dir(cfg);
    (
        certs.join(format!("{stem}.crt")),
        certs.join(format!("{stem}.key")),
        certs.join(format!("{stem}.fullchain.crt")),
        certs.join(format!("{stem}.san")),
    )
}

fn leaf_trusted(ca: &Path, crt: &Path) -> bool {
    util::run(
        "openssl",
        &[
            "verify",
            "-CAfile",
            &ca.display().to_string(),
            &crt.display().to_string(),
        ],
    )
    .ok()
    .is_some_and(|out| out.status.success())
}

fn issue_leaf(cfg: &Config, stem: &str, sans: &[String]) -> Result<(PathBuf, PathBuf), ApiError> {
    if stem.is_empty()
        || stem.len() > 64
        || !stem
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::BadRequest("invalid certificate name".into()));
    }
    if sans.is_empty() {
        return Err(ApiError::BadRequest("certificate needs a hostname".into()));
    }
    let (ca_crt, ca_key) = ensure_ca(cfg)?;
    let (crt, key, fullchain, san_path) = leaf_paths(cfg, stem);
    let san_line = sans.join(",");
    if crt.exists()
        && key.exists()
        && fullchain.exists()
        && std::fs::read_to_string(&san_path).ok().as_deref() == Some(san_line.as_str())
        && leaf_trusted(&ca_crt, &crt)
    {
        return Ok((fullchain, key));
    }
    require_openssl()?;
    let certs = certs_dir(cfg);
    let csr = certs.join(format!("{stem}.csr"));
    let ext = certs.join(format!("{stem}.ext"));
    let cn = cert_cn(sans);
    let crt_s = crt.display().to_string();
    let key_s = key.display().to_string();
    let csr_s = csr.display().to_string();
    let ext_s = ext.display().to_string();
    let ca_crt_s = ca_crt.display().to_string();
    let ca_key_s = ca_key.display().to_string();
    let serial = certs.join("ca.srl").display().to_string();
    std::fs::write(
        &ext,
        format!(
            "basicConstraints = CA:FALSE\nkeyUsage = digitalSignature,keyEncipherment\nextendedKeyUsage = serverAuth\nsubjectAltName = {san_line}\n"
        ),
    )?;
    util::run_ok(
        "openssl",
        &[
            "req",
            "-new",
            "-nodes",
            "-newkey",
            "rsa:2048",
            "-keyout",
            &key_s,
            "-out",
            &csr_s,
            "-subj",
            &format!("/O=CoduOS/CN={cn}"),
        ],
    )?;
    restrict_key(&key);
    util::run_ok(
        "openssl",
        &[
            "x509",
            "-req",
            "-in",
            &csr_s,
            "-CA",
            &ca_crt_s,
            "-CAkey",
            &ca_key_s,
            "-CAserial",
            &serial,
            "-CAcreateserial",
            "-out",
            &crt_s,
            "-days",
            "825",
            "-extfile",
            &ext_s,
        ],
    )?;
    let mut chain = std::fs::read(&crt)?;
    chain.push(b'\n');
    chain.extend(std::fs::read(&ca_crt)?);
    std::fs::write(&fullchain, chain)?;
    std::fs::write(&san_path, &san_line)?;
    let _ = std::fs::remove_file(csr);
    let _ = std::fs::remove_file(ext);
    Ok((fullchain, key))
}

fn cert_cn(sans: &[String]) -> String {
    let raw = sans
        .iter()
        .find_map(|s| s.strip_prefix("DNS:"))
        .or_else(|| sans.iter().find_map(|s| s.strip_prefix("IP:")))
        .unwrap_or("coduos");
    let cn: String = raw.chars().take(64).collect();
    if cn.is_empty() {
        "coduos".into()
    } else {
        cn
    }
}

fn host_sans(hostname: &str) -> Vec<String> {
    if hostname.parse::<std::net::Ipv4Addr>().is_ok() {
        vec![format!("IP:{hostname}")]
    } else {
        vec![format!("DNS:{hostname}")]
    }
}

fn dashboard_sans() -> Vec<String> {
    let mut out = vec!["DNS:localhost".into(), "IP:127.0.0.1".into()];
    if let Ok(h) = hostname::get() {
        if let Ok(name) = h.into_string() {
            let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
            if util::valid_hostname(&name) {
                out.push(format!("DNS:{name}"));
                if !name.contains('.') {
                    let mdns = format!("{name}.local");
                    if util::valid_hostname(&mdns) {
                        out.push(format!("DNS:{mdns}"));
                    }
                }
            }
        }
    }
    for ip in lan_ipv4s() {
        out.push(format!("IP:{ip}"));
    }
    dedup_keep(out)
}

fn dashboard_display_names() -> Vec<String> {
    dashboard_sans()
        .into_iter()
        .filter_map(|s| {
            s.strip_prefix("DNS:")
                .or_else(|| s.strip_prefix("IP:"))
                .map(|v| v.to_string())
        })
        .filter(|n| n != "localhost" && n != "127.0.0.1")
        .collect()
}

fn dedup_keep(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

fn lan_ipv4s() -> Vec<String> {
    let mut ips = ipv4s_from_ip_json();
    if ips.is_empty() {
        ips = ipv4s_from_ip_text();
    }
    ips.sort();
    ips.dedup();
    ips
}

fn skip_lan_ip(ip: &str) -> bool {
    ip.parse::<std::net::Ipv4Addr>()
        .ok()
        .is_none_or(|a| a.is_loopback() || a.is_link_local() || a.is_unspecified() || a.is_multicast())
}

fn ipv4s_from_ip_json() -> Vec<String> {
    let Ok(out) = std::process::Command::new("ip")
        .args(["-j", "-4", "addr"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut ips = Vec::new();
    for iface in arr {
        let Some(infos) = iface.get("addr_info").and_then(|x| x.as_array()) else {
            continue;
        };
        for info in infos {
            if info.get("family").and_then(|x| x.as_str()) != Some("inet") {
                continue;
            }
            let Some(local) = info.get("local").and_then(|x| x.as_str()) else {
                continue;
            };
            if !skip_lan_ip(local) {
                ips.push(local.to_string());
            }
        }
    }
    ips
}

fn ipv4s_from_ip_text() -> Vec<String> {
    let Ok(out) = std::process::Command::new("ip")
        .args(["-o", "-4", "addr", "show"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            while let Some(tok) = parts.next() {
                if tok == "inet" {
                    let addr = parts.next()?.split('/').next()?;
                    if !skip_lan_ip(addr) {
                        return Some(addr.to_string());
                    }
                    return None;
                }
            }
            None
        })
        .collect()
}

fn ca_fingerprint(cfg: &Config) -> Option<String> {
    let crt = certs_dir(cfg).join("ca.crt");
    if !crt.exists() {
        return None;
    }
    let out = util::run_ok(
        "openssl",
        &[
            "x509",
            "-in",
            &crt.display().to_string(),
            "-noout",
            "-fingerprint",
            "-sha256",
        ],
    )
    .ok()?;
    out.trim()
        .rsplit('=')
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn remove_leaf_files(cfg: &Config, stem: &str) {
    let (crt, key, fullchain, san) = leaf_paths(cfg, stem);
    for p in [
        crt,
        key,
        fullchain,
        san,
        certs_dir(cfg).join(format!("{stem}.csr")),
        certs_dir(cfg).join(format!("{stem}.ext")),
    ] {
        let _ = std::fs::remove_file(p);
    }
}

pub fn ca_pem(cfg: &Config) -> Result<Vec<u8>, ApiError> {
    let crt = certs_dir(cfg).join("ca.crt");
    if !crt.exists() {
        ensure_ca(cfg)?;
    }
    std::fs::read(&crt).map_err(ApiError::internal)
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
        let (crt, key) = issue_leaf(cfg, "dashboard", &dashboard_sans())?;
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
                let (crt, key) = issue_leaf(cfg, &host.id, &host_sans(name))?;
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
    let ca_fp = ca_fingerprint(cfg);
    ProxyStatus {
        privileged: privileged(),
        nginx_available,
        certbot_available: util::which("certbot"),
        openssl_available: util::which("openssl"),
        dashboard_upstream: dashboard_upstream(cfg),
        dashboard_tls: state.dashboard_tls,
        dashboard_names: dashboard_display_names(),
        ca_ready: ca_fp.is_some(),
        ca_fingerprint: ca_fp,
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
    remove_leaf_files(cfg, id);
    save_state(cfg, &state)?;
    write_nginx(cfg, &state)?;
    reload_nginx()?;
    Ok(status(cfg))
}

pub fn enabled_hostnames(cfg: &Config) -> Vec<String> {
    load_state(cfg)
        .hosts
        .into_iter()
        .filter(|h| h.enabled)
        .map(|h| h.hostname.to_ascii_lowercase())
        .collect()
}

pub fn ensure_dirs(cfg: &Config) {
    let _ = std::fs::create_dir_all(conf_d(cfg));
    let _ = std::fs::create_dir_all(dir(cfg).join("acme"));
    let state = load_state(cfg);
    if let Err(err) = write_nginx(cfg, &state) {
        tracing::warn!("nginx conf: {err}");
        return;
    }
    // nginx often starts in the same second and loads an empty include glob.
    if let Err(err) = reload_nginx() {
        tracing::warn!("nginx reload: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_sans_dns_and_ip() {
        assert_eq!(host_sans("photos.home.arpa"), vec!["DNS:photos.home.arpa"]);
        assert_eq!(host_sans("192.168.1.20"), vec!["IP:192.168.1.20"]);
    }

    #[test]
    fn skip_loopback_and_link_local() {
        assert!(skip_lan_ip("127.0.0.1"));
        assert!(skip_lan_ip("169.254.1.1"));
        assert!(!skip_lan_ip("192.168.1.141"));
        assert!(!skip_lan_ip("10.0.0.2"));
    }

    #[test]
    fn cert_cn_prefers_dns() {
        assert_eq!(
            cert_cn(&["DNS:photos.home.arpa".into(), "IP:10.0.0.1".into()]),
            "photos.home.arpa"
        );
        assert_eq!(cert_cn(&["IP:10.0.0.1".into()]), "10.0.0.1");
    }

    #[test]
    fn issues_ca_and_per_host_leaf() {
        if !util::which("openssl") {
            return;
        }
        let tmp = std::env::temp_dir().join(format!(
            "coduos-ca-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut cfg = Config::for_environment();
        cfg.data_dir = tmp.clone();
        let (ca, _) = ensure_ca(&cfg).expect("ca");
        let pem = std::fs::read_to_string(&ca).unwrap();
        assert!(pem.contains("BEGIN CERTIFICATE"));
        let (full, key) = issue_leaf(
            &cfg,
            "photos",
            &["DNS:photos.home.arpa".into()],
        )
        .expect("leaf");
        assert!(full.exists());
        assert!(key.exists());
        let text = util::run_ok(
            "openssl",
            &[
                "x509",
                "-in",
                &full.display().to_string(),
                "-noout",
                "-text",
            ],
        )
        .unwrap();
        assert!(text.contains("photos.home.arpa"));
        assert!(text.contains("DNS:photos.home.arpa"));
        let _ = std::fs::remove_dir_all(tmp);
    }
}
