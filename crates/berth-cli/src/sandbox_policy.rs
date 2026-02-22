// SPDX-License-Identifier: Apache-2.0

//! Helpers for parsing and validating basic sandbox policy settings.

use std::collections::BTreeMap;

pub const KEY_SANDBOX: &str = "berth.sandbox";
pub const KEY_SANDBOX_NETWORK: &str = "berth.sandbox-network";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub enabled: bool,
    pub network_deny_all: bool,
}

/// Returns whether a key is reserved for Berth sandbox policy settings.
pub fn is_sandbox_policy_key(key: &str) -> bool {
    matches!(key, KEY_SANDBOX | KEY_SANDBOX_NETWORK)
}

/// Validates one key/value pair for sandbox policy settings.
pub fn validate_sandbox_policy_value(key: &str, value: &str) -> Result<(), String> {
    match key {
        KEY_SANDBOX => parse_sandbox_mode(value).map(|_| ()),
        KEY_SANDBOX_NETWORK => parse_network_mode(value).map(|_| ()),
        _ => Err(format!("Unknown sandbox policy key: {key}")),
    }
}

/// Parses sandbox policy values from installed config.
pub fn parse_sandbox_policy(config: &BTreeMap<String, String>) -> Result<SandboxPolicy, String> {
    let enabled = match config.get(KEY_SANDBOX) {
        Some(v) => parse_sandbox_mode(v)?,
        None => false,
    };
    let network_deny_all = match config.get(KEY_SANDBOX_NETWORK) {
        Some(v) => parse_network_mode(v)?,
        None => false,
    };
    Ok(SandboxPolicy {
        enabled,
        network_deny_all,
    })
}

fn parse_sandbox_mode(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "basic" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!(
            "Invalid value `{value}`. Expected `basic` or `off`."
        )),
    }
}

fn parse_network_mode(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "deny-all" => Ok(true),
        "inherit" => Ok(false),
        _ => Err(format!(
            "Invalid value `{value}`. Expected `inherit` or `deny-all`."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sandbox_policy_defaults_when_unset() {
        let cfg = BTreeMap::new();
        let policy = parse_sandbox_policy(&cfg).unwrap();
        assert!(!policy.enabled);
        assert!(!policy.network_deny_all);
    }

    #[test]
    fn parse_sandbox_policy_reads_values() {
        let cfg = BTreeMap::from([
            (KEY_SANDBOX.to_string(), "basic".to_string()),
            (KEY_SANDBOX_NETWORK.to_string(), "deny-all".to_string()),
        ]);
        let policy = parse_sandbox_policy(&cfg).unwrap();
        assert!(policy.enabled);
        assert!(policy.network_deny_all);
    }

    #[test]
    fn validate_sandbox_policy_rejects_bad_values() {
        assert!(validate_sandbox_policy_value(KEY_SANDBOX, "on").is_err());
        assert!(validate_sandbox_policy_value(KEY_SANDBOX_NETWORK, "deny").is_err());
    }
}
