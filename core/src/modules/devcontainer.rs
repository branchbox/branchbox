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

const SKIP_INFRA_SERVICES: [&str; 6] =
    ["cloudflared", "postgres", "redis", "mysql", "mongodb", "db"];

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
                ".cloudflared.env".to_string(),
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
    // Try to read dockerComposeFile from devcontainer.json (can be string or array)
    let explicit_files: Vec<&str> = match config.get("dockerComposeFile") {
        Some(JsonValue::String(s)) => vec![s.as_str()],
        Some(JsonValue::Array(arr)) => arr.iter().filter_map(|f| f.as_str()).collect(),
        _ => vec![],
    };

    explicit_files
        .into_iter()
        .chain([
            "compose.yaml",
            "compose.yml",
            "docker-compose.yaml",
            "docker-compose.yml",
        ])
        .map(|name| devcontainer_dir.join(name))
        .find(|path| path.exists())
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
/// Detected service information from compose file.
#[derive(Debug, Clone, Default)]
pub struct ServiceInfo {
    /// Name of the main service (e.g., "rails-app", "flask-app", "app")
    pub name: Option<String>,
    /// Detected or default port for the service
    pub port: u16,
    /// Full service URL for tunnel ingress (e.g., "http://flask-app:5000")
    pub service_url: String,
}

/// Default ports by stack/framework type.
pub fn default_port_for_stack(stack_hint: Option<&str>) -> u16 {
    match stack_hint {
        Some(s) if s.contains("flask") || s.contains("python") || s.contains("django") => 5000,
        Some(s) if s.contains("rails") || s.contains("ruby") => 3000,
        Some(s) if s.contains("node") || s.contains("express") || s.contains("next") => 3000,
        Some(s) if s.contains("rust") || s.contains("actix") || s.contains("axum") => 8080,
        Some(s) if s.contains("go") || s.contains("gin") => 8080,
        Some(s) if s.contains("php") || s.contains("laravel") => 8000,
        _ => 3000, // Default fallback
    }
}

fn default_service_info(stack_hint: Option<&str>) -> ServiceInfo {
    let port = default_port_for_stack(stack_hint);
    ServiceInfo {
        name: Some("app".to_string()),
        port,
        service_url: format!("http://app:{}", port),
    }
}

fn detect_main_service_name(services: &serde_yaml::Mapping) -> Option<String> {
    let build_key = YamlValue::String("build".to_string());
    let mut first_non_infra_service = None;

    for (key, value) in services.iter() {
        if let Some(name) = key.as_str() {
            if SKIP_INFRA_SERVICES.contains(&name) {
                continue;
            }

            // Track first non-infrastructure service as fallback
            if first_non_infra_service.is_none() {
                first_non_infra_service = Some(name.to_string());
            }

            // Prefer service with build section
            if let Some(service_map) = value.as_mapping() {
                if service_map.contains_key(&build_key) {
                    return Some(name.to_string());
                }
            }
        }
    }

    first_non_infra_service
}

