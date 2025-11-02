//! Cloudflared tunnel provider.
//!
//! Provides automated provisioning, DNS management, and teardown when the
//! workspace is configured with Cloudflare API credentials. Falls back to
//! manual instructions when automation is unavailable.

use super::{
    ManualInstructions, ProvisioningIntent, ProvisioningOutcome, TunnelDescriptor, TunnelProvider,
    TunnelStatus,
};
use crate::cloudflare::{CloudflareClient, TunnelProvision, TunnelSummary};
use crate::{config::CloudflaredConfig, Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Cloudflared provider wrapping the workspace-level configuration.
#[derive(Debug)]
pub struct CloudflaredProvider<'a> {
    config: &'a CloudflaredConfig,
    workspace_root: &'a Path,
}

impl<'a> CloudflaredProvider<'a> {
    /// Construct a provider tied to the given workspace root + config.
    pub fn new(config: &'a CloudflaredConfig, workspace_root: &'a Path) -> Self {
        Self {
            config,
            workspace_root,
        }
    }

    fn credentials_path(&self) -> Option<PathBuf> {
        self.config.api_token_path.clone().map(|path| {
            if path.is_absolute() {
                path
            } else {
                self.workspace_root.join(path)
            }
        })
    }

    fn has_automation_credentials(&self) -> bool {
        self.config
            .account_id
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
            && self.config.api_token_path.is_some()
            && !self.config.manual_instructions
    }

    fn manual_steps(
        &self,
        intent: &ProvisioningIntent<'_>,
        reason: impl Into<String>,
    ) -> ManualInstructions {
        let reason = reason.into();
        let mut steps = vec![
            "Sign in to Cloudflare Zero Trust and navigate to Access → Tunnels.".to_string(),
            format!(
                "Create (or reuse) a tunnel named `{}`.",
                intent.feature_name.replace('/', "-")
            ),
            format!(
                "Configure the route so `{}` proxies traffic to `{}`.",
                intent.hostname, intent.service_url
            ),
            "Download the tunnel credentials JSON or copy the tunnel token.".to_string(),
        ];

        let credentials_hint = if let Some(path) = self.credentials_path() {
            format!(
                "Store the token in `{}` or place it in `.devcontainer/.cloudflared.env`.",
                path.display()
            )
        } else {
            "Store the token in `.devcontainer/.cloudflared.env` (TUNNEL_TOKEN=...)".to_string()
        };
        steps.push(credentials_hint);
        steps.push(
            "Refer to `legacy/cloudflared/README.md` for script-driven helpers until automation lands.".to_string(),
        );

        ManualInstructions { reason, steps }
    }

    fn load_api_token(&self) -> Result<String> {
        let path = self
            .credentials_path()
            .ok_or_else(|| Error::validation("Cloudflare API token path not configured"))?;

        let content = fs::read_to_string(&path).map_err(|err| {
            Error::validation(format!(
                "Failed to read Cloudflare API token file {}: {}",
                path.display(),
                err
            ))
        })?;

        for line in content.lines() {
            if let Some(value) = line.strip_prefix("CLOUDFLARE_API_TOKEN=") {
                return Ok(value.trim().trim_matches(['"', '\'']).to_string());
            }
            if let Some(value) = line.strip_prefix("CLOUDFLARE_API_KEY=") {
                return Ok(value.trim().trim_matches(['"', '\'']).to_string());
            }
        }

        Err(Error::validation(format!(
            "CLOUDFLARE_API_TOKEN not found in {}",
            path.display()
        )))
    }

