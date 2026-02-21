//! Path helpers for Berth config, runtime, and client integration files.

use std::path::PathBuf;

/// Returns Berth home directory (`$BERTH_HOME` override or `~/.berth`).
pub fn berth_home() -> Option<PathBuf> {
    // Allow override via BERTH_HOME for testing
    if let Ok(home) = std::env::var("BERTH_HOME") {
        return Some(PathBuf::from(home));
    }
    dirs::home_dir().map(|h| h.join(".berth"))
}

/// Returns Berth server config directory (`~/.berth/servers`).
pub fn berth_servers_dir() -> Option<PathBuf> {
    berth_home().map(|h| h.join("servers"))
}

/// Returns the config file path for a server (`<name>.toml`).
pub fn server_config_path(name: &str) -> Option<PathBuf> {
    berth_servers_dir().map(|d| d.join(format!("{name}.toml")))
}

/// Returns the permissions override file path for a server.
pub fn permissions_override_path(name: &str) -> Option<PathBuf> {
    berth_home().map(|h| h.join("permissions").join(format!("{name}.toml")))
}

/// Returns the audit JSONL log path.
pub fn audit_log_path() -> Option<PathBuf> {
    berth_home().map(|h| h.join("audit").join("audit.jsonl"))
}

/// Returns a client MCP config path for the current platform.
pub fn client_config_path(client: &str) -> Option<PathBuf> {
    let (dir_name, file_name) = match client {
        "claude-desktop" => ("claude-desktop", "claude_desktop_config.json"),
        "cursor" => ("cursor", "cursor_mcp_config.json"),
        "windsurf" => ("windsurf", "windsurf_mcp_config.json"),
        _ => return None,
    };

    if let Ok(home) = std::env::var("BERTH_HOME") {
        return Some(
            PathBuf::from(home)
                .join("clients")
                .join(dir_name)
                .join(file_name),
        );
    }

    let home = dirs::home_dir()?;
    Some(match client {
        "claude-desktop" => {
            if cfg!(target_os = "macos") {
                home.join("Library")
                    .join("Application Support")
                    .join("Claude")
                    .join("claude_desktop_config.json")
            } else if cfg!(target_os = "windows") {
                if let Ok(appdata) = std::env::var("APPDATA") {
                    PathBuf::from(appdata)
                        .join("Claude")
                        .join("claude_desktop_config.json")
                } else {
                    home.join("AppData")
                        .join("Roaming")
                        .join("Claude")
                        .join("claude_desktop_config.json")
                }
            } else {
                home.join(".config")
                    .join("Claude")
                    .join("claude_desktop_config.json")
            }
        }
        "cursor" => {
            if cfg!(target_os = "macos") {
                home.join("Library")
                    .join("Application Support")
                    .join("Cursor")
                    .join("User")
                    .join("mcp.json")
            } else if cfg!(target_os = "windows") {
                if let Ok(appdata) = std::env::var("APPDATA") {
                    PathBuf::from(appdata)
                        .join("Cursor")
                        .join("User")
                        .join("mcp.json")
                } else {
                    home.join("AppData")
                        .join("Roaming")
                        .join("Cursor")
                        .join("User")
                        .join("mcp.json")
                }
            } else {
                home.join(".config")
                    .join("Cursor")
                    .join("User")
                    .join("mcp.json")
            }
        }
        "windsurf" => {
            if cfg!(target_os = "macos") {
                home.join("Library")
                    .join("Application Support")
                    .join("Windsurf")
                    .join("User")
                    .join("mcp.json")
            } else if cfg!(target_os = "windows") {
                if let Ok(appdata) = std::env::var("APPDATA") {
                    PathBuf::from(appdata)
                        .join("Windsurf")
                        .join("User")
                        .join("mcp.json")
                } else {
                    home.join("AppData")
                        .join("Roaming")
                        .join("Windsurf")
                        .join("User")
                        .join("mcp.json")
                }
            } else {
                home.join(".config")
                    .join("Windsurf")
                    .join("User")
                    .join("mcp.json")
            }
        }
        _ => unreachable!(),
    })
}

/// Returns the Claude Desktop config path for the current platform.
pub fn claude_desktop_config_path() -> Option<PathBuf> {
    client_config_path("claude-desktop")
}
