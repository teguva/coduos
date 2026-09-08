use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{
    Components, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System, ThreadKind,
    UpdateKind,
};


#[derive(Debug, Clone, Serialize)]
pub struct SystemSummary {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub uptime_secs: u64,
    pub cpu_percent: f32,
    pub cpu_cores: usize,
    pub cpu_temp_c: Option<f32>,
    pub cpu_power_w: Option<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetInfo>,
    pub sensors: Vec<SensorInfo>,
    pub gpus: Vec<GpuInfo>,
    pub processes: Vec<ProcInfo>,
    pub docker: DockerInfo,
    pub batteries: Vec<crate::battery::BatteryCell>,
    pub version: String,
    pub privileged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bps: u64,
    pub tx_bps: u64,
    pub ipv4: Option<String>,
    pub operstate: String,
    pub speed_mbps: Option<u32>,
    pub virtual_iface: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SensorInfo {
    pub label: String,
    pub temp_c: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub util_percent: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub mem_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerInfo {
    pub available: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

impl Default for SystemSummary {
    fn default() -> Self {
        Self {
            hostname: hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "coduos".into()),
            os: System::long_os_version().unwrap_or_else(|| "Linux".into()),
            kernel: System::kernel_version().unwrap_or_default(),
            uptime_secs: System::uptime(),
            cpu_percent: 0.0,
            cpu_cores: 0,
            cpu_temp_c: None,
            cpu_power_w: None,
            mem_used: 0,
            mem_total: 0,
            swap_used: 0,
            swap_total: 0,
            disks: vec![],
            networks: vec![],
            sensors: vec![],
            gpus: vec![],
            processes: vec![],
            docker: DockerInfo {
                available: false,
                version: None,
                error: None,
            },
            batteries: vec![],
            version: env!("CARGO_PKG_VERSION").into(),
            privileged: crate::config::running_as_root(),
        }
    }
}

struct Collector {
    sys: System,
    nets: Networks,
    prev_net: HashMap<String, (u64, u64)>,
    prev_at: Instant,
    prev_rapl_uj: Option<u64>,
    prev_rapl_at: Option<Instant>,
}

pub fn spawn_collector(tx: tokio::sync::watch::Sender<SystemSummary>) {
    tokio::task::spawn_blocking(move || {
        let mut col = Collector {
            sys: System::new(),
            nets: Networks::new_with_refreshed_list(),
            prev_net: HashMap::new(),
            prev_at: Instant::now(),
            prev_rapl_uj: None,
            prev_rapl_at: None,
        };
        loop {
            let summary = col.collect();
            if tx.send(summary).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

impl Collector {
    fn collect(&mut self) -> SystemSummary {
        self.sys.refresh_memory();
        self.sys.refresh_cpu_all();
        let kind = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_exe(UpdateKind::OnlyIfNotSet);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        std::thread::sleep(Duration::from_millis(200));
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind);

        let disks = Disks::new_with_refreshed_list();
        self.nets.refresh(true);

        let now = Instant::now();
        let dt = now.duration_since(self.prev_at).as_secs_f64().max(0.2);
        let addrs = ipv4_map();

        let mut net_infos = Vec::new();
        for (name, data) in self.nets.iter() {
            let n = name.as_str();
            if n == "lo" {
                continue;
            }
            let rx = data.total_received();
            let tx = data.total_transmitted();
            let (rx_bps, tx_bps) = if let Some((prx, ptx)) = self.prev_net.get(n) {
                (
                    ((rx.saturating_sub(*prx) as f64) / dt) as u64,
                    ((tx.saturating_sub(*ptx) as f64) / dt) as u64,
                )
            } else {
                (0, 0)
            };
            self.prev_net.insert(n.to_string(), (rx, tx));
            net_infos.push(NetInfo {
                name: n.to_string(),
                rx_bytes: rx,
                tx_bytes: tx,
                rx_bps,
                tx_bps,
                ipv4: addrs.get(n).cloned(),
                operstate: sysfs_trim(&format!("/sys/class/net/{n}/operstate")),
                speed_mbps: sysfs_trim(&format!("/sys/class/net/{n}/speed"))
                    .parse::<i64>()
                    .ok()
                    .filter(|v| *v > 0)
                    .map(|v| v as u32),
                virtual_iface: is_virtual(n),
            });
        }
        self.prev_at = now;

        let disk_infos = disks
            .iter()
            .filter(|d| interesting_mount(d.mount_point()))
            .map(|d| {
                let total = d.total_space();
                let avail = d.available_space();
                DiskInfo {
                    name: d.name().to_string_lossy().into_owned(),
                    mount: d.mount_point().display().to_string(),
                    fs: d.file_system().to_string_lossy().into_owned(),
                    total,
                    used: total.saturating_sub(avail),
                }
            })
            .collect();

        let mut components = Components::new_with_refreshed_list();
        components.refresh(true);
        let mut sensors = Vec::new();
        for c in components.iter() {
            if let Some(t) = c.temperature() {
                if t.is_finite() && t > 0.0 && t < 150.0 {
                    sensors.push(SensorInfo {
                        label: c.label().to_string(),
                        temp_c: t,
                    });
                }
            }
        }
        let cpu_temp_c = pick_cpu_temp(&sensors);
        let cpu_power_w = self.cpu_power_w();
        let gpus = collect_gpus(&sensors);

        let mut processes: Vec<ProcInfo> = self
            .sys
            .processes()
            .iter()
            .filter(|(pid, p)| {
                if p.thread_kind() == Some(ThreadKind::Kernel) || pid.as_u32() <= 1 {
                    return false;
                }
                let name = p.name().to_string_lossy();
                name != "coduosd"
                    && !name.starts_with("kworker")
                    && !name.starts_with("ksoftirqd")
            })
            .map(|(pid, p)| ProcInfo {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                cpu_percent: p.cpu_usage(),
                mem_bytes: p.memory(),
            })
            .collect();
        processes.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.mem_bytes.cmp(&a.mem_bytes))
        });
        processes.truncate(8);

        SystemSummary {
            hostname: System::host_name().unwrap_or_else(|| "coduos".into()),
            os: System::long_os_version().unwrap_or_else(|| "Linux".into()),
            kernel: System::kernel_version().unwrap_or_default(),
            uptime_secs: System::uptime(),
            cpu_percent: self.sys.global_cpu_usage(),
            cpu_cores: self.sys.cpus().len(),
            cpu_temp_c,
            cpu_power_w,
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            disks: disk_infos,
            networks: net_infos,
            sensors,
            gpus,
            processes,
            docker: docker_info(),
            batteries: crate::battery::summary_batteries(),
            version: env!("CARGO_PKG_VERSION").into(),
            privileged: crate::config::running_as_root(),
        }
    }

    fn cpu_power_w(&mut self) -> Option<f32> {
        if let Some(w) = hwmon_cpu_power_w() {
            return Some(w);
        }
        let now = Instant::now();
        let energy = package_energy_uj()?;
        let watts = if let (Some(prev), Some(at)) = (self.prev_rapl_uj, self.prev_rapl_at) {
            let dt = now.duration_since(at).as_secs_f64();
            if dt >= 0.4 {
                let duj = energy.saturating_sub(prev) as f64;
                Some((duj / dt / 1_000_000.0) as f32)
            } else {
                None
            }
        } else {
            None
        };
        self.prev_rapl_uj = Some(energy);
        self.prev_rapl_at = Some(now);
        watts.filter(|w| w.is_finite() && *w >= 0.0 && *w < 2000.0)
    }
}

pub fn list_processes() -> Vec<ProcInfo> {
    let mut sys = System::new();
    let kind = ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_exe(UpdateKind::OnlyIfNotSet);
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    std::thread::sleep(Duration::from_millis(200));
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let mut processes: Vec<ProcInfo> = sys
        .processes()
        .iter()
        .filter(|(_, p)| p.thread_kind() != Some(ThreadKind::Kernel))
        .map(|(pid, p)| ProcInfo {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().into_owned(),
            cpu_percent: p.cpu_usage(),
            mem_bytes: p.memory(),
        })
        .collect();
    processes.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.mem_bytes.cmp(&a.mem_bytes))
    });
    processes.truncate(250);
    processes
}

