//! Devcontainer configuration parsing
//!
//! Parses devcontainer.json files and resolves configuration for container operations.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parsed devcontainer.json configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DevcontainerConfig {
    /// Container name
    #[serde(default)]
    pub name: Option<String>,

    // === Image-based configuration ===
    /// Docker image to use
    #[serde(default)]
    pub image: Option<String>,

    // === Dockerfile-based configuration ===
    /// Build configuration
    #[serde(default)]
    pub build: Option<BuildConfig>,

    // === Docker Compose configuration ===
    /// Docker Compose file(s)
    #[serde(default)]
    pub docker_compose_file: Option<ComposeFileRef>,

    /// Service to use in Docker Compose
    #[serde(default)]
    pub service: Option<String>,

    /// Run services in Docker Compose (defaults to all)
    #[serde(default)]
    pub run_services: Option<Vec<String>>,

    /// Action when VS Code closes (none, stopContainer, stopCompose)
    #[serde(default)]
    pub shutdown_action: Option<String>,

    // === Container configuration ===
    /// Container user
    #[serde(default)]
    pub container_user: Option<String>,

    /// Remote user (user inside container)
    #[serde(default)]
    pub remote_user: Option<String>,

    /// Working directory inside container
    #[serde(default)]
    pub workspace_folder: Option<String>,

    /// Workspace mount configuration
    #[serde(default)]
    pub workspace_mount: Option<String>,

    /// Additional mounts
    #[serde(default)]
    pub mounts: Option<Vec<MountConfig>>,

    /// Container environment variables
    #[serde(default)]
    pub container_env: Option<HashMap<String, String>>,

    /// Remote environment variables
    #[serde(default)]
    pub remote_env: Option<HashMap<String, String>>,

    /// Ports to forward
    #[serde(default)]
    pub forward_ports: Option<Vec<PortForward>>,

    /// Override command
    #[serde(default)]
    pub override_command: Option<bool>,

    /// Run arguments for docker run
    #[serde(default)]
    pub run_args: Option<Vec<String>>,

    // === Lifecycle commands ===
    /// Command to run when container is created
    #[serde(default)]
    pub on_create_command: Option<LifecycleCommand>,

    /// Command to run when content is updated
    #[serde(default)]
    pub update_content_command: Option<LifecycleCommand>,

    /// Command to run after container is created
    #[serde(default)]
    pub post_create_command: Option<LifecycleCommand>,

    /// Command to run after container starts
    #[serde(default)]
    pub post_start_command: Option<LifecycleCommand>,

    /// Command to run when attaching to container
    #[serde(default)]
    pub post_attach_command: Option<LifecycleCommand>,

    /// Which lifecycle hook to wait for before considering ready
    #[serde(default)]
    pub wait_for: Option<String>,

    // === Features ===
    /// Dev container features to install
    #[serde(default)]
    pub features: Option<HashMap<String, serde_json::Value>>,

    // === Customizations ===
    /// Tool-specific customizations
    #[serde(default)]
    pub customizations: Option<HashMap<String, serde_json::Value>>,

    /// Init process
    #[serde(default)]
    pub init: Option<bool>,

    /// Privileged mode
    #[serde(default)]
    pub privileged: Option<bool>,

    /// Security options
    #[serde(default)]
    pub security_opt: Option<Vec<String>>,

    /// Cap add
    #[serde(default)]
    pub cap_add: Option<Vec<String>>,

    /// User env probe
    #[serde(default)]
    pub user_env_probe: Option<String>,
}

/// Build configuration for Dockerfile-based containers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BuildConfig {
    /// Dockerfile path (relative to .devcontainer)
    #[serde(default)]
    pub dockerfile: Option<String>,

    /// Build context path
    #[serde(default)]
    pub context: Option<String>,

    /// Build arguments
    #[serde(default)]
    pub args: Option<HashMap<String, String>>,

    /// Target stage
    #[serde(default)]
    pub target: Option<String>,

    /// Cache from images
    #[serde(default)]
    pub cache_from: Option<Vec<String>>,
}

/// Docker Compose file reference - can be string or array
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComposeFileRef {
    Single(String),
    Multiple(Vec<String>),
}

impl ComposeFileRef {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            ComposeFileRef::Single(s) => vec![s.clone()],
            ComposeFileRef::Multiple(v) => v.clone(),
        }
    }
}

/// Mount configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MountConfig {
    String(String),
    Object {
        #[serde(rename = "type")]
        mount_type: Option<String>,
        source: Option<String>,
        target: String,
    },
}

/// Port forwarding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PortForward {
    Number(u16),
    String(String),
    Object {
        #[serde(rename = "containerPort")]
        container_port: u16,
        #[serde(rename = "hostPort")]
        host_port: Option<u16>,
        label: Option<String>,
    },
}

