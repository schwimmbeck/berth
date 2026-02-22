// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Command handler for `berth config`.

use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;

use crate::paths;
use crate::runtime_policy::{
    is_runtime_policy_key, parse_runtime_policy, validate_runtime_policy_value, KEY_AUTO_RESTART,
    KEY_MAX_RESTARTS,
};
use crate::sandbox_policy::{
    is_sandbox_policy_key, parse_sandbox_policy, validate_sandbox_policy_value, KEY_SANDBOX,
    KEY_SANDBOX_NETWORK,
};
use crate::secrets::store_secret;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigBundle {
    version: u32,
    servers: BTreeMap<String, BTreeMap<String, String>>,
}

/// Executes the `berth config` command.
pub fn execute(
    server: &str,
    path: Option<&str>,
    set: Option<&str>,
    secure: bool,
    env: bool,
    interactive: bool,
) {
    if server == "export" {
        if set.is_some() || env || interactive || secure {
            eprintln!(
                "{} `config export` does not support {}, {}, {}, or {}.",
                "✗".red().bold(),
                "--set".bold(),
                "--env".bold(),
                "--interactive".bold(),
                "--secure".bold()
            );
            process::exit(1);
        }
        export_config_bundle(path);
        return;
    }

    if server == "import" {
        if set.is_some() || env || interactive || secure {
            eprintln!(
                "{} `config import` does not support {}, {}, {}, or {}.",
                "✗".red().bold(),
                "--set".bold(),
                "--env".bold(),
                "--interactive".bold(),
                "--secure".bold()
            );
            process::exit(1);
        }
        let file_path = match path {
            Some(p) => p,
            None => {
                eprintln!(
                    "{} Missing import file. Use: {}",
                    "✗".red().bold(),
                    "berth config import <file>".bold()
                );
                process::exit(1);
            }
        };
        import_config_bundle(file_path);
        return;
    }

    if path.is_some() {
        eprintln!(
            "{} Unexpected extra argument for {}. Use only with {} or {}.",
            "✗".red().bold(),
            server.cyan(),
            "berth config export [file]".bold(),
            "berth config import <file>".bold()
        );
        process::exit(1);
    }

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
        eprintln!("  Run {} first.", format!("berth install {server}").bold());
        process::exit(1);
    }

    if env {
        if interactive {
            eprintln!(
                "{} {} cannot be used with {}.",
                "✗".red().bold(),
                "--env".bold(),
                "--interactive".bold()
            );
            process::exit(1);
        }
        if secure {
            eprintln!(
                "{} {} requires {}.",
                "✗".red().bold(),
                "--secure".bold(),
                "--set".bold()
            );
            process::exit(1);
        }
        show_env(server);
        return;
    }

    if let Some(kv) = set {
        if interactive {
            eprintln!(
                "{} {} cannot be used with {}.",
                "✗".red().bold(),
                "--set".bold(),
                "--interactive".bold()
            );
            process::exit(1);
        }
        set_config(server, kv, secure, &config_path);
        return;
    }

    if secure {
        eprintln!(
            "{} {} requires {}.",
            "✗".red().bold(),
            "--secure".bold(),
            "--set".bold()
        );
        process::exit(1);
    }

    if interactive {
        prompt_config(server, &config_path);
        return;
    }

    show_config(server, &config_path);
}

