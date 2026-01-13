//! Devcontainer runtime operations
//!
//! High-level operations for managing devcontainer lifecycle: up, down, exec, build.

use super::config::{DevcontainerConfig, DevcontainerType, LifecycleCommand, StringOrArray};
use super::docker::{ContainerState, Docker};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Devcontainer runtime manager
pub struct DevcontainerRuntime {
    docker: Docker,
    workspace_folder: PathBuf,
    config: DevcontainerConfig,
    config_path: PathBuf,
}

/// Result of the `up` operation
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpResult {
    pub outcome: String,
    pub container_id: String,
    pub remote_user: Option<String>,
    pub remote_workspace_folder: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compose_project_name: Option<String>,
}

/// Result of the `exec` operation
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    pub outcome: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Result of the `build` operation
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub outcome: String,
    pub image_name: Option<String>,
}

/// Result of the `down` operation
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownResult {
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_containers: Option<Vec<String>>,
}

/// Result of reading configuration
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadConfigResult {
    pub workspace_folder: String,
    pub config_path: String,
    pub configuration: DevcontainerConfig,
    pub container_type: String,
}

/// Options for the `up` command
#[derive(Debug, Default)]
pub struct UpOptions {
    /// Remove existing container before starting
    pub remove_existing: bool,
    /// Build with no cache
    pub build_no_cache: bool,
    /// Skip post-create commands
    pub skip_post_create: bool,
    /// Additional environment variables
    pub remote_env: HashMap<String, String>,
}

/// Options for the `exec` command
#[derive(Debug, Default)]
pub struct ExecOptions {
    /// User to run command as
    pub user: Option<String>,
    /// Working directory
    pub workdir: Option<String>,
    /// Additional environment variables
    pub remote_env: HashMap<String, String>,
}

/// Options for the `down` command
#[derive(Debug, Default)]
pub struct DownOptions {
    /// Remove volumes
    pub volumes: bool,
    /// Remove orphan containers
    pub remove_orphans: bool,
}

impl DevcontainerRuntime {
    /// Create a new runtime for a workspace
    pub fn new(workspace_folder: &Path) -> Result<Self> {
        let workspace_folder = std::fs::canonicalize(workspace_folder)
            .with_context(|| format!("Failed to resolve workspace: {}", workspace_folder.display()))?;

        let (config, config_path) = DevcontainerConfig::load(&workspace_folder)?;

        Ok(Self {
            docker: Docker::new(),
            workspace_folder,
            config,
            config_path,
        })
    }

    /// Create a runtime with custom Docker paths
    pub fn with_docker(
        workspace_folder: &Path,
        docker_path: Option<String>,
        compose_path: Option<String>,
    ) -> Result<Self> {
        let mut runtime = Self::new(workspace_folder)?;
        runtime.docker = Docker::with_paths(
            docker_path.unwrap_or_else(|| "docker".to_string()),
            compose_path,
        );
        Ok(runtime)
    }

    /// Check if Docker is available
    pub fn is_docker_available(&self) -> bool {
        self.docker.is_available()
    }

    /// Read and return the configuration
    pub fn read_configuration(&self) -> ReadConfigResult {
        let container_type = match self.config.container_type() {
            DevcontainerType::Image => "image",
            DevcontainerType::Dockerfile => "dockerfile",
            DevcontainerType::DockerCompose => "dockerCompose",
        };

        ReadConfigResult {
            workspace_folder: self.workspace_folder.to_string_lossy().to_string(),
            config_path: self.config_path.to_string_lossy().to_string(),
            configuration: self.config.clone(),
            container_type: container_type.to_string(),
        }
    }

    /// Get the devcontainer labels for this workspace
    fn container_labels(&self) -> Vec<(String, String)> {
        vec![
            (
                "devcontainer.local_folder".to_string(),
                self.workspace_folder.to_string_lossy().to_string(),
            ),
            (
                "devcontainer.config_file".to_string(),
                self.config_path.to_string_lossy().to_string(),
            ),
        ]
    }

