// SPDX-License-Identifier: Apache-2.0

//! Command handler for `berth search`.

use berth_registry::Registry;
use colored::Colorize;

/// Executes the `berth search` command.
pub fn execute(query: &str) {
    let registry = Registry::from_seed();
    let results = registry.search(query);

    if results.is_empty() {
        println!(
            "{} No servers found matching '{}'",
            "!".yellow().bold(),
            query
        );
        return;
    }

    println!(
        "{} Found {} server(s) matching '{}':\n",
        "✓".green().bold(),
        results.len(),
        query
    );

    // Header
    println!(
        "  {:<20} {:<50} {:<12} {:>10}",
        "NAME".bold(),
        "DESCRIPTION".bold(),
        "TRUST".bold(),
        "DOWNLOADS".bold(),
    );
    println!("  {}", "─".repeat(94));

    for result in &results {
        let server = result.server;
        let trust_colored = match server.trust_level {
            berth_registry::types::TrustLevel::Official => {
                server.trust_level.to_string().green().bold()
            }
            berth_registry::types::TrustLevel::Verified => server.trust_level.to_string().cyan(),
            berth_registry::types::TrustLevel::Community => server.trust_level.to_string().yellow(),
            berth_registry::types::TrustLevel::Untrusted => server.trust_level.to_string().red(),
        };

        let description = if server.description.len() > 48 {
            format!("{}...", &server.description[..45])
        } else {
            server.description.clone()
        };

        println!(
            "  {:<20} {:<50} {:<12} {:>10}",
            server.name.cyan(),
            description,
            trust_colored,
            format_downloads(server.quality.downloads),
        );
    }

    println!();
    println!(
        "  Run {} for details on a specific server.",
        "berth info <server>".bold()
    );
}

/// Formats a download counter with `K`/`M` suffixes for display.
fn format_downloads(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