    fn account_id(&self) -> Result<String> {
        self.config
            .account_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::validation("Cloudflare account ID missing from configuration"))
    }

    fn build_client(&self) -> Result<CloudflareClient> {
        let token = self.load_api_token()?;
        let account_id = self.account_id()?;
        if let Ok(base) = std::env::var("BRANCHBOX_CLOUDFLARE_API_BASE") {
            CloudflareClient::with_base_url(token, account_id, base)
        } else {
            CloudflareClient::new(token, account_id)
        }
    }

    fn tunnel_name(&self, feature_name: &str) -> String {
        let prefix = self
            .config
            .tunnel_name_prefix
            .clone()
            .unwrap_or_else(|| "branchbox".to_string());
        format!("{}-{}", prefix, feature_name.replace(['/', ':', ' '], "-"))
    }

    fn dns_zone(&self, hostname: &str) -> Result<String> {
        if let Some(zone) = self
            .config
            .dns_zone
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            return Ok(zone.to_string());
        }

        let parts: Vec<&str> = hostname.split('.').collect();
        if parts.len() < 2 {
            return Err(Error::validation(format!(
                "Cannot derive DNS zone from hostname '{}'",
                hostname
            )));
        }

        // Heuristic: derive the zone by taking the last two segments of the hostname.
        // Multi-part public suffixes (e.g., `.co.uk`) are not handled; configure `dns_zone`
        // explicitly for those cases.
        let zone = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
        Ok(zone)
    }

    fn tunnel_token_store(&self) -> PathBuf {
        self.workspace_root
            .join(".branchbox")
            .join("secure")
            .join("tunnels")
    }

    fn sanitized_feature_name(&self, feature_name: &str) -> String {
        feature_name.replace(['/', ':', ' '], "-")
    }

    fn connector_token_path(&self, feature_name: &str) -> PathBuf {
        let sanitized = self.sanitized_feature_name(feature_name);
        self.tunnel_token_store().join(format!("{}.env", sanitized))
    }

    fn existing_connector_token_path(&self, feature_name: &str) -> Option<PathBuf> {
        let path = self.connector_token_path(feature_name);
        if path.exists() {
            Some(path.canonicalize().unwrap_or(path))
        } else {
            None
        }
    }

    fn write_connector_token(&self, feature_name: &str, token: &str) -> Result<PathBuf> {
        let store = self.tunnel_token_store();
        fs::create_dir_all(&store)?;

        let path = self.connector_token_path(feature_name);

        fs::write(
            &path,
            format!(
                "TUNNEL_TOKEN={}\nFEATURE_NAME={}\n",
                token.trim(),
                self.sanitized_feature_name(feature_name)
            ),
        )?;

        Ok(path.canonicalize().unwrap_or(path))
    }

    fn configure_existing_tunnel(
        &self,
        client: &CloudflareClient,
        summary: &TunnelSummary,
        intent: &ProvisioningIntent<'_>,
        dns_zone: &str,
    ) -> Result<()> {
        client.configure_tunnel(&summary.id, intent.hostname, intent.service_url)?;
        client.ensure_cname_record(intent.hostname, dns_zone, &summary.id)?;
        Ok(())
    }

    fn configure_new_tunnel(
        &self,
        client: &CloudflareClient,
        provision: &TunnelProvision,
        intent: &ProvisioningIntent<'_>,
        dns_zone: &str,
    ) -> Result<()> {
        client.configure_tunnel(&provision.id, intent.hostname, intent.service_url)?;
        client.ensure_cname_record(intent.hostname, dns_zone, &provision.id)?;
        Ok(())
    }
}

impl<'a> TunnelProvider for CloudflaredProvider<'a> {
    fn name(&self) -> &str {
        "cloudflared"
    }

    fn provision(&self, intent: &ProvisioningIntent<'_>) -> Result<ProvisioningOutcome> {
        if !self.has_automation_credentials() {
            let reason = if self.config.manual_instructions {
                "Cloudflare automation disabled during init; follow manual steps."
            } else {
                "Cloudflare credentials missing; manual tunnel provisioning required."
            };
            return Ok(ProvisioningOutcome::Manual(
                self.manual_steps(intent, reason),
            ));
        }

        let client = self.build_client()?;
        let tunnel_name = self.tunnel_name(intent.feature_name);
        let dns_zone = self.dns_zone(intent.hostname)?;

        match client.find_tunnel_by_name(&tunnel_name)? {
            Some(existing) => {
                self.configure_existing_tunnel(&client, &existing, intent, &dns_zone)?;
                let token_path = self.existing_connector_token_path(intent.feature_name);
                let descriptor = TunnelDescriptor {
                    provider: self.name().to_string(),
                    tunnel_name: Some(tunnel_name),
                    tunnel_id: Some(existing.id.clone()),
                    hostname: intent.hostname.to_string(),
                    token_path,
                };
                Ok(ProvisioningOutcome::Automated {
                    descriptor,
                    token: None,
                })
            }
            None => {
                let provision = client.create_tunnel(&tunnel_name)?;
                self.configure_new_tunnel(&client, &provision, intent, &dns_zone)?;

                let token_path =
                    self.write_connector_token(intent.feature_name, &provision.token)?;

                let descriptor = TunnelDescriptor {
                    provider: self.name().to_string(),
                    tunnel_name: Some(tunnel_name),
                    tunnel_id: Some(provision.id.clone()),
                    hostname: intent.hostname.to_string(),
                    token_path: Some(token_path.clone()),
                };

                Ok(ProvisioningOutcome::Automated {
                    descriptor,
                    token: Some(provision.token),
                })
            }
        }
    }

