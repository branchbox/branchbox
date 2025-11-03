//! Tunnel provider abstractions.
//!
//! These traits and supporting types decouple the rest of the workflow code
//! from a specific tunneling vendor, enabling support for Cloudflare today and
//! alternative providers (ngrok, localhost.run, etc.) in the future.

pub mod cloudflared;

use crate::Result;
use std::path::{Path, PathBuf};

/// Context supplied when requesting a new tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisioningIntent<'a> {
    /// Repository workspace root (contains `.branchbox/`).
    pub workspace_root: &'a Path,
    /// Feature identifier (e.g., `feature/auth-login`).
    pub feature_name: &'a str,
    /// Full hostname that should route to the worktree.
    pub hostname: &'a str,
    /// Internal service URL exposed from the devcontainer (e.g., `web:3000`).
    pub service_url: &'a str,
}

/// Machine-readable description of an allocated tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelDescriptor {
    pub provider: String,
    pub tunnel_name: Option<String>,
    pub tunnel_id: Option<String>,
    pub hostname: String,
    pub token_path: Option<PathBuf>,
}

/// Guidance for manual tunnel setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualInstructions {
    pub reason: String,
    pub steps: Vec<String>,
}

/// Result of a provisioning attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisioningOutcome {
    /// Tunnel was provisioned automatically.
    Automated {
        descriptor: TunnelDescriptor,
        token: Option<String>,
    },
    /// Automation unavailable; user must follow instructions.
    Manual(ManualInstructions),
    /// Provider is disabled for this workspace.
    Disabled(String),
}

/// High-level tunnel status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelStatus {
    /// Provider could not determine status.
    Unknown,
    /// Tunnel exists but not yet confirmed responsive.
    Pending,
    /// Tunnel is active and reachable.
    Active,
    /// Tunnel managed manually outside automation.
    Manual,
}

/// Trait implemented by tunnel providers.
pub trait TunnelProvider {
    /// Provider identifier (e.g., `cloudflared`).
    fn name(&self) -> &str;

    /// Attempt to provision a tunnel for the given intent.
    fn provision(&self, intent: &ProvisioningIntent<'_>) -> Result<ProvisioningOutcome>;

    /// Retrieve status details for an existing tunnel.
    fn status(&self, descriptor: &TunnelDescriptor) -> Result<TunnelStatus>;

    /// Tear down the tunnel described by descriptor.
    fn teardown(&self, descriptor: &TunnelDescriptor) -> Result<()>;
}
