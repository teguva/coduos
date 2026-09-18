//! Built-in laptop panel and backlight power (NAS-on-laptop).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::ApiError;
use crate::util;

const BACKLIGHT: &str = "/sys/class/backlight";
const DRM: &str = "/sys/class/drm";
const LEDS: &str = "/sys/class/leds";
const LID: &str = "/proc/acpi/button/lid";
const LOGIND_DROPIN_DIR: &str = "/etc/systemd/logind.conf.d";
const LOGIND_DROPIN: &str = "/etc/systemd/logind.conf.d/coduos-lid.conf";
const BL_POWER_ON: &str = "0";
const BL_POWER_OFF: &str = "4";
const LID_DROPIN: &str = "\
# Managed by CoduOS. Do not edit.
[Login]
HandleLidSwitch=ignore
HandleLidSwitchExternalPower=ignore
HandleLidSwitchDocked=ignore
";

#[derive(Debug, Clone, Serialize)]
pub struct DisplayOut {
    pub available: bool,
    pub privileged: bool,
    pub off: bool,
    pub ignore_lid: bool,
    pub lid_available: bool,
    pub hint: String,
    pub lid_hint: String,
}

#[derive(Debug, Deserialize)]
pub struct DisplayIn {
    #[serde(default)]
    pub off: Option<bool>,
    #[serde(default)]
    pub ignore_lid: Option<bool>,
}

pub fn available() -> bool {
    !backlights().is_empty()
}

pub fn lid_available() -> bool {
    lid_present() || available()
}

pub fn snapshot(cfg: &Config) -> DisplayOut {
    DisplayOut {
        available: available(),
        privileged: util::privileged(),
        off: cfg.display_off,
        ignore_lid: cfg.ignore_lid,
        lid_available: lid_available(),
        hint: "Powers down the built-in panel and backlight. HDMI and DisplayPort stay available. Turn this on when the laptop is a NAS."
            .into(),
        lid_hint: "Stops systemd from sleeping or powering off when the lid closes. A desktop session may still have its own lid action."
            .into(),
    }
}

pub fn apply_persisted(cfg: &Config) {
    if cfg.display_off {
        if let Err(err) = apply(true) {
            tracing::warn!("laptop display off: {err}");
        }
    }
    if cfg.ignore_lid {
        if let Err(err) = apply_lid(true) {
            tracing::warn!("ignore lid: {err}");
        }
    }
}

pub fn apply_and_save(
    cfg: &mut Config,
    config_path: &Path,
    off: Option<bool>,
    ignore_lid: Option<bool>,
) -> Result<DisplayOut, ApiError> {
    util::require_privileged()?;
    if off.is_none() && ignore_lid.is_none() {
        return Err(ApiError::BadRequest("nothing to change".into()));
    }
    if let Some(off) = off {
        if off && !available() {
            return Err(ApiError::BadRequest(
                "no laptop backlight found on this machine".into(),
            ));
        }
        apply(off)?;
        cfg.display_off = off;
    }
    if let Some(ignore) = ignore_lid {
        apply_lid(ignore).map_err(ApiError::BadRequest)?;
        cfg.ignore_lid = ignore;
    }
    cfg.save(config_path).map_err(ApiError::internal)?;
    Ok(snapshot(cfg))
}

fn apply(off: bool) -> Result<(), ApiError> {
    let mut ok = 0usize;
    let mut last_err: Option<String> = None;
    for dir in backlights() {
        match set_backlight(&dir, off) {
            Ok(()) => ok += 1,
            Err(err) => last_err = Some(err),
        }
    }
    for conn in internal_connectors() {
        if set_connector(&conn, off).is_ok() {
            ok += 1;
        }
    }
    if off {
        for led in keyboard_backlights() {
            let _ = write_trim(&led.join("brightness"), "0");
        }
    }
    if ok == 0 {
        return Err(ApiError::BadRequest(
            last_err.unwrap_or_else(|| "could not change the laptop display".into()),
        ));
    }
    Ok(())
}

