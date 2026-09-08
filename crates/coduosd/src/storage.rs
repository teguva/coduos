use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{valid_id, Config, FileRoot, StorageMount};
use crate::error::ApiError;
use crate::stats;
use crate::util::{self, privileged, safe_label};

const MEDIA_ROOT: &str = "/media/coduos";

#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    pub privileged: bool,
    pub disks: Vec<Disk>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Disk {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub model: String,
    pub vendor: String,
    pub transport: String,
    pub removable: bool,
    pub system: bool,
    pub partitions: Vec<Partition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Partition {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub fstype: String,
    pub label: String,
    pub uuid: String,
    pub mountpoint: String,
    pub removable: bool,
    pub system: bool,
    pub ro: bool,
    pub used: Option<u64>,
    pub total: Option<u64>,
    pub health: Option<String>,
    pub in_files: bool,
}

#[derive(Debug, Deserialize)]
struct Lsblk {
    blockdevices: Vec<LsblkDev>,
}

#[derive(Debug, Deserialize)]
struct LsblkDev {
    name: Option<String>,
    path: Option<String>,
    size: Option<serde_json::Value>,
    #[serde(rename = "type")]
    kind: Option<String>,
    fstype: Option<String>,
    label: Option<String>,
    uuid: Option<String>,
    mountpoint: Option<String>,
    tran: Option<String>,
    hotplug: Option<serde_json::Value>,
    model: Option<String>,
    vendor: Option<String>,
    ro: Option<serde_json::Value>,
    rm: Option<serde_json::Value>,
    children: Option<Vec<LsblkDev>>,
}

fn json_u64(v: &Option<serde_json::Value>) -> u64 {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
        Some(serde_json::Value::Bool(true)) => 1,
        _ => 0,
    }
}

fn json_bool(v: &Option<serde_json::Value>) -> bool {
    match v {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) != 0,
        Some(serde_json::Value::String(s)) => matches!(s.as_str(), "1" | "true" | "yes"),
        _ => false,
    }
}

fn opt_str(v: &Option<String>) -> String {
    v.as_deref().unwrap_or("").trim().to_string()
}

fn dev_path(dev: &LsblkDev) -> String {
    if let Some(p) = &dev.path {
        if !p.is_empty() {
            return p.clone();
        }
    }
    let name = opt_str(&dev.name);
    if name.is_empty() {
        String::new()
    } else if name.starts_with("/dev/") {
        name
    } else {
        format!("/dev/{name}")
    }
}

fn is_removable(dev: &LsblkDev, parent_tran: &str, parent_rm: bool) -> bool {
    let tran = opt_str(&dev.tran);
    let tran = if tran.is_empty() {
        parent_tran.to_string()
    } else {
        tran
    };
    let rm = json_bool(&dev.rm) || parent_rm;
    rm || matches!(tran.as_str(), "usb" | "mmc" | "ieee1394")
}

fn is_system_mount(mount: &str, data_dir: &Path) -> bool {
    if mount.is_empty() {
        return false;
    }
    let data = data_dir.display().to_string();
    mount == "/"
        || mount == "/boot"
        || mount == "/usr"
        || mount == "/boot/efi"
        || data == mount
        || Path::new(&data).starts_with(mount)
}

fn to_partition(dev: &LsblkDev, removable: bool, data_dir: &Path, roots: &[FileRoot]) -> Partition {
    let mount = opt_str(&dev.mountpoint);
    let system = is_system_mount(&mount, data_dir);
    let (used, total) = if mount.is_empty() {
        (None, None)
    } else {
        stats::disk_usage_by_mount(&mount)
            .map(|(u, t)| (Some(u), Some(t)))
            .unwrap_or((None, None))
    };
    let path = dev_path(dev);
    Partition {
        name: opt_str(&dev.name),
        path: path.clone(),
        size: json_u64(&dev.size),
        fstype: opt_str(&dev.fstype),
        label: opt_str(&dev.label),
        uuid: opt_str(&dev.uuid),
        mountpoint: mount.clone(),
        removable,
        system,
        ro: json_bool(&dev.ro),
        used,
        total,
        health: None,
        in_files: roots.iter().any(|r| {
            !mount.is_empty() && (r.path.display().to_string() == mount || r.path.starts_with(&mount))
        }),
    }
}

