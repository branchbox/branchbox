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

    #[serde(default)]
    pub editor: EditorSettings,

    #[serde(default)]
    pub feature: FeatureSettings,

    #[serde(default)]
    pub github_auth: GitHubAuthSettings,
}

impl Default for BranchBoxConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION.to_string(),
            tunnel: TunnelSettings::default(),
            editor: EditorSettings::default(),
            feature: FeatureSettings::default(),
            github_auth: GitHubAuthSettings::default(),
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

/// Editor preferences applied across devcontainers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EditorSettings {
    /// Preferred agent slug (`codex`, `claude`, etc.)
    #[serde(default)]
    pub default_agent: Option<String>,

    /// Whether to spawn a terminal running the preferred agent on attach.
    #[serde(default)]
    pub auto_launch_agent_terminal: bool,

    /// View identifier (`workbench.view.scm`, `workbench.view.extension.codex`, etc.) to focus.
    #[serde(default)]
    pub preferred_sidebar_view: Option<String>,

    /// Hide the auxiliary/right sidebar if it was previously visible.
    #[serde(default)]
    pub hide_secondary_sidebar: bool,
}

/// GitHub authentication strategy for devcontainers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GitHubAuthStrategy {
    /// Mount ~/.ssh directory and forward SSH agent (default, recommended)
    #[default]
    Ssh,
    /// Use gh CLI token authentication only (legacy)
    GhCli,
    /// Disable GitHub authentication mounts entirely
    None,
}

impl std::fmt::Display for GitHubAuthStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitHubAuthStrategy::Ssh => write!(f, "ssh"),
            GitHubAuthStrategy::GhCli => write!(f, "gh-cli"),
            GitHubAuthStrategy::None => write!(f, "none"),
        }
    }
}

impl std::str::FromStr for GitHubAuthStrategy {
    type Err = crate::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ssh" => Ok(GitHubAuthStrategy::Ssh),
            "gh" | "gh-cli" | "ghcli" => Ok(GitHubAuthStrategy::GhCli),
            "none" | "skip" | "disabled" => Ok(GitHubAuthStrategy::None),
            _ => Err(crate::Error::Config(format!(
                "Unknown auth strategy: {}\nValid options: ssh, gh-cli, none",
                s
            ))),
        }
    }
}

/// SSH agent provider configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SshAgentProvider {
    /// Use the system default SSH agent (SSH_AUTH_SOCK from host)
    #[default]
    System,
    /// Use 1Password SSH agent
    OnePassword,
    /// Custom SSH agent socket path (set via custom_socket_path)
    Custom,
}

impl std::fmt::Display for SshAgentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshAgentProvider::System => write!(f, "system"),
            SshAgentProvider::OnePassword => write!(f, "1password"),
            SshAgentProvider::Custom => write!(f, "custom"),
        }
    }
}

/// GitHub authentication settings for devcontainers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubAuthSettings {
    /// Authentication strategy (ssh, gh-cli, or none)
    #[serde(default)]
    pub strategy: GitHubAuthStrategy,

    /// SSH agent provider (system, 1password, or custom)
    #[serde(default)]
    pub ssh_agent_provider: SshAgentProvider,

    /// Custom SSH agent socket path (only used when ssh_agent_provider is Custom)
    #[serde(default)]
    pub custom_socket_path: Option<String>,

    /// Mount ~/.ssh directory into container (read-only)
    #[serde(default = "default_mount_ssh_dir")]
    pub mount_ssh_dir: bool,

    /// Keep gh CLI mount even when using SSH (useful for PRs/issues)
    #[serde(default = "default_keep_gh_mount")]
    pub keep_gh_mount: bool,
}

impl Default for GitHubAuthSettings {
    fn default() -> Self {
        Self {
            strategy: GitHubAuthStrategy::Ssh,
            ssh_agent_provider: SshAgentProvider::System,
            custom_socket_path: None,
            mount_ssh_dir: true,
            keep_gh_mount: true,
        }
    }
}

fn default_mount_ssh_dir() -> bool {
    true
}

fn default_keep_gh_mount() -> bool {
    true
}

impl GitHubAuthSettings {
    /// Returns the SSH agent socket path based on the provider.
    ///
    /// For devcontainer.json remoteEnv, this returns the template string
    /// that references the host environment variable or fixed path.
    pub fn ssh_auth_sock_value(&self) -> Option<String> {
        if self.strategy != GitHubAuthStrategy::Ssh {
            return None;
        }

        match self.ssh_agent_provider {
            SshAgentProvider::System => Some("${localEnv:SSH_AUTH_SOCK}".to_string()),
            SshAgentProvider::OnePassword => {
                // 1Password uses different paths on different platforms
                // We use the localEnv syntax to let the devcontainer resolve it
                // The actual path is set in the container's SSH config
                Some("${localEnv:SSH_AUTH_SOCK}".to_string())
            }
            SshAgentProvider::Custom => self.custom_socket_path.clone(),
        }
    }

    /// Returns 1Password SSH agent socket path for the current platform.
    pub fn onepassword_socket_path() -> String {
        if cfg!(target_os = "macos") {
            "~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock".to_string()
        } else {
            // Linux and others
            "~/.1password/agent.sock".to_string()
        }
    }

    /// Check if SSH-based auth is enabled.
    pub fn uses_ssh(&self) -> bool {
        self.strategy == GitHubAuthStrategy::Ssh
    }

