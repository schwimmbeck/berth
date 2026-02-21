//! Command handler for `berth permissions`.

use berth_registry::config::InstalledServer;
use colored::Colorize;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process;

use crate::paths;
use crate::permission_filter::{
    clear_permission_overrides, effective_permissions, load_permission_overrides,
    write_permission_overrides,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionExport {
    server: String,
    declared: ScopedPermissions,
    overrides: OverrideLists,
    effective: ScopedPermissions,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopedPermissions {
    network: Vec<String>,
    env: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverrideLists {
    grant: Vec<String>,
    revoke: Vec<String>,
}

/// Executes the `berth permissions` command.
pub fn execute(
    server: &str,
    grant: Option<&str>,
    revoke: Option<&str>,
    reset: bool,
    export_json: bool,
) {
    let action_count =
        grant.is_some() as u8 + revoke.is_some() as u8 + reset as u8 + export_json as u8;
    if action_count > 1 {
        eprintln!(
            "{} Use only one of {}, {}, {}, or {}.",
            "✗".red().bold(),
            "--grant".bold(),
            "--revoke".bold(),
            "--reset".bold(),
            "--export".bold()
        );
        process::exit(1);
    }

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

    let installed = match read_installed(&config_path) {
        Ok(i) => i,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    if let Some(perm) = grant {
        let mut overrides = match load_permission_overrides(server) {
            Ok(o) => o,
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        };
        upsert_permission(&mut overrides.grant, perm);
        remove_permission(&mut overrides.revoke, perm);
        if let Err(msg) = write_permission_overrides(server, &overrides) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
        println!(
            "{} Granted override {} for {}.",
            "✓".green().bold(),
            perm.bold(),
            server.cyan()
        );
        return;
    }

    if let Some(perm) = revoke {
        let mut overrides = match load_permission_overrides(server) {
            Ok(o) => o,
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        };
        upsert_permission(&mut overrides.revoke, perm);
        remove_permission(&mut overrides.grant, perm);
        if let Err(msg) = write_permission_overrides(server, &overrides) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
        println!(
            "{} Revoked override {} for {}.",
            "✓".green().bold(),
            perm.bold(),
            server.cyan()
        );
        return;
    }

    if reset {
        if let Err(msg) = clear_permission_overrides(server) {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
        println!(
            "{} Cleared permission overrides for {}.",
            "✓".green().bold(),
            server.cyan()
        );
        return;
    }

    let overrides = match load_permission_overrides(server) {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };
    let declared = declared_permissions(&installed);
    let effective_network =
        effective_permissions("network", &installed.permissions.network, &overrides);
    let effective_env = effective_permissions("env", &installed.permissions.env, &overrides);

    if export_json {
        let export = PermissionExport {
            server: server.to_string(),
            declared: ScopedPermissions {
                network: installed.permissions.network.clone(),
                env: installed.permissions.env.clone(),
            },
            overrides: OverrideLists {
                grant: overrides.grant.clone(),
                revoke: overrides.revoke.clone(),
            },
            effective: ScopedPermissions {
                network: effective_network,
                env: effective_env,
            },
        };
        let rendered = match serde_json::to_string_pretty(&export) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "{} Failed to serialize export JSON: {}",
                    "✗".red().bold(),
                    e
                );
                process::exit(1);
            }
        };
        println!("{rendered}");
        return;
    }

    println!(
        "{} Permissions for {}:\n",
        "✓".green().bold(),
        server.cyan()
    );
    println!("  {}", "Declared".bold());
    if declared.is_empty() {
        println!("    {}", "none".dimmed());
    } else {
        for perm in &declared {
            println!("    {}", perm);
        }
    }

    println!();
    println!("  {}", "Overrides".bold());
    println!(
        "    {} {}",
        "grant:".dimmed(),
        format_list(&overrides.grant)
    );
    println!(
        "    {} {}",
        "revoke:".dimmed(),
        format_list(&overrides.revoke)
    );

    println!();
    println!("  {}", "Effective".bold());
    println!(
        "    {} {}",
        "network:".dimmed(),
        format_scoped("network", &effective_network)
    );
    println!(
        "    {} {}",
        "env:".dimmed(),
        format_scoped("env", &effective_env)
    );
}

/// Reads and parses an installed server config file.
fn read_installed(path: &Path) -> Result<InstalledServer, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read config: {e}"))?;
    toml::from_str::<InstalledServer>(&content).map_err(|e| format!("Failed to parse config: {e}"))
}

/// Returns declared permissions from installed metadata.
fn declared_permissions(installed: &InstalledServer) -> Vec<String> {
    let mut out = Vec::new();
    for n in &installed.permissions.network {
        out.push(format!("network:{n}"));
    }
    for e in &installed.permissions.env {
        out.push(format!("env:{e}"));
    }
    out
}

/// Appends a permission if it is not already present.
fn upsert_permission(perms: &mut Vec<String>, perm: &str) {
    if !perms.iter().any(|p| p == perm) {
        perms.push(perm.to_string());
        perms.sort();
    }
}

/// Formats effective scoped permissions (e.g. `env:FOO`).
fn format_scoped(scope: &str, perms: &[String]) -> String {
    if perms.is_empty() {
        return "none".dimmed().to_string();
    }
    perms
        .iter()
        .map(|p| format!("{scope}:{p}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Removes one permission from the list if present.
fn remove_permission(perms: &mut Vec<String>, perm: &str) {
    perms.retain(|p| p != perm);
}

/// Formats permissions for compact display.
fn format_list(perms: &[String]) -> String {
    if perms.is_empty() {
        "none".dimmed().to_string()
    } else {
        perms.join(", ")
    }
}
