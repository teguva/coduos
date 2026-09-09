use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{valid_id, Config, FileRoot, StorageMount};
use crate::error::ApiError;
use crate::stats;
use crate::util::{self, privileged, safe_label};

const MEDIA_ROOT: &str = "/media/coduos";

fn normalize_mount_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let s = s.trim_end_matches('/');
    if s.is_empty() {
        "/".into()
    } else {
        s.to_string()
    }
}

fn unescape_proc_mount(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let oct = &s[i + 1..i + 4];
            if let Ok(n) = u8::from_str_radix(oct, 8) {
                out.push(n as char);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

pub fn parse_proc_mounts(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mp = line.split_whitespace().nth(1)?;
            let mp = unescape_proc_mount(mp);
            Some(normalize_mount_path(Path::new(&mp)))
        })
        .collect()
}

pub fn is_path_mounted(path: &Path) -> bool {
    let want = normalize_mount_path(path);
    parse_proc_mounts(&std::fs::read_to_string("/proc/mounts").unwrap_or_default())
        .iter()
        .any(|m| m == &want)
}

/// Built-in locations stay visible. Extra volumes only appear while they are mounted.
pub fn file_root_available(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    if path == Path::new("/") || path == Path::new("/home") {
        return true;
    }
    is_path_mounted(path)
}

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
    pub health: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_label: Option<String>,
    pub auto_mount: bool,
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
    mountpoints: Option<Vec<Option<String>>>,
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

fn is_swap(fstype: &str, mount: &str) -> bool {
    fstype.eq_ignore_ascii_case("swap")
        || mount.eq_ignore_ascii_case("[SWAP]")
        || mount.to_ascii_uppercase().starts_with("[SWAP]")
}

fn mounts_of(dev: &LsblkDev) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(list) = &dev.mountpoints {
        for m in list {
            if let Some(s) = m {
                let s = s.trim();
                if !s.is_empty() && !out.iter().any(|x| x == s) {
                    out.push(s.to_string());
                }
            }
        }
    }
    let single = opt_str(&dev.mountpoint);
    if !single.is_empty() && !out.iter().any(|x| x == &single) {
        out.insert(0, single);
    }
    out
}

fn preferred_mount(mounts: &[String]) -> String {
    if mounts.iter().any(|m| m == "/") {
        return "/".into();
    }
    mounts
        .iter()
        .filter(|m| {
            !m.starts_with("/var") && !m.starts_with("/run") && *m != "/tmp" && *m != "/srv"
        })
        .min_by_key(|m| m.len())
        .cloned()
        .or_else(|| mounts.first().cloned())
        .unwrap_or_default()
}

fn is_system_mount(mount: &str, data_dir: &Path) -> bool {
    if mount.is_empty() {
        return false;
    }
    let data = data_dir.display().to_string();
    mount == "/"
        || mount == "/usr"
        || mount == "/etc"
        || mount == "/home"
        || mount.starts_with("/usr/")
        || mount.starts_with("/etc/")
        || mount.starts_with("/home/")
        || mount.starts_with("/boot")
        || mount.starts_with("/var")
        || mount.starts_with("/opt")
        || mount.starts_with("/tmp")
        || mount.starts_with("/run")
        || mount.starts_with("/sys")
        || mount.starts_with("/proc")
        || mount.starts_with("/nix")
        || data == mount
        || Path::new(&data).starts_with(mount)
}

fn tree_has_system(dev: &LsblkDev, data_dir: &Path) -> bool {
    let mounts = mounts_of(dev);
    let fstype = opt_str(&dev.fstype);
    let mount = preferred_mount(&mounts);
    if mounts.iter().any(|m| is_system_mount(m, data_dir)) || is_swap(&fstype, &mount) {
        return true;
    }
    if let Some(children) = &dev.children {
        return children.iter().any(|ch| tree_has_system(ch, data_dir));
    }
    false
}

fn valid_dev(device: &str) -> Result<&str, ApiError> {
    if !device.starts_with("/dev/")
        || device.contains("..")
        || device.contains('\0')
        || !device
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
    {
        return Err(ApiError::BadRequest("invalid device".into()));
    }
    Ok(device)
}

fn partition_node(disk: &str) -> String {
    let base = disk.rsplit('/').next().unwrap_or(disk);
    if base.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        format!("{disk}p1")
    } else {
        format!("{disk}1")
    }
}

