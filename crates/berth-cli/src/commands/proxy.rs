//! Command handler for `berth proxy`.

use colored::Colorize;

/// Executes the `berth proxy` command.
pub fn execute(server: &str) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth proxy".bold()
    );
    println!("  Would start MCP proxy for server: {}", server.cyan());
}