/// Prompts interactively for required and optional values, then persists the config.
fn prompt_config(server: &str, config_path: &Path) {
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to read config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    let mut installed: InstalledServer = match toml::from_str(&content) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{} Failed to parse config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    let registry = Registry::from_seed();
    let meta = match registry.get(server) {
        Some(m) => m,
        None => {
            eprintln!(
                "{} Server {} not found in the registry.",
                "✗".red().bold(),
                server.cyan()
            );
            process::exit(1);
        }
    };

    println!(
        "{} Interactive configuration for {} (press Enter to keep current value):\n",
        "✓".green().bold(),
        server.cyan()
    );

    for field in meta
        .config
        .required
        .iter()
        .chain(meta.config.optional.iter())
    {
        let current = installed
            .config
            .get(&field.key)
            .cloned()
            .unwrap_or_default();
        let prompt = if current.is_empty() {
            format!(
                "{} {} - {}: ",
                if meta.config.required.iter().any(|f| f.key == field.key) {
                    "[required]".yellow().bold().to_string()
                } else {
                    "[optional]".dimmed().to_string()
                },
                field.key.bold(),
                field.description.dimmed()
            )
        } else {
            format!(
                "{} {} - {} [{}]: ",
                if meta.config.required.iter().any(|f| f.key == field.key) {
                    "[required]".yellow().bold().to_string()
                } else {
                    "[optional]".dimmed().to_string()
                },
                field.key.bold(),
                field.description.dimmed(),
                current
            )
        };

        let input = match prompt_line(&prompt) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{} Failed to read input: {}", "✗".red().bold(), e);
                process::exit(1);
            }
        };

        if !input.is_empty() {
            installed.config.insert(field.key.clone(), input);
        }
    }

    let missing: Vec<String> = installed
        .config_meta
        .required_keys
        .iter()
        .filter(|k| installed.config.get(*k).is_none_or(|v| v.trim().is_empty()))
        .cloned()
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "{} Missing required config after interactive setup: {}",
            "✗".red().bold(),
            missing.join(", ").yellow()
        );
        process::exit(1);
    }

    let rendered = match toml::to_string_pretty(&installed) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} Failed to serialize config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };
    if let Err(e) = fs::write(config_path, rendered) {
        eprintln!("{} Failed to write config: {}", "✗".red().bold(), e);
        process::exit(1);
    }

    println!(
        "\n{} Saved configuration for {}.",
        "✓".green().bold(),
        server.cyan()
    );
}

/// Prints a prompt and returns the trimmed line entered by the user.
fn prompt_line(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Prints configured and required keys for an installed server.
fn show_config(server: &str, config_path: &std::path::Path) {
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to read config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    let installed: InstalledServer = match toml::from_str(&content) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{} Failed to parse config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    println!(
        "{} Configuration for {}:\n",
        "✓".green().bold(),
        server.cyan()
    );

    if !installed.config_meta.required_keys.is_empty() {
        println!("  {}", "Required:".bold());
        for key in &installed.config_meta.required_keys {
            let value = installed.config.get(key).map(|v| v.as_str()).unwrap_or("");
            let status = if value.is_empty() {
                "NOT SET".red().to_string()
            } else {
                "set".green().to_string()
            };
            println!("    {:<24} [{}]", key, status);
        }
    }

    if !installed.config_meta.optional_keys.is_empty() {
        if !installed.config_meta.required_keys.is_empty() {
            println!();
        }
        println!("  {}", "Optional:".bold());
        for key in &installed.config_meta.optional_keys {
            let value = installed.config.get(key).map(|v| v.as_str()).unwrap_or("");
            let status = if value.is_empty() {
                "default".dimmed().to_string()
            } else {
                format!("{}", value.green())
            };
            println!("    {:<24} [{}]", key, status);
        }
    }

    if let Ok(policy) = parse_runtime_policy(&installed.config) {
        println!();
        println!("  {}", "Runtime:".bold());
        println!(
            "    {:<24} [{}]",
            KEY_AUTO_RESTART,
            if policy.enabled {
                "true".green().to_string()
            } else {
                "false".dimmed().to_string()
            }
        );
        println!(
            "    {:<24} [{}]",
            KEY_MAX_RESTARTS,
            format!("{}", policy.max_restarts).dimmed()
        );
    }

    if let Ok(policy) = parse_sandbox_policy(&installed.config) {
        println!();
        println!("  {}", "Sandbox:".bold());
        println!(
            "    {:<24} [{}]",
            KEY_SANDBOX,
            if policy.enabled {
                "basic".green().to_string()
            } else {
                "off".dimmed().to_string()
            }
        );
        println!(
            "    {:<24} [{}]",
            KEY_SANDBOX_NETWORK,
            if policy.network_deny_all {
                "deny-all".yellow().to_string()
            } else {
                "inherit".dimmed().to_string()
            }
        );
    }

    println!();
}

/// Sets a single config value (`key=value`) for an installed server.
fn set_config(server: &str, kv: &str, secure: bool, config_path: &Path) {
    let (key, value) = match kv.split_once('=') {
        Some((k, v)) => (k.trim(), v.trim()),
        None => {
            eprintln!(
                "{} Invalid format. Use: {} key=value",
                "✗".red().bold(),
                "--set".bold()
            );
            process::exit(1);
        }
    };

    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Failed to read config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    let mut installed: InstalledServer = match toml::from_str(&content) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{} Failed to parse config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    // Validate that the key is known
    let is_known = installed
        .config_meta
        .required_keys
        .contains(&key.to_string())
        || installed
            .config_meta
            .optional_keys
            .contains(&key.to_string())
        || is_runtime_policy_key(key);
    let is_known = is_known || is_sandbox_policy_key(key);

    if !is_known {
        eprintln!("{} Unknown config key: {}", "✗".red().bold(), key.cyan());
        let mut all_keys: Vec<&str> = installed
            .config_meta
            .required_keys
            .iter()
            .chain(installed.config_meta.optional_keys.iter())
            .map(|s| s.as_str())
            .collect();
        all_keys.push(KEY_AUTO_RESTART);
        all_keys.push(KEY_MAX_RESTARTS);
        all_keys.push(KEY_SANDBOX);
        all_keys.push(KEY_SANDBOX_NETWORK);
        all_keys.sort_unstable();
        eprintln!("  Known keys: {}", all_keys.join(", "));
        process::exit(1);
    }

    if is_runtime_policy_key(key) {
        if let Err(msg) = validate_runtime_policy_value(key, value) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    }
    if is_sandbox_policy_key(key) {
        if let Err(msg) = validate_sandbox_policy_value(key, value) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    }

    let persisted_value = if secure {
        match store_secret(server, key, value) {
            Ok(reference) => reference,
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        }
    } else {
        value.to_string()
    };

    installed.config.insert(key.to_string(), persisted_value);

    let toml_str = match toml::to_string_pretty(&installed) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} Failed to serialize config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    if let Err(e) = fs::write(config_path, &toml_str) {
        eprintln!("{} Failed to write config: {}", "✗".red().bold(), e);
        process::exit(1);
    }

    if secure {
        println!(
            "{} Stored {} securely for {}.",
            "✓".green().bold(),
            key.bold(),
            server.cyan()
        );
    } else {
        println!(
            "{} Set {} = {} for {}.",
            "✓".green().bold(),
            key.bold(),
            value,
            server.cyan()
        );
    }
}

