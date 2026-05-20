use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Result};

use crate::{command::CommandRunner, config::Config};

#[derive(Clone, Debug)]
pub struct Vm {
    pub vmid: u32,
    pub name: String,
    pub status: String,
    pub node: String,
    pub memory_mb: Option<u64>,
    pub bootdisk_gb: Option<f64>,
    pub pid: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerAction {
    Start,
    Shutdown,
    Reboot,
    Stop,
    Reset,
}

#[derive(Clone, Debug)]
pub struct Proxmox {
    runner: CommandRunner,
    configured_node: String,
    node: String,
    command_timeout: Duration,
}

impl Proxmox {
    pub fn new(runner: CommandRunner, config: &Config) -> Self {
        Self {
            runner,
            configured_node: config.proxmox.node.clone(),
            node: "unknown".to_string(),
            command_timeout: config.proxmox.command_timeout,
        }
    }

    pub fn node(&self) -> &str {
        &self.node
    }

    pub fn detect_node(&mut self) {
        if self.configured_node != "auto" {
            self.node = self.configured_node.clone();
            return;
        }

        self.node = self
            .runner
            .run(
                "hostname",
                &["-s"],
                self.command_timeout.min(Duration::from_secs(3)),
            )
            .or_else(|_| {
                self.runner.run(
                    "hostname",
                    &[],
                    self.command_timeout.min(Duration::from_secs(3)),
                )
            })
            .map(|result| normalize_node_name(&result.stdout))
            .unwrap_or_else(|_| "localhost".to_string());
    }

    pub fn list_vms(&self) -> Result<Vec<Vm>> {
        let result = self.runner.run(
            "qm",
            &["list"],
            self.command_timeout.min(Duration::from_secs(5)),
        )?;
        parse_qm_list(&result.stdout, &self.node)
    }

    pub fn power_action(&self, vmid: u32, action: PowerAction) -> Result<()> {
        let vmid = vmid.to_string();
        let command = action.qm_command();
        let timeout = action.timeout();
        self.runner.run("qm", &[command, vmid.as_str()], timeout)?;
        Ok(())
    }

    pub fn wait_for_status(
        &self,
        vmid: u32,
        desired_status: &str,
        max_wait: Duration,
    ) -> Result<Option<Vm>> {
        let deadline = Instant::now() + max_wait;

        loop {
            let vms = self.list_vms()?;
            let last_seen = vms.into_iter().find(|vm| vm.vmid == vmid);

            if last_seen
                .as_ref()
                .map(|vm| vm.status == desired_status)
                .unwrap_or(false)
            {
                return Ok(last_seen);
            }

            if Instant::now() >= deadline {
                return Ok(last_seen);
            }

            thread::sleep(Duration::from_secs(1));
        }
    }

    pub fn runner(&self) -> &CommandRunner {
        &self.runner
    }
}

impl PowerAction {
    fn qm_command(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Shutdown => "shutdown",
            Self::Reboot => "reboot",
            Self::Stop => "stop",
            Self::Reset => "reset",
        }
    }

    fn timeout(self) -> Duration {
        match self {
            Self::Start => Duration::from_secs(30),
            Self::Shutdown => Duration::from_secs(60),
            Self::Reboot => Duration::from_secs(60),
            Self::Stop => Duration::from_secs(30),
            Self::Reset => Duration::from_secs(30),
        }
    }
}

pub fn parse_qm_list(output: &str, node: &str) -> Result<Vec<Vm>> {
    let mut vms = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.first() == Some(&"VMID") {
            continue;
        }

        if columns.len() < 6 {
            bail!("unexpected `qm list` row: {line}");
        }

        let vmid = columns[0].parse::<u32>()?;
        let status_index = columns
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, value)| is_vm_status(value).then_some(index))
            .ok_or_else(|| anyhow::anyhow!("status column not found in row: {line}"))?;

        if status_index + 3 >= columns.len() {
            bail!("missing numeric columns in `qm list` row: {line}");
        }

        let name = columns[1..status_index].join(" ");
        let status = columns[status_index].to_string();
        let memory_mb = columns[status_index + 1].parse::<u64>().ok();
        let bootdisk_gb = columns[status_index + 2].parse::<f64>().ok();
        let pid = columns[status_index + 3].parse::<u32>().ok();

        vms.push(Vm {
            vmid,
            name,
            status,
            node: node.to_string(),
            memory_mb,
            bootdisk_gb,
            pid,
        });
    }

    Ok(vms)
}

fn is_vm_status(value: &str) -> bool {
    matches!(
        value,
        "running" | "stopped" | "paused" | "suspended" | "prelaunch" | "unknown"
    )
}

fn normalize_node_name(stdout: &str) -> String {
    let trimmed = stdout.trim();
    let short_name = trimmed.split('.').next().unwrap_or(trimmed);

    if short_name.is_empty() {
        "localhost".to_string()
    } else {
        short_name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_qm_list() {
        let input = r#"
      VMID NAME                 STATUS     MEM(MB)    BOOTDISK(GB) PID
       100 win11-dev            running    8192       64.00        12345
       101 ubuntu-lab           stopped    4096       32.00        0
"#;

        let vms = parse_qm_list(input, "pve01").unwrap();

        assert_eq!(vms.len(), 2);
        assert_eq!(vms[0].vmid, 100);
        assert_eq!(vms[0].name, "win11-dev");
        assert_eq!(vms[0].status, "running");
        assert_eq!(vms[0].memory_mb, Some(8192));
        assert_eq!(vms[0].bootdisk_gb, Some(64.0));
        assert_eq!(vms[0].pid, Some(12345));
        assert_eq!(vms[0].node, "pve01");
    }

    #[test]
    fn parses_names_with_spaces_when_status_is_known() {
        let input = r#"
      VMID NAME                 STATUS     MEM(MB)    BOOTDISK(GB) PID
       200 windows 11 dev       running    8192       64.00        999
"#;

        let vms = parse_qm_list(input, "pve01").unwrap();

        assert_eq!(vms[0].name, "windows 11 dev");
        assert_eq!(vms[0].status, "running");
    }
}
