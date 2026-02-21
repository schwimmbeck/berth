//! Command handler for `berth restart`.

use colored::Colorize;
use std::collections::BTreeMap;
use std::fs;
use std::process;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;
use berth_runtime::{ProcessSpec, RuntimeManager};

use crate::paths;
use crate::permission_filter::{
    filter_env_map, load_permission_overrides, undeclared_network_grants,
    validate_network_permissions, NETWORK_PERMISSION_DENIED_PREFIX,
};
use crate::runtime_policy::parse_runtime_policy;
use crate::sandbox_policy::{parse_sandbox_policy, KEY_SANDBOX_NETWORK};
use crate::sandbox_runtime::apply_sandbox_runtime;

/// Executes the `berth restart` command.
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

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to read config file: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    let installed: InstalledServer = match toml::from_str(&content) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{} Failed to parse config file: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    let missing: Vec<String> = installed
        .config_meta
        .required_keys
        .iter()
        .filter(|k| match installed.config.get(*k) {
            Some(v) => v.trim().is_empty(),
            None => true,
        })
        .cloned()
        .collect();

    if !missing.is_empty() {
        eprintln!(
            "{} Cannot restart {}. Missing required config: {}",
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

    let berth_home = match paths::berth_home() {
        Some(h) => h,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    let runtime = RuntimeManager::new(berth_home);
    let registry = Registry::from_seed();
    let (spec, undeclared_network) = match build_process_spec(server, &installed, &registry) {
        Ok(spec) => spec,
        Err(msg) => {
            if msg.starts_with(NETWORK_PERMISSION_DENIED_PREFIX) {
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
    if !undeclared_network.is_empty() {
        println!(
            "{} {} has undeclared network grant override(s): {} (log-only).",
            "!".yellow().bold(),
            server.cyan(),
            undeclared_network.join(", ")
        );
        let _ = runtime.record_audit_event(
            server,
            "permission-network-warning",
            None,
            Some(&installed.runtime.command),
            Some(&installed.runtime.args),
        );
    }

    if let Err(e) = runtime.restart(server, &spec) {
        eprintln!(
            "{} Failed to restart {}: {}",
            "✗".red().bold(),
            server.cyan(),
            e
        );
        process::exit(1);
    }

    println!("{} Restarted {}.", "✓".green().bold(), server.cyan());
}

/// Builds a runtime process spec from installed metadata and config values.
fn build_process_spec(
    name: &str,
    installed: &InstalledServer,
    registry: &Registry,
) -> Result<(ProcessSpec, Vec<String>), String> {
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
    let undeclared_network = undeclared_network_grants(&installed.permissions.network, &overrides);
    let sandbox_policy = parse_sandbox_policy(&installed.config)?;
    if sandbox_policy.network_deny_all {
        return Err(format!(
            "{NETWORK_PERMISSION_DENIED_PREFIX}Server {} blocked by sandbox policy: set `{KEY_SANDBOX_NETWORK}=inherit` or relax network constraints.",
            name.cyan()
        ));
    }
    filter_env_map(&mut env, &installed.permissions.env, &overrides);
    let policy = parse_runtime_policy(&installed.config)?;
    let (command, args) = apply_sandbox_runtime(
        &installed.runtime.command,
        &installed.runtime.args,
        &mut env,
        sandbox_policy,
    );

    Ok((
        ProcessSpec {
            command,
            args,
            env,
            auto_restart: Some(policy),
        },
        undeclared_network,
    ))
}
