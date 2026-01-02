//! Devcontainer Module
//!
//! Manages devcontainer configuration synchronization between main repo and feature worktrees.
//!
//! ## Configuration Strategy (Phase 1)
//!
//! This module uses environment variables for configuration rather than a config file:
//!
//! - `BRANCHBOX_DEVCONTAINER_STRATEGY`: Set to "copy" (default) or "symlink"
//!
//! **Design Rationale:**
//!
//! Environment variables were chosen for Phase 1 because:
//! 1. **Simplicity**: No additional config file format or parsing logic needed
//! 2. **Per-session control**: Easy to override for one-off operations (e.g., `BRANCHBOX_DEVCONTAINER_STRATEGY=symlink branchbox feature start`)
//! 3. **Consistency**: Aligns with existing BranchBox patterns (e.g., `BRANCHBOX_SKIP_HOST_VALIDATION`)
//! 4. **Minimal scope**: Phase 1 only needs one configuration option (strategy)
//!
//! **Future Work (Phase 2+):**
//!
//! When more configuration options are needed (e.g., per-file sync rules, custom exclude patterns),
//! migrate to a structured config file (`.branchbox/config.toml` or similar). The environment
//! variable can remain as an override mechanism for backward compatibility.

use super::Module;
use crate::{Error, Result};
use jsonc_parser::{parse_to_serde_value, ParseOptions};
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncStrategy {
    /// Copy files (default - allows per-feature customization)
    #[default]
    Copy,
    /// Symlink files (updates propagate automatically but no customization)
    Symlink,
}

impl SyncStrategy {
    /// Returns a lowercase string representation for telemetry/registry use.
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncStrategy::Copy => "copy",
            SyncStrategy::Symlink => "symlink",
        }
    }
}

pub struct DevcontainerModule {
    source_dir: PathBuf,
    strategy: SyncStrategy,
    /// Files to exclude (e.g., .env is already symlinked separately)
    exclude: Vec<String>,
}

impl DevcontainerModule {
    pub fn new() -> Self {
        Self {
            source_dir: PathBuf::new(),
            strategy: SyncStrategy::Copy,
            exclude: vec![
                ".env".to_string(),
                ".branchbox.env".to_string(),
                ".gitignore".to_string(),
            ],
        }
    }

    /// Sync devcontainer files to target directory
    pub fn sync_to(&self, target_dir: &Path) -> Result<SyncOutcome> {
        let dest = target_dir.join(".devcontainer");
        if !dest.exists() {
            std::fs::create_dir_all(&dest)?;
        }

        let mut synced_files = Vec::new();
        let mut expected_paths: HashSet<PathBuf> = HashSet::new();

        // Walk source directory, sync all files except excluded ones
        for entry in WalkDir::new(&self.source_dir)
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| !self.is_excluded(e.path()))
        {
            let entry =
                entry.map_err(|e| Error::validation(format!("Failed to walk directory: {}", e)))?;
            let rel_path = entry
                .path()
                .strip_prefix(&self.source_dir)
                .map_err(|e| Error::validation(format!("Path strip failed: {}", e)))?;
            let dest_path = dest.join(rel_path);

            // Use metadata to properly handle symlinks and special files
            let metadata = entry.metadata().map_err(|e| {
                Error::validation(format!(
                    "Failed to read metadata for {}: {}",
                    entry.path().display(),
                    e
                ))
            })?;

            if metadata.is_dir() {
                // Handle type mismatch: if destination is a file, remove it before creating directory
                if dest_path.is_file() {
                    std::fs::remove_file(&dest_path)?;
                }
                std::fs::create_dir_all(&dest_path)?;
                expected_paths.insert(rel_path.to_path_buf());
            } else {
                // Handle type mismatch: if destination is a directory, remove it before creating file
                if dest_path.is_dir() {
                    std::fs::remove_dir_all(&dest_path)?;
                }
                match self.strategy {
                    SyncStrategy::Copy => {
                        std::fs::copy(entry.path(), &dest_path)?;
                    }
                    SyncStrategy::Symlink => {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::symlink;
                            if dest_path.exists() {
                                std::fs::remove_file(&dest_path)?;
                            }
                            let rel_source =
                                pathdiff::diff_paths(entry.path(), dest_path.parent().unwrap())
                                    .ok_or_else(|| {
                                        Error::validation("Path diff failed".to_string())
                                    })?;
                            symlink(rel_source, &dest_path)?;
                        }
                        #[cfg(not(unix))]
                        {
                            std::fs::copy(entry.path(), &dest_path)?;
                        }
                    }
                }
                synced_files.push(rel_path.display().to_string());
                tracing::debug!("Synced .devcontainer/{}", rel_path.display());
                expected_paths.insert(rel_path.to_path_buf());
            }
        }

        // Remove files/directories that no longer exist in the source
        for entry in WalkDir::new(&dest)
            .min_depth(1)
            .contents_first(true)
            .into_iter()
        {
            let entry =
                entry.map_err(|e| Error::validation(format!("Failed to walk directory: {}", e)))?;
            if self.is_excluded(entry.path()) {
                continue;
            }
            let rel_path = entry
                .path()
                .strip_prefix(&dest)
                .map_err(|e| Error::validation(format!("Path strip failed: {}", e)))?;

            if !expected_paths.contains(rel_path) {
                if entry.file_type().is_dir() {
                    std::fs::remove_dir_all(entry.path())?;
                } else {
                    std::fs::remove_file(entry.path())?;
                }
                tracing::debug!("Removed stale .devcontainer/{}", rel_path.display());
            }
        }

        Ok(SyncOutcome {
            synced_files,
            strategy: self.strategy,
        })
    }

    fn is_excluded(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| self.exclude.iter().any(|e| n == e))
            .unwrap_or(false)
    }

    /// Returns the configured sync strategy (copy or symlink).
    pub fn strategy(&self) -> SyncStrategy {
        self.strategy
    }
}

