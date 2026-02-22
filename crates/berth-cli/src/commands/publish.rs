// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Command handler for `berth publish`.

use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths;
use crate::permission_filter::validate_permission_syntax;

/// Executes the `berth publish` command.
pub fn execute(manifest_path: Option<&str>, dry_run: bool) {
    let manifest_path = manifest_path.unwrap_or("berth.toml");
    let manifest = match load_manifest(manifest_path) {
        Ok(manifest) => manifest,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    let validation_errors = validate_manifest(&manifest);
    if !validation_errors.is_empty() {
        eprintln!("{} Manifest validation failed:", "✗".red().bold());
        for err in validation_errors {
            eprintln!("  - {}", err);
        }
        process::exit(1);
    }

    let checks = run_quality_checks(&manifest);
    print_quality_checks(&checks);
    let has_failed = checks.iter().any(|check| !check.passed);
    if has_failed {
        eprintln!(
            "{} Publish blocked: one or more quality checks failed.",
            "✗".red().bold()
        );
        process::exit(1);
    }

    if dry_run {
        println!(
            "{} Manifest {} passed validation and quality checks (dry-run).",
            "✓".green().bold(),
            manifest_path.cyan()
        );
        return;
    }

    let output_path = match write_submission(&manifest, &checks) {
        Ok(path) => path,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };

    println!(
        "{} Submitted {} for manual registry review.",
        "✓".green().bold(),
        manifest.server.name.cyan()
    );
    println!("  Queue entry: {}", output_path.display());
}

/// Loads and parses a publish manifest from disk.
fn load_manifest(path: &str) -> Result<PublishManifest, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read manifest `{path}`: {e}"))?;
    toml::from_str::<PublishManifest>(&content)
        .map_err(|e| format!("Failed to parse manifest `{path}`: {e}"))
}

/// Validates manifest structure and semantics.
fn validate_manifest(manifest: &PublishManifest) -> Vec<String> {
    let mut errors = Vec::new();

    validate_non_empty("server.name", &manifest.server.name, &mut errors);
    validate_non_empty(
        "server.display_name",
        &manifest.server.display_name,
        &mut errors,
    );
    validate_non_empty(
        "server.description",
        &manifest.server.description,
        &mut errors,
    );
    validate_non_empty("server.version", &manifest.server.version, &mut errors);
    validate_non_empty("server.category", &manifest.server.category, &mut errors);
    validate_non_empty(
        "server.maintainer",
        &manifest.server.maintainer,
        &mut errors,
    );
    validate_non_empty(
        "server.trust_level",
        &manifest.server.trust_level,
        &mut errors,
    );

    if !manifest
        .server
        .name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        errors.push("server.name must use lowercase letters, digits, and dashes only.".to_string());
    }

    if !is_basic_semver(&manifest.server.version) {
        errors.push("server.version must look like semantic version `x.y.z`.".to_string());
    }

    validate_non_empty("source.type", &manifest.source.source_type, &mut errors);
    validate_non_empty("source.package", &manifest.source.package, &mut errors);
    validate_non_empty(
        "source.repository",
        &manifest.source.repository,
        &mut errors,
    );

    validate_non_empty("runtime.type", &manifest.runtime.runtime_type, &mut errors);
    validate_non_empty("runtime.command", &manifest.runtime.command, &mut errors);
    if manifest.runtime.transport.trim() != "stdio" {
        errors.push("runtime.transport must be `stdio`.".to_string());
    }

    let mut config_keys = std::collections::BTreeSet::new();
    for field in &manifest.config.required {
        validate_non_empty("config.required[].key", &field.key, &mut errors);
        validate_non_empty(
            "config.required[].description",
            &field.description,
            &mut errors,
        );
        if !field.key.trim().is_empty() && !config_keys.insert(field.key.clone()) {
            errors.push(format!("Duplicate config key `{}`.", field.key));
        }
    }
    for field in &manifest.config.optional {
        validate_non_empty("config.optional[].key", &field.key, &mut errors);
        validate_non_empty(
            "config.optional[].description",
            &field.description,
            &mut errors,
        );
        if !field.key.trim().is_empty() && !config_keys.insert(field.key.clone()) {
            errors.push(format!("Duplicate config key `{}`.", field.key));
        }
    }

    for permission in &manifest.permissions.network {
        if let Err(err) = validate_permission_syntax(&format!("network:{permission}")) {
            errors.push(err);
        }
    }
    for permission in &manifest.permissions.env {
        if let Err(err) = validate_permission_syntax(&format!("env:{permission}")) {
            errors.push(err);
        }
    }
    for permission in &manifest.permissions.filesystem {
        if let Err(err) = validate_permission_syntax(&format!("filesystem:{permission}")) {
            errors.push(err);
        }
    }
    for permission in &manifest.permissions.exec {
        if let Err(err) = validate_permission_syntax(&format!("exec:{permission}")) {
            errors.push(err);
        }
    }

    if manifest.compatibility.clients.is_empty() {
        errors.push("compatibility.clients must include at least one client.".to_string());
    }
    if manifest.compatibility.platforms.is_empty() {
        errors.push("compatibility.platforms must include at least one platform.".to_string());
    }

    let trust_level = manifest.server.trust_level.as_str();
    if !matches!(
        trust_level,
        "untrusted" | "community" | "verified" | "official"
    ) {
        errors.push(
            "server.trust_level must be one of: untrusted, community, verified, official."
                .to_string(),
        );
    }

    errors
}