fn to_partition(
    dev: &LsblkDev,
    removable: bool,
    data_dir: &Path,
    roots: &[FileRoot],
    persisted: &[StorageMount],
) -> Partition {
    let mounts = mounts_of(dev);
    let mount = preferred_mount(&mounts);
    let fstype = opt_str(&dev.fstype);
    let system = mounts.iter().any(|m| is_system_mount(m, data_dir))
        || is_swap(&fstype, &mount)
        || tree_has_system(dev, data_dir);
    let (used, total) = if mount.is_empty() || is_swap(&fstype, &mount) {
        (None, None)
    } else {
        stats::disk_usage_by_mount(&mount)
            .map(|(u, t)| (Some(u), Some(t)))
            .unwrap_or((None, None))
    };
    let path = dev_path(dev);
    let uuid = opt_str(&dev.uuid);
    let auto_mount = persisted
        .iter()
        .any(|m| !uuid.is_empty() && m.uuid == uuid && m.auto_mount);
    let files_root = roots.iter().find(|r| {
        let rp = r.path.display().to_string();
        rp == mount || (!mount.is_empty() && mount != "/" && r.path.starts_with(&mount))
    });
    Partition {
        name: opt_str(&dev.name),
        path: path.clone(),
        size: json_u64(&dev.size),
        fstype,
        label: opt_str(&dev.label),
        uuid,
        mountpoint: mount.clone(),
        removable,
        system,
        ro: json_bool(&dev.ro),
        used,
        total,
        health: None,
        in_files: files_root.is_some(),
        files_label: files_root.map(|r| r.label.clone()),
        auto_mount,
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
            "NAME,PATH,SIZE,TYPE,FSTYPE,LABEL,UUID,MOUNTPOINT,MOUNTPOINTS,TRAN,HOTPLUG,MODEL,VENDOR,RO,RM",
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
        let name = opt_str(&dev.name);
        if kind != "disk" && kind != "rom" {
            continue;
        }
        if name.starts_with("zram")
            || name.starts_with("loop")
            || name.starts_with("ram")
            || name.starts_with("dm-")
        {
            continue;
        }
        let tran = opt_str(&dev.tran);
        let rm = json_bool(&dev.rm) || json_bool(&dev.hotplug);
        let removable = is_removable(&dev, &tran, rm);
        let mut parts: Vec<Partition> = Vec::new();
        if let Some(children) = &dev.children {
            for ch in children {
                let ch_rm = is_removable(ch, &tran, removable);
                parts.push(to_partition(
                    ch,
                    ch_rm,
                    &cfg.data_dir,
                    &cfg.file_roots,
                    &cfg.storage_mounts,
                ));
            }
        } else if !opt_str(&dev.fstype).is_empty() || !opt_str(&dev.mountpoint).is_empty() {
            parts.push(to_partition(
                &dev,
                removable,
                &cfg.data_dir,
                &cfg.file_roots,
                &cfg.storage_mounts,
            ));
        }
        if (kind == "rom" || removable) && json_u64(&dev.size) == 0 && parts.is_empty() {
            continue;
        }
        let system = parts.iter().any(|p| p.system)
            || tree_has_system(&dev, &cfg.data_dir)
            || is_system_mount(&opt_str(&dev.mountpoint), &cfg.data_dir);
        let path = dev_path(&dev);
        let health = smart_health(&path);
        disks.push(Disk {
            name,
            path,
            size: json_u64(&dev.size),
            model: opt_str(&dev.model).trim().to_string(),
            vendor: opt_str(&dev.vendor).trim().to_string(),
            transport: tran,
            removable,
            system,
            health,
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
    if part.system || is_system_mount(&part.mountpoint, Path::new("/")) {
        return Err(ApiError::BadRequest(
            "this volume has system partitions; CoduOS will not change it".into(),
        ));
    }
    Ok(())
}

fn refuse_disk(disk: &Disk) -> Result<(), ApiError> {
    if disk.system {
        return Err(ApiError::BadRequest(
            "this drive has system partitions; CoduOS will not format or remount it".into(),
        ));
    }
    Ok(())
}

fn mount_opts(fstype: &str, read_only: bool) -> String {
    let mut base = String::from("nosuid,nodev,noexec");
    if read_only {
        base.push_str(",ro");
    }
    match fstype.to_ascii_lowercase().as_str() {
        "vfat" | "fat" | "fat32" | "exfat" | "ntfs" | "ntfs3" | "msdos" => {
            let uid = unsafe { libc::geteuid() };
            let gid = unsafe { libc::getegid() };
            format!("{base},uid={uid},gid={gid}")
        }
        _ => base,
    }
}

#[derive(Debug, Clone)]
pub struct MountOpts {
    pub folder: Option<String>,
    pub files_label: Option<String>,
    pub add_to_files: bool,
    pub auto_mount: bool,
    pub read_only: bool,
}

impl Default for MountOpts {
    fn default() -> Self {
        Self {
            folder: None,
            files_label: None,
            add_to_files: false,
            auto_mount: true,
            read_only: false,
        }
    }
}

pub fn mount_device(cfg: &mut Config, config_path: &Path, device: &str) -> Result<Partition, ApiError> {
    mount_device_with(cfg, config_path, device, &MountOpts::default())
}

pub fn mount_device_with(
    cfg: &mut Config,
    config_path: &Path,
    device: &str,
    opts: &MountOpts,
) -> Result<Partition, ApiError> {
    util::require_privileged()?;
    let inv = inventory(cfg)?;
    let (disk, part) = find_part(&inv, device).ok_or(ApiError::NotFound)?;
    refuse_disk(disk)?;
    refuse_system(part)?;
    if !part.mountpoint.is_empty() {
        return Ok(part.clone());
    }
    if part.uuid.is_empty() {
        return Err(ApiError::BadRequest("device has no UUID; cannot persist a mount".into()));
    }
    let fallback = if part.label.is_empty() {
        safe_label(&part.uuid)
    } else {
        safe_label(&part.label)
    };
    let folder = match opts.folder.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => safe_label(raw),
        None => cfg
            .storage_mounts
            .iter()
            .find(|m| m.uuid == part.uuid)
            .and_then(|m| {
                m.mountpoint
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback),
    };
    let dest = PathBuf::from(MEDIA_ROOT).join(&folder);
    if is_path_mounted(&dest) {
        return Err(ApiError::BadRequest(format!(
            "{} is already in use as a mount point",
            dest.display()
        )));
    }
    std::fs::create_dir_all(&dest)
        .map_err(|e| ApiError::BadRequest(format!("could not create mount point: {e}")))?;
    let opts_str = mount_opts(&part.fstype, opts.read_only);
    let src = format!("UUID={}", part.uuid);
    util::run_ok("mount", &["-o", &opts_str, &src, &dest.display().to_string()])?;
    let files_label = opts
        .files_label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(if part.label.is_empty() {
            folder.as_str()
        } else {
            part.label.as_str()
        })
        .to_string();
    upsert_storage_mount(
        cfg,
        StorageMount {
            uuid: part.uuid.clone(),
            mountpoint: dest.clone(),
            device: part.path.clone(),
            label: part.label.clone(),
            auto_mount: opts.auto_mount,
            read_only: opts.read_only,
        },
    );
    cfg.save(config_path)
        .map_err(|e| ApiError::BadRequest(format!("could not persist mount: {e}")))?;
    if opts.add_to_files {
        let _ = add_path_to_files(cfg, config_path, &dest.display().to_string(), &files_label);
    }
    let mut out = part.clone();
    out.mountpoint = dest.display().to_string();
    out.auto_mount = opts.auto_mount;
    out.in_files = opts.add_to_files;
    out.files_label = if opts.add_to_files {
        Some(files_label)
    } else {
        None
    };
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
    let (disk, part) = find_part(&inv, target).ok_or(ApiError::NotFound)?;
    refuse_disk(disk)?;
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
    let drop_ids: Vec<String> = cfg
        .file_roots
        .iter()
        .filter(|r| r.path.display().to_string() == mp)
        .map(|r| r.id.clone())
        .collect();
    cfg.file_roots.retain(|r| r.path.display().to_string() != mp);
    cfg.file_favorites.retain(|f| !drop_ids.iter().any(|id| id == &f.root));
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
    let (disk, part) = find_part(&inv, device).ok_or(ApiError::NotFound)?;
    refuse_disk(disk)?;
    if part.mountpoint.is_empty() {
        return Err(ApiError::BadRequest("mount the volume first".into()));
    }
    if is_swap(&part.fstype, &part.mountpoint) {
        return Err(ApiError::BadRequest("swap cannot be added as a files location".into()));
    }
    if part.mountpoint == "/boot" || part.mountpoint == "/boot/efi" {
        return Err(ApiError::BadRequest("boot partitions cannot be added as a files location".into()));
    }
    if part.system && (part.mountpoint == "/" || part.mountpoint == "/usr") {
        return Err(ApiError::BadRequest("the system disk is already available as a location if configured".into()));
    }
    let label = if part.label.is_empty() {
        part.name.clone()
    } else {
        part.label.clone()
    };
    add_path_to_files(cfg, config_path, &part.mountpoint, &label)
}

fn add_path_to_files(
    cfg: &mut Config,
    config_path: &Path,
    mountpoint: &str,
    files_label: &str,
) -> Result<FileRoot, ApiError> {
    if let Some(existing) = cfg
        .file_roots
        .iter_mut()
        .find(|r| r.path.display().to_string() == mountpoint)
    {
        let label = files_label.trim();
        if !label.is_empty() && existing.label != label {
            existing.label = label.to_string();
            let root = existing.clone();
            cfg.save(config_path)
                .map_err(|e| ApiError::BadRequest(format!("could not save files location: {e}")))?;
            return Ok(root);
        }
        return Ok(existing.clone());
    }
    let mut id = safe_label(files_label);
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
        label: if files_label.trim().is_empty() {
            id
        } else {
            files_label.trim().to_string()
        },
        path: PathBuf::from(mountpoint),
    };
    cfg.file_roots.push(root.clone());
    cfg.save(config_path)
        .map_err(|e| ApiError::BadRequest(format!("could not save files location: {e}")))?;
    Ok(root)
}

fn find_disk<'a>(inv: &'a Inventory, device: &str) -> Option<&'a Disk> {
    inv.disks.iter().find(|d| d.path == device || d.name == device)
}

