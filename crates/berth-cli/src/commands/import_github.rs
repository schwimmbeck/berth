//! Command handler for `berth import-github`.

use colored::Colorize;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{self, Command};

use berth_registry::config::{
    ConfigMeta, InstalledServer, PermissionsInfo, RuntimeInfo, ServerInfo, SourceInfo,
};

use crate::paths;
use crate::permission_filter::validate_permission_syntax;

/// Executes the `berth import-github` command.
pub fn execute(repo: &str, git_ref: &str, manifest_path: &str, dry_run: bool) {
    let parsed = match parse_repo_identifier(repo) {
        Ok(parsed) => parsed,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };
    let fetch = match fetch_manifest(&parsed.owner, &parsed.repo, git_ref, manifest_path) {
        Ok(fetch) => fetch,
        Err(msg) => {
            eprintln!("{} {}", "✗".red().bold(), msg);
            process::exit(1);
        }
    };
    let manifest: GithubManifest = match toml::from_str(&fetch.content) {
        Ok(manifest) => manifest,
        Err(e) => {
            eprintln!(
                "{} Failed to parse manifest from {}: {}",
                "✗".red().bold(),
                fetch.source.cyan(),
                e
            );
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

    if dry_run {
        println!(
            "{} Valid GitHub manifest for {} from {}.",
            "✓".green().bold(),
            manifest.server.name.cyan(),
            fetch.source
        );
        return;
    }

    let config_path = match paths::server_config_path(&manifest.server.name) {
        Some(path) => path,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    if config_path.exists() {
        println!(
            "{} {} is already imported/installed.",
            "!".yellow().bold(),
            manifest.server.name.cyan()
        );
        return;
    }
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

    let installed = to_installed_server(&manifest);
    let rendered = match toml::to_string_pretty(&installed) {
        Ok(rendered) => rendered,
        Err(e) => {
            eprintln!(
                "{} Failed to serialize imported config: {}",
                "✗".red().bold(),
                e
            );
            process::exit(1);
        }
    };
    if let Err(e) = fs::write(&config_path, rendered) {
        eprintln!(
            "{} Failed to write imported server config {}: {}",
            "✗".red().bold(),
            config_path.display(),
            e
        );
        process::exit(1);
    }

    println!(
        "{} Imported {} from {}.",
        "✓".green().bold(),
        manifest.server.name.cyan(),
        fetch.source
    );
}

#[derive(Debug)]
struct ParsedRepo {
    owner: String,
    repo: String,
}

#[derive(Debug)]
struct FetchResult {
    source: String,
    content: String,
}

fn parse_repo_identifier(input: &str) -> Result<ParsedRepo, String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Repository identifier must not be empty.".to_string());
    }

    let normalized = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .unwrap_or(trimmed)
        .trim_start_matches('/');
    let normalized = normalized.strip_suffix(".git").unwrap_or(normalized);

    let mut parts = normalized.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err(format!(
            "Invalid repository `{input}`. Use `owner/repo` or `https://github.com/owner/repo`."
        ));
    }

    if !is_simple_repo_segment(owner) || !is_simple_repo_segment(repo) {
        return Err(format!(
            "Invalid repository `{input}`. Owner and repo may only contain letters, digits, `_`, `-`, and `.`."
        ));
    }

    Ok(ParsedRepo {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

fn is_simple_repo_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn fetch_manifest(
    owner: &str,
    repo: &str,
    git_ref: &str,
    manifest_path: &str,
) -> Result<FetchResult, String> {
    let manifest_path = normalize_manifest_path(manifest_path)?;
    let raw_base = std::env::var("BERTH_GITHUB_RAW_BASE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://raw.githubusercontent.com".to_string());

    if let Some(local_base) = raw_base.strip_prefix("file://") {
        let path = Path::new(local_base)
            .join(owner)
            .join(repo)
            .join(git_ref)
            .join(&manifest_path);
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read local manifest {}: {e}", path.display()))?;
        return Ok(FetchResult {
            source: format!("file://{}", path.display()),
            content,
        });
    }

    let source_url = format!(
        "{}/{}/{}/{}/{}",
        raw_base.trim_end_matches('/'),
        owner,
        repo,
        git_ref,
        manifest_path
    );
    if let Ok(content) = fetch_url_text(&source_url) {
        return Ok(FetchResult {
            source: source_url,
            content,
        });
    }

    if git_ref == "main" {
        let fallback_url = format!(
            "{}/{}/{}/{}/{}",
            raw_base.trim_end_matches('/'),
            owner,
            repo,
            "master",
            manifest_path
        );
        if let Ok(content) = fetch_url_text(&fallback_url) {
            return Ok(FetchResult {
                source: fallback_url,
                content,
            });
        }
    }

    Err(format!(
        "Failed to fetch {manifest_path} from {owner}/{repo}@{git_ref}. Set BERTH_GITHUB_RAW_BASE to a reachable raw-content base URL or file:// path."
    ))
}

fn normalize_manifest_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("Manifest path must not be empty.".to_string());
    }
    if trimmed.split('/').any(|segment| segment == "..") {
        return Err("Manifest path must not contain `..` segments.".to_string());
    }
    Ok(trimmed.to_string())
}

fn fetch_url_text(url: &str) -> Result<String, String> {
    let curl_output = Command::new("curl")
        .args(["-fsSL", "--max-time", "10", url])
        .output();
    if let Ok(out) = curl_output {
        if out.status.success() {
            return String::from_utf8(out.stdout).map_err(|e| format!("Non-UTF-8 response: {e}"));
        }
    }

    let wget_output = Command::new("wget")
        .args(["-q", "-O", "-", "--timeout=10", url])
        .output();
    if let Ok(out) = wget_output {
        if out.status.success() {
            return String::from_utf8(out.stdout).map_err(|e| format!("Non-UTF-8 response: {e}"));
        }
    }

    Err(format!(
        "Network fetch failed for {url} (curl/wget unavailable or request failed)"
    ))
}

fn validate_manifest(manifest: &GithubManifest) -> Vec<String> {
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

    for entry in &manifest.permissions.network {
        if let Err(err) = validate_permission_syntax(&format!("network:{entry}")) {
            errors.push(err);
        }
    }
    for entry in &manifest.permissions.env {
        if let Err(err) = validate_permission_syntax(&format!("env:{entry}")) {
            errors.push(err);
        }
    }
    for entry in &manifest.permissions.filesystem {
        if let Err(err) = validate_permission_syntax(&format!("filesystem:{entry}")) {
            errors.push(err);
        }
    }
    for entry in &manifest.permissions.exec {
        if let Err(err) = validate_permission_syntax(&format!("exec:{entry}")) {
            errors.push(err);
        }
    }

    let mut config_keys = BTreeSet::new();
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

    if manifest.compatibility.clients.is_empty() {
        errors.push("compatibility.clients must include at least one client.".to_string());
    }
    if manifest.compatibility.platforms.is_empty() {
        errors.push("compatibility.platforms must include at least one platform.".to_string());
    }
    if !matches!(
        manifest.server.trust_level.as_str(),
        "untrusted" | "community" | "verified" | "official"
    ) {
        errors.push(
            "server.trust_level must be one of: untrusted, community, verified, official."
                .to_string(),
        );
    }

    errors
}

fn to_installed_server(manifest: &GithubManifest) -> InstalledServer {
    let mut config = BTreeMap::new();
    let required_keys: Vec<String> = manifest
        .config
        .required
        .iter()
        .map(|f| {
            config.insert(f.key.clone(), String::new());
            f.key.clone()
        })
        .collect();
    let optional_keys: Vec<String> = manifest
        .config
        .optional
        .iter()
        .map(|f| {
            config.insert(f.key.clone(), f.default.clone().unwrap_or_default());
            f.key.clone()
        })
        .collect();

    InstalledServer {
        server: ServerInfo {
            name: manifest.server.name.clone(),
            display_name: manifest.server.display_name.clone(),
            version: manifest.server.version.clone(),
            description: manifest.server.description.clone(),
            category: manifest.server.category.clone(),
            maintainer: manifest.server.maintainer.clone(),
            trust_level: manifest.server.trust_level.clone(),
        },
        source: SourceInfo {
            source_type: manifest.source.source_type.clone(),
            package: manifest.source.package.clone(),
            repository: manifest.source.repository.clone(),
        },
        runtime: RuntimeInfo {
            runtime_type: manifest.runtime.runtime_type.clone(),
            command: manifest.runtime.command.clone(),
            args: manifest.runtime.args.clone(),
            transport: manifest.runtime.transport.clone(),
        },
        permissions: PermissionsInfo {
            network: manifest.permissions.network.clone(),
            env: manifest.permissions.env.clone(),
            filesystem: manifest.permissions.filesystem.clone(),
            exec: manifest.permissions.exec.clone(),
        },
        config,
        config_meta: ConfigMeta {
            required_keys,
            optional_keys,
        },
    }
}

fn validate_non_empty(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} is required."));
    }
}

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

