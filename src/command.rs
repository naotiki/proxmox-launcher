use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};

#[derive(Clone, Debug)]
pub struct CommandRunner {
    log_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CommandResult {
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

#[derive(Clone, Debug)]
struct DetachedLaunch {
    program: String,
    args: Vec<OsString>,
    env_vars: Vec<(String, String)>,
    run_as_user: Option<String>,
    setsid: bool,
}

#[derive(Clone, Debug)]
struct InvokingUser {
    name: String,
    uid: Option<u32>,
    home: Option<PathBuf>,
}

impl CommandRunner {
    pub fn new(log_path: PathBuf) -> Self {
        Self { log_path }
    }

    pub fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<CommandResult> {
        ensure_command(program)?;

        let command = command_vec(program, args);
        let start = Instant::now();
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn `{}`", display_command(&command)))?;

        loop {
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                let result = CommandResult {
                    command,
                    exit_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    duration: start.elapsed(),
                };
                self.log_result(&result, false);

                if output.status.success() {
                    return Ok(result);
                }

                bail!(
                    "command failed: {} (exit {:?}){}",
                    display_command(&result.command),
                    result.exit_code,
                    stderr_suffix(&result.stderr)
                );
            }

            if start.elapsed() >= timeout {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                let result = CommandResult {
                    command,
                    exit_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    duration: start.elapsed(),
                };
                self.log_result(&result, true);
                bail!(
                    "command timed out after {}s: {}{}",
                    timeout.as_secs(),
                    display_command(&result.command),
                    stderr_suffix(&result.stderr)
                );
            }

            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn spawn_detached(
        &self,
        program: &str,
        args: &[OsString],
        env_vars: &[(String, String)],
        run_as_invoking_user: bool,
    ) -> Result<u32> {
        let launch = build_detached_launch(program, args, env_vars, run_as_invoking_user)?;
        let mut command = Command::new(&launch.program);
        command.args(&launch.args);
        if launch.run_as_user.is_none() {
            command.envs(launch.env_vars.iter().map(|(key, value)| (key, value)));
        }

        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn viewer `{program}`"))?;

        self.log_detached(&launch, child.id());
        Ok(child.id())
    }

    fn log_result(&self, result: &CommandResult, timeout: bool) {
        let status = if timeout { "timeout" } else { "done" };
        let stderr = result.stderr.trim();
        let line = format!(
            "{} status={} exit={:?} duration_ms={} cmd=\"{}\" stderr=\"{}\"\n",
            unix_timestamp(),
            status,
            result.exit_code,
            result.duration.as_millis(),
            display_command(&result.command),
            sanitize_log_value(stderr)
        );
        append_log_line(&self.log_path, &line);
    }

    fn log_detached(&self, launch: &DetachedLaunch, pid: u32) {
        let mut command = vec![launch.program.clone()];
        command.extend(
            launch
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().to_string()),
        );
        let env_keys = launch
            .env_vars
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let run_as_user = launch.run_as_user.as_deref().unwrap_or("");
        let line = format!(
            "{} status=spawned pid={} setsid={} run_as_user=\"{}\" env_keys=\"{}\" cmd=\"{}\"\n",
            unix_timestamp(),
            pid,
            launch.setsid,
            sanitize_log_value(run_as_user),
            sanitize_log_value(&env_keys),
            display_command(&command),
        );
        append_log_line(&self.log_path, &line);
    }
}

fn build_detached_launch(
    program: &str,
    args: &[OsString],
    env_vars: &[(String, String)],
    run_as_invoking_user: bool,
) -> Result<DetachedLaunch> {
    let invoking_user = run_as_invoking_user.then(invoking_user).flatten();
    let effective_env = viewer_env(invoking_user.as_ref(), env_vars);
    let setsid = command_exists("setsid");

    if let Some(user) = invoking_user {
        return build_user_launch(program, args, &effective_env, &user, setsid);
    }

    ensure_command(program)?;
    let mut launch_args = Vec::new();
    let launch_program = if setsid {
        launch_args.push(OsString::from(program));
        "setsid".to_string()
    } else {
        program.to_string()
    };
    launch_args.extend(args.iter().cloned());

    Ok(DetachedLaunch {
        program: launch_program,
        args: launch_args,
        env_vars: effective_env,
        run_as_user: None,
        setsid,
    })
}

fn build_user_launch(
    program: &str,
    args: &[OsString],
    env_vars: &[(String, String)],
    user: &InvokingUser,
    setsid: bool,
) -> Result<DetachedLaunch> {
    let switcher = user_switcher()?;
    let env_program = if Path::new("/usr/bin/env").is_file() {
        "/usr/bin/env"
    } else {
        "env"
    };

    let mut launch_args = Vec::new();
    match switcher {
        UserSwitcher::Runuser => {
            launch_args.extend([
                OsString::from("-u"),
                OsString::from(&user.name),
                OsString::from("--"),
                OsString::from(env_program),
            ]);
        }
        UserSwitcher::Sudo => {
            launch_args.extend([
                OsString::from("-u"),
                OsString::from(&user.name),
                OsString::from(env_program),
            ]);
        }
    }

    launch_args.extend(
        env_vars
            .iter()
            .map(|(key, value)| OsString::from(format!("{key}={value}"))),
    );
    if setsid {
        launch_args.push(OsString::from("setsid"));
    }
    launch_args.push(OsString::from(program));
    launch_args.extend(args.iter().cloned());

    Ok(DetachedLaunch {
        program: switcher.program().to_string(),
        args: launch_args,
        env_vars: env_vars.to_vec(),
        run_as_user: Some(user.name.clone()),
        setsid,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UserSwitcher {
    Runuser,
    Sudo,
}

impl UserSwitcher {
    fn program(self) -> &'static str {
        match self {
            Self::Runuser => "runuser",
            Self::Sudo => "sudo",
        }
    }
}

fn user_switcher() -> Result<UserSwitcher> {
    if command_exists("runuser") {
        Ok(UserSwitcher::Runuser)
    } else if command_exists("sudo") {
        Ok(UserSwitcher::Sudo)
    } else {
        bail!("viewer must run as the invoking desktop user, but neither `runuser` nor `sudo` was found")
    }
}

fn invoking_user() -> Option<InvokingUser> {
    let name = env::var("SUDO_USER").ok()?;
    if name.is_empty() || name == "root" {
        return None;
    }

    let uid = env::var("SUDO_UID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let home = passwd_home(&name);

    Some(InvokingUser { name, uid, home })
}

fn viewer_env(
    user: Option<&InvokingUser>,
    configured: &[(String, String)],
) -> Vec<(String, String)> {
    let mut values = BTreeMap::new();

    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "XDG_SESSION_TYPE",
        "DESKTOP_SESSION",
        "PATH",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
    ] {
        if let Some(value) = env::var_os(key) {
            values.insert(key.to_string(), value.to_string_lossy().to_string());
        }
    }

    if !values.contains_key("XDG_RUNTIME_DIR") {
        if let Some(uid) = user.and_then(|user| user.uid) {
            values.insert("XDG_RUNTIME_DIR".to_string(), format!("/run/user/{uid}"));
        } else if let Some(value) = env::var_os("XDG_RUNTIME_DIR") {
            values.insert(
                "XDG_RUNTIME_DIR".to_string(),
                value.to_string_lossy().to_string(),
            );
        }
    }

    if !values.contains_key("DBUS_SESSION_BUS_ADDRESS") {
        if let Some(uid) = user.and_then(|user| user.uid) {
            let bus = PathBuf::from(format!("/run/user/{uid}/bus"));
            if bus.exists() {
                values.insert(
                    "DBUS_SESSION_BUS_ADDRESS".to_string(),
                    format!("unix:path={}", bus.display()),
                );
            }
        }
    }

    if !values.contains_key("XAUTHORITY") {
        if let Some(home) = user.and_then(|user| user.home.as_ref()) {
            let xauthority = home.join(".Xauthority");
            if xauthority.exists() {
                values.insert("XAUTHORITY".to_string(), xauthority.display().to_string());
            }
        }
    }

    for (key, value) in configured {
        values.insert(key.clone(), value.clone());
    }

    values.into_iter().collect()
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

pub fn ensure_command(program: &str) -> Result<()> {
    if command_exists(program) {
        Ok(())
    } else {
        bail!("`{program}` was not found in PATH")
    }
}

pub fn command_exists(program: &str) -> bool {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return is_executable(path);
    }

    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths)
                .map(|dir| dir.join(program))
                .any(|candidate| is_executable(&candidate))
        })
        .unwrap_or(false)
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn command_vec(program: &str, args: &[&str]) -> Vec<String> {
    let mut command = vec![program.to_string()];
    command.extend(args.iter().map(|arg| arg.to_string()));
    command
}

fn display_command(command: &[String]) -> String {
    command.join(" ")
}

fn stderr_suffix(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

fn append_log_line(path: &Path, line: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn sanitize_log_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
