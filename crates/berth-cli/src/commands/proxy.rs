//! Command handler for `berth proxy`.

use berth_registry::config::InstalledServer;
use berth_registry::Registry;
use berth_runtime::{ProcessSpec, RuntimeManager};
use colored::Colorize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{self, Command, Stdio};

use crate::paths;
use crate::permission_filter::{
    filter_env_map, load_permission_overrides, validate_network_permissions,
    NETWORK_PERMISSION_DENIED_PREFIX,
};

/// Executes the `berth proxy` command.
pub fn execute(server: &str) {
    let config_path = match paths::server_config_path(server) {
        Some(p) => p,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    if !config_path.exists() {
        eprintln!(
            "{} Server {} is not installed.",
            "✗".red().bold(),
            server.cyan()
        );
        process::exit(1);
    }

    let installed = match read_installed(&config_path) {
        Ok(i) => i,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    let missing = missing_required_keys(&installed);
    if !missing.is_empty() {
        eprintln!(
            "{} Cannot proxy {}. Missing required config: {}",
            "✗".red().bold(),
            server.cyan(),
            missing.join(", ").yellow()
        );
        eprintln!(
            "  Run {} to configure.",
            format!("berth config {server} --set <key>=<value>").bold()
        );
        process::exit(1);
    }

    let registry = Registry::from_seed();
    let spec = match build_process_spec(server, &installed, &registry) {
        Ok(spec) => spec,
        Err(msg) => {
            if msg.starts_with(NETWORK_PERMISSION_DENIED_PREFIX) {
                let berth_home = match paths::berth_home() {
                    Some(h) => h,
                    None => {
                        eprintln!("{} Could not determine home directory.", "✗".red().bold());
                        process::exit(1);
                    }
                };
                let runtime = RuntimeManager::new(berth_home);
                let _ = runtime.record_audit_event(
                    server,
                    "permission-network-denied",
                    None,
                    Some(&installed.runtime.command),
                    Some(&installed.runtime.args),
                );
            }
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    let berth_home = match paths::berth_home() {
        Some(h) => h,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    let runtime = RuntimeManager::new(berth_home);

    let mut child = match Command::new(&spec.command)
        .args(&spec.args)
        .envs(&spec.env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to start proxy process: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    let pid = child.id();
    let _ = runtime.record_audit_event(
        server,
        "proxy-start",
        Some(pid),
        Some(&spec.command),
        Some(&spec.args),
    );

    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => {
            let _ = runtime.record_audit_event(
                server,
                "proxy-error",
                Some(pid),
                Some(&spec.command),
                Some(&spec.args),
            );
            eprintln!(
                "{} Failed while proxying {}: {}",
                "✗".red().bold(),
                server.cyan(),
                e
            );
            process::exit(1);
        }
    };

    let _ = runtime.record_audit_event(
        server,
        "proxy-end",
        Some(pid),
        Some(&spec.command),
        Some(&spec.args),
    );

    match status.code() {
        Some(code) => process::exit(code),
        None => process::exit(1),
    }
}

/// Reads and parses an installed server config file.
fn read_installed(path: &Path) -> Result<InstalledServer, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read config: {e}"))?;
    toml::from_str::<InstalledServer>(&content).map_err(|e| format!("Failed to parse config: {e}"))
}

/// Returns required config keys that are missing or empty.
fn missing_required_keys(installed: &InstalledServer) -> Vec<String> {
    installed
        .config_meta
        .required_keys
        .iter()
        .filter(|k| installed.config.get(*k).is_none_or(|v| v.trim().is_empty()))
        .cloned()
        .collect()
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
                        env.insert(env_var.clone(), value.clone());
                    }
                }
            }
        }
    }

    let overrides = load_permission_overrides(name)?;
    validate_network_permissions(name, &installed.permissions.network, &overrides)?;
    filter_env_map(&mut env, &installed.permissions.env, &overrides);

    Ok(ProcessSpec {
        command: installed.runtime.command.clone(),
        args: installed.runtime.args.clone(),
        env,
        auto_restart: None,
    })
}
