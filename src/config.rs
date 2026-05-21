use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub keep_temp_files: bool,
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
    pub spice: ViewerLaunchConfig,
    pub vnc: ViewerLaunchConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewerLaunchConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub run_as_invoking_user: bool,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CliOptions {
    pub keep_temp_files: bool,
    pub help: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigSection {
    Root,
    ViewerSpice,
    ViewerSpiceEnv,
    ViewerVnc,
    ViewerVncEnv,
    Other,
}

impl Config {
    pub fn load(options: CliOptions) -> Result<Self> {
        let mut config = Self {
            keep_temp_files: options.keep_temp_files,
            ..Self::default()
        };

        if let Some(path) = config_file_path() {
            ensure_default_config_file(&path)?;
            if path.exists() {
                let raw = fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                apply_config_file(&mut config, &raw, &path)?;
            }
        }

        Ok(config)
    }

    pub fn temp_dir(&self) -> &Path {
        &self.spice.vv_dir
    }
}

impl CliOptions {
    pub fn parse<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut options = Self::default();

        for arg in args {
            match arg.into().as_str() {
                "--keep-temp-files" => options.keep_temp_files = true,
                "-h" | "--help" => options.help = true,
                other => bail!("unknown option `{other}`\n\n{}", usage()),
            }
        }

        Ok(options)
    }
}

pub fn usage() -> &'static str {
    "\
Usage: pve-vm-launcher [OPTIONS]

Options:
      --keep-temp-files  Keep generated .vv/.remmina files and skip startup temp cleanup
  -h, --help             Show this help
"
}

impl ViewerLaunchConfig {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: Vec::new(),
            run_as_invoking_user: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let temp_dir = env::temp_dir().join("pve-vm-launcher");

        Self {
            keep_temp_files: false,
            proxmox: ProxmoxConfig {
                node: "auto".to_string(),
                command_timeout: Duration::from_secs(15),
            },
            viewer: ViewerConfig {
                spice: ViewerLaunchConfig::new("remote-viewer"),
                vnc: ViewerLaunchConfig::new("remmina"),
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

pub fn config_file_path() -> Option<PathBuf> {
    if let Some(home) = invoking_user_home() {
        return Some(
            home.join(".config")
                .join("pve-vm-launcher")
                .join("config.toml"),
        );
    }

    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Some(
            PathBuf::from(path)
                .join("pve-vm-launcher")
                .join("config.toml"),
        );
    }

    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("pve-vm-launcher")
            .join("config.toml")
    })
}

fn ensure_default_config_file(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        chown_for_invoking_user(parent);

        if let Some(config_home) = parent.parent() {
            chown_for_invoking_user(config_home);
        }
    }

    fs::write(path, default_config_toml())
        .with_context(|| format!("failed to write default config {}", path.display()))?;
    chown_for_invoking_user(path);

    Ok(())
}

fn default_config_toml() -> &'static str {
    include_str!("../docs/config.example.toml")
}

fn invoking_user_home() -> Option<PathBuf> {
    let user = env::var("SUDO_USER").ok()?;
    if user.is_empty() || user == "root" {
        return None;
    }

    passwd_home(&user)
}

fn chown_for_invoking_user(path: &Path) {
    let uid = env::var("SUDO_UID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let gid = env::var("SUDO_GID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());

    if uid.is_none() && gid.is_none() {
        return;
    }

    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::chown(path, uid, gid);
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn passwd_home(user_name: &str) -> Option<PathBuf> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        if name != user_name {
            return None;
        }

        let _password = fields.next()?;
        let _uid = fields.next()?;
        let _gid = fields.next()?;
        let _gecos = fields.next()?;
        let home = fields.next()?;
        (!home.is_empty()).then(|| PathBuf::from(home))
    })
}

fn apply_config_file(config: &mut Config, raw: &str, path: &Path) -> Result<()> {
    let mut section = ConfigSection::Root;
    let mut spice_env = BTreeMap::new();
    let mut vnc_env = BTreeMap::new();

    for (index, raw_line) in raw.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            section =
                parse_section(line).with_context(|| format!("{}:{line_number}", path.display()))?;
            continue;
        }

        let (key, value) =
            parse_assignment(line).with_context(|| format!("{}:{line_number}", path.display()))?;

        match section {
            ConfigSection::ViewerSpice => apply_viewer_key(&mut config.viewer.spice, key, value)
                .with_context(|| format!("{}:{line_number}", path.display()))?,
            ConfigSection::ViewerVnc => apply_viewer_key(&mut config.viewer.vnc, key, value)
                .with_context(|| format!("{}:{line_number}", path.display()))?,
            ConfigSection::ViewerSpiceEnv => {
                spice_env.insert(key.to_string(), parse_string(value)?);
            }
            ConfigSection::ViewerVncEnv => {
                vnc_env.insert(key.to_string(), parse_string(value)?);
            }
            ConfigSection::Root | ConfigSection::Other => {}
        }
    }

    config.viewer.spice.env = spice_env.into_iter().collect();
    config.viewer.vnc.env = vnc_env.into_iter().collect();
    Ok(())
}

fn parse_section(line: &str) -> Result<ConfigSection> {
    if !line.ends_with(']') {
        bail!("unterminated section header");
    }

    Ok(match &line[1..line.len() - 1] {
        "viewer.spice" => ConfigSection::ViewerSpice,
        "viewer.spice.env" => ConfigSection::ViewerSpiceEnv,
        "viewer.vnc" => ConfigSection::ViewerVnc,
        "viewer.vnc.env" => ConfigSection::ViewerVncEnv,
        _ => ConfigSection::Other,
    })
}

