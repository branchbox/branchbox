//! Docker Compose Module
//!
//! Manages Docker Compose configuration for feature worktrees:
//! - Container naming validation and uniqueness
//! - Compose configuration validation
//! - Network isolation per worktree
//! - Environment variable validation

use super::Module;
use crate::{Error, Result};
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

    /// Cleanup orphaned containers
    fn cleanup_orphaned_containers(&self) -> Result<()> {
        // Remove orphaned containers
        let output = Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                &format!(
                    "label=com.docker.compose.project={}",
                    self.compose_project_name
                ),
                "--format",
                "{{.ID}}",
            ])
            .output()
            .map_err(|e| Error::validation(format!("Failed to list containers: {}", e)))?;

        if output.status.success() {
            let container_ids = String::from_utf8_lossy(&output.stdout);
            if !container_ids.trim().is_empty() {
                tracing::info!("Removing orphaned containers...");
                for id in container_ids.lines() {
                    let _ = Command::new("docker")
                        .args(["rm", "-f", id.trim()])
                        .output();
                }
            }
        }

        // Remove orphaned networks
        let output = Command::new("docker")
            .args([
                "network",
                "ls",
                "--filter",
                &format!(
                    "label=com.docker.compose.project={}",
                    self.compose_project_name
                ),
                "--format",
                "{{.ID}}",
            ])
            .output()
            .map_err(|e| Error::validation(format!("Failed to list networks: {}", e)))?;

        if output.status.success() {
            let network_ids = String::from_utf8_lossy(&output.stdout);
            if !network_ids.trim().is_empty() {
                tracing::info!("Removing orphaned networks...");
                for id in network_ids.lines() {
                    let _ = Command::new("docker")
                        .args(["network", "rm", id.trim()])
                        .output();
                }
            }
        }

        Ok(())
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
        self.compose_project_name = std::env::var("COMPOSE_PROJECT_NAME")
            .unwrap_or_else(|_| format!("{}-{}", base_prefix, work_feature));
        self.devcontainer_name = std::env::var("DEVCONTAINER_NAME")
            .unwrap_or_else(|_| format!("{}-{}", base_prefix, work_feature));

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

        if compose_file.exists() {
            // Check if containers are running
            let ps_output = Command::new("docker")
                .args([
                    "compose",
                    "-f",
                    compose_file.to_str().unwrap(),
                    "ps",
                    "--quiet",
                ])
                .current_dir(feature_dir)
                .output()
                .map_err(|e| Error::validation(format!("Failed to check containers: {}", e)))?;

            if ps_output.status.success() && !ps_output.stdout.is_empty() {
                tracing::info!("Stopping containers for {}...", self.compose_project_name);

                let down_output = Command::new("docker")
                    .args([
                        "compose",
                        "-f",
                        compose_file.to_str().unwrap(),
                        "down",
                        "-v",
                    ])
                    .current_dir(feature_dir)
                    .output()
                    .map_err(|e| Error::validation(format!("Failed to stop containers: {}", e)))?;

                if down_output.status.success() {
                    tracing::info!("Containers stopped and removed");
                } else {
                    tracing::warn!("Failed to stop some containers");
                }
            } else {
                tracing::info!("No running containers found");
            }
        } else {
            tracing::info!("No compose file found, skipping container cleanup");
        }

        // Clean up orphaned containers
        self.cleanup_orphaned_containers()?;

        Ok(())
    }

    fn validate(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        if self.compose_file_name.is_empty() {
            return Ok(());
        }

        let compose_file = feature_dir
            .join(".devcontainer")
            .join(&self.compose_file_name);

        if compose_file.exists() {
            let output = Command::new("docker")
                .args(["compose", "-f", compose_file.to_str().unwrap(), "config"])
                .current_dir(feature_dir)
                .output()
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
    fn test_cleanup_orphaned_containers() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("test-cleanup");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = ComposeModule::new();
        module.compose_project_name = "branchbox-test-orphan-cleanup".to_string();

        // Should succeed even if no orphaned containers exist
        let result = module.cleanup_orphaned_containers();
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
