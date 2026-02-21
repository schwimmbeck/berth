use std::path::PathBuf;

pub fn berth_home() -> Option<PathBuf> {
    // Allow override via BERTH_HOME for testing
    if let Ok(home) = std::env::var("BERTH_HOME") {
        return Some(PathBuf::from(home));
    }
    dirs::home_dir().map(|h| h.join(".berth"))
}

pub fn berth_servers_dir() -> Option<PathBuf> {
    berth_home().map(|h| h.join("servers"))
}

pub fn server_config_path(name: &str) -> Option<PathBuf> {
    berth_servers_dir().map(|d| d.join(format!("{name}.toml")))
}

pub fn claude_desktop_config_path() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("BERTH_HOME") {
        return Some(
            PathBuf::from(home)
                .join("clients")
                .join("claude-desktop")
                .join("claude_desktop_config.json"),
        );
    }

    let home = dirs::home_dir()?;
    Some(if cfg!(target_os = "macos") {
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
    })
}