fn parse_assignment(line: &str) -> Result<(&str, &str)> {
    let Some((key, value)) = line.split_once('=') else {
        bail!("expected `key = value`");
    };

    let key = key.trim();
    if key.is_empty() {
        bail!("empty key");
    }

    Ok((key, value.trim()))
}

fn apply_viewer_key(viewer: &mut ViewerLaunchConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "command" => viewer.command = parse_string(value)?,
        "args" => viewer.args = parse_string_array(value)?,
        "run_as_invoking_user" => viewer.run_as_invoking_user = parse_bool(value)?,
        _ => {}
    }

    Ok(())
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => bail!("expected boolean"),
    }
}

fn parse_string(value: &str) -> Result<String> {
    let value = value.trim();
    if !value.starts_with('"') || !value.ends_with('"') {
        bail!("expected quoted string");
    }

    unescape_toml_string(&value[1..value.len() - 1])
}

fn parse_string_array(value: &str) -> Result<Vec<String>> {
    let value = value.trim();
    if !value.starts_with('[') || !value.ends_with(']') {
        bail!("expected string array");
    }

    let inner = value[1..value.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }

    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for character in inner.chars() {
        if escaped {
            current.push('\\');
            current.push(character);
            escaped = false;
            continue;
        }

        match character {
            '\\' if in_string => escaped = true,
            '"' => {
                current.push(character);
                in_string = !in_string;
            }
            ',' if !in_string => {
                values.push(parse_string(current.trim())?);
                current.clear();
            }
            _ => current.push(character),
        }
    }

    if in_string {
        bail!("unterminated string in array");
    }
    if escaped {
        current.push('\\');
    }

    if !current.trim().is_empty() {
        values.push(parse_string(current.trim())?);
    }
    Ok(values)
}

fn unescape_toml_string(value: &str) -> Result<String> {
    let mut output = String::new();
    let mut chars = value.chars();

    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }

        match chars.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some('"') => output.push('"'),
            Some('\\') => output.push('\\'),
            Some(other) => bail!("unsupported escape sequence `\\{other}`"),
            None => bail!("unterminated escape sequence"),
        }
    }

    Ok(output)
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match character {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..index],
            _ => {}
        }
    }

    line
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keep_temp_files_option() {
        let options = CliOptions::parse(["--keep-temp-files"]).unwrap();

        assert_eq!(
            options,
            CliOptions {
                keep_temp_files: true,
                help: false,
            }
        );
    }

    #[test]
    fn parses_help_option() {
        let options = CliOptions::parse(["--help"]).unwrap();

        assert!(options.help);
    }

    #[test]
    fn rejects_unknown_option() {
        let error = CliOptions::parse(["--wat"]).unwrap_err().to_string();

        assert!(error.contains("unknown option `--wat`"));
    }

    #[test]
    fn applies_viewer_launch_config_file() {
        let raw = r#"
            [viewer.spice]
            command = "virt-viewer"
            args = ["--full-screen", "--zoom=100"]
            run_as_invoking_user = false

            [viewer.spice.env]
            GDK_BACKEND = "x11"
            SPICE_DEBUG = "1"

            [viewer.vnc]
            command = "flatpak"
            args = ["run", "org.remmina.Remmina"]

            [viewer.vnc.env]
            REMMINA_PREF = "local"
        "#;
        let mut config = Config::default();

        apply_config_file(&mut config, raw, Path::new("config.toml")).unwrap();

        assert_eq!(config.viewer.spice.command, "virt-viewer");
        assert!(!config.viewer.spice.run_as_invoking_user);
        assert_eq!(
            config.viewer.spice.args,
            vec!["--full-screen".to_string(), "--zoom=100".to_string()]
        );
        assert_eq!(
            config.viewer.spice.env,
            vec![
                ("GDK_BACKEND".to_string(), "x11".to_string()),
                ("SPICE_DEBUG".to_string(), "1".to_string()),
            ]
        );
        assert_eq!(config.viewer.vnc.command, "flatpak");
        assert_eq!(
            config.viewer.vnc.args,
            vec!["run".to_string(), "org.remmina.Remmina".to_string()]
        );
        assert_eq!(
            config.viewer.vnc.env,
            vec![("REMMINA_PREF".to_string(), "local".to_string())]
        );
    }

    #[test]
    fn strips_comments_outside_strings() {
        let raw = r#"
            [viewer.spice]
            command = "remote-viewer#debug" # comment
            args = ["--title=a#b"]
        "#;
        let mut config = Config::default();

        apply_config_file(&mut config, raw, Path::new("config.toml")).unwrap();

        assert_eq!(config.viewer.spice.command, "remote-viewer#debug");
        assert_eq!(config.viewer.spice.args, vec!["--title=a#b".to_string()]);
    }

    #[test]
    fn bundled_default_config_is_parseable() {
        let mut config = Config::default();

        apply_config_file(
            &mut config,
            default_config_toml(),
            Path::new("config.example.toml"),
        )
        .unwrap();

        assert_eq!(config.viewer.spice.command, "remote-viewer");
        assert_eq!(config.viewer.vnc.command, "remmina");
        assert!(config.viewer.spice.run_as_invoking_user);
        assert!(config.viewer.vnc.run_as_invoking_user);
    }
}
