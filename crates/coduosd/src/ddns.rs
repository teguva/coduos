use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::error::ApiError;
use crate::state::AppState;
use crate::util::{self, privileged};

static FILE: Mutex<()> = Mutex::new(());

const MIN_INTERVAL: u64 = 60;
const MAX_INTERVAL: u64 = 3600;
const DEFAULT_INTERVAL: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DdnsState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub zone: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub generic_url: String,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    #[serde(default)]
    pub ipv6: bool,
    #[serde(default)]
    pub proxied: bool,
    #[serde(default)]
    pub last_ipv4: String,
    #[serde(default)]
    pub last_ipv6: String,
    #[serde(default)]
    pub last_ok_at: String,
    #[serde(default)]
    pub last_checked_at: String,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub last_result: String,
}

fn default_provider() -> String {
    "cloudflare".into()
}
fn default_interval() -> u64 {
    DEFAULT_INTERVAL
}

impl Default for DdnsState {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_provider(),
            hostname: String::new(),
            zone: String::new(),
            username: String::new(),
            token: String::new(),
            password: String::new(),
            generic_url: String::new(),
            interval_secs: DEFAULT_INTERVAL,
            ipv6: false,
            proxied: false,
            last_ipv4: String::new(),
            last_ipv6: String::new(),
            last_ok_at: String::new(),
            last_checked_at: String::new(),
            last_error: String::new(),
            last_result: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DdnsStatus {
    pub privileged: bool,
    pub enabled: bool,
    pub provider: String,
    pub hostname: String,
    pub zone: String,
    pub username: String,
    pub token_set: bool,
    pub password_set: bool,
    pub generic_url: String,
    pub interval_secs: u64,
    pub ipv6: bool,
    pub proxied: bool,
    pub last_ipv4: String,
    pub last_ipv6: String,
    pub last_ok_at: String,
    pub last_checked_at: String,
    pub last_error: Option<String>,
    pub last_result: String,
}

#[derive(Debug, Deserialize)]
pub struct DdnsSettingsIn {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub hostname: Option<String>,
    pub zone: Option<String>,
    pub username: Option<String>,
    pub token: Option<String>,
    pub password: Option<String>,
    pub generic_url: Option<String>,
    pub interval_secs: Option<u64>,
    pub ipv6: Option<bool>,
    pub proxied: Option<bool>,
}

pub fn ensure_dirs(cfg: &Config) {
    let _ = std::fs::create_dir_all(dir(cfg));
}

pub fn status(cfg: &Config) -> DdnsStatus {
    let s = load_state(cfg);
    to_status(&s)
}

pub async fn update_settings(
    state: &AppState,
    body: DdnsSettingsIn,
) -> Result<DdnsStatus, ApiError> {
    let cfg = state.config.read().await;
    let mut s = load_state(&cfg);
    if let Some(v) = body.enabled {
        s.enabled = v;
    }
    if let Some(v) = body.provider {
        s.provider = normalize_provider(&v)?;
    }
    if let Some(v) = body.hostname {
        s.hostname = v.trim().trim_end_matches('.').to_lowercase();
        if !s.hostname.is_empty() && !util::valid_hostname(&s.hostname) {
            return Err(ApiError::BadRequest("invalid hostname".into()));
        }
    }
    if let Some(v) = body.zone {
        s.zone = v.trim().trim_end_matches('.').to_lowercase();
    }
    if let Some(v) = body.username {
        s.username = v.trim().to_string();
    }
    if let Some(v) = body.token.filter(|t| !t.is_empty()) {
        s.token = v;
    }
    if let Some(v) = body.password.filter(|p| !p.is_empty()) {
        s.password = v;
    }
    if let Some(v) = body.generic_url {
        s.generic_url = v.trim().to_string();
    }
    if let Some(v) = body.interval_secs {
        s.interval_secs = v.clamp(MIN_INTERVAL, MAX_INTERVAL);
    }
    if let Some(v) = body.ipv6 {
        s.ipv6 = v;
    }
    if let Some(v) = body.proxied {
        s.proxied = v;
    }
    save_state(&cfg, &s)?;
    drop(cfg);
    if s.enabled {
        refresh(state, false).await?;
        let cfg = state.config.read().await;
        return Ok(status(&cfg));
    }
    Ok(to_status(&s))
}

pub async fn refresh(state: &AppState, force: bool) -> Result<DdnsStatus, ApiError> {
    let cfg = state.config.read().await.clone();
    tick(&state.http, &cfg, force).await?;
    Ok(status(&cfg))
}

pub fn spawn_updater(state: AppState) {
    tokio::spawn(async move {
        {
            let cfg = state.config.read().await.clone();
            if load_state(&cfg).enabled {
                if let Err(err) = tick(&state.http, &cfg, false).await {
                    tracing::warn!("ddns: {err}");
                }
            }
        }
        let mut wait = tokio::time::interval(Duration::from_secs(30));
        wait.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        wait.tick().await;
        loop {
            wait.tick().await;
            let cfg = state.config.read().await.clone();
            let s = load_state(&cfg);
            if !s.enabled {
                continue;
            }
            if let Err(err) = tick(&state.http, &cfg, false).await {
                tracing::warn!("ddns: {err}");
            }
            let extra = s.interval_secs.saturating_sub(30).min(MAX_INTERVAL);
            if extra > 0 {
                tokio::time::sleep(Duration::from_secs(extra)).await;
            }
        }
    });
}

fn dir(cfg: &Config) -> PathBuf {
    cfg.data_dir.join("ddns")
}

fn state_path(cfg: &Config) -> PathBuf {
    dir(cfg).join("state.json")
}

fn load_state(cfg: &Config) -> DdnsState {
    let _g = FILE.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::read_to_string(state_path(cfg))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(cfg: &Config, state: &DdnsState) -> Result<(), ApiError> {
    let _g = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = dir(cfg);
    std::fs::create_dir_all(&dir)?;
    let path = state_path(cfg);
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(state).map_err(ApiError::internal)?;
    std::fs::write(&tmp, raw)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn to_status(s: &DdnsState) -> DdnsStatus {
    DdnsStatus {
        privileged: privileged(),
        enabled: s.enabled,
        provider: s.provider.clone(),
        hostname: s.hostname.clone(),
        zone: s.zone.clone(),
        username: s.username.clone(),
        token_set: !s.token.is_empty(),
        password_set: !s.password.is_empty(),
        generic_url: s.generic_url.clone(),
        interval_secs: s.interval_secs,
        ipv6: s.ipv6,
        proxied: s.proxied,
        last_ipv4: s.last_ipv4.clone(),
        last_ipv6: s.last_ipv6.clone(),
        last_ok_at: s.last_ok_at.clone(),
        last_checked_at: s.last_checked_at.clone(),
        last_error: if s.last_error.is_empty() {
            None
        } else {
            Some(s.last_error.clone())
        },
        last_result: s.last_result.clone(),
    }
}

fn normalize_provider(raw: &str) -> Result<String, ApiError> {
    let p = raw.trim().to_lowercase();
    match p.as_str() {
        "cloudflare" | "duckdns" | "noip" | "dynu" | "namecheap" | "generic" => Ok(p),
        _ => Err(ApiError::BadRequest(
            "provider must be cloudflare, duckdns, noip, dynu, namecheap, or generic".into(),
        )),
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

async fn tick(http: &reqwest::Client, cfg: &Config, force: bool) -> Result<(), ApiError> {
    let mut s = load_state(cfg);
    s.last_checked_at = now_rfc3339();
    if !s.enabled {
        save_state(cfg, &s)?;
        return Ok(());
    }
    if s.hostname.is_empty() && s.provider != "generic" {
        s.last_error = "hostname is required".into();
        s.last_result = "error".into();
        save_state(cfg, &s)?;
        return Err(ApiError::BadRequest(s.last_error.clone()));
    }
    let ipv4 = match public_ip(http, false).await {
        Ok(ip) => ip,
        Err(err) => {
            s.last_error = err.to_string();
            s.last_result = "error".into();
            save_state(cfg, &s)?;
            return Err(err);
        }
    };
    let ipv6 = if s.ipv6 {
        match public_ip(http, true).await {
            Ok(ip) => Some(ip),
            Err(err) => {
                tracing::warn!("ddns ipv6 lookup: {err}");
                None
            }
        }
    } else {
        None
    };
    let same = !force
        && s.last_ipv4 == ipv4
        && ipv6.as_deref().unwrap_or("") == s.last_ipv6
        && s.last_result == "ok";
    if same {
        s.last_result = "unchanged".into();
        s.last_error.clear();
        save_state(cfg, &s)?;
        return Ok(());
    }
    match apply_provider(http, &s, &ipv4, ipv6.as_deref()).await {
        Ok(()) => {
            s.last_ipv4 = ipv4;
            s.last_ipv6 = ipv6.unwrap_or_default();
            s.last_ok_at = now_rfc3339();
            s.last_error.clear();
            s.last_result = "ok".into();
            save_state(cfg, &s)?;
            Ok(())
        }
        Err(err) => {
            s.last_error = err.to_string();
            s.last_result = "error".into();
            save_state(cfg, &s)?;
            Err(err)
        }
    }
}

async fn apply_provider(
    http: &reqwest::Client,
    s: &DdnsState,
    ipv4: &str,
    ipv6: Option<&str>,
) -> Result<(), ApiError> {
    match s.provider.as_str() {
        "cloudflare" => cloudflare(http, s, ipv4, ipv6).await,
        "duckdns" => duckdns(http, s, ipv4, ipv6).await,
        "noip" => dyndns2(http, s, "https://dynupdate.no-ip.com/nic/update", ipv4).await,
        "dynu" => dynu(http, s, ipv4).await,
        "namecheap" => namecheap(http, s, ipv4).await,
        "generic" => generic(http, s, ipv4, ipv6).await,
        other => Err(ApiError::BadRequest(format!("unknown provider {other}"))),
    }
}

async fn public_ip(http: &reqwest::Client, v6: bool) -> Result<String, ApiError> {
    let urls: &[&str] = if v6 {
        &["https://api6.ipify.org", "https://ipv6.icanhazip.com"]
    } else {
        &[
            "https://api.ipify.org",
            "https://ipv4.icanhazip.com",
            "https://ifconfig.me/ip",
        ]
    };
    let mut last = ApiError::BadRequest("no public IP lookup URL worked".into());
    for url in urls {
        match fetch_text(http, url).await {
            Ok(body) => {
                let ip = body.trim();
                if v6 {
                    if ip.parse::<Ipv6Addr>().is_ok() {
                        return Ok(ip.to_string());
                    }
                } else if ip.parse::<Ipv4Addr>().is_ok() {
                    return Ok(ip.to_string());
                }
                last = ApiError::BadRequest(format!("{url} returned a non-IP"));
            }
            Err(err) => last = err,
        }
    }
    Err(last)
}

async fn fetch_text(http: &reqwest::Client, url: &str) -> Result<String, ApiError> {
    let res = http
        .get(url)
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("lookup {url}: {e}")))?;
    let status = res.status();
    let body = res
        .text()
        .await
        .map_err(|e| ApiError::BadRequest(format!("lookup body: {e}")))?;
    if !status.is_success() {
        return Err(ApiError::BadRequest(format!(
            "{url} HTTP {status}: {}",
            body.trim()
        )));
    }
    Ok(body)
}

async fn cloudflare(
    http: &reqwest::Client,
    s: &DdnsState,
    ipv4: &str,
    ipv6: Option<&str>,
) -> Result<(), ApiError> {
    if s.token.is_empty() {
        return Err(ApiError::BadRequest("Cloudflare API token is required".into()));
    }
    let zone_name = if s.zone.is_empty() {
        zone_from_host(&s.hostname)
    } else {
        s.zone.clone()
    };
    let zone_id = cf_zone_id(http, &s.token, &zone_name).await?;
    cf_upsert(http, &s.token, &zone_id, &s.hostname, "A", ipv4, s.proxied).await?;
    if let Some(ip) = ipv6 {
        cf_upsert(
            http,
            &s.token,
            &zone_id,
            &s.hostname,
            "AAAA",
            ip,
            s.proxied,
        )
        .await?;
    }
    Ok(())
}

async fn cf_zone_id(http: &reqwest::Client, token: &str, zone: &str) -> Result<String, ApiError> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones?name={}",
        q(zone)
    );
    let v = cf_get(http, token, &url).await?;
    let id = v["result"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|z| z["id"].as_str())
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "Cloudflare zone {zone} not found for this token"
        )));
    }
    Ok(id)
}

