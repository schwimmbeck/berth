// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Command handler for `berth update`.

use berth_registry::config::InstalledServer;
use colored::Colorize;
use std::fs;
use std::process;

use berth_registry::Registry;

use crate::paths;

enum UpdateResult {
    Updated { from: String, to: String },
    UpToDate { version: String },
}

/// Executes the `berth update` command.
pub fn execute(server: Option<&str>, all: bool) {
    if all && server.is_some() {
        eprintln!(
            "{} Use either a server name or {}, not both.",
            "✗".red().bold(),
            "--all".bold()
        );
        process::exit(1);
    }

    if !all && server.is_none() {
        eprintln!(
            "{} Specify a server name or use {}.",
            "✗".red().bold(),
            "--all".bold()
        );
        process::exit(1);
    }

    let registry = Registry::from_seed();

    if all {
        let targets = match installed_server_names() {
            Ok(t) => t,
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        };

        let mut updated = 0usize;
        let mut up_to_date = 0usize;
        let mut failed = 0usize;

        for name in &targets {
            match update_one(name, &registry) {
                Ok(UpdateResult::Updated { from, to }) => {
                    println!(
                        "{} Updated {} ({} -> {}).",
                        "✓".green().bold(),
                        name.cyan(),
                        from,
                        to
                    );
                    updated += 1;
                }
                Ok(UpdateResult::UpToDate { version }) => {
                    println!(
                        "{} {} is already up to date (v{}).",
                        "!".yellow().bold(),
                        name.cyan(),
                        version
                    );
                    up_to_date += 1;
                }
                Err(msg) => {
                    eprintln!("{} {}", "✗".red().bold(), msg);
                    failed += 1;
                }
            }
        }

        println!(
            "\n{} Updated: {}, up to date: {}, failed: {}",
            "•".dimmed(),
            updated,
            up_to_date,
            failed
        );

        if failed > 0 {
            process::exit(1);
        }

        return;
    }

    if let Some(name) = server {
        match update_one(name, &registry) {
            Ok(UpdateResult::Updated { from, to }) => {
                println!(
                    "{} Updated {} ({} -> {}).",
                    "✓".green().bold(),
                    name.cyan(),
                    from,
                    to
                );
            }
            Ok(UpdateResult::UpToDate { version }) => {
                println!(
                    "{} {} is already up to date (v{}).",
                    "!".yellow().bold(),
                    name.cyan(),
                    version
                );
            }
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        }
    }
}

/// Lists all installed servers by config file stem.
fn installed_server_names() -> Result<Vec<String>, String> {
    let servers_dir = paths::berth_servers_dir().ok_or("Could not determine home directory.")?;
    if !servers_dir.exists() {
        return Err("No servers installed.".to_string());
    }

    let mut names: Vec<String> = fs::read_dir(&servers_dir)
        .map_err(|e| format!("Failed to read installed servers: {e}"))?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                path.file_stem().map(|n| n.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();

    names.sort();
    if names.is_empty() {
        return Err("No servers installed.".to_string());
    }

    Ok(names)
}

/// Updates a single installed server from seed registry metadata.
fn update_one(name: &str, registry: &Registry) -> Result<UpdateResult, String> {
    let config_path =
        paths::server_config_path(name).ok_or("Could not determine home directory.")?;
    if !config_path.exists() {
        return Err(format!("Server {} is not installed.", name.cyan()));
    }

    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config file: {e}"))?;
    let current: InstalledServer =
        toml::from_str(&content).map_err(|e| format!("Failed to parse config file: {e}"))?;

    let meta = registry
        .get(name)
        .ok_or_else(|| format!("Server {} not found in the registry.", name.cyan()))?;

    if current.server.version == meta.version {
        return Ok(UpdateResult::UpToDate {
            version: meta.version.clone(),
        });
    }

    let from_version = current.server.version.clone();
    let mut updated = InstalledServer::from_metadata(meta);
    merge_config_values(&current, &mut updated);

    let rendered =
        toml::to_string_pretty(&updated).map_err(|e| format!("Failed to serialize config: {e}"))?;
    fs::write(&config_path, rendered).map_err(|e| format!("Failed to write config file: {e}"))?;

    Ok(UpdateResult::Updated {
        from: from_version,
        to: updated.server.version,
    })
}

/// Preserves non-empty existing config values for keys in the new schema.
fn merge_config_values(current: &InstalledServer, updated: &mut InstalledServer) {
    for (key, old_value) in &current.config {
        if old_value.trim().is_empty() {
            continue;
        }
        if let Some(new_value) = updated.config.get_mut(key) {
            *new_value = old_value.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_existing_non_empty_values() {
        let registry = Registry::from_seed();
        let meta = registry.get("github").unwrap();
        let mut current = InstalledServer::from_metadata(meta);
        current
            .config
            .insert("token".to_string(), "abc123".to_string());
        current.config.insert(
            "enterprise-url".to_string(),
            "https://ghe.example".to_string(),
        );

        let mut updated = InstalledServer::from_metadata(meta);
        merge_config_values(&current, &mut updated);

        assert_eq!(updated.config.get("token"), Some(&"abc123".to_string()));
        assert_eq!(
            updated.config.get("enterprise-url"),
            Some(&"https://ghe.example".to_string())
        );
    }

    #[test]
    fn merge_keeps_new_default_when_existing_value_is_empty() {
        let registry = Registry::from_seed();
        let meta = registry.get("postgres").unwrap();
        let mut current = InstalledServer::from_metadata(meta);
        current.config.insert(
            "connection-string".to_string(),
            "postgres://localhost/db".to_string(),
        );
        current.config.insert("schema".to_string(), "".to_string());

        let mut updated = InstalledServer::from_metadata(meta);
        merge_config_values(&current, &mut updated);

        assert_eq!(
            updated.config.get("connection-string"),
            Some(&"postgres://localhost/db".to_string())
        );
        assert_eq!(updated.config.get("schema"), Some(&"public".to_string()));
    }
}
