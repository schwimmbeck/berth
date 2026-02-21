//! Command handler for `berth permissions`.

use colored::Colorize;

/// Executes the `berth permissions` command.
pub fn execute(server: &str, grant: Option<&str>, revoke: Option<&str>) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth permissions".bold()
    );
    println!("  Server: {}", server.cyan());
    if let Some(perm) = grant {
        println!("  Would grant permission: {}", perm);
    }
    if let Some(perm) = revoke {
        println!("  Would revoke permission: {}", perm);
    }
    if grant.is_none() && revoke.is_none() {
        println!("  Would show permissions for this server.");
    }
}
