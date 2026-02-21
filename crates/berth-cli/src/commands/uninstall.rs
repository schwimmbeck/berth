//! Command handler for `berth uninstall`.

use colored::Colorize;
use std::fs;
use std::process;

use crate::paths;

/// Executes the `berth uninstall` command.
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

    if let Err(e) = fs::remove_file(&config_path) {
        eprintln!("{} Failed to remove config file: {}", "✗".red().bold(), e);
        process::exit(1);
    }

    println!("{} Uninstalled {}.", "✓".green().bold(), server.cyan());
}