/// Prints environment-variable mapping for a registry server definition.
fn show_env(server: &str) {
    let registry = Registry::from_seed();

    let meta = match registry.get(server) {
        Some(m) => m,
        None => {
            eprintln!(
                "{} Server {} not found in the registry.",
                "✗".red().bold(),
                server.cyan()
            );
            process::exit(1);
        }
    };

    println!(
        "{} Environment variables for {}:\n",
        "✓".green().bold(),
        server.cyan()
    );

    if !meta.config.required.is_empty() {
        println!("  {}", "Required:".bold());
        for field in &meta.config.required {
            if let Some(env_var) = &field.env {
                println!("    {:<30} {}", env_var, field.description.dimmed());
            }
        }
    }

    if !meta.config.optional.is_empty() {
        if !meta.config.required.is_empty() {
            println!();
        }
        println!("  {}", "Optional:".bold());
        for field in &meta.config.optional {
            if let Some(env_var) = &field.env {
                println!("    {:<30} {}", env_var, field.description.dimmed());
            }
        }
    }

    println!();
}

/// Exports all installed non-empty server config values as a TOML bundle.
fn export_config_bundle(path: Option<&str>) {
    let entries = match installed_server_entries() {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    let mut servers: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (name, config_path) in &entries {
        let installed = match read_installed(config_path) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!(
                    "{} Failed to read {}: {}",
                    "✗".red().bold(),
                    name.cyan(),
                    msg
                );
                process::exit(1);
            }
        };

        let values: BTreeMap<String, String> = installed
            .config
            .into_iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .collect();
        servers.insert(name.clone(), values);
    }

    let bundle = ConfigBundle {
        version: 1,
        servers,
    };
    let rendered = match toml::to_string_pretty(&bundle) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "{} Failed to serialize config bundle: {}",
                "✗".red().bold(),
                e
            );
            process::exit(1);
        }
    };

    if let Some(out_path) = path {
        let out = PathBuf::from(out_path);
        if let Some(parent) = out.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!(
                        "{} Failed to create output directory {}: {}",
                        "✗".red().bold(),
                        parent.display(),
                        e
                    );
                    process::exit(1);
                }
            }
        }
        if let Err(e) = fs::write(&out, rendered) {
            eprintln!(
                "{} Failed to write export file {}: {}",
                "✗".red().bold(),
                out.display(),
                e
            );
            process::exit(1);
        }
        println!(
            "{} Exported {} server config(s) to {}.",
            "✓".green().bold(),
            entries.len(),
            out.display()
        );
        return;
    }

    println!("{rendered}");
}