impl Default for DevcontainerModule {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_json_with_comments(contents: &str, path: &Path) -> Result<JsonValue> {
    let options = ParseOptions {
        allow_trailing_commas: true,
        ..Default::default()
    };

    match parse_to_serde_value(contents, &options) {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(Error::validation(format!(
            "Failed to parse {}: document is empty",
            path.display()
        ))),
        Err(err) => Err(Error::validation(format!(
            "Failed to parse {}: {}",
            path.display(),
            err
        ))),
    }
}

impl Module for DevcontainerModule {
    fn name(&self) -> &str {
        "devcontainer"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join(".devcontainer").exists()
    }

    fn init(&mut self, main_dir: &Path, _feature_dir: &Path) -> Result<()> {
        self.source_dir = main_dir.join(".devcontainer");

        if !self.source_dir.exists() {
            return Err(Error::validation(format!(
                "Devcontainer directory not found: {}",
                self.source_dir.display()
            )));
        }

        // Check for strategy override via env var
        if let Ok(strategy) = std::env::var("BRANCHBOX_DEVCONTAINER_STRATEGY") {
            self.strategy = match strategy.to_lowercase().as_str() {
                "symlink" => SyncStrategy::Symlink,
                _ => SyncStrategy::Copy,
            };
        }

        tracing::info!(
            "Devcontainer module initialized (strategy: {:?})",
            self.strategy
        );
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Syncing devcontainer configuration...");
        let outcome = self.sync_to(feature_dir)?;
        if matches!(self.strategy, SyncStrategy::Symlink) {
            tracing::info!("Skipping workspace configuration (symlink strategy in use)");
        } else {
            self.configure_workspace_settings_impl(feature_dir)?;
        }
        tracing::info!(
            "Synced {} devcontainer files ({:?})",
            outcome.synced_files.len(),
            outcome.strategy
        );
        Ok(())
    }

    fn teardown(&self, _main_dir: &Path, _feature_dir: &Path) -> Result<()> {
        // No cleanup needed - devcontainer removed with worktree
        Ok(())
    }

