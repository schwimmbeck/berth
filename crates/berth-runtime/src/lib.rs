//! Synchronous runtime state manager for installed MCP servers.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeState {
    status: ServerStatus,
    updated_at_epoch_secs: u64,
}

impl Default for RuntimeState {
    fn default() -> Self {
        RuntimeState {
            status: ServerStatus::Stopped,
            updated_at_epoch_secs: now_epoch_secs(),
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
        Ok(self.read_state(server)?.status)
    }

    /// Marks a server as running and appends a `START` log entry.
    pub fn start(&self, server: &str) -> io::Result<StartOutcome> {
        let mut state = self.read_state(server)?;
        if state.status == ServerStatus::Running {
            return Ok(StartOutcome::AlreadyRunning);
        }

        state.status = ServerStatus::Running;
        state.updated_at_epoch_secs = now_epoch_secs();
        self.write_state(server, &state)?;
        self.append_log(server, "START")?;
        Ok(StartOutcome::Started)
    }

    /// Marks a server as stopped and appends a `STOP` log entry.
    pub fn stop(&self, server: &str) -> io::Result<StopOutcome> {
        let mut state = self.read_state(server)?;
        if state.status == ServerStatus::Stopped {
            return Ok(StopOutcome::AlreadyStopped);
        }

        state.status = ServerStatus::Stopped;
        state.updated_at_epoch_secs = now_epoch_secs();
        self.write_state(server, &state)?;
        self.append_log(server, "STOP")?;
        Ok(StopOutcome::Stopped)
    }

    /// Restarts a server by issuing stop then start transitions.
    pub fn restart(&self, server: &str) -> io::Result<()> {
        let _ = self.stop(server)?;
        let _ = self.start(server)?;
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
        fs::create_dir_all(self.logs_dir())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(server))?;
        writeln!(file, "[{}] {}", now_epoch_secs(), event)
    }
}

/// Returns current unix timestamp in seconds.
fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> (tempfile::TempDir, RuntimeManager) {
        let tmp = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::new(tmp.path().join(".berth"));
        (tmp, manager)
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
        let outcome = manager.start("github").unwrap();
        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(manager.status("github").unwrap(), ServerStatus::Running);
    }

    #[test]
    fn starting_running_server_reports_already_running() {
        let (_tmp, manager) = manager();
        manager.start("github").unwrap();
        let outcome = manager.start("github").unwrap();
        assert_eq!(outcome, StartOutcome::AlreadyRunning);
    }

    #[test]
    fn stop_transitions_to_stopped() {
        let (_tmp, manager) = manager();
        manager.start("github").unwrap();
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
        manager.restart("github").unwrap();
        assert_eq!(manager.status("github").unwrap(), ServerStatus::Running);
    }

    #[test]
    fn tail_logs_returns_last_lines() {
        let (_tmp, manager) = manager();
        manager.start("github").unwrap();
        manager.stop("github").unwrap();
        manager.start("github").unwrap();

        let lines = manager.tail_logs("github", 2).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("STOP"));
        assert!(lines[1].contains("START"));
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
