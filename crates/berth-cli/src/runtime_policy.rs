//! Helpers for parsing and validating runtime auto-restart policy settings.

use std::collections::BTreeMap;

use berth_runtime::AutoRestartPolicy;

pub const KEY_AUTO_RESTART: &str = "berth.auto-restart";
pub const KEY_MAX_RESTARTS: &str = "berth.max-restarts";
pub const DEFAULT_MAX_RESTARTS: u32 = 3;

/// Returns whether a key is reserved for Berth runtime policy settings.
pub fn is_runtime_policy_key(key: &str) -> bool {
    matches!(key, KEY_AUTO_RESTART | KEY_MAX_RESTARTS)
}

/// Validates one key/value pair for runtime policy settings.
pub fn validate_runtime_policy_value(key: &str, value: &str) -> Result<(), String> {
    match key {
        KEY_AUTO_RESTART => parse_bool(value).map(|_| ()),
        KEY_MAX_RESTARTS => parse_max_restarts(value).map(|_| ()),
        _ => Err(format!("Unknown runtime policy key: {key}")),
    }
}

/// Parses runtime auto-restart policy from installed config values.
pub fn parse_runtime_policy(
    config: &BTreeMap<String, String>,
) -> Result<AutoRestartPolicy, String> {
    let enabled = match config.get(KEY_AUTO_RESTART) {
        Some(v) => parse_bool(v)?,
        None => false,
    };
    let max_restarts = match config.get(KEY_MAX_RESTARTS) {
        Some(v) => parse_max_restarts(v)?,
        None => DEFAULT_MAX_RESTARTS,
    };
    Ok(AutoRestartPolicy {
        enabled,
        max_restarts,
    })
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!(
            "Invalid value `{value}`. Expected `true` or `false`."
        )),
    }
}

fn parse_max_restarts(value: &str) -> Result<u32, String> {
    let parsed: u32 = value
        .trim()
        .parse()
        .map_err(|_| format!("Invalid value `{value}`. Expected a positive integer (>= 1)."))?;
    if parsed == 0 {
        return Err(format!(
            "Invalid value `{value}`. Expected a positive integer (>= 1)."
        ));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_runtime_policy_defaults_when_unset() {
        let cfg = BTreeMap::new();
        let policy = parse_runtime_policy(&cfg).unwrap();
        assert!(!policy.enabled);
        assert_eq!(policy.max_restarts, DEFAULT_MAX_RESTARTS);
    }

    #[test]
    fn parse_runtime_policy_reads_values() {
        let cfg = BTreeMap::from([
            (KEY_AUTO_RESTART.to_string(), "true".to_string()),
            (KEY_MAX_RESTARTS.to_string(), "5".to_string()),
        ]);
        let policy = parse_runtime_policy(&cfg).unwrap();
        assert!(policy.enabled);
        assert_eq!(policy.max_restarts, 5);
    }

    #[test]
    fn validate_runtime_policy_rejects_bad_values() {
        assert!(validate_runtime_policy_value(KEY_AUTO_RESTART, "maybe").is_err());
        assert!(validate_runtime_policy_value(KEY_MAX_RESTARTS, "0").is_err());
    }
}
