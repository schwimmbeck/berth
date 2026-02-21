//! Command handler for `berth status`.

use colored::Colorize;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::process;
use std::process::Command;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;
use berth_runtime::{ProcessSpec, RuntimeManager, ServerStatus};

use crate::paths;
use crate::permission_filter::{
    filter_env_map, load_permission_overrides, validate_network_permissions,
};
use crate::runtime_policy::parse_runtime_policy;
use crate::sandbox_policy::parse_sandbox_policy;
use crate::sandbox_runtime::apply_sandbox_runtime;
use crate::secrets::resolve_config_value;

#[derive(Debug, Deserialize)]
struct RuntimeStateSnapshot {
    #[serde(default)]
    pid: Option<u32>,
}

/// Executes the `berth status` command.
pub fn execute() {
    let servers_dir = match paths::berth_servers_dir() {
        Some(d) => d,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    if !servers_dir.exists() {
        println!("{} No servers installed.", "!".yellow().bold());
        println!("  Run {} to install one.", "berth install <server>".bold());
        return;
    }

    let mut entries: Vec<_> = match fs::read_dir(&servers_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
            .collect(),
        Err(e) => {
            eprintln!(
                "{} Failed to read installed servers directory: {}",
                "✗".red().bold(),
                e
            );
            process::exit(1);
        }
    };

    if entries.is_empty() {
        println!("{} No servers installed.", "!".yellow().bold());
        println!("  Run {} to install one.", "berth install <server>".bold());
        return;
    }

    entries.sort_by_key(|e| e.path());

    let berth_home = match paths::berth_home() {
        Some(h) => h,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    let runtime = RuntimeManager::new(berth_home);
    let registry = Registry::from_seed();

    println!("{} MCP server status:\n", "✓".green().bold());
    println!(
        "  {:<20} {:<12} {:<12} {:<8} {:<12}",
        "NAME".bold(),
        "VERSION".bold(),
        "STATUS".bold(),
        "PID".bold(),
        "MEMORY".bold(),
    );
    println!("  {}", "─".repeat(72));

    let mut had_error = false;
    for entry in &entries {
        let path = entry.path();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let installed = match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<InstalledServer>(&content) {
                Ok(installed) => installed,
                Err(_) => {
                    had_error = true;
                    println!(
                        "  {:<20} {:<12} {:<12} {:<8} {:<12}",
                        name.cyan(),
                        "?",
                        "error".red(),
                        "-",
                        "-"
                    );
                    continue;
                }
            },
            Err(_) => {
                had_error = true;
                println!(
                    "  {:<20} {:<12} {:<12} {:<8} {:<12}",
                    name.cyan(),
                    "?",
                    "error".red(),
                    "-",
                    "-"
                );
                continue;
            }
        };
        let version = installed.server.version.clone();

        let spec = match build_process_spec(&name, &installed, &registry) {
            Ok(spec) => Some(spec),
            Err(_) => {
                had_error = true;
                None
            }
        };

        let (status_display, pid_display, memory_display) = match spec.as_ref().map_or_else(
            || runtime.status(&name),
            |s| runtime.status_with_spec(&name, Some(s)),
        ) {
            Ok(ServerStatus::Running) => {
                let pid = read_runtime_pid(&name);
                let pid_display = pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "-".to_string());
                let memory_display = pid
                    .and_then(resident_memory_kib)
                    .map(|kib| format!("{kib} KiB"))
                    .unwrap_or_else(|| "-".to_string());
                ("running".green().to_string(), pid_display, memory_display)
            }
            Ok(ServerStatus::Stopped) => (
                "stopped".dimmed().to_string(),
                "-".to_string(),
                "-".to_string(),
            ),
            Err(_) => {
                had_error = true;
                ("error".red().to_string(), "-".to_string(), "-".to_string())
            }
        };

        println!(
            "  {:<20} {:<12} {:<12} {:<8} {:<12}",
            name.cyan(),
            version,
            status_display,
            pid_display,
            memory_display
        );
    }
    println!();

    if had_error {
        process::exit(1);
    }
}

/// Builds a runtime process spec from installed metadata and config values.
fn build_process_spec(
    name: &str,
    installed: &InstalledServer,
    registry: &Registry,
) -> Result<ProcessSpec, String> {
    let mut env = BTreeMap::new();

    if let Some(meta) = registry.get(name) {
        for field in meta
            .config
            .required
            .iter()
            .chain(meta.config.optional.iter())
        {
            if let Some(env_var) = &field.env {
                if let Some(value) = installed.config.get(&field.key) {
                    if !value.trim().is_empty() {
                        let resolved =
                            resolve_config_value(name, &field.key, value).map_err(|e| {
                                format!(
                                    "Failed to resolve config key `{}` for {}: {e}",
                                    field.key,
                                    name.cyan()
                                )
                            })?;
                        env.insert(env_var.clone(), resolved);
                    }
                }
            }
        }
    }

    let overrides = load_permission_overrides(name)?;
    validate_network_permissions(name, &installed.permissions.network, &overrides)?;
    filter_env_map(&mut env, &installed.permissions.env, &overrides);
    let mut policy = parse_runtime_policy(&installed.config)?;
    let sandbox_policy = parse_sandbox_policy(&installed.config)?;
    if sandbox_policy.network_deny_all {
        policy.enabled = false;
    }
    let (command, args) = apply_sandbox_runtime(
        &installed.runtime.command,
        &installed.runtime.args,
        &mut env,
        sandbox_policy,
        &installed.permissions.filesystem,
    );

    Ok(ProcessSpec {
        command,
        args,
        env,
        auto_restart: Some(policy),
    })
}

/// Reads the persisted runtime PID for a server, if present.
fn read_runtime_pid(server: &str) -> Option<u32> {
    let berth_home = paths::berth_home()?;
    let state_path = berth_home.join("runtime").join(format!("{server}.toml"));
    if !state_path.exists() {
        return None;
    }
    let content = fs::read_to_string(state_path).ok()?;
    let state: RuntimeStateSnapshot = toml::from_str(&content).ok()?;
    state.pid
}

/// Returns current resident memory (KiB) for a process id, if available.
#[cfg(unix)]
fn resident_memory_kib(pid: u32) -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Returns current resident memory (KiB) for a process id, if available.
#[cfg(windows)]
fn resident_memory_kib(pid: u32) -> Option<u64> {
    let filter = format!("PID eq {pid}");
    let output = Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let decoded = String::from_utf8_lossy(&output.stdout);
    let line = decoded.lines().next().map(str::trim)?;
    if line.starts_with("INFO:") {
        return None;
    }

    let cols: Vec<&str> = line.trim_matches('"').split("\",\"").collect();
    if cols.len() < 5 {
        return None;
    }
    let digits: String = cols[4].chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Returns current resident memory (KiB) for a process id, if available.
#[cfg(not(any(unix, windows)))]
fn resident_memory_kib(_pid: u32) -> Option<u64> {
    None
}
