use std::{
    ffi::OsString,
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

use crate::{
    command::{ensure_command, CommandRunner},
    config::Config,
    proxmox::Vm,
};

#[derive(Clone, Debug)]
pub struct ViewerSession {
    pub vmid: u32,
    pub protocol: Protocol,
    pub process_id: u32,
    pub temp_files: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol {
    Auto,
    Spice,
    Vnc,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Spice => "SPICE",
            Self::Vnc => "VNC",
        }
    }
}

pub fn cleanup_temp_dir(config: &Config) -> Result<()> {
    let temp_dir = config.temp_dir();
    if !temp_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(temp_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if name.starts_with("spice-") && name.ends_with(".vv")
            || name.starts_with("remmina-") && name.ends_with(".remmina")
        {
            let _ = fs::remove_file(path);
        }
    }

    Ok(())
}

pub fn attach_spice(
    config: &Config,
    runner: &CommandRunner,
    node: &str,
    vm: &Vm,
) -> Result<ViewerSession> {
    if !config.spice.enabled {
        bail!("SPICE support is disabled");
    }
    ensure_command(&config.viewer.spice_viewer)?;

    let path = format!("/nodes/{node}/qemu/{}/spiceproxy", vm.vmid);
    let result = runner.run(
        "pvesh",
        &["create", path.as_str(), "--output-format", "json"],
        Duration::from_secs(10),
    )?;

    if result.stdout.trim().is_empty() {
        bail!("SPICE proxy returned empty output");
    }
    let vv_contents = spice_vv_from_pvesh_json(&result.stdout)?;

    ensure_private_dir(&config.spice.vv_dir)?;
    let vv_path = config
        .spice
        .vv_dir
        .join(format!("spice-{}-{}.vv", vm.vmid, timestamp_millis()));
    write_private_file(&vv_path, vv_contents.as_bytes())
        .with_context(|| format!("failed to write {}", vv_path.display()))?;

    let process_id = runner.spawn_detached(
        &config.viewer.spice_viewer,
        &[OsString::from(vv_path.as_os_str())],
    )?;

    delete_later(vv_path.clone(), config.spice.delete_vv_after);

    Ok(ViewerSession {
        vmid: vm.vmid,
        protocol: Protocol::Spice,
        process_id,
        temp_files: vec![vv_path],
    })
}

pub fn attach_vnc(
    config: &Config,
    runner: &CommandRunner,
    node: &str,
    vm: &Vm,
) -> Result<ViewerSession> {
    if !config.vnc.enabled {
        bail!("VNC support is disabled");
    }
    ensure_command(&config.viewer.vnc_viewer)?;

    let path = format!("/nodes/{node}/qemu/{}/vncproxy", vm.vmid);
    let result = runner.run(
        "pvesh",
        &[
            "create",
            path.as_str(),
            "--websocket",
            "0",
            "--output-format",
            "json",
        ],
        Duration::from_secs(10),
    )?;

    let value: Value = serde_json::from_str(&result.stdout)
        .with_context(|| "failed to parse vncproxy JSON output")?;
    let port = value
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("vncproxy output did not include `port`"))?;
    let ticket = value
        .get("ticket")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("vncproxy output did not include `ticket`"))?;

    if ticket.contains('\n') || ticket.contains('\r') {
        bail!("vncproxy ticket contains an unsupported newline");
    }

    ensure_private_dir(&config.vnc.profile_dir)?;
    let profile_path = config.vnc.profile_dir.join(format!(
        "remmina-{}-{}.remmina",
        vm.vmid,
        timestamp_millis()
    ));
    let profile = remmina_profile(vm, port, ticket);
    write_private_file(&profile_path, profile.as_bytes())
        .with_context(|| format!("failed to write {}", profile_path.display()))?;

    let process_id = runner.spawn_detached(
        &config.viewer.vnc_viewer,
        &[
            OsString::from("-c"),
            OsString::from(profile_path.as_os_str()),
        ],
    )?;

    delete_later(profile_path.clone(), config.vnc.delete_profile_after);

    Ok(ViewerSession {
        vmid: vm.vmid,
        protocol: Protocol::Vnc,
        process_id,
        temp_files: vec![profile_path],
    })
}