#[derive(Debug, Clone, Deserialize)]
struct GithubManifest {
    server: ManifestServer,
    source: ManifestSource,
    runtime: ManifestRuntime,
    #[serde(default)]
    permissions: ManifestPermissions,
    #[serde(default)]
    config: ManifestConfig,
    compatibility: ManifestCompatibility,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestServer {
    name: String,
    display_name: String,
    description: String,
    version: String,
    category: String,
    maintainer: String,
    trust_level: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestSource {
    #[serde(rename = "type")]
    source_type: String,
    package: String,
    repository: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestRuntime {
    #[serde(rename = "type")]
    runtime_type: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    transport: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
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

#[derive(Debug, Clone, Default, Deserialize)]
struct ManifestConfig {
    #[serde(default)]
    required: Vec<ManifestConfigField>,
    #[serde(default)]
    optional: Vec<ManifestConfigField>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestConfigField {
    key: String,
    #[allow(dead_code)]
    #[serde(default)]
    env: Option<String>,
    description: String,
    #[allow(dead_code)]
    #[serde(default)]
    sensitive: bool,
    #[serde(default)]
    default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestCompatibility {
    #[serde(default)]
    clients: Vec<String>,
    #[serde(default)]
    platforms: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_repo_identifier_accepts_owner_repo_and_url() {
        let a = parse_repo_identifier("acme/mcp-demo").unwrap();
        assert_eq!(a.owner, "acme");
        assert_eq!(a.repo, "mcp-demo");

        let b = parse_repo_identifier("https://github.com/acme/mcp-demo").unwrap();
        assert_eq!(b.owner, "acme");
        assert_eq!(b.repo, "mcp-demo");
    }

    #[test]
    fn parse_repo_identifier_rejects_invalid_values() {
        assert!(parse_repo_identifier("").is_err());
        assert!(parse_repo_identifier("acme").is_err());
        assert!(parse_repo_identifier("a/b/c").is_err());
    }

    #[test]
    fn validate_manifest_rejects_invalid_permission_and_transport() {
        let manifest: GithubManifest = toml::from_str(
            r#"
[server]
name = "demo"
display_name = "Demo"
description = "Demo"
version = "1.0.0"
category = "developer-tools"
maintainer = "Acme"
trust_level = "community"

[source]
type = "npm"
package = "demo"
repository = "https://github.com/acme/demo"

[runtime]
type = "node"
command = "npx"
args = ["demo"]
transport = "http"

[permissions]
env = ["bad-var"]

[compatibility]
clients = ["claude-desktop"]
platforms = ["macos"]
"#,
        )
        .unwrap();

        let errors = validate_manifest(&manifest);
        assert!(errors.iter().any(|e| e.contains("runtime.transport")));
        assert!(errors
            .iter()
            .any(|e| e.contains("Invalid permission format")));
    }

    #[test]
    fn fetch_manifest_supports_file_scheme() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp
            .path()
            .join("acme")
            .join("demo")
            .join("main")
            .join("berth.toml");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(
            &file,
            r#"
[server]
name = "demo"
display_name = "Demo"
description = "Demo"
version = "1.0.0"
category = "developer-tools"
maintainer = "Acme"
trust_level = "community"
[source]
type = "npm"
package = "demo"
repository = "https://github.com/acme/demo"
[runtime]
type = "node"
command = "npx"
args = ["demo"]
transport = "stdio"
[compatibility]
clients = ["claude-desktop"]
platforms = ["linux"]
"#,
        )
        .unwrap();

        let old = std::env::var("BERTH_GITHUB_RAW_BASE").ok();
        std::env::set_var(
            "BERTH_GITHUB_RAW_BASE",
            format!("file://{}", tmp.path().display()),
        );
        let result = fetch_manifest("acme", "demo", "main", "berth.toml").unwrap();
        assert!(result.content.contains("display_name"));
        if let Some(value) = old {
            std::env::set_var("BERTH_GITHUB_RAW_BASE", value);
        } else {
            std::env::remove_var("BERTH_GITHUB_RAW_BASE");
        }
    }
}
