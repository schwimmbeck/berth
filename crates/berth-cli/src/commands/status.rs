//! Command handler for `berth status`.

use colored::Colorize;
use std::fs;
use std::process;

use berth_registry::config::InstalledServer;
use berth_runtime::{RuntimeManager, ServerStatus};

use crate::paths;

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

    println!("{} MCP server status:\n", "✓".green().bold());
    println!(
        "  {:<20} {:<12} {:<12}",
        "NAME".bold(),
        "VERSION".bold(),
        "STATUS".bold(),
    );
    println!("  {}", "─".repeat(50));

    let mut had_error = false;
    for entry in &entries {
        let path = entry.path();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let version = match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<InstalledServer>(&content) {
                Ok(installed) => installed.server.version,
                Err(_) => {
                    had_error = true;
                    "?".to_string()
                }
            },
            Err(_) => {
                had_error = true;
                "?".to_string()
            }
        };

        let status_display = match runtime.status(&name) {
            Ok(ServerStatus::Running) => "running".green().to_string(),
            Ok(ServerStatus::Stopped) => "stopped".dimmed().to_string(),
            Err(_) => {
                had_error = true;
                "error".red().to_string()
            }
        };

        println!(
            "  {:<20} {:<12} {:<12}",
            name.cyan(),
            version,
            status_display
        );
    }
    println!();

    if had_error {
        process::exit(1);
    }
}
