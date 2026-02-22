// SPDX-License-Identifier: Apache-2.0

//! Secret storage helpers for optional secure config values.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::paths;

const SECRET_REF_PREFIX: &str = "secret://";
const KEYRING_SERVICE: &str = "berth";
const SECRET_BACKEND_ENV: &str = "BERTH_SECRET_BACKEND";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretBackend {
    Keyring,
    File,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileSecrets {
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

/// Stores a secret and returns a persisted secret reference.
pub fn store_secret(server: &str, key: &str, value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err("secret value must not be empty".to_string());
    }

    match secret_backend() {
        SecretBackend::Keyring => store_keyring_secret(server, key, value)?,
        SecretBackend::File => store_file_secret(server, key, value)?,
    }

    Ok(secret_ref(server, key))
}

/// Resolves a config value that may contain a secret reference.
pub fn resolve_config_value(_server: &str, _key: &str, raw_value: &str) -> Result<String, String> {
    if let Some((secret_server, secret_key)) = parse_secret_ref(raw_value) {
        return match secret_backend() {
            SecretBackend::Keyring => get_keyring_secret(&secret_server, &secret_key),
            SecretBackend::File => get_file_secret(&secret_server, &secret_key),
        };
    }
    Ok(raw_value.to_string())
}

fn secret_backend() -> SecretBackend {
    match std::env::var(SECRET_BACKEND_ENV)
        .ok()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "file" => SecretBackend::File,
        _ => SecretBackend::Keyring,
    }
}

fn secret_ref(server: &str, key: &str) -> String {
    format!("{SECRET_REF_PREFIX}{server}/{key}")
}

fn secret_id(server: &str, key: &str) -> String {
    format!("{server}:{key}")
}

fn parse_secret_ref(value: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix(SECRET_REF_PREFIX)?;
    let (server, key) = rest.split_once('/')?;
    if server.trim().is_empty() || key.trim().is_empty() {
        return None;
    }
    Some((server.to_string(), key.to_string()))
}

fn store_keyring_secret(server: &str, key: &str, value: &str) -> Result<(), String> {
    let account = secret_id(server, key);

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("security")
            .args([
                "add-generic-password",
                "-U",
                "-a",
                &account,
                "-s",
                KEYRING_SERVICE,
                "-w",
                value,
            ])
            .status()
            .map_err(|e| format!("failed to invoke macOS keychain tool: {e}"))?;
        if status.success() {
            return Ok(());
        }
        return Err(
            "failed to store secret in macOS keychain (`security add-generic-password`)"
                .to_string(),
        );
    }

    #[cfg(target_os = "linux")]
    {
        let mut child = Command::new("secret-tool")
            .args([
                "store",
                "--label",
                "Berth Secret",
                "service",
                KEYRING_SERVICE,
                "account",
                &account,
            ])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to invoke secret-tool: {e}"))?;
        if let Some(stdin) = &mut child.stdin {
            stdin
                .write_all(value.as_bytes())
                .map_err(|e| format!("failed to write secret to secret-tool stdin: {e}"))?;
        }
        let status = child
            .wait()
            .map_err(|e| format!("failed waiting for secret-tool: {e}"))?;
        if status.success() {
            return Ok(());
        }
        Err("failed to store secret via secret-tool (libsecret keychain unavailable?)".to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = value;
        Err(
            "OS keychain backend is not supported on this platform; use BERTH_SECRET_BACKEND=file"
                .to_string(),
        )
    }
}

fn get_keyring_secret(server: &str, key: &str) -> Result<String, String> {
    let account = secret_id(server, key);

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-a",
                &account,
                "-s",
                KEYRING_SERVICE,
                "-w",
            ])
            .output()
            .map_err(|e| format!("failed to invoke macOS keychain tool: {e}"))?;
        if !output.status.success() {
            return Err(
                "failed to read secret from macOS keychain (`security find-generic-password`)"
                    .to_string(),
            );
        }
        return String::from_utf8(output.stdout)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("keychain value was not valid utf-8: {e}"));
    }

    #[cfg(target_os = "linux")]
    {
        let output = Command::new("secret-tool")
            .args(["lookup", "service", KEYRING_SERVICE, "account", &account])
            .output()
            .map_err(|e| format!("failed to invoke secret-tool: {e}"))?;
        if !output.status.success() {
            return Err("failed to read secret via secret-tool".to_string());
        }
        String::from_utf8(output.stdout)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("secret-tool output was not valid utf-8: {e}"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(
            "OS keychain backend is not supported on this platform; use BERTH_SECRET_BACKEND=file"
                .to_string(),
        )
    }
}

fn secrets_file_path() -> Result<std::path::PathBuf, String> {
    paths::berth_home()
        .map(|p| p.join("credentials").join("secrets.toml"))
        .ok_or("Could not determine home directory.".to_string())
}

fn read_file_secrets() -> Result<FileSecrets, String> {
    let path = secrets_file_path()?;
    read_file_secrets_from(&path)
}

fn write_file_secrets(secrets: &FileSecrets) -> Result<(), String> {
    let path = secrets_file_path()?;
    write_file_secrets_to(&path, secrets)
}

fn read_file_secrets_from(path: &Path) -> Result<FileSecrets, String> {
    if !path.exists() {
        return Ok(FileSecrets::default());
    }
    let content =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    toml::from_str::<FileSecrets>(&content)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

fn write_file_secrets_to(path: &Path, secrets: &FileSecrets) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
    }
    let rendered =
        toml::to_string_pretty(secrets).map_err(|e| format!("failed to serialize secrets: {e}"))?;
    fs::write(path, rendered).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(())
}

fn store_file_secret(server: &str, key: &str, value: &str) -> Result<(), String> {
    let mut secrets = read_file_secrets()?;
    secrets
        .secrets
        .insert(secret_id(server, key), value.to_string());
    write_file_secrets(&secrets)
}

fn get_file_secret(server: &str, key: &str) -> Result<String, String> {
    let secrets = read_file_secrets()?;
    secrets
        .secrets
        .get(&secret_id(server, key))
        .cloned()
        .ok_or_else(|| format!("secret not found for {server}:{key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_plain_config_value_returns_input() {
        let resolved = resolve_config_value("github", "token", "abc123").unwrap();
        assert_eq!(resolved, "abc123");
    }

    #[test]
    fn file_secret_store_round_trip_helpers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("secrets.toml");

        let mut secrets = FileSecrets::default();
        secrets
            .secrets
            .insert("github:token".to_string(), "secret-value".to_string());
        write_file_secrets_to(&path, &secrets).unwrap();

        let loaded = read_file_secrets_from(&path).unwrap();
        assert_eq!(
            loaded.secrets.get("github:token"),
            Some(&"secret-value".to_string())
        );
    }
}
