use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::types::ServerMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledServer {
    pub server: ServerInfo,
    pub source: SourceInfo,
    pub runtime: RuntimeInfo,
    pub permissions: PermissionsInfo,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub config_meta: ConfigMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub maintainer: String,
    pub trust_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    #[serde(rename = "type")]
    pub source_type: String,
    pub package: String,
    pub repository: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    #[serde(rename = "type")]
    pub runtime_type: String,
    pub command: String,
    pub args: Vec<String>,
    pub transport: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsInfo {
    pub network: Vec<String>,
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigMeta {
    pub required_keys: Vec<String>,
    pub optional_keys: Vec<String>,
}

impl InstalledServer {
    pub fn from_metadata(meta: &ServerMetadata) -> Self {
        let mut config = BTreeMap::new();

        // Pre-populate required keys with empty strings
        for field in &meta.config.required {
            config.insert(field.key.clone(), String::new());
        }

        // Pre-populate optional keys with defaults if available
        for field in &meta.config.optional {
            let value = field.default.clone().unwrap_or_default();
            config.insert(field.key.clone(), value);
        }

        let required_keys = meta.config.required.iter().map(|f| f.key.clone()).collect();
        let optional_keys = meta.config.optional.iter().map(|f| f.key.clone()).collect();

        InstalledServer {
            server: ServerInfo {
                name: meta.name.clone(),
                display_name: meta.display_name.clone(),
                version: meta.version.clone(),
                description: meta.description.clone(),
                category: meta.category.clone(),
                maintainer: meta.maintainer.clone(),
                trust_level: meta.trust_level.to_string(),
            },
            source: SourceInfo {
                source_type: meta.source.source_type.clone(),
                package: meta.source.package.clone(),
                repository: meta.source.repository.clone(),
            },
            runtime: RuntimeInfo {
                runtime_type: meta.runtime.runtime_type.clone(),
                command: meta.runtime.command.clone(),
                args: meta.runtime.args.clone(),
                transport: meta.transport.clone(),
            },
            permissions: PermissionsInfo {
                network: meta.permissions.network.clone(),
                env: meta.permissions.env.clone(),
            },
            config,
            config_meta: ConfigMeta {
                required_keys,
                optional_keys,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;

    fn github_metadata() -> ServerMetadata {
        let registry = Registry::from_seed();
        registry.get("github").unwrap().clone()
    }

    #[test]
    fn from_metadata_sets_server_fields() {
        let meta = github_metadata();
        let installed = InstalledServer::from_metadata(&meta);

        assert_eq!(installed.server.name, "github");
        assert_eq!(installed.server.display_name, "GitHub MCP Server");
        assert_eq!(installed.server.version, "1.2.0");
        assert_eq!(installed.server.trust_level, "official");
    }

    #[test]
    fn from_metadata_sets_source_and_runtime() {
        let meta = github_metadata();
        let installed = InstalledServer::from_metadata(&meta);

        assert_eq!(installed.source.source_type, "npm");
        assert_eq!(installed.runtime.runtime_type, "node");
        assert_eq!(installed.runtime.command, "npx");
        assert_eq!(installed.runtime.transport, "stdio");
    }

    #[test]
    fn from_metadata_populates_required_keys_empty() {
        let meta = github_metadata();
        let installed = InstalledServer::from_metadata(&meta);

        // Required key "token" should exist with empty value
        assert_eq!(installed.config.get("token"), Some(&String::new()));
        assert!(installed
            .config_meta
            .required_keys
            .contains(&"token".to_string()));
    }

    #[test]
    fn from_metadata_populates_optional_keys_with_defaults() {
        let meta = github_metadata();
        let installed = InstalledServer::from_metadata(&meta);

        // Optional keys should exist
        for key in &installed.config_meta.optional_keys {
            assert!(installed.config.contains_key(key));
        }
    }

    #[test]
    fn toml_round_trip() {
        let meta = github_metadata();
        let installed = InstalledServer::from_metadata(&meta);

        let toml_str = toml::to_string_pretty(&installed).unwrap();
        let deserialized: InstalledServer = toml::from_str(&toml_str).unwrap();

        assert_eq!(deserialized.server.name, installed.server.name);
        assert_eq!(deserialized.server.version, installed.server.version);
        assert_eq!(deserialized.config.len(), installed.config.len());
        assert_eq!(
            deserialized.config_meta.required_keys,
            installed.config_meta.required_keys
        );
    }
}
