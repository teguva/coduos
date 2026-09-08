use std::process::Output;

use crate::error::ApiError;

pub fn privileged() -> bool {
    crate::config::running_as_root()
}

pub fn require_privileged() -> Result<(), ApiError> {
    if privileged() {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "needs the installed daemon running as root".into(),
        ))
    }
}

pub fn valid_hostname(name: &str) -> bool {
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

pub fn valid_hostport_host(host: &str) -> bool {
    if valid_hostname(host) {
        return true;
    }
    let ok_ip = host.parse::<std::net::Ipv4Addr>().is_ok()
        || host.parse::<std::net::Ipv6Addr>().is_ok();
    ok_ip
}

pub fn safe_label(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, '-' | '_' | ' ') && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "disk".into()
    } else {
        out
    }
}

pub fn run(cmd: &str, args: &[&str]) -> Result<Output, ApiError> {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .map_err(|err| ApiError::BadRequest(format!("{cmd}: {err}")))
}

pub fn run_ok(cmd: &str, args: &[&str]) -> Result<String, ApiError> {
    let out = run(cmd, args)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let msg = err.trim();
        let msg = if msg.is_empty() { stdout.trim() } else { msg };
        return Err(ApiError::BadRequest(format!(
            "{cmd} failed: {msg}"
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn which(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
