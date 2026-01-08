//! Builder pattern for devcontainer configuration
//!
//! Provides a fluent API for constructing devcontainer configurations
//! with sensible defaults and easy customization.

use super::config::*;
use super::detector::ProjectDetector;
use super::presets::{self, PresetOptions, StackPreset};
use std::path::Path;

/// Builder for constructing complete devcontainer configurations
///
/// # Example
///
/// ```no_run
/// use worktree_core::devcontainer::{DevcontainerBuilder, StackPreset};
/// use std::path::Path;
///
/// let builder = DevcontainerBuilder::new(Path::new("/path/to/project"))
///     .with_preset(StackPreset::Rails)
///     .with_project_name("my-awesome-app")
///     .with_ruby_version("3.2")
///     .with_database(true);
///
/// let (devcontainer, compose, dockerfile, env_sample) = builder.build()?;
/// # Ok::<(), worktree_core::Error>(())
/// ```
#[derive(Debug)]
pub struct DevcontainerBuilder {
    project_path: std::path::PathBuf,
    preset: Option<StackPreset>,
    project_name: Option<String>,
    options: PresetOptions,
    existing_config: Option<DevcontainerConfig>,
}

impl DevcontainerBuilder {
    /// Create a new builder for the given project path
    pub fn new(project_path: &Path) -> Self {
        Self {
            project_path: project_path.to_path_buf(),
            preset: None,
            project_name: None,
            options: PresetOptions::new(),
            existing_config: None,
        }
    }

    /// Set the stack preset to use
    pub fn with_preset(mut self, preset: StackPreset) -> Self {
        self.preset = Some(preset);
        self
    }

    /// Set the project name
    pub fn with_project_name(mut self, name: impl Into<String>) -> Self {
        self.project_name = Some(name.into());
        self
    }

    /// Set the Ruby version
    pub fn with_ruby_version(mut self, version: impl Into<String>) -> Self {
        self.options.ruby_version = Some(version.into());
        self
    }

    /// Set the Node.js version
    pub fn with_node_version(mut self, version: impl Into<String>) -> Self {
        self.options.node_version = Some(version.into());
        self
    }

    /// Set the Python version
    pub fn with_python_version(mut self, version: impl Into<String>) -> Self {
        self.options.python_version = Some(version.into());
        self
    }

    /// Set the application port
    pub fn with_port(mut self, port: u16) -> Self {
        self.options.port = Some(port);
        self
    }

    /// Enable/disable database configuration
    pub fn with_database(mut self, enabled: bool) -> Self {
        self.options.with_database = enabled;
        self
    }

    /// Set the database type
    pub fn with_database_type(mut self, db_type: impl Into<String>) -> Self {
        self.options.database_type = Some(db_type.into());
        self.options.with_database = true;
        self
    }

    /// Enable/disable AI coding agent mounts
    pub fn with_coding_agents(mut self, enabled: bool) -> Self {
        self.options.coding_agents = enabled;
        self
    }

    /// Add extra VS Code extensions
    pub fn with_extra_extensions(mut self, extensions: Vec<String>) -> Self {
        self.options.extra_extensions = extensions;
        self
    }

    /// Load and merge with an existing devcontainer.json
    pub fn with_existing_config(mut self, config: DevcontainerConfig) -> Self {
        self.existing_config = Some(config);
        self
    }

    /// Auto-detect project settings from the project path
    pub fn auto_detect(mut self) -> Self {
        let detector = ProjectDetector::new(&self.project_path);

        // Detect project name if not set
        if self.project_name.is_none() {
            self.project_name = detector.project_name();
        }

        // Detect versions from project files
        let detected = detector.detect_all();
        if self.options.ruby_version.is_none() {
            self.options.ruby_version = detected.ruby_version;
        }
        if self.options.node_version.is_none() {
            self.options.node_version = detected.node_version;
        }
        if self.options.python_version.is_none() {
            self.options.python_version = detected.python_version;
        }
        if self.options.port.is_none() {
            self.options.port = detected.default_port;
        }

        // Auto-detect preset if not set
        if self.preset.is_none() {
            self.preset = Some(self.detect_preset());
        }

        self
    }

    /// Detect the appropriate preset based on project files
    fn detect_preset(&self) -> StackPreset {
        if self.project_path.join("Cargo.toml").exists() {
            return StackPreset::Rust;
        }

        if self.project_path.join("Gemfile").exists() {
            if let Ok(content) = std::fs::read_to_string(self.project_path.join("Gemfile")) {
                if content.contains("gem \"rails\"") || content.contains("gem 'rails'") {
                    return StackPreset::Rails;
                }
            }
        }

        if self.project_path.join("package.json").exists() {
            return StackPreset::NodeJs;
        }

        if self.project_path.join("pyproject.toml").exists()
            || self.project_path.join("requirements.txt").exists()
            || self.project_path.join("setup.py").exists()
        {
            return StackPreset::Python;
        }

        StackPreset::Generic
    }

