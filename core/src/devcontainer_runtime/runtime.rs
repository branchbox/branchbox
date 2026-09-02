//! Devcontainer runtime operations
//!
//! High-level operations for managing devcontainer lifecycle: up, down, exec, build.

use super::config::{DevcontainerConfig, DevcontainerType, LifecycleCommand, StringOrArray};
use super::docker::{ComposeExecOptions, ContainerState, Docker};
use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempPath;

const BRANCHBOX_CONTAINER_ENV_NAMES_DIGEST_LABEL: &str =
    "devcontainer.branchbox.container_env_names_sha256";

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

#[derive(Serialize)]
struct ComposeEnvironmentOverride<'a> {
    services: BTreeMap<&'a str, ComposeServiceEnvironment<'a>>,
}

#[derive(Serialize)]
struct ComposeServiceEnvironment<'a> {
    environment: &'a BTreeMap<String, String>,
    labels: BTreeMap<&'static str, String>,
}

fn container_environment_names_digest(environment: &BTreeMap<String, String>) -> String {
    let mut digest = Sha256::new();
    digest.update(b"branchbox-container-env-names-v1\0");
    digest.update((environment.len() as u64).to_be_bytes());
    for name in environment.keys() {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
    }
    format!("{:x}", digest.finalize())
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
        let workspace_folder = std::fs::canonicalize(workspace_folder).with_context(|| {
            format!(
                "Failed to resolve workspace: {}",
                workspace_folder.display()
            )
        })?;

        let (config, config_path) = DevcontainerConfig::load(&workspace_folder)?;

        Ok(Self {
            docker: Docker::new(),
            workspace_folder,
            config,
            config_path,
        })
    }

    /// Create a runtime with custom Docker paths
    ///
    /// If docker_path or compose_path are None, falls back to DOCKER_PATH
    /// and DOCKER_COMPOSE_PATH environment variables, then to defaults.
    pub fn with_docker(
        workspace_folder: &Path,
        docker_path: Option<String>,
        compose_path: Option<String>,
    ) -> Result<Self> {
        let mut runtime = Self::new(workspace_folder)?;

        // Only override if custom paths are provided; otherwise keep env-based defaults
        if docker_path.is_some() || compose_path.is_some() {
            let effective_docker = docker_path.unwrap_or_else(|| {
                std::env::var("DOCKER_PATH").unwrap_or_else(|_| "docker".to_string())
            });
            let effective_compose =
                compose_path.or_else(|| std::env::var("DOCKER_COMPOSE_PATH").ok());
            runtime.docker = Docker::with_paths(effective_docker, effective_compose);
        }

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

    /// Get the stable labels used to locate a devcontainer for this workspace. Mutable
    /// configuration bindings must not participate in discovery, or a mismatch would hide the
    /// existing container before validation can reject it.
    fn container_identity_labels(&self) -> Vec<(String, String)> {
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

    /// Get all labels installed when creating the devcontainer.
    fn container_labels(&self) -> Vec<(String, String)> {
        let environment = self.configured_container_environment();
        let mut labels = self.container_identity_labels();
        labels.push((
            BRANCHBOX_CONTAINER_ENV_NAMES_DIGEST_LABEL.to_string(),
            container_environment_names_digest(&environment),
        ));
        labels
    }

    /// Find existing container for this workspace
    pub fn find_container(&self) -> Result<Option<String>> {
        // First try standard devcontainer labels
        let labels = self.container_identity_labels();
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

    /// Expand devcontainer variables in a mount string
    fn expand_mount_variables(&self, input: &str) -> String {
        let workspace_name = self.workspace_name();
        let workspace_folder = self.workspace_folder.to_string_lossy();

        input
            .replace("${localWorkspaceFolder}", &workspace_folder)
            .replace("${localWorkspaceFolderBasename}", &workspace_name)
            .replace(
                "${containerWorkspaceFolder}",
                &self.config.effective_workspace_folder(&workspace_name),
            )
            .replace("${containerWorkspaceFolderBasename}", &workspace_name)
    }

    /// Return only environment declared by the devcontainer configuration. Deliberately avoid
    /// reading the supervisor's ambient environment: callers must opt in to additional remote
    /// values explicitly.
    fn configured_container_environment(&self) -> BTreeMap<String, String> {
        self.config
            .container_env
            .as_ref()
            .into_iter()
            .flat_map(|environment| environment.iter())
            .map(|(name, value)| (name.clone(), self.expand_mount_variables(value)))
            .collect()
    }

    /// Merge configured remote environment with one invocation's explicit overrides. The
    /// explicit CLI value wins, matching the devcontainer metadata merge contract.
    fn remote_environment(&self, explicit: &HashMap<String, String>) -> BTreeMap<String, String> {
        let mut environment: BTreeMap<String, String> = self
            .config
            .remote_env
            .as_ref()
            .into_iter()
            .flat_map(|environment| environment.iter())
            .map(|(name, value)| (name.clone(), self.expand_mount_variables(value)))
            .collect();
        environment.extend(
            explicit
                .iter()
                .map(|(name, value)| (name.clone(), self.expand_mount_variables(value))),
        );
        environment
    }

    /// Docker Compose has no `up --env` equivalent for service container environment. Add a
    /// private, invocation-local override file, just as the reference devcontainer CLI does.
    /// Dollar signs are doubled so Compose does not substitute values a second time.
    fn compose_container_environment_override(&self, service: &str) -> Result<TempPath> {
        let configured_environment = self.configured_container_environment();
        let environment: BTreeMap<String, String> = configured_environment
            .iter()
            .map(|(name, value)| (name.clone(), value.replace('$', "$$")))
            .collect();

        let services = BTreeMap::from([(
            service,
            ComposeServiceEnvironment {
                environment: &environment,
                labels: BTreeMap::from([(
                    BRANCHBOX_CONTAINER_ENV_NAMES_DIGEST_LABEL,
                    container_environment_names_digest(&configured_environment),
                )]),
            },
        )]);
        let document = ComposeEnvironmentOverride { services };
        let mut file = tempfile::Builder::new()
            .prefix("branchbox-devcontainer-environment-")
            .suffix(".yaml")
            .tempfile()
            .context("Failed to create private devcontainer environment override")?;
        serde_yaml::to_writer(file.as_file_mut(), &document)
            .context("Failed to serialize devcontainer environment override")?;
        file.as_file_mut()
            .flush()
            .context("Failed to flush devcontainer environment override")?;
        file.as_file()
            .sync_all()
            .context("Failed to persist devcontainer environment override")?;
        Ok(file.into_temp_path())
    }

    fn validate_existing_container_environment(
        &self,
        labels: &HashMap<String, String>,
        actual_environment: &HashMap<String, String>,
    ) -> Result<()> {
        let expected = self.configured_container_environment();
        let expected_names_digest = container_environment_names_digest(&expected);
        if labels.get(BRANCHBOX_CONTAINER_ENV_NAMES_DIGEST_LABEL) != Some(&expected_names_digest)
            || expected
                .iter()
                .any(|(name, value)| actual_environment.get(name) != Some(value))
        {
            anyhow::bail!(
                "Existing devcontainer no longer matches containerEnv; rerun with --remove-existing-container"
            );
        }
        Ok(())
    }

    /// Parse a mount string in format "source:target" or "source:target:options"
    fn parse_mount_string(mount: &str) -> Option<(String, String)> {
        // Handle Docker mount syntax: source=...,target=...,type=...
        if mount.contains("source=") && mount.contains("target=") {
            let mut source = None;
            let mut target = None;
            for part in mount.split(',') {
                if let Some(s) = part.strip_prefix("source=") {
                    source = Some(s.to_string());
                } else if let Some(t) = part.strip_prefix("target=") {
                    target = Some(t.to_string());
                }
            }
            if let (Some(s), Some(t)) = (source, target) {
                return Some((s, t));
            }
        }

        // Handle simple colon-separated format: source:target[:options]
        let parts: Vec<&str> = mount.split(':').collect();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }

        None
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
                self.validate_existing_container_environment(&info.labels, &info.environment)?;
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
        let service = self
            .config
            .service
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Docker Compose config requires 'service' field"))?;
        let mut compose_files = self.config.compose_files(&devcontainer_dir);
        let container_environment_override =
            self.compose_container_environment_override(service)?;
        compose_files.push(container_environment_override.to_path_buf());
        let compose_file_refs: Vec<&Path> = compose_files.iter().map(|p| p.as_path()).collect();
        let remote_environment = self.remote_environment(&options.remote_env);

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
            self.run_lifecycle_commands(&container_id, &remote_environment)?;
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

        // Setup mounts - use workspaceMount from config if provided, otherwise default
        let workspace_name = self.workspace_name();
        let workspace_folder_str = self.workspace_folder.to_string_lossy().to_string();
        let mut mount_strings: Vec<(String, String)> = Vec::new();

        if let Some(ref ws_mount) = self.config.workspace_mount {
            // Parse and expand the workspaceMount string
            let expanded = self.expand_mount_variables(ws_mount);
            if let Some((source, target)) = Self::parse_mount_string(&expanded) {
                mount_strings.push((source, target));
            }
        } else {
            // Default workspace mount
            mount_strings.push((
                workspace_folder_str.clone(),
                self.config.effective_workspace_folder(&workspace_name),
            ));
        }

        // Add additional mounts from config
        if let Some(ref config_mounts) = self.config.mounts {
            for mount in config_mounts {
                match mount {
                    super::config::MountConfig::String(s) => {
                        let expanded = self.expand_mount_variables(s);
                        if let Some((source, target)) = Self::parse_mount_string(&expanded) {
                            mount_strings.push((source, target));
                        }
                    }
                    super::config::MountConfig::Object { source, target, .. } => {
                        let expanded_target = self.expand_mount_variables(target);
                        let expanded_source = source
                            .as_ref()
                            .map(|s| self.expand_mount_variables(s))
                            .unwrap_or_default();
                        if !expanded_source.is_empty() {
                            mount_strings.push((expanded_source, expanded_target));
                        }
                    }
                }
            }
        }

        let mounts: Vec<(&str, &str)> = mount_strings
            .iter()
            .map(|(s, t)| (s.as_str(), t.as_str()))
            .collect();

        // `containerEnv` belongs to the container creation boundary. Remote environment is
        // applied only to lifecycle and later tool-spawned processes.
        let container_environment = self.configured_container_environment();
        let env: Vec<(&str, &str)> = container_environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let remote_environment = self.remote_environment(&options.remote_env);

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
            Some(
                &self
                    .config
                    .effective_workspace_folder(&self.workspace_name()),
            ),
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
            self.run_lifecycle_commands(&container_id, &remote_environment)?;
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

    fn run_lifecycle_commands(
        &self,
        container_id: &str,
        remote_environment: &BTreeMap<String, String>,
    ) -> Result<()> {
        let user = self.config.effective_remote_user();
        let workdir = Some(
            self.config
                .effective_workspace_folder(&self.workspace_name()),
        );
        let workdir_ref = workdir.as_deref();

        // Run commands in order
        if let Some(ref cmd) = self.config.on_create_command {
            tracing::info!("Running onCreateCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref, remote_environment)?;
        }

        if let Some(ref cmd) = self.config.update_content_command {
            tracing::info!("Running updateContentCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref, remote_environment)?;
        }

        if let Some(ref cmd) = self.config.post_create_command {
            tracing::info!("Running postCreateCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref, remote_environment)?;
        }

        if let Some(ref cmd) = self.config.post_start_command {
            tracing::info!("Running postStartCommand");
            self.run_lifecycle_command(container_id, cmd, user, workdir_ref, remote_environment)?;
        }

        Ok(())
    }

    fn run_lifecycle_command(
        &self,
        container_id: &str,
        command: &LifecycleCommand,
        user: Option<&str>,
        workdir: Option<&str>,
        remote_environment: &BTreeMap<String, String>,
    ) -> Result<()> {
        let environment: Vec<(&str, &str)> = remote_environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        match command {
            LifecycleCommand::String(s) => {
                let cmd = vec!["sh", "-c", s.as_str()];
                self.docker
                    .exec(container_id, &cmd, user, workdir, &environment, false)?;
            }
            LifecycleCommand::Array(arr) => {
                let cmd: Vec<&str> = arr.iter().map(|s| s.as_str()).collect();
                self.docker
                    .exec(container_id, &cmd, user, workdir, &environment, false)?;
            }
            LifecycleCommand::Object(map) => {
                for (name, cmd) in map {
                    tracing::debug!(name = %name, "Running lifecycle sub-command");
                    match cmd {
                        StringOrArray::String(s) => {
                            let cmd = vec!["sh", "-c", s.as_str()];
                            self.docker.exec(
                                container_id,
                                &cmd,
                                user,
                                workdir,
                                &environment,
                                false,
                            )?;
                        }
                        StringOrArray::Array(arr) => {
                            let cmd: Vec<&str> = arr.iter().map(|s| s.as_str()).collect();
                            self.docker.exec(
                                container_id,
                                &cmd,
                                user,
                                workdir,
                                &environment,
                                false,
                            )?;
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
            anyhow::bail!("Container is not running (state: {:?})", info.state);
        }

        let user = options
            .user
            .as_deref()
            .or_else(|| self.config.effective_remote_user());

        let default_workdir = self
            .config
            .effective_workspace_folder(&self.workspace_name());
        let workdir = options
            .workdir
            .as_deref()
            .or(Some(default_workdir.as_str()));
        let remote_environment = self.remote_environment(&options.remote_env);
        let environment: Vec<(&str, &str)> = remote_environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();

        // For compose-based containers, use compose exec
        if self.config.container_type() == DevcontainerType::DockerCompose {
            let devcontainer_dir = self.devcontainer_dir();
            let compose_files = self.config.compose_files(&devcontainer_dir);
            let compose_file_refs: Vec<&Path> = compose_files.iter().map(|p| p.as_path()).collect();

            let service = self.config.service.as_deref().unwrap_or("app");
            let project_name = self.workspace_name();

            let cmd_refs: Vec<&str> = command.iter().map(|s| s.as_str()).collect();

            let output = self.docker.compose_exec_with_options(
                &compose_file_refs,
                service,
                &cmd_refs,
                ComposeExecOptions {
                    project_name: Some(&project_name),
                    user,
                    workdir,
                    environment: &environment,
                },
            )?;

            return Ok(ExecResult {
                outcome: if output.success { "success" } else { "error" }.to_string(),
                exit_code: output.exit_code,
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }

        // For other container types, use docker exec directly
        let cmd_refs: Vec<&str> = command.iter().map(|s| s.as_str()).collect();

        let output =
            self.docker
                .exec(&container_id, &cmd_refs, user, workdir, &environment, false)?;

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

                let output =
                    self.docker
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

    #[test]
    fn configured_container_and_remote_environment_remain_separate() {
        let temp_dir = create_test_workspace();
        std::fs::write(
            temp_dir.path().join(".devcontainer/devcontainer.json"),
            r#"{
                "image": "example.invalid/dev:latest",
                "containerEnv": {
                    "CONTAINER_ONLY": "static",
                    "WORKSPACE_NAME": "${localWorkspaceFolderBasename}"
                },
                "remoteEnv": {
                    "CONFIG_ONLY": "configured",
                    "PRECEDENCE": "configuration"
                }
            }"#,
        )
        .unwrap();
        let runtime = DevcontainerRuntime::new(temp_dir.path()).unwrap();

        let container = runtime.configured_container_environment();
        assert_eq!(container.get("CONTAINER_ONLY").unwrap(), "static");
        assert_eq!(
            container.get("WORKSPACE_NAME").unwrap(),
            &runtime.workspace_name()
        );
        assert!(!container.contains_key("CONFIG_ONLY"));

        let names_digest = container_environment_names_digest(&container);
        let mut different_values = container.clone();
        different_values.insert(
            "CONTAINER_ONLY".to_string(),
            "different-low-entropy-secret".to_string(),
        );
        assert_eq!(
            container_environment_names_digest(&different_values),
            names_digest,
            "the public binding must not depend on containerEnv values"
        );
        let mut different_names = container.clone();
        different_names.insert("ADDED_NAME".to_string(), "static".to_string());
        assert_ne!(
            container_environment_names_digest(&different_names),
            names_digest
        );
        let labels: HashMap<_, _> = runtime.container_labels().into_iter().collect();
        assert_eq!(
            labels.get(BRANCHBOX_CONTAINER_ENV_NAMES_DIGEST_LABEL),
            Some(&names_digest)
        );
        assert!(!labels.contains_key("devcontainer.branchbox.container_env_sha256"));
        assert!(runtime
            .container_identity_labels()
            .iter()
            .all(|(name, _)| name != BRANCHBOX_CONTAINER_ENV_NAMES_DIGEST_LABEL));

        let explicit = HashMap::from([
            ("PRECEDENCE".to_string(), "explicit".to_string()),
            ("EXPLICIT_ONLY".to_string(), "per-command".to_string()),
        ]);
        let remote = runtime.remote_environment(&explicit);
        assert_eq!(remote.get("CONFIG_ONLY").unwrap(), "configured");
        assert_eq!(remote.get("PRECEDENCE").unwrap(), "explicit");
        assert_eq!(remote.get("EXPLICIT_ONLY").unwrap(), "per-command");
        assert!(!remote.contains_key("CONTAINER_ONLY"));
    }
}