fn smart_health(device: &str) -> Option<String> {
    if !util::which("smartctl") {
        return None;
    }
    let out = std::process::Command::new("smartctl")
        .args(["-H", "-j", device])
        .output()
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.get("smart_status")
        .and_then(|s| s.get("passed"))
        .and_then(|p| p.as_bool())
        .map(|ok| if ok { "passed".into() } else { "failed".into() })
        .or_else(|| {
            v.get("smart_status")
                .and_then(|s| s.get("string"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
}

pub fn inventory(cfg: &Config) -> Result<Inventory, ApiError> {
    let out = util::run(
        "lsblk",
        &[
            "-J",
            "-b",
            "-o",
            "NAME,PATH,SIZE,TYPE,FSTYPE,LABEL,UUID,MOUNTPOINT,TRAN,HOTPLUG,MODEL,VENDOR,RO,RM",
        ],
    )?;
    if !out.status.success() {
        return Err(ApiError::BadRequest(format!(
            "lsblk: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let parsed: Lsblk = serde_json::from_slice(&out.stdout)
        .map_err(|e| ApiError::BadRequest(format!("lsblk json: {e}")))?;
    let mut disks = Vec::new();
    for dev in parsed.blockdevices {
        let kind = opt_str(&dev.kind);
        if kind != "disk" && kind != "rom" {
            continue;
        }
        let tran = opt_str(&dev.tran);
        let rm = json_bool(&dev.rm) || json_bool(&dev.hotplug);
        let removable = is_removable(&dev, &tran, rm);
        let mut parts: Vec<Partition> = Vec::new();
        if let Some(children) = &dev.children {
            for ch in children {
                let ch_rm = is_removable(ch, &tran, removable);
                parts.push(to_partition(ch, ch_rm, &cfg.data_dir, &cfg.file_roots));
            }
        } else if !opt_str(&dev.fstype).is_empty() || !opt_str(&dev.mountpoint).is_empty() {
            parts.push(to_partition(&dev, removable, &cfg.data_dir, &cfg.file_roots));
        }
        let system = parts.iter().any(|p| p.system)
            || is_system_mount(&opt_str(&dev.mountpoint), &cfg.data_dir);
        let path = dev_path(&dev);
        let health = smart_health(&path);
        for p in &mut parts {
            if p.health.is_none() {
                p.health = health.clone();
            }
        }
        disks.push(Disk {
            name: opt_str(&dev.name),
            path,
            size: json_u64(&dev.size),
            model: opt_str(&dev.model),
            vendor: opt_str(&dev.vendor),
            transport: tran,
            removable,
            system,
            partitions: parts,
        });
    }
    Ok(Inventory {
        privileged: privileged(),
        disks,
    })
}

fn find_part<'a>(inv: &'a Inventory, device: &str) -> Option<(&'a Disk, &'a Partition)> {
    for d in &inv.disks {
        if d.path == device {
            if let Some(p) = d.partitions.first() {
                return Some((d, p));
            }
        }
        for p in &d.partitions {
            if p.path == device || p.uuid == device || p.mountpoint == device {
                return Some((d, p));
            }
        }
    }
    None
}

fn refuse_system(part: &Partition) -> Result<(), ApiError> {
    if part.system {
        return Err(ApiError::Forbidden);
    }
    if part.mountpoint == "/" || part.mountpoint == "/boot" {
        return Err(ApiError::Forbidden);
    }
    if part.path.contains("/dm-") || part.path.contains("mapper") {
        if part.system {
            return Err(ApiError::Forbidden);
        }
    }
    Ok(())
}

fn mount_opts(fstype: &str) -> String {
    let base = "nosuid,nodev,noexec";
    match fstype.to_ascii_lowercase().as_str() {
        "vfat" | "fat" | "fat32" | "exfat" | "ntfs" | "ntfs3" | "msdos" => {
            let uid = unsafe { libc::geteuid() };
            let gid = unsafe { libc::getegid() };
            format!("{base},uid={uid},gid={gid}")
        }
        _ => base.into(),
    }
}

pub fn mount_device(cfg: &mut Config, config_path: &Path, device: &str) -> Result<Partition, ApiError> {
    util::require_privileged()?;
    let inv = inventory(cfg)?;
    let (_disk, part) = find_part(&inv, device).ok_or(ApiError::NotFound)?;
    if !part.removable {
        return Err(ApiError::BadRequest("only removable media can be mounted here".into()));
    }
    refuse_system(part)?;
    if !part.mountpoint.is_empty() {
        return Ok(part.clone());
    }
    if part.uuid.is_empty() {
        return Err(ApiError::BadRequest("device has no UUID; cannot persist a mount".into()));
    }
    let label = if part.label.is_empty() {
        safe_label(&part.uuid)
    } else {
        safe_label(&part.label)
    };
    let dest = PathBuf::from(MEDIA_ROOT).join(&label);
    std::fs::create_dir_all(&dest)?;
    let opts = mount_opts(&part.fstype);
    let src = format!("UUID={}", part.uuid);
    util::run_ok("mount", &["-o", &opts, &src, &dest.display().to_string()])?;
    cfg.storage_mounts.retain(|m| m.uuid != part.uuid);
    cfg.storage_mounts.push(StorageMount {
        uuid: part.uuid.clone(),
        mountpoint: dest.clone(),
        device: part.path.clone(),
        label: part.label.clone(),
    });
    cfg.save(config_path)
        .map_err(|e| ApiError::BadRequest(format!("could not persist mount: {e}")))?;
    let mut out = part.clone();
    out.mountpoint = dest.display().to_string();
    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct UnmountResult {
    pub ok: bool,
    pub busy: bool,
    pub pids: Vec<u32>,
    pub message: String,
}

pub fn unmount_device(
    cfg: &mut Config,
    config_path: &Path,
    target: &str,
    force: bool,
) -> Result<UnmountResult, ApiError> {
    util::require_privileged()?;
    let inv = inventory(cfg)?;
    let (_disk, part) = find_part(&inv, target).ok_or(ApiError::NotFound)?;
    if !part.removable {
        return Err(ApiError::BadRequest("only removable media can be ejected here".into()));
    }
    refuse_system(part)?;
    let mp = if part.mountpoint.is_empty() {
        return Ok(UnmountResult {
            ok: true,
            busy: false,
            pids: vec![],
            message: "already unmounted".into(),
        });
    } else {
        part.mountpoint.clone()
    };
    let uuid = part.uuid.clone();
    if force {
        let _ = util::run("umount", &["-l", &mp]);
    } else {
        let out = util::run("umount", &[&mp])?;
        if !out.status.success() {
            let pids = busy_pids(&mp);
            return Ok(UnmountResult {
                ok: false,
                busy: true,
                pids,
                message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
    }
    cfg.storage_mounts.retain(|m| m.uuid != uuid && m.mountpoint.display().to_string() != mp);
    let _ = cfg.save(config_path);
    Ok(UnmountResult {
        ok: true,
        busy: false,
        pids: vec![],
        message: "ejected".into(),
    })
}

fn busy_pids(mount: &str) -> Vec<u32> {
    let out = std::process::Command::new("fuser")
        .args(["-m", mount])
        .output();
    let Ok(out) = out else {
        return vec![];
    };
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let mut pids = Vec::new();
    for tok in text.split_whitespace() {
        let tok = tok.trim_matches(|c: char| !c.is_ascii_digit());
        if let Ok(pid) = tok.parse::<u32>() {
            if pid > 1 {
                pids.push(pid);
            }
        }
    }
    pids.sort();
    pids.dedup();
    pids
}

pub fn add_to_files(cfg: &mut Config, config_path: &Path, device: &str) -> Result<FileRoot, ApiError> {
    let inv = inventory(cfg)?;
    let (_disk, part) = find_part(&inv, device).ok_or(ApiError::NotFound)?;
    if part.mountpoint.is_empty() {
        return Err(ApiError::BadRequest("mount the volume first".into()));
    }
    if part.system && part.mountpoint == "/" {
        return Err(ApiError::BadRequest("the system disk is already available as a location if configured".into()));
    }
    if let Some(existing) = cfg
        .file_roots
        .iter()
        .find(|r| r.path.display().to_string() == part.mountpoint)
    {
        return Ok(existing.clone());
    }
    let mut id = if part.label.is_empty() {
        safe_label(&part.uuid)
    } else {
        safe_label(&part.label)
    };
    if !valid_id(&id) {
        id = format!("disk-{id}");
        id = crate::config::slugify(&id);
    }
    let mut candidate = id.clone();
    let mut n = 2;
    while cfg.file_roots.iter().any(|r| r.id == candidate) {
        candidate = format!("{id}-{n}");
        n += 1;
    }
    let root = FileRoot {
        id: candidate,
        label: if part.label.is_empty() {
            part.name.clone()
        } else {
            part.label.clone()
        },
        path: PathBuf::from(&part.mountpoint),
    };
    cfg.file_roots.push(root.clone());
    cfg.save(config_path)
        .map_err(|e| ApiError::BadRequest(format!("could not save files location: {e}")))?;
    Ok(root)
}

pub fn remount_persisted(cfg: &Config) {
    if !privileged() {
        return;
    }
    std::fs::create_dir_all(MEDIA_ROOT).ok();
    for m in &cfg.storage_mounts {
        if m.mountpoint.exists() {
            let mounted = std::fs::read_to_string("/proc/mounts")
                .unwrap_or_default()
                .lines()
                .any(|l| l.split_whitespace().nth(1) == Some(&m.mountpoint.display().to_string()));
            if mounted {
                continue;
            }
        }
        if let Err(err) = std::fs::create_dir_all(&m.mountpoint) {
            tracing::warn!("storage remount mkdir {}: {err}", m.mountpoint.display());
            continue;
        }
        let dest = m.mountpoint.display().to_string();
        let src = format!("UUID={}", m.uuid);
        match util::run("mount", &["-o", "nosuid,nodev,noexec", &src, &dest]) {
            Ok(out) if out.status.success() => {
                tracing::info!("remounted {} at {dest}", m.uuid);
            }
            Ok(out) => tracing::warn!(
                "remount {} failed: {}",
                m.uuid,
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(err) => tracing::warn!("remount {}: {err}", m.uuid),
        }
    }
}
