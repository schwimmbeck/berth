//! Synchronous runtime state manager for installed MCP servers.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns crate version for runtime diagnostics/tests.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Runtime status persisted for a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Running,
    Stopped,
}

impl fmt::Display for ServerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerStatus::Running => write!(f, "running"),
            ServerStatus::Stopped => write!(f, "stopped"),
        }
    }
}

/// Result of attempting to start a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    AlreadyRunning,
}

/// Result of attempting to stop a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    Stopped,
    AlreadyStopped,
}

/// Runtime process specification for launching a server.
#[derive(Debug, Clone, Default)]
pub struct ProcessSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeState {
    status: ServerStatus,
    updated_at_epoch_secs: u64,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        RuntimeState {
            status: ServerStatus::Stopped,
            updated_at_epoch_secs: now_epoch_secs(),
            pid: None,
            command: None,
            args: Vec::new(),
        }
    }
}

pub struct RuntimeManager {
    berth_home: PathBuf,
}

impl RuntimeManager {
    /// Creates a manager rooted at a Berth home directory.
    pub fn new<P: Into<PathBuf>>(berth_home: P) -> Self {
        RuntimeManager {
            berth_home: berth_home.into(),
        }
    }

    /// Returns current persisted status for a server.
    pub fn status(&self, server: &str) -> io::Result<ServerStatus> {
        let mut state = self.read_state(server)?;

        if state.status == ServerStatus::Running {
            if let Some(pid) = state.pid {
                if process_is_alive(pid) {
                    return Ok(ServerStatus::Running);
                }
            }

            state.status = ServerStatus::Stopped;
            state.pid = None;
            state.updated_at_epoch_secs = now_epoch_secs();
            self.write_state(server, &state)?;
            self.append_log(server, "EXIT")?;
        }

        Ok(ServerStatus::Stopped)
    }

    /// Starts a server subprocess and records runtime state.
    pub fn start(&self, server: &str, spec: &ProcessSpec) -> io::Result<StartOutcome> {
        if spec.command.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "process command must not be empty",
            ));
        }

        let mut state = self.read_state(server)?;
        if let Some(pid) = state.pid {
            if process_is_alive(pid) {
                state.status = ServerStatus::Running;
                state.updated_at_epoch_secs = now_epoch_secs();
                self.write_state(server, &state)?;
                return Ok(StartOutcome::AlreadyRunning);
            }

            state.status = ServerStatus::Stopped;
            state.pid = None;
        }

        fs::create_dir_all(self.logs_dir())?;
        let log_file = self.open_log_append(server)?;
        let err_file = log_file.try_clone()?;

        let child = Command::new(&spec.command)
            .args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(err_file))
            .spawn()
            .map_err(|e| io::Error::new(e.kind(), format!("failed to spawn process: {e}")))?;
        let pid = child.id();
        drop(child);

        state.status = ServerStatus::Running;
        state.pid = Some(pid);
        state.command = Some(spec.command.clone());
        state.args = spec.args.clone();
        state.updated_at_epoch_secs = now_epoch_secs();
        self.write_state(server, &state)?;
        self.append_log(server, &format!("START pid={pid}"))?;
        Ok(StartOutcome::Started)
    }

    /// Stops a running server subprocess and records runtime state.
    pub fn stop(&self, server: &str) -> io::Result<StopOutcome> {
        let mut state = self.read_state(server)?;
        let mut outcome = StopOutcome::AlreadyStopped;

        if let Some(pid) = state.pid {
            if process_is_alive(pid) {
                terminate_process(pid)?;
                outcome = StopOutcome::Stopped;
            }
        } else if state.status == ServerStatus::Running {
            outcome = StopOutcome::Stopped;
        }

        state.status = ServerStatus::Stopped;
        state.pid = None;
        state.updated_at_epoch_secs = now_epoch_secs();
        self.write_state(server, &state)?;
        self.append_log(server, "STOP")?;
        Ok(outcome)
    }

    /// Restarts a server by stopping then starting with the same process spec.
    pub fn restart(&self, server: &str, spec: &ProcessSpec) -> io::Result<()> {
        let _ = self.stop(server)?;
        let _ = self.start(server, spec)?;
        Ok(())
    }

    /// Returns the last `lines` log lines for a server.
    pub fn tail_logs(&self, server: &str, lines: usize) -> io::Result<Vec<String>> {
        if lines == 0 {
            return Ok(Vec::new());
        }

        let path = self.log_path(server);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path)?;
        let all: Vec<String> = content.lines().map(ToString::to_string).collect();
        if all.len() <= lines {
            return Ok(all);
        }

        Ok(all[all.len() - lines..].to_vec())
    }

    /// Runtime state directory path.
    fn runtime_dir(&self) -> PathBuf {
        self.berth_home.join("runtime")
    }

    /// Runtime log directory path.
    fn logs_dir(&self) -> PathBuf {
        self.berth_home.join("logs")
    }

    /// Per-server state file path.
    fn state_path(&self, server: &str) -> PathBuf {
        self.runtime_dir().join(format!("{server}.toml"))
    }

    /// Per-server log file path.
    fn log_path(&self, server: &str) -> PathBuf {
        self.logs_dir().join(format!("{server}.log"))
    }

    /// Reads persisted state, defaulting to stopped when missing.
    fn read_state(&self, server: &str) -> io::Result<RuntimeState> {
        let path = self.state_path(server);
        if !path.exists() {
            return Ok(RuntimeState::default());
        }

        let content = fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Persists a server runtime state as TOML.
    fn write_state(&self, server: &str, state: &RuntimeState) -> io::Result<()> {
        fs::create_dir_all(self.runtime_dir())?;
        let serialized = toml::to_string_pretty(state)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(self.state_path(server), serialized)
    }

    /// Appends one lifecycle event line to a server log file.
    fn append_log(&self, server: &str, event: &str) -> io::Result<()> {
        let mut file = self.open_log_append(server)?;
        writeln!(file, "[{}] {}", now_epoch_secs(), event)
    }

    /// Opens the server log file in append mode, creating it if needed.
    fn open_log_append(&self, server: &str) -> io::Result<std::fs::File> {
        fs::create_dir_all(self.logs_dir())?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(server))
    }
}