/// Runs deterministic publish quality checks.
fn run_quality_checks(manifest: &PublishManifest) -> Vec<QualityCheck> {
    let mut checks = Vec::new();
    let declared_permissions = manifest.permissions.network.len()
        + manifest.permissions.env.len()
        + manifest.permissions.filesystem.len()
        + manifest.permissions.exec.len();
    checks.push(QualityCheck {
        name: "permissions-declared".to_string(),
        passed: declared_permissions > 0,
        detail: if declared_permissions > 0 {
            format!("{declared_permissions} permission entries declared")
        } else {
            "at least one permission declaration is required".to_string()
        },
    });

    checks.push(QualityCheck {
        name: "runtime-args".to_string(),
        passed: !manifest.runtime.args.is_empty(),
        detail: if manifest.runtime.args.is_empty() {
            "runtime.args should not be empty for reproducible startup".to_string()
        } else {
            format!("{} runtime args declared", manifest.runtime.args.len())
        },
    });

    let docs_ok = manifest
        .config
        .required
        .iter()
        .chain(manifest.config.optional.iter())
        .all(|field| !field.description.trim().is_empty());
    checks.push(QualityCheck {
        name: "config-docs".to_string(),
        passed: docs_ok,
        detail: if docs_ok {
            "all config keys include descriptions".to_string()
        } else {
            "all config keys must include non-empty descriptions".to_string()
        },
    });

    let scan = manifest.quality.security_scan.to_ascii_lowercase();
    checks.push(QualityCheck {
        name: "security-scan".to_string(),
        passed: scan != "failed",
        detail: if scan == "failed" {
            "security_scan is marked failed".to_string()
        } else {
            format!("security_scan={}", manifest.quality.security_scan)
        },
    });

    checks
}

/// Prints quality check outcomes for user feedback.
fn print_quality_checks(checks: &[QualityCheck]) {
    println!("{}", "Quality checks:".bold());
    for check in checks {
        if check.passed {
            println!("  {} {} ({})", "✓".green().bold(), check.name, check.detail);
        } else {
            println!("  {} {} ({})", "✗".red().bold(), check.name, check.detail);
        }
    }
}

/// Writes a publish submission artifact into Berth's local review queue.
fn write_submission(
    manifest: &PublishManifest,
    checks: &[QualityCheck],
) -> Result<std::path::PathBuf, String> {
    let queue_dir =
        paths::publish_queue_dir().ok_or("Could not determine home directory.".to_string())?;
    fs::create_dir_all(&queue_dir)
        .map_err(|e| format!("Failed to create publish queue directory: {e}"))?;

    let file_name = format!(
        "{}-{}.json",
        sanitize_name(&manifest.server.name),
        now_epoch_secs()
    );
    let path = queue_dir.join(file_name);
    let submission = PublishSubmission {
        submitted_at_epoch_secs: now_epoch_secs(),
        status: "pending-manual-review".to_string(),
        manifest: manifest.clone(),
        quality_checks: checks.to_vec(),
    };
    let payload = serde_json::to_string_pretty(&submission)
        .map_err(|e| format!("Failed to serialize submission payload: {e}"))?;
    fs::write(&path, payload).map_err(|e| format!("Failed to write submission: {e}"))?;
    Ok(path)
}

/// Returns current unix timestamp in seconds.
fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Replaces non-safe filename characters with '-'.
fn sanitize_name(name: &str) -> String {
    let out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if out.is_empty() {
        "submission".to_string()
    } else {
        out
    }
}

/// Pushes a non-empty validation error when `value` is blank.
fn validate_non_empty(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} is required."));
    }
}

