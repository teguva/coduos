use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_www_dir")]
    pub www_dir: PathBuf,
    #[serde(default = "default_github_owner")]
    pub github_owner: String,
    #[serde(default = "default_github_repo")]
    pub github_repo: String,
    #[serde(default)]
    pub file_roots: Vec<FileRoot>,
    #[serde(default)]
    pub units: Vec<UnitSpec>,
    #[serde(default)]
    pub storage_mounts: Vec<StorageMount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_favorites: Vec<FileFavorite>,
    #[serde(default, skip_serializing_if = "BatteryConfig::is_empty")]
    pub battery: BatteryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BatteryConfig {
    /// Stop charging at this percent. 100 = full. Applied at boot when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_pct: Option<u8>,
    /// Resume charging below this percent (ThinkPad-style hysteresis).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_pct: Option<u8>,
}

impl BatteryConfig {
    fn is_empty(&self) -> bool {
        self.limit_pct.is_none() && self.start_pct.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFavorite {
    pub root: String,
    pub path: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRoot {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

fn default_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMount {
    pub uuid: String,
    pub mountpoint: PathBuf,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub label: String,
    /// Remount this volume when the daemon starts. Missing from older configs = true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub auto_mount: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitSpec {
    pub id: String,
    pub unit: String,
    pub label: String,
}

fn default_bind() -> String {
    "127.0.0.1:13209".into()
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/coduos")
}

fn default_www_dir() -> PathBuf {
    PathBuf::from("/usr/share/coduos/www")
}

fn default_github_owner() -> String {
    "teguva".into()
}

fn default_github_repo() -> String {
    "coduos".into()
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<(Self, PathBuf)> {
        let path = match path {
            Some(p) => p.to_path_buf(),
            None => default_config_path(),
        };

        let mut cfg = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read config {}", path.display()))?;
            toml::from_str::<Config>(&raw).context("parse config")?
        } else {
            Self::for_environment()
        };

        if cfg.file_roots.is_empty() {
            cfg.file_roots = default_file_roots();
        }
        if cfg.units.is_empty() {
            cfg.units = default_units();
        }
        if !running_as_root() && cfg.bind.ends_with(":80") {
            tracing::warn!("not root; binding 127.0.0.1:13209 instead of :80");
            cfg.bind = "127.0.0.1:13209".into();
            if cfg.data_dir == PathBuf::from("/var/lib/coduos") {
                cfg.data_dir = PathBuf::from("./data");
            }
            if !cfg.www_dir.exists() {
                cfg.www_dir = PathBuf::from("web/dist");
            }
        }

        Ok((cfg, path))
    }

    pub fn for_environment() -> Self {
        let mut cfg = Self {
            bind: default_bind(),
            data_dir: default_data_dir(),
            www_dir: default_www_dir(),
            github_owner: default_github_owner(),
            github_repo: default_github_repo(),
            file_roots: default_file_roots(),
            units: default_units(),
            storage_mounts: Vec::new(),
            file_favorites: Vec::new(),
            battery: BatteryConfig::default(),
        };
        if !running_as_root() {
            cfg.bind = "127.0.0.1:13209".into();
            cfg.data_dir = PathBuf::from("./data");
            cfg.www_dir = PathBuf::from("web/dist");
        }
        cfg
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("coduos.db")
    }

    pub fn apps_dir(&self) -> PathBuf {
        self.data_dir.join("apps")
    }

    pub fn icons_dir(&self) -> PathBuf {
        self.data_dir.join("icons")
    }

    pub fn file_root(&self, id: &str) -> Option<&FileRoot> {
        self.file_roots.iter().find(|r| r.id == id)
    }

    pub fn unit(&self, id: &str) -> Option<&UnitSpec> {
        self.units.iter().find(|u| u.id == id)
    }
}

fn default_config_path() -> PathBuf {
    if running_as_root() {
        PathBuf::from("/etc/coduos/coduos.toml")
    } else if Path::new("/etc/coduos/coduos.toml").exists() {
        PathBuf::from("/etc/coduos/coduos.toml")
    } else {
        PathBuf::from("coduos.toml")
    }
}

fn default_file_roots() -> Vec<FileRoot> {
    let mut roots = Vec::new();
    if Path::new("/DATA").is_dir() {
        roots.push(FileRoot {
            id: "data".into(),
            label: "Data".into(),
            path: PathBuf::from("/DATA"),
        });
    }
    if Path::new("/home").is_dir() {
        roots.push(FileRoot {
            id: "home".into(),
            label: "Home".into(),
            path: PathBuf::from("/home"),
        });
    }
    if roots.is_empty() {
        roots.push(FileRoot {
            id: "root".into(),
            label: "Filesystem".into(),
            path: PathBuf::from("/"),
        });
    }
    roots
}

fn default_units() -> Vec<UnitSpec> {
    vec![
        UnitSpec {
            id: "docker".into(),
            unit: "docker.service".into(),
            label: "Docker".into(),
        },
        UnitSpec {
            id: "ssh".into(),
            unit: "ssh.service".into(),
            label: "SSH".into(),
        },
        UnitSpec {
            id: "nginx".into(),
            unit: "nginx.service".into(),
            label: "nginx".into(),
        },
        UnitSpec {
            id: "wireguard".into(),
            unit: "coduos-wg.service".into(),
            label: "WireGuard".into(),
        },
    ]
}

pub fn running_as_root() -> bool {
    #[cfg(unix)]
    {
        libc_geteuid() == 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
fn libc_geteuid() -> u32 {
    unsafe { libc::geteuid() }
}

pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if matches!(c, '-' | '_' | ' ') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "app".into()
    } else {
        out.chars().take(48).collect()
    }
}