    fn validate(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        let devcontainer = feature_dir.join(".devcontainer");
        if !devcontainer.exists() {
            return Err(Error::validation(
                "Feature worktree missing .devcontainer directory".to_string(),
            ));
        }
        Ok(())
    }
}

impl DevcontainerModule {
    fn configure_workspace_settings_impl(&self, feature_dir: &Path) -> Result<()> {
        let devcontainer_dir = feature_dir.join(".devcontainer");
        configure_workspace_settings(&devcontainer_dir)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct SyncOutcome {
    pub synced_files: Vec<String>,
    pub strategy: SyncStrategy,
}

/// Result of configuring workspace settings
#[derive(Debug, Default)]
pub struct ConfigureOutcome {
    /// Whether devcontainer.json was modified
    pub devcontainer_modified: bool,
    /// Whether compose.yaml was modified
    pub compose_modified: bool,
    /// List of changes made
    pub changes: Vec<String>,
}

/// Find the compose file path by reading dockerComposeFile from devcontainer.json
/// or falling back to common compose file names.
fn find_compose_file(devcontainer_dir: &Path, config: &JsonValue) -> Option<PathBuf> {
    // First, try to read dockerComposeFile from devcontainer.json
    // It can be a string or an array of strings
    if let Some(compose_file) = config.get("dockerComposeFile") {
        let compose_files: Vec<&str> = if let Some(file) = compose_file.as_str() {
            vec![file]
        } else if let Some(files) = compose_file.as_array() {
            files.iter().filter_map(|f| f.as_str()).collect()
        } else {
            vec![]
        };

        // Use the first compose file that exists
        for file in compose_files {
            let path = devcontainer_dir.join(file);
            if path.exists() {
                return Some(path);
            }
        }
    }

    // Fallback: check common compose file names
    let common_names = [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ];

    for name in common_names {
        let path = devcontainer_dir.join(name);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Result of adding cloudflared service
#[derive(Debug, Default)]
pub struct CloudflaredOutcome {
    /// Whether compose file was modified
    pub compose_modified: bool,
    /// Whether cloudflared.env was created
    pub env_created: bool,
    /// List of changes made
    pub changes: Vec<String>,
}

/// Add cloudflared tunnel service to the compose file.
///
/// This adds a cloudflared service that:
/// - Uses the official cloudflare/cloudflared image
/// - Reads configuration from .cloudflared.env
/// - Runs the tunnel command
///
/// Also creates a template .cloudflared.env file if it doesn't exist.
///
/// # Arguments
///
/// * `devcontainer_dir` - Path to the .devcontainer directory
/// * `main_service_name` - Optional name of the main service to add depends_on (auto-detected if None)
///
/// # Returns
///
/// Returns a `CloudflaredOutcome` describing what changes were made.
pub fn add_cloudflared_service(
    devcontainer_dir: &Path,
    main_service_name: Option<&str>,
) -> Result<CloudflaredOutcome> {
    let mut outcome = CloudflaredOutcome::default();

    let config_path = devcontainer_dir.join("devcontainer.json");
    if !config_path.exists() {
        return Ok(outcome);
    }

    let config_contents = std::fs::read_to_string(&config_path)?;
    let config = parse_json_with_comments(&config_contents, &config_path)?;

    let compose_path = match find_compose_file(devcontainer_dir, &config) {
        Some(path) => path,
        None => return Ok(outcome), // No compose file, nothing to do
    };

    let compose_filename = compose_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("compose.yaml");

    let compose_contents = std::fs::read_to_string(&compose_path)?;
    let mut compose: YamlValue = serde_yaml::from_str(&compose_contents).map_err(|err| {
        Error::validation(format!(
            "Failed to parse {}: {}",
            compose_path.display(),
            err
        ))
    })?;

    let mut compose_updated = false;

    if let Some(root) = compose.as_mapping_mut() {
        let services_key = YamlValue::String("services".to_string());

        // Check if cloudflared service already exists
        if let Some(services) = root.get(&services_key).and_then(|v| v.as_mapping()) {
            let cloudflared_key = YamlValue::String("cloudflared".to_string());
            if services.contains_key(&cloudflared_key) {
                tracing::info!("cloudflared service already exists in {}", compose_filename);
                return Ok(outcome);
            }
        }

        // Find the main service name (first service with a build context, or explicitly provided)
        let detected_main_service = if let Some(name) = main_service_name {
            Some(name.to_string())
        } else if let Some(services) = root.get(&services_key).and_then(|v| v.as_mapping()) {
            // Find first service with a build section (likely the main app)
            services.iter().find_map(|(key, value)| {
                if let (Some(name), Some(service_map)) = (key.as_str(), value.as_mapping()) {
                    let build_key = YamlValue::String("build".to_string());
                    if service_map.contains_key(&build_key) {
                        return Some(name.to_string());
                    }
                }
                None
            })
        } else {
            None
        };

        // Create cloudflared service
        let cloudflared_service = serde_yaml::from_str::<YamlValue>(
            r#"
image: cloudflare/cloudflared:latest
restart: unless-stopped
env_file:
  - .cloudflared.env
command: ["tunnel", "--no-autoupdate", "run"]
"#,
        )
        .expect("hardcoded YAML is valid");

        // Add cloudflared service to services
        if let Some(services) = root.get_mut(&services_key).and_then(|v| v.as_mapping_mut()) {
            services.insert(
                YamlValue::String("cloudflared".to_string()),
                cloudflared_service,
            );
            compose_updated = true;
            outcome
                .changes
                .push(format!("{}: added cloudflared service", compose_filename));

            // Add depends_on to main service if we found one
            if let Some(main_name) = detected_main_service {
                let main_key = YamlValue::String(main_name.clone());
                if let Some(main_service) = services.get_mut(&main_key) {
                    if let Some(service_map) = main_service.as_mapping_mut() {
                        let depends_key = YamlValue::String("depends_on".to_string());

                        // Get or create depends_on array
                        let depends_on = service_map
                            .entry(depends_key.clone())
                            .or_insert_with(|| YamlValue::Sequence(vec![]));

                        if let Some(deps) = depends_on.as_sequence_mut() {
                            let cloudflared_dep = YamlValue::String("cloudflared".to_string());
                            if !deps.contains(&cloudflared_dep) {
                                deps.push(cloudflared_dep);
                                outcome.changes.push(format!(
                                    "{}: added cloudflared to {} depends_on",
                                    compose_filename, main_name
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    if compose_updated {
        let mut serialized = serde_yaml::to_string(&compose).map_err(|err| {
            Error::validation(format!(
                "Failed to serialize {}: {}",
                compose_path.display(),
                err
            ))
        })?;
        if serialized.starts_with("---\n") {
            serialized = serialized.split_off(4);
        }
        if !serialized.ends_with('\n') {
            serialized.push('\n');
        }
        std::fs::write(&compose_path, serialized)?;
        outcome.compose_modified = true;
        tracing::info!("Added cloudflared service to {}", compose_filename);
    }

    // Create .cloudflared.env template if it doesn't exist
    let env_path = devcontainer_dir.join(".cloudflared.env");
    if !env_path.exists() {
        let env_template = r#"# Cloudflare Tunnel Configuration
# See: https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/

# Tunnel token from Cloudflare Zero Trust dashboard
# Get this from: Access → Tunnels → Create a tunnel → Copy token
TUNNEL_TOKEN=

# Optional: Hostname for this development environment
# DEV_HOSTNAME=feature-name.your-domain.com
"#;
        std::fs::write(&env_path, env_template)?;
        outcome.env_created = true;
        outcome
            .changes
            .push("created .cloudflared.env template".to_string());
        tracing::info!("Created .cloudflared.env template");
    }

    Ok(outcome)
}

/// Configure devcontainer workspace settings for worktree compatibility.
///
/// This ensures the devcontainer.json and compose.yaml are configured correctly
/// for git worktrees by:
/// - Setting `workspaceFolder` to `/workspaces/${localWorkspaceFolderBasename}`
/// - Setting `workspaceMount` to use the dynamic folder basename
/// - Updating compose.yaml volume mounts to use `../..:/workspaces:cached`
///
/// This function is called both during `branchbox init` (to configure newly generated
/// or existing devcontainers) and during `branchbox feature start` (via DevcontainerModule).
///
/// # Arguments
///
/// * `devcontainer_dir` - Path to the .devcontainer directory
///
/// # Returns
///
/// Returns a `ConfigureOutcome` describing what changes were made.
pub fn configure_workspace_settings(devcontainer_dir: &Path) -> Result<ConfigureOutcome> {
    let mut outcome = ConfigureOutcome::default();

    let config_path = devcontainer_dir.join("devcontainer.json");
    if !config_path.exists() {
        return Ok(outcome);
    }

    let config_contents = std::fs::read_to_string(&config_path)?;
    let mut config = parse_json_with_comments(&config_contents, &config_path)?;

    // Determine compose file path from devcontainer.json or fallback to common names
    let compose_path = find_compose_file(devcontainer_dir, &config);

    let workspace_folder = "/workspaces/${localWorkspaceFolderBasename}";
    let workspace_mount =
        "source=${localWorkspaceFolder},target=/workspaces/${localWorkspaceFolderBasename},type=bind,consistency=cached";

    let mut devcontainer_needs_update = false;

    match config.as_object_mut() {
        Some(map) => {
            let folder_value = JsonValue::String(workspace_folder.to_string());
            if map
                .get("workspaceFolder")
                .map(|value| value != &folder_value)
                .unwrap_or(true)
            {
                let old_value = map
                    .get("workspaceFolder")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<none>");
                outcome.changes.push(format!(
                    "workspaceFolder: {} → {}",
                    old_value, workspace_folder
                ));
                map.insert("workspaceFolder".to_string(), folder_value);
                devcontainer_needs_update = true;
            }

            let mount_value = JsonValue::String(workspace_mount.to_string());
            if map
                .get("workspaceMount")
                .map(|value| value != &mount_value)
                .unwrap_or(true)
            {
                outcome
                    .changes
                    .push("workspaceMount: updated for worktree compatibility".to_string());
                map.insert("workspaceMount".to_string(), mount_value);
                devcontainer_needs_update = true;
            }
        }
        None => {
            return Err(Error::validation(format!(
                "{} is not a JSON object",
                config_path.display()
            )))
        }
    }

    if devcontainer_needs_update {
        let mut formatted = serde_json::to_string_pretty(&config)?;
        formatted.push('\n');
        std::fs::write(&config_path, formatted)?;
        outcome.devcontainer_modified = true;
        tracing::info!("Updated devcontainer.json for worktree compatibility");
    }

    if let Some(compose_path) = compose_path {
        let compose_contents = std::fs::read_to_string(&compose_path)?;
        let compose_filename = compose_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("compose.yaml");
        let mut compose: YamlValue = serde_yaml::from_str(&compose_contents).map_err(|err| {
            Error::validation(format!(
                "Failed to parse {}: {}",
                compose_path.display(),
                err
            ))
        })?;

        let desired = "../..:/workspaces:cached";
        let alternate = "..:/workspaces:cached";

        let mut compose_updated = false;
        if let Some(root) = compose.as_mapping_mut() {
            let services_key = YamlValue::String("services".to_string());
            if let Some(services) = root
                .get_mut(&services_key)
                .and_then(|value| value.as_mapping_mut())
            {
                for service in services.values_mut() {
                    if let Some(service_map) = service.as_mapping_mut() {
                        let volumes_key = YamlValue::String("volumes".to_string());
                        if let Some(volumes) = service_map
                            .get_mut(&volumes_key)
                            .and_then(|value| value.as_sequence_mut())
                        {
                            let mut found_desired = false;
                            for entry in volumes.iter_mut() {
                                let raw = entry.as_str().map(ToString::to_string);
                                if let Some(raw) = raw {
                                    if raw == alternate {
                                        *entry = YamlValue::String(desired.to_string());
                                        compose_updated = true;
                                        found_desired = true; // We just set it to desired
                                        outcome.changes.push(format!(
                                            "{}: {} → {}",
                                            compose_filename, alternate, desired
                                        ));
                                    } else if raw == desired {
                                        found_desired = true;
                                    }
                                }
                            }

                            if !found_desired {
                                volumes.insert(0, YamlValue::String(desired.to_string()));
                                compose_updated = true;
                                outcome
                                    .changes
                                    .push(format!("{}: added {}", compose_filename, desired));
                            }
                            // Continue to next service (don't break - multi-service support)
                        }
                    }
                }
            }
        }

        if compose_updated {
            let mut serialized = serde_yaml::to_string(&compose).map_err(|err| {
                Error::validation(format!(
                    "Failed to serialize {}: {}",
                    compose_path.display(),
                    err
                ))
            })?;
            if serialized.starts_with("---\n") {
                serialized = serialized.split_off(4);
            }
            if !serialized.ends_with('\n') {
                serialized.push('\n');
            }
            std::fs::write(&compose_path, serialized)?;
            outcome.compose_modified = true;
            tracing::info!("Updated {} for worktree compatibility", compose_filename);
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tempfile::TempDir;

    fn env_guard() -> MutexGuard<'static, ()> {
        static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("core crate lives under repo root")
            .to_path_buf()
    }

    #[test]
    fn test_detect_with_devcontainer() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".devcontainer")).unwrap();

        let module = DevcontainerModule::new();
        assert!(module.detect(temp.path()));
    }

    #[test]
    fn test_detect_without_devcontainer() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let module = DevcontainerModule::new();
        assert!(!module.detect(temp.path()));
    }

    #[test]
    fn test_sync_to_copies_files() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        // Create source devcontainer
        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(
            main.join(".devcontainer/devcontainer.json"),
            r#"{"name": "test"}"#,
        )
        .unwrap();
        std::fs::write(main.join(".devcontainer/compose.yaml"), "services: {}").unwrap();

        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        let outcome = module.sync_to(&feature).unwrap();

        // Verify files synced
        assert!(feature.join(".devcontainer/devcontainer.json").exists());
        assert!(feature.join(".devcontainer/compose.yaml").exists());
        assert_eq!(outcome.synced_files.len(), 2);
        assert!(matches!(outcome.strategy, SyncStrategy::Copy));
    }

    #[test]
    fn test_sync_excludes_env_file() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(main.join(".devcontainer/.env"), "SECRET=123").unwrap();
        std::fs::write(main.join(".devcontainer/devcontainer.json"), "{}").unwrap();
        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        let outcome = module.sync_to(&feature).unwrap();

        // .env should be excluded
        assert!(!feature.join(".devcontainer/.env").exists());
        // But other files should sync
        assert!(feature.join(".devcontainer/devcontainer.json").exists());
        assert_eq!(outcome.synced_files.len(), 1);
    }

    #[test]
    fn test_sync_with_subdirectories() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer/scripts")).unwrap();
        std::fs::write(main.join(".devcontainer/scripts/setup.sh"), "#!/bin/bash").unwrap();
        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        module.sync_to(&feature).unwrap();

        // Verify subdirectory synced
        assert!(feature.join(".devcontainer/scripts/setup.sh").exists());
    }

    #[test]
    fn test_sync_preserves_canonical_devcontainer_files() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");
        let repo_devcontainer = repo_root().join(".devcontainer");

        std::fs::create_dir_all(main.join(".devcontainer/scripts")).unwrap();

        for file in ["compose.yaml", "devcontainer.json"] {
            fs::copy(
                repo_devcontainer.join(file),
                main.join(".devcontainer").join(file),
            )
            .unwrap();
        }
        fs::copy(
            repo_devcontainer.join("scripts/ensure-gitdir.sh"),
            main.join(".devcontainer/scripts/ensure-gitdir.sh"),
        )
        .unwrap();

        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        module.sync_to(&feature).unwrap();

        let synced_compose =
            fs::read_to_string(feature.join(".devcontainer/compose.yaml")).unwrap();
        let expected_compose = fs::read_to_string(repo_devcontainer.join("compose.yaml")).unwrap();
        assert_eq!(synced_compose, expected_compose);

        let synced_devcontainer =
            fs::read_to_string(feature.join(".devcontainer/devcontainer.json")).unwrap();
        let expected_devcontainer =
            fs::read_to_string(repo_devcontainer.join("devcontainer.json")).unwrap();
        assert_eq!(synced_devcontainer, expected_devcontainer);
    }

    #[test]
    fn test_strategy_from_env_var() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::create_dir_all(&feature).unwrap();

        std::env::set_var("BRANCHBOX_DEVCONTAINER_STRATEGY", "symlink");
        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        std::env::remove_var("BRANCHBOX_DEVCONTAINER_STRATEGY");

        assert!(matches!(module.strategy, SyncStrategy::Symlink));
    }

    #[test]
    fn test_name() {
        let _guard = env_guard();
        let module = DevcontainerModule::new();
        assert_eq!(module.name(), "devcontainer");
    }

    #[test]
    fn test_default() {
        let _guard = env_guard();
        let module = DevcontainerModule::default();
        assert_eq!(module.name(), "devcontainer");
        assert!(matches!(module.strategy, SyncStrategy::Copy));
    }

    #[test]
    fn test_validate_missing_devcontainer() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");
        std::fs::create_dir_all(&feature).unwrap();

        let module = DevcontainerModule::new();
        let result = module.validate(&main, &feature);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_with_devcontainer() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");
        std::fs::create_dir_all(feature.join(".devcontainer")).unwrap();

        let module = DevcontainerModule::new();
        let result = module.validate(&main, &feature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sync_removes_stale_files() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        // Setup source devcontainer with one file
        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(
            main.join(".devcontainer/settings.json"),
            r#"{"keep": true}"#,
        )
        .unwrap();

        // Prepare feature devcontainer with stale content
        let feature_dev = feature.join(".devcontainer");
        std::fs::create_dir_all(feature_dev.join("stale_dir")).unwrap();
        std::fs::write(feature_dev.join("settings.json"), "old").unwrap();
        std::fs::write(feature_dev.join("stale.txt"), "remove me").unwrap();
        std::fs::write(feature_dev.join("stale_dir/nested.txt"), "remove me").unwrap();
        // Simulate symlinked env artifacts that should be preserved
        std::fs::write(feature_dev.join(".env"), "SHOULD_STAY=1").unwrap();
        std::fs::write(feature_dev.join(".branchbox.env"), "WORK_FEATURE=keep").unwrap();

        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        module.sync_to(&feature).unwrap();

        // Verify source file refreshed
        let new_contents =
            std::fs::read_to_string(feature.join(".devcontainer/settings.json")).unwrap();
        assert_eq!(new_contents, r#"{"keep": true}"#);

        // Stale artifacts should be removed
        assert!(!feature.join(".devcontainer/stale.txt").exists());
        assert!(!feature.join(".devcontainer/stale_dir").exists());

        // Excluded env files should remain untouched
        assert!(feature.join(".devcontainer/.env").exists());
        let managed_env =
            std::fs::read_to_string(feature.join(".devcontainer/.branchbox.env")).unwrap();
        assert!(managed_env.contains("WORK_FEATURE=keep"));
    }

    #[test]
    fn test_teardown_does_nothing() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        let module = DevcontainerModule::new();
        let result = module.teardown(&main, &feature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_configure_workspace_settings_for_feature() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(
            main.join(".devcontainer/devcontainer.json"),
            r#"{
  "name": "Test",
  "workspaceFolder": "/workspaces",
  "workspaceMount": "source=${localWorkspaceFolder},target=/workspaces,type=bind,consistency=cached"
}"#,
        )
        .unwrap();
        std::fs::write(
            main.join(".devcontainer/compose.yaml"),
            r#"services:
  rust-dev:
    volumes:
      - ..:/workspaces:cached
      - dind-data:/var/lib/docker
"#,
        )
        .unwrap();

        std::fs::create_dir_all(&feature).unwrap();
        std::fs::write(
            feature.join(".git"),
            "gitdir: ../main/.git/worktrees/feature\n",
        )
        .unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        module.setup(&main, &feature).unwrap();

        let feature_config =
            std::fs::read_to_string(feature.join(".devcontainer/devcontainer.json")).unwrap();
        assert!(feature_config.contains("/workspaces/${localWorkspaceFolderBasename}"));
        assert!(feature_config.contains(
            "source=${localWorkspaceFolder},target=/workspaces/${localWorkspaceFolderBasename}"
        ));

        let feature_compose =
            std::fs::read_to_string(feature.join(".devcontainer/compose.yaml")).unwrap();
        assert!(feature_compose.contains("- ../..:/workspaces:cached"));
        assert!(!feature_compose.contains("- ..:/workspaces:cached"));
    }

    #[test]
    fn test_configure_workspace_settings_handles_jsonc_comments() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(
            main.join(".devcontainer/devcontainer.json"),
            r#"// comment allowed by VS Code JSONC
{
  // comment before name
  "name": "Test",
  "workspaceFolder": "/workspaces",
}
"#,
        )
        .unwrap();
        std::fs::write(
            main.join(".devcontainer/compose.yaml"),
            r#"services:
  rust-dev:
    volumes:
      - ..:/workspaces:cached
"#,
        )
        .unwrap();

        std::fs::create_dir_all(&feature).unwrap();
        std::fs::write(
            feature.join(".git"),
            "gitdir: ../main/.git/worktrees/feature\n",
        )
        .unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        module.setup(&main, &feature).unwrap();

        let config =
            std::fs::read_to_string(feature.join(".devcontainer/devcontainer.json")).unwrap();
        assert!(config.contains("/workspaces/${localWorkspaceFolderBasename}"));
        assert!(!config.contains("// comment"));

        let compose = std::fs::read_to_string(feature.join(".devcontainer/compose.yaml")).unwrap();
        assert!(compose.contains("- ../..:/workspaces:cached"));
    }

    #[test]
    fn test_configure_workspace_settings_for_main_repo() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let target = temp.path().join("main-copy");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(
            main.join(".devcontainer/devcontainer.json"),
            r#"{
  "name": "Test",
  "workspaceFolder": "/workspaces",
  "workspaceMount": "source=${localWorkspaceFolder},target=/workspaces,type=bind,consistency=cached"
}"#,
        )
        .unwrap();
        std::fs::write(
            main.join(".devcontainer/compose.yaml"),
            r#"services:
  rust-dev:
    volumes:
      - ..:/workspaces:cached
      - dind-data:/var/lib/docker
"#,
        )
        .unwrap();

        std::fs::create_dir_all(target.join(".git")).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &target).unwrap();
        module.setup(&main, &target).unwrap();

        let config =
            std::fs::read_to_string(target.join(".devcontainer/devcontainer.json")).unwrap();
        assert!(config.contains("/workspaces/${localWorkspaceFolderBasename}"));
        assert!(config.contains(
            "source=${localWorkspaceFolder},target=/workspaces/${localWorkspaceFolderBasename},type=bind,consistency=cached"
        ));

        let compose = std::fs::read_to_string(target.join(".devcontainer/compose.yaml")).unwrap();
        assert!(compose.contains("- ../..:/workspaces:cached"));
        assert!(!compose.contains("- ..:/workspaces:cached"));
    }

    #[test]
    fn test_configure_workspace_settings_multi_service_compose() {
        // Test that all services in a multi-service compose file are updated
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        // Create a compose file with multiple services, each with volumes
        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    image: node:20
    volumes:
      - ..:/workspaces:cached
      - app-data:/data
  db:
    image: postgres:15
    volumes:
      - ..:/workspaces:cached
      - db-data:/var/lib/postgresql/data
  redis:
    image: redis:7
    volumes:
      - ..:/workspaces:cached
volumes:
  app-data: null
  db-data: null
"#,
        )
        .unwrap();

        let outcome = configure_workspace_settings(&devcontainer_dir).unwrap();

        // Should have updated all three services
        assert!(outcome.compose_modified);
        // Check that we have 3 changes for the 3 services
        let compose_changes: Vec<_> = outcome
            .changes
            .iter()
            .filter(|c| c.contains("compose.yaml"))
            .collect();
        assert_eq!(
            compose_changes.len(),
            3,
            "Expected 3 compose changes for 3 services, got: {:?}",
            compose_changes
        );

        // Verify the file was actually updated correctly
        let compose = std::fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap();

        // Count occurrences of the correct mount
        let correct_mount_count = compose.matches("../..:/workspaces:cached").count();
        assert_eq!(
            correct_mount_count, 3,
            "Expected 3 correct mounts, got {}. Compose:\n{}",
            correct_mount_count, compose
        );

        // Ensure no old mounts remain
        assert!(
            !compose.contains("- ..:/workspaces:cached"),
            "Old mount still present in compose:\n{}",
            compose
        );
    }

