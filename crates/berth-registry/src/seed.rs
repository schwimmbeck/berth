// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Embedded seed registry loader.

use crate::types::ServerMetadata;

const SEED_DATA: &str = include_str!("../data/seed_registry.json");

/// Parses the embedded seed registry JSON into typed metadata.
pub fn load_seed_registry() -> Vec<ServerMetadata> {
    serde_json::from_str(SEED_DATA).expect("embedded seed registry should be valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_registry_parses() {
        let servers = load_seed_registry();
        assert_eq!(servers.len(), 30);
    }

    #[test]
    fn seed_registry_has_expected_servers() {
        let servers = load_seed_registry();
        let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"github"));
        assert!(names.contains(&"filesystem"));
        assert!(names.contains(&"brave-search"));
        assert!(names.contains(&"postgres"));
        assert!(names.contains(&"slack"));
        assert!(names.contains(&"notion"));
        assert!(names.contains(&"google-drive"));
        assert!(names.contains(&"sqlite"));
        assert!(names.contains(&"fetch"));
        assert!(names.contains(&"memory"));
        assert!(names.contains(&"puppeteer"));
        assert!(names.contains(&"sequential-thinking"));
        assert!(names.contains(&"google-maps"));
        assert!(names.contains(&"docker"));
        assert!(names.contains(&"kubernetes"));
        assert!(names.contains(&"aws"));
        assert!(names.contains(&"linear"));
        assert!(names.contains(&"gitlab"));
        assert!(names.contains(&"sentry"));
        assert!(names.contains(&"datadog"));
        assert!(names.contains(&"redis"));
        assert!(names.contains(&"mongodb"));
        assert!(names.contains(&"stripe"));
        assert!(names.contains(&"shopify"));
        assert!(names.contains(&"twilio"));
        assert!(names.contains(&"sendgrid"));
        assert!(names.contains(&"figma"));
        assert!(names.contains(&"vercel"));
        assert!(names.contains(&"supabase"));
        assert!(names.contains(&"prisma"));
    }
}
