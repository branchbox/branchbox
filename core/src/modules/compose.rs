//! Docker Compose Module
//!
//! Manages Docker Compose configuration for feature worktrees:
//! - Container naming validation and uniqueness
//! - Compose configuration validation
//! - Network isolation per worktree
//! - Environment variable validation

use super::Module;
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Docker Compose module
pub struct ComposeModule {
    enabled: bool,
    compose_project_name: String,
    devcontainer_name: String,
    compose_file_name: String,
}

impl ComposeModule {
    /// Create a new Compose module
    pub fn new() -> Self {
        Self {
            enabled: false,
            compose_project_name: String::new(),
            devcontainer_name: String::new(),
            compose_file_name: String::new(),
        }
    }

    /// Check for container name conflicts
    fn check_container_conflicts(&self) -> Result<()> {
        let output = Command::new("docker")
            .args([
                "ps",
                "--filter",
                &format!(
                    "label=com.docker.compose.project={}",
                    self.compose_project_name
                ),
                "--format",
                "{{.Names}}",
            ])
            .output()
            .map_err(|e| Error::validation(format!("Failed to check containers: {}", e)))?;

        if output.status.success() {
            let containers = String::from_utf8_lossy(&output.stdout);
            if !containers.trim().is_empty() {
                tracing::warn!(
                    "Containers with project name '{}' are already running: {}",
                    self.compose_project_name,
                    containers.trim()
                );
            }
        }

        Ok(())
    }

    fn env_value(path: &Path, key: &str) -> Option<String> {
        let contents = fs::read_to_string(path).ok()?;
        contents.lines().find_map(|line| {
            let (candidate, value) = line.trim().split_once('=')?;
            if candidate.trim() != key {
                return None;
            }
            let value = value.trim();
            Some(
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
                    .or_else(|| {
                        value
                            .strip_prefix('"')
                            .and_then(|value| value.strip_suffix('"'))
                    })
                    .unwrap_or(value)
                    .to_string(),
            )
        })
    }

    fn docker_output(args: &[&str], description: &str) -> Result<std::process::Output> {
        Command::new("docker")
            .args(args)
            .output()
            .map_err(|err| Error::validation(format!("Failed to {description}: {err}")))
    }

