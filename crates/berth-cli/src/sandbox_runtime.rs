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

#[derive(Debug, Clone, Copy)]
struct RuntimeProbes {
    platform: HostPlatform,
    has_setpriv: bool,
    has_sandbox_exec: bool,
    has_landlock_restrict: bool,
}

impl RuntimeProbes {
    fn detect() -> Self {
        Self {
            platform: host_platform(),
            has_setpriv: path_has_binary("setpriv"),
            has_sandbox_exec: path_has_binary("sandbox-exec"),
            has_landlock_restrict: path_has_binary("landlock-restrict"),
        }
    }
}

/// Applies sandbox runtime adaptation to command/args/env.
pub fn apply_sandbox_runtime(
    command: &str,
    args: &[String],
    env_map: &mut BTreeMap<String, String>,
    policy: SandboxPolicy,
    filesystem_permissions: &[String],
) -> (String, Vec<String>) {
    apply_sandbox_runtime_with_probes(
        command,
        args,
        env_map,
        policy,
        filesystem_permissions,
        RuntimeProbes::detect(),
    )
}

fn apply_sandbox_runtime_with_probes(
    command: &str,
    args: &[String],
    env_map: &mut BTreeMap<String, String>,
    policy: SandboxPolicy,
    filesystem_permissions: &[String],
    probes: RuntimeProbes,
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

    match probes.platform {
        HostPlatform::Linux => {
            let mut wrapped_command = command.to_string();
            let mut wrapped_args = args.to_vec();
            let mut backend = Vec::new();

            if probes.has_landlock_restrict {
                let mut landlock_args = vec!["--best-effort".to_string()];
                for permission in filesystem_permissions {
                    if let Some((mode, path)) = parse_filesystem_permission(permission) {
                        if path == "*" || path.is_empty() {
                            continue;
                        }
                        match mode {
                            "read" => {
                                landlock_args.push("--ro".to_string());
                                landlock_args.push(path.to_string());
                            }
                            "write" => {
                                landlock_args.push("--rw".to_string());
                                landlock_args.push(path.to_string());
                            }
                            _ => {}
                        }
                    }
                }
                landlock_args.push("--".to_string());
                landlock_args.push(wrapped_command);
                landlock_args.extend(wrapped_args);
                wrapped_command = "landlock-restrict".to_string();
                wrapped_args = landlock_args;
                backend.push("linux-landlock");
            }

            if probes.has_setpriv {
                let mut setpriv_args = vec![
                    "--no-new-privs".to_string(),
                    "--".to_string(),
                    wrapped_command,
                ];
                setpriv_args.extend(wrapped_args);
                wrapped_command = "setpriv".to_string();
                wrapped_args = setpriv_args;
                backend.push("linux-setpriv");
            }

            if backend.is_empty() {
                env_map.insert("BERTH_SANDBOX_BACKEND".to_string(), "none".to_string());
                return (command.to_string(), args.to_vec());
            }

            env_map.insert("BERTH_SANDBOX_BACKEND".to_string(), backend.join("+"));
            (wrapped_command, wrapped_args)
        }
        HostPlatform::MacOs if probes.has_sandbox_exec => {
            env_map.insert(
                "BERTH_SANDBOX_BACKEND".to_string(),
                "macos-sandbox-exec".to_string(),
            );
            let profile = build_macos_profile(policy, filesystem_permissions);
            let mut wrapped = vec!["-p".to_string(), profile, command.to_string()];
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

fn build_macos_profile(policy: SandboxPolicy, filesystem_permissions: &[String]) -> String {
    let mut write_paths = vec!["/tmp".to_string(), "/private/tmp".to_string()];
    let mut allow_all_writes = false;

    for permission in filesystem_permissions {
        let (mode, path) = match parse_filesystem_permission(permission) {
            Some(parts) => parts,
            None => continue,
        };
        if mode != "write" {
            continue;
        }
        if path == "*" {
            allow_all_writes = true;
            break;
        }
        if !path.is_empty() {
            write_paths.push(path.to_string());
        }
    }

    write_paths.sort();
    write_paths.dedup();

    let mut profile = vec![
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        "(import \"system.sb\")".to_string(),
        "(allow process*)".to_string(),
        "(allow file-read*)".to_string(),
    ];

    if !policy.network_deny_all {
        profile.push("(allow network*)".to_string());
    }

    if allow_all_writes {
        profile.push("(allow file-write*)".to_string());
    } else {
        profile.push("(allow file-write*".to_string());
        for path in write_paths {
            profile.push(format!(
                "    (subpath \"{}\")",
                escape_sandbox_string(&path)
            ));
        }
        profile.push(")".to_string());
    }

    profile.join("\n")
}

fn parse_filesystem_permission(permission: &str) -> Option<(&str, &str)> {
    let value = permission.strip_prefix("filesystem:").unwrap_or(permission);
    let (mode, path) = value.split_once(':')?;
    Some((mode, path))
}

fn escape_sandbox_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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
            &[],
            RuntimeProbes {
                platform: HostPlatform::Linux,
                has_setpriv: true,
                has_sandbox_exec: false,
                has_landlock_restrict: false,
            },
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
            &[],
            RuntimeProbes {
                platform: HostPlatform::Linux,
                has_setpriv: true,
                has_sandbox_exec: false,
                has_landlock_restrict: false,
            },
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
            &[],
            RuntimeProbes {
                platform: HostPlatform::Linux,
                has_setpriv: false,
                has_sandbox_exec: false,
                has_landlock_restrict: false,
            },
        );
        assert_eq!(cmd, "npx");
        assert_eq!(args, vec!["-y".to_string()]);
        assert_eq!(
            env_map.get("BERTH_SANDBOX_BACKEND"),
            Some(&"none".to_string())
        );
    }

    #[test]
    fn macos_profile_includes_declared_write_paths() {
        let mut env_map = BTreeMap::new();
        let (cmd, args) = apply_sandbox_runtime_with_probes(
            "npx",
            &["-y".to_string(), "server".to_string()],
            &mut env_map,
            SandboxPolicy {
                enabled: true,
                network_deny_all: false,
            },
            &[
                "read:/workspace".to_string(),
                "write:/workspace".to_string(),
            ],
            RuntimeProbes {
                platform: HostPlatform::MacOs,
                has_setpriv: false,
                has_sandbox_exec: true,
                has_landlock_restrict: false,
            },
        );

        assert_eq!(cmd, "sandbox-exec");
        assert_eq!(args[0], "-p");
        assert!(args[1].contains("(deny default)"));
        assert!(args[1].contains("(allow network*)"));
        assert!(args[1].contains("(subpath \"/workspace\")"));
        assert_eq!(args[2], "npx");
        assert_eq!(args[3], "-y");
        assert_eq!(args[4], "server");
        assert_eq!(
            env_map.get("BERTH_SANDBOX_BACKEND"),
            Some(&"macos-sandbox-exec".to_string())
        );
    }

    #[test]
    fn linux_landlock_wraps_with_filesystem_permissions_when_available() {
        let mut env_map = BTreeMap::new();
        let (cmd, args) = apply_sandbox_runtime_with_probes(
            "npx",
            &["-y".to_string(), "server".to_string()],
            &mut env_map,
            SandboxPolicy {
                enabled: true,
                network_deny_all: false,
            },
            &[
                "read:/workspace".to_string(),
                "write:/tmp".to_string(),
                "exec:git".to_string(),
            ],
            RuntimeProbes {
                platform: HostPlatform::Linux,
                has_setpriv: false,
                has_sandbox_exec: false,
                has_landlock_restrict: true,
            },
        );

        assert_eq!(cmd, "landlock-restrict");
        assert_eq!(args[0], "--best-effort");
        assert!(args.contains(&"--ro".to_string()));
        assert!(args.contains(&"/workspace".to_string()));
        assert!(args.contains(&"--rw".to_string()));
        assert!(args.contains(&"/tmp".to_string()));
        assert_eq!(
            env_map.get("BERTH_SANDBOX_BACKEND"),
            Some(&"linux-landlock".to_string())
        );
    }
}
