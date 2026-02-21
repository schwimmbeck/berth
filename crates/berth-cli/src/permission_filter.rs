//! Shared permission override and effective-permission helpers.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use crate::paths;

/// Prefix used in user-facing errors for network-permission launch denials.
pub const NETWORK_PERMISSION_DENIED_PREFIX: &str = "Network permission denied";

/// User-managed permission overrides for a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionOverrides {
    #[serde(default)]
    pub grant: Vec<String>,
    #[serde(default)]
    pub revoke: Vec<String>,
}

/// Loads permission overrides from disk if present.
pub fn load_permission_overrides(server: &str) -> Result<PermissionOverrides, String> {
    let path =
        paths::permissions_override_path(server).ok_or("Could not determine home directory.")?;
    if !path.exists() {
        return Ok(PermissionOverrides::default());
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read overrides: {e}"))?;
    toml::from_str::<PermissionOverrides>(&content)
        .map_err(|e| format!("Failed to parse overrides: {e}"))
}

/// Persists permission overrides for a server.
pub fn write_permission_overrides(
    server: &str,
    overrides: &PermissionOverrides,
) -> Result<(), String> {
    let path =
        paths::permissions_override_path(server).ok_or("Could not determine home directory.")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    let rendered = toml::to_string_pretty(overrides)
        .map_err(|e| format!("Failed to serialize overrides: {e}"))?;
    fs::write(path, rendered).map_err(|e| format!("Failed to write overrides: {e}"))
}

/// Clears persisted permission overrides for a server.
pub fn clear_permission_overrides(server: &str) -> Result<(), String> {
    let path =
        paths::permissions_override_path(server).ok_or("Could not determine home directory.")?;
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(path).map_err(|e| format!("Failed to clear overrides: {e}"))
}

/// Validates one user-supplied permission string.
pub fn validate_permission_syntax(permission: &str) -> Result<(), String> {
    if let Some(value) = permission.strip_prefix("env:") {
        return validate_env_permission(value, permission);
    }
    if let Some(value) = permission.strip_prefix("network:") {
        return validate_network_permission(value, permission);
    }
    if let Some(value) = permission.strip_prefix("filesystem:") {
        return validate_filesystem_permission(value, permission);
    }
    if let Some(value) = permission.strip_prefix("exec:") {
        return validate_exec_permission(value, permission);
    }
    Err(format!(
        "Invalid permission format `{permission}`. Use `env:<VAR>`, `env:*`, `network:<host>:<port>`, `network:*`, `filesystem:<read|write>:<path>`, `filesystem:*`, `exec:<command>`, or `exec:*`."
    ))
}

/// Computes effective permissions of one prefix (`env` or `network`).
pub fn effective_permissions(
    prefix: &str,
    declared: &[String],
    overrides: &PermissionOverrides,
) -> Vec<String> {
    let mut values: BTreeSet<String> = declared.iter().cloned().collect();
    let mut allow_all = false;
    let prefix_with_colon = format!("{prefix}:");

    for perm in &overrides.grant {
        if let Some(value) = perm.strip_prefix(&prefix_with_colon) {
            if value == "*" {
                allow_all = true;
            } else {
                values.insert(value.to_string());
            }
        }
    }

    for perm in &overrides.revoke {
        if let Some(value) = perm.strip_prefix(&prefix_with_colon) {
            if value == "*" {
                allow_all = false;
                values.clear();
            } else {
                values.remove(value);
            }
        }
    }

    if allow_all {
        return vec!["*".to_string()];
    }
    values.into_iter().collect()
}

/// Filters an env map in-place based on declared and overridden env permissions.
pub fn filter_env_map(
    env: &mut BTreeMap<String, String>,
    declared_env: &[String],
    overrides: &PermissionOverrides,
) {
    let effective = effective_permissions("env", declared_env, overrides);
    if effective.iter().any(|e| e == "*") {
        return;
    }

    let allowed: BTreeSet<String> = effective.into_iter().collect();
    env.retain(|k, _| allowed.contains(k));
}

/// Validates that declared network access is not fully revoked by overrides.
pub fn validate_network_permissions(
    server: &str,
    declared_network: &[String],
    overrides: &PermissionOverrides,
) -> Result<(), String> {
    if declared_network.is_empty() {
        return Ok(());
    }

    let effective = effective_permissions("network", declared_network, overrides);
    if effective.is_empty() {
        return Err(format!(
            "{NETWORK_PERMISSION_DENIED_PREFIX} for {server}: effective network permissions are empty."
        ));
    }
    Ok(())
}

/// Returns override-granted network permissions that are not declared by the server.
pub fn undeclared_network_grants(
    declared_network: &[String],
    overrides: &PermissionOverrides,
) -> Vec<String> {
    let declared: BTreeSet<&str> = declared_network.iter().map(String::as_str).collect();
    let mut out: BTreeSet<String> = BTreeSet::new();

    for grant in &overrides.grant {
        if let Some(value) = grant.strip_prefix("network:") {
            if value == "*" || !declared.contains(value) {
                out.insert(value.to_string());
            }
        }
    }

    out.into_iter().collect()
}

fn validate_env_permission(value: &str, original: &str) -> Result<(), String> {
    if value == "*" {
        return Ok(());
    }
    if value.is_empty() {
        return Err(format!(
            "Invalid permission format `{original}`. Environment variable name is required."
        ));
    }
    let mut chars = value.chars();
    let first = chars.next().unwrap_or_default();
    if !(first.is_ascii_uppercase() || first == '_') {
        return Err(format!(
            "Invalid permission format `{original}`. Env vars must match `[A-Z_][A-Z0-9_]*`."
        ));
    }
    if !chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return Err(format!(
            "Invalid permission format `{original}`. Env vars must match `[A-Z_][A-Z0-9_]*`."
        ));
    }
    Ok(())
}