fn set_backlight(dir: &Path, off: bool) -> Result<(), String> {
    if off {
        if dir.join("bl_power").exists() {
            let _ = write_trim(&dir.join("bl_power"), BL_POWER_OFF);
        }
        write_trim(&dir.join("brightness"), "0")?;
        Ok(())
    } else {
        if dir.join("bl_power").exists() {
            let _ = write_trim(&dir.join("bl_power"), BL_POWER_ON);
        }
        let max = std::fs::read_to_string(dir.join("max_brightness"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1);
        write_trim(&dir.join("brightness"), &max.to_string())
    }
}

fn set_connector(dir: &Path, off: bool) -> Result<(), String> {
    write_trim(&dir.join("status"), if off { "off" } else { "detect" })
}

fn lid_present() -> bool {
    let Ok(entries) = std::fs::read_dir(LID) else {
        return false;
    };
    entries.flatten().any(|e| e.path().is_dir())
}

fn apply_lid(ignore: bool) -> Result<(), String> {
    if ignore {
        std::fs::create_dir_all(LOGIND_DROPIN_DIR).map_err(|err| {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                "needs the installed daemon running as root".into()
            } else {
                format!("could not write logind drop-in: {err}")
            }
        })?;
        write_trim(Path::new(LOGIND_DROPIN), LID_DROPIN.trim_end())?;
    } else if Path::new(LOGIND_DROPIN).exists() {
        std::fs::remove_file(LOGIND_DROPIN).map_err(|err| {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                "needs the installed daemon running as root".into()
            } else {
                format!("could not remove logind drop-in: {err}")
            }
        })?;
    }
    reload_logind()
}

fn reload_logind() -> Result<(), String> {
    let out = std::process::Command::new("systemctl")
        .args(["kill", "-s", "HUP", "systemd-logind.service"])
        .output()
        .map_err(|err| format!("systemctl: {err}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.trim();
        return Err(if msg.is_empty() {
            "could not reload logind".into()
        } else {
            format!("could not reload logind: {msg}")
        });
    }
    Ok(())
}

fn backlights() -> Vec<PathBuf> {
    read_class_dirs(BACKLIGHT)
}

fn keyboard_backlights() -> Vec<PathBuf> {
    read_class_dirs(LEDS)
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains("kbd_backlight") || n.contains("kbd-backlight"))
        })
        .collect()
}

fn internal_connectors() -> Vec<PathBuf> {
    read_class_dirs(DRM)
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_internal_connector)
        })
        .collect()
}

fn is_internal_connector(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("EDP") || n.contains("LVDS") || n.contains("-DSI") || n.contains("_DSI")
}

fn read_class_dirs(root: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.exists())
        .collect();
    out.sort();
    out
}

fn write_trim(path: &Path, value: &str) -> Result<(), String> {
    std::fs::write(path, format!("{value}\n")).map_err(|err| {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            "needs the installed daemon running as root".into()
        } else {
            format!("could not write {}: {err}", path.display())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_connector_names() {
        assert!(is_internal_connector("card1-eDP-1"));
        assert!(is_internal_connector("card0-LVDS-1"));
        assert!(is_internal_connector("card1-DSI-1"));
        assert!(!is_internal_connector("card1-HDMI-A-1"));
        assert!(!is_internal_connector("card1-DP-1"));
        assert!(!is_internal_connector("card1-Writeback-1"));
    }

    #[test]
    fn lid_dropin_ignores_all_lid_actions() {
        assert!(LID_DROPIN.contains("HandleLidSwitch=ignore"));
        assert!(LID_DROPIN.contains("HandleLidSwitchExternalPower=ignore"));
        assert!(LID_DROPIN.contains("HandleLidSwitchDocked=ignore"));
        assert!(!LID_DROPIN.contains("HandleLidSwitch=suspend"));
    }
}
