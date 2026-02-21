//! Shared permission override and effective-permission helpers.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use crate::paths;

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
}