    #[test]
    fn test_workspace_configuration_skips_for_symlink_strategy() {
        let _guard = env_guard();
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(
            main.join(".devcontainer/devcontainer.json"),
            r#"{
  "name": "Test",
  "workspaceFolder": "/workspaces",
  "workspaceMount": "source=${localWorkspaceFolder},target=/workspaces,type=bind,consistency=cached"
}"#,
        )
        .unwrap();
        std::fs::write(
            main.join(".devcontainer/compose.yaml"),
            r#"services:
  rust-dev:
    volumes:
      - ..:/workspaces:cached
      - dind-data:/var/lib/docker
"#,
        )
        .unwrap();

        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        module.strategy = SyncStrategy::Symlink;
        module.setup(&main, &feature).unwrap();

        let main_config =
            std::fs::read_to_string(main.join(".devcontainer/devcontainer.json")).unwrap();
        let feature_config =
            std::fs::read_to_string(feature.join(".devcontainer/devcontainer.json")).unwrap();

        assert_eq!(feature_config, main_config);
        assert!(main_config.contains("\"/workspaces\""));
    }

    #[test]
    fn test_configure_workspace_settings_docker_compose_yml() {
        // Test that docker-compose.yml is detected and updated
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test", "dockerComposeFile": "docker-compose.yml"}"#,
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("docker-compose.yml"),
            r#"services:
  app:
    volumes:
      - ..:/workspaces:cached
"#,
        )
        .unwrap();

