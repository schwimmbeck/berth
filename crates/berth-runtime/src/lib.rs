// SPDX-License-Identifier: Apache-2.0

//! Synchronous runtime state manager for installed MCP servers.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub auto_restart: Option<AutoRestartPolicy>,
}

/// Auto-restart policy applied to supervised server processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoRestartPolicy {
    pub enabled: bool,
    pub max_restarts: u32,
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
    #[serde(default)]
    auto_restart_enabled: bool,
    #[serde(default)]
    max_restarts: u32,
    #[serde(default)]
    restart_attempts: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditEvent {
    timestamp_epoch_secs: u64,
    server: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Vec<String>>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        RuntimeState {
            status: ServerStatus::Stopped,
            updated_at_epoch_secs: now_epoch_secs(),
            pid: None,
            command: None,
            args: Vec::new(),
            auto_restart_enabled: false,
            max_restarts: 0,
            restart_attempts: 0,
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
        self.status_with_spec(server, None)
    }

    /// Returns current persisted status for a server with optional restart spec.
    pub fn status_with_spec(
        &self,
        server: &str,
        spec: Option<&ProcessSpec>,
    ) -> io::Result<ServerStatus> {
        let mut state = self.read_state(server)?;

        if state.status == ServerStatus::Running {
            let old_pid = state.pid;
            let old_command = state.command.clone();
            let old_args = state.args.clone();

            if let Some(pid) = state.pid {
                if process_is_alive(pid) {
                    return Ok(ServerStatus::Running);
                }
            }

            let expects_external_supervisor = spec
                .and_then(|s| s.auto_restart)
                .is_some_and(|policy| policy.enabled)
                && !state.auto_restart_enabled;
            if expects_external_supervisor
                && self.wait_for_supervisor_replacement(server, old_pid)?
            {
                return Ok(ServerStatus::Running);
            }

            // Record that a previously running process exited.
            state.status = ServerStatus::Stopped;
            state.pid = None;
            state.updated_at_epoch_secs = now_epoch_secs();
            self.write_state(server, &state)?;
            self.append_log(server, "EXIT")?;
            self.append_audit_event(AuditEvent {
                timestamp_epoch_secs: now_epoch_secs(),
                server: server.to_string(),
                action: "exit".to_string(),
                pid: old_pid,
                command: old_command,
                args: if old_args.is_empty() {
                    None
                } else {
                    Some(old_args)
                },
            })?;

            // Attempt bounded auto-restart when policy is enabled.
            if state.auto_restart_enabled && state.restart_attempts < state.max_restarts {
                if let Some(spec) = spec {
                    let child = Command::new(&spec.command)
                        .args(&spec.args)
                        .envs(&spec.env)
                        .stdin(Stdio::null())
                        .stdout(Stdio::from(self.open_log_append(server)?))
                        .stderr(Stdio::from(self.open_log_append(server)?))
                        .spawn()
                        .map_err(|e| {
                            io::Error::new(e.kind(), format!("failed to spawn process: {e}"))
                        })?;
                    let pid = child.id();
                    drop(child);

                    state.status = ServerStatus::Running;
                    state.pid = Some(pid);
                    state.command = Some(spec.command.clone());
                    state.args = spec.args.clone();
                    state.restart_attempts += 1;
                    state.updated_at_epoch_secs = now_epoch_secs();
                    self.write_state(server, &state)?;
                    self.append_log(
                        server,
                        &format!(
                            "AUTO_RESTART pid={pid} attempt={}/{}",
                            state.restart_attempts, state.max_restarts
                        ),
                    )?;
                    self.append_audit_event(AuditEvent {
                        timestamp_epoch_secs: now_epoch_secs(),
                        server: server.to_string(),
                        action: "auto-restart".to_string(),
                        pid: Some(pid),
                        command: Some(spec.command.clone()),
                        args: if spec.args.is_empty() {
                            None
                        } else {
                            Some(spec.args.clone())
                        },
                    })?;
                    return Ok(ServerStatus::Running);
                }
            }
        }

        Ok(ServerStatus::Stopped)
    }

    /// Waits briefly for an external supervisor to replace a dead pid in state.
    fn wait_for_supervisor_replacement(
        &self,
        server: &str,
        old_pid: Option<u32>,
    ) -> io::Result<bool> {
        for _ in 0..10 {
            thread::sleep(Duration::from_millis(50));
            let state = self.read_state(server)?;
            if state.status != ServerStatus::Running {
                return Ok(false);
            }
            if let Some(pid) = state.pid {
                if Some(pid) != old_pid && process_is_alive(pid) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
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
        state.auto_restart_enabled = spec.auto_restart.map(|p| p.enabled).unwrap_or(false);
        state.max_restarts = spec.auto_restart.map(|p| p.max_restarts).unwrap_or(0);
        state.restart_attempts = 0;
        state.updated_at_epoch_secs = now_epoch_secs();
        self.write_state(server, &state)?;
        self.append_log(server, &format!("START pid={pid}"))?;
        self.append_audit_event(AuditEvent {
            timestamp_epoch_secs: now_epoch_secs(),
            server: server.to_string(),
            action: "start".to_string(),
            pid: Some(pid),
            command: Some(spec.command.clone()),
            args: if spec.args.is_empty() {
                None
            } else {
                Some(spec.args.clone())
            },
        })?;
        Ok(StartOutcome::Started)
    }

    /// Stops a running server subprocess and records runtime state.
    pub fn stop(&self, server: &str) -> io::Result<StopOutcome> {
        let mut state = self.read_state(server)?;
        let old_pid = state.pid;
        let old_command = state.command.clone();
        let old_args = state.args.clone();
        let mut outcome = StopOutcome::AlreadyStopped;
        let pid_to_stop = state.pid.filter(|pid| process_is_alive(*pid));

        if pid_to_stop.is_some() || state.status == ServerStatus::Running {
            outcome = StopOutcome::Stopped;
        }

        // Mark stopped before signaling so a background supervisor can observe intent and exit.
        state.status = ServerStatus::Stopped;
        state.pid = None;
        state.restart_attempts = 0;
        state.updated_at_epoch_secs = now_epoch_secs();
        self.write_state(server, &state)?;
        self.append_log(server, "STOP")?;

        if let Some(pid) = pid_to_stop {
            terminate_process(pid)?;
        }

        // Close a narrow race where a supervisor could spawn a replacement pid concurrently.
        for _ in 0..5 {
            let latest = self.read_state(server)?;
            let Some(pid) = latest.pid else {
                break;
            };
            if !process_is_alive(pid) {
                break;
            }
            terminate_process(pid)?;
            let mut reset = latest;
            reset.status = ServerStatus::Stopped;
            reset.pid = None;
            reset.restart_attempts = 0;
            reset.updated_at_epoch_secs = now_epoch_secs();
            self.write_state(server, &reset)?;
            thread::sleep(Duration::from_millis(20));
        }

        if outcome == StopOutcome::Stopped {
            self.append_audit_event(AuditEvent {
                timestamp_epoch_secs: now_epoch_secs(),
                server: server.to_string(),
                action: "stop".to_string(),
                pid: old_pid,
                command: old_command,
                args: if old_args.is_empty() {
                    None
                } else {
                    Some(old_args)
                },
            })?;
        }
        Ok(outcome)
    }

    /// Restarts a server by stopping then starting with the same process spec.
    pub fn restart(&self, server: &str, spec: &ProcessSpec) -> io::Result<()> {
        let _ = self.stop(server)?;
        let _ = self.start(server, spec)?;
        let state = self.read_state(server)?;
        self.append_audit_event(AuditEvent {
            timestamp_epoch_secs: now_epoch_secs(),
            server: server.to_string(),
            action: "restart".to_string(),
            pid: state.pid,
            command: state.command,
            args: if state.args.is_empty() {
                None
            } else {
                Some(state.args)
            },
        })?;
        Ok(())
    }

    /// Runs a tokio-backed supervision loop for one server until stopped.
    pub fn run_supervisor(&self, server: &str, spec: &ProcessSpec) -> io::Result<()> {
        let policy = match spec.auto_restart {
            Some(policy) if policy.enabled => policy,
            _ => return Ok(()),
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|e| io::Error::other(format!("failed to build tokio runtime: {e}")))?;

        runtime.block_on(self.run_supervisor_loop(server, spec, policy))
    }

    /// Async supervision loop that monitors pid transitions and performs bounded restarts.
    async fn run_supervisor_loop(
        &self,
        server: &str,
        spec: &ProcessSpec,
        policy: AutoRestartPolicy,
    ) -> io::Result<()> {
        let poll_interval = Duration::from_millis(100);
        let mut restart_attempts = self.read_state(server)?.restart_attempts;

        loop {
            let state = self.read_state(server)?;
            if state.status != ServerStatus::Running {
                return Ok(());
            }

            let monitored_pid = match state.pid {
                Some(pid) => pid,
                None => {
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }
            };

            loop {
                if !process_is_alive(monitored_pid) {
                    break;
                }
                tokio::time::sleep(poll_interval).await;
                let latest = self.read_state(server)?;
                if latest.status != ServerStatus::Running {
                    return Ok(());
                }
                if latest.pid != Some(monitored_pid) {
                    // Another process took ownership; this supervisor exits.
                    return Ok(());
                }
            }

            let state_after_exit = self.read_state(server)?;
            if state_after_exit.status != ServerStatus::Running {
                return Ok(());
            }
            if state_after_exit.pid != Some(monitored_pid) {
                return Ok(());
            }

            self.append_log(server, "EXIT")?;
            self.append_audit_event(AuditEvent {
                timestamp_epoch_secs: now_epoch_secs(),
                server: server.to_string(),
                action: "exit".to_string(),
                pid: Some(monitored_pid),
                command: state_after_exit.command.clone(),
                args: if state_after_exit.args.is_empty() {
                    None
                } else {
                    Some(state_after_exit.args.clone())
                },
            })?;

            if restart_attempts >= policy.max_restarts {
                let mut stopped_state = state_after_exit;
                stopped_state.status = ServerStatus::Stopped;
                stopped_state.pid = None;
                stopped_state.updated_at_epoch_secs = now_epoch_secs();
                stopped_state.restart_attempts = restart_attempts;
                self.write_state(server, &stopped_state)?;
                return Ok(());
            }

            let control_state = self.read_state(server)?;
            if control_state.status != ServerStatus::Running
                || control_state.pid != Some(monitored_pid)
            {
                return Ok(());
            }

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

            // Stop could have raced with this spawn; terminate immediately if so.
            if self.read_state(server)?.status != ServerStatus::Running {
                let _ = terminate_process(pid);
                return Ok(());
            }

            restart_attempts += 1;
            let mut restarted_state = self.read_state(server)?;
            restarted_state.status = ServerStatus::Running;
            restarted_state.pid = Some(pid);
            restarted_state.command = Some(spec.command.clone());
            restarted_state.args = spec.args.clone();
            restarted_state.updated_at_epoch_secs = now_epoch_secs();
            restarted_state.restart_attempts = restart_attempts;
            self.write_state(server, &restarted_state)?;
            self.append_log(
                server,
                &format!(
                    "AUTO_RESTART pid={pid} attempt={}/{}",
                    restart_attempts, policy.max_restarts
                ),
            )?;
            self.append_audit_event(AuditEvent {
                timestamp_epoch_secs: now_epoch_secs(),
                server: server.to_string(),
                action: "auto-restart".to_string(),
                pid: Some(pid),
                command: Some(spec.command.clone()),
                args: if spec.args.is_empty() {
                    None
                } else {
                    Some(spec.args.clone())
                },
            })?;
        }
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

    /// Appends a custom audit event for non-lifecycle runtime actions.
    pub fn record_audit_event(
        &self,
        server: &str,
        action: &str,
        pid: Option<u32>,
        command: Option<&str>,
        args: Option<&[String]>,
    ) -> io::Result<()> {
        if action.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audit action must not be empty",
            ));
        }

        self.append_audit_event(AuditEvent {
            timestamp_epoch_secs: now_epoch_secs(),
            server: server.to_string(),
            action: action.to_string(),
            pid,
            command: command.map(ToString::to_string),
            args: args.filter(|v| !v.is_empty()).map(|v| v.to_vec()),
        })
    }

    /// Runtime state directory path.
    fn runtime_dir(&self) -> PathBuf {
        self.berth_home.join("runtime")
    }

    /// Runtime log directory path.
    fn logs_dir(&self) -> PathBuf {
        self.berth_home.join("logs")
    }

    /// Audit log directory path.
    fn audit_dir(&self) -> PathBuf {
        self.berth_home.join("audit")
    }

    /// Per-server state file path.
    fn state_path(&self, server: &str) -> PathBuf {
        self.runtime_dir().join(format!("{server}.toml"))
    }

    /// Per-server log file path.
    fn log_path(&self, server: &str) -> PathBuf {
        self.logs_dir().join(format!("{server}.log"))
    }

    /// JSONL audit log file path.
    fn audit_log_path(&self) -> PathBuf {
        self.audit_dir().join("audit.jsonl")
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

    /// Appends one audit event as JSONL.
    fn append_audit_event(&self, event: AuditEvent) -> io::Result<()> {
        fs::create_dir_all(self.audit_dir())?;
        let json = serde_json::to_string(&event)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.audit_log_path())?;
        writeln!(file, "{json}")
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
    let pid_str = pid.to_string();
    if let Ok(out) = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid_str])
        .output()
    {
        if !out.status.success() {
            return false;
        }
        let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if stat.is_empty() {
            return false;
        }
        // Zombie processes are dead for supervision purposes.
        if stat.starts_with('Z') {
            return false;
        }
        return true;
    }

    Command::new("kill")
        .arg("-0")
        .arg(&pid_str)
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
    let pid_str = pid.to_string();
    let status = Command::new("kill").arg(&pid_str).status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("failed to signal process {pid}"),
        ));
    }

    if wait_for_process_exit(pid, 50, Duration::from_millis(20)) {
        return Ok(());
    }

    // Escalate if the process does not exit after TERM.
    let kill_status = Command::new("kill").args(["-9", &pid_str]).status()?;
    if kill_status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("failed to force terminate process {pid}"),
        ))
    }
}