    fn output_lines(output: &std::process::Output, description: &str) -> Result<Vec<String>> {
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Failed to {description}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    fn is_compose_project_name(value: &str) -> bool {
        value
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
            && value.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '-' | '_')
            })
    }

    /// Discover the Compose projects created by the devcontainer CLI for this exact worktree.
    fn discover_devcontainer_projects(&self, feature_dir: &Path) -> Result<BTreeSet<String>> {
        let mut workspace_paths = BTreeSet::from([feature_dir.to_string_lossy().to_string()]);
        if let Ok(canonical) = feature_dir.canonicalize() {
            workspace_paths.insert(canonical.to_string_lossy().to_string());
        }

        let mut projects = BTreeSet::new();
        for workspace_path in workspace_paths {
            let filter = format!("label=devcontainer.local_folder={workspace_path}");
            let output = Self::docker_output(
                &[
                    "ps",
                    "-a",
                    "--filter",
                    &filter,
                    "--format",
                    "{{.Label \"com.docker.compose.project\"}}",
                ],
                "discover devcontainer Compose projects",
            )?;
            projects.extend(
                Self::output_lines(&output, "discover devcontainer Compose projects")?
                    .into_iter()
                    .filter(|project| Self::is_compose_project_name(project)),
            );
        }
        Ok(projects)
    }

    fn project_resource_ids(&self, project: &str, kind: &str) -> Result<Vec<String>> {
        let filter = format!("label=com.docker.compose.project={project}");
        let args = match kind {
            "container" => vec!["ps", "-a", "--filter", &filter, "--format", "{{.ID}}"],
            "network" => vec!["network", "ls", "--filter", &filter, "--format", "{{.ID}}"],
            "volume" => vec!["volume", "ls", "--filter", &filter, "--format", "{{.Name}}"],
            _ => {
                return Err(Error::validation(format!(
                    "Unknown Docker resource: {kind}"
                )))
            }
        };
        let output = Self::docker_output(&args, &format!("list {kind}s for project '{project}'"))?;
        Self::output_lines(&output, &format!("list {kind}s for project '{project}'"))
    }

    /// Remove and verify only resources bearing an exact, owned Compose project label.
    fn cleanup_project_resources(&self, project: &str) -> Result<()> {
        for (kind, remove_args) in [
            ("container", vec!["rm", "-f"]),
            ("network", vec!["network", "rm"]),
            ("volume", vec!["volume", "rm"]),
        ] {
            for id in self.project_resource_ids(project, kind)? {
                let mut args = remove_args.clone();
                args.push(&id);
                let output = Self::docker_output(
                    &args,
                    &format!("remove {kind} '{id}' for project '{project}'"),
                )?;
                if !output.status.success() {
                    tracing::warn!(
                        "Failed to remove {} '{}' for project '{}': {}",
                        kind,
                        id,
                        project,
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
            }
        }

        let mut remaining = Vec::new();
        for kind in ["container", "network", "volume"] {
            for id in self.project_resource_ids(project, kind)? {
                remaining.push(format!("{kind}:{id}"));
            }
        }
        if !remaining.is_empty() {
            return Err(Error::validation(format!(
                "Docker teardown left resources for Compose project '{project}': {}",
                remaining.join(", ")
            )));
        }
        Ok(())
    }

    fn docker_compose_unavailable(status: &std::process::ExitStatus, stderr: &str) -> bool {
        if status.code() == Some(125) {
            return true;
        }

        let stderr_lower = stderr.to_ascii_lowercase();
        stderr_lower.contains("is not a docker command")
            || stderr_lower.contains("unknown command \"compose\"")
            || stderr_lower.contains("unknown shorthand flag")
    }

    fn run_compose_command(
        &self,
        feature_dir: &Path,
        args: &[&str],
    ) -> Result<std::process::Output> {
        match Command::new("docker")
            .arg("compose")
            .args(args)
            .current_dir(feature_dir)
            .output()
        {
            Ok(output) if output.status.success() => Ok(output),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !Self::docker_compose_unavailable(&output.status, &stderr) {
                    return Ok(output);
                }
                let primary_error = stderr.trim().to_string();

                Command::new("docker-compose")
                    .args(args)
                    .current_dir(feature_dir)
                    .output()
                    .map_err(|fallback_error| {
                        Error::validation(format!(
                            "Failed to run Docker Compose (tried `docker compose` which failed with: '{}', then `docker-compose` which failed with: {})",
                            primary_error,
                            fallback_error
                        ))
                    })
            }
            Err(primary_error) => Command::new("docker-compose")
                .args(args)
                .current_dir(feature_dir)
                .output()
                .map_err(|fallback_error| {
                    Error::validation(format!(
                        "Failed to run Docker Compose (docker compose error: {}; docker-compose error: {})",
                        primary_error, fallback_error
                    ))
                }),
        }
    }

    fn down_project(&self, feature_dir: &Path, compose_file: &Path, project: &str) -> Result<()> {
        let compose_file = compose_file
            .to_str()
            .ok_or_else(|| Error::validation("Compose file path is not valid UTF-8"))?;
        let project_dir = feature_dir.join(".devcontainer");
        let project_dir = project_dir
            .to_str()
            .ok_or_else(|| Error::validation("Compose project path is not valid UTF-8"))?;
        let env_file = feature_dir.join(".devcontainer/.branchbox.env");
        let env_file_string = env_file.to_string_lossy().to_string();
        let mut args = Vec::new();
        if env_file.exists() {
            args.extend(["--env-file", env_file_string.as_str()]);
        }
        args.extend([
            "--project-name",
            project,
            "-f",
            compose_file,
            "--project-directory",
            project_dir,
            "down",
            "--volumes",
            "--remove-orphans",
        ]);
        let output = self.run_compose_command(feature_dir, &args)?;
        if !output.status.success() {
            tracing::warn!(
                "Docker Compose down failed for '{}'; trying exact label cleanup: {}",
                project,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        self.cleanup_project_resources(project)
    }
}

impl Default for ComposeModule {
    fn default() -> Self {
        Self::new()
    }
}

impl Module for ComposeModule {
    fn name(&self) -> &str {
        "compose"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        // Check for compose files
        let compose_yaml = project_dir.join(".devcontainer/compose.yaml");
        let compose_yml = project_dir.join(".devcontainer/docker-compose.yml");
        let dockerfile = project_dir.join(".devcontainer/Dockerfile");

        compose_yaml.exists() || compose_yml.exists() || dockerfile.exists()
    }

    fn init(&mut self, main_dir: &Path, feature_dir: &Path) -> Result<()> {
        let work_feature = feature_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid feature directory name".to_string()))?;

        // Set compose project name and devcontainer name
        let base_prefix = std::env::var("BASE_PREFIX").unwrap_or_else(|_| "app".to_string());
        let managed_env = feature_dir.join(".devcontainer/.branchbox.env");
        self.compose_project_name = Self::env_value(&managed_env, "COMPOSE_PROJECT_NAME")
            .or_else(|| std::env::var("COMPOSE_PROJECT_NAME").ok())
            .unwrap_or_else(|| format!("{}-{}", base_prefix, work_feature));
        self.devcontainer_name = Self::env_value(&managed_env, "DEVCONTAINER_NAME")
            .or_else(|| std::env::var("DEVCONTAINER_NAME").ok())
            .unwrap_or_else(|| format!("{}-{}", base_prefix, work_feature));

        // Find compose file
        if main_dir.join(".devcontainer/compose.yaml").exists() {
            self.compose_file_name = "compose.yaml".to_string();
        } else if main_dir.join(".devcontainer/docker-compose.yml").exists() {
            self.compose_file_name = "docker-compose.yml".to_string();
        }

        tracing::info!("Project name: {}", self.compose_project_name);
        tracing::info!("Devcontainer name: {}", self.devcontainer_name);

        self.enabled = true;
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Validating Docker Compose configuration...");

        // Validate compose configuration
        if !self.compose_file_name.is_empty() {
            let compose_file = feature_dir
                .join(".devcontainer")
                .join(&self.compose_file_name);
            if compose_file.exists() {
                self.validate(_main_dir, feature_dir)?;
            } else {
                tracing::info!("No compose file found, skipping validation");
            }
        }

        // Check for container name conflicts
        self.check_container_conflicts()?;

        tracing::info!("Compose configuration validated");
        Ok(())
    }

    fn teardown(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Stopping and removing containers...");

        let compose_file = feature_dir
            .join(".devcontainer")
            .join(&self.compose_file_name);
        if !compose_file.exists() {
            tracing::info!("No compose file found, skipping container cleanup");
            return Ok(());
        }

        let mut projects = self.discover_devcontainer_projects(feature_dir)?;
        projects.insert(self.compose_project_name.clone());
        for project in projects {
            tracing::info!(
                "Removing Docker resources for Compose project '{}'...",
                project
            );
            self.down_project(feature_dir, &compose_file, &project)?;
        }

        tracing::info!("Containers, networks, and volumes removed");
        Ok(())
    }

    fn validate(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        if self.compose_file_name.is_empty() {
            return Ok(());
        }

        let compose_file = feature_dir
            .join(".devcontainer")
            .join(&self.compose_file_name);
        let compose_file_str = compose_file
            .to_str()
            .ok_or_else(|| Error::validation("Compose file path is not valid UTF-8".to_string()))?;

        if compose_file.exists() {
            let output = self
                .run_compose_command(feature_dir, &["-f", compose_file_str, "config"])
                .map_err(|e| Error::validation(format!("Failed to validate compose: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(Error::validation(format!(
                    "Docker Compose configuration is invalid: {}",
                    stderr
                )));
            }

            tracing::info!("Compose configuration is valid");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_compose_yaml() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/compose.yaml"),
            "version: '3'",
        )
        .unwrap();

        let module = ComposeModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_detect_dockerfile() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/Dockerfile"),
            "FROM ubuntu",
        )
        .unwrap();

        let module = ComposeModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_detect_no_compose() {
        let temp_dir = TempDir::new().unwrap();

        let module = ComposeModule::new();
        assert!(!module.detect(temp_dir.path()));
    }

    #[test]
    fn test_init() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::create_dir_all(main_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            main_dir.path().join(".devcontainer/compose.yaml"),
            "version: '3'",
        )
        .unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        assert!(module.enabled);
        assert!(!module.compose_project_name.is_empty());
        assert_eq!(module.compose_file_name, "compose.yaml");
    }

    #[test]
    fn test_init_docker_compose_yml() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();
        std::fs::create_dir_all(main_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            main_dir.path().join(".devcontainer/docker-compose.yml"),
            "version: '3'",
        )
        .unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        assert!(module.enabled);
        assert_eq!(module.compose_file_name, "docker-compose.yml");
    }

    #[test]
    fn test_init_restores_managed_compose_identity() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir_all(feature_dir.join(".devcontainer")).unwrap();
        std::fs::create_dir_all(main_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            main_dir.path().join(".devcontainer/compose.yaml"),
            "version: '3'",
        )
        .unwrap();
        std::fs::write(
            feature_dir.join(".devcontainer/.branchbox.env"),
            "COMPOSE_PROJECT_NAME=persisted-project\nDEVCONTAINER_NAME='persisted-container'\n",
        )
        .unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        assert_eq!(module.compose_project_name, "persisted-project");
        assert_eq!(module.devcontainer_name, "persisted-container");
    }

    #[test]
    fn test_name() {
        let module = ComposeModule::new();
        assert_eq!(module.name(), "compose");
    }

    #[test]
    fn test_default() {
        let module = ComposeModule::default();
        assert_eq!(module.name(), "compose");
        assert!(!module.enabled);
    }

    #[test]
    fn test_validate_no_compose_file() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();

        let module = ComposeModule::new();
        // Should not error when no compose file
        module.validate(main_dir.path(), &feature_dir).unwrap();
    }

    // Integration tests requiring Docker
    // Run with: cargo test -- --ignored

    #[test]
    #[ignore]
    fn test_validate_with_valid_compose_file() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir_all(feature_dir.join(".devcontainer")).unwrap();

        // Create a valid compose file
        let compose_content = r#"
