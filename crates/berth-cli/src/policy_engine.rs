// SPDX-License-Identifier: Apache-2.0

//! Organization-wide policy enforcement for runtime launches.

use berth_registry::config::PermissionsInfo;
use serde::{Deserialize, Serialize};
use std::fs;

use crate::paths;
use crate::permission_filter::{effective_permissions, PermissionOverrides};

/// Prefix used in user-facing errors for org-policy launch denials.
pub const POLICY_DENIED_PREFIX: &str = "Policy denied";

/// Global policy model loaded from `~/.berth/policy.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalPolicy {
    #[serde(default)]
    pub servers: ServerPolicy,
    #[serde(default)]
    pub permissions: PermissionPolicy,
}

/// Server-scoped deny list.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerPolicy {
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Permission-oriented deny toggles.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionPolicy {
    #[serde(default)]
    pub deny_network_wildcard: bool,
    #[serde(default)]
    pub deny_env_wildcard: bool,
    #[serde(default)]
    pub deny_filesystem_write: bool,
    #[serde(default)]
    pub deny_exec_wildcard: bool,
}

/// Loads policy file from Berth home; returns permissive defaults when missing.
pub fn load_global_policy() -> Result<GlobalPolicy, String> {
    let Some(path) = paths::policy_path() else {
        return Err("Could not determine home directory.".to_string());
    };
    if !path.exists() {
        return Ok(GlobalPolicy::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read policy file {}: {e}", path.display()))?;
    toml::from_str::<GlobalPolicy>(&content)
        .map_err(|e| format!("Failed to parse policy file {}: {e}", path.display()))
}

/// Enforces global policy against effective permissions of one server launch.
pub fn enforce_global_policy(
    server: &str,
    declared: &PermissionsInfo,
    overrides: &PermissionOverrides,
    policy: &GlobalPolicy,
) -> Result<(), String> {
    if server_denied(server, policy) {
        return Err(format!(
            "{POLICY_DENIED_PREFIX} for {server}: server is blocked by org policy."
        ));
    }

    let effective_network = effective_permissions("network", &declared.network, overrides);
    let effective_env = effective_permissions("env", &declared.env, overrides);
    let effective_filesystem = effective_permissions("filesystem", &declared.filesystem, overrides);
    let effective_exec = effective_permissions("exec", &declared.exec, overrides);

    if policy.permissions.deny_network_wildcard
        && effective_network.iter().any(|entry| entry == "*")
    {
        return Err(format!(
            "{POLICY_DENIED_PREFIX} for {server}: network wildcard `*` is blocked by org policy."
        ));
    }
    if policy.permissions.deny_env_wildcard && effective_env.iter().any(|entry| entry == "*") {
        return Err(format!(
            "{POLICY_DENIED_PREFIX} for {server}: env wildcard `*` is blocked by org policy."
        ));
    }
    if policy.permissions.deny_filesystem_write
        && effective_filesystem
            .iter()
            .any(|entry| entry == "*" || entry.trim_start().starts_with("write:"))
    {
        return Err(format!(
            "{POLICY_DENIED_PREFIX} for {server}: filesystem write access is blocked by org policy."
        ));
    }
    if policy.permissions.deny_exec_wildcard && effective_exec.iter().any(|entry| entry == "*") {
        return Err(format!(
            "{POLICY_DENIED_PREFIX} for {server}: exec wildcard `*` is blocked by org policy."
        ));
    }

    Ok(())
}

fn server_denied(server: &str, policy: &GlobalPolicy) -> bool {
    policy.servers.deny.iter().any(|entry| {
        let normalized = entry.trim();
        normalized == "*" || normalized.eq_ignore_ascii_case(server)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared() -> PermissionsInfo {
        PermissionsInfo {
            network: vec!["api.github.com:443".to_string()],
            env: vec!["GITHUB_TOKEN".to_string()],
            filesystem: vec!["read:/tmp".to_string(), "write:/tmp".to_string()],
            exec: vec!["git".to_string()],
        }
    }

    #[test]
    fn enforce_global_policy_blocks_denied_server() {
        let policy = GlobalPolicy {
            servers: ServerPolicy {
                deny: vec!["github".to_string()],
            },
            permissions: PermissionPolicy::default(),
        };
        let err = enforce_global_policy(
            "github",
            &declared(),
            &PermissionOverrides::default(),
            &policy,
        )
        .unwrap_err();
        assert!(err.contains(POLICY_DENIED_PREFIX));
        assert!(err.contains("server is blocked"));
    }

    #[test]
    fn enforce_global_policy_blocks_wildcards_and_write_access() {
        let policy = GlobalPolicy {
            servers: ServerPolicy::default(),
            permissions: PermissionPolicy {
                deny_network_wildcard: true,
                deny_env_wildcard: true,
                deny_filesystem_write: true,
                deny_exec_wildcard: true,
            },
        };
        let overrides = PermissionOverrides {
            grant: vec![
                "network:*".to_string(),
                "env:*".to_string(),
                "exec:*".to_string(),
            ],
            revoke: Vec::new(),
        };
        let err = enforce_global_policy("github", &declared(), &overrides, &policy).unwrap_err();
        assert!(err.contains(POLICY_DENIED_PREFIX));
    }

    #[test]
    fn enforce_global_policy_allows_when_policy_is_permissive() {
        let result = enforce_global_policy(
            "github",
            &declared(),
            &PermissionOverrides::default(),
            &GlobalPolicy::default(),
        );
        assert!(result.is_ok());
    }
}
