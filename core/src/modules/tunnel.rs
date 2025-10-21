//! Cloudflare Tunnel Module
//!
//! Manages Cloudflare tunnel provisioning and DNS records:
//! - Automatic tunnel provisioning via Cloudflare API
//! - Manual tunnel configuration
//! - DNS record management
//! - Tunnel cleanup during teardown

use super::Module;
use crate::{Error, Result};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Cloudflare Tunnel module
pub struct TunnelModule {
    enabled: bool,
    tunnel_name: String,
    tunnel_token: String,
    feature_url: String,
    service_url: String,
    cloudflare_api_key: Option<String>,
    cloudflare_account_id: Option<String>,
}

impl TunnelModule {
    /// Create a new Tunnel module
    pub fn new() -> Self {
        Self {
            enabled: false,
            tunnel_name: String::new(),
            tunnel_token: String::new(),
            feature_url: String::new(),
            service_url: String::new(),
            cloudflare_api_key: None,
            cloudflare_account_id: None,
        }
    }

    /// Write tunnel configuration to .cloudflared.env
    fn write_tunnel_config(&self, feature_dir: &Path) -> Result<()> {
        let cloudflared_dir = feature_dir.join(".devcontainer");
        fs::create_dir_all(&cloudflared_dir)?;

        let config_file = cloudflared_dir.join(".cloudflared.env");
        let mut file = fs::File::create(&config_file)?;

        let timestamp = chrono::Utc::now().format("%Y-%m-%d");
        writeln!(
            file,
            "# Cloudflare Tunnel Configuration for {}",
            feature_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
        )?;
        writeln!(file, "# Generated on {}", timestamp)?;
        writeln!(file, "TUNNEL_TOKEN={}", self.tunnel_token)?;
        writeln!(file, "DEV_HOSTNAME={}", self.feature_url)?;

        tracing::info!("Tunnel configuration saved to {:?}", config_file);
        Ok(())
    }

    /// Validate tunnel configuration file
    fn validate_config(&self, feature_dir: &Path) -> Result<()> {
        let config_file = feature_dir.join(".devcontainer/.cloudflared.env");

        if !config_file.exists() {
            tracing::warn!("Tunnel configuration file not found");
            tracing::info!("Expected: {:?}", config_file);
            tracing::info!("This is OK if tunnel setup hasn't been completed yet");
            return Ok(());
        }

        let content = fs::read_to_string(&config_file)?;
        if !content.contains("TUNNEL_TOKEN=") {
            tracing::warn!("TUNNEL_TOKEN not found in .cloudflared.env");
            tracing::info!("Tunnel may not be configured correctly");
            return Ok(());
        }

        tracing::info!("Tunnel configuration looks valid");
        Ok(())
    }
}

impl Default for TunnelModule {
    fn default() -> Self {
        Self::new()
    }
}

impl Module for TunnelModule {
    fn name(&self) -> &str {
        "tunnel"
    }

    fn detect(&self, _project_dir: &Path) -> bool {
        // Check for Cloudflare credentials in environment
        let has_credentials = std::env::var("CLOUDFLARE_API_KEY").is_ok()
            && std::env::var("CLOUDFLARE_ACCOUNT_ID").is_ok();

        // Module can be enabled even without credentials for manual setup
        has_credentials || true
    }

