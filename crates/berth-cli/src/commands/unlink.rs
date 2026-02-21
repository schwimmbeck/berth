//! Command handler for `berth unlink`.

use colored::Colorize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::paths;

/// Executes the `berth unlink` command.
pub fn execute(client: &str) {
    if client != "claude-desktop" {
        eprintln!(
            "{} Client {} is not supported yet. Use {} for now.",
            "✗".red().bold(),
            client.cyan(),
            "claude-desktop".bold()
        );
        process::exit(1);
    }

    unlink_claude_desktop();
}

/// Removes Berth-managed server entries from Claude Desktop config.
fn unlink_claude_desktop() {
    let config_path = match paths::claude_desktop_config_path() {
        Some(p) => p,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    if !config_path.exists() {
        println!(
            "{} Claude Desktop config not found at {}.",
            "!".yellow().bold(),
            config_path.display()
        );
        return;
    }

    let content = match fs::read_to_string(&config_path) {
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

    let mut root = match serde_json::from_str::<Value>(&content) {
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

    let installed = match installed_server_names() {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    let backup = backup_path(&config_path);
    if let Err(e) = fs::copy(&config_path, &backup) {
        eprintln!(
            "{} Failed to create backup {}: {}",
            "✗".red().bold(),
            backup.display(),
            e
        );
        process::exit(1);
    }

    let mut removed = 0usize;
    if let Some(root_obj) = root.as_object_mut() {
        if let Some(mcp) = root_obj
            .get_mut("mcpServers")
            .and_then(Value::as_object_mut)
        {
            for server in &installed {
                if mcp.remove(server).is_some() {
                    removed += 1;
                }
            }
        }
    } else {
        eprintln!(
            "{} Client config root must be a JSON object.",
            "✗".red().bold()
        );
        process::exit(1);
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

    if let Err(e) = fs::write(&config_path, rendered) {
        eprintln!(
            "{} Failed to write client config {}: {}",
            "✗".red().bold(),
            config_path.display(),
            e
        );
        process::exit(1);
    }

    if removed == 0 {
        println!(
            "{} No Berth-managed servers were present in {}.",
            "!".yellow().bold(),
            "claude-desktop".cyan()
        );
    } else {
        println!(
            "{} Unlinked {} server(s) from {}.",
            "✓".green().bold(),
            removed,
            "claude-desktop".cyan()
        );
    }
    println!("  Config: {}", config_path.display());
    println!("  Backup: {}", backup.display());
}

/// Lists installed server names derived from `~/.berth/servers/*.toml`.
fn installed_server_names() -> Result<Vec<String>, String> {
    let servers_dir = paths::berth_servers_dir().ok_or("Could not determine home directory.")?;
    if !servers_dir.exists() {
        return Ok(Vec::new());
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
    Ok(names)
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
