//! Runtime helpers that adapt process launch for sandbox policies.

use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::path::PathBuf;

use crate::sandbox_policy::SandboxPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostPlatform {
    Linux,
    MacOs,
    Other,
}

/// Applies sandbox runtime adaptation to command/args/env.
pub fn apply_sandbox_runtime(
    command: &str,
    args: &[String],
    env_map: &mut BTreeMap<String, String>,
    policy: SandboxPolicy,
) -> (String, Vec<String>) {
    apply_sandbox_runtime_with_probes(
        command,
        args,
        env_map,
        policy,
        host_platform(),
        path_has_binary("setpriv"),
        path_has_binary("sandbox-exec"),
    )
}

fn apply_sandbox_runtime_with_probes(
    command: &str,
    args: &[String],
    env_map: &mut BTreeMap<String, String>,
    policy: SandboxPolicy,
    platform: HostPlatform,
    has_setpriv: bool,
    has_sandbox_exec: bool,
) -> (String, Vec<String>) {
    if !policy.enabled {
        return (command.to_string(), args.to_vec());
    }

    env_map.insert("BERTH_SANDBOX_MODE".to_string(), "basic".to_string());
    env_map.insert(
        "BERTH_SANDBOX_NETWORK".to_string(),
        if policy.network_deny_all {
            "deny-all".to_string()
        } else {
            "inherit".to_string()
        },
    );

    match platform {
        HostPlatform::Linux if has_setpriv => {
            env_map.insert(
                "BERTH_SANDBOX_BACKEND".to_string(),
                "linux-setpriv".to_string(),
            );
            let mut wrapped = vec![
                "--no-new-privs".to_string(),
                "--".to_string(),
                command.to_string(),
            ];
            wrapped.extend(args.iter().cloned());
            ("setpriv".to_string(), wrapped)
        }
        HostPlatform::MacOs if has_sandbox_exec => {
            // Keep policy permissive for now; this is a compatibility scaffold.
            env_map.insert(
                "BERTH_SANDBOX_BACKEND".to_string(),
                "macos-sandbox-exec".to_string(),
            );
            let mut wrapped = vec![
                "-p".to_string(),
                "(version 1) (allow default)".to_string(),
                command.to_string(),
            ];
            wrapped.extend(args.iter().cloned());
            ("sandbox-exec".to_string(), wrapped)
        }
        _ => {
            env_map.insert("BERTH_SANDBOX_BACKEND".to_string(), "none".to_string());
            (command.to_string(), args.to_vec())
        }
    }
}

fn host_platform() -> HostPlatform {
    if cfg!(target_os = "linux") {
        HostPlatform::Linux
    } else if cfg!(target_os = "macos") {
        HostPlatform::MacOs
    } else {
        HostPlatform::Other
    }
}

fn path_has_binary(name: &str) -> bool {
    let path = match env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };

    env::split_paths(&path).any(|dir| candidate_paths(&dir, name).iter().any(|p| p.is_file()))
}

fn candidate_paths(dir: &Path, name: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![dir.join(format!("{name}.exe")), dir.join(name)]
    }
    #[cfg(not(windows))]
    {
        vec![dir.join(name)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_policy::SandboxPolicy;

    #[test]
    fn sandbox_disabled_keeps_original_command() {
        let mut env_map = BTreeMap::new();
        let (cmd, args) = apply_sandbox_runtime_with_probes(
            "npx",
            &["-y".to_string()],
            &mut env_map,
            SandboxPolicy {
                enabled: false,
                network_deny_all: false,
            },
            HostPlatform::Linux,
            true,
            false,
        );
        assert_eq!(cmd, "npx");
        assert_eq!(args, vec!["-y".to_string()]);
        assert!(env_map.is_empty());
    }

    #[test]
    fn linux_sandbox_wraps_with_setpriv_when_available() {
        let mut env_map = BTreeMap::new();
        let (cmd, args) = apply_sandbox_runtime_with_probes(
            "npx",
            &["-y".to_string(), "server".to_string()],
            &mut env_map,
            SandboxPolicy {
                enabled: true,
                network_deny_all: false,
            },
            HostPlatform::Linux,
            true,
            false,
        );
        assert_eq!(cmd, "setpriv");
        assert_eq!(
            args,
            vec![
                "--no-new-privs".to_string(),
                "--".to_string(),
                "npx".to_string(),
                "-y".to_string(),
                "server".to_string()
            ]
        );
        assert_eq!(
            env_map.get("BERTH_SANDBOX_BACKEND"),
            Some(&"linux-setpriv".to_string())
        );
    }

    #[test]
    fn sandbox_falls_back_when_backend_unavailable() {
        let mut env_map = BTreeMap::new();
        let (cmd, args) = apply_sandbox_runtime_with_probes(
            "npx",
            &["-y".to_string()],
            &mut env_map,
            SandboxPolicy {
                enabled: true,
                network_deny_all: false,
            },
            HostPlatform::Linux,
            false,
            false,
        );
        assert_eq!(cmd, "npx");
        assert_eq!(args, vec!["-y".to_string()]);
        assert_eq!(
            env_map.get("BERTH_SANDBOX_BACKEND"),
            Some(&"none".to_string())
        );
    }
}
