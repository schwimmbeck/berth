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
