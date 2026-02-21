//! Command handler for `berth link`.

use colored::Colorize;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;

use crate::paths;
use crate::permission_filter::{filter_env_map, load_permission_overrides};

#[derive(Serialize)]
struct ClientServerConfig {
    command: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
}

/// Executes the `berth link` command.
pub fn execute(client: &str) {
    let config_path = match paths::client_config_path(client) {
        Some(p) => p,
        None => {
            eprintln!(
                "{} Unsupported client {}. Supported: {}, {}, {}, {}, {}.",
                "✗".red().bold(),
                client.cyan(),
                "claude-desktop".bold(),
                "cursor".bold(),
                "windsurf".bold(),
                "continue".bold(),
                "vscode".bold()
            );
            process::exit(1);
        }
    };
    link_client(client, &config_path);
}

/// Links all installable Berth servers into a supported client config file.
fn link_client(client: &str, config_path: &Path) {
    let linked_servers = match load_linkable_servers() {
        Ok(servers) => servers,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    if let Some(parent) = config_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!(
                "{} Failed to create client config directory {}: {}",
                "✗".red().bold(),
                parent.display(),
                e
            );
            process::exit(1);
        }
    }

    let (mut root, backup_path) = if config_path.exists() {
        let content = match fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "{} Failed to read existing client config {}: {}",
                    "✗".red().bold(),
                    config_path.display(),
                    e
                );
                process::exit(1);
            }
        };

        let parsed = match serde_json::from_str::<Value>(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "{} Existing client config is not valid JSON ({}): {}",
                    "✗".red().bold(),
                    config_path.display(),
                    e
                );
                process::exit(1);
            }
        };

        let backup = backup_path(config_path);
        if let Err(e) = fs::copy(config_path, &backup) {
            eprintln!(
                "{} Failed to create backup {}: {}",
                "✗".red().bold(),
                backup.display(),
                e
            );
            process::exit(1);
        }

        (parsed, Some(backup))
    } else {
        (Value::Object(Map::new()), None)
    };

    if !root.is_object() {
        eprintln!(
            "{} Client config root must be a JSON object.",
            "✗".red().bold()
        );
        process::exit(1);
    }

    let root_obj = root.as_object_mut().expect("checked object above; qed");

    let mcp_value = root_obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    if !mcp_value.is_object() {
        eprintln!(
            "{} `mcpServers` in client config must be a JSON object.",
            "✗".red().bold()
        );
        process::exit(1);
    }

    let mcp_servers = mcp_value
        .as_object_mut()
        .expect("checked object above; qed");

    for (name, cfg) in &linked_servers {
        let value = match serde_json::to_value(cfg) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "{} Failed to serialize {}: {}",
                    "✗".red().bold(),
                    name.cyan(),
                    e
                );
                process::exit(1);
            }
        };
        mcp_servers.insert(name.clone(), value);
    }

    let rendered = match serde_json::to_string_pretty(&root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "{} Failed to serialize client config JSON: {}",
                "✗".red().bold(),
                e
            );
            process::exit(1);
        }
    };

    if let Err(e) = fs::write(config_path, rendered) {
        eprintln!(
            "{} Failed to write client config {}: {}",
            "✗".red().bold(),
            config_path.display(),
            e
        );
        process::exit(1);
    }

    println!(
        "{} Linked {} to {} with {} server(s).",
        "✓".green().bold(),
        "berth".bold(),
        client.cyan(),
        linked_servers.len()
    );
    println!("  Config: {}", config_path.display());
    if let Some(backup) = backup_path {
        println!("  Backup: {}", backup.display());
    }
}

/// Loads installed server definitions and converts them to client entries.
fn load_linkable_servers() -> Result<Vec<(String, ClientServerConfig)>, String> {
    let servers_dir = paths::berth_servers_dir().ok_or("Could not determine home directory.")?;

    if !servers_dir.exists() {
        return Err("No servers installed. Run `berth install <server>` first.".to_string());
    }

    let mut entries: Vec<_> = fs::read_dir(&servers_dir)
        .map_err(|e| format!("Failed to read installed servers: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();

    if entries.is_empty() {
        return Err("No servers installed. Run `berth install <server>` first.".to_string());
    }

    entries.sort_by_key(|e| e.path());
    let registry = Registry::from_seed();
    let mut out = Vec::new();

    for entry in &entries {
        let path = entry.path();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read config for {name}: {e}"))?;
        let installed: InstalledServer = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config for {name}: {e}"))?;

        let missing_required: Vec<String> = installed
            .config_meta
            .required_keys
            .iter()
            .filter(|k| installed.config.get(*k).is_none_or(|v| v.trim().is_empty()))
            .cloned()
            .collect();

        if !missing_required.is_empty() {
            return Err(format!(
                "Server {} is missing required config: {}",
                name.cyan(),
                missing_required.join(", ")
            ));
        }

        let mut env = BTreeMap::new();
        if let Some(meta) = registry.get(&name) {
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
        let overrides = load_permission_overrides(&name)?;
        filter_env_map(&mut env, &installed.permissions.env, &overrides);

        out.push((
            name,
            ClientServerConfig {
                command: installed.runtime.command,
                args: installed.runtime.args,
                env,
            },
        ));
    }

    Ok(out)
}

/// Returns a deterministic backup path next to the client config file.
fn backup_path(config_path: &Path) -> PathBuf {
    let file_name = config_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    config_path.with_file_name(format!("{file_name}.bak"))
}
