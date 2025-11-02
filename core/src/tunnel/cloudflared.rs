//! Cloudflared tunnel provider stub.
//!
//! The initial implementation focuses on capturing configuration and surfacing
//! clear manual instructions when automation is unavailable. Full API-backed
//! provisioning will be layered on in subsequent milestones.

use super::{
    ManualInstructions, ProvisioningIntent, ProvisioningOutcome, TunnelDescriptor, TunnelProvider,
    TunnelStatus,
};
use crate::config::CloudflaredConfig;
use crate::Result;
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
        self.config
            .api_token_path
            .clone()
            .map(|path| self.workspace_root.join(path))
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
            "Refer to `legacy/cloudflared/README.md` for script-driven helpers until automation lands."
                .to_string(),
        );

        ManualInstructions { reason, steps }
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

        // Automation hooks are not yet wired; return a manual path noting the limitation.
        let manual = self.manual_steps(
            intent,
            "Cloudflare automation stub reached; API provisioning will be added in the next milestone.",
        );
        Ok(ProvisioningOutcome::Manual(manual))
    }

    fn status(&self, _descriptor: &TunnelDescriptor) -> Result<TunnelStatus> {
        // Without API integration we cannot determine live status.
        Ok(TunnelStatus::Manual)
    }

    fn teardown(&self, _descriptor: &TunnelDescriptor) -> Result<()> {
        // Stub implementation: real teardown will arrive with API integration.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CloudflaredConfig;
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
    fn manual_outcome_even_when_credentials_present_for_now() {
        let mut config = CloudflaredConfig::default();
        config.manual_instructions = false;
        config.account_id = Some("acct".into());
        config.api_token_path = Some(PathBuf::from(".branchbox/secure/cloudflared.env"));

        let temp = TempDir::new().unwrap();
        let credentials_path = temp.path().join(".branchbox/secure/cloudflared.env");
        fs::create_dir_all(credentials_path.parent().unwrap()).unwrap();
        fs::write(&credentials_path, "CLOUDFLARE_TUNNEL_TOKEN=faketoken\n").unwrap();

        let provider = CloudflaredProvider::new(&config, temp.path());
        let intent = ProvisioningIntent {
            workspace_root: temp.path(),
            feature_name: "feature/login",
            hostname: "login.dev.example.com",
            service_url: "web:3000",
        };

        let outcome = provider.provision(&intent).unwrap();
        match outcome {
            ProvisioningOutcome::Manual(instructions) => {
                assert!(instructions.reason.contains("stub"), "expected stub notice");
            }
            other => panic!("unexpected outcome: {:?}", other),
        }
    }
}
