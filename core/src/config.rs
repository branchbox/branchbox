//! BranchBox configuration schema and helpers.
//!
//! This module centralizes serialization logic for project-level configuration
//! stored under `.branchbox/config.json`. It currently focuses on tunnel
//! defaults, leaving room for future workspace metadata.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_VERSION: &str = "1";

/// Complete BranchBox configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BranchBoxConfig {
    #[serde(default = "default_version")]
    pub version: String,

    #[serde(default)]
    pub tunnel: TunnelSettings,
}

impl Default for BranchBoxConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION.to_string(),
            tunnel: TunnelSettings::default(),
        }
    }
}

impl BranchBoxConfig {
    /// Returns path to `.branchbox/config.json` under the provided workspace.
    pub fn path(workspace: &Path) -> PathBuf {
        workspace.join(".branchbox").join("config.json")
    }

    /// Load configuration from disk if present, otherwise return defaults.
    pub fn load(workspace: &Path) -> Result<Self> {
        let path = Self::path(workspace);
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)?;
        let mut config: BranchBoxConfig = serde_json::from_str(&content)?;

        // Ensure version upgraded when blank.
        if config.version.is_empty() {
            config.version = CONFIG_VERSION.to_string();
        }

        Ok(config)
    }

    /// Persist configuration to disk.
    pub fn save(&self, workspace: &Path) -> Result<()> {
        let config_dir = workspace.join(".branchbox");
        fs::create_dir_all(&config_dir)?;

        let path = config_dir.join("config.json");
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;

        Ok(())
    }
}

fn default_version() -> String {
    CONFIG_VERSION.to_string()
}

/// Global tunnel settings for the project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TunnelSettings {
    #[serde(default = "default_tunnel_enabled")]
    pub enabled: bool,

    #[serde(default)]
    pub default_provider: Option<String>,

    #[serde(default)]
    pub providers: TunnelProviders,
}

impl TunnelSettings {
    /// Ensure defaults align with BranchBox expectations (enabled + Cloudflared).
    pub fn ensure_defaults(&mut self) {
        if self.default_provider.is_none() {
            self.default_provider = Some("cloudflared".to_string());
        }
    }

    /// Returns `true` when a Cloudflared config exists.
    pub fn has_cloudflared(&self) -> bool {
        self.providers.cloudflared.is_some()
    }
}

impl Default for TunnelSettings {
    fn default() -> Self {
        let mut settings = TunnelSettings {
            enabled: true,
            default_provider: Some("cloudflared".to_string()),
            providers: TunnelProviders::default(),
        };
        settings.ensure_defaults();
        settings
    }
}

fn default_tunnel_enabled() -> bool {
    true
}

/// Collection of provider-specific configuration values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TunnelProviders {
    #[serde(default)]
    pub cloudflared: Option<CloudflaredConfig>,
}

/// Cloudflared-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudflaredConfig {
    pub account_id: Option<String>,

    /// Path to file containing API token (optional for manual setups).
    #[serde(default)]
    pub api_token_path: Option<PathBuf>,

    #[serde(default)]
    pub tunnel_name_prefix: Option<String>,

    /// Root DNS zone (e.g., `example.com`) used when creating proxied records.
    #[serde(default)]
    pub dns_zone: Option<String>,

    #[serde(default)]
    pub manual_instructions: bool,
}

impl Default for CloudflaredConfig {
    fn default() -> Self {
        Self {
            account_id: None,
            api_token_path: None,
            tunnel_name_prefix: Some("branchbox".to_string()),
            dns_zone: None,
            manual_instructions: true,
        }
    }
}

impl CloudflaredConfig {
    /// Returns path to the default secure credentials file.
    pub fn default_credentials_path(workspace: &Path) -> PathBuf {
        workspace
            .join(".branchbox")
            .join("secure")
            .join("cloudflared.env")
    }

    /// Whether API token is available.
    pub fn has_api_token(&self) -> bool {
        self.api_token_path.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_has_tunnel_enabled() {
        let config = BranchBoxConfig::default();
        assert!(config.tunnel.enabled);
        assert_eq!(
            config.tunnel.default_provider.as_deref(),
            Some("cloudflared")
        );
    }

    #[test]
    fn config_round_trip() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path();

        let mut config = BranchBoxConfig::default();
        config.tunnel.providers.cloudflared = Some(CloudflaredConfig {
            account_id: Some("abc123".to_string()),
            ..Default::default()
        });

        config.save(workspace).unwrap();

        let loaded = BranchBoxConfig::load(workspace).unwrap();
        assert_eq!(config, loaded);
    }
}