pub fn signal_process(pid: u32) -> Result<(), crate::error::ApiError> {
    use crate::error::ApiError;
    if pid <= 1 {
        return Err(ApiError::Forbidden);
    }
    if pid == std::process::id() {
        return Err(ApiError::Forbidden);
    }
    let mut sys = System::new();
    let kind = ProcessRefreshKind::nothing()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        true,
        kind,
    );
    let Some(proc) = sys.process(sysinfo::Pid::from_u32(pid)) else {
        return Err(ApiError::NotFound);
    };
    if proc.thread_kind() == Some(ThreadKind::Kernel) {
        return Err(ApiError::Forbidden);
    }
    let name = proc.name().to_string_lossy();
    if name == "coduosd" {
        return Err(ApiError::Forbidden);
    }
    if proc
        .exe()
        .map(|p| p.file_name().is_some_and(|n| n == "coduosd"))
        .unwrap_or(false)
    {
        return Err(ApiError::Forbidden);
    }
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        if rc != 0 {
            return Err(ApiError::BadRequest(format!(
                "could not signal pid {pid}: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err(ApiError::BadRequest("signals are unix-only".into()))
    }
}

pub fn docker_stats() -> Vec<DockerStat> {
    let out = std::process::Command::new("docker")
        .args([
            "stats",
            "--no-stream",
            "--format",
            "{{json .}}",
        ])
        .output();
    let Ok(out) = out else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<DockerStatRaw>(line.trim()).ok())
        .map(|raw| DockerStat {
            name: raw.name.unwrap_or_default(),
            cpu_percent: parse_pct(&raw.cpu_perc.unwrap_or_default()),
            mem_usage: raw.mem_usage.unwrap_or_default(),
            pids: raw.pids.unwrap_or_default(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerStat {
    pub name: String,
    pub cpu_percent: f32,
    pub mem_usage: String,
    pub pids: String,
}

#[derive(Deserialize)]
struct DockerStatRaw {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "CPUPerc")]
    cpu_perc: Option<String>,
    #[serde(rename = "MemUsage")]
    mem_usage: Option<String>,
    #[serde(rename = "PIDs")]
    pids: Option<String>,
}

use serde::Deserialize;

fn parse_pct(s: &str) -> f32 {
    s.trim().trim_end_matches('%').parse().unwrap_or(0.0)
}

fn interesting_mount(mount: &Path) -> bool {
    mount == PathBuf::from("/")
        || mount.starts_with("/DATA")
        || mount.starts_with("/mnt")
        || mount.starts_with("/media")
        || mount.starts_with("/home")
}

fn is_virtual(name: &str) -> bool {
    name.starts_with("docker")
        || name.starts_with("br-")
        || name.starts_with("veth")
        || name.starts_with("virbr")
        || name.starts_with("cni")
        || name.starts_with("flannel")
}

fn sysfs_trim(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn ipv4_map() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(out) = std::process::Command::new("ip")
        .args(["-j", "-4", "addr"])
        .output()
    else {
        return map;
    };
    if !out.status.success() {
        return map;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return map;
    };
    let Some(arr) = v.as_array() else {
        return map;
    };
    for iface in arr {
        let Some(name) = iface.get("ifname").and_then(|x| x.as_str()) else {
            continue;
        };
        if let Some(infos) = iface.get("addr_info").and_then(|x| x.as_array()) {
            for info in infos {
                if info.get("family").and_then(|x| x.as_str()) == Some("inet") {
                    if let Some(local) = info.get("local").and_then(|x| x.as_str()) {
                        map.insert(name.to_string(), local.to_string());
                        break;
                    }
                }
            }
        }
    }
    map
}

fn pick_cpu_temp(sensors: &[SensorInfo]) -> Option<f32> {
    let prefer = ["tctl", "package", "coretemp", "k10temp", "zenpower", "cpu"];
    for key in prefer {
        if let Some(s) = sensors.iter().find(|s| s.label.to_ascii_lowercase().contains(key)) {
            return Some(s.temp_c);
        }
    }
    sensors.first().map(|s| s.temp_c)
}

fn collect_gpus(sensors: &[SensorInfo]) -> Vec<GpuInfo> {
    let mut gpus = nvidia_gpus();
    gpus.extend(sysfs_gpus(sensors));
    gpus
}

fn nvidia_gpus() -> Vec<GpuInfo> {
    let out = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(out) = out else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() < 5 {
                return None;
            }
            Some(GpuInfo {
                name: parts[0].to_string(),
                vendor: "nvidia".into(),
                util_percent: parts[1].parse().ok(),
                mem_used: parts[2]
                    .parse::<f64>()
                    .ok()
                    .map(|mib| (mib * 1024.0 * 1024.0) as u64),
                mem_total: parts[3]
                    .parse::<f64>()
                    .ok()
                    .map(|mib| (mib * 1024.0 * 1024.0) as u64),
                temp_c: parts[4].parse().ok(),
                power_w: parts.get(5).and_then(|s| s.parse().ok()),
            })
        })
        .collect()
}

fn sysfs_u64(path: &PathBuf) -> Option<u64> {
    sysfs_trim(&path.display().to_string()).parse().ok()
}

fn sysfs_f32_milli(path: &PathBuf) -> Option<f32> {
    sysfs_u64(path).map(|v| v as f32 / 1000.0)
}

fn sysfs_f32_micro(path: &PathBuf) -> Option<f32> {
    sysfs_u64(path).map(|v| v as f32 / 1_000_000.0)
}

fn first_hwmon(dev: &PathBuf) -> Option<PathBuf> {
    let dir = std::fs::read_dir(dev.join("hwmon")).ok()?;
    dir.flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("hwmon")))
}

