//! Command handler for `berth stop`.

use colored::Colorize;
use std::fs;
use std::process;

use berth_runtime::{RuntimeManager, StopOutcome};

use crate::paths;

/// Executes the `berth stop` command.
pub fn execute(server: Option<&str>) {
    let targets = resolve_targets(server);
    let berth_home = match paths::berth_home() {
        Some(h) => h,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    let runtime = RuntimeManager::new(berth_home);

    let mut stopped = 0usize;
    let mut already_stopped = 0usize;
    let mut failed = 0usize;

    for name in &targets {
        match runtime.stop(name) {
            Ok(StopOutcome::Stopped) => {
                println!("{} Stopped {}.", "✓".green().bold(), name.cyan());
                stopped += 1;
            }
            Ok(StopOutcome::AlreadyStopped) => {
                println!(
                    "{} {} is already stopped.",
                    "!".yellow().bold(),
                    name.cyan()
                );
                already_stopped += 1;
            }
            Err(e) => {
                eprintln!("{} Failed to stop {}: {}", "✗".red().bold(), name.cyan(), e);
                failed += 1;
            }
        }
    }

    if targets.len() > 1 {
        println!(
            "\n{} Stopped: {}, already stopped: {}, failed: {}",
            "•".dimmed(),
            stopped,
            already_stopped,
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