fn validate_network_permission(value: &str, original: &str) -> Result<(), String> {
    if value == "*" {
        return Ok(());
    }
    let (host, port) = value.split_once(':').ok_or_else(|| {
        format!(
            "Invalid permission format `{original}`. Network permissions must be `network:<host>:<port>`."
        )
    })?;
    if host.is_empty() {
        return Err(format!(
            "Invalid permission format `{original}`. Host is required."
        ));
    }
    if host != "*"
        && !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(format!(
            "Invalid permission format `{original}`. Host may contain only letters, digits, `.`, or `-`."
        ));
    }
    if port == "*" {
        return Ok(());
    }
    let parsed_port: u16 = port.parse().map_err(|_| {
        format!("Invalid permission format `{original}`. Port must be `*` or 1-65535.")
    })?;
    if parsed_port == 0 {
        return Err(format!(
            "Invalid permission format `{original}`. Port must be `*` or 1-65535."
        ));
    }
    Ok(())
}

fn validate_filesystem_permission(value: &str, original: &str) -> Result<(), String> {
    if value == "*" {
        return Ok(());
    }
    let (mode, path) = value.split_once(':').ok_or_else(|| {
        format!(
            "Invalid permission format `{original}`. Filesystem permissions must be `filesystem:<read|write>:<path>`."
        )
    })?;
    if !matches!(mode, "read" | "write") {
        return Err(format!(
            "Invalid permission format `{original}`. Mode must be `read` or `write`."
        ));
    }
    if path.trim().is_empty() {
        return Err(format!(
            "Invalid permission format `{original}`. Filesystem path is required."
        ));
    }
    Ok(())
}

