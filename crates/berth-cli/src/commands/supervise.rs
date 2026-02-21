//! Internal command handler for `berth __supervise`.

use colored::Colorize;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Command, Stdio};

use berth_runtime::{ProcessSpec, RuntimeManager};

use crate::paths;

/// Executes the hidden supervisor process command.
pub fn execute(server: &str) {
    let berth_home = match paths::berth_home() {
        Some(h) => h,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    let mut payload = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut payload) {
        eprintln!("{} Failed to read supervisor spec: {}", "✗".red().bold(), e);
        process::exit(1);
    }
    if payload.trim().is_empty() {
        eprintln!("{} Missing supervisor spec payload.", "✗".red().bold());
        process::exit(1);
    }

    let spec: ProcessSpec = match serde_json::from_str(payload.trim()) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!(
                "{} Invalid supervisor spec payload: {}",
                "✗".red().bold(),
                e
            );
            process::exit(1);
        }
    };

    let runtime = RuntimeManager::new(berth_home);
    if let Err(e) = runtime.run_supervisor(server, &spec) {
        eprintln!(
            "{} Supervisor loop failed for {}: {}",
            "✗".red().bold(),
            server.cyan(),
            e
        );
        process::exit(1);
    }
}

/// Spawns a detached supervisor process and sends it the process spec over stdin.
pub fn spawn_detached(server: &str, spec: &ProcessSpec, berth_home: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("failed to locate current exe: {e}"))?;
    let payload =
        serde_json::to_string(spec).map_err(|e| format!("failed to serialize spec: {e}"))?;

    let mut child = Command::new(exe)
        .arg("__supervise")
        .arg(server)
        .env("BERTH_HOME", berth_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn supervisor: {e}"))?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err("failed to open supervisor stdin".to_string());
    };

    stdin
        .write_all(payload.as_bytes())
        .map_err(|e| format!("failed to write supervisor spec: {e}"))?;
    drop(stdin);
    Ok(())
}
