// SPDX-License-Identifier: Apache-2.0

//! Registry loading and query APIs for Berth.

pub mod config;
pub mod search;
pub mod seed;
pub mod types;

use search::{search_servers, SearchResult};
use seed::load_seed_registry;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use types::ServerMetadata;

/// In-memory registry loaded from the embedded seed dataset.
pub struct Registry {
    servers: Vec<ServerMetadata>,
}

impl Registry {
    /// Builds a registry from embedded seed JSON.
    pub fn from_seed() -> Self {
        let index_file = env::var_os("BERTH_REGISTRY_INDEX_FILE").map(PathBuf::from);
        let index_url = env::var("BERTH_REGISTRY_INDEX_URL")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let cache_path = default_cache_path();
        if let Ok(servers) = load_registry_servers(
            index_file.as_deref(),
            cache_path.as_deref(),
            index_url.as_deref(),
        ) {
            return Registry { servers };
        }

        Registry {
            servers: load_seed_registry(),
        }
    }

    /// Searches servers by keyword and relevance.
    pub fn search(&self, query: &str) -> Vec<SearchResult<'_>> {
        search_servers(&self.servers, query)
    }

    /// Returns a server by exact name.
    pub fn get(&self, name: &str) -> Option<&ServerMetadata> {
        search::find_server(&self.servers, name)
    }

    /// Returns all registry servers.
    pub fn list_all(&self) -> &[ServerMetadata] {
        &self.servers
    }
}

fn load_registry_servers(
    index_file: Option<&Path>,
    cache_path: Option<&Path>,
    index_url: Option<&str>,
) -> Result<Vec<ServerMetadata>, String> {
    if let Some(path) = index_file {
        let data = fs::read_to_string(path).map_err(|e| {
            format!(
                "failed reading registry index override {}: {e}",
                path.display()
            )
        })?;
        let servers = parse_registry_json(&data)?;
        if let Some(cache) = cache_path {
            let _ = write_cache(cache, &data);
        }
        return Ok(servers);
    }

    if let Some(url) = index_url {
        let data = fetch_registry_json(url)?;
        let servers = parse_registry_json(&data)?;
        if let Some(cache) = cache_path {
            let _ = write_cache(cache, &data);
        }
        return Ok(servers);
    }

    if let Some(cache) = cache_path {
        if cache.exists() {
            let data = fs::read_to_string(cache).map_err(|e| {
                format!(
                    "failed reading cached registry index {}: {e}",
                    cache.display()
                )
            })?;
            let servers = parse_registry_json(&data)?;
            return Ok(servers);
        }
    }

    Err("no registry override or cache available".to_string())
}

fn parse_registry_json(data: &str) -> Result<Vec<ServerMetadata>, String> {
    serde_json::from_str(data).map_err(|e| format!("invalid registry json: {e}"))
}

fn write_cache(path: &Path, data: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "failed creating registry cache directory {}: {e}",
                    parent.display()
                )
            })?;
        }
    }
    fs::write(path, data)
        .map_err(|e| format!("failed writing registry cache {}: {e}", path.display()))
}

fn fetch_registry_json(url: &str) -> Result<String, String> {
    let curl_output = Command::new("curl")
        .args(["-fsSL", "--max-time", "5", url])
        .output();
    if let Ok(output) = curl_output {
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map_err(|e| format!("registry response was not utf-8: {e}"));
        }
    }

    let wget_output = Command::new("wget")
        .args(["-q", "-O", "-", "--timeout=5", url])
        .output();
    if let Ok(output) = wget_output {
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map_err(|e| format!("registry response was not utf-8: {e}"));
        }
    }

    Err(format!(
        "failed to fetch registry index from {url} (curl/wget unavailable or request failed)"
    ))
}

fn default_cache_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("BERTH_REGISTRY_CACHE") {
        return Some(PathBuf::from(path));
    }
    env::var_os("BERTH_HOME").map(|home| PathBuf::from(home).join("registry").join("index.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn registry_from_seed() {
        let registry = Registry::from_seed();
        assert_eq!(registry.list_all().len(), 30);
    }

    #[test]
    fn registry_search() {
        let registry = Registry::from_seed();
        let results = registry.search("github");
        assert!(!results.is_empty());
    }

    #[test]
    fn registry_get() {
        let registry = Registry::from_seed();
        assert!(registry.get("slack").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn registry_loads_from_override_file_and_writes_cache() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("index.json");
        let cache = temp.path().join("cache/index.json");

        let mut server = load_seed_registry()[0].clone();
        server.name = "override-server".to_string();
        server.display_name = "Override Server".to_string();
        fs::write(&source, serde_json::to_string(&vec![server]).unwrap()).unwrap();

        let servers = load_registry_servers(Some(&source), Some(&cache), None).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "override-server");
        assert!(cache.exists());
    }

    #[test]
    fn registry_loads_from_cache_when_available() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache/index.json");

        let mut server = load_seed_registry()[0].clone();
        server.name = "cached-server".to_string();
        server.display_name = "Cached Server".to_string();
        write_cache(&cache, &serde_json::to_string(&vec![server]).unwrap()).unwrap();

        let servers = load_registry_servers(None, Some(&cache), None).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "cached-server");
    }
}
