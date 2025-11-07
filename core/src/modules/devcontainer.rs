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
        let outcome = self.sync_with_workspace_settings(feature_dir)?;
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
    /// Sync devcontainer files and reapply workspace-specific settings (workspace folder/mount).
    pub fn sync_with_workspace_settings(&self, feature_dir: &Path) -> Result<SyncOutcome> {
        let outcome = self.sync_to(feature_dir)?;
        if matches!(self.strategy, SyncStrategy::Symlink) {
            tracing::info!("Skipping workspace configuration (symlink strategy in use)");
        } else {
            self.configure_workspace_settings(feature_dir)?;
        }
        Ok(outcome)
    }

    fn configure_workspace_settings(&self, feature_dir: &Path) -> Result<()> {
        let config_path = feature_dir.join(".devcontainer/devcontainer.json");
        if !config_path.exists() {
            return Ok(());
        }

        let compose_path = feature_dir.join(".devcontainer/compose.yaml");
        let config_contents = std::fs::read_to_string(&config_path)?;
        let mut config = parse_json_with_comments(&config_contents, &config_path)?;

        let is_main_repo = feature_dir.join(".git").is_dir();

        let (workspace_folder, workspace_mount) = if is_main_repo {
            (
                "/workspaces",
                "source=${localWorkspaceFolder},target=/workspaces,type=bind,consistency=cached",
            )
        } else {
            (
                "/workspaces/${localWorkspaceFolderBasename}",
                "source=${localWorkspaceFolder},target=/workspaces/${localWorkspaceFolderBasename},type=bind,consistency=cached",
            )
        };

        match config.as_object_mut() {
            Some(map) => {
                map.insert(
                    "workspaceFolder".to_string(),
                    JsonValue::String(workspace_folder.to_string()),
                );
                map.insert(
                    "workspaceMount".to_string(),
                    JsonValue::String(workspace_mount.to_string()),
                );
            }
            None => {
                return Err(Error::validation(format!(
                    "{} is not a JSON object",
                    config_path.display()
                )))
            }
        }

        let mut formatted = serde_json::to_string_pretty(&config)?;
        formatted.push('\n');
        std::fs::write(&config_path, formatted)?;

        if compose_path.exists() {
            self.update_compose_volumes(&compose_path, is_main_repo, feature_dir)?;
        }

        Ok(())
    }

    fn update_compose_volumes(
        &self,
        compose_path: &Path,
        is_main_repo: bool,
        feature_dir: &Path,
    ) -> Result<()> {
        let compose_contents = std::fs::read_to_string(compose_path)?;
        let mut compose: YamlValue = serde_yaml::from_str(&compose_contents).map_err(|err| {
            Error::validation(format!(
                "Failed to parse {}: {}",
                compose_path.display(),
                err
            ))
        })?;

        let mut updated = false;
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
                            if is_main_repo {
                                updated |= Self::ensure_main_volumes(volumes);
                            } else {
                                updated |= Self::ensure_feature_volumes(volumes, feature_dir);
                            }
                            break;
                        }
                    }
                }
            }
        }

        if updated {
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
            std::fs::write(compose_path, serialized)?;
        }

        Ok(())
    }

    fn ensure_main_volumes(volumes: &mut Vec<YamlValue>) -> bool {
        let desired = "..:/workspaces:cached";
        let alternate = "../..:/workspaces:cached";
        let mut updated = false;

        let mut found_desired = false;
        for entry in volumes.iter_mut() {
            if let Some(raw) = entry.as_str().map(|s| s.to_string()) {
                if raw == alternate {
                    *entry = YamlValue::String(desired.to_string());
                    updated = true;
                }
                if raw == desired {
                    found_desired = true;
                }
            }
        }

        if !found_desired {
            volumes.insert(0, YamlValue::String(desired.to_string()));
            updated = true;
        }

        updated
    }

    fn ensure_feature_volumes(volumes: &mut Vec<YamlValue>, feature_dir: &Path) -> bool {
        let feature_name = feature_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("feature");

        let workspace_mount = format!("..:/workspaces/${{WORK_FEATURE:-{}}}:cached", feature_name);
        let legacy_mounts = ["../..:/workspaces:cached", "..:/workspaces:cached"];

        let mut updated = false;
        volumes.retain(|entry| {
            if let Some(raw) = entry.as_str() {
                if legacy_mounts.contains(&raw) {
                    updated = true;
                    return false;
                }
            }
            true
        });

        if !volumes
            .iter()
            .any(|entry| entry.as_str() == Some(&workspace_mount))
        {
            volumes.insert(0, YamlValue::String(workspace_mount.clone()));
            updated = true;
        }

        let git_mount = "../../${BRANCHBOX_MAIN_NAME:-main}/.git:/workspaces/${BRANCHBOX_MAIN_NAME:-main}/.git:cached";
        if !volumes
            .iter()
            .any(|entry| entry.as_str() == Some(git_mount))
        {
            volumes.insert(1, YamlValue::String(git_mount.to_string()));
            updated = true;
        }

        updated
    }
}

#[derive(Debug)]
pub struct SyncOutcome {
    pub synced_files: Vec<String>,
    pub strategy: SyncStrategy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tempfile::TempDir;

    fn env_guard() -> MutexGuard<'static, ()> {
        static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap()
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
        // Simulate symlinked .env that should be preserved
        std::fs::write(feature_dev.join(".env"), "SHOULD_STAY=1").unwrap();

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

        // Excluded .env should remain untouched
        assert!(feature.join(".devcontainer/.env").exists());
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
        assert!(feature_compose.contains("- ..:/workspaces/${WORK_FEATURE:-feature}:cached"));
        assert!(feature_compose.contains(
            "- ../../${BRANCHBOX_MAIN_NAME:-main}/.git:/workspaces/${BRANCHBOX_MAIN_NAME:-main}/.git:cached"
        ));
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
        assert!(compose.contains("- ..:/workspaces/${WORK_FEATURE:-feature}:cached"));
    }

    #[test]
    fn test_sync_with_workspace_settings_restores_mounts() {
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

        // Simulate a raw sync that would leave the feature with incorrect mounts.
        module.sync_to(&feature).unwrap();
        let compose_before =
            std::fs::read_to_string(feature.join(".devcontainer/compose.yaml")).unwrap();
        assert!(compose_before.contains("- ..:/workspaces:cached"));

        module.sync_with_workspace_settings(&feature).unwrap();
        let compose_after =
            std::fs::read_to_string(feature.join(".devcontainer/compose.yaml")).unwrap();
        assert!(compose_after.contains("- ..:/workspaces/${WORK_FEATURE:-feature}:cached"));
        assert!(compose_after.contains(
            "- ../../${BRANCHBOX_MAIN_NAME:-main}/.git:/workspaces/${BRANCHBOX_MAIN_NAME:-main}/.git:cached"
        ));
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
        assert!(config.contains("\"/workspaces\""));
        assert!(config.contains(
            "source=${localWorkspaceFolder},target=/workspaces,type=bind,consistency=cached"
        ));

        let compose = std::fs::read_to_string(target.join(".devcontainer/compose.yaml")).unwrap();
        assert!(compose.contains("- ..:/workspaces:cached"));
        assert!(!compose.contains("- ../..:/workspaces:cached"));
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
}
