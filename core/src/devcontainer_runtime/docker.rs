//! Docker and Docker Compose command wrappers
//!
//! Provides safe wrappers around Docker CLI commands for container management.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Docker CLI wrapper
pub struct Docker {
    /// Path to docker binary
    docker_path: String,
    /// Path to docker compose binary (or "docker compose")
    compose_path: Option<String>,
}

/// Container information from docker inspect
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub state: ContainerState,
    pub labels: HashMap<String, String>,
    pub environment: HashMap<String, String>,
}

/// Container state
#[derive(Debug, Clone, PartialEq)]
pub enum ContainerState {
    Running,
    Exited,
    Paused,
    Restarting,
    Dead,
    Created,
    Unknown(String),
}

impl From<&str> for ContainerState {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "running" => ContainerState::Running,
            "exited" => ContainerState::Exited,
            "paused" => ContainerState::Paused,
            "restarting" => ContainerState::Restarting,
            "dead" => ContainerState::Dead,
            "created" => ContainerState::Created,
            other => ContainerState::Unknown(other.to_string()),
        }
    }
}

/// Output from docker commands
#[derive(Debug)]
pub struct DockerOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub(crate) struct ComposeExecOptions<'a> {
    pub project_name: Option<&'a str>,
    pub user: Option<&'a str>,
    pub workdir: Option<&'a str>,
    pub environment: &'a [(&'a str, &'a str)],
}

impl Docker {
    /// Create a new Docker wrapper with default paths
    pub fn new() -> Self {
        Self {
            docker_path: std::env::var("DOCKER_PATH").unwrap_or_else(|_| "docker".to_string()),
            compose_path: std::env::var("DOCKER_COMPOSE_PATH").ok(),
        }
    }

    /// Create a Docker wrapper with custom paths
    pub fn with_paths(docker_path: impl Into<String>, compose_path: Option<String>) -> Self {
        Self {
            docker_path: docker_path.into(),
            compose_path,
        }
    }