async fn cf_upsert(
    http: &reqwest::Client,
    token: &str,
    zone_id: &str,
    name: &str,
    rtype: &str,
    content: &str,
    proxied: bool,
) -> Result<(), ApiError> {
    let list = format!(
        "https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records?type={rtype}&name={}",
        q(name)
    );
    let v = cf_get(http, token, &list).await?;
    let rec_id = v["result"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r["id"].as_str())
        .map(|s| s.to_string());
    let body = json!({
        "type": rtype,
        "name": name,
        "content": content,
        "ttl": 1,
        "proxied": proxied
    });
    let req = if let Some(id) = rec_id {
        http.put(format!(
            "https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records/{id}"
        ))
        .json(&body)
    } else {
        http.post(format!(
            "https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records"
        ))
        .json(&body)
    };
    let res = req
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("Cloudflare: {e}")))?;
    let status = res.status();
    let v: serde_json::Value = res.json().await.unwrap_or(json!({}));
    if !status.is_success() || v["success"] == false {
        let msg = cf_err(&v);
        return Err(ApiError::BadRequest(format!("Cloudflare: {msg}")));
    }
    Ok(())
}

async fn cf_get(http: &reqwest::Client, token: &str, url: &str) -> Result<serde_json::Value, ApiError> {
    let res = http
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("Cloudflare: {e}")))?;
    let status = res.status();
    let v: serde_json::Value = res.json().await.unwrap_or(json!({}));
    if !status.is_success() || v["success"] == false {
        return Err(ApiError::BadRequest(format!("Cloudflare: {}", cf_err(&v))));
    }
    Ok(v)
}