version: '3.8'
services:
  app:
    image: alpine:latest
    command: sleep 3600
"#;
        std::fs::write(
            feature_dir.join(".devcontainer/compose.yaml"),
            compose_content,
        )
        .unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Should successfully validate
        let result = module.validate(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_validate_with_invalid_compose_file() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir_all(main_dir.path().join(".devcontainer")).unwrap();
        std::fs::create_dir_all(feature_dir.join(".devcontainer")).unwrap();

        // Create an invalid compose file in main_dir (where init() looks for it)
        let compose_content = "invalid: yaml: content: [[[";
        std::fs::write(
            main_dir.path().join(".devcontainer/compose.yaml"),
            compose_content,
        )
        .unwrap();

        // Also create it in feature_dir (where validate() will check it)
        std::fs::write(
            feature_dir.join(".devcontainer/compose.yaml"),
            compose_content,
        )
        .unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Should fail validation
        let result = module.validate(main_dir.path(), &feature_dir);
        assert!(result.is_err());
    }

    #[test]
    #[ignore]
    fn test_check_container_conflicts() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-conflict-check");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Should succeed even if no containers exist
        let result = module.check_container_conflicts();
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_cleanup_project_resources() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-cleanup");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = ComposeModule::new();
        module.compose_project_name = "branchbox-test-orphan-cleanup".to_string();

        // Should succeed even if no orphaned containers exist
        let result = module.cleanup_project_resources("branchbox-test-orphan-cleanup");
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_setup_with_docker() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-setup");
        std::fs::create_dir_all(feature_dir.join(".devcontainer")).unwrap();

        let compose_content = r#"