/// Detect the main service name and port from compose file.
///
/// This function reads the compose file and extracts:
/// - The main service name (first service with a build section)
/// - The port (from exposed ports, or guessed from stack type)
///
/// # Arguments
///
/// * `devcontainer_dir` - Path to the .devcontainer directory
/// * `stack_hint` - Optional hint about the stack type (e.g., "flask", "rails")
///
/// # Returns
///
/// Returns `ServiceInfo` with detected values, falling back to sensible defaults.
pub fn detect_main_service(
    devcontainer_dir: &Path,
    stack_hint: Option<&str>,
) -> Result<ServiceInfo> {
    let config_path = devcontainer_dir.join("devcontainer.json");
    if !config_path.exists() {
        return Ok(default_service_info(stack_hint));
    }

    let config_contents = std::fs::read_to_string(&config_path)?;
    let config = parse_json_with_comments(&config_contents, &config_path)?;

    let compose_path = match find_compose_file(devcontainer_dir, &config) {
        Some(p) => p,
        None => {
            return Ok(default_service_info(stack_hint));
        }
    };

    let compose_contents = std::fs::read_to_string(&compose_path)?;
    let compose: YamlValue = serde_yaml::from_str(&compose_contents).map_err(|err| {
        Error::validation(format!(
            "Failed to parse {}: {}",
            compose_path.display(),
            err
        ))
    })?;

    let mut service_name: Option<String> = None;
    let mut detected_port: Option<u16> = None;

    if let Some(root) = compose.as_mapping() {
        let services_key = YamlValue::String("services".to_string());

        if let Some(services) = root.get(&services_key).and_then(|v| v.as_mapping()) {
            // Find main service (first with build section, excluding cloudflared/postgres/redis)
            for (key, value) in services.iter() {
                if let Some(name) = key.as_str() {
                    // Skip infrastructure services
                    if SKIP_INFRA_SERVICES.contains(&name) {
                        continue;
                    }

                    if let Some(service_map) = value.as_mapping() {
                        let build_key = YamlValue::String("build".to_string());
                        if service_map.contains_key(&build_key) {
                            service_name = Some(name.to_string());

                            // Try to detect port from ports section
                            let ports_key = YamlValue::String("ports".to_string());
                            if let Some(ports) =
                                service_map.get(&ports_key).and_then(|v| v.as_sequence())
                            {
                                for port_entry in ports {
                                    if let Some(port_str) = port_entry.as_str() {
                                        // Parse "8080:8080" or "3000:3000" format
                                        if let Some(container_port) = port_str
                                            .split(':')
                                            .next_back()
                                            .and_then(|p| p.split('/').next())
                                            .and_then(|p| p.parse::<u16>().ok())
                                        {
                                            detected_port = Some(container_port);
                                            break;
                                        }
                                    }
                                }
                            }
                            break;
                        }
                    }
                }
            }

            // If no service with build, take first non-infrastructure service
            if service_name.is_none() {
                for (key, _) in services.iter() {
                    if let Some(name) = key.as_str() {
                        if !SKIP_INFRA_SERVICES.contains(&name) {
                            service_name = Some(name.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }

    let port = detected_port.unwrap_or_else(|| default_port_for_stack(stack_hint));
    let name = service_name.unwrap_or_else(|| "app".to_string());
    let service_url = format!("http://{}:{}", name, port);

    Ok(ServiceInfo {
        name: Some(name),
        port,
        service_url,
    })
}

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
        let cloudflared_exists =
            if let Some(services) = root.get(&services_key).and_then(|v| v.as_mapping()) {
                let cloudflared_key = YamlValue::String("cloudflared".to_string());
                if services.contains_key(&cloudflared_key) {
                    tracing::info!("cloudflared service already exists in {}", compose_filename);
                    true
                } else {
                    false
                }
            } else {
                false
            };

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

        // Only add cloudflared service if it doesn't already exist
        if !cloudflared_exists {
            // Create cloudflared service
            // Use required: false so compose doesn't fail if env file is missing
            let cloudflared_service = serde_yaml::from_str::<YamlValue>(
                r#"
image: cloudflare/cloudflared:latest
restart: unless-stopped
env_file:
  - path: .cloudflared.env
    required: false
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
/// - Updating compose.yaml volume mount for the main app service to use `../..:/workspaces:cached`
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
                let main_service_name = detect_main_service_name(services);
                if let Some(main_name) = main_service_name {
                    let main_key = YamlValue::String(main_name);
                    if let Some(service) = services.get_mut(&main_key) {
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
            tracing::info!("Updated {} for worktree compatibility", compose_filename);
        }
    }

    Ok(outcome)
}

// ============================================================================
// Coding Agent Mount Injection
// ============================================================================

/// Detected container user information for AI coding agent mounts.
#[derive(Debug, Clone)]
pub struct ContainerUser {
    /// Username (e.g., "vscode", "root", "node")
    pub username: String,
    /// Home directory path (e.g., "/home/vscode", "/root")
    pub home_path: String,
}

impl Default for ContainerUser {
    fn default() -> Self {
        Self {
            username: "vscode".to_string(),
            home_path: "/home/vscode".to_string(),
        }
    }
}

/// Result of injecting coding agent mounts.
#[derive(Debug, Default)]
pub struct CodingAgentOutcome {
    /// Whether compose file was modified
    pub compose_modified: bool,
    /// List of changes made
    pub changes: Vec<String>,
    /// Detected container user
    pub container_user: Option<ContainerUser>,
}

/// Find Dockerfile path based on devcontainer.json configuration.
fn find_dockerfile(devcontainer_dir: &Path, config: &JsonValue) -> Option<PathBuf> {
    // Check for "build.dockerfile" in devcontainer.json
    if let Some(build) = config.get("build") {
        if let Some(dockerfile) = build.get("dockerfile").and_then(|d| d.as_str()) {
            let path = devcontainer_dir.join(dockerfile);
            if path.exists() {
                return Some(path);
            }
        }
    }

    // Check for "dockerFile" (legacy) in devcontainer.json
    if let Some(dockerfile) = config.get("dockerFile").and_then(|d| d.as_str()) {
        let path = devcontainer_dir.join(dockerfile);
        if path.exists() {
            return Some(path);
        }
    }

    // Try common names
    for name in ["Dockerfile", "Dockerfile.dev", "dockerfile"] {
        let path = devcontainer_dir.join(name);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Parse Dockerfile content to find the final USER directive.
fn parse_dockerfile_user(contents: &str) -> Option<String> {
    let mut last_user: Option<String> = None;

    for line in contents.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse USER directive (case-insensitive for robustness)
        if trimmed.to_uppercase().starts_with("USER ") {
            let user_part = trimmed[5..].trim();
            // Handle "USER username" and "USER username:group" formats
            let username = user_part.split(':').next().unwrap();
            // Handle ARG substitution - if it contains ${}, skip it
            if !username.contains("${") {
                last_user = Some(username.to_string());
            }
        }
    }

    last_user
}

/// Map username to home directory path.
fn get_home_path_for_user(username: &str) -> String {
    match username {
        "root" => "/root".to_string(),
        user => format!("/home/{}", user),
    }
}

/// Detect the container user from Dockerfile.
///
/// Parses the Dockerfile to find the final USER directive.
/// If no USER directive is found, defaults to "vscode".
///
/// # Arguments
/// * `devcontainer_dir` - Path to .devcontainer directory
/// * `config` - Parsed devcontainer.json
///
/// # Returns
/// ContainerUser with detected username and home path
pub fn detect_container_user(devcontainer_dir: &Path, config: &JsonValue) -> ContainerUser {
    let dockerfile_path = find_dockerfile(devcontainer_dir, config);

    if let Some(path) = dockerfile_path {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Some(user) = parse_dockerfile_user(&contents) {
                return ContainerUser {
                    username: user.clone(),
                    home_path: get_home_path_for_user(&user),
                };
            }
        }
    }

    ContainerUser::default()
}

/// Generate the shared config mount entries for AI coding agents.
fn coding_agent_mount_entries(home_path: &str) -> Vec<String> {
    let host_pattern = "${SHARED_CONFIG_DIR:-../..}";
    vec![
        format!("{}/.codex:{}/.codex", host_pattern, home_path),
        format!("{}/.claude:{}/.claude", host_pattern, home_path),
        format!("{}/.claude.json:{}/.claude.json", host_pattern, home_path),
        format!("{}/.gh:{}/.config/gh", host_pattern, home_path),
    ]
}

/// Inject shared AI coding agent volume mounts into compose file.
///
/// This function:
/// 1. Detects the container user from Dockerfile
/// 2. Adds volume mounts for .codex, .claude, .claude.json, .gh to main service
/// 3. Is idempotent - won't add duplicate mounts
///
/// # Arguments
/// * `devcontainer_dir` - Path to .devcontainer directory
///
/// # Returns
/// CodingAgentOutcome describing changes made
pub fn inject_coding_agent_mounts(devcontainer_dir: &Path) -> Result<CodingAgentOutcome> {
    let mut outcome = CodingAgentOutcome::default();

    let config_path = devcontainer_dir.join("devcontainer.json");
    if !config_path.exists() {
        return Ok(outcome);
    }

    let config_contents = std::fs::read_to_string(&config_path)?;
    let config = parse_json_with_comments(&config_contents, &config_path)?;

    // Detect container user
    let container_user = detect_container_user(devcontainer_dir, &config);
    outcome.container_user = Some(container_user.clone());

    // Find compose file
    let compose_path = match find_compose_file(devcontainer_dir, &config) {
        Some(p) => p,
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

    // Generate mount entries
    let mount_entries = coding_agent_mount_entries(&container_user.home_path);

    let mut compose_updated = false;

    if let Some(root) = compose.as_mapping_mut() {
        let services_key = YamlValue::String("services".to_string());

        if let Some(services) = root.get_mut(&services_key).and_then(|v| v.as_mapping_mut()) {
            // Find main service (skip infrastructure services)
            let main_service_name = detect_main_service_name(services);

            if let Some(main_name) = main_service_name {
                let main_key = YamlValue::String(main_name.clone());

                if let Some(service) = services.get_mut(&main_key) {
                    if let Some(service_map) = service.as_mapping_mut() {
                        let volumes_key = YamlValue::String("volumes".to_string());

                        // Get or create volumes array
                        let volumes = service_map
                            .entry(volumes_key.clone())
                            .or_insert_with(|| YamlValue::Sequence(vec![]));

                        if let Some(volumes_seq) = volumes.as_sequence_mut() {
                            // Check existing mounts to avoid duplicates
                            let existing: HashSet<String> = volumes_seq
                                .iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect();

                            for entry in &mount_entries {
                                // Check if mount already exists by target path
                                // Use next_back() because source may contain colons (e.g., ${VAR:-default})
                                let target_path = entry.split(':').next_back().unwrap_or("");
                                let already_exists = existing.iter().any(|e| {
                                    // Check if any part matches the target path
                                    // This handles both "source:target" and "source:target:options"
                                    e.split(':').any(|part| part == target_path)
                                });

                                if !already_exists {
                                    volumes_seq.push(YamlValue::String(entry.clone()));
                                    compose_updated = true;
                                    outcome.changes.push(format!(
                                        "{}: added mount {}",
                                        compose_filename, entry
                                    ));
                                }
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
        tracing::info!(
            "Injected coding agent mounts into {} (user: {})",
            compose_filename,
            container_user.username
        );
    }

    // Always ensure SHARED_CONFIG_DIR is set in .branchbox.env
    // (even if compose mounts already existed from templates)
    if let Some(change) = ensure_shared_config_dir(devcontainer_dir)? {
        outcome.changes.push(change);
    }

    Ok(outcome)
}

/// Ensure SHARED_CONFIG_DIR is set in .branchbox.env file.
///
/// Returns a change description if the file was modified.
fn ensure_shared_config_dir(devcontainer_dir: &Path) -> Result<Option<String>> {
    let env_path = devcontainer_dir.join(".branchbox.env");
    let shared_config_line = "SHARED_CONFIG_DIR=../..";

    if env_path.exists() {
        let contents = std::fs::read_to_string(&env_path)?;

        // Check if SHARED_CONFIG_DIR is already set (not commented)
        let has_shared_config = contents
            .lines()
            .any(|line| line.trim().starts_with("SHARED_CONFIG_DIR="));

        if !has_shared_config {
            // Append SHARED_CONFIG_DIR to existing file
            let mut new_contents = contents;
            if !new_contents.ends_with('\n') {
                new_contents.push('\n');
            }
            new_contents.push_str("\n# Shared config directory for AI coding agent mounts\n");
            new_contents.push_str(shared_config_line);
            new_contents.push('\n');
            std::fs::write(&env_path, new_contents)?;
            return Ok(Some(
                ".branchbox.env: added SHARED_CONFIG_DIR=../..".to_string(),
            ));
        }
    } else {
        // Create .branchbox.env with SHARED_CONFIG_DIR
        let contents = format!(
            "# BranchBox devcontainer overrides (auto-generated)\n\
             # Shared config directory for AI coding agent mounts\n\
             # This path is relative to the compose file location (.devcontainer/)\n\
             {}\n",
            shared_config_line
        );
        std::fs::write(&env_path, contents)?;
        return Ok(Some(
            ".branchbox.env: created with SHARED_CONFIG_DIR=../..".to_string(),
        ));
    }

    Ok(None)
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
        // Test that only the main app service is updated in a multi-service compose file
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        // Create a compose file with multiple services
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
      - db-data:/var/lib/postgresql/data
  redis:
    image: redis:7
    volumes:
      - redis-data:/data
volumes:
  app-data: null
  db-data: null
  redis-data: null
"#,
        )
        .unwrap();

        let outcome = configure_workspace_settings(&devcontainer_dir).unwrap();

        // Should have updated the main app service only
        assert!(outcome.compose_modified);
        let compose_changes: Vec<_> = outcome
            .changes
            .iter()
            .filter(|c| c.contains("compose.yaml"))
            .collect();
        assert_eq!(
            compose_changes.len(),
            1,
            "Expected 1 compose change for main service, got: {:?}",
            compose_changes
        );

        // Verify the file was actually updated correctly
        let compose = std::fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap();

        // Count occurrences of the correct mount
        let correct_mount_count = compose.matches("../..:/workspaces:cached").count();
        assert_eq!(
            correct_mount_count, 1,
            "Expected 1 correct mount, got {}. Compose:\n{}",
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

        // Should not modify compose since cloudflared already exists
        assert!(!outcome.compose_modified);

        // But should still create .cloudflared.env if it doesn't exist (P2 bug fix)
        assert!(outcome.env_created);
        assert!(devcontainer_dir.join(".cloudflared.env").exists());
        let env_content =
            std::fs::read_to_string(devcontainer_dir.join(".cloudflared.env")).unwrap();
        assert!(env_content.contains("TUNNEL_TOKEN="));
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

    #[test]
    fn test_default_port_for_stack() {
        assert_eq!(default_port_for_stack(Some("flask")), 5000);
        assert_eq!(default_port_for_stack(Some("python")), 5000);
        assert_eq!(default_port_for_stack(Some("django")), 5000);
        assert_eq!(default_port_for_stack(Some("rails")), 3000);
        assert_eq!(default_port_for_stack(Some("ruby")), 3000);
        assert_eq!(default_port_for_stack(Some("node")), 3000);
        assert_eq!(default_port_for_stack(Some("express")), 3000);
        assert_eq!(default_port_for_stack(Some("next")), 3000);
        assert_eq!(default_port_for_stack(Some("rust")), 8080);
        assert_eq!(default_port_for_stack(Some("actix")), 8080);
        assert_eq!(default_port_for_stack(Some("go")), 8080);
        assert_eq!(default_port_for_stack(Some("php")), 8000);
        assert_eq!(default_port_for_stack(Some("laravel")), 8000);
        assert_eq!(default_port_for_stack(None), 3000); // Default fallback
        assert_eq!(default_port_for_stack(Some("unknown")), 3000);
    }

    #[test]
    fn test_detect_main_service_rails() {
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
    ports:
      - "3000:3000"
  postgres:
    image: postgres:15
"#,
        )
        .unwrap();

        let info = detect_main_service(&devcontainer_dir, None).unwrap();
        assert_eq!(info.name, Some("rails-app".to_string()));
        assert_eq!(info.port, 3000);
        assert_eq!(info.service_url, "http://rails-app:3000");
    }

    #[test]
    fn test_detect_main_service_flask() {
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
  flask-app:
    build:
      context: ..
    ports:
      - "5000:5000"
  redis:
    image: redis:7
"#,
        )
        .unwrap();

        let info = detect_main_service(&devcontainer_dir, None).unwrap();
        assert_eq!(info.name, Some("flask-app".to_string()));
        assert_eq!(info.port, 5000);
        assert_eq!(info.service_url, "http://flask-app:5000");
    }

    #[test]
    fn test_detect_main_service_no_ports() {
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
  web:
    build:
      context: ..
"#,
        )
        .unwrap();

        // Without ports, should fall back to default port (3000)
        let info = detect_main_service(&devcontainer_dir, None).unwrap();
        assert_eq!(info.name, Some("web".to_string()));
        assert_eq!(info.port, 3000);
        assert_eq!(info.service_url, "http://web:3000");
    }

    #[test]
    fn test_detect_main_service_with_stack_hint() {
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
  app:
    build:
      context: ..
"#,
        )
        .unwrap();

        // With flask stack hint, should use port 5000
        let info = detect_main_service(&devcontainer_dir, Some("flask")).unwrap();
        assert_eq!(info.name, Some("app".to_string()));
        assert_eq!(info.port, 5000);
        assert_eq!(info.service_url, "http://app:5000");
    }

    #[test]
    fn test_detect_main_service_no_devcontainer() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        // Don't create the directory

        let info = detect_main_service(&devcontainer_dir, Some("flask")).unwrap();
        assert_eq!(info.name, Some("app".to_string()));
        assert_eq!(info.port, 5000);
        assert_eq!(info.service_url, "http://app:5000");
    }

    // ========================================================================
    // Coding Agent Mount Injection Tests
    // ========================================================================

    #[test]
    fn test_parse_dockerfile_user_simple() {
        let contents = "FROM ubuntu:22.04\nUSER vscode\n";
        assert_eq!(parse_dockerfile_user(contents), Some("vscode".to_string()));
    }

    #[test]
    fn test_parse_dockerfile_user_multiple() {
        let contents = "FROM ubuntu:22.04\nUSER root\nRUN apt-get update\nUSER myuser\n";
        assert_eq!(parse_dockerfile_user(contents), Some("myuser".to_string()));
    }

    #[test]
    fn test_parse_dockerfile_user_with_group() {
        let contents = "FROM ubuntu:22.04\nUSER developer:developers\n";
        assert_eq!(
            parse_dockerfile_user(contents),
            Some("developer".to_string())
        );
    }

    #[test]
    fn test_parse_dockerfile_user_none() {
        let contents = "FROM ubuntu:22.04\nRUN echo hello\n";
        assert_eq!(parse_dockerfile_user(contents), None);
    }

    #[test]
    fn test_parse_dockerfile_user_root() {
        let contents = "FROM ubuntu:22.04\nUSER root\n";
        assert_eq!(parse_dockerfile_user(contents), Some("root".to_string()));
    }

    #[test]
    fn test_parse_dockerfile_user_case_insensitive() {
        let contents = "FROM ubuntu:22.04\nuser myuser\n";
        assert_eq!(parse_dockerfile_user(contents), Some("myuser".to_string()));
    }

    #[test]
    fn test_parse_dockerfile_user_with_arg_variable() {
        // Skip ARG variables, return None
        let contents = "FROM ubuntu:22.04\nUSER ${USERNAME:-vscode}\n";
        assert_eq!(parse_dockerfile_user(contents), None);
    }

    #[test]
    fn test_parse_dockerfile_user_skips_comments() {
        let contents = "FROM ubuntu:22.04\n# USER commented_out\nUSER actual_user\n";
        assert_eq!(
            parse_dockerfile_user(contents),
            Some("actual_user".to_string())
        );
    }

    #[test]
    fn test_get_home_path_for_user_root() {
        assert_eq!(get_home_path_for_user("root"), "/root");
    }

    #[test]
    fn test_get_home_path_for_user_vscode() {
        assert_eq!(get_home_path_for_user("vscode"), "/home/vscode");
    }

    #[test]
    fn test_get_home_path_for_user_node() {
        assert_eq!(get_home_path_for_user("node"), "/home/node");
    }

    #[test]
    fn test_coding_agent_mount_entries() {
        let entries = coding_agent_mount_entries("/home/vscode");

        assert_eq!(entries.len(), 4);
        assert!(entries
            .iter()
            .any(|e| e.contains(".codex:/home/vscode/.codex")));
        assert!(entries
            .iter()
            .any(|e| e.contains(".claude:/home/vscode/.claude")));
        assert!(entries
            .iter()
            .any(|e| e.contains(".claude.json:/home/vscode/.claude.json")));
        assert!(entries
            .iter()
            .any(|e| e.contains(".gh:/home/vscode/.config/gh")));
    }

    #[test]
    fn test_coding_agent_mount_entries_root() {
        let entries = coding_agent_mount_entries("/root");

        assert!(entries.iter().any(|e| e.contains(".claude:/root/.claude")));
        assert!(entries.iter().any(|e| e.contains(".gh:/root/.config/gh")));
    }

    #[test]
    fn test_inject_coding_agent_mounts_no_devcontainer() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        // Don't create the directory

        let outcome = inject_coding_agent_mounts(&devcontainer_dir).unwrap();
        assert!(!outcome.compose_modified);
        assert!(outcome.changes.is_empty());
    }

    #[test]
    fn test_inject_coding_agent_mounts_adds_mounts() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("Dockerfile"),
            "FROM ubuntu:22.04\nUSER developer\n",
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    build: .
    volumes:
      - ../..:/workspaces:cached
"#,
        )
        .unwrap();

        let outcome = inject_coding_agent_mounts(&devcontainer_dir).unwrap();

        assert!(outcome.compose_modified);
        // 4 mount changes + 1 for creating .branchbox.env with SHARED_CONFIG_DIR
        assert_eq!(outcome.changes.len(), 5);
        assert_eq!(
            outcome.container_user.as_ref().unwrap().username,
            "developer"
        );

        let compose = std::fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap();
        assert!(compose.contains(".codex:/home/developer/.codex"));
        assert!(compose.contains(".claude:/home/developer/.claude"));
        assert!(compose.contains(".claude.json:/home/developer/.claude.json"));
        assert!(compose.contains(".gh:/home/developer/.config/gh"));

        // Verify .branchbox.env was created with SHARED_CONFIG_DIR
        let branchbox_env =
            std::fs::read_to_string(devcontainer_dir.join(".branchbox.env")).unwrap();
        assert!(branchbox_env.contains("SHARED_CONFIG_DIR=../.."));
    }

    #[test]
    fn test_inject_coding_agent_mounts_idempotent() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        // Already has the mounts
        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    build: .
    volumes:
      - ../..:/workspaces:cached
      - ${SHARED_CONFIG_DIR:-../..}/.codex:/home/vscode/.codex
      - ${SHARED_CONFIG_DIR:-../..}/.claude:/home/vscode/.claude
      - ${SHARED_CONFIG_DIR:-../..}/.claude.json:/home/vscode/.claude.json
      - ${SHARED_CONFIG_DIR:-../..}/.gh:/home/vscode/.config/gh
"#,
        )
        .unwrap();

        // Already has SHARED_CONFIG_DIR in .branchbox.env
        std::fs::write(
            devcontainer_dir.join(".branchbox.env"),
            "SHARED_CONFIG_DIR=../..\n",
        )
        .unwrap();

        let outcome = inject_coding_agent_mounts(&devcontainer_dir).unwrap();

        // Should not modify because mounts and env already exist
        assert!(!outcome.compose_modified);
        assert!(outcome.changes.is_empty());
    }

    #[test]
    fn test_inject_coding_agent_mounts_skips_infrastructure_services() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  postgres:
    image: postgres:15
  redis:
    image: redis:7
  app:
    build: .
    volumes:
      - ../..:/workspaces:cached
"#,
        )
        .unwrap();

        let outcome = inject_coding_agent_mounts(&devcontainer_dir).unwrap();

        // Should add to 'app' service, not postgres/redis
        assert!(outcome.compose_modified);

        let compose = std::fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap();
        // Verify mounts are in compose file (under app service)
        assert!(compose.contains(".codex:/home/vscode/.codex"));
    }

    #[test]
    fn test_inject_coding_agent_mounts_with_root_user() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test"}"#,
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("Dockerfile"),
            "FROM ubuntu:22.04\nUSER root\n",
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    build: .
"#,
        )
        .unwrap();

        let outcome = inject_coding_agent_mounts(&devcontainer_dir).unwrap();

        assert!(outcome.compose_modified);
        assert_eq!(outcome.container_user.as_ref().unwrap().home_path, "/root");

        let compose = std::fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap();
        assert!(compose.contains(".claude:/root/.claude"));
        assert!(compose.contains(".gh:/root/.config/gh"));
    }

    #[test]
    fn test_inject_coding_agent_mounts_no_compose_file() {
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

        let outcome = inject_coding_agent_mounts(&devcontainer_dir).unwrap();

        // Should not modify anything
        assert!(!outcome.compose_modified);
        assert!(outcome.changes.is_empty());
    }

    #[test]
    fn test_inject_coding_agent_mounts_detects_dockerfile_from_build_config() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test", "build": {"dockerfile": "Dockerfile.dev"}}"#,
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("Dockerfile.dev"),
            "FROM node:20\nUSER node\n",
        )
        .unwrap();

        std::fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    build: .
"#,
        )
        .unwrap();

        let outcome = inject_coding_agent_mounts(&devcontainer_dir).unwrap();

        assert!(outcome.compose_modified);
        assert_eq!(outcome.container_user.as_ref().unwrap().username, "node");

        let compose = std::fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap();
        assert!(compose.contains(".claude:/home/node/.claude"));
    }

    #[test]
    fn test_detect_container_user_defaults_to_vscode() {
        let temp = TempDir::new().unwrap();
        let devcontainer_dir = temp.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();

        let config = serde_json::json!({"name": "Test"});
        let user = detect_container_user(&devcontainer_dir, &config);

        assert_eq!(user.username, "vscode");
        assert_eq!(user.home_path, "/home/vscode");
    }

    #[test]
    fn test_container_user_default() {
        let user = ContainerUser::default();
        assert_eq!(user.username, "vscode");
        assert_eq!(user.home_path, "/home/vscode");
    }
}
