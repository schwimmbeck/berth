//! Command handler for `berth restart`.

use colored::Colorize;
use std::fs;
use std::process;

use berth_registry::config::InstalledServer;
use berth_runtime::RuntimeManager;

use crate::paths;

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

    if let Err(e) = runtime.restart(server) {
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