        let outcome = configure_workspace_settings(&devcontainer_dir).unwrap();

        assert!(outcome.compose_modified);
        assert!(outcome
            .changes
            .iter()
            .any(|c| c.contains("docker-compose.yml")));

        let compose = std::fs::read_to_string(devcontainer_dir.join("docker-compose.yml")).unwrap();
        assert!(compose.contains("../..:/workspaces:cached"));
    }

    #[test]
    fn test_configure_workspace_settings_fallback_compose_names() {
        // Test fallback when dockerComposeFile is not specified
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        // No dockerComposeFile specified, but docker-compose.yaml exists
        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("docker-compose.yaml"),
            r#"services:
  app:
    volumes:
      - ..:/workspaces:cached
"#,
        )
        .unwrap();

        let outcome = configure_workspace_settings(&devcontainer_dir).unwrap();

        assert!(outcome.compose_modified);
        assert!(outcome
            .changes
            .iter()
            .any(|c| c.contains("docker-compose.yaml")));
    }

    #[test]
    fn test_configure_workspace_settings_dockerfile_only() {
        // Test Dockerfile-only setup (no compose file)
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test", "workspaceFolder": "/workspaces"}"#,
        )
        .unwrap();

        std::fs::write(devcontainer_dir.join("Dockerfile"), "FROM ubuntu:22.04\n").unwrap();

        let outcome = configure_workspace_settings(&devcontainer_dir).unwrap();

        // devcontainer.json should be updated
        assert!(outcome.devcontainer_modified);
        // No compose file to update
        assert!(!outcome.compose_modified);
    }

    #[test]
    fn test_add_cloudflared_service() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test", "dockerComposeFile": "compose.yaml"}"#,
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  rails-app:
    build:
      context: ..
      dockerfile: .devcontainer/Dockerfile
    volumes:
      - ../..:/workspaces:cached
  postgres:
    image: postgres:15
