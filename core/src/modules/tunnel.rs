//! Cloudflare Tunnel Module
//!
//! Manages Cloudflare tunnel provisioning and DNS records:
//! - Automatic tunnel provisioning via Cloudflare API
//! - Manual tunnel configuration
//! - DNS record management
//! - Tunnel cleanup during teardown

use super::Module;
use crate::{
    cloudflare::{CloudflareClient, TunnelProvision, TunnelSummary},
    Error, Result,
};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Cloudflare Tunnel module
pub struct TunnelModule {
    enabled: bool,
    tunnel_name: String,
    feature_url: String,
    base_domain: String,
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
            feature_url: String::new(),
            base_domain: String::new(),
            service_url: String::new(),
            cloudflare_api_key: None,
            cloudflare_account_id: None,
        }
    }

    /// Write tunnel configuration to .cloudflared.env
    fn write_tunnel_config(
        &self,
        feature_dir: &Path,
        tunnel_token: &str,
        feature_hostname: &str,
    ) -> Result<()> {
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
        writeln!(file, "TUNNEL_TOKEN={}", tunnel_token)?;
        writeln!(file, "DEV_HOSTNAME={}", feature_hostname)?;

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

    fn read_existing_token(&self, feature_dir: &Path) -> Option<String> {
        let token_path = feature_dir.join(".devcontainer/.cloudflared.env");
        fs::read_to_string(token_path).ok().and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("TUNNEL_TOKEN="))
                .and_then(|line| line.split('=').nth(1))
                .map(|value| value.trim().trim_matches(&['"', '\'']).to_string())
        })
    }

    fn manual_setup_instructions(&self) {
        tracing::info!("\nManual Cloudflare Tunnel Setup Required:\n");
        tracing::info!("1. Open Cloudflare Dashboard: https://dash.cloudflare.com");
        tracing::info!("2. Navigate to: Zero Trust > Access > Tunnels");
        tracing::info!("3. Create a tunnel named: {}", self.tunnel_name);
        tracing::info!("4. Configure route:");
        tracing::info!("   - Hostname: {}", self.feature_url);
        tracing::info!("   - Service: {}", self.service_url);
        tracing::info!("5. Copy the tunnel token and add to .cloudflared.env");
        tracing::info!("   (or export CLOUDFLARE_TUNNEL_TOKEN for non-interactive usage)");
    }

    fn build_client(&self) -> Result<CloudflareClient> {
        let api_key = self
            .cloudflare_api_key
            .as_ref()
            .ok_or_else(|| Error::validation("Missing Cloudflare API key"))?;
        let account_id = self
            .cloudflare_account_id
            .as_ref()
            .ok_or_else(|| Error::validation("Missing Cloudflare account id"))?;

        CloudflareClient::new(api_key.clone(), account_id.clone())
    }

    fn ensure_dns(&self, client: &CloudflareClient, tunnel: &TunnelSummary) -> Result<()> {
        client.ensure_cname_record(&self.feature_url, &self.base_domain, &tunnel.id)
    }

    fn ensure_dns_with_provision(
        &self,
        client: &CloudflareClient,
        provision: &TunnelProvision,
    ) -> Result<()> {
        client.ensure_cname_record(&self.feature_url, &self.base_domain, &provision.id)
    }

    fn infer_base_domain(&self, hostname: &str) -> String {
        hostname
            .splitn(2, '.')
            .nth(1)
            .map(|rest| rest.to_string())
            .unwrap_or_else(|| self.base_domain.clone())
    }

    fn tunnel_token_from_env(&self) -> Option<String> {
        std::env::var("CLOUDFLARE_TUNNEL_TOKEN").ok()
    }

    fn find_feature_hostname(&self, feature_dir: &Path) -> Option<String> {
        let env_candidate = feature_dir.join(".env");
        if let Some(host) = Self::extract_env_var(&env_candidate, "APP_URL") {
            return Some(host);
        }

        let cloudflared = feature_dir.join(".devcontainer/.cloudflared.env");
        Self::extract_env_var(&cloudflared, "DEV_HOSTNAME")
    }

    fn extract_env_var(path: &Path, key: &str) -> Option<String> {
        fs::read_to_string(path).ok().and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with(key))
                .and_then(|line| line.split('=').nth(1))
                .map(|value| value.trim().trim_matches(&['"', '\'']).to_string())
        })
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
        self.base_domain = std::env::var("BASE_DOMAIN").unwrap_or_else(|_| "localhost".to_string());

        self.tunnel_name = format!("{}-{}", base_prefix, work_feature);
        self.feature_url = format!("{}-{}.{}", base_prefix, work_feature, self.base_domain);

        // Get service URL from environment or use default
        self.service_url =
            std::env::var("SERVICE_URL").unwrap_or_else(|_| "rails-app:3000".to_string());

        tracing::info!("Initialized tunnel: {}", self.tunnel_name);
        tracing::info!("Feature URL: https://{}", self.feature_url);

        self.enabled = true;
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Setting up Cloudflare tunnel...");

        let mut tunnel_token = self
            .read_existing_token(feature_dir)
            .or_else(|| self.tunnel_token_from_env());

        if self.cloudflare_api_key.is_some() && self.cloudflare_account_id.is_some() {
            match self.build_client() {
                Ok(client) => match client.find_tunnel_by_name(&self.tunnel_name) {
                    Ok(Some(existing)) => {
                        tracing::warn!(
                                "Tunnel '{}' already exists in Cloudflare; token cannot be retrieved via API",
                                existing.name
                            );
                        if let Err(err) = self.ensure_dns(&client, &existing) {
                            tracing::warn!(
                                "Failed to ensure DNS for existing tunnel '{}': {}",
                                existing.name,
                                err
                            );
                        }
                        if tunnel_token.is_none() {
                            tracing::info!(
                                    "Provide the existing tunnel token via CLOUDFLARE_TUNNEL_TOKEN or update .cloudflared.env manually."
                                );
                        }
                    }
                    Ok(None) => match client.create_tunnel(&self.tunnel_name) {
                        Ok(provision) => {
                            tracing::info!("Provisioned Cloudflare tunnel '{}'", provision.name);
                            if let Err(err) = client.configure_tunnel(
                                &provision.id,
                                &self.feature_url,
                                &self.service_url,
                            ) {
                                tracing::warn!("Failed to configure tunnel ingress: {}", err);
                            }

                            if let Err(err) = self.ensure_dns_with_provision(&client, &provision) {
                                tracing::warn!(
                                    "Failed to configure DNS record for {}: {}",
                                    self.feature_url,
                                    err
                                );
                            }

                            tunnel_token = Some(provision.token);
                        }
                        Err(err) => {
                            tracing::warn!(
                                "Failed to provision tunnel via Cloudflare API: {}",
                                err
                            );
                            self.manual_setup_instructions();
                        }
                    },
                    Err(err) => {
                        tracing::warn!("Failed to query Cloudflare tunnels: {}", err);
                        self.manual_setup_instructions();
                    }
                },
                Err(err) => {
                    tracing::warn!("Failed to initialize Cloudflare client: {}", err);
                    self.manual_setup_instructions();
                }
            }
        } else {
            tracing::info!("No Cloudflare API credentials detected.");
            self.manual_setup_instructions();
        }

        if let Some(token) = tunnel_token {
            self.write_tunnel_config(feature_dir, &token, &self.feature_url)?;
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

        let feature_url = self.find_feature_hostname(feature_dir).unwrap_or_default();

        if feature_url.is_empty() {
            tracing::info!("No tunnel information found");
            return Ok(());
        }

        let tunnel_name = feature_url.split('.').next().unwrap_or("");
        tracing::info!("Tunnel name: {}", tunnel_name);
        tracing::info!("Feature URL: {}", feature_url);

        if self.cloudflare_api_key.is_some() && self.cloudflare_account_id.is_some() {
            match self.build_client() {
                Ok(client) => {
                    match client.find_tunnel_by_name(tunnel_name) {
                        Ok(Some(summary)) => match client.delete_tunnel(&summary.id) {
                            Ok(_) => tracing::info!("Tunnel '{}' deleted via API", summary.name),
                            Err(err) => tracing::warn!(
                                "Failed to delete tunnel '{}': {}",
                                summary.name,
                                err
                            ),
                        },
                        Ok(None) => {
                            tracing::info!(
                                "Tunnel '{}' not found (might already be removed)",
                                tunnel_name
                            );
                        }
                        Err(err) => {
                            tracing::warn!("Failed to look up tunnel '{}': {}", tunnel_name, err);
                        }
                    }

                    let base_domain = self.infer_base_domain(&feature_url);
                    match client.delete_dns_record(&feature_url, &base_domain) {
                        Ok(true) => tracing::info!("DNS record {} removed", feature_url),
                        Ok(false) => tracing::info!("DNS record {} not found", feature_url),
                        Err(err) => tracing::warn!(
                            "Failed to delete DNS record for {}: {}",
                            feature_url,
                            err
                        ),
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to initialize Cloudflare client for teardown: {}",
                        err
                    );
                }
            }
        } else {
            tracing::info!("Skipping Cloudflare API cleanup (no credentials detected)");
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

        let module = TunnelModule::new();

        module
            .write_tunnel_config(temp_dir.path(), "test_token", "test.example.com")
            .unwrap();

        let config_file = temp_dir.path().join(".devcontainer/.cloudflared.env");
        assert!(config_file.exists());

        let content = std::fs::read_to_string(&config_file).unwrap();
        assert!(content.contains("TUNNEL_TOKEN=test_token"));
        assert!(content.contains("DEV_HOSTNAME=test.example.com"));
    }

    #[test]
    fn read_existing_token_extracts_value() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/.cloudflared.env"),
            "TUNNEL_TOKEN=abc123\nDEV_HOSTNAME=test.example.com",
        )
        .unwrap();

        let module = TunnelModule::new();
        let token = module.read_existing_token(temp_dir.path());
        assert_eq!(token.as_deref(), Some("abc123"));
    }
}
