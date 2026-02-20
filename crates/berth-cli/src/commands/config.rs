use colored::Colorize;
use std::fs;
use std::process;

use berth_registry::config::InstalledServer;
use berth_registry::Registry;

use crate::paths;

pub fn execute(server: &str, set: Option<&str>, env: bool) {
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
        show_env(server);
        return;
    }

    if let Some(kv) = set {
        set_config(server, kv, &config_path);
        return;
    }

    show_config(server, &config_path);
}

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

    println!();
}

fn set_config(server: &str, kv: &str, config_path: &std::path::Path) {
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
            .contains(&key.to_string());

    if !is_known {
        eprintln!("{} Unknown config key: {}", "✗".red().bold(), key.cyan());
        let all_keys: Vec<&str> = installed
            .config_meta
            .required_keys
            .iter()
            .chain(installed.config_meta.optional_keys.iter())
            .map(|s| s.as_str())
            .collect();
        eprintln!("  Known keys: {}", all_keys.join(", "));
        process::exit(1);
    }

    installed.config.insert(key.to_string(), value.to_string());

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

    println!(
        "{} Set {} = {} for {}.",
        "✓".green().bold(),
        key.bold(),
        value,
        server.cyan()
    );
}

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
