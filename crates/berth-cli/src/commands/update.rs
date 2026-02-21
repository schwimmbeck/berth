//! Command handler for `berth update`.

use colored::Colorize;
use std::process;

use berth_registry::Registry;

use crate::paths;

/// Executes the `berth update` command.
pub fn execute(server: Option<&str>, all: bool) {
    if !all && server.is_none() {
        eprintln!(
            "{} Specify a server name or use {}.",
            "✗".red().bold(),
            "--all".bold()
        );
        process::exit(1);
    }

    if all {
        // Check if any servers are installed
        let servers_dir = match paths::berth_servers_dir() {
            Some(d) if d.exists() => d,
            _ => {
                eprintln!("{} No servers installed.", "!".yellow().bold());
                process::exit(1);
            }
        };

        let has_servers = std::fs::read_dir(servers_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .any(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
            })
            .unwrap_or(false);

        if !has_servers {
            eprintln!("{} No servers installed.", "!".yellow().bold());
            process::exit(1);
        }

        println!(
            "{} {} is not yet available. Coming soon!",
            "!".yellow().bold(),
            "berth update --all".bold()
        );
    } else if let Some(name) = server {
        // Validate server exists in registry
        let registry = Registry::from_seed();
        if registry.get(name).is_none() {
            eprintln!(
                "{} Server {} not found in the registry.",
                "✗".red().bold(),
                name.cyan()
            );
            process::exit(1);
        }

        // Check if installed
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

        println!(
            "{} {} is not yet available. Coming soon!",
            "!".yellow().bold(),
            format!("berth update {name}").bold()
        );
    }
}
