use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;

use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::ApiError;
use crate::util::{self, privileged, valid_hostname};

const IFACE: &str = "coduos";
const SUBNET_PREFIX: &str = "10.8.0.";
const SERVER_ADDR: &str = "10.8.0.1/24";
/// In-tunnel resolver. A LAN IP such as 192.168.1.1 cannot be used here: Android
/// sends DNS into the tunnel before handshake, so the endpoint never resolves.
const CLIENT_DNS: &str = "10.8.0.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnState {
    pub enabled: bool,
    pub listen_port: u16,
    pub endpoint: String,
    pub dns: String,
    pub public_key: String,
    pub private_key: String,
    pub peers: Vec<Peer>,
    #[serde(default)]
    pub next_ip: u8,
}

impl Default for VpnState {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_port: 51820,
            endpoint: String::new(),
            dns: CLIENT_DNS.into(),
            public_key: String::new(),
            private_key: String::new(),
            peers: vec![],
            next_ip: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub private_key: String,
    pub address: String,
    pub enabled: bool,
    /// `lan` or `full`
    pub tunnel: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VpnStatus {
    pub privileged: bool,
    pub wg_available: bool,
    pub enabled: bool,
    pub listen_port: u16,
    pub endpoint: String,
    pub dns: String,
    pub public_key: String,
    pub address: String,
    pub peers: Vec<PeerView>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerView {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub address: String,
    pub enabled: bool,
    pub tunnel: String,
    pub latest_handshake: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct VpnSettingsIn {
    pub enabled: Option<bool>,
    pub endpoint: Option<String>,
    pub dns: Option<String>,
    pub listen_port: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct PeerIn {
    pub name: String,
    pub tunnel: Option<String>,
}

fn dir(cfg: &Config) -> PathBuf {
    cfg.data_dir.join("wireguard")
}

fn state_path(cfg: &Config) -> PathBuf {
    dir(cfg).join("state.json")
}

fn conf_path(cfg: &Config) -> PathBuf {
    dir(cfg).join("coduos.conf")
}

fn load_state(cfg: &Config) -> VpnState {
    let path = state_path(cfg);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(cfg: &Config, state: &VpnState) -> Result<(), ApiError> {
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

fn wg_keys() -> Result<(String, String), ApiError> {
    use std::io::Write;
    let privkey = util::run_ok("wg", &["genkey"])?.trim().to_string();
    let mut child = std::process::Command::new("wg")
        .arg("pubkey")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ApiError::BadRequest(format!("wg pubkey: {e}")))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| ApiError::BadRequest("wg pubkey stdin".into()))?
        .write_all(privkey.as_bytes())
        .map_err(ApiError::internal)?;
    let out = child.wait_with_output().map_err(ApiError::internal)?;
    if !out.status.success() {
        return Err(ApiError::BadRequest(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok((
        privkey,
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

fn ensure_server_keys(state: &mut VpnState) -> Result<(), ApiError> {
    if !state.private_key.is_empty() && !state.public_key.is_empty() {
        return Ok(());
    }
    let (privkey, pubkey) = wg_keys()?;
    state.private_key = privkey;
    state.public_key = pubkey;
    Ok(())
}

fn allowed_ips(tunnel: &str) -> String {
    if tunnel == "full" {
        "0.0.0.0/0, ::/0".into()
    } else {
        lan_allowed_ips()
    }
}

fn lan_allowed_ips() -> String {
    let mut nets = vec!["10.8.0.0/24".to_string()];
    for cidr in lan_cidrs() {
        if !nets.iter().any(|n| n == &cidr) {
            nets.push(cidr);
        }
    }
    nets.join(", ")
}

fn ipv4_cidr(addr: &str, prefix: u8) -> Option<String> {
    if prefix > 32 {
        return None;
    }
    let ip: std::net::Ipv4Addr = addr.parse().ok()?;
    if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    let mask = if prefix == 0 {
        0
    } else {
        !0u32 << (32 - prefix)
    };
    let net = std::net::Ipv4Addr::from(u32::from(ip) & mask);
    Some(format!("{net}/{prefix}"))
}

fn lan_cidrs() -> Vec<String> {
    let mut nets = lan_cidrs_from_ip_json();
    if nets.is_empty() {
        nets = lan_cidrs_from_ip_text();
    }
    nets.sort();
    nets.dedup();
    nets
}

fn lan_cidrs_from_ip_json() -> Vec<String> {
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
    let mut nets = Vec::new();
    for iface in arr {
        let Some(name) = iface.get("ifname").and_then(|x| x.as_str()) else {
            continue;
        };
        if name == "lo" || crate::stats::is_virtual(name) {
            continue;
        }
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
            let prefix = info
                .get("prefixlen")
                .and_then(|x| x.as_u64())
                .unwrap_or(24)
                .min(32) as u8;
            if let Some(cidr) = ipv4_cidr(local, prefix) {
                if cidr != "10.8.0.0/24" {
                    nets.push(cidr);
                }
            }
        }
    }
    nets
}

fn lan_cidrs_from_ip_text() -> Vec<String> {
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
            let name = parts.nth(1)?;
            if name == "lo" || crate::stats::is_virtual(name) {
                return None;
            }
            while let Some(tok) = parts.next() {
                if tok == "inet" {
                    let spec = parts.next()?;
                    let (addr, prefix) = spec.split_once('/')?;
                    let prefix: u8 = prefix.parse().ok()?;
                    let cidr = ipv4_cidr(addr, prefix)?;
                    if cidr == "10.8.0.0/24" {
                        return None;
                    }
                    return Some(cidr);
                }
            }
            None
        })
        .collect()
}

fn default_gateway_v4() -> Option<String> {
    let out = std::process::Command::new("ip")
        .args(["-4", "route", "show", "default"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    while let Some(tok) = parts.next() {
        if tok == "via" {
            let ip = parts.next()?;
            if ip.parse::<std::net::Ipv4Addr>().is_ok() {
                return Some(ip.to_string());
            }
        }
    }
    None
}

fn is_lan_dns(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    addr.is_private() && addr != std::net::Ipv4Addr::new(10, 8, 0, 1)
}

fn client_dns(configured: &str) -> String {
    let v = configured.trim();
    if v.is_empty() || is_lan_dns(v) {
        CLIENT_DNS.into()
    } else {
        v.to_string()
    }
}

fn normalize_client_dns(raw: &str) -> Result<String, ApiError> {
    let v = raw.trim();
    if v.is_empty() {
        return Ok(CLIENT_DNS.into());
    }
    if v.parse::<std::net::Ipv4Addr>().is_err() && !valid_hostname(v) {
        return Err(ApiError::BadRequest("invalid DNS".into()));
    }
    if is_lan_dns(v) {
        return Err(ApiError::BadRequest(
            "use 10.8.0.1, not the router. Android sends DNS into the tunnel before handshake, so the VPN never comes up. CoduOS forwards 10.8.0.1:53 to the LAN resolver for .home names.".into(),
        ));
    }
    Ok(v.to_string())
}

fn ipv4_for_endpoint(host: &str) -> String {
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return host.to_string();
    }
    let Ok(mut addrs) = (host, 0u16).to_socket_addrs() else {
        return host.to_string();
    };
    addrs
        .find_map(|a| match a {
            SocketAddr::V4(v) => Some(v.ip().to_string()),
            SocketAddr::V6(_) => None,
        })
        .unwrap_or_else(|| host.to_string())
}

fn client_endpoint(state: &VpnState) -> String {
    let port = state.listen_port;
    let raw = state.endpoint.trim();
    if raw.is_empty() {
        return format!("nas.local:{port}");
    }
    let (host, port) = if let Ok(p) = raw.parse::<std::net::Ipv4Addr>() {
        (p.to_string(), port)
    } else if let Some((h, pstr)) = raw.rsplit_once(':') {
        if h.parse::<std::net::Ipv4Addr>().is_ok() || valid_hostname(h) {
            (h.to_string(), pstr.parse().unwrap_or(port))
        } else {
            (raw.to_string(), port)
        }
    } else {
        (raw.to_string(), port)
    };
    format!("{}:{port}", ipv4_for_endpoint(&host))
}

fn write_conf(cfg: &Config, state: &VpnState) -> Result<(), ApiError> {
    let mut body = String::new();
    body.push_str("[Interface]\n");
    body.push_str(&format!("PrivateKey = {}\n", state.private_key));
    body.push_str(&format!("Address = {SERVER_ADDR}\n"));
    body.push_str(&format!("ListenPort = {}\n", state.listen_port));
    body.push_str("PostUp = sysctl -qw net.ipv4.ip_forward=1\n");
    body.push_str("PostUp = sysctl -qw net.ipv4.conf.%i.rp_filter=2\n");
    body.push_str("PostUp = iptables -I FORWARD 1 -i %i -j ACCEPT || true\n");
    body.push_str(
        "PostUp = iptables -I FORWARD 1 -o %i -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT || true\n",
    );
    body.push_str(
        "PostUp = iptables -t nat -A POSTROUTING -s 10.8.0.0/24 ! -d 10.8.0.0/24 -j MASQUERADE || true\n",
    );
    if let Some(gw) = default_gateway_v4() {
        for proto in ["udp", "tcp"] {
            body.push_str(&format!(
                "PostUp = iptables -t nat -A PREROUTING -i %i -d 10.8.0.1 -p {proto} --dport 53 -j DNAT --to-destination {gw}:53 || true\n"
            ));
        }
    }
    body.push_str("PostDown = iptables -D FORWARD -i %i -j ACCEPT || true\n");
    body.push_str(
        "PostDown = iptables -D FORWARD -o %i -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT || true\n",
    );
    body.push_str(
        "PostDown = iptables -t nat -D POSTROUTING -s 10.8.0.0/24 ! -d 10.8.0.0/24 -j MASQUERADE || true\n",
    );
    if let Some(gw) = default_gateway_v4() {
        for proto in ["udp", "tcp"] {
            body.push_str(&format!(
                "PostDown = iptables -t nat -D PREROUTING -i %i -d 10.8.0.1 -p {proto} --dport 53 -j DNAT --to-destination {gw}:53 || true\n"
            ));
        }
    }
    body.push('\n');
    for peer in state.peers.iter().filter(|p| p.enabled) {
        body.push_str("[Peer]\n");
        body.push_str(&format!("PublicKey = {}\n", peer.public_key));
        body.push_str(&format!("AllowedIPs = {}\n", peer.address));
        body.push('\n');
    }
    let path = conf_path(cfg);
    std::fs::create_dir_all(dir(cfg))?;
    let tmp = path.with_extension("conf.tmp");
    std::fs::write(&tmp, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn apply(cfg: &Config, state: &VpnState) -> Result<(), ApiError> {
    write_conf(cfg, state)?;
    if !privileged() {
        return Ok(());
    }
    if state.enabled {
        let _ = util::run("systemctl", &["enable", "coduos-wg.service"]);
        let out = util::run("systemctl", &["restart", "coduos-wg.service"])?;
        if !out.status.success() {
            return Err(ApiError::BadRequest(format!(
                "coduos-wg.service: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    } else {
        let _ = util::run("systemctl", &["stop", "coduos-wg.service"]);
    }
    Ok(())
}

fn handshake_map() -> HashMap<String, (u64, u64, u64)> {
    let mut map = HashMap::new();
    let out = std::process::Command::new("wg")
        .args(["show", IFACE, "dump"])
        .output();
    let Ok(out) = out else {
        return map;
    };
    if !out.status.success() {
        return map;
    }
    for (i, line) in String::from_utf8_lossy(&out.stdout).lines().enumerate() {
        if i == 0 {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 7 {
            continue;
        }
        let pubkey = cols[0].to_string();
        let hs: u64 = cols[4].parse().unwrap_or(0);
        let rx: u64 = cols[5].parse().unwrap_or(0);
        let tx: u64 = cols[6].parse().unwrap_or(0);
        map.insert(pubkey, (hs, rx, tx));
    }
    map
}

use std::collections::HashMap;

fn view_peers(state: &VpnState) -> Vec<PeerView> {
    let stats = handshake_map();
    state
        .peers
        .iter()
        .map(|p| {
            let (hs, rx, tx) = stats.get(&p.public_key).copied().unwrap_or((0, 0, 0));
            PeerView {
                id: p.id.clone(),
                name: p.name.clone(),
                public_key: p.public_key.clone(),
                address: p.address.clone(),
                enabled: p.enabled,
                tunnel: p.tunnel.clone(),
                latest_handshake: if hs > 0 { Some(hs) } else { None },
                rx_bytes: rx,
                tx_bytes: tx,
            }
        })
        .collect()
}

pub fn status(cfg: &Config) -> VpnStatus {
    let state = load_state(cfg);
    let wg_available = util::which("wg");
    VpnStatus {
        privileged: privileged(),
        wg_available,
        enabled: state.enabled,
        listen_port: state.listen_port,
        endpoint: state.endpoint.clone(),
        dns: client_dns(&state.dns),
        public_key: state.public_key.clone(),
        address: SERVER_ADDR.into(),
        peers: view_peers(&state),
        error: if wg_available {
            None
        } else {
            Some("wireguard-tools (wg) is not installed".into())
        },
    }
}

pub fn update_settings(cfg: &Config, body: VpnSettingsIn) -> Result<VpnStatus, ApiError> {
    util::require_privileged()?;
    if !util::which("wg") {
        return Err(ApiError::BadRequest("install wireguard-tools first".into()));
    }
    let mut state = load_state(cfg);
    ensure_server_keys(&mut state)?;
    if let Some(v) = body.enabled {
        state.enabled = v;
    }
    if let Some(v) = body.endpoint {
        if !v.is_empty() {
            let host = v.split(':').next().unwrap_or(&v);
            if !valid_hostname(host) && host.parse::<std::net::Ipv4Addr>().is_err() {
                return Err(ApiError::BadRequest("invalid endpoint host".into()));
            }
        }
        state.endpoint = v;
    }
    if let Some(v) = body.dns {
        state.dns = normalize_client_dns(&v)?;
    }
    if let Some(p) = body.listen_port {
        if p < 1 {
            return Err(ApiError::BadRequest("invalid listen port".into()));
        }
        state.listen_port = p;
    }
    save_state(cfg, &state)?;
    apply(cfg, &state)?;
    Ok(status(cfg))
}

pub fn add_peer(cfg: &Config, body: PeerIn) -> Result<PeerView, ApiError> {
    util::require_privileged()?;
    let name = body.name.trim();
    if name.is_empty() || name.len() > 48 {
        return Err(ApiError::BadRequest("client name required".into()));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
    {
        return Err(ApiError::BadRequest("client name has invalid characters".into()));
    }
    let tunnel = match body.tunnel.as_deref().unwrap_or("lan") {
        "full" => "full",
        _ => "lan",
    };
    let mut state = load_state(cfg);
    ensure_server_keys(&mut state)?;
    if state.next_ip < 2 {
        state.next_ip = 2;
    }
    if state.next_ip > 250 {
        return Err(ApiError::BadRequest("no more client addresses in 10.8.0.0/24".into()));
    }
    let (privkey, pubkey) = wg_keys()?;
    let ip = state.next_ip;
    state.next_ip += 1;
    let id = crate::config::slugify(name);
    let mut id = if id.is_empty() { format!("peer-{ip}") } else { id };
    if state.peers.iter().any(|p| p.id == id) {
        id = format!("{id}-{ip}");
    }
    let peer = Peer {
        id: id.clone(),
        name: name.to_string(),
        public_key: pubkey,
        private_key: privkey,
        address: format!("{SUBNET_PREFIX}{ip}/32"),
        enabled: true,
        tunnel: tunnel.into(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state.peers.push(peer);
    save_state(cfg, &state)?;
    apply(cfg, &state)?;
    Ok(view_peers(&state)
        .into_iter()
        .find(|p| p.id == id)
        .ok_or(ApiError::Internal("peer missing after save".into()))?)
}

pub fn set_peer_enabled(cfg: &Config, id: &str, enabled: bool) -> Result<VpnStatus, ApiError> {
    util::require_privileged()?;
    let mut state = load_state(cfg);
    let peer = state
        .peers
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or(ApiError::NotFound)?;
    peer.enabled = enabled;
    save_state(cfg, &state)?;
    apply(cfg, &state)?;
    Ok(status(cfg))
}

pub fn revoke_peer(cfg: &Config, id: &str) -> Result<VpnStatus, ApiError> {
    util::require_privileged()?;
    let mut state = load_state(cfg);
    let before = state.peers.len();
    state.peers.retain(|p| p.id != id);
    if state.peers.len() == before {
        return Err(ApiError::NotFound);
    }
    save_state(cfg, &state)?;
    apply(cfg, &state)?;
    Ok(status(cfg))
}

fn client_conf(cfg: &Config, id: &str) -> Result<(Peer, String), ApiError> {
    let state = load_state(cfg);
    let peer = state
        .peers
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or(ApiError::NotFound)?;
    let endpoint = client_endpoint(&state);
    let conf = format!(
        "[Interface]\nPrivateKey = {}\nAddress = {}\nDNS = {}\n\n[Peer]\nPublicKey = {}\nAllowedIPs = {}\nEndpoint = {}\nPersistentKeepalive = 25\n",
        peer.private_key,
        peer.address.replace("/32", "/24"),
        client_dns(&state.dns),
        state.public_key,
        allowed_ips(&peer.tunnel),
        endpoint,
    );
    Ok((peer, conf))
}

pub fn peer_config(cfg: &Config, id: &str) -> Result<String, ApiError> {
    Ok(client_conf(cfg, id)?.1)
}

pub fn peer_qr(cfg: &Config, id: &str) -> Result<String, ApiError> {
    let conf = peer_config(cfg, id)?;
    let code = QrCode::new(conf.as_bytes())
        .map_err(|e| ApiError::BadRequest(format!("qr: {e}")))?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .build())
}

pub fn conf_file_name(cfg: &Config, id: &str) -> Result<String, ApiError> {
    let (peer, _) = client_conf(cfg, id)?;
    Ok(format!("{}.conf", crate::config::slugify(&peer.name)))
}

pub fn ensure_dirs(cfg: &Config) {
    let _ = std::fs::create_dir_all(dir(cfg));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_cidr_from_address() {
        assert_eq!(ipv4_cidr("192.168.1.140", 24).as_deref(), Some("192.168.1.0/24"));
        assert_eq!(ipv4_cidr("10.0.0.5", 8).as_deref(), Some("10.0.0.0/8"));
        assert_eq!(ipv4_cidr("127.0.0.1", 8), None);
    }

    #[test]
    fn full_tunnel_routes_everything() {
        assert_eq!(allowed_ips("full"), "0.0.0.0/0, ::/0");
    }

    #[test]
    fn lan_router_is_not_client_dns() {
        assert_eq!(client_dns("192.168.1.1"), "10.8.0.1");
        assert_eq!(client_dns("1.1.1.1"), "1.1.1.1");
        assert_eq!(client_dns(""), "10.8.0.1");
        assert!(normalize_client_dns("192.168.1.1").is_err());
        assert_eq!(normalize_client_dns("10.8.0.1").unwrap(), "10.8.0.1");
    }

    #[test]
    fn endpoint_host_port_split() {
        let mut state = VpnState::default();
        state.endpoint = "88.196.57.169".into();
        state.listen_port = 51820;
        assert_eq!(client_endpoint(&state), "88.196.57.169:51820");
        state.endpoint = "88.196.57.169:51820".into();
        assert_eq!(client_endpoint(&state), "88.196.57.169:51820");
    }
}