    /// Build all configuration files
    ///
    /// Returns a tuple of (devcontainer.json, compose.yaml, Dockerfile, .env.sample)
    pub fn build(self) -> crate::Result<BuildOutput> {
        let preset = self.preset.unwrap_or_else(|| self.detect_preset());
        let project_name = self
            .project_name
            .or_else(|| {
                self.project_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "project".to_string());

        // Build devcontainer.json
        let mut devcontainer =
            presets::devcontainer_config(preset, &project_name, &self.options);

        // Merge with existing config if provided
        if let Some(existing) = self.existing_config {
            devcontainer = merge_configs(existing, devcontainer);
        }

        // Build compose.yaml
        let compose = presets::compose_config(preset, &project_name, &self.options);

        // Build Dockerfile
        let dockerfile = presets::dockerfile_config(preset, &self.options);

        // Build .env.sample
        let env_sample = presets::env_sample(preset, &project_name, &self.options);

        Ok(BuildOutput {
            devcontainer,
            compose,
            dockerfile,
            env_sample,
            preset,
            project_name,
        })
    }
}

/// Output from the builder
#[derive(Debug)]
pub struct BuildOutput {
    /// devcontainer.json configuration
    pub devcontainer: DevcontainerConfig,

    /// compose.yaml configuration
    pub compose: ComposeConfig,

    /// Dockerfile configuration
    pub dockerfile: DockerfileConfig,

    /// .env.sample content
    pub env_sample: String,

    /// The preset that was used
    pub preset: StackPreset,

    /// The project name
    pub project_name: String,
}

impl BuildOutput {
    /// Write all files to the project directory
    pub fn write_to(&self, project_path: &Path) -> crate::Result<WriteResult> {
        use std::fs;

        let devcontainer_dir = project_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir)?;

        let mut written_files = Vec::new();

        // Write devcontainer.json
        let devcontainer_path = devcontainer_dir.join("devcontainer.json");
        let devcontainer_json = self.devcontainer.to_json_pretty().map_err(|e| {
            crate::Error::validation(format!("Failed to serialize devcontainer.json: {}", e))
        })?;
        fs::write(&devcontainer_path, &devcontainer_json)?;
        written_files.push(devcontainer_path);

        // Write compose.yaml
        let compose_path = devcontainer_dir.join("compose.yaml");
        let compose_yaml = self.compose.to_yaml().map_err(|e| {
            crate::Error::validation(format!("Failed to serialize compose.yaml: {}", e))
        })?;
        fs::write(&compose_path, &compose_yaml)?;
        written_files.push(compose_path);

        // Write Dockerfile
        let dockerfile_path = devcontainer_dir.join("Dockerfile");
        let dockerfile_content = self.dockerfile.to_dockerfile();
        fs::write(&dockerfile_path, &dockerfile_content)?;
        written_files.push(dockerfile_path);

        // Write .env.sample (only if it doesn't exist)
        let env_sample_path = project_path.join(".env.sample");
        let env_existed = env_sample_path.exists();
        if !env_existed {
            fs::write(&env_sample_path, &self.env_sample)?;
            written_files.push(env_sample_path);
        }

        Ok(WriteResult {
            files: written_files,
            env_sample_skipped: env_existed,
        })
    }
}

/// Result of writing configuration files
#[derive(Debug)]
pub struct WriteResult {
    /// Files that were written
    pub files: Vec<std::path::PathBuf>,

    /// Whether .env.sample was skipped (already existed)
    pub env_sample_skipped: bool,
}

/// Merge an existing configuration with a new one
///
/// The existing configuration takes precedence for most fields,
/// but features and extensions are merged.
fn merge_configs(existing: DevcontainerConfig, new: DevcontainerConfig) -> DevcontainerConfig {
    let mut merged = existing.clone();

    // Preserve existing name if set, otherwise use new
    if merged.name.is_empty() {
        merged.name = new.name;
    }

    // Merge features (add new ones that don't exist)
    if let Some(new_features) = new.features {
        let features = merged.features.get_or_insert_with(std::collections::BTreeMap::new);
        for (key, value) in new_features {
            features.entry(key).or_insert(value);
        }
    }

    // Merge forward ports
    if let Some(new_ports) = new.forward_ports {
        let ports = merged.forward_ports.get_or_insert_with(Vec::new);
        for port in new_ports {
            if !ports.iter().any(|p| matches!((p, &port), (PortConfig::Simple(a), PortConfig::Simple(b)) if a == b)) {
                ports.push(port);
            }
        }
    }

    // Update workspace settings if not set
    if merged.workspace_folder.is_none() {
        merged.workspace_folder = new.workspace_folder;
    }
    if merged.workspace_mount.is_none() {
        merged.workspace_mount = new.workspace_mount;
    }

    // Merge customizations/extensions
    if let Some(new_customizations) = new.customizations {
        let customizations = merged.customizations.get_or_insert_with(Customizations::default);
        if let Some(new_vscode) = new_customizations.vscode {
            let vscode = customizations.vscode.get_or_insert_with(VscodeCustomizations::default);
            if let Some(new_extensions) = new_vscode.extensions {
                let extensions = vscode.extensions.get_or_insert_with(Vec::new);
                for ext in new_extensions {
                    if !extensions.contains(&ext) {
                        extensions.push(ext);
                    }
                }
            }
        }
    }

    merged
}