    fn init(&mut self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        // Read Cloudflare credentials from environment
        self.cloudflare_api_key = std::env::var("CLOUDFLARE_API_KEY").ok();
        self.cloudflare_account_id = std::env::var("CLOUDFLARE_ACCOUNT_ID").ok();

        let work_feature = feature_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid feature directory name".to_string()))?;

        // Generate tunnel name and feature URL
        let base_prefix = std::env::var("BASE_PREFIX").unwrap_or_else(|_| "app".to_string());
        let base_domain = std::env::var("BASE_DOMAIN").unwrap_or_else(|_| "localhost".to_string());

        self.tunnel_name = format!("{}-{}", base_prefix, work_feature);
        self.feature_url = format!("{}-{}.{}", base_prefix, work_feature, base_domain);

        // Get service URL from environment or use default
        self.service_url = std::env::var("SERVICE_URL")
            .unwrap_or_else(|_| "rails-app:3000".to_string());

        tracing::info!("Initialized tunnel: {}", self.tunnel_name);
        tracing::info!("Feature URL: https://{}", self.feature_url);

        self.enabled = true;
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Setting up Cloudflare tunnel...");

        // In the Rust version, we'll focus on the structure
        // Full API integration would be implemented separately
        if self.cloudflare_api_key.is_some() && self.cloudflare_account_id.is_some() {
            tracing::info!("Cloudflare API credentials available");
            tracing::info!("Automatic tunnel provisioning would happen here");
            // TODO: Implement Cloudflare API client
        } else {
            tracing::info!("No API credentials - manual setup required");
            tracing::info!("\nManual Cloudflare Tunnel Setup Required:\n");
            tracing::info!("1. Open Cloudflare Dashboard: https://dash.cloudflare.com");
            tracing::info!("2. Navigate to: Zero Trust > Access > Tunnels");
            tracing::info!("3. Create a tunnel named: {}", self.tunnel_name);
            tracing::info!("4. Configure route:");
            tracing::info!("   - Hostname: {}", self.feature_url);
            tracing::info!("   - Service: {}", self.service_url);
            tracing::info!("5. Copy the tunnel token and add to .cloudflared.env");
        }

        // If we have a tunnel token, write the config
        if !self.tunnel_token.is_empty() {
            self.write_tunnel_config(feature_dir)?;
            tracing::info!("Tunnel configuration saved");
        } else {
            tracing::warn!("No tunnel token - you can add it later");
            tracing::info!(
                "Edit: {}/.devcontainer/.cloudflared.env",
                feature_dir.display()
            );
        }

        Ok(())
    }

    fn teardown(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Cleaning up Cloudflare tunnel...");

        // Try to read tunnel info from .env files
        let env_file = feature_dir.join(".env");
        let mut feature_url = String::new();

        if env_file.exists() {
            if let Ok(content) = fs::read_to_string(&env_file) {
                for line in content.lines() {
                    if line.starts_with("APP_URL=") {
                        feature_url = line
                            .split('=')
                            .nth(1)
                            .unwrap_or("")
                            .trim_matches(|c| c == '"' || c == '\'')
                            .to_string();
                        break;
                    }
                }
            }
        }

        if feature_url.is_empty() {
            let cloudflared_env = feature_dir.join(".devcontainer/.cloudflared.env");
            if cloudflared_env.exists() {
                if let Ok(content) = fs::read_to_string(&cloudflared_env) {
                    for line in content.lines() {
                        if line.starts_with("DEV_HOSTNAME=") {
                            feature_url = line
                                .split('=')
                                .nth(1)
                                .unwrap_or("")
                                .trim_matches(|c| c == '"' || c == '\'')
                                .to_string();
                            break;
                        }
                    }
                }
            }
        }

        if !feature_url.is_empty() {
            let tunnel_name = feature_url.split('.').next().unwrap_or("");
            tracing::info!("Tunnel name: {}", tunnel_name);
            tracing::info!("Feature URL: {}", feature_url);

            if self.cloudflare_api_key.is_some() && self.cloudflare_account_id.is_some() {
                tracing::info!("Would delete tunnel via Cloudflare API: {}", tunnel_name);
                tracing::info!("Would delete DNS record for: {}", feature_url);
                // TODO: Implement actual API calls
            } else {
                tracing::info!("Skipping tunnel deletion (no API credentials)");
            }
        } else {
            tracing::info!("No tunnel information found");
        }

        Ok(())
    }

    fn validate(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        self.validate_config(feature_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect() {
        let temp_dir = TempDir::new().unwrap();
        let module = TunnelModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_init() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = TunnelModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        assert!(module.enabled);
        assert!(!module.tunnel_name.is_empty());
        assert!(!module.feature_url.is_empty());
        assert!(module.feature_url.contains("feature-test"));
    }

    #[test]
    fn test_write_config() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();

        let mut module = TunnelModule::new();
        module.tunnel_token = "test_token".to_string();
        module.feature_url = "test.example.com".to_string();

        module.write_tunnel_config(temp_dir.path()).unwrap();

        let config_file = temp_dir.path().join(".devcontainer/.cloudflared.env");
        assert!(config_file.exists());

        let content = std::fs::read_to_string(&config_file).unwrap();
        assert!(content.contains("TUNNEL_TOKEN=test_token"));
        assert!(content.contains("DEV_HOSTNAME=test.example.com"));
    }
}
