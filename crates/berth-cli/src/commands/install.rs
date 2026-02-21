//! Command handler for `berth install`.

use colored::Colorize;
use std::fs;
use std::process;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;

use crate::paths;

/// Executes the `berth install` command.
pub fn execute(server_spec: &str) {
    let (server, requested_version) = match parse_server_spec(server_spec) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
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
    if let Some(version) = requested_version {
        if meta.version != version {
            eprintln!(
                "{} Version {} for {} is not available in the seed registry (available: {}).",
                "✗".red().bold(),
                version.bold(),
                server.cyan(),
                meta.version
            );
            process::exit(1);
        }
    }

    let config_path = match paths::server_config_path(server) {
        Some(p) => p,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    if config_path.exists() {
        println!(
            "{} {} is already installed.",
            "!".yellow().bold(),
            server.cyan()
        );
        return;
    }

    // Create the servers directory if needed
    if let Some(parent) = config_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!(
                "{} Failed to create directory {}: {}",
                "✗".red().bold(),
                parent.display(),
                e
            );
            process::exit(1);
        }
    }

    let installed = InstalledServer::from_metadata(meta);
    let toml_str = match toml::to_string_pretty(&installed) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} Failed to serialize config: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    if let Err(e) = fs::write(&config_path, &toml_str) {
        eprintln!("{} Failed to write config file: {}", "✗".red().bold(), e);
        process::exit(1);
    }

    println!(
        "{} Installed {} (v{}).",
        "✓".green().bold(),
        server.cyan(),
        meta.version
    );

    // Suggest berth config if there are required config fields
    if !meta.config.required.is_empty() {
        let keys: Vec<&str> = meta
            .config
            .required
            .iter()
            .map(|f| f.key.as_str())
            .collect();
        println!(
            "\n  This server requires configuration: {}",
            keys.join(", ").yellow()
        );
        println!(
            "  Run {} to configure it.",
            format!("berth config {server}").bold()
        );
    }
}

/// Parses `server` or `server@version` install specs.
fn parse_server_spec(spec: &str) -> Result<(&str, Option<&str>), String> {
    if let Some((server, version)) = spec.rsplit_once('@') {
        if server.is_empty() || version.is_empty() {
            return Err(
                "Invalid server format. Use `<server>` or `<server>@<version>`.".to_string(),
            );
        }
        return Ok((server, Some(version)));
    }
    Ok((spec, None))
}