    /// Check if Docker is available
    pub fn is_available(&self) -> bool {
        Command::new(&self.docker_path)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Find containers by labels
    pub fn find_containers(&self, labels: &[(&str, &str)]) -> Result<Vec<String>> {
        let mut cmd = Command::new(&self.docker_path);
        cmd.args(["ps", "-q", "-a"]);

        for (key, value) in labels {
            cmd.arg("--filter").arg(format!("label={}={}", key, value));
        }

        let output = cmd.output().context("Failed to run docker ps")?;
        if !output.status.success() {
            anyhow::bail!(
                "docker ps failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let ids: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        Ok(ids)
    }

    /// Get container info by ID
    pub fn inspect_container(&self, container_id: &str) -> Result<ContainerInfo> {
        let output = Command::new(&self.docker_path)
            .args(["inspect", "--format", "{{json .}}", container_id])
            .output()
            .context("Failed to run docker inspect")?;

        if !output.status.success() {
            anyhow::bail!(
                "docker inspect failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("Failed to parse docker inspect output")?;

        let id = json["Id"].as_str().unwrap_or(container_id).to_string();

        let name = json["Name"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string();

        let state_str = json["State"]["Status"].as_str().unwrap_or("unknown");

        let labels: HashMap<String, String> = json["Config"]["Labels"]
            .as_object()
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let environment: HashMap<String, String> = json["Config"]["Env"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .filter_map(|entry| {
                entry
                    .split_once('=')
                    .map(|(name, value)| (name.to_string(), value.to_string()))
            })
            .collect();

        Ok(ContainerInfo {
            id,
            name,
            state: ContainerState::from(state_str),
            labels,
            environment,
        })
    }

    /// Start a container
    pub fn start_container(&self, container_id: &str) -> Result<DockerOutput> {
        self.run_command(&["start".to_string(), container_id.to_string()])
    }

    /// Stop a container
    pub fn stop_container(&self, container_id: &str) -> Result<DockerOutput> {
        self.run_command(&["stop".to_string(), container_id.to_string()])
    }

    /// Remove a container
    pub fn remove_container(&self, container_id: &str, force: bool) -> Result<DockerOutput> {
        let mut args = vec!["rm".to_string()];
        if force {
            args.push("-f".to_string());
        }
        args.push(container_id.to_string());
        self.run_command(&args)
    }

    /// Execute a command in a running container
    pub fn exec(
        &self,
        container_id: &str,
        command: &[&str],
        user: Option<&str>,
        workdir: Option<&str>,
        env: &[(&str, &str)],
        interactive: bool,
    ) -> Result<DockerOutput> {
        let mut args: Vec<String> = vec!["exec".to_string()];

        if interactive {
            args.push("-it".to_string());
        }

        if let Some(u) = user {
            args.push("-u".to_string());
            args.push(u.to_string());
        }

        if let Some(w) = workdir {
            args.push("-w".to_string());
            args.push(w.to_string());
        }

        for (key, value) in env {
            args.push("-e".to_string());
            args.push(format!("{}={}", key, value));
        }

        args.push(container_id.to_string());
        args.extend(command.iter().map(|s| s.to_string()));

        self.run_command(&args)
    }

    /// Build an image from a Dockerfile
    pub fn build(
        &self,
        context: &Path,
        dockerfile: &Path,
        tag: &str,
        build_args: &HashMap<String, String>,
        no_cache: bool,
    ) -> Result<DockerOutput> {
        let mut args: Vec<String> = vec![
            "build".to_string(),
            "-t".to_string(),
            tag.to_string(),
            "-f".to_string(),
            dockerfile.to_string_lossy().to_string(),
        ];

        if no_cache {
            args.push("--no-cache".to_string());
        }

        for (key, value) in build_args {
            args.push("--build-arg".to_string());
            args.push(format!("{}={}", key, value));
        }

        args.push(context.to_string_lossy().to_string());

        self.run_command(&args)
    }

    /// Run a new container
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &self,
        image: &str,
        name: Option<&str>,
        labels: &[(&str, &str)],
        mounts: &[(&str, &str)],
        ports: &[(u16, u16)],
        env: &[(&str, &str)],
        workdir: Option<&str>,
        command: Option<&[&str]>,
        detach: bool,
        remove: bool,
    ) -> Result<DockerOutput> {
        let mut args: Vec<String> = vec!["run".to_string()];

        if detach {
            args.push("-d".to_string());
        }

        if remove {
            args.push("--rm".to_string());
        }

        if let Some(n) = name {
            args.push("--name".to_string());
            args.push(n.to_string());
        }

        for (key, value) in labels {
            args.push("--label".to_string());
            args.push(format!("{}={}", key, value));
        }

        for (source, target) in mounts {
            args.push("-v".to_string());
            args.push(format!("{}:{}", source, target));
        }

        for (host, container) in ports {
            args.push("-p".to_string());
            args.push(format!("{}:{}", host, container));
        }

        for (key, value) in env {
            args.push("-e".to_string());
            args.push(format!("{}={}", key, value));
        }

        if let Some(w) = workdir {
            args.push("-w".to_string());
            args.push(w.to_string());
        }

        args.push(image.to_string());

        if let Some(cmd) = command {
            args.extend(cmd.iter().map(|s| s.to_string()));
        }

        self.run_command(&args)
    }

    // === Docker Compose methods ===

    /// Get the compose command prefix
    fn compose_cmd(&self) -> Vec<String> {
        if let Some(ref path) = self.compose_path {
            vec![path.clone()]
        } else {
            vec![self.docker_path.clone(), "compose".to_string()]
        }
    }

    /// Run docker compose up
    pub fn compose_up(
        &self,
        compose_files: &[&Path],
        project_name: Option<&str>,
        service: Option<&str>,
        build: bool,
        detach: bool,
    ) -> Result<DockerOutput> {
        let mut cmd_parts = self.compose_cmd();

        for file in compose_files {
            cmd_parts.push("-f".to_string());
            cmd_parts.push(file.to_string_lossy().to_string());
        }

        if let Some(name) = project_name {
            cmd_parts.push("-p".to_string());
            cmd_parts.push(name.to_string());
        }

        cmd_parts.push("up".to_string());

        if build {
            cmd_parts.push("--build".to_string());
        }

        if detach {
            cmd_parts.push("-d".to_string());
        }

        if let Some(svc) = service {
            cmd_parts.push(svc.to_string());
        }

        self.run_compose_command(&cmd_parts)
    }

    /// Run docker compose down
    pub fn compose_down(
        &self,
        compose_files: &[&Path],
        project_name: Option<&str>,
        volumes: bool,
        remove_orphans: bool,
    ) -> Result<DockerOutput> {
        let mut cmd_parts = self.compose_cmd();

        for file in compose_files {
            cmd_parts.push("-f".to_string());
            cmd_parts.push(file.to_string_lossy().to_string());
        }

        if let Some(name) = project_name {
            cmd_parts.push("-p".to_string());
            cmd_parts.push(name.to_string());
        }

        cmd_parts.push("down".to_string());

        if volumes {
            cmd_parts.push("-v".to_string());
        }

        if remove_orphans {
            cmd_parts.push("--remove-orphans".to_string());
        }

        self.run_compose_command(&cmd_parts)
    }

    /// Run docker compose exec
    pub fn compose_exec(
        &self,
        compose_files: &[&Path],
        project_name: Option<&str>,
        service: &str,
        command: &[&str],
        user: Option<&str>,
        workdir: Option<&str>,
    ) -> Result<DockerOutput> {
        self.compose_exec_with_options(
            compose_files,
            service,
            command,
            ComposeExecOptions {
                project_name,
                user,
                workdir,
                environment: &[],
            },
        )
    }

    pub(crate) fn compose_exec_with_options(
        &self,
        compose_files: &[&Path],
        service: &str,
        command: &[&str],
        options: ComposeExecOptions<'_>,
    ) -> Result<DockerOutput> {
        let mut cmd_parts = self.compose_cmd();

        for file in compose_files {
            cmd_parts.push("-f".to_string());
            cmd_parts.push(file.to_string_lossy().to_string());
        }

        if let Some(name) = options.project_name {
            cmd_parts.push("-p".to_string());
            cmd_parts.push(name.to_string());
        }

        cmd_parts.push("exec".to_string());
        cmd_parts.push("-T".to_string()); // No TTY for non-interactive

        if let Some(u) = options.user {
            cmd_parts.push("-u".to_string());
            cmd_parts.push(u.to_string());
        }

        if let Some(w) = options.workdir {
            cmd_parts.push("-w".to_string());
            cmd_parts.push(w.to_string());
        }

        for (key, value) in options.environment {
            cmd_parts.push("-e".to_string());
            cmd_parts.push(format!("{key}={value}"));
        }

        cmd_parts.push(service.to_string());
        cmd_parts.extend(command.iter().map(|s| s.to_string()));

        self.run_compose_command(&cmd_parts)
    }

    /// Run docker compose build
    pub fn compose_build(
        &self,
        compose_files: &[&Path],
        project_name: Option<&str>,
        service: Option<&str>,
        no_cache: bool,
    ) -> Result<DockerOutput> {
        let mut cmd_parts = self.compose_cmd();

        for file in compose_files {
            cmd_parts.push("-f".to_string());
            cmd_parts.push(file.to_string_lossy().to_string());
        }

        if let Some(name) = project_name {
            cmd_parts.push("-p".to_string());
            cmd_parts.push(name.to_string());
        }

        cmd_parts.push("build".to_string());

        if no_cache {
            cmd_parts.push("--no-cache".to_string());
        }

        if let Some(svc) = service {
            cmd_parts.push(svc.to_string());
        }

        self.run_compose_command(&cmd_parts)
    }

    /// Run docker compose ps to get container ID for a service
    pub fn compose_ps(
        &self,
        compose_files: &[&Path],
        project_name: Option<&str>,
        service: &str,
    ) -> Result<Option<String>> {
        let mut cmd_parts = self.compose_cmd();

        for file in compose_files {
            cmd_parts.push("-f".to_string());
            cmd_parts.push(file.to_string_lossy().to_string());
        }

        if let Some(name) = project_name {
            cmd_parts.push("-p".to_string());
            cmd_parts.push(name.to_string());
        }

        cmd_parts.push("ps".to_string());
        cmd_parts.push("-q".to_string());
        cmd_parts.push(service.to_string());

        let output = self.run_compose_command(&cmd_parts)?;
        if output.success {
            let id = output.stdout.trim();
            if id.is_empty() {
                Ok(None)
            } else {
                Ok(Some(id.to_string()))
            }
        } else {
            Ok(None)
        }
    }

    // === Internal methods ===

    fn run_command(&self, args: &[String]) -> Result<DockerOutput> {
        tracing::debug!(
            command = %self.docker_path,
            operation = args.first().map(String::as_str).unwrap_or("unknown"),
            argument_count = args.len(),
            "Running docker command"
        );

        let output = Command::new(&self.docker_path)
            .args(args)
            .output()
            .context("Failed to run Docker command")?;

        Ok(Self::parse_output(output))
    }

    fn run_compose_command(&self, cmd_parts: &[String]) -> Result<DockerOutput> {
        let (program, args) = (&cmd_parts[0], &cmd_parts[1..]);
        tracing::debug!(
            command = %program,
            argument_count = args.len(),
            "Running docker compose command"
        );

        let output = Command::new(program)
            .args(args)
            .output()
            .context("Failed to run Docker Compose command")?;

        Ok(Self::parse_output(output))
    }

    fn parse_output(output: Output) -> DockerOutput {
        DockerOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        }
    }
}

impl Default for Docker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_state_from_str() {
        assert_eq!(ContainerState::from("running"), ContainerState::Running);
        assert_eq!(ContainerState::from("Running"), ContainerState::Running);
        assert_eq!(ContainerState::from("exited"), ContainerState::Exited);
        assert_eq!(
            ContainerState::from("unknown_state"),
            ContainerState::Unknown("unknown_state".to_string())
        );
    }

    #[test]
    fn test_docker_new() {
        let docker = Docker::new();
        assert_eq!(docker.docker_path, "docker");
    }

    #[test]
    fn test_docker_with_paths() {
        let docker = Docker::with_paths("/custom/docker", Some("/custom/compose".to_string()));
        assert_eq!(docker.docker_path, "/custom/docker");
        assert_eq!(docker.compose_path, Some("/custom/compose".to_string()));
    }
}
