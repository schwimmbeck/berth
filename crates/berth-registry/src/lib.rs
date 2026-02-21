//! Registry loading and query APIs for Berth.

pub mod config;
pub mod search;
pub mod seed;
pub mod types;

use search::{search_servers, SearchResult};
use seed::load_seed_registry;
use types::ServerMetadata;

/// In-memory registry loaded from the embedded seed dataset.
pub struct Registry {
    servers: Vec<ServerMetadata>,
}

impl Registry {
    /// Builds a registry from embedded seed JSON.
    pub fn from_seed() -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_from_seed() {
        let registry = Registry::from_seed();
        assert_eq!(registry.list_all().len(), 10);
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
}
