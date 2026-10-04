//! BranchBox configuration schema and helpers.
//!
//! This module centralizes serialization logic for project-level configuration
//! stored under `.branchbox/config.json`. It currently focuses on tunnel
//! defaults, leaving room for future workspace metadata. Key-level, format-preserving
//! edits (`branchbox config`) live in [`crate::config_edit`].

use crate::atomic_fs::{self, LOCK_TIMEOUT};
use crate::{runtime::RuntimeProviderKind, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The configuration format version BranchBox writes.
pub(crate) const CONFIG_VERSION: &str = "1";

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
    pub runtime: RuntimeSettings,
}

impl Default for BranchBoxConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION.to_string(),
            tunnel: TunnelSettings::default(),
            editor: EditorSettings::default(),
            feature: FeatureSettings::default(),
            runtime: RuntimeSettings::default(),
        }
    }
}

/// Workspace execution-boundary defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeSettings {
    /// Runtime used when `feature start --runtime` is not supplied.
    #[serde(default)]
    pub provider: RuntimeProviderKind,

    /// Docker Sandboxes-specific workspace policy.
    #[serde(default)]
    pub sbx: SbxRuntimeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SbxRuntimeSettings {
    /// Compose services explicitly started by devcontainers in SBX. Compose still starts their
    /// declared dependencies, while unrelated host-integrated sidecars remain stopped.
    #[serde(default)]
    pub run_services: Vec<String>,
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

    /// Persist configuration to disk atomically under the `.branchbox` lock. A new file gets the
    /// whole configuration (mode 0644). An existing file is edited in place, key by key: only
    /// the settings that differ from what it holds change, and keys this version does not know,
    /// the file's formatting and its permissions are kept.
    pub fn save(&self, workspace: &Path) -> Result<()> {
        let config_dir = workspace.join(".branchbox");
        let _lock = atomic_fs::lock_state_dir(&config_dir, LOCK_TIMEOUT)?;

        let path = Self::path(workspace);
        if path.exists() {
            // Edit only what changed, so keys this version does not know survive (DESIGN
            // §5.10), as they do `config set`.
            return crate::config_edit::plan_save(workspace, self)?.commit();
        }
        let content = serde_json::to_string_pretty(self)?;
        atomic_fs::write_atomic(&path, content.as_bytes(), 0o644)
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

    #[cfg(unix)]
    #[test]
    fn save_replaces_the_file_atomically_keeping_its_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let workspace = temp.path();
        BranchBoxConfig::default().save(workspace).unwrap();
        let path = BranchBoxConfig::path(workspace);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o644);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = BranchBoxConfig::default();
        config.feature.branch_prefix = "spike".to_string();
        config.save(workspace).unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(BranchBoxConfig::load(workspace).unwrap(), config);
        let leftovers = fs::read_dir(workspace.join(".branchbox"))
            .unwrap()
            .filter(|entry| {
                let name = entry.as_ref().unwrap().file_name();
                name.to_string_lossy().ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn save_over_an_existing_file_keeps_unknown_keys_and_formatting() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path();
        fs::create_dir_all(workspace.join(".branchbox")).unwrap();
        let text = "{\n  \"x_team\": {\"keep\": true},\n  \"tunnel\": {\"enabled\": true, \
                    \"providers\": {\"cloudflared\": {\"account_id\": \"a\", \"x_extra\": 1}}}\n}\n";
        fs::write(BranchBoxConfig::path(workspace), text).unwrap();

        let mut config = BranchBoxConfig::load(workspace).unwrap();
        config.feature.branch_prefix = "spike".to_string();
        config.save(workspace).unwrap();
        let saved = fs::read_to_string(BranchBoxConfig::path(workspace)).unwrap();
        assert!(
            saved.starts_with("{\n  \"x_team\": {\"keep\": true},"),
            "{saved}"
        );
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(value["feature"]["branch_prefix"], "spike");
        assert_eq!(value["tunnel"]["providers"]["cloudflared"]["x_extra"], 1);
        assert_eq!(BranchBoxConfig::load(workspace).unwrap(), config);

        // Dropping a provider removes its whole section, as saving `None` always did.
        config.tunnel.enabled = false;
        config.tunnel.providers.cloudflared = None;
        config.save(workspace).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(BranchBoxConfig::path(workspace)).unwrap())
                .unwrap();
        assert!(
            value["tunnel"]["providers"].get("cloudflared").is_none(),
            "{value}"
        );
        assert_eq!(value["x_team"]["keep"], true);
        assert_eq!(BranchBoxConfig::load(workspace).unwrap(), config);
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
    fn legacy_config_defaults_to_container_runtime() {
        let config: BranchBoxConfig = serde_json::from_str(r#"{"version":"1"}"#).unwrap();
        assert_eq!(config.runtime.provider, RuntimeProviderKind::Container);
    }

    #[test]
    fn runtime_provider_can_be_configured() {
        let config: BranchBoxConfig =
            serde_json::from_str(r#"{"version":"1","runtime":{"provider":"sbx"}}"#).unwrap();
        assert_eq!(config.runtime.provider, RuntimeProviderKind::Sbx);
    }
}