/// Load an existing devcontainer.json if it exists
pub fn load_existing_config(project_path: &Path) -> Option<DevcontainerConfig> {
    let devcontainer_path = project_path.join(".devcontainer/devcontainer.json");
    if devcontainer_path.exists() {
        DevcontainerConfig::from_file(&devcontainer_path).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_builder_auto_detect_rails() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Gemfile"), "gem 'rails'\nruby '3.2.0'").unwrap();
        fs::write(temp.path().join(".ruby-version"), "3.2.2").unwrap();

        let builder = DevcontainerBuilder::new(temp.path()).auto_detect();
        let output = builder.build().unwrap();

        assert_eq!(output.preset, StackPreset::Rails);
        assert!(output.devcontainer.name.to_lowercase().contains("project"));
    }

    #[test]
    fn test_builder_auto_detect_nodejs() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("package.json"), r#"{"name": "my-node-app"}"#).unwrap();
        fs::write(temp.path().join(".nvmrc"), "20").unwrap();

        let builder = DevcontainerBuilder::new(temp.path()).auto_detect();
        let output = builder.build().unwrap();

        assert_eq!(output.preset, StackPreset::NodeJs);
        assert_eq!(output.project_name, "my-node-app");
    }

    #[test]
    fn test_builder_with_explicit_settings() {
        let temp = TempDir::new().unwrap();

        let builder = DevcontainerBuilder::new(temp.path())
            .with_preset(StackPreset::Rails)
            .with_project_name("custom-name")
            .with_ruby_version("3.3")
            .with_port(4000);

        let output = builder.build().unwrap();

        assert_eq!(output.preset, StackPreset::Rails);
        assert_eq!(output.project_name, "custom-name");
        assert!(output.dockerfile.to_dockerfile().contains("3.3"));
    }

    #[test]
    fn test_builder_write_to() {
        let temp = TempDir::new().unwrap();

        let output = DevcontainerBuilder::new(temp.path())
            .with_preset(StackPreset::Generic)
            .with_project_name("test-project")
            .build()
            .unwrap();

        let result = output.write_to(temp.path()).unwrap();

        assert!(temp.path().join(".devcontainer/devcontainer.json").exists());
        assert!(temp.path().join(".devcontainer/compose.yaml").exists());
        assert!(temp.path().join(".devcontainer/Dockerfile").exists());
        assert!(temp.path().join(".env.sample").exists());
        assert!(!result.env_sample_skipped);
    }

    #[test]
    fn test_builder_preserves_existing_env_sample() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(".env.sample"), "EXISTING=value").unwrap();

        let output = DevcontainerBuilder::new(temp.path())
            .with_preset(StackPreset::Generic)
            .build()
            .unwrap();

        let result = output.write_to(temp.path()).unwrap();

        assert!(result.env_sample_skipped);
        let content = fs::read_to_string(temp.path().join(".env.sample")).unwrap();
        assert!(content.contains("EXISTING=value"));
    }

    #[test]
    fn test_merge_configs() {
        let existing = DevcontainerConfig {
            name: "Existing Name".to_string(),
            features: Some(
                [("feature-a".to_string(), Feature::empty())]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        };

        let new = DevcontainerConfig {
            name: "New Name".to_string(),
            features: Some(
                [
                    ("feature-a".to_string(), Feature::with_option("new", "option")),
                    ("feature-b".to_string(), Feature::empty()),
                ]
                .into_iter()
                .collect(),
            ),
            workspace_folder: Some("/workspaces/${localWorkspaceFolderBasename}".to_string()),
            ..Default::default()
        };

        let merged = merge_configs(existing, new);

        // Existing name is preserved
        assert_eq!(merged.name, "Existing Name");

        // Existing feature is preserved (not overwritten)
        let features = merged.features.unwrap();
        assert!(matches!(features.get("feature-a"), Some(Feature::Empty)));

        // New feature is added
        assert!(features.contains_key("feature-b"));

        // New workspace folder is set
        assert!(merged.workspace_folder.is_some());
    }
}