fn cf_err(v: &serde_json::Value) -> String {
    v["errors"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|e| e["message"].as_str())
        .unwrap_or("request failed")
        .to_string()
}

async fn duckdns(
    http: &reqwest::Client,
    s: &DdnsState,
    ipv4: &str,
    ipv6: Option<&str>,
) -> Result<(), ApiError> {
    if s.token.is_empty() {
        return Err(ApiError::BadRequest("DuckDNS token is required".into()));
    }
    let domain = duck_name(&s.hostname);
    let mut url = format!(
        "https://www.duckdns.org/update?domains={}&token={}&ip={}",
        q(&domain),
        q(&s.token),
        q(ipv4)
    );
    if let Some(ip) = ipv6 {
        url.push_str("&ipv6=");
        url.push_str(&q(ip));
    }
    let body = fetch_text(http, &url).await?;
    if body.trim().eq_ignore_ascii_case("ok") {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "DuckDNS: {}",
            body.trim()
        )))
    }
}

async fn dynu(http: &reqwest::Client, s: &DdnsState, ipv4: &str) -> Result<(), ApiError> {
    let pass = if !s.password.is_empty() {
        s.password.as_str()
    } else {
        s.token.as_str()
    };
    if pass.is_empty() {
        return Err(ApiError::BadRequest("Dynu password or token is required".into()));
    }
    let mut url = format!(
        "https://api.dynu.com/nic/update?hostname={}&password={}&myip={}",
        q(&s.hostname),
        q(pass),
        q(ipv4)
    );
    if !s.username.is_empty() {
        url.push_str("&username=");
        url.push_str(&q(&s.username));
    }
    nic_update(http, http.get(&url)).await
}

