// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Schwimmbeck Dominik

//! Core registry metadata types parsed from seed JSON.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerMetadata {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub source: ServerSource,
    pub runtime: ServerRuntime,
    pub transport: String,
    pub permissions: ServerPermissions,
    pub config: ServerConfig,
    pub compatibility: ServerCompatibility,
    pub quality: ServerQuality,
    pub category: String,
    pub tags: Vec<String>,
    pub maintainer: String,
    pub trust_level: TrustLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub package: String,
    pub repository: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerRuntime {
    #[serde(rename = "type")]
    pub runtime_type: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPermissions {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub exec: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub required: Vec<ConfigField>,
    pub optional: Vec<ConfigField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    pub key: String,
    #[serde(default)]
    pub env: Option<String>,
    pub description: String,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCompatibility {
    pub clients: Vec<String>,
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerQuality {
    pub security_scan: String,
    pub health_check: bool,
    pub last_verified: String,
    pub downloads: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    Untrusted,
    Community,
    Verified,
    Official,
}

impl fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrustLevel::Untrusted => write!(f, "untrusted"),
            TrustLevel::Community => write!(f, "community"),
            TrustLevel::Verified => write!(f, "verified"),
            TrustLevel::Official => write!(f, "official"),
        }
    }
}