/// Imports server config values from a TOML bundle and applies known keys.
fn import_config_bundle(path: &str) {
    let content = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "{} Failed to read import file {}: {}",
                "✗".red().bold(),
                path,
                e
            );
            process::exit(1);
        }
    };
    let bundle: ConfigBundle = match toml::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} Failed to parse import file: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };
    if bundle.version != 1 {
        eprintln!(
            "{} Unsupported config bundle version {}.",
            "✗".red().bold(),
            bundle.version
        );
        process::exit(1);
    }

    let mut updated_servers = 0usize;
    let mut updated_values = 0usize;
    let mut skipped_not_installed = 0usize;
    let mut skipped_unknown_keys = 0usize;

    for (server, import_values) in &bundle.servers {
        let config_path = match paths::server_config_path(server) {
            Some(p) => p,
            None => {
                eprintln!("{} Could not determine home directory.", "✗".red().bold());
                process::exit(1);
            }
        };
        if !config_path.exists() {
            skipped_not_installed += 1;
            continue;
        }

        let mut installed = match read_installed(&config_path) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!(
                    "{} Failed to load config for {}: {}",
                    "✗".red().bold(),
                    server.cyan(),
                    msg
                );
                process::exit(1);
            }
        };

        let mut changed = false;
        for (key, value) in import_values {
            if value.trim().is_empty() {
                continue;
            }
            let is_known = installed.config_meta.required_keys.contains(key)
                || installed.config_meta.optional_keys.contains(key);
            if !is_known {
                skipped_unknown_keys += 1;
                continue;
            }
            if installed.config.get(key).is_none_or(|v| v != value) {
                installed.config.insert(key.clone(), value.clone());
                updated_values += 1;
                changed = true;
            }
        }

        if changed {
            let rendered = match toml::to_string_pretty(&installed) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "{} Failed to serialize updated config for {}: {}",
                        "✗".red().bold(),
                        server.cyan(),
                        e
                    );
                    process::exit(1);
                }
            };
            if let Err(e) = fs::write(&config_path, rendered) {
                eprintln!(
                    "{} Failed to write updated config for {}: {}",
                    "✗".red().bold(),
                    server.cyan(),
                    e
                );
                process::exit(1);
            }
            updated_servers += 1;
        }
    }

    println!(
        "{} Import summary: updated servers: {}, updated values: {}, skipped (not installed): {}, skipped unknown keys: {}.",
        "✓".green().bold(),
        updated_servers,
        updated_values,
        skipped_not_installed,
        skipped_unknown_keys
    );
}

/// Returns installed server file entries as `(server_name, path)` sorted by name.
fn installed_server_entries() -> Result<Vec<(String, PathBuf)>, String> {
    let servers_dir = paths::berth_servers_dir().ok_or("Could not determine home directory.")?;
    if !servers_dir.exists() {
        return Err("No servers installed. Run `berth install <server>` first.".to_string());
    }

    let mut entries: Vec<(String, PathBuf)> = fs::read_dir(&servers_dir)
        .map_err(|e| format!("Failed to read installed servers: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().to_string();
            Some((name, p))
        })
        .collect();

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.is_empty() {
        return Err("No servers installed. Run `berth install <server>` first.".to_string());
    }
    Ok(entries)
}

/// Reads and parses an installed server config file.
fn read_installed(path: &Path) -> Result<InstalledServer, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read config: {e}"))?;
    toml::from_str::<InstalledServer>(&content).map_err(|e| format!("Failed to parse config: {e}"))
}
