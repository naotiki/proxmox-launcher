use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    command::CommandRunner,
    config::Config,
    proxmox::{PowerAction, Proxmox, Vm},
    viewer::{self, Protocol},
};

pub const ACTIONS: [Action; 8] = [
    Action::Attach(Protocol::Auto),
    Action::Attach(Protocol::Spice),
    Action::Attach(Protocol::Vnc),
    Action::Start,
    Action::Shutdown,
    Action::Reboot,
    Action::Stop,
    Action::Reset,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Attach(Protocol),
    Start,
    Shutdown,
    Reboot,
    Stop,
    Reset,
    Refresh,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Mode {
    Browsing,
    ActionMenu,
    Confirm {
        pending: PendingAction,
        message: String,
    },
    Logs,
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingAction {
    pub vmid: u32,
    pub action: Action,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub proxmox: Proxmox,
    pub vms: Vec<Vm>,
    pub selected: usize,
    pub mode: Mode,
    pub action_menu_index: usize,
    pub logs: VecDeque<String>,
    pub status_line: String,
    pub should_quit: bool,
    pub started_at: Instant,
    last_refresh: Instant,
}

impl App {
    pub fn new(config: Config) -> Self {
        let runner = CommandRunner::new(config.logging.file.clone());
        let proxmox = Proxmox::new(runner, &config);

        Self {
            config,
            proxmox,
            vms: Vec::new(),
            selected: 0,
            mode: Mode::Browsing,
            action_menu_index: 0,
            logs: VecDeque::new(),
            status_line: "Starting".to_string(),
            should_quit: false,
            started_at: Instant::now(),
            last_refresh: Instant::now(),
        }
    }

    pub fn bootstrap(&mut self) {
        self.proxmox.detect_node();
        self.log(format!("Node: {}", self.proxmox.node()));
        self.refresh();
    }

    pub fn tick(&mut self) {
        if self.last_refresh.elapsed() >= self.config.ui.refresh_interval
            && matches!(self.mode, Mode::Browsing | Mode::Logs)
        {
            self.refresh();
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.mode.clone() {
            Mode::Browsing => self.handle_browsing_key(key),
            Mode::ActionMenu => self.handle_action_menu_key(key),
            Mode::Confirm { pending, .. } => self.handle_confirm_key(key, pending),
            Mode::Logs => self.handle_logs_key(key),
            Mode::Help => self.handle_help_key(key),
        }
    }

    pub fn selected_vm(&self) -> Option<&Vm> {
        self.vms.get(self.selected)
    }

    pub fn log(&mut self, message: impl Into<String>) {
        let elapsed = self.started_at.elapsed().as_secs_f32();
        let message = message.into();
        self.status_line = message.clone();
        self.logs.push_back(format!("{elapsed:>7.1}s  {message}"));
        while self.logs.len() > 200 {
            self.logs.pop_front();
        }
    }

    fn handle_browsing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Enter => self.open_action_menu(),
            KeyCode::Char('r') => self.execute(Action::Refresh),
            KeyCode::Char('s') => self.execute(Action::Start),
            KeyCode::Char('S') => self.execute(Action::Shutdown),
            KeyCode::Char('f') => self.execute(Action::Stop),
            KeyCode::Char('b') => self.execute(Action::Reboot),
            KeyCode::Char('x') => self.execute(Action::Reset),
            KeyCode::Char('a') => self.execute(Action::Attach(Protocol::Auto)),
            KeyCode::Char('p') => self.execute(Action::Attach(Protocol::Spice)),
            KeyCode::Char('v') => self.execute(Action::Attach(Protocol::Vnc)),
            KeyCode::Char('l') => self.mode = Mode::Logs,
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
    }

    fn handle_action_menu_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Browsing,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.action_menu_index == 0 {
                    self.action_menu_index = ACTIONS.len() - 1;
                } else {
                    self.action_menu_index -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.action_menu_index = (self.action_menu_index + 1) % ACTIONS.len();
            }
            KeyCode::Enter => {
                let action = ACTIONS[self.action_menu_index];
                self.mode = Mode::Browsing;
                self.execute(action);
            }
            KeyCode::Char('1') => self.choose_menu_action(0),
            KeyCode::Char('2') => self.choose_menu_action(1),
            KeyCode::Char('3') => self.choose_menu_action(2),
            KeyCode::Char('4') => self.choose_menu_action(3),
            KeyCode::Char('5') => self.choose_menu_action(4),
            KeyCode::Char('6') => self.choose_menu_action(5),
            KeyCode::Char('7') => self.choose_menu_action(6),
            KeyCode::Char('8') => self.choose_menu_action(7),
            _ => {}
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent, pending: PendingAction) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                self.mode = Mode::Browsing;
                self.perform_pending(pending);
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.mode = Mode::Browsing;
                self.log("Canceled");
            }
            _ => {}
        }
    }

    fn handle_logs_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('l') | KeyCode::Char('q') => self.mode = Mode::Browsing,
            _ => {}
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.mode = Mode::Browsing,
            _ => {}
        }
    }

    fn choose_menu_action(&mut self, index: usize) {
        if let Some(action) = ACTIONS.get(index).copied() {
            self.mode = Mode::Browsing;
            self.execute(action);
        }
    }

    fn open_action_menu(&mut self) {
        if self.selected_vm().is_some() {
            self.mode = Mode::ActionMenu;
        } else {
            self.log("No VM selected");
        }
    }

    fn select_previous(&mut self) {
        if self.vms.is_empty() {
            self.selected = 0;
        } else if self.selected == 0 {
            self.selected = self.vms.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn select_next(&mut self) {
        if self.vms.is_empty() {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1) % self.vms.len();
        }
    }

    fn execute(&mut self, action: Action) {
        if action == Action::Refresh {
            self.refresh();
            return;
        }

        let Some(vm) = self.selected_vm().cloned() else {
            self.log("No VM selected");
            return;
        };

        let pending = PendingAction {
            vmid: vm.vmid,
            action,
        };

        if self.needs_confirmation(&vm, action) {
            self.mode = Mode::Confirm {
                pending,
                message: confirmation_message(&vm, action),
            };
            return;
        }

        self.perform_pending(pending);
    }

    fn needs_confirmation(&self, vm: &Vm, action: Action) -> bool {
        match action {
            Action::Shutdown | Action::Reboot | Action::Stop | Action::Reset => {
                self.config.ui.confirm_destructive_actions
            }
            Action::Attach(_) => vm.status != "running",
            Action::Start | Action::Refresh => false,
        }
    }

    fn perform_pending(&mut self, pending: PendingAction) {
        let result = match pending.action {
            Action::Refresh => {
                self.refresh();
                return;
            }
            Action::Start => self.run_power_action(pending.vmid, PowerAction::Start),
            Action::Shutdown => self.run_power_action(pending.vmid, PowerAction::Shutdown),
            Action::Reboot => self.run_power_action(pending.vmid, PowerAction::Reboot),
            Action::Stop => self.run_power_action(pending.vmid, PowerAction::Stop),
            Action::Reset => self.run_power_action(pending.vmid, PowerAction::Reset),
            Action::Attach(protocol) => self.attach(pending.vmid, protocol),
        };

        if let Err(error) = result {
            self.log(format!("ERROR: {error:#}"));
        }
    }

    fn run_power_action(&mut self, vmid: u32, action: PowerAction) -> anyhow::Result<()> {
        self.log(format!("Running {} for VM {vmid}", action_label(action)));
        self.proxmox.power_action(vmid, action)?;
        self.poll_after_power_action(vmid, action);
        Ok(())
    }

    fn poll_after_power_action(&mut self, vmid: u32, action: PowerAction) {
        let desired = match action {
            PowerAction::Start | PowerAction::Reboot | PowerAction::Reset => "running",
            PowerAction::Shutdown | PowerAction::Stop => "stopped",
        };

        match self
            .proxmox
            .wait_for_status(vmid, desired, Duration::from_secs(5))
        {
            Ok(Some(vm)) => {
                self.refresh();
                self.log(format!("VM {} is {}", vm.vmid, vm.status));
            }
            Ok(None) => {
                self.refresh();
                self.log(format!("VM {vmid} was not found after action"));
            }
            Err(error) => {
                self.log(format!("Refresh after action failed: {error:#}"));
            }
        }
    }

    fn attach(&mut self, vmid: u32, protocol: Protocol) -> anyhow::Result<()> {
        let mut vm = self
            .find_vm(vmid)
            .ok_or_else(|| anyhow::anyhow!("VM {vmid} is no longer in the list"))?;

        if vm.status != "running" {
            self.log(format!("Starting VM {vmid} before attach"));
            self.proxmox.power_action(vmid, PowerAction::Start)?;
            if let Some(updated) =
                self.proxmox
                    .wait_for_status(vmid, "running", Duration::from_secs(30))?
            {
                vm = updated;
            }
            self.refresh();

            if vm.status != "running" {
                anyhow::bail!("VM {vmid} did not reach running state");
            }
        }

        match protocol {
            Protocol::Auto => self.attach_auto(&vm),
            Protocol::Spice => self.attach_spice(&vm),
            Protocol::Vnc => self.attach_vnc(&vm),
        }
    }

    fn attach_auto(&mut self, vm: &Vm) -> anyhow::Result<()> {
        match viewer::attach_spice(&self.config, self.proxmox.runner(), self.proxmox.node(), vm) {
            Ok(session) => {
                self.log_session(session);
                Ok(())
            }
            Err(spice_error) => {
                self.log(format!("SPICE failed, trying VNC: {spice_error:#}"));
                self.attach_vnc(vm)
            }
        }
    }

    fn attach_spice(&mut self, vm: &Vm) -> anyhow::Result<()> {
        let session =
            viewer::attach_spice(&self.config, self.proxmox.runner(), self.proxmox.node(), vm)?;
        self.log_session(session);
        Ok(())
    }

    fn attach_vnc(&mut self, vm: &Vm) -> anyhow::Result<()> {
        let session =
            viewer::attach_vnc(&self.config, self.proxmox.runner(), self.proxmox.node(), vm)?;
        self.log_session(session);
        Ok(())
    }

    fn log_session(&mut self, session: viewer::ViewerSession) {
        self.log(format!(
            "{} viewer started for VM {} (pid {}, temp files: {})",
            session.protocol.label(),
            session.vmid,
            session.process_id,
            session.temp_files.len()
        ));
    }

    fn refresh(&mut self) {
        match self.proxmox.list_vms() {
            Ok(vms) => {
                self.vms = vms;
                if self.selected >= self.vms.len() {
                    self.selected = self.vms.len().saturating_sub(1);
                }
                self.last_refresh = Instant::now();
                self.log(format!("Refreshed {} VM(s)", self.vms.len()));
            }
            Err(error) => {
                self.last_refresh = Instant::now();
                self.log(format!("Refresh failed: {error:#}"));
            }
        }
    }

    fn find_vm(&self, vmid: u32) -> Option<Vm> {
        self.vms.iter().find(|vm| vm.vmid == vmid).cloned()
    }
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Attach(Protocol::Auto) => "Attach Auto",
            Self::Attach(Protocol::Spice) => "Attach SPICE",
            Self::Attach(Protocol::Vnc) => "Attach VNC",
            Self::Start => "Start",
            Self::Shutdown => "Shutdown",
            Self::Reboot => "Reboot",
            Self::Stop => "Stop",
            Self::Reset => "Reset",
            Self::Refresh => "Refresh",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Attach(Protocol::Auto) => "Try SPICE first, then VNC",
            Self::Attach(Protocol::Spice) => "Create .vv and open remote-viewer",
            Self::Attach(Protocol::Vnc) => "Start vncproxy and open Remmina",
            Self::Start => "qm start",
            Self::Shutdown => "qm shutdown",
            Self::Reboot => "qm reboot",
            Self::Stop => "qm stop",
            Self::Reset => "qm reset",
            Self::Refresh => "qm list",
        }
    }
}

fn confirmation_message(vm: &Vm, action: Action) -> String {
    match action {
        Action::Attach(protocol) => format!(
            "VM {} {} is {}. Start it and attach via {}?",
            vm.vmid,
            vm.name,
            vm.status,
            protocol.label()
        ),
        _ => format!("Run {} for VM {} {}?", action.label(), vm.vmid, vm.name),
    }
}

fn action_label(action: PowerAction) -> &'static str {
    match action {
        PowerAction::Start => "start",
        PowerAction::Shutdown => "shutdown",
        PowerAction::Reboot => "reboot",
        PowerAction::Stop => "stop",
        PowerAction::Reset => "reset",
    }
}
