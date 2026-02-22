// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Command handler for `berth policy`.

use berth_registry::config::InstalledServer;
use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process;

use crate::paths;
use crate::permission_filter::load_permission_overrides;
use crate::policy_engine::{enforce_global_policy, load_global_policy, GlobalPolicy};

/// Executes the `berth policy` command.
pub fn execute(server: Option<&str>, set: Option<&str>, init: bool, json: bool) {
    if server.is_some() && (set.is_some() || init) {
        eprintln!(
            "{} `berth policy <server>` cannot be combined with `--set` or `--init`.",
            "✗".red().bold()
        );
        process::exit(1);
    }
    if set.is_some() && init {
        eprintln!(
            "{} Use either `--set` or `--init`, not both.",
            "✗".red().bold()
        );
        process::exit(1);
    }

    let policy_path = match paths::policy_path() {
        Some(path) => path,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    if init {
        if let Err(msg) = initialize_policy_file(&policy_path) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
        println!(
            "{} Initialized policy file at {}.",
            "✓".green().bold(),
            policy_path.display()
        );
        return;
    }

    let mut policy = match load_global_policy() {
        Ok(policy) => policy,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    if let Some(expr) = set {
        if let Err(msg) = apply_policy_set(&mut policy, expr) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
        if let Err(msg) = write_policy_file(&policy_path, &policy) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
        println!("{} Updated policy: {}.", "✓".green().bold(), expr.bold());
        return;
    }

    if let Some(server_name) = server {
        let installed = match read_installed(server_name) {
            Ok(installed) => installed,
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        };
        let overrides = match load_permission_overrides(server_name) {
            Ok(overrides) => overrides,
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        };

        let validation =
            enforce_global_policy(server_name, &installed.permissions, &overrides, &policy);
        if json {
            let payload = match &validation {
                Ok(()) => serde_json::json!({
                    "server": server_name,
                    "allowed": true
                }),
                Err(msg) => serde_json::json!({
                    "server": server_name,
                    "allowed": false,
                    "reason": msg
                }),
            };
            match serde_json::to_string_pretty(&payload) {
                Ok(rendered) => println!("{rendered}"),
                Err(e) => {
                    eprintln!(
                        "{} Failed to serialize policy validation JSON: {}",
                        "✗".red().bold(),
                        e
                    );
                    process::exit(1);
                }
            }
            if validation.is_err() {
                process::exit(1);
            }
        } else {
            match validation {
                Ok(()) => {
                    println!(
                        "{} Policy allows {}.",
                        "✓".green().bold(),
                        server_name.cyan()
                    );
                }
                Err(msg) => {
                    eprintln!("{} {}", "✗".red().bold(), msg);
                    process::exit(1);
                }
            }
        }
        return;
    }

    if json {
        match serde_json::to_string_pretty(&policy) {
            Ok(rendered) => println!("{rendered}"),
            Err(e) => {
                eprintln!(
                    "{} Failed to serialize policy JSON: {}",
                    "✗".red().bold(),
                    e
                );
                process::exit(1);
            }
        }
        return;
    }

    print_policy(&policy, &policy_path);
}

fn initialize_policy_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create policy directory: {e}"))?;
    }
    write_policy_file(path, &GlobalPolicy::default())
}

fn write_policy_file(path: &Path, policy: &GlobalPolicy) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create policy directory: {e}"))?;
    }
    let rendered =
        toml::to_string_pretty(policy).map_err(|e| format!("Failed to serialize policy: {e}"))?;
    fs::write(path, rendered).map_err(|e| format!("Failed to write policy file: {e}"))
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!(
            "Expected boolean `true` or `false`, got `{value}`."
        )),
    }
}

fn apply_policy_set(policy: &mut GlobalPolicy, expr: &str) -> Result<(), String> {
    let (key, value) = expr
        .split_once('=')
        .ok_or("Invalid --set format. Use key=value.")?;
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() {
        return Err("Invalid --set format. Key is required.".to_string());
    }

    match key {
        "servers.deny" => {
            policy.servers.deny = value
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
        }
        "permissions.deny_network_wildcard" => {
            policy.permissions.deny_network_wildcard = parse_bool(value)?;
        }
        "permissions.deny_env_wildcard" => {
            policy.permissions.deny_env_wildcard = parse_bool(value)?;
        }
        "permissions.deny_filesystem_write" => {
            policy.permissions.deny_filesystem_write = parse_bool(value)?;
        }
        "permissions.deny_exec_wildcard" => {
            policy.permissions.deny_exec_wildcard = parse_bool(value)?;
        }
        _ => {
            return Err(format!(
                "Unknown policy key `{key}`. Supported: servers.deny, permissions.deny_network_wildcard, permissions.deny_env_wildcard, permissions.deny_filesystem_write, permissions.deny_exec_wildcard."
            ));
        }
    }
    Ok(())
}

fn read_installed(server: &str) -> Result<InstalledServer, String> {
    let config_path =
        paths::server_config_path(server).ok_or("Could not determine home directory.")?;
    if !config_path.exists() {
        return Err(format!("Server {server} is not installed."));
    }
    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config: {e}"))?;
    toml::from_str::<InstalledServer>(&content).map_err(|e| format!("Failed to parse config: {e}"))
}

fn print_policy(policy: &GlobalPolicy, path: &Path) {
    println!(
        "{} Policy file: {}\n",
        "✓".green().bold(),
        path.display().to_string().cyan()
    );

    println!("  {}", "Servers".bold());
    if policy.servers.deny.is_empty() {
        println!("    {} {}", "deny:".dimmed(), "none".dimmed());
    } else {
        println!(
            "    {} {}",
            "deny:".dimmed(),
            policy.servers.deny.join(", ")
        );
    }

    println!();
    println!("  {}", "Permission Guards".bold());
    println!(
        "    {} {}",
        "deny_network_wildcard:".dimmed(),
        policy.permissions.deny_network_wildcard
    );
    println!(
        "    {} {}",
        "deny_env_wildcard:".dimmed(),
        policy.permissions.deny_env_wildcard
    );
    println!(
        "    {} {}",
        "deny_filesystem_write:".dimmed(),
        policy.permissions.deny_filesystem_write
    );
    println!(
        "    {} {}",
        "deny_exec_wildcard:".dimmed(),
        policy.permissions.deny_exec_wildcard
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_policy_set_updates_known_keys() {
        let mut policy = GlobalPolicy::default();
        apply_policy_set(&mut policy, "permissions.deny_network_wildcard=true").unwrap();
        apply_policy_set(&mut policy, "permissions.deny_env_wildcard=true").unwrap();
        apply_policy_set(&mut policy, "servers.deny=github,filesystem").unwrap();

        assert!(policy.permissions.deny_network_wildcard);
        assert!(policy.permissions.deny_env_wildcard);
        assert_eq!(policy.servers.deny, vec!["github", "filesystem"]);
    }

    #[test]
    fn apply_policy_set_rejects_invalid_input() {
        let mut policy = GlobalPolicy::default();
        assert!(apply_policy_set(&mut policy, "bad").is_err());
        assert!(apply_policy_set(&mut policy, "permissions.deny_exec_wildcard=maybe").is_err());
        assert!(apply_policy_set(&mut policy, "permissions.unknown=true").is_err());
    }
}
