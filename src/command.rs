use std::{
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

    pub fn spawn_detached(&self, program: &str, args: &[OsString]) -> Result<u32> {
        ensure_command(program)?;

        let use_setsid = command_exists("setsid");
        let mut command = if use_setsid {
            let mut command = Command::new("setsid");
            command.arg(program);
            command.args(args);
            command
        } else {
            let mut command = Command::new(program);
            command.args(args);
            command
        };

        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn viewer `{program}`"))?;

        self.log_detached(program, args, child.id(), use_setsid);
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

    fn log_detached(&self, program: &str, args: &[OsString], pid: u32, setsid: bool) {
        let mut command = vec![program.to_string()];
        command.extend(args.iter().map(|arg| arg.to_string_lossy().to_string()));
        let line = format!(
            "{} status=spawned pid={} setsid={} cmd=\"{}\"\n",
            unix_timestamp(),
            pid,
            setsid,
            display_command(&command),
        );
        append_log_line(&self.log_path, &line);
    }
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
