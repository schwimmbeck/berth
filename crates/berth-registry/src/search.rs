// SPDX-License-Identifier: Apache-2.0

//! Search and ranking helpers for registry servers.

use crate::types::ServerMetadata;

/// Ranked search result pairing metadata with a relevance score.
#[derive(Debug)]
pub struct SearchResult<'a> {
    pub server: &'a ServerMetadata,
    pub score: u32,
}

/// Performs keyword search and returns descending relevance results.
pub fn search_servers<'a>(servers: &'a [ServerMetadata], query: &str) -> Vec<SearchResult<'a>> {
    let query_lower = query.to_lowercase();

    let mut results: Vec<SearchResult<'a>> = servers
        .iter()
        .filter_map(|server| {
            let score = relevance_score(server, &query_lower);
            if score > 0 {
                Some(SearchResult { server, score })
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| b.score.cmp(&a.score));
    results
}

/// Finds a server by exact name.
pub fn find_server<'a>(servers: &'a [ServerMetadata], name: &str) -> Option<&'a ServerMetadata> {
    servers.iter().find(|s| s.name == name)
}

/// Computes relevance score for a server against a lowercase query.
fn relevance_score(server: &ServerMetadata, query: &str) -> u32 {
    let mut score = 0u32;

    // Exact name match (highest priority)
    if server.name.to_lowercase() == query {
        score += 100;
    }
    // Name contains query
    else if server.name.to_lowercase().contains(query) {
        score += 60;
    }

    // Display name contains query
    if server.display_name.to_lowercase().contains(query) {
        score += 40;
    }

    // Exact tag match
    if server.tags.iter().any(|t| t.to_lowercase() == query) {
        score += 30;
    }

    // Category match
    if server.category.to_lowercase().contains(query) {
        score += 20;
    }

    // Description contains query
    if server.description.to_lowercase().contains(query) {
        score += 10;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_registry;

    #[test]
    fn search_exact_name() {
        let servers = load_seed_registry();
        let results = search_servers(&servers, "github");
        assert!(!results.is_empty());
        assert_eq!(results[0].server.name, "github");
        assert!(results[0].score >= 100);
    }

    #[test]
    fn search_partial_name() {
        let servers = load_seed_registry();
        let results = search_servers(&servers, "post");
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.server.name == "postgres"));
    }

    #[test]
    fn search_by_tag() {
        let servers = load_seed_registry();
        let results = search_servers(&servers, "sql");
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.server.name == "postgres"));
    }

    #[test]
    fn search_by_category() {
        let servers = load_seed_registry();
        let results = search_servers(&servers, "search");
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.server.name == "brave-search"));
    }

    #[test]
    fn search_case_insensitive() {
        let servers = load_seed_registry();
        let results = search_servers(&servers, "GitHub");
        assert!(!results.is_empty());
        assert_eq!(results[0].server.name, "github");
    }

    #[test]
    fn search_no_results() {
        let servers = load_seed_registry();
        let results = search_servers(&servers, "nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn find_server_exact() {
        let servers = load_seed_registry();
        let server = find_server(&servers, "github");
        assert!(server.is_some());
        assert_eq!(server.unwrap().display_name, "GitHub MCP Server");
    }

    #[test]
    fn find_server_not_found() {
        let servers = load_seed_registry();
        let server = find_server(&servers, "nonexistent");
        assert!(server.is_none());
    }
}
