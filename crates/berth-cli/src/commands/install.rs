// SPDX-License-Identifier: Apache-2.0

//! Command handler for `berth install`.

use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::process::Command;

use berth_registry::config::InstalledServer;
use berth_registry::config::RuntimeInfo;
use berth_registry::types::ServerMetadata;
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

    let installed = match prepare_installed_server(server, meta) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };
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

/// Builds installed config from metadata and prepares runtime artifacts when needed.
fn prepare_installed_server(
    server: &str,
    meta: &ServerMetadata,
) -> Result<InstalledServer, String> {
    let mut installed = InstalledServer::from_metadata(meta);
    match installed.runtime.runtime_type.as_str() {
        "node" => Ok(installed),
        "python" => {
            ensure_python_runtime(&mut installed.runtime, &installed.source.package);
            Ok(installed)
        }
        "binary" => {
            let binary_path = install_binary_artifact(server, &installed.source.package)?;
            installed.runtime.command = binary_path.to_string_lossy().to_string();
            Ok(installed)
        }
        other => Err(format!(
            "Unsupported runtime type `{other}` for {}.",
            server.cyan()
        )),
    }
}

/// Ensures python runtimes default to `uvx <package>` when command/args are missing.
fn ensure_python_runtime(runtime: &mut RuntimeInfo, package: &str) {
    if runtime.command.trim().is_empty() {
        runtime.command = "uvx".to_string();
    }
    if runtime.args.is_empty() && !package.trim().is_empty() {
        runtime.args.push(package.to_string());
    }
}

/// Installs a binary artifact from local path/file URL/http URL into Berth's bin directory.
fn install_binary_artifact(server: &str, package: &str) -> Result<PathBuf, String> {
    let bin_dir =
        paths::berth_bin_dir().ok_or_else(|| "Could not determine home directory.".to_string())?;
    fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("failed to create {}: {e}", bin_dir.display()))?;

    let mut file_name = server.to_string();
    if cfg!(windows) && !file_name.to_ascii_lowercase().ends_with(".exe") {
        file_name.push_str(".exe");
    }
    let destination = bin_dir.join(file_name);

    if package.starts_with("http://") || package.starts_with("https://") {
        download_binary(package, &destination)?;
    } else {
        let source = package.strip_prefix("file://").unwrap_or(package);
        let source_path = Path::new(source);
        if !source_path.exists() {
            return Err(format!(
                "Binary source {} does not exist.",
                source_path.display()
            ));
        }
        fs::copy(source_path, &destination).map_err(|e| {
            format!(
                "failed to copy binary {} -> {}: {e}",
                source_path.display(),
                destination.display()
            )
        })?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&destination)
            .map_err(|e| {
                format!(
                    "failed to read binary metadata {}: {e}",
                    destination.display()
                )
            })?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&destination, perms).map_err(|e| {
            format!(
                "failed to set executable permissions on {}: {e}",
                destination.display()
            )
        })?;
    }

    Ok(destination)
}

/// Downloads a binary artifact using `curl` or `wget`.
fn download_binary(url: &str, destination: &Path) -> Result<(), String> {
    let destination_str = destination.to_string_lossy().to_string();
    let curl_status = Command::new("curl")
        .args(["-fsSL", "--max-time", "20", "-o", &destination_str, url])
        .status();
    if curl_status.is_ok_and(|s| s.success()) {
        return Ok(());
    }

    let wget_status = Command::new("wget")
        .args(["-q", "-O", &destination_str, url])
        .status();
    if wget_status.is_ok_and(|s| s.success()) {
        return Ok(());
    }

    Err(format!(
        "failed to download binary from {url} (curl/wget unavailable or request failed)"
    ))
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
