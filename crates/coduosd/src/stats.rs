use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use sysinfo::{Disks, Networks, System};
use tokio::sync::watch;

#[derive(Debug, Clone, Serialize)]
pub struct SystemSummary {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub uptime_secs: u64,
    pub cpu_percent: f32,
    pub cpu_cores: usize,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetInfo>,
    pub docker: DockerInfo,
    pub version: String,
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
            mem_used: 0,
            mem_total: 0,
            swap_used: 0,
            swap_total: 0,
            disks: vec![],
            networks: vec![],
            docker: DockerInfo {
                available: false,
                version: None,
                error: None,
            },
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

pub fn spawn_collector(tx: watch::Sender<SystemSummary>) {
    tokio::task::spawn_blocking(move || {
        let mut sys = System::new();
        loop {
            let summary = collect(&mut sys);
            if tx.send(summary).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn collect(sys: &mut System) -> SystemSummary {
    sys.refresh_memory();
    sys.refresh_cpu_all();
    std::thread::sleep(Duration::from_millis(200));
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let disks = Disks::new_with_refreshed_list();
    let nets = Networks::new_with_refreshed_list();

    let disk_infos = disks
        .iter()
        .filter(|d| {
            let mount = d.mount_point();
            mount == PathBuf::from("/")
                || mount.starts_with("/DATA")
                || mount.starts_with("/mnt")
                || mount.starts_with("/media")
                || mount.starts_with("/home")
        })
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

    let net_infos = nets
        .iter()
        .filter(|(name, _)| {
            let n = name.as_str();
            !n.starts_with("lo") && !n.starts_with("docker") && !n.starts_with("br-") && !n.starts_with("veth")
        })
        .map(|(name, data)| NetInfo {
            name: name.clone(),
            rx_bytes: data.total_received(),
            tx_bytes: data.total_transmitted(),
        })
        .collect();

    let docker = docker_info();

    SystemSummary {
        hostname: System::host_name().unwrap_or_else(|| "coduos".into()),
        os: System::long_os_version().unwrap_or_else(|| "Linux".into()),
        kernel: System::kernel_version().unwrap_or_default(),
        uptime_secs: System::uptime(),
        cpu_percent: sys.global_cpu_usage(),
        cpu_cores: sys.cpus().len(),
        mem_used: sys.used_memory(),
        mem_total: sys.total_memory(),
        swap_used: sys.used_swap(),
        swap_total: sys.total_swap(),
        disks: disk_infos,
        networks: net_infos,
        docker,
        version: env!("CARGO_PKG_VERSION").into(),
    }
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

pub fn channel() -> (watch::Sender<SystemSummary>, watch::Receiver<SystemSummary>) {
    watch::channel(SystemSummary::default())
}

pub type SummaryTx = watch::Sender<SystemSummary>;
pub type SummaryRx = watch::Receiver<SystemSummary>;
