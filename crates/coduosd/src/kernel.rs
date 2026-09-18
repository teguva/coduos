//! Host kernel knobs that Docker apps (Redis/Valkey, Immich) expect.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::ApiError;
use crate::util;

const CONF_PATH: &str = "/etc/sysctl.d/99-coduos-memory.conf";
const CONF: &str = "# Managed by CoduOS for Redis/Valkey (Immich) and similar Docker apps.\nvm.overcommit_memory = 1\n";
const PROC: &str = "/proc/sys/vm/overcommit_memory";
const HINT: &str = "Preferred for Immich and other Docker apps that run Redis. Lets the kernel grant RAM more freely; the OOM killer still stops runaway processes. Turn off only if you need the default Linux heuristic.";

#[derive(Debug, Clone, Serialize)]
pub struct KernelOut {
    pub privileged: bool,
    pub memory_overcommit: bool,
    pub applied: bool,
    pub hint: String,
}

#[derive(Debug, Deserialize)]
pub struct KernelIn {
    pub memory_overcommit: bool,
}

pub fn snapshot(cfg: &Config) -> KernelOut {
    KernelOut {
        privileged: util::privileged(),
        memory_overcommit: cfg.memory_overcommit,
        applied: read_overcommit() == Some(1),
        hint: HINT.into(),
    }
}

pub fn apply_persisted(cfg: &Config) {
    if !util::privileged() {
        return;
    }
    match apply(cfg.memory_overcommit) {
        Ok(Apply::Enabled) => tracing::info!("vm.overcommit_memory=1 (Redis/Valkey)"),
        Ok(Apply::Disabled) => tracing::info!("vm.overcommit_memory=0 (CoduOS drop-in removed)"),
        Ok(Apply::Unchanged) => {}
        Err(err) => tracing::warn!("vm.overcommit_memory: {err}"),
    }
}

pub fn apply_and_save(
    cfg: &mut Config,
    config_path: &Path,
    enabled: bool,
) -> Result<KernelOut, ApiError> {
    util::require_privileged()?;
    apply(enabled).map_err(ApiError::BadRequest)?;
    cfg.memory_overcommit = enabled;
    cfg.save(config_path).map_err(ApiError::internal)?;
    Ok(snapshot(cfg))
}

enum Apply {
    Enabled,
    Disabled,
    Unchanged,
}

fn apply(enabled: bool) -> Result<Apply, String> {
    if enabled {
        ensure_overcommit()
    } else {
        clear_overcommit()
    }
}

fn ensure_overcommit() -> Result<Apply, String> {
    let already = read_overcommit() == Some(1);
    let conf_ok = conf_matches();
    if already && conf_ok {
        return Ok(Apply::Unchanged);
    }
    if let Some(parent) = Path::new(CONF_PATH).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if !conf_ok {
        std::fs::write(CONF_PATH, CONF).map_err(|e| e.to_string())?;
    }
    if !already {
        sysctl_overcommit(1)?;
    }
    Ok(Apply::Enabled)
}

fn clear_overcommit() -> Result<Apply, String> {
    let ours = conf_is_ours();
    if ours {
        std::fs::remove_file(CONF_PATH).map_err(|e| e.to_string())?;
    }
    if read_overcommit() != Some(0) {
        sysctl_overcommit(0)?;
        return Ok(Apply::Disabled);
    }
    Ok(if ours {
        Apply::Disabled
    } else {
        Apply::Unchanged
    })
}

fn sysctl_overcommit(value: i32) -> Result<(), String> {
    let out = std::process::Command::new("sysctl")
        .args(["-w", &format!("vm.overcommit_memory={value}")])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.trim().to_string());
    }
    Ok(())
}

fn read_overcommit() -> Option<i32> {
    std::fs::read_to_string(PROC)
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn conf_matches() -> bool {
    std::fs::read_to_string(CONF_PATH)
        .ok()
        .is_some_and(|s| s.contains("vm.overcommit_memory = 1"))
}

fn conf_is_ours() -> bool {
    std::fs::read_to_string(CONF_PATH).ok().is_some_and(|s| {
        s.contains("Managed by CoduOS") && s.contains("vm.overcommit_memory")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_sets_overcommit() {
        assert!(CONF.contains("vm.overcommit_memory = 1"));
        assert!(CONF.contains("Managed by CoduOS"));
        assert!(CONF.starts_with('#'));
    }

    #[test]
    fn hint_explains_redis() {
        assert!(HINT.contains("Redis"));
        assert!(HINT.contains("Immich"));
    }
}