    /// Check if gh CLI mount should be included.
    pub fn include_gh_mount(&self) -> bool {
        self.strategy == GitHubAuthStrategy::GhCli || self.keep_gh_mount
    }
}

/// Feature workflow defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureSettings {
    /// Default branch prefix when creating worktrees (defaults to `feature`).
    #[serde(default = "default_feature_branch_prefix")]
    pub branch_prefix: String,

    #[serde(default)]
    pub teardown: FeatureTeardownSettings,
}

impl Default for FeatureSettings {
    fn default() -> Self {
        Self {
            branch_prefix: default_feature_branch_prefix(),
            teardown: FeatureTeardownSettings::default(),
        }
    }
}

fn default_feature_branch_prefix() -> String {
    "feature".to_string()
}

/// Teardown defaults for features.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureTeardownSettings {
    /// Delete the feature branch by default during teardown.
    #[serde(default = "default_teardown_delete_branch")]
    pub delete_branch_by_default: bool,

    /// Force-delete unmerged branches by default (`git branch -D`).
    #[serde(default)]
    pub force_delete_unmerged_by_default: bool,

    /// Prompt before force-deleting an unmerged branch (interactive shells only).
    #[serde(default = "default_teardown_prompt_force_delete")]
    pub prompt_force_delete_unmerged: bool,
}

impl Default for FeatureTeardownSettings {
    fn default() -> Self {
        Self {
            delete_branch_by_default: default_teardown_delete_branch(),
            force_delete_unmerged_by_default: false,
            prompt_force_delete_unmerged: default_teardown_prompt_force_delete(),
        }
    }
}

fn default_teardown_delete_branch() -> bool {
    true
}

fn default_teardown_prompt_force_delete() -> bool {
    true
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

    /// Service URL for tunnel ingress (e.g., `http://app:5001`).
    #[serde(default)]
    pub service_url: Option<String>,

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
            service_url: None,
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

    #[test]
    fn editor_settings_defaults_to_noop() {
        let config = BranchBoxConfig::default();
        assert_eq!(EditorSettings::default(), config.editor);
    }

    #[test]
    fn feature_settings_defaults_are_stable() {
        let config = BranchBoxConfig::default();
        assert_eq!(config.feature.branch_prefix, "feature");
        assert!(config.feature.teardown.delete_branch_by_default);
        assert!(!config.feature.teardown.force_delete_unmerged_by_default);
        assert!(config.feature.teardown.prompt_force_delete_unmerged);
    }

    #[test]
    fn github_auth_defaults_to_ssh() {
        let config = BranchBoxConfig::default();
        assert_eq!(config.github_auth.strategy, GitHubAuthStrategy::Ssh);
        assert_eq!(
            config.github_auth.ssh_agent_provider,
            SshAgentProvider::System
        );
        assert!(config.github_auth.mount_ssh_dir);
        assert!(config.github_auth.keep_gh_mount);
    }

    #[test]
    fn github_auth_strategy_parsing() {
        assert_eq!(
            "ssh".parse::<GitHubAuthStrategy>().unwrap(),
            GitHubAuthStrategy::Ssh
        );
        assert_eq!(
            "gh-cli".parse::<GitHubAuthStrategy>().unwrap(),
            GitHubAuthStrategy::GhCli
        );
        assert_eq!(
            "none".parse::<GitHubAuthStrategy>().unwrap(),
            GitHubAuthStrategy::None
        );
        assert!("invalid".parse::<GitHubAuthStrategy>().is_err());
    }

    #[test]
    fn github_auth_ssh_socket_value() {
        let settings = GitHubAuthSettings::default();
        assert_eq!(
            settings.ssh_auth_sock_value(),
            Some("${localEnv:SSH_AUTH_SOCK}".to_string())
        );

        let mut gh_settings = GitHubAuthSettings::default();
        gh_settings.strategy = GitHubAuthStrategy::GhCli;
        assert_eq!(gh_settings.ssh_auth_sock_value(), None);
    }

    #[test]
    fn github_auth_round_trip() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path();

        let mut config = BranchBoxConfig::default();
        config.github_auth.strategy = GitHubAuthStrategy::GhCli;
        config.github_auth.ssh_agent_provider = SshAgentProvider::OnePassword;
        config.github_auth.mount_ssh_dir = false;

        config.save(workspace).unwrap();

        let loaded = BranchBoxConfig::load(workspace).unwrap();
        assert_eq!(config.github_auth, loaded.github_auth);
    }

    #[test]
    fn github_auth_uses_ssh_helper() {
        let mut settings = GitHubAuthSettings::default();
        assert!(settings.uses_ssh());

        settings.strategy = GitHubAuthStrategy::GhCli;
        assert!(!settings.uses_ssh());

        settings.strategy = GitHubAuthStrategy::None;
        assert!(!settings.uses_ssh());
    }

    #[test]
    fn github_auth_include_gh_mount_helper() {
        let mut settings = GitHubAuthSettings::default();
        // SSH with keep_gh_mount=true (default) should include gh mount
        assert!(settings.include_gh_mount());

        // SSH with keep_gh_mount=false should not include gh mount
        settings.keep_gh_mount = false;
        assert!(!settings.include_gh_mount());

        // GhCli always includes gh mount
        settings.strategy = GitHubAuthStrategy::GhCli;
        assert!(settings.include_gh_mount());
    }
}