async fn dyndns2(
    http: &reqwest::Client,
    s: &DdnsState,
    endpoint: &str,
    ipv4: &str,
) -> Result<(), ApiError> {
    if s.username.is_empty() || s.password.is_empty() {
        return Err(ApiError::BadRequest("username and password are required".into()));
    }
    let url = format!(
        "{endpoint}?hostname={}&myip={}",
        q(&s.hostname),
        q(ipv4)
    );
    nic_update(
        http,
        http.get(&url).basic_auth(&s.username, Some(&s.password)),
    )
    .await
}

async fn namecheap(http: &reqwest::Client, s: &DdnsState, ipv4: &str) -> Result<(), ApiError> {
    if s.password.is_empty() {
        return Err(ApiError::BadRequest(
            "Namecheap dynamic DNS password is required".into(),
        ));
    }
    let (host, domain) = split_namecheap(&s.hostname);
    let url = format!(
        "https://dynamicdns.park-your-domain.com/update?host={}&domain={}&password={}&ip={}",
        q(&host),
        q(&domain),
        q(&s.password),
        q(ipv4)
    );
    let body = fetch_text(http, &url).await?;
    if body.to_lowercase().contains("<errcount>0</errcount>")
        || body.to_lowercase().contains("good")
    {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "Namecheap: {}",
            body.trim().chars().take(200).collect::<String>()
        )))
    }
}

