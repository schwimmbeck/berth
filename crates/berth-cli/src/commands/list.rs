// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Command handler for `berth list`.

use colored::Colorize;
use std::fs;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;
use berth_runtime::{RuntimeManager, ServerStatus};

use crate::paths;

/// Executes the `berth list` command.
pub fn execute() {
    let servers_dir = match paths::berth_servers_dir() {
        Some(d) => d,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            return;
        }
    };

    if !servers_dir.exists() {
        print_no_servers();
        return;
    }

    let entries: Vec<_> = match fs::read_dir(&servers_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
            .collect(),
        Err(_) => {
            print_no_servers();
            return;
        }
    };

    if entries.is_empty() {
        print_no_servers();
        return;
    }

    let registry = Registry::from_seed();
    let runtime = RuntimeManager::new(paths::berth_home().unwrap_or_else(|| servers_dir.clone()));

    println!(
        "{} {} server(s) installed:\n",
        "✓".green().bold(),
        entries.len()
    );

    println!(
        "  {:<20} {:<12} {:<12} {}",
        "NAME".bold(),
        "VERSION".bold(),
        "STATUS".bold(),
        "UPDATE".bold(),
    );
    println!("  {}", "─".repeat(60));

    for entry in &entries {
        let path = entry.path();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let (version, update) = match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<InstalledServer>(&content) {
                Ok(installed) => {
                    let ver = installed.server.version.clone();
                    let upd = match registry.get(&name) {
                        Some(meta) if meta.version != ver => {
                            format!("{} available", meta.version).yellow().to_string()
                        }
                        Some(_) => "up to date".green().to_string(),
                        None => "unknown".dimmed().to_string(),
                    };
                    (ver, upd)
                }
                Err(_) => ("?".to_string(), "parse error".red().to_string()),
            },
            Err(_) => ("?".to_string(), "read error".red().to_string()),
        };

        let status = match runtime.status(&name) {
            Ok(ServerStatus::Running) => "running".green().to_string(),
            Ok(ServerStatus::Stopped) => "stopped".dimmed().to_string(),
            Err(_) => "error".red().to_string(),
        };

        println!(
            "  {:<20} {:<12} {:<12} {}",
            name.cyan(),
            version,
            status,
            update,
        );
    }
    println!();
}

/// Prints a consistent "no servers installed" hint block.
fn print_no_servers() {
    println!("{} No servers installed.\n", "!".yellow().bold());
    println!(
        "  Run {} to find servers, or {} to install one.",
        "berth search <query>".bold(),
        "berth install <server>".bold(),
    );
}
