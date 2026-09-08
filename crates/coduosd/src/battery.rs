//! Host battery status and charge-limit knobs (ThinkPad thresholds, IdeaPad
//! conservation / charge_types, generic `charge_control_*_threshold`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::ApiError;
use crate::util;

const POWER_SUPPLY: &str = "/sys/class/power_supply";

#[derive(Debug, Clone, Serialize)]
pub struct BatteryPack {
    pub present: bool,
    pub privileged: bool,
    pub ac_online: bool,
    pub batteries: Vec<BatteryCell>,
    pub limit: ChargeLimit,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatteryCell {
    pub id: String,
    pub name: String,
    pub capacity_pct: Option<u8>,
    pub status: String,
    pub charging: bool,
    pub ac_online: bool,
    pub power_w: Option<f32>,
    pub health_pct: Option<u8>,
    pub cycle_count: Option<u32>,
    pub limit_pct: Option<u8>,
    pub start_pct: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChargeLimit {
    pub supported: bool,
    pub kind: Option<String>,
    pub can_set: bool,
    pub limit_pct: Option<u8>,
    pub start_pct: Option<u8>,
    pub min_pct: u8,
    pub max_pct: u8,
    pub presets: Vec<u8>,
    pub charge_type: Option<String>,
    pub charge_types: Vec<String>,
    pub conservation: Option<bool>,
    pub hint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChargeLimitIn {
    pub limit_pct: u8,
    pub start_pct: Option<u8>,
}

struct Hardware {
    bats: Vec<BatDev>,
    ac_online: bool,
    conservation: Option<PathBuf>,
}

struct BatDev {
    id: String,
    path: PathBuf,
    start: Option<PathBuf>,
    stop: Option<PathBuf>,
    charge_types: Option<PathBuf>,
}

pub fn snapshot() -> BatteryPack {
    let hw = scan();
    let limit = read_limit(&hw);
    let batteries = hw
        .bats
        .iter()
        .filter_map(|b| read_cell(b, hw.ac_online, limit.limit_pct, limit.start_pct))
        .collect::<Vec<_>>();
    BatteryPack {
        present: !batteries.is_empty(),
        privileged: util::privileged(),
        ac_online: hw.ac_online,
        batteries,
        limit,
    }
}

pub fn summary_batteries() -> Vec<BatteryCell> {
    snapshot().batteries
}

pub fn apply_persisted(cfg: &Config) {
    let Some(limit) = cfg.battery.limit_pct else {
        return;
    };
    if let Err(err) = apply(limit, cfg.battery.start_pct) {
        tracing::warn!("battery charge limit: {err}");
    }
}

pub fn apply_and_save(
    cfg: &mut Config,
    config_path: &Path,
    limit_pct: u8,
    start_pct: Option<u8>,
) -> Result<BatteryPack, ApiError> {
    util::require_privileged()?;
    apply(limit_pct, start_pct)?;
    cfg.battery.limit_pct = Some(limit_pct);
    cfg.battery.start_pct = start_pct;
    cfg.save(config_path).map_err(ApiError::internal)?;
    Ok(snapshot())
}

fn apply(limit_pct: u8, start_pct: Option<u8>) -> Result<(), ApiError> {
    if !(50..=100).contains(&limit_pct) {
        return Err(ApiError::BadRequest(
            "charge limit must be between 50% and 100%".into(),
        ));
    }
    let hw = scan();
    if hw.bats.is_empty() {
        return Err(ApiError::BadRequest("no battery found".into()));
    }
    let kind = detect_kind(&hw);
    match kind.as_deref() {
        Some("thresholds") | Some("end_only") => apply_thresholds(&hw, limit_pct, start_pct),
        Some("charge_type") => apply_charge_type(&hw, limit_pct),
        Some("conservation") => apply_conservation(&hw, limit_pct < 95),
        _ => Err(ApiError::BadRequest(
            "this machine has no charge limiter in sysfs".into(),
        )),
    }
}

fn apply_thresholds(hw: &Hardware, stop: u8, start_in: Option<u8>) -> Result<(), ApiError> {
    let start = start_in
        .unwrap_or_else(|| default_start(stop))
        .min(stop.saturating_sub(1));
    if start >= stop {
        return Err(ApiError::BadRequest(
            "start threshold must be below the stop threshold".into(),
        ));
    }
    for bat in &hw.bats {
        let Some(stop_path) = bat.stop.as_ref() else {
            continue;
        };
        if let Some(start_path) = bat.start.as_ref() {
            let cur_stop = read_u8(stop_path).unwrap_or(100);
            if start >= cur_stop {
                write_pct(stop_path, stop)?;
                write_pct(start_path, start)?;
            } else {
                write_pct(start_path, start)?;
                write_pct(stop_path, stop)?;
            }
        } else {
            write_pct(stop_path, stop)?;
        }
    }
    // Keep IdeaPad conservation in sync when both exist.
    if hw.conservation.is_some() {
        let _ = apply_conservation(hw, stop < 95);
    }
    Ok(())
}

fn apply_charge_type(hw: &Hardware, limit_pct: u8) -> Result<(), ApiError> {
    let limited = limit_pct < 95;
    for bat in &hw.bats {
        let Some(path) = bat.charge_types.as_ref() else {
            continue;
        };
        let raw = read_trim(path);
        let (types, _) = parse_charge_types(&raw);
        let want = pick_charge_type(&types, limited).ok_or_else(|| {
            ApiError::BadRequest("no matching charge type for this limit".into())
        })?;
        write_str(path, &want)?;
    }
    if hw.conservation.is_some() {
        apply_conservation(hw, limited)?;
    }
    Ok(())
}

fn apply_conservation(hw: &Hardware, on: bool) -> Result<(), ApiError> {
    let Some(path) = hw.conservation.as_ref() else {
        return Err(ApiError::BadRequest("conservation mode is not available".into()));
    };
    write_str(path, if on { "1" } else { "0" })
}

fn default_start(stop: u8) -> u8 {
    if stop >= 100 {
        95
    } else {
        stop.saturating_sub(5).max(40)
    }
}

fn detect_kind(hw: &Hardware) -> Option<String> {
    let has_stop = hw.bats.iter().any(|b| b.stop.is_some());
    let has_start = hw.bats.iter().any(|b| b.start.is_some());
    let has_types = hw.bats.iter().any(|b| b.charge_types.is_some());
    if has_stop && has_start {
        Some("thresholds".into())
    } else if has_stop {
        Some("end_only".into())
    } else if has_types {
        Some("charge_type".into())
    } else if hw.conservation.is_some() {
        Some("conservation".into())
    } else {
        None
    }
}

fn read_limit(hw: &Hardware) -> ChargeLimit {
    let kind = detect_kind(hw);
    let mut start_pct = None;
    let mut stop_pct = None;
    let mut charge_types = Vec::new();
    let mut charge_type = None;
    for bat in &hw.bats {
        if start_pct.is_none() {
            start_pct = bat.start.as_ref().and_then(|p| read_u8(p));
        }
        if stop_pct.is_none() {
            stop_pct = bat.stop.as_ref().and_then(|p| read_u8(p));
        }
        if let Some(path) = bat.charge_types.as_ref() {
            let (types, current) = parse_charge_types(&read_trim(path));
            if charge_types.is_empty() {
                charge_types = types;
                charge_type = current;
            }
        }
    }
    let conservation = hw.conservation.as_ref().and_then(|p| {
        let v = read_trim(p);
        match v.as_str() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        }
    });
    let long_life = charge_type
        .as_deref()
        .is_some_and(|t| t.eq_ignore_ascii_case("Long_Life") || t.eq_ignore_ascii_case("Long Life"));
    let limited = conservation == Some(true) || long_life;
    let (limit_pct, presets, min_pct, hint) = match kind.as_deref() {
        Some("thresholds") | Some("end_only") => (
            stop_pct,
            vec![60, 80, 90, 100],
            50,
            Some(
                "Charging starts below the lower threshold and stops at the upper one, so a plugged-in laptop does not sit at 100%."
                    .into(),
            ),
        ),
        Some("charge_type") | Some("conservation") => (
            Some(if limited { 80 } else { 100 }),
            vec![80, 100],
            80,
            Some(
                "This laptop supports a conservation / Long Life hold around 80%, or a full charge."
                    .into(),
            ),
        ),
        _ => (None, vec![], 50, None),
    };
    ChargeLimit {
        supported: kind.is_some(),
        can_set: kind.is_some() && util::privileged(),
        kind,
        limit_pct,
        start_pct,
        min_pct,
        max_pct: 100,
        presets,
        charge_type,
        charge_types,
        conservation,
        hint,
    }
}

fn read_cell(
    bat: &BatDev,
    ac_online: bool,
    limit_pct: Option<u8>,
    start_pct: Option<u8>,
) -> Option<BatteryCell> {
    let present = read_trim(&bat.path.join("present"));
    if present == "0" {
        return None;
    }
    let status = read_trim(&bat.path.join("status"));
    if status.is_empty() && present.is_empty() {
        return None;
    }
    let model = read_trim(&bat.path.join("model_name"));
    let mfr = read_trim(&bat.path.join("manufacturer"));
    let name = [mfr.as_str(), model.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let capacity_pct = read_u8(&bat.path.join("capacity"));
    let charging = status.eq_ignore_ascii_case("Charging");
    Some(BatteryCell {
        id: bat.id.clone(),
        name: if name.is_empty() {
            bat.id.clone()
        } else {
            name
        },
        capacity_pct,
        status: if status.is_empty() {
            "Unknown".into()
        } else {
            status
        },
        charging,
        ac_online,
        power_w: power_w(&bat.path),
        health_pct: health_pct(&bat.path),
        cycle_count: read_u32(&bat.path.join("cycle_count")).filter(|n| *n > 0),
        limit_pct,
        start_pct,
    })
}

fn power_w(dev: &Path) -> Option<f32> {
    if let Some(uw) = read_u64(&dev.join("power_now")) {
        let w = uw as f32 / 1_000_000.0;
        if w.is_finite() && w >= 0.0 && w < 200.0 {
            return Some(w);
        }
    }
    let ua = read_u64(&dev.join("current_now"))?;
    let uv = read_u64(&dev.join("voltage_now"))?;
    let w = (ua as f64 * uv as f64) / 1e12;
    if w.is_finite() && w >= 0.0 && w < 200.0 {
        Some(w as f32)
    } else {
        None
    }
}

fn health_pct(dev: &Path) -> Option<u8> {
    let (now, full) = if let (Some(n), Some(f)) = (
        read_u64(&dev.join("energy_full")),
        read_u64(&dev.join("energy_full_design")),
    ) {
        (n, f)
    } else {
        (
            read_u64(&dev.join("charge_full"))?,
            read_u64(&dev.join("charge_full_design"))?,
        )
    };
    if full == 0 {
        return None;
    }
    let pct = (now as f64 / full as f64 * 100.0).round();
    if pct.is_finite() && pct > 0.0 && pct <= 150.0 {
        Some(pct.min(100.0) as u8)
    } else {
        None
    }
}

fn scan() -> Hardware {
    let mut bats = Vec::new();
    let mut ac_online = false;
    if let Ok(entries) = std::fs::read_dir(POWER_SUPPLY) {
        for ent in entries.flatten() {
            let path = ent.path();
            let kind = read_trim(&path.join("type")).to_ascii_lowercase();
            match kind.as_str() {
                "battery" => bats.push(bat_dev(&path)),
                "mains" | "usb" => {
                    if read_trim(&path.join("online")) == "1" {
                        ac_online = true;
                    }
                }
                _ => {}
            }
        }
    }
    Hardware {
        bats,
        ac_online,
        conservation: find_conservation(),
    }
}

fn bat_dev(path: &Path) -> BatDev {
    let id = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "BAT".into());
    let start = first_existing(path, &[
        "charge_control_start_threshold",
        "charge_start_threshold",
    ]);
    let stop = first_existing(path, &[
        "charge_control_end_threshold",
        "charge_stop_threshold",
    ]);
    let charge_types = first_existing(path, &["charge_types"]);
    BatDev {
        id,
        path: path.to_path_buf(),
        start,
        stop,
        charge_types,
    }
}

fn first_existing(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names.iter().map(|n| dir.join(n)).find(|p| p.exists())
}

fn find_conservation() -> Option<PathBuf> {
    let drivers = PathBuf::from("/sys/bus/platform/drivers/ideapad_acpi");
    if let Ok(entries) = std::fs::read_dir(&drivers) {
        for ent in entries.flatten() {
            let p = ent.path().join("conservation_mode");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let devices = PathBuf::from("/sys/bus/platform/devices");
    if let Ok(entries) = std::fs::read_dir(&devices) {
        for ent in entries.flatten() {
            let p = ent.path().join("conservation_mode");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn parse_charge_types(raw: &str) -> (Vec<String>, Option<String>) {
    let mut types = Vec::new();
    let mut current = None;
    let mut buf = String::new();
    let mut in_brack = false;
    for c in raw.chars() {
        match c {
            '[' => {
                flush_token(&mut buf, &mut types);
                in_brack = true;
            }
            ']' => {
                if !buf.is_empty() {
                    current = Some(buf.clone());
                    types.push(std::mem::take(&mut buf));
                }
                in_brack = false;
            }
            c if c.is_whitespace() && !in_brack => flush_token(&mut buf, &mut types),
            c => buf.push(c),
        }
    }
    flush_token(&mut buf, &mut types);
    (types, current)
}

fn flush_token(buf: &mut String, types: &mut Vec<String>) {
    if !buf.is_empty() {
        types.push(std::mem::take(buf));
    }
}

fn pick_charge_type(types: &[String], limited: bool) -> Option<String> {
    let find = |needles: &[&str]| {
        types
            .iter()
            .find(|t| needles.iter().any(|n| t.eq_ignore_ascii_case(n)))
            .cloned()
    };
    if limited {
        find(&["Long_Life", "Long Life", "Custom"])
    } else {
        find(&["Standard", "Fast", "Normal"]).or_else(|| {
            types
                .iter()
                .find(|t| {
                    !t.eq_ignore_ascii_case("Long_Life") && !t.eq_ignore_ascii_case("Long Life")
                })
                .cloned()
        })
    }
}

fn write_pct(path: &Path, value: u8) -> Result<(), ApiError> {
    write_str(path, &value.to_string())
}

fn write_str(path: &Path, value: &str) -> Result<(), ApiError> {
    match std::fs::write(path, format!("{value}\n")) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(ApiError::BadRequest(
                "needs the installed daemon running as root".into(),
            ))
        }
        Err(err) => Err(ApiError::BadRequest(format!(
            "could not write {}: {err}",
            path.display()
        ))),
    }
}

fn read_trim(path: &Path) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn read_u8(path: &Path) -> Option<u8> {
    read_trim(path).parse().ok()
}

fn read_u32(path: &Path) -> Option<u32> {
    read_trim(path).parse().ok()
}

fn read_u64(path: &Path) -> Option<u64> {
    read_trim(path).parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bracket_current() {
        let (types, cur) = parse_charge_types("Fast Standard [Long_Life]");
        assert_eq!(types, vec!["Fast", "Standard", "Long_Life"]);
        assert_eq!(cur.as_deref(), Some("Long_Life"));
    }
}