/// Sends a termination signal to a process.
#[cfg(windows)]
fn terminate_process(pid: u32) -> io::Result<()> {
    // Try graceful shutdown first, then force-kill if needed.
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("failed to terminate process {pid}"),
        ));
    }

    if wait_for_process_exit(pid, 50, Duration::from_millis(20)) {
        return Ok(());
    }

    let force_status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    if force_status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("failed to force terminate process {pid}"),
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

/// Waits for a process to exit, checking liveness repeatedly.
fn wait_for_process_exit(pid: u32, attempts: u32, interval: Duration) -> bool {
    for _ in 0..attempts {
        if !process_is_alive(pid) {
            return true;
        }
        thread::sleep(interval);
    }
    !process_is_alive(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn manager() -> (tempfile::TempDir, RuntimeManager) {
        let tmp = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::new(tmp.path().join(".berth"));
        (tmp, manager)
    }

    fn wait_until_process_exits(manager: &RuntimeManager, server: &str) {
        for _ in 0..100 {
            let state = manager.read_state(server).unwrap();
            match state.pid {
                Some(pid) if process_is_alive(pid) => thread::sleep(Duration::from_millis(20)),
                _ => return,
            }
        }
    }

    #[cfg(unix)]
    fn long_running_spec() -> ProcessSpec {
        ProcessSpec {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "sleep 60".to_string()],
            env: BTreeMap::new(),
            auto_restart: None,
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
            auto_restart: None,
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn long_running_spec() -> ProcessSpec {
        ProcessSpec {
            command: "unsupported".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            auto_restart: None,
        }
    }

    #[cfg(unix)]
    fn ignores_term_spec() -> ProcessSpec {
        ProcessSpec {
            command: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "trap '' TERM; while true; do sleep 1; done".to_string(),
            ],
            env: BTreeMap::new(),
            auto_restart: None,
        }
    }

    #[cfg(unix)]
    fn crash_spec_with_policy(max_restarts: u32) -> ProcessSpec {
        ProcessSpec {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 1".to_string()],
            env: BTreeMap::new(),
            auto_restart: Some(AutoRestartPolicy {
                enabled: true,
                max_restarts,
            }),
        }
    }

    #[cfg(windows)]
    fn crash_spec_with_policy(max_restarts: u32) -> ProcessSpec {
        ProcessSpec {
            command: "cmd".to_string(),
            args: vec!["/C".to_string(), "exit /B 1".to_string()],
            env: BTreeMap::new(),
            auto_restart: Some(AutoRestartPolicy {
                enabled: true,
                max_restarts,
            }),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn crash_spec_with_policy(max_restarts: u32) -> ProcessSpec {
        ProcessSpec {
            command: "unsupported".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            auto_restart: Some(AutoRestartPolicy {
                enabled: true,
                max_restarts,
            }),
        }
    }

    #[cfg(unix)]
    fn fail_once_then_run_spec(marker_path: &str, max_restarts: u32) -> ProcessSpec {
        ProcessSpec {
            command: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!(
                    "if [ -f '{marker_path}' ]; then sleep 60; else touch '{marker_path}'; exit 1; fi"
                ),
            ],
            env: BTreeMap::new(),
            auto_restart: Some(AutoRestartPolicy {
                enabled: true,
                max_restarts,
            }),
        }
    }

    #[cfg(windows)]
    fn fail_once_then_run_spec(marker_path: &str, max_restarts: u32) -> ProcessSpec {
        ProcessSpec {
            command: "cmd".to_string(),
            args: vec![
                "/C".to_string(),
                format!(
                    "if exist \"{marker_path}\" (timeout /T 60 /NOBREAK >NUL) else (type nul > \"{marker_path}\" & exit /B 1)"
                ),
            ],
            env: BTreeMap::new(),
            auto_restart: Some(AutoRestartPolicy {
                enabled: true,
                max_restarts,
            }),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn fail_once_then_run_spec(_marker_path: &str, max_restarts: u32) -> ProcessSpec {
        ProcessSpec {
            command: "unsupported".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            auto_restart: Some(AutoRestartPolicy {
                enabled: true,
                max_restarts,
            }),
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

    #[cfg(unix)]
    #[test]
    fn stop_escalates_when_process_ignores_term() {
        let (_tmp, manager) = manager();
        let spec = ignores_term_spec();
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
    fn start_stop_writes_audit_events() {
        let (_tmp, manager) = manager();
        let spec = long_running_spec();
        manager.start("github", &spec).unwrap();
        manager.stop("github").unwrap();

        let audit_path = manager.audit_log_path();
        let content = fs::read_to_string(audit_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines.iter().any(|l| l.contains("\"action\":\"start\"")));
        assert!(lines.iter().any(|l| l.contains("\"action\":\"stop\"")));
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

    #[test]
    fn status_with_spec_auto_restarts_crashed_process() {
        let (_tmp, manager) = manager();
        let crash = crash_spec_with_policy(1);
        let recover = long_running_spec();
        manager.start("github", &crash).unwrap();
        wait_until_process_exits(&manager, "github");

        let status = manager.status_with_spec("github", Some(&recover)).unwrap();
        assert_eq!(status, ServerStatus::Running);

        let audit = fs::read_to_string(manager.audit_log_path()).unwrap();
        assert!(audit.contains("\"action\":\"auto-restart\""));
        let _ = manager.stop("github");
    }

    #[test]
    fn auto_restart_respects_max_restarts_bound() {
        let (_tmp, manager) = manager();
        let crash = crash_spec_with_policy(1);
        manager.start("github", &crash).unwrap();
        wait_until_process_exits(&manager, "github");

        let _ = manager.status_with_spec("github", Some(&crash)).unwrap();
        wait_until_process_exits(&manager, "github");
        let second = manager.status_with_spec("github", Some(&crash)).unwrap();
        assert_eq!(second, ServerStatus::Stopped);

        let audit = fs::read_to_string(manager.audit_log_path()).unwrap();
        let count = audit
            .lines()
            .filter(|l| l.contains("\"action\":\"auto-restart\""))
            .count();
        assert_eq!(count, 1);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn tokio_supervisor_recovers_crash_without_status_polling() {
        let (tmp, manager) = manager();
        let marker = tmp.path().join(".berth/runtime/github.restart-flag");
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        let marker_path = marker.to_string_lossy().to_string();

        let supervisor_spec = fail_once_then_run_spec(&marker_path, 1);
        let mut start_spec = supervisor_spec.clone();
        start_spec.auto_restart = None;
        manager.start("github", &start_spec).unwrap();

        let supervisor_manager = RuntimeManager::new(tmp.path().join(".berth"));
        let thread_spec = supervisor_spec.clone();
        let handle =
            thread::spawn(move || supervisor_manager.run_supervisor("github", &thread_spec));

        let mut recovered = false;
        for _ in 0..200 {
            let state = manager.read_state("github").unwrap();
            if state.restart_attempts == 1 && state.pid.is_some_and(process_is_alive) {
                recovered = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(recovered);

        let _ = manager.stop("github");
        handle.join().unwrap().unwrap();
    }
}