fn hwmon_temp_c(hwmon: &PathBuf) -> Option<f32> {
    for i in 1..=8 {
        if let Some(t) = sysfs_f32_milli(&hwmon.join(format!("temp{i}_input"))) {
            if t > 0.0 && t < 120.0 {
                return Some(t);
            }
        }
    }
    None
}

fn hwmon_power_w(hwmon: &PathBuf) -> Option<f32> {
    sysfs_f32_micro(&hwmon.join("power1_input"))
        .or_else(|| sysfs_f32_micro(&hwmon.join("power1_average")))
        .filter(|w| *w > 0.0 && *w < 2000.0)
}

fn gpu_busy(dev: &PathBuf, card: &PathBuf) -> Option<f32> {
    sysfs_trim(&dev.join("gpu_busy_percent").display().to_string())
        .parse()
        .ok()
        .or_else(|| {
            sysfs_trim(&card.join("gt/gt0/rps_busy_percentage").display().to_string())
                .parse()
                .ok()
        })
        .or_else(|| {
            sysfs_trim(&dev.join("gt/gt0/rps_busy_percentage").display().to_string())
                .parse()
                .ok()
        })
}

fn hwmon_cpu_power_w() -> Option<f32> {
    let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") else {
        return None;
    };
    for ent in entries.flatten() {
        let p = ent.path();
        let name = sysfs_trim(&p.join("name").display().to_string()).to_ascii_lowercase();
        if !(name.contains("zenpower")
            || name.contains("amd_energy")
            || name.contains("rapl")
            || name == "corepower")
        {
            continue;
        }
        if let Some(w) = hwmon_power_w(&p) {
            return Some(w);
        }
    }
    None
}

