//! Command handler for `berth start`.

use colored::Colorize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;
use berth_runtime::{ProcessSpec, RuntimeManager, StartOutcome};

use crate::paths;
use crate::permission_filter::{
    filter_env_map, load_permission_overrides, undeclared_network_grants,
    validate_network_permissions, NETWORK_PERMISSION_DENIED_PREFIX,
};
use crate::runtime_policy::parse_runtime_policy;
use crate::sandbox_policy::{parse_sandbox_policy, KEY_SANDBOX_NETWORK};
use crate::sandbox_runtime::apply_sandbox_runtime;
use crate::secrets::resolve_config_value;

/// Executes the `berth start` command.
pub fn execute(server: Option<&str>) {
    let targets = resolve_targets(server);
    let registry = Registry::from_seed();
    let berth_home = match paths::berth_home() {
        Some(h) => h,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    let runtime = RuntimeManager::new(berth_home);

    let mut started = 0usize;
    let mut already_running = 0usize;
    let mut failed = 0usize;

    for name in &targets {
        let config_path = match paths::server_config_path(name) {
            Some(p) => p,
            None => {
                eprintln!("{} Could not determine home directory.", "✗".red().bold());
                failed += 1;
                continue;
            }
        };

        let installed = match read_installed(name, &config_path) {
            Ok(i) => i,
            Err(()) => {
                failed += 1;
                continue;
            }
        };

        let missing = missing_required_keys(&installed);
        if !missing.is_empty() {
            eprintln!(
                "{} Cannot start {}. Missing required config: {}",
                "✗".red().bold(),
                name.cyan(),
                missing.join(", ").yellow()
            );
            eprintln!(
                "  Run {} to configure.",
                format!("berth config {name} --set <key>=<value>").bold()
            );
            failed += 1;
            continue;
        }

        let (spec, undeclared_network) = match build_process_spec(name, &installed, &registry) {
            Ok(spec) => spec,
            Err(msg) => {
                if msg.starts_with(NETWORK_PERMISSION_DENIED_PREFIX) {
                    let _ = runtime.record_audit_event(
                        name,
                        "permission-network-denied",
                        None,
                        Some(&installed.runtime.command),
                        Some(&installed.runtime.args),
                    );
                }
                eprintln!("{} {}", "✗".red().bold(), msg);
                failed += 1;
                continue;
            }
        };
        if !undeclared_network.is_empty() {
            println!(
                "{} {} has undeclared network grant override(s): {} (log-only).",
                "!".yellow().bold(),
                name.cyan(),
                undeclared_network.join(", ")
            );
            let _ = runtime.record_audit_event(
                name,
                "permission-network-warning",
                None,
                Some(&installed.runtime.command),
                Some(&installed.runtime.args),
            );
        }
        match runtime.start(name, &spec) {
            Ok(StartOutcome::Started) => {
                println!("{} Started {}.", "✓".green().bold(), name.cyan());
                started += 1;
            }
            Ok(StartOutcome::AlreadyRunning) => {
                println!(
                    "{} {} is already running.",
                    "!".yellow().bold(),
                    name.cyan()
                );
                already_running += 1;
            }
            Err(e) => {
                eprintln!(
                    "{} Failed to start {}: {}",
                    "✗".red().bold(),
                    name.cyan(),
                    e
                );
                failed += 1;
            }
        }
    }

    if targets.len() > 1 {
        println!(
            "\n{} Started: {}, already running: {}, failed: {}",
            "•".dimmed(),
            started,
            already_running,
            failed
        );
    }

    if failed > 0 {
        process::exit(1);
    }
}

/// Resolves target server names from a specific name or all installed servers.
fn resolve_targets(server: Option<&str>) -> Vec<String> {
    if let Some(name) = server {
        let config_path = match paths::server_config_path(name) {
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
                name.cyan()
            );
            process::exit(1);
        }
        return vec![name.to_string()];
    }

    let servers_dir = match paths::berth_servers_dir() {
        Some(d) => d,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    if !servers_dir.exists() {
        eprintln!("{} No servers installed.", "!".yellow().bold());
        process::exit(1);
    }

    let mut servers: Vec<String> = match fs::read_dir(servers_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                if path.extension().is_some_and(|ext| ext == "toml") {
                    Some(path.file_stem()?.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect(),
        Err(e) => {
            eprintln!(
                "{} Failed to read installed servers: {}",
                "✗".red().bold(),
                e
            );
            process::exit(1);
        }
    };

    servers.sort();
    if servers.is_empty() {
        eprintln!("{} No servers installed.", "!".yellow().bold());
        process::exit(1);
    }

    servers
}

/// Loads and parses an installed server config file.
fn read_installed(name: &str, config_path: &Path) -> Result<InstalledServer, ()> {
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{} Failed to read config for {}: {}",
                "✗".red().bold(),
                name.cyan(),
                e
            );
            return Err(());
        }
    };

    match toml::from_str::<InstalledServer>(&content) {
        Ok(i) => Ok(i),
        Err(e) => {
            eprintln!(
                "{} Failed to parse config for {}: {}",
                "✗".red().bold(),
                name.cyan(),
                e
            );
            Err(())
        }
    }
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
        &installed.permissions.filesystem,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_required_keys_detects_empty_values() {
        let mut installed = InstalledServer {
            server: berth_registry::config::ServerInfo {
                name: "github".to_string(),
                display_name: "GitHub".to_string(),
                version: "1.2.0".to_string(),
                description: "d".to_string(),
                category: "developer-tools".to_string(),
                maintainer: "Anthropic".to_string(),
                trust_level: "official".to_string(),
            },
            source: berth_registry::config::SourceInfo {
                source_type: "npm".to_string(),
                package: "@pkg".to_string(),
                repository: "https://example.com".to_string(),
            },
            runtime: berth_registry::config::RuntimeInfo {
                runtime_type: "node".to_string(),
                command: "npx".to_string(),
                args: vec![],
                transport: "stdio".to_string(),
            },
            permissions: berth_registry::config::PermissionsInfo {
                network: vec![],
                env: vec![],
                filesystem: vec![],
                exec: vec![],
            },
            config: std::collections::BTreeMap::from([
                ("token".to_string(), "".to_string()),
                ("enterprise-url".to_string(), "".to_string()),
            ]),
            config_meta: berth_registry::config::ConfigMeta {
                required_keys: vec!["token".to_string()],
                optional_keys: vec!["enterprise-url".to_string()],
            },
        };

        assert_eq!(missing_required_keys(&installed), vec!["token".to_string()]);

        installed
            .config
            .insert("token".to_string(), "abc123".to_string());
        assert!(missing_required_keys(&installed).is_empty());
    }
}