async fn generic(
    http: &reqwest::Client,
    s: &DdnsState,
    ipv4: &str,
    ipv6: Option<&str>,
) -> Result<(), ApiError> {
    if s.generic_url.is_empty() {
        return Err(ApiError::BadRequest("update URL is required".into()));
    }
    let url = s
        .generic_url
        .replace("{ip}", ipv4)
        .replace("{ipv4}", ipv4)
        .replace("{ipv6}", ipv6.unwrap_or(""))
        .replace("{hostname}", &s.hostname)
        .replace("{host}", &s.hostname);
    let body = fetch_text(http, &url).await?;
    let t = body.trim().to_lowercase();
    if t.contains("badauth") || t.contains("nohost") || t.contains("abuse") || t.starts_with("ko")
    {
        return Err(ApiError::BadRequest(format!("provider: {}", body.trim())));
    }
    Ok(())
}

async fn nic_update(
    _http: &reqwest::Client,
    req: reqwest::RequestBuilder,
) -> Result<(), ApiError> {
    let res = req
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("update: {e}")))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    let t = body.trim().to_lowercase();
    if !status.is_success() {
        return Err(ApiError::BadRequest(format!(
            "HTTP {status}: {}",
            body.trim()
        )));
    }
    if t.starts_with("good") || t.starts_with("nochg") || t.contains("updated") || t == "ok" {
        return Ok(());
    }
    if t.contains("badauth") {
        return Err(ApiError::BadRequest("provider rejected the password".into()));
    }
    if t.contains("nohost") {
        return Err(ApiError::BadRequest("hostname is not on this account".into()));
    }
    Err(ApiError::BadRequest(format!("provider: {}", body.trim())))
}

fn zone_from_host(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        host.to_string()
    }
}

fn duck_name(host: &str) -> String {
    host.trim()
        .trim_end_matches('.')
        .to_lowercase()
        .strip_suffix(".duckdns.org")
        .unwrap_or(host)
        .to_string()
}

fn split_namecheap(host: &str) -> (String, String) {
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 2 {
        ("@".into(), host.to_string())
    } else {
        (
            parts[..parts.len() - 2].join("."),
            format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]),
        )
    }
}

fn q(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_zone() {
        assert_eq!(zone_from_host("nas.example.com"), "example.com");
        assert_eq!(zone_from_host("example.com"), "example.com");
    }

    #[test]
    fn duck_strips_suffix() {
        assert_eq!(duck_name("home.duckdns.org"), "home");
        assert_eq!(duck_name("home"), "home");
    }

    #[test]
    fn namecheap_host() {
        assert_eq!(
            split_namecheap("nas.example.com"),
            ("nas".into(), "example.com".into())
        );
        assert_eq!(
            split_namecheap("example.com"),
            ("@".into(), "example.com".into())
        );
    }
}