/// Performs a lightweight semantic-version check (`x.y.z`).
fn is_basic_semver(version: &str) -> bool {
    let mut parts = version.split('.');
    let (Some(a), Some(b), Some(c), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !a.is_empty()
        && !b.is_empty()
        && !c.is_empty()
        && a.chars().all(|ch| ch.is_ascii_digit())
        && b.chars().all(|ch| ch.is_ascii_digit())
        && c.chars().all(|ch| ch.is_ascii_digit())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PublishSubmission {
    submitted_at_epoch_secs: u64,
    status: String,
    manifest: PublishManifest,
    quality_checks: Vec<QualityCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QualityCheck {
    name: String,
    passed: bool,
    detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PublishManifest {
    server: ManifestServer,
    source: ManifestSource,
    runtime: ManifestRuntime,
    #[serde(default)]
    permissions: ManifestPermissions,
    #[serde(default)]
    config: ManifestConfig,
    compatibility: ManifestCompatibility,
    #[serde(default)]
    quality: ManifestQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestServer {
    name: String,
    display_name: String,
    description: String,
    version: String,
    category: String,
    maintainer: String,
    trust_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestSource {
    #[serde(rename = "type")]
    source_type: String,
    package: String,
    repository: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestRuntime {
    #[serde(rename = "type")]
    runtime_type: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    transport: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ManifestPermissions {
    #[serde(default)]
    network: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    filesystem: Vec<String>,
    #[serde(default)]
    exec: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ManifestConfig {
    #[serde(default)]
    required: Vec<ManifestConfigField>,
    #[serde(default)]
    optional: Vec<ManifestConfigField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestConfigField {
    key: String,
    #[serde(default)]
    env: Option<String>,
    description: String,
    #[serde(default)]
    sensitive: bool,
    #[serde(default)]
    default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestCompatibility {
    #[serde(default)]
    clients: Vec<String>,
    #[serde(default)]
    platforms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestQuality {
    #[serde(default = "default_security_scan")]
    security_scan: String,
    #[serde(default)]
    health_check: bool,
    #[serde(default)]
    last_verified: String,
    #[serde(default)]
    downloads: u64,
}

impl Default for ManifestQuality {
    fn default() -> Self {
        ManifestQuality {
            security_scan: default_security_scan(),
            health_check: false,
            last_verified: String::new(),
            downloads: 0,
        }
    }
}

fn default_security_scan() -> String {
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> PublishManifest {
        PublishManifest {
            server: ManifestServer {
                name: "acme-github".to_string(),
                display_name: "Acme GitHub MCP Server".to_string(),
                description: "Acme MCP server".to_string(),
                version: "1.0.0".to_string(),
                category: "developer-tools".to_string(),
                maintainer: "Acme".to_string(),
                trust_level: "community".to_string(),
            },
            source: ManifestSource {
                source_type: "npm".to_string(),
                package: "@acme/mcp-github".to_string(),
                repository: "https://github.com/acme/mcp-github".to_string(),
            },
            runtime: ManifestRuntime {
                runtime_type: "node".to_string(),
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "@acme/mcp-github".to_string()],
                transport: "stdio".to_string(),
            },
            permissions: ManifestPermissions {
                network: vec!["api.github.com:443".to_string()],
                env: vec!["GITHUB_TOKEN".to_string()],
                filesystem: vec!["read:/workspace".to_string()],
                exec: vec!["git".to_string()],
            },
            config: ManifestConfig {
                required: vec![ManifestConfigField {
                    key: "token".to_string(),
                    env: Some("GITHUB_TOKEN".to_string()),
                    description: "API token".to_string(),
                    sensitive: true,
                    default: None,
                }],
                optional: Vec::new(),
            },
            compatibility: ManifestCompatibility {
                clients: vec!["claude-desktop".to_string()],
                platforms: vec!["macos".to_string()],
            },
            quality: ManifestQuality {
                security_scan: "passed".to_string(),
                health_check: true,
                last_verified: "2026-02-21".to_string(),
                downloads: 0,
            },
        }
    }

    #[test]
    fn validate_manifest_accepts_valid_input() {
        let manifest = valid_manifest();
        let errors = validate_manifest(&manifest);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_manifest_rejects_bad_permissions_and_transport() {
        let mut manifest = valid_manifest();
        manifest.runtime.transport = "http".to_string();
        manifest.permissions.env = vec!["bad-var".to_string()];
        let errors = validate_manifest(&manifest);
        assert!(errors.iter().any(|e| e.contains("runtime.transport")));
        assert!(errors
            .iter()
            .any(|e| e.contains("Invalid permission format")));
    }

    #[test]
    fn quality_checks_fail_when_permissions_missing() {
        let mut manifest = valid_manifest();
        manifest.permissions = ManifestPermissions::default();
        let checks = run_quality_checks(&manifest);
        assert!(checks
            .iter()
            .any(|c| c.name == "permissions-declared" && !c.passed));
    }
}