fn wait_for_dev(path: &str) -> Result<(), ApiError> {
    for _ in 0..40 {
        if Path::new(path).exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(ApiError::BadRequest(format!(
        "device {path} did not appear after partitioning"
    )))
}

fn mkfs(fstype: &str, label: &str, dev: &str) -> Result<(), ApiError> {
    let label = if label.is_empty() { "data" } else { label };
    match fstype {
        "ext4" => {
            if !util::which("mkfs.ext4") {
                return Err(ApiError::BadRequest("mkfs.ext4 is not installed".into()));
            }
            util::run_ok("mkfs.ext4", &["-F", "-q", "-L", label, "-m", "0", dev])?;
        }
        "xfs" => {
            if !util::which("mkfs.xfs") {
                return Err(ApiError::BadRequest("mkfs.xfs is not installed".into()));
            }
            util::run_ok("mkfs.xfs", &["-f", "-L", label, dev])?;
        }
        "btrfs" => {
            if !util::which("mkfs.btrfs") {
                return Err(ApiError::BadRequest("mkfs.btrfs is not installed".into()));
            }
            util::run_ok("mkfs.btrfs", &["-f", "-L", label, dev])?;
        }
        "exfat" => {
            if !util::which("mkfs.exfat") {
                return Err(ApiError::BadRequest("mkfs.exfat is not installed".into()));
            }
            util::run_ok("mkfs.exfat", &["-n", label, dev])?;
        }
        _ => {
            return Err(ApiError::BadRequest(
                "filesystem must be ext4, xfs, btrfs, or exfat".into(),
            ))
        }
    }
    Ok(())
}

fn unmount_if_needed(mp: &str) -> Result<(), ApiError> {
    if mp.is_empty() {
        return Ok(());
    }
    if is_system_mount(mp, Path::new("/nonexistent")) {
        return Err(ApiError::BadRequest(
            "refusing to unmount a system path".into(),
        ));
    }
    let out = util::run("umount", &[mp])?;
    if out.status.success() {
        return Ok(());
    }
    let _ = util::run("umount", &["-l", mp]);
    Ok(())
}

/// Wipe a non-system disk (GPT + one partition) or reformat a partition, then mount it.
pub fn format_and_mount(
    cfg: &mut Config,
    config_path: &Path,
    device: &str,
    fstype: &str,
    label: &str,
) -> Result<Partition, ApiError> {
    util::require_privileged()?;
    let device = valid_dev(device)?.to_string();
    let fstype = fstype.trim().to_ascii_lowercase();
    if !matches!(fstype.as_str(), "ext4" | "xfs" | "btrfs" | "exfat") {
        return Err(ApiError::BadRequest(
            "filesystem must be ext4, xfs, btrfs, or exfat".into(),
        ));
    }
    let label = {
        let s = safe_label(label);
        let max = if fstype == "exfat" { 15 } else { 16 };
        s.chars().take(max).collect::<String>()
    };
    let inv = inventory(cfg)?;
    if let Some(disk) = find_disk(&inv, &device) {
        refuse_disk(disk)?;
        for p in &disk.partitions {
            refuse_system(p)?;
            unmount_if_needed(&p.mountpoint)?;
        }
        if !util::which("parted") {
            return Err(ApiError::BadRequest("parted is required to format a disk".into()));
        }
        util::run_ok(
            "parted",
            &["-s", &disk.path, "--", "mklabel", "gpt", "mkpart", "primary", "1MiB", "100%"],
        )?;
        let _ = util::run("partprobe", &[&disk.path]);
        let _ = util::run("udevadm", &["settle", "-t", "8"]);
        let part_path = partition_node(&disk.path);
        wait_for_dev(&part_path)?;
        mkfs(&fstype, &label, &part_path)?;
        let _ = util::run("udevadm", &["settle", "-t", "8"]);
        return mount_device(cfg, config_path, &part_path);
    }
    let (disk, part) = find_part(&inv, &device).ok_or(ApiError::NotFound)?;
    refuse_disk(disk)?;
    refuse_system(part)?;
    unmount_if_needed(&part.mountpoint)?;
    mkfs(&fstype, &label, &part.path)?;
    let _ = util::run("udevadm", &["settle", "-t", "8"]);
    mount_device(cfg, config_path, &part.path)
}

fn upsert_storage_mount(cfg: &mut Config, mount: StorageMount) {
    if let Some(existing) = cfg.storage_mounts.iter_mut().find(|m| m.uuid == mount.uuid) {
        *existing = mount;
    } else {
        cfg.storage_mounts.push(mount);
    }
}

fn persist_dest(part: &Partition, existing: Option<&StorageMount>) -> PathBuf {
    if !part.mountpoint.is_empty() {
        return PathBuf::from(&part.mountpoint);
    }
    if let Some(m) = existing {
        return m.mountpoint.clone();
    }
    let slug = if part.label.is_empty() {
        safe_label(&part.uuid)
    } else {
        safe_label(&part.label)
    };
    PathBuf::from(MEDIA_ROOT).join(slug)
}

pub fn set_auto_mount(
    cfg: &mut Config,
    config_path: &Path,
    device: &str,
    enabled: bool,
) -> Result<Partition, ApiError> {
    util::require_privileged()?;
    let device = valid_dev(device)?.to_string();
    let inv = inventory(cfg)?;
    let (disk, part) = find_part(&inv, &device).ok_or(ApiError::NotFound)?;
    refuse_disk(disk)?;
    refuse_system(part)?;
    if part.uuid.is_empty() {
        return Err(ApiError::BadRequest(
            "device has no UUID; cannot persist a mount".into(),
        ));
    }
    let existing = cfg
        .storage_mounts
        .iter()
        .find(|m| m.uuid == part.uuid)
        .cloned();
    if !enabled && existing.is_none() {
        let mut out = part.clone();
        out.auto_mount = false;
        return Ok(out);
    }
    let dest = persist_dest(part, existing.as_ref());
    if enabled {
        std::fs::create_dir_all(&dest)
            .map_err(|e| ApiError::BadRequest(format!("could not create mount point: {e}")))?;
    }
    let read_only = existing.as_ref().map(|m| m.read_only).unwrap_or(false);
    upsert_storage_mount(
        cfg,
        StorageMount {
            uuid: part.uuid.clone(),
            mountpoint: dest,
            device: part.path.clone(),
            label: part.label.clone(),
            auto_mount: enabled,
            read_only,
        },
    );
    cfg.save(config_path)
        .map_err(|e| ApiError::BadRequest(format!("could not persist mount: {e}")))?;
    let inv = inventory(cfg)?;
    find_part(&inv, &device)
        .map(|(_, p)| p.clone())
        .ok_or(ApiError::NotFound)
}

pub fn remount_persisted(cfg: &Config) {
    if !privileged() {
        return;
    }
    std::fs::create_dir_all(MEDIA_ROOT).ok();
    for m in &cfg.storage_mounts {
        if !m.auto_mount {
            continue;
        }
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
        let opts = if m.read_only {
            "nosuid,nodev,noexec,ro"
        } else {
            "nosuid,nodev,noexec"
        };
        match util::run("mount", &["-o", opts, &src, &dest]) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_node_nvme_and_sata() {
        assert_eq!(partition_node("/dev/sdb"), "/dev/sdb1");
        assert_eq!(partition_node("/dev/nvme0n1"), "/dev/nvme0n1p1");
        assert_eq!(partition_node("/dev/mmcblk0"), "/dev/mmcblk0p1");
    }

    #[test]
    fn valid_dev_rejects_junk() {
        assert!(valid_dev("/dev/sdb").is_ok());
        assert!(valid_dev("/dev/nvme0n1p2").is_ok());
        assert!(valid_dev("../dev/sdb").is_err());
        assert!(valid_dev("/dev/sda;reboot").is_err());
        assert!(valid_dev("/tmp/x").is_err());
    }

    #[test]
    fn system_mounts_are_protected() {
        let data = Path::new("/var/lib/coduos");
        assert!(is_system_mount("/", data));
        assert!(is_system_mount("/boot/efi", data));
        assert!(is_system_mount("/home", data));
        assert!(!is_system_mount("/media/coduos/data", data));
        assert!(!is_system_mount("/mnt/disk", data));
    }

    #[test]
    fn persist_dest_prefers_live_mount() {
        let part = Partition {
            name: "sdb1".into(),
            path: "/dev/sdb1".into(),
            size: 1,
            fstype: "ext4".into(),
            label: "backup".into(),
            uuid: "abc".into(),
            mountpoint: "/mnt/backup".into(),
            removable: false,
            system: false,
            ro: false,
            used: None,
            total: None,
            health: None,
            in_files: false,
            files_label: None,
            auto_mount: false,
        };
        assert_eq!(persist_dest(&part, None), PathBuf::from("/mnt/backup"));
        let unmounted = Partition {
            mountpoint: String::new(),
            ..part
        };
        assert_eq!(
            persist_dest(&unmounted, None),
            PathBuf::from("/media/coduos/backup")
        );
    }

    #[test]
    fn storage_mount_auto_defaults_on() {
        let m: StorageMount = toml::from_str(
            "uuid = \"abc\"\nmountpoint = \"/media/coduos/data\"\n",
        )
        .unwrap();
        assert!(m.auto_mount);
        let off: StorageMount = toml::from_str(
            "uuid = \"abc\"\nmountpoint = \"/media/coduos/data\"\nauto_mount = false\n",
        )
        .unwrap();
        assert!(!off.auto_mount);
    }

    #[test]
    fn mount_opts_fat_and_readonly() {
        let ext = mount_opts("ext4", false);
        assert!(ext.contains("nosuid"));
        assert!(!ext.split(',').any(|p| p == "ro"));
        assert!(mount_opts("ext4", true).split(',').any(|p| p == "ro"));
        assert!(mount_opts("exfat", false).contains("uid="));
    }

    #[test]
    fn parse_proc_mounts_unescapes_and_skips_unmounted() {
        let text = "\
/dev/sda1 / ext4 rw 0 0
/dev/sdb1 /media/coduos/backup ext4 rw 0 0
/dev/sdc1 /mnt/data\\040disk xfs rw 0 0
";
        let mounts = parse_proc_mounts(text);
        assert!(mounts.iter().any(|m| m == "/"));
        assert!(mounts.iter().any(|m| m == "/media/coduos/backup"));
        assert!(mounts.iter().any(|m| m == "/mnt/data disk"));
        assert!(!mounts.iter().any(|m| m == "/media/coduos/old"));
    }

    #[test]
    fn extra_volume_roots_need_a_live_mount() {
        assert!(file_root_available(Path::new("/")));
        assert!(!file_root_available(Path::new("/media/coduos/does-not-exist")));
    }
}
