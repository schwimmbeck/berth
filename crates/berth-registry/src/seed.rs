use crate::types::ServerMetadata;

const SEED_DATA: &str = include_str!("../data/seed_registry.json");

pub fn load_seed_registry() -> Vec<ServerMetadata> {
    serde_json::from_str(SEED_DATA).expect("embedded seed registry should be valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_registry_parses() {
        let servers = load_seed_registry();
        assert_eq!(servers.len(), 5);
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
    }
}