/// Returns current unix timestamp in seconds.
fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Returns whether a process is currently alive.
#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|s| s.success())
}

/// Returns whether a process is currently alive.
#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    let output = Command::new("cmd")
        .args(["/C", "tasklist", "/FI", &format!("PID eq {pid}")])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
        }
        _ => false,
    }
}

/// Returns whether a process is currently alive.
#[cfg(not(any(unix, windows)))]
fn process_is_alive(_pid: u32) -> bool {
    false
}

/// Sends a termination signal to a process.
#[cfg(unix)]
fn terminate_process(pid: u32) -> io::Result<()> {
    let status = Command::new("kill").arg(pid.to_string()).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("failed to signal process {pid}"),
        ))
    }
}

/// Sends a termination signal to a process.
#[cfg(windows)]
fn terminate_process(pid: u32) -> io::Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("failed to terminate process {pid}"),
        ))
    }
}

/// Sends a termination signal to a process.
#[cfg(not(any(unix, windows)))]
fn terminate_process(_pid: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process termination is not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> (tempfile::TempDir, RuntimeManager) {
        let tmp = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::new(tmp.path().join(".berth"));
        (tmp, manager)
    }

    #[cfg(unix)]
    fn long_running_spec() -> ProcessSpec {
        ProcessSpec {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "sleep 60".to_string()],
            env: BTreeMap::new(),
        }
    }

    #[cfg(windows)]
    fn long_running_spec() -> ProcessSpec {
        ProcessSpec {
            command: "cmd".to_string(),
            args: vec![
                "/C".to_string(),
                "timeout".to_string(),
                "/T".to_string(),
                "60".to_string(),
                "/NOBREAK".to_string(),
            ],
            env: BTreeMap::new(),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn long_running_spec() -> ProcessSpec {
        ProcessSpec {
            command: "unsupported".to_string(),
            args: vec![],
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn version_is_set() {
        assert!(!version().is_empty());
    }

    #[test]
    fn missing_state_defaults_to_stopped() {
        let (_tmp, manager) = manager();
        let status = manager.status("github").unwrap();
        assert_eq!(status, ServerStatus::Stopped);
    }

    #[test]
    fn start_transitions_to_running() {
        let (_tmp, manager) = manager();
        let spec = long_running_spec();
        let outcome = manager.start("github", &spec).unwrap();
        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(manager.status("github").unwrap(), ServerStatus::Running);
        let _ = manager.stop("github");
    }

    #[test]
    fn starting_running_server_reports_already_running() {
        let (_tmp, manager) = manager();
        let spec = long_running_spec();
        manager.start("github", &spec).unwrap();
        let outcome = manager.start("github", &spec).unwrap();
        assert_eq!(outcome, StartOutcome::AlreadyRunning);
        let _ = manager.stop("github");
    }

    #[test]
    fn stop_transitions_to_stopped() {
        let (_tmp, manager) = manager();
        let spec = long_running_spec();
        manager.start("github", &spec).unwrap();
        let outcome = manager.stop("github").unwrap();
        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(manager.status("github").unwrap(), ServerStatus::Stopped);
    }

    #[test]
    fn stopping_stopped_server_reports_already_stopped() {
        let (_tmp, manager) = manager();
        let outcome = manager.stop("github").unwrap();
        assert_eq!(outcome, StopOutcome::AlreadyStopped);
    }

    #[test]
    fn restart_ends_in_running_state() {
        let (_tmp, manager) = manager();
        let spec = long_running_spec();
        manager.restart("github", &spec).unwrap();
        assert_eq!(manager.status("github").unwrap(), ServerStatus::Running);
        let _ = manager.stop("github");
    }

    #[test]
    fn tail_logs_returns_last_lines() {
        let (_tmp, manager) = manager();
        let spec = long_running_spec();
        manager.start("github", &spec).unwrap();
        manager.stop("github").unwrap();
        manager.start("github", &spec).unwrap();
        let _ = manager.stop("github");

        let lines = manager.tail_logs("github", 2).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().any(|l| l.contains("STOP")));
        assert!(lines.iter().any(|l| l.contains("START")));
    }

    #[test]
    fn malformed_state_file_returns_error() {
        let (tmp, manager) = manager();
        let runtime_dir = tmp.path().join(".berth/runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(runtime_dir.join("github.toml"), "not = [valid").unwrap();

        let err = manager.status("github").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