    /// Find existing container for this workspace
    pub fn find_container(&self) -> Result<Option<String>> {
        // First try standard devcontainer labels
        let labels = self.container_labels();
        let label_refs: Vec<(&str, &str)> = labels
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let containers = self.docker.find_containers(&label_refs)?;
        if let Some(container) = containers.into_iter().next() {
            return Ok(Some(container));
        }

        // For Docker Compose, also search by compose project and service
        if self.config.container_type() == DevcontainerType::DockerCompose {
            if let Some(service) = self.config.service.as_deref() {
                let project_name = self.workspace_name();
                let compose_labels = vec![
                    ("com.docker.compose.project", project_name.as_str()),
                    ("com.docker.compose.service", service),
                ];
                let containers = self.docker.find_containers(&compose_labels)?;
                return Ok(containers.into_iter().next());
            }
        }

        Ok(None)
    }

    /// Get workspace name (last component of path)
    fn workspace_name(&self) -> String {
        self.workspace_folder
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string())
    }

    /// Get the devcontainer directory
    fn devcontainer_dir(&self) -> PathBuf {
        self.config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.workspace_folder.join(".devcontainer"))
    }

    /// Create and start the devcontainer
    pub fn up(&self, options: UpOptions) -> Result<UpResult> {
        // Check for existing container
        if let Some(container_id) = self.find_container()? {
            if options.remove_existing {
                tracing::info!(container_id = %container_id, "Removing existing container");
                self.docker.remove_container(&container_id, true)?;
            } else {
                // Check if it's running
                let info = self.docker.inspect_container(&container_id)?;
                if info.state == ContainerState::Running {
                    tracing::info!(container_id = %container_id, "Container already running");
                    return Ok(UpResult {
                        outcome: "existing".to_string(),
                        container_id,
                        remote_user: self.config.effective_remote_user().map(|s| s.to_string()),
                        remote_workspace_folder: self
                            .config
                            .effective_workspace_folder(&self.workspace_name()),
                        compose_project_name: None,
                    });
                } else {
                    // Start the existing container
                    tracing::info!(container_id = %container_id, "Starting existing container");
                    self.docker.start_container(&container_id)?;
                    return Ok(UpResult {
                        outcome: "started".to_string(),
                        container_id,
                        remote_user: self.config.effective_remote_user().map(|s| s.to_string()),
                        remote_workspace_folder: self
                            .config
                            .effective_workspace_folder(&self.workspace_name()),
                        compose_project_name: None,
                    });
                }
            }
        }

        // Create new container based on type
        match self.config.container_type() {
            DevcontainerType::DockerCompose => self.up_compose(options),
            DevcontainerType::Dockerfile => self.up_dockerfile(options),
            DevcontainerType::Image => self.up_image(options),
        }
    }

    fn up_compose(&self, options: UpOptions) -> Result<UpResult> {
        let devcontainer_dir = self.devcontainer_dir();
        let compose_files = self.config.compose_files(&devcontainer_dir);
        let compose_file_refs: Vec<&Path> = compose_files.iter().map(|p| p.as_path()).collect();

        let service = self
            .config
            .service
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Docker Compose config requires 'service' field"))?;

        let project_name = self.workspace_name();

        tracing::info!(
            compose_files = ?compose_file_refs,
            service = %service,
            project = %project_name,
            "Starting devcontainer with Docker Compose"
        );

        // Run docker compose up
        let output = self.docker.compose_up(
            &compose_file_refs,
            Some(&project_name),
            Some(service),
            options.build_no_cache,
            true, // detach
        )?;

        if !output.success {
            anyhow::bail!("docker compose up failed: {}", output.stderr);
        }

        // Get the container ID
        let container_id = self
            .docker
            .compose_ps(&compose_file_refs, Some(&project_name), service)?
            .ok_or_else(|| anyhow::anyhow!("Failed to get container ID after compose up"))?;

        // Run lifecycle commands if not skipped
        if !options.skip_post_create {
            self.run_lifecycle_commands(&container_id)?;
        }

        Ok(UpResult {
            outcome: "created".to_string(),
            container_id,
            remote_user: self.config.effective_remote_user().map(|s| s.to_string()),
            remote_workspace_folder: self
                .config
                .effective_workspace_folder(&self.workspace_name()),
            compose_project_name: Some(project_name),
        })
    }

    fn up_dockerfile(&self, options: UpOptions) -> Result<UpResult> {
        let devcontainer_dir = self.devcontainer_dir();
        let dockerfile = self
            .config
            .dockerfile_path(&devcontainer_dir)
            .ok_or_else(|| anyhow::anyhow!("No Dockerfile specified in build config"))?;
        let context = self
            .config
            .build_context(&devcontainer_dir)
            .unwrap_or_else(|| devcontainer_dir.clone());

        let image_name = format!(
            "devcontainer-{}:latest",
            self.workspace_name().to_lowercase().replace(' ', "-")
        );

        tracing::info!(
            dockerfile = %dockerfile.display(),
            context = %context.display(),
            image = %image_name,
            "Building devcontainer image"
        );

        // Build the image
        let build_args = self
            .config
            .build
            .as_ref()
            .and_then(|b| b.args.clone())
            .unwrap_or_default();

        let output = self.docker.build(
            &context,
            &dockerfile,
            &image_name,
            &build_args,
            options.build_no_cache,
        )?;

        if !output.success {
            anyhow::bail!("docker build failed: {}", output.stderr);
        }

        // Now run the container
        self.run_container(&image_name, options)
    }

    fn up_image(&self, options: UpOptions) -> Result<UpResult> {
        let image = self
            .config
            .effective_image()
            .ok_or_else(|| anyhow::anyhow!("No image specified in devcontainer.json"))?;

        tracing::info!(image = %image, "Starting devcontainer from image");

        self.run_container(image, options)
    }

    fn run_container(&self, image: &str, options: UpOptions) -> Result<UpResult> {
        let container_name = format!(
            "devcontainer-{}",
            self.workspace_name().to_lowercase().replace(' ', "-")
        );

        let labels = self.container_labels();
        let label_refs: Vec<(&str, &str)> = labels
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        // Setup mounts
        let workspace_mount = (
            self.workspace_folder.to_string_lossy().to_string(),
            self.config
                .effective_workspace_folder(&self.workspace_name()),
        );
        let mounts = vec![(workspace_mount.0.as_str(), workspace_mount.1.as_str())];

        // Setup environment
        let mut env: Vec<(&str, &str)> = Vec::new();
        for (k, v) in &options.remote_env {
            env.push((k.as_str(), v.as_str()));
        }

        // Determine command
        let command: Option<Vec<&str>> = if self.config.override_command.unwrap_or(true) {
            Some(vec!["sleep", "infinity"])
        } else {
            None
        };

        let output = self.docker.run(
            image,
            Some(&container_name),
            &label_refs,
            &mounts,
            &[], // ports - handled by forward_ports later
            &env,
            Some(&self.config.effective_workspace_folder(&self.workspace_name())),
            command.as_deref(),
            true,  // detach
            false, // don't auto-remove
        )?;

        if !output.success {
            anyhow::bail!("docker run failed: {}", output.stderr);
        }

        // Get container ID from output (docker run -d prints the ID)
        let container_id = output.stdout.trim().to_string();

        // Run lifecycle commands if not skipped
        if !options.skip_post_create {
            self.run_lifecycle_commands(&container_id)?;
        }

        Ok(UpResult {
            outcome: "created".to_string(),
            container_id,
            remote_user: self.config.effective_remote_user().map(|s| s.to_string()),
            remote_workspace_folder: self
                .config
                .effective_workspace_folder(&self.workspace_name()),
            compose_project_name: None,
        })
    }

    fn run_lifecycle_commands(&self, container_id: &str) -> Result<()> {
        let user = self.config.effective_remote_user();
        let workdir = Some(self.config.effective_workspace_folder(&self.workspace_name()));
        let workdir_ref = workdir.as_deref();

        // Run commands in order
        if let Some(ref cmd) = self.config.on_create_command {
            tracing::info!("Running onCreateCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref)?;
        }

        if let Some(ref cmd) = self.config.update_content_command {
            tracing::info!("Running updateContentCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref)?;
        }

        if let Some(ref cmd) = self.config.post_create_command {
            tracing::info!("Running postCreateCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref)?;
        }

        if let Some(ref cmd) = self.config.post_start_command {
            tracing::info!("Running postStartCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref)?;
        }

        Ok(())
    }

    fn run_lifecycle_command(
        &self,
        container_id: &str,
        command: &LifecycleCommand,
        user: Option<&str>,
        workdir: Option<&str>,
    ) -> Result<()> {
        match command {
            LifecycleCommand::String(s) => {
                let cmd = vec!["sh", "-c", s.as_str()];
                self.docker
                    .exec(container_id, &cmd, user, workdir, &[], false)?;
            }
            LifecycleCommand::Array(arr) => {
                let cmd: Vec<&str> = arr.iter().map(|s| s.as_str()).collect();
                self.docker
                    .exec(container_id, &cmd, user, workdir, &[], false)?;
            }
            LifecycleCommand::Object(map) => {
                for (name, cmd) in map {
                    tracing::debug!(name = %name, "Running lifecycle sub-command");
                    match cmd {
                        StringOrArray::String(s) => {
                            let cmd = vec!["sh", "-c", s.as_str()];
                            self.docker
                                .exec(container_id, &cmd, user, workdir, &[], false)?;
                        }
                        StringOrArray::Array(arr) => {
                            let cmd: Vec<&str> = arr.iter().map(|s| s.as_str()).collect();
                            self.docker
                                .exec(container_id, &cmd, user, workdir, &[], false)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Execute a command in the running devcontainer
    pub fn exec(&self, command: &[String], options: ExecOptions) -> Result<ExecResult> {
        let container_id = self
            .find_container()?
            .ok_or_else(|| anyhow::anyhow!("No devcontainer found for this workspace"))?;

        // Verify container is running
        let info = self.docker.inspect_container(&container_id)?;
        if info.state != ContainerState::Running {
            anyhow::bail!(
                "Container is not running (state: {:?})",
                info.state
            );
        }

        let user = options
            .user
            .as_deref()
            .or_else(|| self.config.effective_remote_user());

        let default_workdir = self.config.effective_workspace_folder(&self.workspace_name());
        let workdir = options.workdir.as_deref().or(Some(default_workdir.as_str()));

        // For compose-based containers, use compose exec
        if self.config.container_type() == DevcontainerType::DockerCompose {
            let devcontainer_dir = self.devcontainer_dir();
            let compose_files = self.config.compose_files(&devcontainer_dir);
            let compose_file_refs: Vec<&Path> = compose_files.iter().map(|p| p.as_path()).collect();

            let service = self.config.service.as_deref().unwrap_or("app");
            let project_name = self.workspace_name();

            let cmd_refs: Vec<&str> = command.iter().map(|s| s.as_str()).collect();

            let output = self.docker.compose_exec(
                &compose_file_refs,
                Some(&project_name),
                service,
                &cmd_refs,
                user,
                workdir,
            )?;

            return Ok(ExecResult {
                outcome: if output.success { "success" } else { "error" }.to_string(),
                exit_code: output.exit_code,
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }

        // For other container types, use docker exec directly
        let env: Vec<(&str, &str)> = options
            .remote_env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let cmd_refs: Vec<&str> = command.iter().map(|s| s.as_str()).collect();

        let output = self
            .docker
            .exec(&container_id, &cmd_refs, user, workdir, &env, false)?;

        Ok(ExecResult {
            outcome: if output.success { "success" } else { "error" }.to_string(),
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// Build the devcontainer image
    pub fn build(&self, no_cache: bool, image_name: Option<String>) -> Result<BuildResult> {
        match self.config.container_type() {
            DevcontainerType::DockerCompose => {
                let devcontainer_dir = self.devcontainer_dir();
                let compose_files = self.config.compose_files(&devcontainer_dir);
                let compose_file_refs: Vec<&Path> =
                    compose_files.iter().map(|p| p.as_path()).collect();

                let service = self.config.service.as_deref();
                let project_name = self.workspace_name();

                tracing::info!(
                    compose_files = ?compose_file_refs,
                    service = ?service,
                    "Building with Docker Compose"
                );

                let output = self.docker.compose_build(
                    &compose_file_refs,
                    Some(&project_name),
                    service,
                    no_cache,
                )?;

                if !output.success {
                    anyhow::bail!("docker compose build failed: {}", output.stderr);
                }

                Ok(BuildResult {
                    outcome: "success".to_string(),
                    image_name: None,
                })
            }
            DevcontainerType::Dockerfile => {
                let devcontainer_dir = self.devcontainer_dir();
                let dockerfile = self
                    .config
                    .dockerfile_path(&devcontainer_dir)
                    .ok_or_else(|| anyhow::anyhow!("No Dockerfile specified"))?;
                let context = self
                    .config
                    .build_context(&devcontainer_dir)
                    .unwrap_or_else(|| devcontainer_dir.clone());

                let image = image_name.unwrap_or_else(|| {
                    format!(
                        "devcontainer-{}:latest",
                        self.workspace_name().to_lowercase().replace(' ', "-")
                    )
                });

                let build_args = self
                    .config
                    .build
                    .as_ref()
                    .and_then(|b| b.args.clone())
                    .unwrap_or_default();

                tracing::info!(
                    dockerfile = %dockerfile.display(),
                    context = %context.display(),
                    image = %image,
                    "Building Dockerfile"
                );

                let output = self
                    .docker
                    .build(&context, &dockerfile, &image, &build_args, no_cache)?;

                if !output.success {
                    anyhow::bail!("docker build failed: {}", output.stderr);
                }

                Ok(BuildResult {
                    outcome: "success".to_string(),
                    image_name: Some(image),
                })
            }
            DevcontainerType::Image => {
                // Nothing to build for image-based containers
                Ok(BuildResult {
                    outcome: "skipped".to_string(),
                    image_name: self.config.image.clone(),
                })
            }
        }
    }

    /// Stop and optionally remove the devcontainer
    pub fn down(&self, options: DownOptions) -> Result<DownResult> {
        match self.config.container_type() {
            DevcontainerType::DockerCompose => {
                let devcontainer_dir = self.devcontainer_dir();
                let compose_files = self.config.compose_files(&devcontainer_dir);
                let compose_file_refs: Vec<&Path> =
                    compose_files.iter().map(|p| p.as_path()).collect();

                let project_name = self.workspace_name();

                tracing::info!(
                    compose_files = ?compose_file_refs,
                    project = %project_name,
                    "Stopping devcontainer with Docker Compose"
                );

                let output = self.docker.compose_down(
                    &compose_file_refs,
                    Some(&project_name),
                    options.volumes,
                    options.remove_orphans,
                )?;

                if !output.success {
                    anyhow::bail!("docker compose down failed: {}", output.stderr);
                }

                Ok(DownResult {
                    outcome: "stopped".to_string(),
                    removed_containers: None,
                })
            }
            _ => {
                // For image/dockerfile-based containers
                let container_id = self.find_container()?;

                if let Some(id) = container_id {
                    tracing::info!(container_id = %id, "Stopping and removing container");

                    self.docker.stop_container(&id)?;
                    self.docker.remove_container(&id, true)?;

                    Ok(DownResult {
                        outcome: "removed".to_string(),
                        removed_containers: Some(vec![id]),
                    })
                } else {
                    Ok(DownResult {
                        outcome: "not_found".to_string(),
                        removed_containers: None,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let devcontainer_dir = temp_dir.path().join(".devcontainer");
        std::fs::create_dir_all(&devcontainer_dir).unwrap();
        std::fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"name": "Test", "image": "ubuntu:latest"}"#,
        )
        .unwrap();
        temp_dir
    }

    #[test]
    fn test_runtime_new() {
        let temp_dir = create_test_workspace();
        let runtime = DevcontainerRuntime::new(temp_dir.path());
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_read_configuration() {
        let temp_dir = create_test_workspace();
        let runtime = DevcontainerRuntime::new(temp_dir.path()).unwrap();
        let config = runtime.read_configuration();

        assert_eq!(config.container_type, "image");
        assert!(config.config_path.ends_with("devcontainer.json"));
    }

    #[test]
    fn test_workspace_name() {
        let temp_dir = create_test_workspace();
        let runtime = DevcontainerRuntime::new(temp_dir.path()).unwrap();

        // The workspace name should be the temp dir name
        let name = runtime.workspace_name();
        assert!(!name.is_empty());
    }
}