    fn status(&self, descriptor: &TunnelDescriptor) -> Result<TunnelStatus> {
        if !self.has_automation_credentials() {
            return Ok(TunnelStatus::Manual);
        }

        let client = self.build_client()?;
        let tunnel_name = descriptor
            .tunnel_name
            .as_ref()
            .cloned()
            .or_else(|| descriptor.hostname.split('.').next().map(|s| s.to_string()))
            .unwrap_or_default();

        if tunnel_name.is_empty() {
            return Ok(TunnelStatus::Unknown);
        }

        match client.find_tunnel_by_name(&tunnel_name)? {
            Some(_) => Ok(TunnelStatus::Active),
            None => Ok(TunnelStatus::Pending),
        }
    }

    fn teardown(&self, descriptor: &TunnelDescriptor) -> Result<()> {
        if !self.has_automation_credentials() {
            return Ok(());
        }

        let client = self.build_client()?;

        if let Some(tunnel_id) = descriptor.tunnel_id.as_ref() {
            if let Err(err) = client.delete_tunnel(tunnel_id) {
                tracing::warn!("Failed to delete Cloudflare tunnel {}: {}", tunnel_id, err);
            }
        }

        if !descriptor.hostname.is_empty() {
            let zone = self.dns_zone(&descriptor.hostname)?;
            if let Err(err) = client.delete_dns_record(&descriptor.hostname, &zone) {
                tracing::warn!(
                    "Failed to delete DNS record {} in zone {}: {}",
                    descriptor.hostname,
                    zone,
                    err
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CloudflaredConfig;
    use mockito::{Matcher, Server};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn manual_outcome_when_credentials_missing() {
        let config = CloudflaredConfig::default();
        let temp = TempDir::new().unwrap();

        let provider = CloudflaredProvider::new(&config, temp.path());
        let intent = ProvisioningIntent {
            workspace_root: temp.path(),
            feature_name: "feature/login",
            hostname: "login.dev.example.com",
            service_url: "web:3000",
        };

        let outcome = provider.provision(&intent).unwrap();
        assert!(matches!(outcome, ProvisioningOutcome::Manual(_)));
    }

    #[test]
    fn automated_outcome_with_valid_credentials() {
        let mut config = CloudflaredConfig {
            manual_instructions: false,
            account_id: Some("acct".into()),
            dns_zone: Some("example.com".into()),
            ..Default::default()
        };

        let temp = TempDir::new().unwrap();
        let credentials_path = temp.path().join(".branchbox/secure/cloudflared.env");
        fs::create_dir_all(credentials_path.parent().unwrap()).unwrap();
        fs::write(&credentials_path, "CLOUDFLARE_API_TOKEN=test-token\n").unwrap();
        config.api_token_path = Some(credentials_path.clone());

        let mut server = Server::new();
        let base = format!("{}/client/v4", server.url());
        std::env::set_var("BRANCHBOX_CLOUDFLARE_API_BASE", &base);

        let expected_tunnel = "branchbox-feature-login";

        let cfd_path = format!(
            "/client/v4/accounts/{}/cfd_tunnel",
            config.account_id.as_ref().unwrap()
        );

        let find = server
            .mock("GET", Matcher::Exact(cfd_path.clone()))
            .match_query(Matcher::UrlEncoded("name".into(), expected_tunnel.into()))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"errors":[],"result":[]}"#)
            .create();

        let create = server
            .mock("POST", Matcher::Exact(cfd_path.clone()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                "{{\"success\":true,\"errors\":[],\"result\":{{\"id\":\"tunnel-id\",\"name\":\"{}\",\"token\":\"connector-token\"}}}}",
                expected_tunnel
            ))
            .create();

        let configure_path = format!(
            "/client/v4/accounts/{}/cfd_tunnel/{}/configurations",
            config.account_id.as_ref().unwrap(),
            "tunnel-id"
        );

        let configure = server
            .mock("PUT", Matcher::Exact(configure_path))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"errors":[],"result":{}}"#)
            .create();

        let zones = server
            .mock("GET", "/client/v4/zones")
            .match_query(Matcher::UrlEncoded(
                "name".into(),
                config.dns_zone.clone().unwrap(),
            ))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"errors":[],"result":[{"id":"zone-id"}]}"#)
            .create();

        let hostname = "login.dev.example.com";

        let records = server
            .mock(
                "GET",
                Matcher::Exact("/client/v4/zones/zone-id/dns_records".to_string()),
            )
            .match_query(Matcher::UrlEncoded("name".into(), hostname.into()))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"errors":[],"result":[]}"#)
            .create();

        let create_record = server
            .mock(
                "POST",
                Matcher::Exact("/client/v4/zones/zone-id/dns_records".to_string()),
            )
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"success":true,"errors":[],"result":{"id":"dns-id","content":"target"}}"#,
            )
            .create();

        let provider = CloudflaredProvider::new(&config, temp.path());
        let intent = ProvisioningIntent {
            workspace_root: temp.path(),
            feature_name: "feature/login",
            hostname,
            service_url: "web:3000",
        };

        let outcome = provider.provision(&intent).unwrap();

        match outcome {
            ProvisioningOutcome::Automated { descriptor, token } => {
                assert_eq!(descriptor.provider, "cloudflared");
                assert_eq!(descriptor.hostname, hostname);
                assert_eq!(descriptor.tunnel_id.as_deref(), Some("tunnel-id"));
                assert_eq!(descriptor.tunnel_name.as_deref(), Some(expected_tunnel));

                let expected_path = provider.connector_token_path("feature/login");
                assert!(
                    expected_path.exists(),
                    "provider must persist connector token to disk"
                );

                if let Some(token_path) = descriptor.token_path.as_ref() {
                    assert_eq!(
                        token_path, &expected_path,
                        "descriptor token path should point at the file we just wrote"
                    );
                }

                assert_eq!(token.as_deref(), Some("connector-token"));
            }
            other => panic!("unexpected outcome: {:?}", other),
        }

        find.assert();
        create.assert();
        configure.assert();
        zones.assert();
        records.assert();
        create_record.assert();

        std::env::remove_var("BRANCHBOX_CLOUDFLARE_API_BASE");
    }

    #[test]
    fn existing_tunnel_preserves_token_path() {
        let mut config = CloudflaredConfig {
            manual_instructions: false,
            account_id: Some("acct".into()),
            dns_zone: Some("example.com".into()),
            ..Default::default()
        };

        let temp = TempDir::new().unwrap();
        let credentials_path = temp.path().join(".branchbox/secure/cloudflared.env");
        fs::create_dir_all(credentials_path.parent().unwrap()).unwrap();
        fs::write(&credentials_path, "CLOUDFLARE_API_TOKEN=test-token\n").unwrap();
        config.api_token_path = Some(credentials_path);

        let provider = CloudflaredProvider::new(&config, temp.path());
        let _expected_token_path = provider
            .write_connector_token("feature/login", "existing-token")
            .expect("write token");

        let mut server = Server::new();
        let base = format!("{}/client/v4", server.url());
        std::env::set_var("BRANCHBOX_CLOUDFLARE_API_BASE", &base);

        let expected_tunnel = "branchbox-feature-login";
        let hostname = "login.dev.example.com";

        let cfd_path = format!(
            "/client/v4/accounts/{}/cfd_tunnel",
            config.account_id.as_ref().unwrap()
        );

        let find = server
            .mock("GET", Matcher::Exact(cfd_path.clone()))
            .match_query(Matcher::UrlEncoded("name".into(), expected_tunnel.into()))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                "{{\"success\":true,\"errors\":[],\"result\":[{{\"id\":\"tunnel-id\",\"name\":\"{}\",\"deleted_at\":null}}]}}",
                expected_tunnel
            ))
            .create();

        let configure_path = format!(
            "/client/v4/accounts/{}/cfd_tunnel/{}/configurations",
            config.account_id.as_ref().unwrap(),
            "tunnel-id"
        );

        let configure = server
            .mock("PUT", Matcher::Exact(configure_path))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"errors":[],"result":{}}"#)
            .create();

        let zones = server
            .mock("GET", "/client/v4/zones")
            .match_query(Matcher::UrlEncoded(
                "name".into(),
                config.dns_zone.clone().unwrap(),
            ))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"errors":[],"result":[{"id":"zone-id"}]}"#)
            .create();

        let dns_records = server
            .mock(
                "GET",
                Matcher::Exact("/client/v4/zones/zone-id/dns_records".to_string()),
            )
            .match_query(Matcher::UrlEncoded("name".into(), hostname.into()))
            .expect_at_least(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"success":true,"errors":[],"result":[{"id":"record-123","content":"tunnel-id.cfargotunnel.com"}]}"#,
            )
            .create();

        let intent = ProvisioningIntent {
            workspace_root: temp.path(),
            feature_name: "feature/login",
            hostname,
            service_url: "web:3000",
        };

        let outcome = provider.provision(&intent).unwrap();

        match outcome {
            ProvisioningOutcome::Automated { descriptor, token } => {
                assert!(token.is_none(), "existing tunnel should not return token");

                let expected_path = provider.connector_token_path("feature/login");
                assert!(
                    expected_path.exists(),
                    "existing connector token should remain on disk"
                );

                if let Some(token_path) = descriptor.token_path.as_ref() {
                    assert_eq!(
                        token_path, &expected_path,
                        "descriptor token path should reference existing connector token"
                    );
                }
            }
            other => panic!("unexpected outcome: {:?}", other),
        }

        find.assert();
        configure.assert();
        zones.assert();
        dns_records.assert();

        std::env::remove_var("BRANCHBOX_CLOUDFLARE_API_BASE");
    }
}