fn package_energy_uj() -> Option<u64> {
    let Ok(entries) = std::fs::read_dir("/sys/class/powercap") else {
        return None;
    };
    let mut fallback = None;
    for ent in entries.flatten() {
        let p = ent.path();
        let fname = p.file_name()?.to_string_lossy().into_owned();
        let name = sysfs_trim(&p.join("name").display().to_string()).to_ascii_lowercase();
        let energy = sysfs_u64(&p.join("energy_uj"));
        let Some(uj) = energy else {
            continue;
        };
        if name.starts_with("package") {
            return Some(uj);
        }
        if fname.contains("rapl") && fname.ends_with(":0") && !fname.contains(":0:") {
            fallback = Some(uj);
        }
    }
    fallback
}

fn sysfs_gpus(sensors: &[SensorInfo]) -> Vec<GpuInfo> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return out;
    };
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let card = ent.path();
        let dev = card.join("device");
        let vendor_id = sysfs_trim(&dev.join("vendor").display().to_string()).to_ascii_lowercase();
        let vendor = match vendor_id.as_str() {
            "0x1002" => "amd",
            "0x8086" => "intel",
            "0x10de" => continue,
            _ => continue,
        };
        let hwmon = first_hwmon(&dev);
        let mut temp = hwmon.as_ref().and_then(hwmon_temp_c);
        if temp.is_none() {
            let keys = match vendor {
                "amd" => ["amdgpu", "edge", "junction"],
                "intel" => ["i915", "xe", "igpu"],
                _ => ["gpu", "drm", "igpu"],
            };
            temp = sensors.iter().find_map(|s| {
                let l = s.label.to_ascii_lowercase();
                keys.iter().find(|k| l.contains(*k)).map(|_| s.temp_c)
            });
        }
        let power_w = hwmon.as_ref().and_then(hwmon_power_w);
        let util = gpu_busy(&dev, &card);
        let mem_used = sysfs_u64(&dev.join("mem_info_vram_used"));
        let mem_total = sysfs_u64(&dev.join("mem_info_vram_total")).filter(|n| *n > 0);
        let pretty = match vendor {
            "amd" => "AMD graphics".into(),
            "intel" => "Intel graphics".into(),
            _ => format!("{vendor} GPU"),
        };
        out.push(GpuInfo {
            name: pretty,
            vendor: vendor.into(),
            util_percent: util,
            mem_used,
            mem_total,
            temp_c: temp,
            power_w,
        });
    }
    out
}

fn docker_info() -> DockerInfo {
    match std::process::Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
    {
        Ok(out) if out.status.success() => DockerInfo {
            available: true,
            version: Some(String::from_utf8_lossy(&out.stdout).trim().to_string()),
            error: None,
        },
        Ok(out) => DockerInfo {
            available: false,
            version: None,
            error: Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        },
        Err(err) => DockerInfo {
            available: false,
            version: None,
            error: Some(err.to_string()),
        },
    }
}

pub fn channel() -> (tokio::sync::watch::Sender<SystemSummary>, tokio::sync::watch::Receiver<SystemSummary>) {
    tokio::sync::watch::channel(SystemSummary::default())
}

pub type SummaryTx = tokio::sync::watch::Sender<SystemSummary>;
pub type SummaryRx = tokio::sync::watch::Receiver<SystemSummary>;

pub fn disk_usage_by_mount(mount: &str) -> Option<(u64, u64)> {
    let disks = Disks::new_with_refreshed_list();
    disks.iter().find_map(|d| {
        if d.mount_point().display().to_string() == mount {
            let total = d.total_space();
            Some((total.saturating_sub(d.available_space()), total))
        } else {
            None
        }
    })
}