/// Lifecycle command - can be string, array, or object
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LifecycleCommand {
    String(String),
    Array(Vec<String>),
    Object(HashMap<String, StringOrArray>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrArray {
    String(String),
    Array(Vec<String>),
}

/// Type of devcontainer setup
#[derive(Debug, Clone, PartialEq)]
pub enum DevcontainerType {
    /// Uses a pre-built Docker image
    Image,
    /// Uses a Dockerfile to build
    Dockerfile,
    /// Uses Docker Compose
    DockerCompose,
}

impl DevcontainerConfig {
    /// Load configuration from a workspace folder
    pub fn load(workspace_folder: &Path) -> Result<(Self, PathBuf)> {
        let devcontainer_dir = workspace_folder.join(".devcontainer");
        let config_path = if devcontainer_dir.join("devcontainer.json").exists() {
            devcontainer_dir.join("devcontainer.json")
        } else if workspace_folder.join(".devcontainer.json").exists() {
            workspace_folder.join(".devcontainer.json")
        } else {
            anyhow::bail!(
                "No devcontainer.json found in {} or {}",
                devcontainer_dir.display(),
                workspace_folder.display()
            );
        };

        Self::load_from_path(&config_path)
    }

    /// Load configuration from a specific path
    pub fn load_from_path(config_path: &Path) -> Result<(Self, PathBuf)> {
        let contents = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;

        // Parse JSONC (JSON with comments)
        let config: DevcontainerConfig = jsonc_parser::parse_to_serde_value(&contents, &Default::default())
            .map_err(|e| anyhow::anyhow!("Failed to parse JSONC: {:?}", e))?
            .map(|v| serde_json::from_value(v))
            .transpose()
            .with_context(|| format!("Failed to deserialize {}", config_path.display()))?
            .unwrap_or_default();

        Ok((config, config_path.to_path_buf()))
    }

    /// Determine the type of devcontainer configuration
    pub fn container_type(&self) -> DevcontainerType {
        if self.docker_compose_file.is_some() {
            DevcontainerType::DockerCompose
        } else if self.build.is_some() {
            DevcontainerType::Dockerfile
        } else {
            DevcontainerType::Image
        }
    }

    /// Get the effective image name for image-based containers
    pub fn effective_image(&self) -> Option<&str> {
        self.image.as_deref()
    }

    /// Get the Dockerfile path for Dockerfile-based containers
    pub fn dockerfile_path(&self, devcontainer_dir: &Path) -> Option<PathBuf> {
        self.build.as_ref().and_then(|b| {
            b.dockerfile.as_ref().map(|df| {
                let context = b.context.as_deref().unwrap_or(".");
                devcontainer_dir.join(context).join(df)
            })
        })
    }

    /// Get the build context for Dockerfile-based containers
    pub fn build_context(&self, devcontainer_dir: &Path) -> Option<PathBuf> {
        self.build.as_ref().map(|b| {
            let context = b.context.as_deref().unwrap_or(".");
            devcontainer_dir.join(context)
        })
    }

    /// Get Docker Compose files as resolved paths
    pub fn compose_files(&self, devcontainer_dir: &Path) -> Vec<PathBuf> {
        self.docker_compose_file
            .as_ref()
            .map(|cf| {
                cf.to_vec()
                    .into_iter()
                    .map(|f| devcontainer_dir.join(&f))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get the effective remote user
    pub fn effective_remote_user(&self) -> Option<&str> {
        self.remote_user
            .as_deref()
            .or(self.container_user.as_deref())
    }

    /// Get the effective workspace folder inside the container
    pub fn effective_workspace_folder(&self, workspace_name: &str) -> String {
        let folder = self
            .workspace_folder
            .clone()
            .unwrap_or_else(|| format!("/workspaces/{}", workspace_name));

        // Expand common devcontainer variables
        folder
            .replace("${localWorkspaceFolderBasename}", workspace_name)
            .replace("${containerWorkspaceFolderBasename}", workspace_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_image_config() {
        let json = r#"{
            "name": "Test",
            "image": "mcr.microsoft.com/devcontainers/rust:1"
        }"#;

        let config: DevcontainerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, Some("Test".to_string()));
        assert_eq!(config.image, Some("mcr.microsoft.com/devcontainers/rust:1".to_string()));
        assert_eq!(config.container_type(), DevcontainerType::Image);
    }

    #[test]
    fn test_parse_dockerfile_config() {
        let json = r#"{
            "name": "Test",
            "build": {
                "dockerfile": "Dockerfile",
                "context": ".."
            }
        }"#;

        let config: DevcontainerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.container_type(), DevcontainerType::Dockerfile);
    }

    #[test]
    fn test_parse_compose_config() {
        let json = r#"{
            "name": "Test",
            "dockerComposeFile": ["docker-compose.yml", "docker-compose.dev.yml"],
            "service": "app"
        }"#;

        let config: DevcontainerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.container_type(), DevcontainerType::DockerCompose);
        assert_eq!(config.service, Some("app".to_string()));
    }

    #[test]
    fn test_parse_lifecycle_commands() {
        let json = r#"{
            "postCreateCommand": "npm install",
            "postStartCommand": ["npm", "run", "dev"]
        }"#;

        let config: DevcontainerConfig = serde_json::from_str(json).unwrap();
        assert!(config.post_create_command.is_some());
        assert!(config.post_start_command.is_some());
    }

    #[test]
    fn test_load_from_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let devcontainer_dir = temp_dir.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();
        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test", "image": "ubuntu"}"#,
        )
        .unwrap();

        let (config, path) = DevcontainerConfig::load(temp_dir.path()).unwrap();
        assert_eq!(config.name, Some("Test".to_string()));
        assert!(path.ends_with("devcontainer.json"));
    }
}
