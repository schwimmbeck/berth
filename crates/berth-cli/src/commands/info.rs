//! Command handler for `berth info`.

use berth_registry::types::TrustLevel;
use berth_registry::Registry;
use colored::Colorize;
use std::process;

/// Executes the `berth info` command.
pub fn execute(server_name: &str) {
    let registry = Registry::from_seed();

    let server = match registry.get(server_name) {
        Some(s) => s,
        None => {
            eprintln!(
                "{} Server '{}' not found in registry.",
                "✗".red().bold(),
                server_name
            );
            eprintln!(
                "  Run {} to search for available servers.",
                "berth search <query>".bold()
            );
            process::exit(1);
        }
    };

    // Header
    println!();
    println!(
        "  {} {}",
        server.display_name.bold(),
        format!("v{}", server.version).dimmed()
    );
    println!("  {}", server.description);
    println!();

    // Metadata
    println!("  {}", "Metadata".underline().bold());
    println!("  {:<18} {}", "Name:".dimmed(), server.name);
    println!("  {:<18} {}", "Category:".dimmed(), server.category);
    println!("  {:<18} {}", "Tags:".dimmed(), server.tags.join(", "));
    println!("  {:<18} {}", "Maintainer:".dimmed(), server.maintainer);
    println!(
        "  {:<18} {}",
        "Trust level:".dimmed(),
        match server.trust_level {
            TrustLevel::Official => server.trust_level.to_string().green().bold(),
            TrustLevel::Verified => server.trust_level.to_string().cyan(),
            TrustLevel::Community => server.trust_level.to_string().yellow(),
            TrustLevel::Untrusted => server.trust_level.to_string().red(),
        }
    );
    println!("  {:<18} {}", "Transport:".dimmed(), server.transport);
    println!();

    // Source
    println!("  {}", "Source".underline().bold());
    println!("  {:<18} {}", "Type:".dimmed(), server.source.source_type);
    println!("  {:<18} {}", "Package:".dimmed(), server.source.package);
    println!(
        "  {:<18} {}",
        "Repository:".dimmed(),
        server.source.repository
    );
    println!();

    // Runtime
    println!("  {}", "Runtime".underline().bold());
    println!("  {:<18} {}", "Type:".dimmed(), server.runtime.runtime_type);
    println!(
        "  {:<18} {} {}",
        "Command:".dimmed(),
        server.runtime.command,
        server.runtime.args.join(" ")
    );
    println!();

    // Permissions
    println!("  {}", "Permissions".underline().bold());
    if server.permissions.network.is_empty() {
        println!("  {:<18} {}", "Network:".dimmed(), "none".dimmed());
    } else {
        println!(
            "  {:<18} {}",
            "Network:".dimmed(),
            server.permissions.network.join(", ")
        );
    }
    if server.permissions.env.is_empty() {
        println!("  {:<18} {}", "Environment:".dimmed(), "none".dimmed());
    } else {
        println!(
            "  {:<18} {}",
            "Environment:".dimmed(),
            server.permissions.env.join(", ")
        );
    }
    println!();

    // Configuration
    println!("  {}", "Configuration".underline().bold());
    if server.config.required.is_empty() {
        println!("  {:<18} {}", "Required:".dimmed(), "none".dimmed());
    } else {
        println!("  {}:", "  Required".dimmed());
        for field in &server.config.required {
            let sensitive_tag = if field.sensitive {
                " (sensitive)".yellow().to_string()
            } else {
                String::new()
            };
            let env_tag = field
                .env
                .as_ref()
                .map(|e| format!(" [env: {}]", e).dimmed().to_string())
                .unwrap_or_default();
            println!(
                "    {} {}{}{}",
                "•".dimmed(),
                field.key.bold(),
                env_tag,
                sensitive_tag
            );
            println!("      {}", field.description);
        }
    }
    if !server.config.optional.is_empty() {
        println!("  {}:", "  Optional".dimmed());
        for field in &server.config.optional {
            let default_tag = field
                .default
                .as_ref()
                .map(|d| format!(" (default: {})", d).dimmed().to_string())
                .unwrap_or_default();
            println!("    {} {}{}", "•".dimmed(), field.key.bold(), default_tag);
            println!("      {}", field.description);
        }
    }
    println!();

    // Compatibility
    println!("  {}", "Compatibility".underline().bold());
    println!(
        "  {:<18} {}",
        "Clients:".dimmed(),
        server.compatibility.clients.join(", ")
    );
    println!(
        "  {:<18} {}",
        "Platforms:".dimmed(),
        server.compatibility.platforms.join(", ")
    );
    println!();

    // Quality
    println!("  {}", "Quality".underline().bold());
    let scan_colored = if server.quality.security_scan == "pass" {
        "pass".green().to_string()
    } else {
        server.quality.security_scan.red().to_string()
    };
    println!("  {:<18} {}", "Security scan:".dimmed(), scan_colored);
    println!(
        "  {:<18} {}",
        "Health check:".dimmed(),
        if server.quality.health_check {
            "yes".green()
        } else {
            "no".red()
        }
    );
    println!(
        "  {:<18} {}",
        "Last verified:".dimmed(),
        server.quality.last_verified
    );
    println!(
        "  {:<18} {}",
        "Downloads:".dimmed(),
        server.quality.downloads
    );
    println!();
}
