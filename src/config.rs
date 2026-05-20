use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Config {
    pub proxmox: ProxmoxConfig,
    pub viewer: ViewerConfig,
    pub vnc: VncConfig,
    pub spice: SpiceConfig,
    pub ui: UiConfig,
    pub logging: LoggingConfig,
}

#[derive(Clone, Debug)]
pub struct ProxmoxConfig {
    pub node: String,
    pub command_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct ViewerConfig {
    pub vnc_viewer: String,
    pub spice_viewer: String,
}

#[derive(Clone, Debug)]
pub struct VncConfig {
    pub enabled: bool,
    pub profile_dir: PathBuf,
    pub delete_profile_after: Duration,
}

#[derive(Clone, Debug)]
pub struct SpiceConfig {
    pub enabled: bool,
    pub vv_dir: PathBuf,
    pub delete_vv_after: Duration,
}

#[derive(Clone, Debug)]
pub struct UiConfig {
    pub refresh_interval: Duration,
    pub confirm_destructive_actions: bool,
}

#[derive(Clone, Debug)]
pub struct LoggingConfig {
    pub file: PathBuf,
}

impl Config {
    pub fn load() -> Self {
        Self::default()
    }

    pub fn temp_dir(&self) -> &Path {
        &self.spice.vv_dir
    }
}

impl Default for Config {
    fn default() -> Self {
        let temp_dir = env::temp_dir().join("pve-vm-launcher");

        Self {
            proxmox: ProxmoxConfig {
                node: "auto".to_string(),
                command_timeout: Duration::from_secs(15),
            },
            viewer: ViewerConfig {
                vnc_viewer: "remmina".to_string(),
                spice_viewer: "remote-viewer".to_string(),
            },
            vnc: VncConfig {
                enabled: true,
                profile_dir: temp_dir.clone(),
                delete_profile_after: Duration::from_secs(30),
            },
            spice: SpiceConfig {
                enabled: true,
                vv_dir: temp_dir,
                delete_vv_after: Duration::from_secs(30),
            },
            ui: UiConfig {
                refresh_interval: Duration::from_secs(3),
                confirm_destructive_actions: true,
            },
            logging: LoggingConfig {
                file: default_log_file(),
            },
        }
    }
}

fn default_log_file() -> PathBuf {
    if let Some(home) = env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("pve-vm-launcher")
            .join("app.log")
    } else {
        env::temp_dir().join("pve-vm-launcher").join("app.log")
    }
}
