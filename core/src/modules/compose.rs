//! Docker Compose Module
//!
//! Manages Docker Compose configuration for feature worktrees:
//! - Container naming validation and uniqueness
//! - Compose configuration validation
//! - Network isolation per worktree
//! - Environment variable validation

use super::Module;
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

const TEARDOWN_PROJECTS_KEY: &str = "BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS";
const TEARDOWN_WORKSPACE_KEY: &str = "BRANCHBOX_TEARDOWN_WORKSPACE_SHA256";
const COMPOSE_IDENTITY_KEY: &str = "BRANCHBOX_COMPOSE_IDENTITY_SHA256";

fn managed_env_contents(feature_dir: &Path) -> Result<String> {
    if fs::symlink_metadata(feature_dir.join(".devcontainer"))
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(Error::validation(
            "Compose cleanup identity cannot use a symlinked devcontainer directory",
        ));
    }
    let path = feature_dir.join(".devcontainer/.branchbox.env");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Ok(fs::read_to_string(path)?)
        }
        Ok(_) => Err(Error::validation(
            "Compose cleanup identity requires a regular managed env file",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn ensure_managed_env_directory(feature_dir: &Path) -> Result<()> {
    let directory = feature_dir.join(".devcontainer");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(Error::validation(
            "Compose cleanup identity requires a regular devcontainer directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&directory)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn workspace_digest(feature_dir: &Path) -> Result<String> {
    let workspace = feature_dir.canonicalize()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(workspace.as_os_str().as_encoded_bytes())
    ))
}

fn compose_identity_digest(feature_dir: &Path, project: &str) -> Result<String> {
    let workspace = feature_dir.canonicalize()?;
    let mut digest = Sha256::new();
    digest.update(workspace.as_os_str().as_encoded_bytes());
    digest.update([0]);
    digest.update(project.as_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn compose_identity_line(feature_dir: &Path, project: &str) -> Result<String> {
    if !ComposeModule::is_compose_project_name(project) {
        return Err(Error::validation(
            "Invalid managed Compose project identity",
        ));
    }
    Ok(format!(
        "{COMPOSE_IDENTITY_KEY}={}\n",
        compose_identity_digest(feature_dir, project)?
    ))
}

/// Retained cleanup identity is valid only for the workspace that observed the exact labels.
/// Copying the managed env to another worktree cannot authorize cleanup of the old project.
pub(crate) fn retained_teardown_projects(feature_dir: &Path) -> Result<BTreeSet<String>> {
    let contents = managed_env_contents(feature_dir)?;
    let values = |key: &str| {
        contents
            .lines()
            .filter_map(move |line| {
                let (name, value) = line.split_once('=')?;
                (name.trim() == key).then_some(value.trim())
            })
            .collect::<Vec<_>>()
    };
    let projects = values(TEARDOWN_PROJECTS_KEY);
    let scopes = values(TEARDOWN_WORKSPACE_KEY);
    if projects.is_empty() && scopes.is_empty() {
        return Ok(BTreeSet::new());
    }
    if projects.len() != 1 || scopes.len() != 1 || scopes[0] != workspace_digest(feature_dir)? {
        return Err(Error::validation("Compose cleanup identity is malformed or belongs to another workspace; preserve it and retry from its owning worktree"));
    }
    let names: BTreeSet<_> = projects[0].split(',').map(ToOwned::to_owned).collect();
    if names.is_empty()
        || !names
            .iter()
            .all(|name| ComposeModule::is_compose_project_name(name))
    {
        return Err(Error::validation(
            "Compose cleanup identity contains an invalid project name",
        ));
    }
    Ok(names)
}

pub(crate) fn teardown_identity_lines(
    feature_dir: &Path,
    projects: &BTreeSet<String>,
) -> Result<String> {
    if !projects
        .iter()
        .all(|project| ComposeModule::is_compose_project_name(project))
    {
        return Err(Error::validation(
            "Cannot retain an invalid Compose cleanup project name",
        ));
    }
    if projects.is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "{TEARDOWN_PROJECTS_KEY}={}\n{TEARDOWN_WORKSPACE_KEY}={}\n",
        projects.iter().cloned().collect::<Vec<_>>().join(","),
        workspace_digest(feature_dir)?
    ))
}

pub(crate) fn persist_teardown_projects(
    feature_dir: &Path,
    projects: &BTreeSet<String>,
) -> Result<()> {
    let contents = managed_env_contents(feature_dir)?;
    // Preserve unrelated bytes (which may include private values) and existing permissions.
    let mut updated = contents
        .split_inclusive('\n')
        .filter(|line| {
            !line.split_once('=').is_some_and(|(name, _)| {
                matches!(name.trim(), TEARDOWN_PROJECTS_KEY | TEARDOWN_WORKSPACE_KEY)
            })
        })
        .collect::<String>();
    if !projects.is_empty() {
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&teardown_identity_lines(feature_dir, projects)?);
    }
    if updated != contents {
        ensure_managed_env_directory(feature_dir)?;
        crate::atomic_fs::write_atomic(
            &feature_dir.join(".devcontainer/.branchbox.env"),
            updated.as_bytes(),
            0o600,
        )?;
    }
    Ok(())
}

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

    /// A configuration filename or an ambient project name cannot establish resource ownership.
    /// Restore legacy identity only through the feature registry or exact Docker label evidence.
    fn managed_cleanup_project(
        &self,
        main_dir: &Path,
        feature_dir: &Path,
        known: &BTreeSet<String>,
    ) -> Result<Option<String>> {
        let contents = managed_env_contents(feature_dir)?;
        let values = |key: &str| -> Result<Option<String>> {
            let matches: Vec<_> = contents
                .lines()
                .filter_map(|line| {
                    let (name, value) = line.trim().split_once('=')?;
                    (name.trim() == key).then_some(value.trim())
                })
                .collect();
            if matches.len() > 1 {
                return Err(Error::validation(
                    "Duplicate managed Compose identity field",
                ));
            }
            Ok(matches.first().map(|value| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
                    .or_else(|| {
                        value
                            .strip_prefix('"')
                            .and_then(|value| value.strip_suffix('"'))
                    })
                    .unwrap_or(value)
                    .to_string()
            }))
        };
        let project = values("COMPOSE_PROJECT_NAME")?;
        if let Some(project) = &project {
            if !Self::is_compose_project_name(project) {
                return Err(Error::validation(
                    "Invalid managed Compose project identity",
                ));
            }
        }
        if values("WORK_FEATURE")?.is_some_and(|feature| {
            feature_dir.file_name().and_then(|name| name.to_str()) != Some(feature.as_str())
        }) {
            return Err(Error::validation("Managed Compose identity belongs to another feature; restore this feature's managed env before retrying"));
        }
        let expected = crate::workflows::feature::recorded_compose_project(main_dir, feature_dir)?;
        if let (Some(project), Some(expected)) = (&project, &expected) {
            if project != expected {
                return Err(Error::validation("Managed Compose project differs from this feature's recorded identity; restore its managed env before retrying"));
            }
        }
        if let Some(scope) = values(COMPOSE_IDENTITY_KEY)? {
            let project = project
                .ok_or_else(|| Error::validation("Managed Compose identity has no project name"))?;
            if scope != compose_identity_digest(feature_dir, &project)? {
                return Err(Error::validation("Managed Compose identity is invalid or belongs to another workspace; restore this feature's managed env before retrying"));
            }
            return Ok(Some(project));
        }
        if let Some(expected) = expected {
            return Ok(Some(expected));
        }
        match project {
            Some(project) if known.contains(&project) => Ok(Some(project)),
            None if !known.is_empty() => Ok(None),
            _ => Err(Error::validation("Legacy managed Compose ownership is ambiguous; restore this feature's recorded or workspace-bound identity before retrying")),
        }
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

    pub(crate) fn is_compose_project_name(value: &str) -> bool {
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
        let mut errors = Vec::new();
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
                    errors.push(format!(
                        "Cannot remove {kind} '{id}' for project '{project}'"
                    ));
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
        if !remaining.is_empty() || !errors.is_empty() {
            return Err(Error::validation(format!(
                "Docker teardown did not verify cleanup for Compose project '{project}': {}; {}",
                errors.join(", "),
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

        let root_compose_config = fs::read_to_string(project_dir.join(".devcontainer.json"))
            .ok()
            .and_then(|contents| {
                jsonc_parser::parse_to_serde_value(&contents, &Default::default())
                    .ok()
                    .flatten()
            })
            .and_then(|config| config.get("dockerComposeFile").cloned())
            .is_some_and(|files| {
                files.as_str().is_some_and(|file| !file.is_empty())
                    || files.as_array().is_some_and(|files| {
                        !files.is_empty()
                            && files
                                .iter()
                                .all(|file| file.as_str().is_some_and(|file| !file.is_empty()))
                    })
            });
        compose_yaml.exists() || compose_yml.exists() || dockerfile.exists() || root_compose_config
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

    fn teardown(&self, main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Stopping and removing containers...");

        let compose_file = feature_dir
            .join(".devcontainer")
            .join(&self.compose_file_name);
        let mut projects = retained_teardown_projects(feature_dir)?;
        projects.extend(self.discover_devcontainer_projects(feature_dir)?);
        if compose_file.is_file() {
            if let Some(project) = self.managed_cleanup_project(main_dir, feature_dir, &projects)? {
                projects.insert(project);
            }
        }
        // Container labels can disappear before network/volume cleanup completes. Retain their
        // exact project identity first, so a retry cannot mistake an empty container probe for success.
        persist_teardown_projects(feature_dir, &projects)?;
        let mut errors = Vec::new();
        for project in &projects {
            tracing::info!(
                "Removing Docker resources for Compose project '{}'...",
                project
            );
            let result = if compose_file.is_file() {
                self.down_project(feature_dir, &compose_file, project)
            } else {
                self.cleanup_project_resources(project)
            };
            if let Err(error) = result {
                errors.push(error.to_string());
            }
        }
        if !errors.is_empty() {
            return Err(Error::validation(errors.join("; ")));
        }
        persist_teardown_projects(feature_dir, &BTreeSet::new())?;

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
    fn managed_compose_binding_checks_workspace_project_and_alias() {
        let workspace = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        let main = TempDir::new().unwrap();
        fs::create_dir(workspace.path().join(".devcontainer")).unwrap();
        fs::create_dir(other.path().join(".devcontainer")).unwrap();
        let record = format!(
            "COMPOSE_PROJECT_NAME=fixture-owned\n{}",
            compose_identity_line(workspace.path(), "fixture-owned").unwrap()
        );
        let env = workspace.path().join(".devcontainer/.branchbox.env");
        fs::write(&env, &record).unwrap();
        let module = ComposeModule::new();
        assert_eq!(
            module
                .managed_cleanup_project(main.path(), workspace.path(), &BTreeSet::new())
                .unwrap(),
            Some("fixture-owned".to_string())
        );
        #[cfg(unix)]
        {
            let alias = main.path().join("alias");
            std::os::unix::fs::symlink(workspace.path(), &alias).unwrap();
            assert_eq!(
                compose_identity_line(&alias, "fixture-owned").unwrap(),
                compose_identity_line(workspace.path(), "fixture-owned").unwrap()
            );
            assert_eq!(
                module
                    .managed_cleanup_project(main.path(), &alias, &BTreeSet::new())
                    .unwrap(),
                Some("fixture-owned".to_string())
            );
        }
        fs::write(other.path().join(".devcontainer/.branchbox.env"), &record).unwrap();
        assert!(module
            .managed_cleanup_project(main.path(), other.path(), &BTreeSet::new())
            .is_err());
        fs::write(
            &env,
            record.replace(
                "COMPOSE_PROJECT_NAME=fixture-owned",
                "COMPOSE_PROJECT_NAME=foreign",
            ),
        )
        .unwrap();
        assert!(module
            .managed_cleanup_project(main.path(), workspace.path(), &BTreeSet::new())
            .is_err());
        fs::write(&env, "COMPOSE_PROJECT_NAME=fixture-owned\n").unwrap();
        assert!(module
            .managed_cleanup_project(main.path(), workspace.path(), &BTreeSet::new())
            .is_err());
        assert_eq!(
            module
                .managed_cleanup_project(
                    main.path(),
                    workspace.path(),
                    &BTreeSet::from(["fixture-owned".to_string()])
                )
                .unwrap(),
            Some("fixture-owned".to_string())
        );
    }

    #[test]
    fn teardown_identity_is_atomic_scoped_and_preserves_unrelated_env_bytes() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join(".devcontainer")).unwrap();
        let env = temp.path().join(".devcontainer/.branchbox.env");
        let original = "# managed fixture\nCOMPOSE_PROJECT_NAME=fixture-owned\nPRIVATE_FIXTURE='literal space'\n";
        fs::write(&env, original).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&env, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let projects = BTreeSet::from(["vsc-owned".to_string(), "fixture-owned".to_string()]);
        persist_teardown_projects(temp.path(), &projects).unwrap();
        assert!(fs::read_to_string(&env).unwrap().starts_with(original));
        assert_eq!(retained_teardown_projects(temp.path()).unwrap(), projects);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&env).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        persist_teardown_projects(temp.path(), &BTreeSet::new()).unwrap();
        assert_eq!(fs::read_to_string(&env).unwrap(), original);
    }

    #[test]
    fn teardown_identity_creates_a_managed_directory_for_root_devcontainer_configs() {
        let workspace = TempDir::new().unwrap();
        let projects = BTreeSet::from(["vsc-root-config".to_string()]);
        assert!(retained_teardown_projects(workspace.path())
            .unwrap()
            .is_empty());
        persist_teardown_projects(workspace.path(), &projects).unwrap();
        assert!(workspace
            .path()
            .join(".devcontainer/.branchbox.env")
            .is_file());
        assert_eq!(
            retained_teardown_projects(workspace.path()).unwrap(),
            projects
        );
    }

    #[test]
    fn copied_malformed_and_unsafe_teardown_identity_never_authorizes_cleanup() {
        let source = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        for path in [source.path(), other.path()] {
            fs::create_dir(path.join(".devcontainer")).unwrap();
        }
        let projects = BTreeSet::from(["vsc-owned".to_string()]);
        persist_teardown_projects(source.path(), &projects).unwrap();
        let contents =
            fs::read_to_string(source.path().join(".devcontainer/.branchbox.env")).unwrap();
        let env = other.path().join(".devcontainer/.branchbox.env");
        fs::write(&env, &contents).unwrap();
        assert!(retained_teardown_projects(other.path()).is_err());
        for contents in [
            format!("{TEARDOWN_PROJECTS_KEY}=vsc-owned\n"),
            format!("{TEARDOWN_PROJECTS_KEY}=invalid/name\n{TEARDOWN_WORKSPACE_KEY}={}\n", workspace_digest(other.path()).unwrap()),
            format!("{TEARDOWN_PROJECTS_KEY}=vsc-owned\n{TEARDOWN_PROJECTS_KEY}=foreign\n{TEARDOWN_WORKSPACE_KEY}={}\n", workspace_digest(other.path()).unwrap()),
        ] {
            fs::write(&env, &contents).unwrap();
            assert!(retained_teardown_projects(other.path()).is_err());
            assert_eq!(fs::read_to_string(&env).unwrap(), contents);
        }
    }

    #[cfg(unix)]
    #[test]
    fn teardown_identity_refuses_symlinked_files_and_parent_directories() {
        use std::os::unix::fs::symlink;
        let source = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        fs::create_dir(source.path().join(".devcontainer")).unwrap();
        let private = other.path().join("private-fixture");
        fs::write(&private, "unchanged\n").unwrap();
        let env = source.path().join(".devcontainer/.branchbox.env");
        symlink(&private, &env).unwrap();
        let projects = BTreeSet::from(["vsc-owned".to_string()]);
        assert!(retained_teardown_projects(source.path()).is_err());
        assert!(persist_teardown_projects(source.path(), &projects).is_err());
        fs::remove_file(&env).unwrap();
        fs::remove_dir(source.path().join(".devcontainer")).unwrap();
        symlink(other.path(), source.path().join(".devcontainer")).unwrap();
        assert!(retained_teardown_projects(source.path()).is_err());
        assert!(persist_teardown_projects(source.path(), &projects).is_err());
        assert_eq!(fs::read_to_string(&private).unwrap(), "unchanged\n");
        assert!(!other.path().join(".branchbox.env").exists());
    }

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