fn spice_vv_from_pvesh_json(stdout: &str) -> Result<String> {
    let value = parse_json_object(stdout)?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("spiceproxy JSON output was not an object"))?;

    for required in ["type", "host", "proxy", "password"] {
        if string_field(object, required).is_none() {
            bail!("spiceproxy JSON output did not include `{required}`");
        }
    }

    let preferred_order = [
        "type",
        "host",
        "port",
        "tls-port",
        "password",
        "proxy",
        "ca",
        "host-subject",
        "toggle-fullscreen",
        "release-cursor",
        "secure-attention",
        "title",
        "delete-this-file",
    ];

    let mut output = String::from("[virt-viewer]\n");
    for key in preferred_order {
        if let Some(value) = vv_value(object, key) {
            output.push_str(key);
            output.push('=');
            output.push_str(&value);
            output.push('\n');
        }
    }

    let mut extra_keys: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|key| !preferred_order.contains(key))
        .collect();
    extra_keys.sort_unstable();

    for key in extra_keys {
        if let Some(value) = vv_value(object, key) {
            output.push_str(key);
            output.push('=');
            output.push_str(&value);
            output.push('\n');
        }
    }

    Ok(output)
}

fn parse_json_object(stdout: &str) -> Result<Value> {
    let trimmed = stdout.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }

    let start = trimmed
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("spiceproxy output was not JSON"))?;
    serde_json::from_str(&trimmed[start..])
        .with_context(|| "failed to parse spiceproxy JSON output")
}

fn vv_value(object: &Map<String, Value>, key: &str) -> Option<String> {
    match object.get(key)? {
        Value::String(value) => Some(escape_vv_value(value)),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(if *value { "1" } else { "0" }.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn escape_vv_value(value: &str) -> String {
    value.replace('\r', "").replace('\n', "\\n")
}

fn remmina_profile(vm: &Vm, port: u64, ticket: &str) -> String {
    format!(
        "\
[remmina]
name=Proxmox VM {} {}
protocol=VNC
server=127.0.0.1:{}
password={}
disableclipboard=0
viewmode=1
quality=9
colordepth=32
",
        vm.vmid, vm.name, port, ticket
    )
}

fn ensure_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        builder.create(path)?;
    }

    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)?;
    }

    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn delete_later(path: PathBuf, delay: Duration) {
    thread::spawn(move || {
        thread::sleep(delay);
        let _ = fs::remove_file(path);
    });
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_spice_proxy_json_as_virt_viewer_file() {
        let json = r#"{
            "ca": "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----\n",
            "delete-this-file": 1,
            "host": "pvespiceproxy:ticket:127:basic:61000::token",
            "host-subject": "OU=PVE Cluster Node,O=Proxmox Virtual Environment,CN=basic.home.lan",
            "password": "secret",
            "proxy": "http://basic.home.lan:3128",
            "release-cursor": "Ctrl+Alt+R",
            "secure-attention": "Ctrl+Alt+Ins",
            "title": "VM 127 - onicha",
            "tls-port": 61000,
            "toggle-fullscreen": "Shift+F11",
            "type": "spice"
        }"#;

        let vv = spice_vv_from_pvesh_json(json).unwrap();

        assert!(vv.starts_with("[virt-viewer]\ntype=spice\n"));
        assert!(vv.contains("host=pvespiceproxy:ticket:127:basic:61000::token\n"));
        assert!(vv.contains("tls-port=61000\n"));
        assert!(vv.contains("password=secret\n"));
        assert!(vv.contains("proxy=http://basic.home.lan:3128\n"));
        assert!(
            vv.contains("ca=-----BEGIN CERTIFICATE-----\\nabc\\n-----END CERTIFICATE-----\\n\n")
        );
    }
}