fn validate_exec_permission(value: &str, original: &str) -> Result<(), String> {
    if value == "*" {
        return Ok(());
    }
    if value.trim().is_empty() {
        return Err(format!(
            "Invalid permission format `{original}`. Exec command is required."
        ));
    }
    if value.contains(char::is_whitespace) {
        return Err(format!(
            "Invalid permission format `{original}`. Exec command must not contain whitespace."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_permissions_apply_grant_and_revoke() {
        let overrides = PermissionOverrides {
            grant: vec!["env:FOO".to_string()],
            revoke: vec!["env:BAR".to_string()],
        };
        let declared = vec!["BAR".to_string(), "BAZ".to_string()];
        let eff = effective_permissions("env", &declared, &overrides);
        assert!(eff.contains(&"FOO".to_string()));
        assert!(eff.contains(&"BAZ".to_string()));
        assert!(!eff.contains(&"BAR".to_string()));
    }

    #[test]
    fn filter_env_map_removes_non_allowed_entries() {
        let overrides = PermissionOverrides {
            grant: vec![],
            revoke: vec!["env:GITHUB_TOKEN".to_string()],
        };
        let declared = vec!["GITHUB_TOKEN".to_string()];
        let mut env = BTreeMap::from([
            ("GITHUB_TOKEN".to_string(), "abc".to_string()),
            ("OTHER".to_string(), "xyz".to_string()),
        ]);

        filter_env_map(&mut env, &declared, &overrides);
        assert!(!env.contains_key("GITHUB_TOKEN"));
        assert!(!env.contains_key("OTHER"));
    }

    #[test]
    fn validate_network_permissions_rejects_full_revoke() {
        let overrides = PermissionOverrides {
            grant: vec![],
            revoke: vec!["network:*".to_string()],
        };
        let declared = vec!["api.github.com:443".to_string()];
        let err = validate_network_permissions("github", &declared, &overrides).unwrap_err();
        assert!(err.contains(NETWORK_PERMISSION_DENIED_PREFIX));
    }

    #[test]
    fn validate_network_permissions_allows_declared_network() {
        let overrides = PermissionOverrides::default();
        let declared = vec!["api.github.com:443".to_string()];
        assert!(validate_network_permissions("github", &declared, &overrides).is_ok());
    }

    #[test]
    fn undeclared_network_grants_identifies_extra_grants() {
        let declared = vec!["api.github.com:443".to_string()];
        let overrides = PermissionOverrides {
            grant: vec![
                "network:api.github.com:443".to_string(),
                "network:example.com:443".to_string(),
                "network:*".to_string(),
            ],
            revoke: vec![],
        };
        let undeclared = undeclared_network_grants(&declared, &overrides);
        assert_eq!(
            undeclared,
            vec!["*".to_string(), "example.com:443".to_string()]
        );
    }

    #[test]
    fn validate_permission_syntax_accepts_valid_formats() {
        assert!(validate_permission_syntax("env:GITHUB_TOKEN").is_ok());
        assert!(validate_permission_syntax("env:*").is_ok());
        assert!(validate_permission_syntax("network:api.github.com:443").is_ok());
        assert!(validate_permission_syntax("network:*:443").is_ok());
        assert!(validate_permission_syntax("network:*").is_ok());
        assert!(validate_permission_syntax("filesystem:read:/tmp").is_ok());
        assert!(validate_permission_syntax("filesystem:write:/var/log").is_ok());
        assert!(validate_permission_syntax("filesystem:*").is_ok());
        assert!(validate_permission_syntax("exec:git").is_ok());
        assert!(validate_permission_syntax("exec:*").is_ok());
    }

    #[test]
    fn validate_permission_syntax_rejects_invalid_formats() {
        assert!(validate_permission_syntax("env:GITHUB-TOKEN").is_err());
        assert!(validate_permission_syntax("network:api.github.com").is_err());
        assert!(validate_permission_syntax("network:api.github.com:99999").is_err());
        assert!(validate_permission_syntax("filesystem:/tmp").is_err());
        assert!(validate_permission_syntax("filesystem:run:/tmp").is_err());
        assert!(validate_permission_syntax("exec:").is_err());
        assert!(validate_permission_syntax("exec:git status").is_err());
    }
}