version: '3.8'
services:
  test:
    image: alpine:latest
    command: sleep 3600
"#;
        std::fs::write(
            feature_dir.join(".devcontainer/compose.yaml"),
            compose_content,
        )
        .unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Setup should validate and check conflicts
        let result = module.setup(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_teardown_with_docker() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-teardown");
        std::fs::create_dir_all(feature_dir.join(".devcontainer")).unwrap();

        let compose_content = r#"
version: '3.8'
services:
  test:
    image: alpine:latest
    command: sleep 1
"#;
        std::fs::write(
            feature_dir.join(".devcontainer/compose.yaml"),
            compose_content,
        )
        .unwrap();

        let mut module = ComposeModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        // Teardown should succeed even if no containers running
        let result = module.teardown(main_dir.path(), &feature_dir);
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_full_lifecycle_with_docker() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-lifecycle");
        std::fs::create_dir_all(main_dir.path().join(".devcontainer")).unwrap();
        std::fs::create_dir_all(feature_dir.join(".devcontainer")).unwrap();

        let compose_content = r#"
version: '3.8'
services:
  test:
    image: alpine:latest
    command: sleep 3600
    labels:
      com.docker.compose.project: branchbox-test-lifecycle
"#;

        std::fs::write(
            main_dir.path().join(".devcontainer/compose.yaml"),
            compose_content,
        )
        .unwrap();
        std::fs::write(
            feature_dir.join(".devcontainer/compose.yaml"),
            compose_content,
        )
        .unwrap();

        let mut module = ComposeModule::new();

        // Init
        module.init(main_dir.path(), &feature_dir).unwrap();
        assert!(module.enabled);

        // Setup (validate + check conflicts)
        module.setup(main_dir.path(), &feature_dir).unwrap();

        // Validate
        module.validate(main_dir.path(), &feature_dir).unwrap();

        // Teardown (cleanup containers)
        module.teardown(main_dir.path(), &feature_dir).unwrap();
    }
}