"#,
        )
        .unwrap();

        let outcome = add_cloudflared_service(&devcontainer_dir, None).unwrap();

        assert!(outcome.compose_modified);
        assert!(outcome.env_created);
        assert!(outcome
            .changes
            .iter()
            .any(|c| c.contains("added cloudflared service")));
        assert!(outcome
            .changes
            .iter()
            .any(|c| c.contains("added cloudflared to rails-app depends_on")));

        // Verify compose file has cloudflared service
        let compose = std::fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap();
        assert!(compose.contains("cloudflared:"));
        assert!(compose.contains("cloudflare/cloudflared:latest"));
        assert!(compose.contains(".cloudflared.env"));

        // Verify .cloudflared.env was created
        assert!(devcontainer_dir.join(".cloudflared.env").exists());
        let env_content =
            std::fs::read_to_string(devcontainer_dir.join(".cloudflared.env")).unwrap();
        assert!(env_content.contains("TUNNEL_TOKEN="));
    }

    #[test]
    fn test_add_cloudflared_service_already_exists() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        // Compose file already has cloudflared
        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    build: .
  cloudflared:
    image: cloudflare/cloudflared:latest
"#,
        )
        .unwrap();

        let outcome = add_cloudflared_service(&devcontainer_dir, None).unwrap();

        // Should not modify since cloudflared already exists
        assert!(!outcome.compose_modified);
        assert!(outcome.changes.is_empty() || outcome.env_created);
    }

    #[test]
    fn test_add_cloudflared_service_no_compose() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        // No compose file, only Dockerfile
        std::fs::write(devcontainer_dir.join("Dockerfile"), "FROM ubuntu:22.04\n").unwrap();

        let outcome = add_cloudflared_service(&devcontainer_dir, None).unwrap();

        // Should not modify anything
        assert!(!outcome.compose_modified);
        assert!(!outcome.env_created);
    }
}
