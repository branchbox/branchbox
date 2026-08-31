//! Devcontainer runtime inside an orchestrator-owned isolation boundary.
//!
//! This provider deliberately owns no VM and no SSH control plane. The outer orchestrator
//! materializes a signed assignment and opaque lease files in the guest; BranchBox validates only
//! the versioned assignment, paths, consumers, and digests, creates the Git worktree through the
//! normal feature workflow, and operates Docker/devcontainers directly in the current guest.

use super::{
    exec_result, RuntimeContext, RuntimeExecResult, RuntimeMetadata, RuntimePort, RuntimeProvider,
    RuntimeProviderKind, RuntimeResidue, RuntimeTeardownReport,
};
use crate::{devcontainer_runtime::DevcontainerConfig, Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

const LEGACY_MANIFEST_VERSION: &str = "1";
const MANAGED_MANIFEST_VERSION: &str = "2";
const MANAGED_RUNTIME_ROOT: &str = "/run/branchbox/managed";
const LEASE_TARGET_ROOT: &str = "/run/branchbox/leases";
const PROJECT_ENVIRONMENT_TARGET: &str = "/run/branchbox/leases/project-env";
const MAX_PROVIDER_ENVIRONMENT_BINDINGS: usize = 16;
const MAX_PROVIDER_ENVIRONMENT_VALUE_BYTES: usize = 16 * 1024;
const LEGACY_PROVIDER_EXECUTABLE: &str = "codex";
const LEGACY_PROVIDER_ENVIRONMENT: &str = "OPENAI_API_KEY";
const IN_GUEST_PORT_PROXY_SCRIPT: &str = r#"set -eu
docker_bin="$1"
container_id="$2"
proxy_name="$3"
host_port="$4"
runtime_port="$5"
network_id=$("$docker_bin" inspect -f '{{range .NetworkSettings.Networks}}{{.NetworkID}}{{end}}' "$container_id" | head -n 1)
target_host=$("$docker_bin" inspect -f '{{.Name}}' "$container_id")
target_host=${target_host#/}
"$docker_bin" rm -f "$proxy_name" >/dev/null 2>&1 || true
exec "$docker_bin" run -d --name "$proxy_name" --restart unless-stopped --network "$network_id" -p "127.0.0.1:${host_port}:${runtime_port}" alpine/socat -dd "TCP-LISTEN:${runtime_port},fork,reuseaddr" "TCP:${target_host}:${runtime_port}""#;
const PROVIDER_STATE_VERSION: &str = "1";
const LEGACY_REDACTED_ENVIRONMENT: [&str; 1] = [LEGACY_PROVIDER_ENVIRONMENT];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InGuestTunnelPlacement {
    Outer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InGuestLeaseRecord {
    pub lease_id: String,
    pub scope: String,
    pub consumer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InGuestRuntimeMetadata {
    pub run_id: String,
    pub assignment_lease_id: String,
    pub outer_runtime_id: String,
    pub repository_revision: String,
    pub task_branch: String,
    pub tunnel_placement: InGuestTunnelPlacement,
    /// Project Docker is disabled until a task-scoped daemon can be provided without exposing the
    /// supervisor daemon or other assignment containers.
    pub project_docker: String,
    pub leases: Vec<InGuestLeaseRecord>,
    /// Provider-private state contains cleanup paths. Public lease records never contain them.
    pub state_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct InGuestFacadePlan {
    manifest_path: PathBuf,
    tunnel_placement: InGuestTunnelPlacement,
    published_ports: Vec<RuntimePort>,
    mounts: Vec<InGuestMount>,
}

impl InGuestFacadePlan {
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    pub fn published_ports(&self) -> &[RuntimePort] {
        &self.published_ports
    }

    pub fn tunnel_placement(&self) -> InGuestTunnelPlacement {
        self.tunnel_placement
    }

    pub fn mounts(&self) -> impl Iterator<Item = (&Path, &Path)> {
        self.mounts
            .iter()
            .filter_map(|mount| match (&mount.scope, &mount.target) {
                (LeaseScope::ProjectEnvironment | LeaseScope::ProviderEnvironment, _) => None,
                (_, MaterializationTarget::File(target)) => {
                    Some((mount.source.as_path(), target.as_path()))
                }
                (_, MaterializationTarget::Environment(_)) => None,
            })
    }

    pub fn project_environment(&self) -> Option<(&Path, &str)> {
        self.mounts
            .iter()
            .find(|mount| mount.scope == LeaseScope::ProjectEnvironment)
            .map(|mount| (mount.source.as_path(), mount.consumer.as_str()))
    }
}

#[derive(Debug, Clone)]
struct InGuestMount {
    source: PathBuf,
    target: MaterializationTarget,
    sha256: String,
    scope: LeaseScope,
    consumer: String,
}

#[derive(Debug, Clone)]
enum MaterializationTarget {
    File(PathBuf),
    Environment(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignmentManifest {
    version: String,
    run_id: String,
    lease_id: String,
    outer_runtime_id: String,
    workspace: PathBuf,
    repository: AssignedRepository,
    task_branch: String,
    tunnel_placement: InGuestTunnelPlacement,
    #[serde(default)]
    published_ports: Vec<RuntimePort>,
    #[serde(default)]
    leases: Vec<AssignedLease>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignedRepository {
    path: PathBuf,
    revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignedLease {
    lease_id: String,
    scope: LeaseScope,
    consumer: String,
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    inherited_environment: Vec<String>,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    materializations: Vec<AssignedMaterialization>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum LeaseScope {
    ModelIdentity,
    SourceControlIdentity,
    ProjectEnvironment,
    ProviderEnvironment,
    PlatformTunnel,
}

impl LeaseScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::ModelIdentity => "model-identity",
            Self::SourceControlIdentity => "source-control-identity",
            Self::ProjectEnvironment => "project-environment",
            Self::ProviderEnvironment => "provider-environment",
            Self::PlatformTunnel => "platform-tunnel",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignedMaterialization {
    source_path: PathBuf,
    #[serde(default)]
    target_path: Option<PathBuf>,
    #[serde(default)]
    environment_name: Option<String>,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderState {
    version: String,
    manifest_path: PathBuf,
    worktree_path: PathBuf,
    #[serde(default)]
    workspace_paths: Vec<String>,
    config_path: PathBuf,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    outer_runtime_id: Option<String>,
    materializations: Vec<StateMaterialization>,
    #[serde(default)]
    proxy_names: Vec<String>,
    #[serde(default)]
    compose_projects: Vec<String>,
    #[serde(default)]
    container_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StateMaterialization {
    source_path: PathBuf,
    sha256: String,
}

#[derive(Debug, Default)]
struct OwnedDockerIdentity {
    container_ids: BTreeSet<String>,
    compose_projects: BTreeSet<String>,
}

pub(super) struct InGuestRuntimeProvider {
    devcontainer: PathBuf,
    docker: PathBuf,
}

impl InGuestRuntimeProvider {
    pub(super) fn new() -> Result<Self> {
        let devcontainer = resolve_binary("BRANCHBOX_DEVCONTAINER_PATH", "devcontainer")?;
        let docker = resolve_binary("BRANCHBOX_DOCKER_PATH", "docker")?;
        Ok(Self {
            devcontainer,
            docker,
        })
    }

    fn command(&self, binary: &Path) -> Command {
        let mut command = Command::new(binary);
        for name in LEGACY_REDACTED_ENVIRONMENT {
            command.env_remove(name);
        }
        command
    }

    fn devcontainer_output(&self, args: &[&str], worktree: Option<&Path>) -> Result<Output> {
        let mut command = self.command(&self.devcontainer);
        command.args(args);
        if let Some(worktree) = worktree {
            command.current_dir(worktree);
        }
        command.output().map_err(|err| {
            Error::validation(format!(
                "Failed to execute in-guest Dev Containers CLI '{}': {err}",
                self.devcontainer.display()
            ))
        })
    }

    fn docker_output(&self, args: &[&str]) -> Result<Output> {
        self.command(&self.docker)
            .args(args)
            .output()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute in-guest Docker CLI '{}': {err}",
                    self.docker.display()
                ))
            })
    }

    fn checked_devcontainer(
        &self,
        operation: &str,
        args: &[&str],
        worktree: Option<&Path>,
    ) -> Result<Output> {
        let output = self.devcontainer_output(args, worktree)?;
        if output.status.success() {
            return Ok(output);
        }
        Err(Error::validation(format!(
            "in-guest {operation} failed: {}",
            bounded_failure(&output.stderr)
        )))
    }

    fn checked_docker(&self, operation: &str, args: &[&str]) -> Result<Output> {
        let output = self.docker_output(args)?;
        if output.status.success() {
            return Ok(output);
        }
        Err(Error::validation(format!(
            "in-guest {operation} failed: {}",
            bounded_failure(&output.stderr)
        )))
    }

    fn config_path(worktree_path: &Path) -> Result<PathBuf> {
        let generated = worktree_path.join(".devcontainer/.devcontainer.json");
        if generated.is_file() {
            return Ok(generated);
        }
        let (_, source) = DevcontainerConfig::load(worktree_path).map_err(|err| {
            Error::validation(format!(
                "Could not discover in-guest devcontainer config: {err}"
            ))
        })?;
        Ok(source)
    }

    fn start_devcontainer(&self, worktree_path: &Path, config_path: &Path) -> Result<String> {
        self.verify_resolved_configuration(worktree_path, config_path)?;
        let worktree = worktree_path.to_string_lossy();
        let config = config_path.to_string_lossy();
        let output = self.checked_devcontainer(
            "devcontainer startup",
            &[
                "up",
                "--workspace-folder",
                worktree.as_ref(),
                "--config",
                config.as_ref(),
                "--log-format",
                "json",
            ],
            Some(worktree_path),
        ).map_err(|err| {
            Error::validation(format!(
                "{err}. In-guest startup strips host identity, platform secret hooks, ambient env files, and supervisor Docker access; repository primary commands and container lifecycle hooks must tolerate that boundary"
            ))
        })?;
        parse_container_id(&output)
    }

    fn verify_resolved_configuration(
        &self,
        worktree_path: &Path,
        config_path: &Path,
    ) -> Result<()> {
        let output = self.checked_devcontainer(
            "resolved configuration inspection",
            &[
                "read-configuration",
                "--workspace-folder",
                &worktree_path.to_string_lossy(),
                "--config",
                &config_path.to_string_lossy(),
                "--include-merged-configuration",
                "--log-format",
                "json",
            ],
            Some(worktree_path),
        )?;
        validate_resolved_configuration(&output.stdout)
    }

    fn probe(&self, worktree_path: &Path, config_path: &Path) -> Result<bool> {
        let output = self.devcontainer_output(
            &[
                "exec",
                "--workspace-folder",
                &worktree_path.to_string_lossy(),
                "--config",
                &config_path.to_string_lossy(),
                "true",
            ],
            Some(worktree_path),
        )?;
        Ok(output.status.success())
    }

    fn discover_compose_projects(&self, worktree_path: &Path) -> Result<Vec<String>> {
        let mut paths = BTreeSet::from([worktree_path.to_string_lossy().into_owned()]);
        if let Ok(canonical) = fs::canonicalize(worktree_path) {
            paths.insert(canonical.to_string_lossy().into_owned());
        }
        let mut projects = BTreeSet::new();
        for workspace in paths {
            let filter = format!("label=devcontainer.local_folder={workspace}");
            let output = self.checked_docker(
                "Compose project discovery",
                &[
                    "ps",
                    "-a",
                    "--filter",
                    &filter,
                    "--format",
                    "{{.Label \"com.docker.compose.project\"}}",
                ],
            )?;
            projects.extend(
                output_lines(&output.stdout)
                    .into_iter()
                    .filter(|project| is_compose_project_name(project)),
            );
        }
        Ok(projects.into_iter().collect())
    }

    fn discover_owned_docker_identity(&self, state: &ProviderState) -> Result<OwnedDockerIdentity> {
        let mut identity = OwnedDockerIdentity {
            container_ids: BTreeSet::new(),
            compose_projects: state.compose_projects.iter().cloned().collect(),
        };
        let mut workspace_paths: BTreeSet<String> = state.workspace_paths.iter().cloned().collect();
        workspace_paths.extend(workspace_candidates(&state.worktree_path));

        for workspace in &workspace_paths {
            let filter = format!("label=devcontainer.local_folder={workspace}");
            let output = self.checked_docker(
                "devcontainer cleanup identity discovery",
                &[
                    "ps",
                    "-a",
                    "--filter",
                    &filter,
                    "--format",
                    "{{.ID}}\t{{.Label \"com.docker.compose.project\"}}",
                ],
            )?;
            for line in output_lines(&output.stdout) {
                let mut fields = line.split('\t');
                if let Some(container) = fields.next().filter(|value| !value.is_empty()) {
                    identity.container_ids.insert(container.to_string());
                }
                if let Some(project) = fields.next().filter(|value| is_compose_project_name(value))
                {
                    identity.compose_projects.insert(project.to_string());
                }
            }
        }

        // A Compose dependency can be created before the primary devcontainer receives its
        // devcontainer.local_folder label. Compose records the exact working directory and config
        // files on every service, so use those labels to recover the project without guessing from
        // container names.
        let output = self.checked_docker(
            "partial Compose identity discovery",
            &[
                "ps",
                "-a",
                "--filter",
                "label=com.docker.compose.project",
                "--format",
                "{{.ID}}\t{{.Label \"com.docker.compose.project\"}}\t{{.Label \"com.docker.compose.project.working_dir\"}}\t{{.Label \"com.docker.compose.project.config_files\"}}",
            ],
        )?;
        for line in output_lines(&output.stdout) {
            let mut fields = line.splitn(4, '\t');
            let container = fields.next().unwrap_or_default();
            let project = fields.next().unwrap_or_default();
            let working_dir = fields.next().unwrap_or_default();
            let config_files = fields.next().unwrap_or_default();
            if !is_compose_project_name(project)
                || !compose_labels_belong_to_workspace(working_dir, config_files, &workspace_paths)
            {
                continue;
            }
            if !container.is_empty() {
                identity.container_ids.insert(container.to_string());
            }
            identity.compose_projects.insert(project.to_string());
        }

        if let Some(container_id) = state.container_id.as_ref() {
            identity.container_ids.insert(container_id.clone());
        }
        Ok(identity)
    }

    fn verify_untrusted_boundary(
        &self,
        container_id: &str,
        metadata: &RuntimeMetadata,
    ) -> Result<()> {
        let output = self.checked_docker(
            "devcontainer boundary inspection",
            &["inspect", container_id],
        )?;
        let (allowed_sources, forbidden_environment) =
            if let Some(identity) = metadata.in_guest.as_ref() {
                let state = Self::read_state(&identity.state_path)?;
                let assignment = load_assignment(&state.manifest_path)?;
                let environment_sources = assignment.provider_environment_sources();
                let allowed_sources = state
                    .materializations
                    .into_iter()
                    .map(|materialization| materialization.source_path)
                    .filter(|source| !environment_sources.contains(source))
                    .collect::<BTreeSet<_>>();
                (allowed_sources, assignment.provider_environment_names())
            } else {
                (BTreeSet::new(), BTreeSet::new())
            };
        validate_container_inspection(&output.stdout, &allowed_sources, &forbidden_environment)
    }

    fn proxy_name(run_id: &str, runtime_port: u16) -> String {
        let identity: String = run_id
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .take(24)
            .collect();
        format!("branchbox-in-guest-{identity}-port-{runtime_port}")
    }

    fn reconcile_port_proxy(
        &self,
        run_id: &str,
        container_id: &str,
        port: RuntimePort,
    ) -> Result<String> {
        let proxy_name = Self::proxy_name(run_id, port.runtime);
        let mut command = self.command(Path::new("sh"));
        let output = command
            .args(["-c", IN_GUEST_PORT_PROXY_SCRIPT, "branchbox-in-guest-proxy"])
            .arg(&self.docker)
            .args([
                container_id,
                &proxy_name,
                &port.host.to_string(),
                &port.runtime.to_string(),
            ])
            .output()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to reconcile an in-guest published-port proxy: {err}"
                ))
            })?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "in-guest published-port reconciliation failed: {}",
                bounded_failure(&output.stderr)
            )));
        }
        Ok(proxy_name)
    }

    fn write_state(path: &Path, state: &ProviderState) -> Result<()> {
        let parent = path.parent().ok_or_else(|| {
            Error::validation("In-guest provider state path has no parent directory")
        })?;
        fs::create_dir_all(parent)?;
        let rendered = serde_json::to_vec_pretty(state)?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, rendered)?;
        set_owner_only(&temporary)?;
        fs::rename(&temporary, path)?;
        set_owner_only(path)?;
        Ok(())
    }

    fn read_state(path: &Path) -> Result<ProviderState> {
        let path = validate_private_regular_file(path, "provider state")?;
        let source = fs::read(path)?;
        let state: ProviderState = serde_json::from_slice(&source)?;
        if state.version != PROVIDER_STATE_VERSION {
            return Err(Error::validation(format!(
                "Unsupported in-guest provider state version '{}'",
                state.version
            )));
        }
        Ok(state)
    }

    fn update_state_after_start(
        &self,
        metadata: &RuntimeMetadata,
        container_id: &str,
        proxy_names: Vec<String>,
        compose_projects: Vec<String>,
    ) -> Result<()> {
        let in_guest = metadata.in_guest.as_ref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing assignment identity")
        })?;
        let mut state = Self::read_state(&in_guest.state_path)?;
        state.container_id = Some(container_id.to_string());
        state.proxy_names.extend(proxy_names);
        state.proxy_names.sort();
        state.proxy_names.dedup();
        state.compose_projects.extend(compose_projects);
        state.compose_projects.sort();
        state.compose_projects.dedup();
        Self::write_state(&in_guest.state_path, &state)
    }

    fn record_partial_start_identity(
        &self,
        metadata: &RuntimeMetadata,
        worktree_path: &Path,
    ) -> Result<()> {
        let in_guest = metadata.in_guest.as_ref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing assignment identity")
        })?;
        let mut state = Self::read_state(&in_guest.state_path)?;
        if state.container_id.is_none() {
            state.container_id = metadata.container_id.clone();
        }
        state
            .compose_projects
            .extend(self.discover_compose_projects(worktree_path)?);
        state.compose_projects.sort();
        state.compose_projects.dedup();
        Self::write_state(&in_guest.state_path, &state)
    }

    fn remove_owned_docker_resources(&self, state: &ProviderState) -> Result<Vec<RuntimeResidue>> {
        for proxy in &state.proxy_names {
            let _ = self.docker_output(&["rm", "-f", proxy]);
        }
        let identity = self.discover_owned_docker_identity(state)?;
        for container in identity.container_ids {
            let _ = self.docker_output(&["rm", "-f", &container]);
        }

        for project in &identity.compose_projects {
            for (_kind, list_args, remove_prefix) in [
                (
                    "container",
                    vec!["ps", "-a", "--format", "{{.ID}}"],
                    vec!["rm", "-f"],
                ),
                (
                    "network",
                    vec!["network", "ls", "--format", "{{.ID}}"],
                    vec!["network", "rm"],
                ),
                (
                    "volume",
                    vec!["volume", "ls", "--format", "{{.Name}}"],
                    vec!["volume", "rm"],
                ),
            ] {
                let filter = format!("label=com.docker.compose.project={project}");
                let mut args = list_args.clone();
                let insert_at = 2;
                args.insert(insert_at, "--filter");
                args.insert(insert_at + 1, &filter);
                let output = self.checked_docker("Compose cleanup discovery", &args)?;
                for identifier in output_lines(&output.stdout) {
                    let mut remove = remove_prefix.clone();
                    remove.push(&identifier);
                    let _ = self.docker_output(&remove);
                }
            }
        }
        self.inspect_residue(state, &identity.compose_projects)
    }

    fn inspect_residue(
        &self,
        state: &ProviderState,
        compose_projects: &BTreeSet<String>,
    ) -> Result<Vec<RuntimeResidue>> {
        let mut residue = Vec::new();
        for (kind, args) in [
            ("container", vec!["ps", "-a", "--format", "{{.Names}}"]),
            ("network", vec!["network", "ls", "--format", "{{.Name}}"]),
            ("volume", vec!["volume", "ls", "--format", "{{.Name}}"]),
        ] {
            let mut identifiers = BTreeSet::new();
            for project in compose_projects {
                let filter = format!("label=com.docker.compose.project={project}");
                let mut filtered = args.clone();
                let insert_at = 2;
                filtered.insert(insert_at, "--filter");
                filtered.insert(insert_at + 1, &filter);
                identifiers.extend(output_lines(
                    &self
                        .checked_docker("Compose residue inspection", &filtered)?
                        .stdout,
                ));
            }
            if !identifiers.is_empty() {
                residue.push(RuntimeResidue {
                    kind: kind.to_string(),
                    identifiers: identifiers.into_iter().collect(),
                });
            }
        }
        let mut workspace_containers = BTreeSet::new();
        let mut workspace_paths: BTreeSet<String> = state.workspace_paths.iter().cloned().collect();
        workspace_paths.extend(workspace_candidates(&state.worktree_path));
        for workspace in workspace_paths {
            let filter = format!("label=devcontainer.local_folder={workspace}");
            workspace_containers.extend(output_lines(
                &self
                    .checked_docker(
                        "devcontainer residue inspection",
                        &["ps", "-a", "--filter", &filter, "--format", "{{.Names}}"],
                    )?
                    .stdout,
            ));
        }
        if !workspace_containers.is_empty() {
            residue.push(RuntimeResidue {
                kind: "container".to_string(),
                identifiers: workspace_containers.into_iter().collect(),
            });
        }
        let proxies: Vec<_> = state
            .proxy_names
            .iter()
            .filter(|name| {
                self.docker_output(&["inspect", name])
                    .is_ok_and(|output| output.status.success())
            })
            .cloned()
            .collect();
        if !proxies.is_empty() {
            residue.push(RuntimeResidue {
                kind: "port-proxy".to_string(),
                identifiers: proxies,
            });
        }
        Ok(residue)
    }

    fn erase_materializations(&self, state: &ProviderState, residue: &mut Vec<RuntimeResidue>) {
        let mut remaining = Vec::new();
        for materialization in &state.materializations {
            match fs::remove_file(&materialization.source_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => remaining.push(materialization.source_path.display().to_string()),
            }
        }
        if !remaining.is_empty() {
            residue.push(RuntimeResidue {
                kind: "lease-materialization".to_string(),
                identifiers: remaining,
            });
        }
    }

    pub(super) fn exec_provider_interactive(
        &self,
        metadata: &RuntimeMetadata,
        provider: &str,
        inherited_environment: &[String],
        args: &[String],
    ) -> Result<i32> {
        let provider_environment =
            self.provider_execution_environment(metadata, provider, inherited_environment)?;
        for name in inherited_environment {
            if std::env::var_os(name).is_none() {
                return Err(Error::validation(format!(
                    "Allowlisted environment '{name}' is not present in the supervisor process"
                )));
            }
        }
        let container_id = metadata.container_id.as_deref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing the devcontainer ID")
        })?;
        self.verify_untrusted_boundary(container_id, metadata)?;
        let workspace = metadata.workspace_folder.as_deref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing the container workspace")
        })?;
        let user = metadata.container_user.as_deref().unwrap_or("root");
        let mut command = Command::new(&self.docker);
        apply_provider_process_environment(
            &mut command,
            inherited_environment,
            &provider_environment,
        )?;
        command.args([
            "exec",
            "--interactive",
            "--user",
            user,
            "--workdir",
            workspace,
        ]);
        for name in inherited_environment {
            command.args(["--env", name]);
        }
        for binding in &provider_environment {
            command.args(["--env", &binding.name]);
        }
        command.arg(container_id).arg(provider).args(args);
        let status = command.status().map_err(|err| {
            Error::validation(format!(
                "Failed to execute the managed provider in the devcontainer: {err}"
            ))
        })?;
        Ok(status.code().unwrap_or(-1))
    }

    fn provider_execution_environment(
        &self,
        metadata: &RuntimeMetadata,
        provider: &str,
        inherited_environment: &[String],
    ) -> Result<Vec<ProviderEnvironmentValue>> {
        let in_guest = metadata.in_guest.as_ref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing assignment identity")
        })?;
        let state = Self::read_state(&in_guest.state_path)?;
        let assignment = load_assignment(&state.manifest_path)?;
        let consumer = assignment.authorize_provider_execution(provider, inherited_environment)?;
        assignment
            .provider_environment(consumer)
            .map(|mount| {
                let MaterializationTarget::Environment(name) = &mount.target else {
                    return Err(Error::validation(
                        "Provider environment binding target is invalid",
                    ));
                };
                Ok(ProviderEnvironmentValue {
                    name: name.clone(),
                    value: read_provider_environment_value(&mount.source, &mount.sha256)?,
                })
            })
            .collect()
    }
}

struct ProviderEnvironmentValue {
    name: String,
    value: String,
}

fn apply_provider_process_environment(
    command: &mut Command,
    inherited_environment: &[String],
    provider_environment: &[ProviderEnvironmentValue],
) -> Result<()> {
    command.env_clear().env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    for name in inherited_environment {
        let value = std::env::var_os(name).ok_or_else(|| {
            Error::validation(format!(
                "Allowlisted environment '{name}' is not present in the supervisor process"
            ))
        })?;
        command.env(name, value);
    }
    for binding in provider_environment {
        command.env(&binding.name, &binding.value);
    }
    Ok(())
}

impl Drop for ProviderEnvironmentValue {
    fn drop(&mut self) {
        // Replacing every UTF-8 byte with NUL preserves String's UTF-8 invariant while erasing
        // the allocation before it is released.
        unsafe { self.value.as_bytes_mut() }.fill(0);
        self.value.clear();
    }
}

impl RuntimeProvider for InGuestRuntimeProvider {
    fn kind(&self) -> RuntimeProviderKind {
        RuntimeProviderKind::InGuest
    }

    fn validate(&self) -> Result<()> {
        self.checked_devcontainer("preflight", &["--version"], None)?;
        self.checked_docker("preflight", &["info"])?;
        let compose =
            self.checked_docker("Compose preflight", &["compose", "version", "--short"])?;
        ensure_compose_override_version(&String::from_utf8_lossy(&compose.stdout))
    }

    fn exists(&self, metadata: &RuntimeMetadata) -> Result<bool> {
        let Some(container_id) = metadata.container_id.as_deref() else {
            return Ok(false);
        };
        Ok(self
            .docker_output(&["inspect", container_id])?
            .status
            .success())
    }

    fn environment_ready(&self, metadata: &RuntimeMetadata, worktree_path: &Path) -> Result<bool> {
        let config = metadata
            .config_path
            .clone()
            .unwrap_or(Self::config_path(worktree_path)?);
        self.probe(worktree_path, &config)
    }

    fn prepare(&self, context: &RuntimeContext<'_>) -> Result<RuntimeMetadata> {
        let manifest_path = context.runtime_manifest_path.ok_or_else(|| {
            Error::validation(
                "Runtime 'in-guest' requires --runtime-manifest with an orchestrator-owned assignment path",
            )
        })?;
        let assignment = load_assignment(manifest_path)?;
        if assignment.manifest.published_ports != context.published_ports {
            return Err(Error::validation(
                "In-guest published ports changed after assignment validation",
            ));
        }
        let config_path = Self::config_path(context.worktree_path)?;
        let (config, _) = DevcontainerConfig::load_from_path(&config_path).map_err(|err| {
            Error::validation(format!("Could not read in-guest runtime config: {err}"))
        })?;
        let workspace_folder = effective_workspace_folder(&config, context.worktree_path);
        let container_user = effective_container_user(&config);
        let compose_projects = deterministic_compose_projects(
            context.runtime_name,
            context.worktree_path,
            &config,
            &config_path,
        );
        let workspace_paths = workspace_candidates(context.worktree_path)
            .into_iter()
            .collect();
        let state_path = assignment
            .manifest
            .repository
            .path
            .join(".branchbox/runtime/in-guest")
            .join(format!(
                "{}.json",
                safe_identity(&assignment.manifest.run_id)?
            ));
        let state = ProviderState {
            version: PROVIDER_STATE_VERSION.to_string(),
            manifest_path: assignment.manifest_path.clone(),
            worktree_path: context.worktree_path.to_path_buf(),
            workspace_paths,
            config_path: config_path.clone(),
            run_id: Some(assignment.manifest.run_id.clone()),
            outer_runtime_id: Some(assignment.manifest.outer_runtime_id.clone()),
            materializations: assignment
                .mounts
                .iter()
                .map(|mount| StateMaterialization {
                    source_path: mount.source.clone(),
                    sha256: mount.sha256.clone(),
                })
                .collect(),
            proxy_names: context
                .published_ports
                .iter()
                .map(|port| Self::proxy_name(&assignment.manifest.run_id, port.runtime))
                .collect(),
            compose_projects: compose_projects.into_iter().collect(),
            container_id: None,
        };
        Self::write_state(&state_path, &state)?;

        Ok(RuntimeMetadata {
            provider: RuntimeProviderKind::InGuest,
            runtime_id: Some(assignment.manifest.outer_runtime_id.clone()),
            published_ports: context.published_ports.to_vec(),
            container_id: None,
            workspace_folder: Some(workspace_folder),
            container_user: Some(container_user),
            config_path: Some(config_path),
            in_guest: Some(InGuestRuntimeMetadata {
                run_id: assignment.manifest.run_id,
                assignment_lease_id: assignment.manifest.lease_id,
                outer_runtime_id: assignment.manifest.outer_runtime_id,
                repository_revision: assignment.manifest.repository.revision,
                task_branch: assignment.manifest.task_branch,
                tunnel_placement: assignment.manifest.tunnel_placement,
                project_docker: "disabled".to_string(),
                leases: assignment.leases,
                state_path,
            }),
            version: None,
        })
    }

    fn start_environment(
        &self,
        context: &RuntimeContext<'_>,
        metadata: &mut RuntimeMetadata,
    ) -> Result<()> {
        let config = metadata
            .config_path
            .clone()
            .unwrap_or(Self::config_path(context.worktree_path)?);
        let result = (|| {
            let container_id = self.start_devcontainer(context.worktree_path, &config)?;
            // Persist the primary identity before any later boundary/probe/proxy check can fail.
            metadata.container_id = Some(container_id.clone());
            self.record_partial_start_identity(metadata, context.worktree_path)?;
            self.verify_untrusted_boundary(&container_id, metadata)?;
            if !self.probe(context.worktree_path, &config)? {
                return Err(Error::validation(
                    "In-guest devcontainer did not remain ready after startup. Repository primary commands and container-side lifecycle hooks must succeed without host SSH/1Password state; supply project configuration through an explicit project-environment materialization or fix the source devcontainer convention",
                ));
            }
            let run_id = metadata
                .in_guest
                .as_ref()
                .map(|identity| identity.run_id.clone())
                .ok_or_else(|| Error::validation("In-guest assignment identity is missing"))?;
            let mut proxies = Vec::new();
            for port in &metadata.published_ports {
                proxies.push(self.reconcile_port_proxy(&run_id, &container_id, *port)?);
            }
            let projects = self.discover_compose_projects(context.worktree_path)?;
            self.update_state_after_start(metadata, &container_id, proxies, projects)
        })();
        if result.is_err() {
            if let Err(discovery_err) =
                self.record_partial_start_identity(metadata, context.worktree_path)
            {
                tracing::warn!(
                    "Failed to persist partial in-guest startup identity: {}",
                    discovery_err
                );
            }
        }
        result
    }

    fn exec(
        &self,
        metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<RuntimeExecResult> {
        if command.is_empty() {
            return Err(Error::validation("Runtime command cannot be empty"));
        }
        let config = metadata
            .config_path
            .clone()
            .unwrap_or(Self::config_path(worktree_path)?);
        let container_id = self.start_devcontainer(worktree_path, &config)?;
        self.verify_untrusted_boundary(&container_id, metadata)?;
        if let Some(identity) = metadata.in_guest.as_ref() {
            for port in &metadata.published_ports {
                self.reconcile_port_proxy(&identity.run_id, &container_id, *port)?;
            }
        }
        let mut process = self.command(&self.devcontainer);
        process
            .args([
                "exec",
                "--workspace-folder",
                &worktree_path.to_string_lossy(),
                "--config",
                &config.to_string_lossy(),
                "/bin/sh",
                "-lc",
                "exec \"$@\"",
                "branchbox-in-guest-exec",
            ])
            .args(command)
            .current_dir(worktree_path);
        process.output().map(exec_result).map_err(|err| {
            Error::validation(format!("Failed to execute command in devcontainer: {err}"))
        })
    }

    fn exec_interactive(
        &self,
        metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<i32> {
        if command.is_empty() {
            return Err(Error::validation("Runtime command cannot be empty"));
        }
        let config = metadata
            .config_path
            .clone()
            .unwrap_or(Self::config_path(worktree_path)?);
        let container_id = self.start_devcontainer(worktree_path, &config)?;
        self.verify_untrusted_boundary(&container_id, metadata)?;
        let mut process = self.command(&self.devcontainer);
        let status = process
            .args([
                "exec",
                "--workspace-folder",
                &worktree_path.to_string_lossy(),
                "--config",
                &config.to_string_lossy(),
                "/bin/sh",
                "-lc",
                "exec \"$@\"",
                "branchbox-in-guest-exec",
            ])
            .args(command)
            .current_dir(worktree_path)
            .status()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute interactive devcontainer command: {err}"
                ))
            })?;
        Ok(status.code().unwrap_or(-1))
    }

    fn exec_provider_interactive(
        &self,
        metadata: &RuntimeMetadata,
        provider: &str,
        inherited_environment: &[String],
        args: &[String],
    ) -> Result<i32> {
        InGuestRuntimeProvider::exec_provider_interactive(
            self,
            metadata,
            provider,
            inherited_environment,
            args,
        )
    }

    fn destroy(&self, metadata: &RuntimeMetadata) -> Result<RuntimeTeardownReport> {
        let identity = metadata.in_guest.as_ref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing assignment identity")
        })?;
        let state = match Self::read_state(&identity.state_path) {
            Ok(state) => state,
            Err(_err) if !identity.state_path.exists() => {
                return Ok(RuntimeTeardownReport::unverified(
                    self.kind(),
                    metadata.runtime_id.clone(),
                    format!(
                        "provider state is missing at {}",
                        identity.state_path.display()
                    ),
                ))
            }
            Err(err) => return Err(err),
        };
        let mut residue = self.remove_owned_docker_resources(&state)?;
        self.erase_materializations(&state, &mut residue);
        if residue.is_empty() {
            if let Err(err) = fs::remove_file(&identity.state_path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    residue.push(RuntimeResidue {
                        kind: "provider-state".to_string(),
                        identifiers: vec![identity.state_path.display().to_string()],
                    });
                }
            }
        } else {
            residue.push(RuntimeResidue {
                kind: "provider-state".to_string(),
                identifiers: vec![identity.state_path.display().to_string()],
            });
        }
        Ok(RuntimeTeardownReport {
            provider: self.kind(),
            runtime_id: metadata.runtime_id.clone(),
            verified: true,
            residue_free: residue.is_empty(),
            residue,
        })
    }
}

pub(crate) fn recover_runtime_metadata(
    repo_root: &Path,
    worktree_path: &Path,
) -> Result<Option<RuntimeMetadata>> {
    let state_dir = repo_root.join(".branchbox/runtime/in-guest");
    let entries = match fs::read_dir(&state_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let requested_paths = workspace_candidates(worktree_path);
    for entry in entries {
        let entry = entry?;
        let state_path = entry.path();
        if state_path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
        {
            continue;
        }
        let state = match InGuestRuntimeProvider::read_state(&state_path) {
            Ok(state) => state,
            Err(err) => {
                tracing::warn!(
                    "Ignoring invalid in-guest provider state '{}': {}",
                    state_path.display(),
                    err
                );
                continue;
            }
        };
        let mut state_paths: BTreeSet<String> = state.workspace_paths.iter().cloned().collect();
        state_paths.extend(workspace_candidates(&state.worktree_path));
        if requested_paths.is_disjoint(&state_paths) {
            continue;
        }
        let run_id = state.run_id.clone().unwrap_or_else(|| {
            state_path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("recovered-failed-start")
                .to_string()
        });
        let outer_runtime_id = state
            .outer_runtime_id
            .clone()
            .unwrap_or_else(|| "recovered-outer-runtime".to_string());
        return Ok(Some(RuntimeMetadata {
            provider: RuntimeProviderKind::InGuest,
            runtime_id: Some(outer_runtime_id.clone()),
            published_ports: Vec::new(),
            container_id: state.container_id.clone(),
            workspace_folder: None,
            container_user: None,
            config_path: Some(state.config_path.clone()),
            in_guest: Some(InGuestRuntimeMetadata {
                run_id,
                assignment_lease_id: "recovered-failed-start".to_string(),
                outer_runtime_id,
                repository_revision: String::new(),
                task_branch: String::new(),
                tunnel_placement: InGuestTunnelPlacement::Outer,
                project_docker: "disabled".to_string(),
                leases: Vec::new(),
                state_path,
            }),
            version: None,
        }));
    }
    Ok(None)
}

#[derive(Debug)]
struct LoadedAssignment {
    manifest_path: PathBuf,
    manifest: AssignmentManifest,
    mounts: Vec<InGuestMount>,
    leases: Vec<InGuestLeaseRecord>,
}

impl LoadedAssignment {
    fn authorize_provider_execution(
        &self,
        provider: &str,
        inherited_environment: &[String],
    ) -> Result<&str> {
        validate_label(provider, "provider")?;
        if self.manifest.version == LEGACY_MANIFEST_VERSION {
            if provider != LEGACY_PROVIDER_EXECUTABLE
                || inherited_environment != [LEGACY_PROVIDER_ENVIRONMENT]
            {
                return Err(Error::validation(
                    "Legacy provider execution does not match the version 1 contract",
                ));
            }
            let identity = self
                .manifest
                .leases
                .iter()
                .find(|lease| {
                    lease.scope == LeaseScope::ModelIdentity && lease.consumer == provider
                })
                .ok_or_else(|| {
                    Error::validation(
                        "Provider execution requires one exact model-identity consumer binding",
                    )
                })?;
            return Ok(identity.consumer.as_str());
        }
        let identities = self
            .manifest
            .leases
            .iter()
            .filter(|lease| {
                lease.scope == LeaseScope::ModelIdentity
                    && lease.executable.as_deref() == Some(provider)
            })
            .collect::<Vec<_>>();
        if identities.len() != 1 {
            return Err(Error::validation(
                "Provider execution requires one exact model-identity consumer binding",
            ));
        }
        if identities[0].inherited_environment != inherited_environment {
            return Err(Error::validation(
                "Inherited provider environment differs from the managed assignment",
            ));
        }
        Ok(identities[0].consumer.as_str())
    }

    fn provider_environment<'a>(
        &'a self,
        provider: &'a str,
    ) -> impl Iterator<Item = &'a InGuestMount> + 'a {
        self.mounts.iter().filter(move |mount| {
            mount.scope == LeaseScope::ProviderEnvironment && mount.consumer == provider
        })
    }

    fn provider_environment_sources(&self) -> BTreeSet<PathBuf> {
        self.mounts
            .iter()
            .filter(|mount| mount.scope == LeaseScope::ProviderEnvironment)
            .map(|mount| mount.source.clone())
            .collect()
    }

    fn provider_environment_names(&self) -> BTreeSet<String> {
        let mut names = self
            .manifest
            .leases
            .iter()
            .flat_map(|lease| lease.inherited_environment.iter().cloned())
            .collect::<BTreeSet<_>>();
        names.extend(self.mounts.iter().filter_map(|mount| {
            if let MaterializationTarget::Environment(name) = &mount.target {
                Some(name.clone())
            } else {
                None
            }
        }));
        if self.manifest.version == LEGACY_MANIFEST_VERSION
            && self
                .manifest
                .leases
                .iter()
                .any(|lease| lease.scope == LeaseScope::ModelIdentity)
        {
            names.insert(LEGACY_PROVIDER_ENVIRONMENT.to_string());
        }
        names
    }
}

pub fn load_in_guest_facade_plan(
    manifest_path: &Path,
    repo_root: &Path,
    workspace_mount_path: &Path,
    worktree_path: &Path,
    task_branch: &str,
    repository_revision: &str,
) -> Result<InGuestFacadePlan> {
    let assignment = load_assignment(manifest_path)?;
    validate_assignment_paths(
        &assignment,
        repo_root,
        workspace_mount_path,
        worktree_path,
        task_branch,
        repository_revision,
    )?;
    Ok(InGuestFacadePlan {
        manifest_path: assignment.manifest_path,
        tunnel_placement: assignment.manifest.tunnel_placement,
        published_ports: assignment.manifest.published_ports,
        mounts: assignment.mounts,
    })
}

fn load_assignment(manifest_path: &Path) -> Result<LoadedAssignment> {
    let manifest_path = validate_private_regular_file(manifest_path, "assignment manifest")?;
    let manifest: AssignmentManifest = serde_json::from_slice(&fs::read(&manifest_path)?)
        .map_err(|err| Error::validation(format!("Invalid in-guest assignment manifest: {err}")))?;
    if !matches!(
        manifest.version.as_str(),
        LEGACY_MANIFEST_VERSION | MANAGED_MANIFEST_VERSION
    ) {
        return Err(Error::validation(format!(
            "Unsupported in-guest assignment version '{}'",
            manifest.version
        )));
    }
    validate_opaque_identifier(&manifest.run_id, "run_id")?;
    validate_opaque_identifier(&manifest.lease_id, "lease_id")?;
    validate_opaque_identifier(&manifest.outer_runtime_id, "outer_runtime_id")?;
    validate_git_revision(&manifest.repository.revision)?;
    validate_ports(&manifest.published_ports)?;

    let assignment_root = manifest_path.parent().ok_or_else(|| {
        Error::validation("In-guest assignment manifest must have a parent directory")
    })?;
    let has_materializations = manifest
        .leases
        .iter()
        .any(|lease| !lease.materializations.is_empty());
    let materialization_root = if has_materializations {
        Some(
            fs::canonicalize(assignment_root.join("materializations")).map_err(|err| {
                Error::validation(format!(
                    "Cannot validate assignment materialization directory: {err}"
                ))
            })?,
        )
    } else {
        None
    };
    let mut mounts = Vec::new();
    let mut leases = Vec::new();
    let mut sources = BTreeSet::new();
    let mut file_targets = BTreeSet::new();
    let mut provider_environment_targets = BTreeSet::new();
    let mut project_environment_seen = false;
    let mut provider_environment_bindings = 0_usize;
    let mut model_consumers = BTreeSet::new();
    let mut model_executables = BTreeSet::new();
    let mut provider_environment_consumers = BTreeSet::new();
    let mut lease_ids = BTreeSet::new();
    for lease in &manifest.leases {
        validate_opaque_identifier(&lease.lease_id, "lease.lease_id")?;
        validate_label(&lease.consumer, "lease.consumer")?;
        if !lease_ids.insert(lease.lease_id.clone()) {
            return Err(Error::validation(
                "Managed assignment lease identifiers must be unique",
            ));
        }
        if manifest.version == LEGACY_MANIFEST_VERSION
            && (!lease.inherited_environment.is_empty()
                || lease.executable.is_some()
                || lease.scope == LeaseScope::ProviderEnvironment)
        {
            return Err(Error::validation(
                "Version 1 assignments cannot declare provider-environment bindings",
            ));
        }
        if lease.scope == LeaseScope::ModelIdentity {
            if !model_consumers.insert(lease.consumer.clone()) || !lease.materializations.is_empty()
            {
                return Err(Error::validation(
                    "Model identities must be unique outer leases per consumer",
                ));
            }
            if manifest.version == MANAGED_MANIFEST_VERSION
                && (lease.executable.as_deref().is_none_or(|executable| {
                    validate_label(executable, "lease.executable").is_err()
                }) || lease.inherited_environment.len() > MAX_PROVIDER_ENVIRONMENT_BINDINGS
                    || lease
                        .expires_at
                        .is_none_or(|expires_at| expires_at <= Utc::now()))
            {
                return Err(Error::validation(
                    "Managed model identities require a live exact executable and at most 16 inherited bindings",
                ));
            }
            if let Some(executable) = lease.executable.as_ref() {
                if !model_executables.insert(executable.clone()) {
                    return Err(Error::validation(
                        "Managed provider executables must be unique",
                    ));
                }
            }
            if lease
                .inherited_environment
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            {
                return Err(Error::validation(
                    "Inherited provider environment must be unique and sorted",
                ));
            }
            for name in &lease.inherited_environment {
                validate_provider_environment_name(name)?;
                if !provider_environment_targets.insert((lease.consumer.clone(), name.clone())) {
                    return Err(Error::validation(
                        "Provider environment bindings must be unique per consumer",
                    ));
                }
            }
        } else if !lease.inherited_environment.is_empty() || lease.executable.is_some() {
            return Err(Error::validation(
                "Only model-identity leases may declare an executable or inherit supervisor environment",
            ));
        }
        if lease.scope == LeaseScope::SourceControlIdentity && !lease.materializations.is_empty() {
            return Err(Error::validation(
                "Source-control identity must remain an outer lease",
            ));
        }
        if lease.scope == LeaseScope::PlatformTunnel && !lease.materializations.is_empty() {
            return Err(Error::validation(
                "Outer platform-tunnel leases cannot be materialized into the devcontainer",
            ));
        }
        if lease.scope == LeaseScope::ProjectEnvironment {
            if project_environment_seen || lease.materializations.len() != 1 {
                return Err(Error::validation(
                    "In-guest assignments permit exactly one project-environment materialization",
                ));
            }
            project_environment_seen = true;
        }
        if lease.scope == LeaseScope::ProviderEnvironment {
            provider_environment_consumers.insert(lease.consumer.clone());
            provider_environment_bindings += lease.materializations.len();
            if lease.materializations.is_empty()
                || provider_environment_bindings > MAX_PROVIDER_ENVIRONMENT_BINDINGS
                || lease
                    .expires_at
                    .is_none_or(|expires_at| expires_at <= Utc::now())
            {
                return Err(Error::validation(
                    "Provider-environment leases require 1-16 live materializations",
                ));
            }
        }
        for materialization in &lease.materializations {
            let source = validate_private_regular_file(
                &materialization.source_path,
                "lease materialization",
            )?;
            if !source.starts_with(materialization_root.as_ref().expect("validated above")) {
                return Err(Error::validation(
                    "Lease materialization source escapes the assignment materializations directory",
                ));
            }
            let target = match (
                materialization.target_path.as_ref(),
                materialization.environment_name.as_ref(),
            ) {
                (Some(target), None) if lease.scope != LeaseScope::ProviderEnvironment => {
                    validate_lease_target(target)?;
                    if lease.scope == LeaseScope::ProjectEnvironment
                        && target != Path::new(PROJECT_ENVIRONMENT_TARGET)
                    {
                        return Err(Error::validation(format!(
                            "Project-environment materialization target must be {PROJECT_ENVIRONMENT_TARGET}"
                        )));
                    }
                    if !file_targets.insert(target.clone()) {
                        return Err(Error::validation(
                            "Lease materialization source and target paths must be unique",
                        ));
                    }
                    MaterializationTarget::File(target.clone())
                }
                (None, Some(name)) if lease.scope == LeaseScope::ProviderEnvironment => {
                    validate_provider_environment_name(name)?;
                    if !provider_environment_targets.insert((lease.consumer.clone(), name.clone()))
                    {
                        return Err(Error::validation(
                            "Provider environment bindings must be unique per consumer",
                        ));
                    }
                    MaterializationTarget::Environment(name.clone())
                }
                _ => {
                    return Err(Error::validation(
                        "Lease materialization must declare one scope-compatible target",
                    ));
                }
            };
            validate_sha256(&materialization.sha256)?;
            let actual = file_sha256(&source)?;
            if actual != materialization.sha256 {
                return Err(Error::validation(format!(
                    "Lease materialization digest mismatch for lease '{}'",
                    lease.lease_id
                )));
            }
            if !sources.insert(source.clone()) {
                return Err(Error::validation(
                    "Lease materialization source and target paths must be unique",
                ));
            }
            if lease.scope == LeaseScope::ProjectEnvironment {
                validate_project_environment_file(&source)?;
            }
            if lease.scope == LeaseScope::ProviderEnvironment {
                validate_provider_environment_value_file(&source, &materialization.sha256)?;
            }
            mounts.push(InGuestMount {
                source,
                target,
                sha256: materialization.sha256.clone(),
                scope: lease.scope,
                consumer: lease.consumer.clone(),
            });
        }
        leases.push(InGuestLeaseRecord {
            lease_id: lease.lease_id.clone(),
            scope: lease.scope.as_str().to_string(),
            consumer: lease.consumer.clone(),
            expires_at: lease.expires_at,
            state: if lease.scope == LeaseScope::ProjectEnvironment {
                "primary-env-file".to_string()
            } else if lease.scope == LeaseScope::ProviderEnvironment {
                "provider-env".to_string()
            } else if lease.materializations.is_empty() {
                "outer".to_string()
            } else {
                "materialized".to_string()
            },
        });
    }
    if !provider_environment_consumers.is_subset(&model_consumers) {
        return Err(Error::validation(
            "Provider-environment consumers require an exact model-identity binding",
        ));
    }
    Ok(LoadedAssignment {
        manifest_path,
        manifest,
        mounts,
        leases,
    })
}

fn validate_assignment_paths(
    assignment: &LoadedAssignment,
    repo_root: &Path,
    workspace_mount_path: &Path,
    worktree_path: &Path,
    task_branch: &str,
    repository_revision: &str,
) -> Result<()> {
    let expected_repo = fs::canonicalize(repo_root)?;
    let assigned_repo = fs::canonicalize(&assignment.manifest.repository.path)?;
    if assigned_repo != expected_repo {
        return Err(Error::validation(
            "Assigned repository path does not match the BranchBox repository",
        ));
    }
    let assigned_workspace = fs::canonicalize(&assignment.manifest.workspace)?;
    let expected_workspace = fs::canonicalize(workspace_mount_path)?;
    let expected_worktree = if worktree_path.exists() {
        fs::canonicalize(worktree_path)?
    } else {
        let parent = worktree_path.parent().ok_or_else(|| {
            Error::validation("BranchBox in-guest worktree has no parent directory")
        })?;
        let basename = worktree_path
            .file_name()
            .ok_or_else(|| Error::validation("BranchBox in-guest worktree has no basename"))?;
        fs::canonicalize(parent)?.join(basename)
    };
    if assigned_workspace != expected_workspace
        || !assigned_repo.starts_with(&assigned_workspace)
        || !expected_worktree.starts_with(&assigned_workspace)
    {
        return Err(Error::validation(
            "Assigned workspace does not contain the repository and BranchBox worktree",
        ));
    }
    if assignment.manifest.task_branch != task_branch {
        return Err(Error::validation(format!(
            "Assigned task branch '{}' does not match BranchBox branch '{}'",
            assignment.manifest.task_branch, task_branch
        )));
    }
    if assignment.manifest.repository.revision != repository_revision {
        return Err(Error::validation(
            "Assigned repository revision does not match the requested BranchBox base revision",
        ));
    }
    Ok(())
}

fn validate_private_regular_file(path: &Path, description: &str) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(Error::validation(format!(
            "In-guest {description} path must be absolute"
        )));
    }
    let metadata = fs::symlink_metadata(path).map_err(|err| {
        Error::validation(format!("Cannot inspect in-guest {description}: {err}"))
    })?;
    if !metadata.file_type().is_file() {
        return Err(Error::validation(format!(
            "In-guest {description} must be a regular non-symlink file"
        )));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(Error::validation(format!(
            "In-guest {description} must use owner-only permissions"
        )));
    }
    fs::canonicalize(path).map_err(Into::into)
}

fn validate_lease_target(path: &Path) -> Result<()> {
    let root = Path::new(LEASE_TARGET_ROOT);
    if !path.is_absolute()
        || !path.starts_with(root)
        || path == root
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Error::validation(format!(
            "Lease target must be an individual path below {LEASE_TARGET_ROOT}"
        )));
    }
    Ok(())
}

fn validate_ports(ports: &[RuntimePort]) -> Result<()> {
    let mut host = BTreeSet::new();
    let mut runtime = BTreeSet::new();
    for port in ports {
        if port.host == 0
            || port.runtime == 0
            || !host.insert(port.host)
            || !runtime.insert(port.runtime)
        {
            return Err(Error::validation(
                "In-guest published ports must be non-zero and unique",
            ));
        }
    }
    Ok(())
}

fn validate_git_revision(revision: &str) -> Result<()> {
    if !matches!(revision.len(), 40 | 64)
        || !revision
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(Error::validation(
            "Assigned repository revision must be a full Git object digest",
        ));
    }
    Ok(())
}

fn validate_sha256(digest: &str) -> Result<()> {
    if digest.len() != 64
        || !digest
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, 'a'..='f'))
    {
        return Err(Error::validation(
            "Materialization sha256 must be 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn validate_project_environment_file(path: &Path) -> Result<()> {
    const MAX_BYTES: usize = 64 * 1024;
    const MAX_ENTRIES: usize = 256;
    const MAX_VALUE_BYTES: usize = 16 * 1024;

    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(Error::validation(
            "Project-environment materialization must contain 1-256 entries and be at most 64 KiB",
        ));
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| {
        Error::validation("Project-environment materialization must be valid UTF-8")
    })?;
    if !source.ends_with('\n') || source.contains('\r') || source.contains('\0') {
        return Err(Error::validation(
            "Project-environment materialization must use canonical LF-terminated dotenv lines",
        ));
    }

    let mut previous: Option<&str> = None;
    let mut count = 0_usize;
    for line in source.strip_suffix('\n').unwrap_or(source).split('\n') {
        count += 1;
        if count > MAX_ENTRIES || line.is_empty() || line.starts_with('#') {
            return Err(Error::validation(
                "Project-environment materialization must contain only canonical KEY=value lines",
            ));
        }
        let (name, value) = line.split_once('=').ok_or_else(|| {
            Error::validation(
                "Project-environment materialization must contain only canonical KEY=value lines",
            )
        })?;
        if !is_canonical_environment_name(name) {
            return Err(Error::validation(format!(
                "Project-environment name '{name}' is not canonical uppercase dotenv syntax"
            )));
        }
        if is_reserved_project_environment_name(name) {
            return Err(Error::validation(format!(
                "Project-environment name '{name}' is reserved by the runtime boundary"
            )));
        }
        if previous.is_some_and(|previous| previous >= name) {
            return Err(Error::validation(
                "Project-environment names must be unique and sorted lexicographically",
            ));
        }
        if value.len() > MAX_VALUE_BYTES || value.chars().any(char::is_control) {
            return Err(Error::validation(format!(
                "Project-environment value for '{name}' exceeds the safe single-line limit"
            )));
        }
        previous = Some(name);
    }
    Ok(())
}

fn validate_provider_environment_name(name: &str) -> Result<()> {
    if !is_canonical_environment_name(name)
        || is_reserved_provider_environment_name(name)
        || name.len() > 128
    {
        return Err(Error::validation(
            "Provider environment name is not an admissible managed binding",
        ));
    }
    Ok(())
}

fn is_reserved_provider_environment_name(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "BASH_ENV",
        "CDPATH",
        "CONTAINER_HOST",
        "ENV",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_DIR",
        "GIT_EXEC_PATH",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_TEMPLATE_DIR",
        "GIT_WORK_TREE",
        "HOME",
        "HOSTNAME",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "LOGNAME",
        "PATH",
        "PROMPT_COMMAND",
        "PWD",
        "RUBYOPT",
        "SHELL",
        "SHELLOPTS",
        "SSH_AUTH_SOCK",
        "USER",
    ];
    EXACT.contains(&name)
        || [
            "BRANCHBOX_",
            "COMPOSE_",
            "CONTAINERD_",
            "DEVCONTAINER_",
            "DOCKER_",
            "DYLD_",
            "GIT_CONFIG_",
            "NODE_OPTIONS",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn validate_provider_environment_value_file(path: &Path, expected_sha256: &str) -> Result<()> {
    read_provider_environment_value(path, expected_sha256).map(|_| ())
}

fn read_provider_environment_value(path: &Path, expected_sha256: &str) -> Result<String> {
    let bytes = fs::read(path)?;
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() < 8
        || bytes.len() > MAX_PROVIDER_ENVIRONMENT_VALUE_BYTES
        || bytes.iter().any(|byte| byte.is_ascii_control())
        || actual_sha256 != expected_sha256
    {
        return Err(Error::validation(
            "Provider environment materialization is invalid",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| Error::validation("Provider environment materialization is invalid"))
}

fn is_canonical_environment_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_uppercase())
        && characters.all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}

fn is_reserved_project_environment_name(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "AGENTIFY_PROJECT_ENVIRONMENT_BUNDLE",
        "ANTHROPIC_API_KEY",
        "AZURE_OPENAI_API_KEY",
        "BASH_ENV",
        "CDPATH",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_TUNNEL_TOKEN",
        "CONTAINERD_ADDRESS",
        "CONTAINER_HOST",
        "DEV_HOSTNAME",
        "DOCKER_CERT_PATH",
        "DOCKER_CONTEXT",
        "DOCKER_HOST",
        "DOCKER_TLS_VERIFY",
        "ENV",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_DIR",
        "GIT_EXEC_PATH",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_TEMPLATE_DIR",
        "GIT_WORK_TREE",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GROQ_API_KEY",
        "HOME",
        "HOSTNAME",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "LOGNAME",
        "OPENAI_API_KEY",
        "OP_SERVICE_ACCOUNT_TOKEN",
        "PATH",
        "PROMPT_COMMAND",
        "PWD",
        "SHELL",
        "SHELLOPTS",
        "SSH_AUTH_SOCK",
        "TUNNEL_TOKEN",
        "USER",
    ];
    EXACT.contains(&name)
        || [
            "BRANCHBOX_",
            "CODEX_",
            "COMPOSE_",
            "DEVCONTAINER_",
            "DYLD_",
            "GIT_CONFIG_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut source = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_opaque_identifier(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
        })
    {
        return Err(Error::validation(format!(
            "In-guest {field} must be an opaque non-secret identifier"
        )));
    }
    Ok(())
}

fn validate_label(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.contains("//")
        || value.contains('=')
        || value.chars().any(char::is_whitespace)
    {
        return Err(Error::validation(format!(
            "In-guest {field} must be a non-secret label, not a URL or value"
        )));
    }
    Ok(())
}

fn safe_identity(value: &str) -> Result<String> {
    validate_opaque_identifier(value, "run_id")?;
    Ok(value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect())
}

fn parse_container_id(output: &Output) -> Result<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            value
                .get("containerId")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| {
            Error::validation("In-guest devcontainer startup did not return a container ID")
        })
}

fn validate_resolved_configuration(source: &[u8]) -> Result<()> {
    let configuration = String::from_utf8_lossy(source)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .ok_or_else(|| {
            Error::validation("Dev Containers CLI did not return resolved configuration evidence")
        })?;
    let normalized = configuration.to_string().to_ascii_lowercase();
    if contains_project_docker_reference(&normalized)
        || normalized.contains("/run/agentify-assignment")
        || normalized.contains(MANAGED_RUNTIME_ROOT)
        || normalized.contains("initializecommand")
        || normalized.contains("${localenv:")
    {
        return Err(Error::validation(
            "Resolved devcontainer configuration reintroduced a host hook, ambient path, container supervisor authority, or credential directory",
        ));
    }
    Ok(())
}

fn contains_project_docker_reference(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    [
        "docker-outside-of-docker",
        "docker-in-docker",
        "docker-from-docker",
        "docker.sock",
        "containerd.sock",
        "podman.sock",
        "buildkit.sock",
        "buildkitd.sock",
        "/var/run/docker",
        "/run/docker",
        "/var/lib/docker",
        "/run/containerd",
        "/var/lib/containerd",
        "/run/podman",
        "/run/buildkit",
        "docker_host",
        "container_host",
        "containerd_address",
        "buildkit_host",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn validate_container_inspection(
    source: &[u8],
    allowed_sources: &BTreeSet<PathBuf>,
    forbidden_environment: &BTreeSet<String>,
) -> Result<()> {
    let documents: serde_json::Value = serde_json::from_slice(source).map_err(|err| {
        Error::validation(format!(
            "Docker returned invalid container inspection JSON: {err}"
        ))
    })?;
    let container = documents
        .as_array()
        .and_then(|documents| documents.first())
        .ok_or_else(|| Error::validation("Docker returned no container inspection record"))?;
    let host = container
        .get("HostConfig")
        .unwrap_or(&serde_json::Value::Null);
    let privileged = host
        .get("Privileged")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let host_pid = host
        .get("PidMode")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|mode| mode == "host");
    let host_ipc = host
        .get("IpcMode")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|mode| mode == "host");
    let host_network = host
        .get("NetworkMode")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|mode| mode == "host");
    let host_cgroup = host
        .get("CgroupnsMode")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|mode| mode == "host");
    let host_user = host
        .get("UsernsMode")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|mode| mode == "host");
    let host_uts = host
        .get("UTSMode")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|mode| mode == "host");
    let elevated_capability = host
        .get("CapAdd")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|capabilities| {
            capabilities.iter().any(|capability| {
                capability.as_str().is_some_and(|capability| {
                    matches!(
                        capability.to_ascii_uppercase().as_str(),
                        "ALL" | "SYS_ADMIN" | "NET_ADMIN" | "SYS_PTRACE"
                    )
                })
            })
        });
    let host_device = host
        .get("Devices")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|devices| !devices.is_empty())
        || host
            .get("DeviceRequests")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|devices| !devices.is_empty());
    let disabled_confinement = host
        .get("SecurityOpt")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|options| {
            options.iter().any(|option| {
                option.as_str().is_some_and(|option| {
                    let normalized = option.to_ascii_lowercase();
                    normalized.contains("unconfined") || normalized.contains("label=disable")
                })
            })
        });
    if privileged
        || host_pid
        || host_ipc
        || host_network
        || host_cgroup
        || host_user
        || host_uts
        || elevated_capability
        || host_device
        || disabled_confinement
    {
        return Err(Error::validation(
            "In-guest devcontainer resolved to privileged host authority",
        ));
    }
    let unsafe_mount = container
        .get("Mounts")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|mounts| {
            mounts.iter().any(|mount| {
                let source = mount
                    .get("Source")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let destination = mount
                    .get("Destination")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                contains_supervisor_mount(source, allowed_sources)
                    || contains_supervisor_mount(destination, &BTreeSet::new())
            })
        });
    if unsafe_mount {
        return Err(Error::validation(
            "In-guest devcontainer resolved a supervisor socket or credential-directory mount",
        ));
    }
    let inherited_forbidden_environment = container
        .get("Config")
        .and_then(|config| config.get("Env"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|environment| {
            environment.iter().any(|entry| {
                entry.as_str().is_some_and(|entry| {
                    let name = entry.split('=').next().unwrap_or_default();
                    forbidden_environment.contains(name)
                        || matches!(
                            name,
                            "DOCKER_HOST"
                                | "DOCKER_CONTEXT"
                                | "DOCKER_CERT_PATH"
                                | "DOCKER_TLS_VERIFY"
                                | "CONTAINER_HOST"
                                | "CONTAINERD_ADDRESS"
                                | "BUILDKIT_HOST"
                        )
                })
            })
        });
    if inherited_forbidden_environment {
        return Err(Error::validation(
            "In-guest devcontainer may not persist model identity or container supervisor endpoints in its environment",
        ));
    }
    Ok(())
}

fn contains_supervisor_mount(value: &str, allowed_sources: &BTreeSet<PathBuf>) -> bool {
    let normalized = value.to_ascii_lowercase();
    contains_project_docker_reference(&normalized)
        || normalized == "/run/agentify-assignment"
        || normalized.starts_with("/run/agentify-assignment/")
        || normalized == MANAGED_RUNTIME_ROOT
        || normalized.starts_with(&format!("{MANAGED_RUNTIME_ROOT}/"))
        || ((normalized == "/run/agentify-runtime"
            || normalized.starts_with("/run/agentify-runtime/"))
            && !allowed_sources.contains(Path::new(value)))
}

fn effective_workspace_folder(config: &DevcontainerConfig, worktree_path: &Path) -> String {
    let basename = worktree_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    config
        .workspace_folder
        .as_deref()
        .unwrap_or("/workspaces/${localWorkspaceFolderBasename}")
        .replace("${localWorkspaceFolderBasename}", basename)
}

fn effective_container_user(config: &DevcontainerConfig) -> String {
    config
        .remote_user
        .as_deref()
        .or(config.container_user.as_deref())
        .unwrap_or("root")
        .to_string()
}

fn resolve_binary(environment: &str, name: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(environment) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(Error::validation(format!(
            "{environment} does not point to a regular executable file"
        )));
    }
    which::which(name).map_err(|_| {
        Error::validation(format!(
            "Runtime 'in-guest' requires '{name}' in the current Firecracker guest"
        ))
    })
}

fn ensure_compose_override_version(version: &str) -> Result<()> {
    let normalized = version.trim().trim_start_matches('v');
    let mut parts = normalized
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok());
    let candidate = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    if candidate < (2, 30, 0) {
        return Err(Error::validation(
            "Runtime 'in-guest' requires Docker Compose 2.30.0+ for raw project-environment env files and exclusive outer-tunnel overrides",
        ));
    }
    Ok(())
}

fn set_owner_only(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn workspace_candidates(path: &Path) -> BTreeSet<String> {
    let mut paths = BTreeSet::from([path.to_string_lossy().into_owned()]);
    if let Ok(canonical) = fs::canonicalize(path) {
        paths.insert(canonical.to_string_lossy().into_owned());
    }
    paths
}

fn compose_labels_belong_to_workspace(
    working_dir: &str,
    config_files: &str,
    workspace_paths: &BTreeSet<String>,
) -> bool {
    let belongs = |candidate: &str| {
        let candidate = Path::new(candidate);
        workspace_paths.iter().any(|workspace| {
            let workspace = Path::new(workspace);
            candidate == workspace || candidate.starts_with(workspace)
        })
    };
    (!working_dir.is_empty() && belongs(working_dir))
        || config_files
            .split(',')
            .map(str::trim)
            .any(|path| !path.is_empty() && belongs(path))
}

fn deterministic_compose_projects(
    runtime_name: &str,
    worktree_path: &Path,
    config: &DevcontainerConfig,
    config_path: &Path,
) -> BTreeSet<String> {
    let mut projects = BTreeSet::new();
    if is_compose_project_name(runtime_name) {
        projects.insert(runtime_name.to_string());
    }
    if let Some(basename) = worktree_path.file_name().and_then(|name| name.to_str()) {
        if is_compose_project_name(basename) {
            projects.insert(basename.to_string());
            let devcontainer_project = format!("{basename}_devcontainer");
            if is_compose_project_name(&devcontainer_project) {
                projects.insert(devcontainer_project);
            }
        }
    }

    let devcontainer_dir = config_path.parent().unwrap_or(worktree_path);
    let compose_references: Vec<String> = match config.docker_compose_file.as_ref() {
        Some(reference) => reference.to_vec(),
        None => [
            "compose.yaml",
            "compose.yml",
            "docker-compose.yaml",
            "docker-compose.yml",
        ]
        .iter()
        .filter(|name| devcontainer_dir.join(name).is_file())
        .map(|name| (*name).to_string())
        .collect(),
    };
    let compose_files: Vec<PathBuf> = compose_references
        .into_iter()
        .map(|path| devcontainer_dir.join(path))
        .collect();
    for compose_file in compose_files {
        let Ok(source) = fs::read_to_string(compose_file) else {
            continue;
        };
        let Ok(document) = serde_yaml::from_str::<serde_yaml::Value>(&source) else {
            continue;
        };
        if let Some(name) = document.get("name").and_then(serde_yaml::Value::as_str) {
            if is_compose_project_name(name) {
                projects.insert(name.to_string());
            }
        }
    }
    projects
}

fn output_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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

fn bounded_failure(bytes: &[u8]) -> String {
    const MAX_BYTES: usize = 2_048;
    const MAX_LINES: usize = 12;
    let rendered = String::from_utf8_lossy(bytes);
    let mut lines: Vec<_> = rendered.lines().rev().take(MAX_LINES).collect();
    lines.reverse();
    let mut detail = lines.join("\n");
    for name in LEGACY_REDACTED_ENVIRONMENT {
        if let Some(value) = std::env::var_os(name).and_then(|value| value.into_string().ok()) {
            if !value.is_empty() {
                detail = detail.replace(&value, "[REDACTED]");
            }
        }
    }
    if detail.len() <= MAX_BYTES {
        return detail;
    }
    let mut start = detail.len() - MAX_BYTES;
    while !detail.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &detail[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn private_write(path: &Path, content: &[u8]) {
        fs::write(path, content).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    fn assignment_fixture(with_materialization: bool) -> (tempfile::TempDir, PathBuf, String) {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let repository = workspace.join("agentify");
        fs::create_dir_all(&repository).unwrap();
        let materializations = root.path().join("materializations");
        let digest = if with_materialization {
            fs::create_dir_all(&materializations).unwrap();
            let source = materializations.join("opaque-project-env");
            private_write(
                &source,
                b"ACCOUNT_NAME=Matchup\nADMIN_EMAIL=admin@example.com\n",
            );
            file_sha256(&source).unwrap()
        } else {
            "0".repeat(64)
        };
        let leases = if with_materialization {
            serde_json::json!([{
                "lease_id": "lease_project",
                "scope": "project-environment",
                "consumer": "rails-app",
                "materializations": [{
                    "source_path": materializations.join("opaque-project-env"),
                    "target_path": "/run/branchbox/leases/project-env",
                    "sha256": digest
                }]
            }])
        } else {
            serde_json::json!([{
                "lease_id": "lease_tunnel",
                "scope": "platform-tunnel",
                "consumer": "outer-connector",
                "materializations": []
            }])
        };
        let manifest = serde_json::json!({
            "version": "1",
            "run_id": "run_123",
            "lease_id": "assignment_123",
            "outer_runtime_id": "vm_123",
            "workspace": workspace,
            "repository": {
                "path": repository,
                "revision": "a".repeat(40)
            },
            "task_branch": "feature/coding-demo",
            "tunnel_placement": "outer",
            "published_ports": [{"host": 3000, "runtime": 3000}],
            "leases": leases
        });
        let manifest_path = root.path().join("branchbox-in-guest.json");
        private_write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap().as_slice(),
        );
        (root, manifest_path, "a".repeat(40))
    }

    #[cfg(unix)]
    fn managed_assignment_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let repository = workspace.join("repository");
        let materializations = root.path().join("materializations");
        fs::create_dir_all(&repository).unwrap();
        fs::create_dir_all(&materializations).unwrap();
        let provider_secret = materializations.join("provider-secret");
        private_write(&provider_secret, b"opaque-provider-value");
        let manifest = serde_json::json!({
            "version": "2",
            "run_id": "run_123",
            "lease_id": "assignment_123",
            "outer_runtime_id": "vm_123",
            "workspace": workspace,
            "repository": {
                "path": repository,
                "revision": "a".repeat(40)
            },
            "task_branch": "feature/coding-demo",
            "tunnel_placement": "outer",
            "published_ports": [{"host": 3000, "runtime": 3000}],
            "leases": [
                {
                    "lease_id": "lease_model",
                    "scope": "model-identity",
                    "consumer": "coding-agent",
                    "executable": "provider-cli",
                    "inherited_environment": ["MODEL_ACCESS_TOKEN"],
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": []
                },
                {
                    "lease_id": "lease_delivery",
                    "scope": "provider-environment",
                    "consumer": "coding-agent",
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": [{
                        "source_path": provider_secret.clone(),
                        "environment_name": "SOURCE_DELIVERY_TOKEN",
                        "sha256": file_sha256(&provider_secret).unwrap()
                    }]
                }
            ]
        });
        let manifest_path = root.path().join("branchbox-in-guest.json");
        private_write(
            &manifest_path,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        (root, manifest_path, provider_secret)
    }

    #[cfg(unix)]
    #[test]
    fn validates_manifest_and_sibling_materialization_digest() {
        let (root, manifest, revision) = assignment_fixture(true);
        let workspace = root.path().join("workspace");
        let repository = workspace.join("agentify");
        let worktree = workspace.join("coding-demo");
        let plan = load_in_guest_facade_plan(
            &manifest,
            &repository,
            &workspace,
            &worktree,
            "feature/coding-demo",
            &revision,
        )
        .unwrap();

        assert_eq!(
            plan.published_ports(),
            &[RuntimePort {
                host: 3000,
                runtime: 3000
            }]
        );
        assert_eq!(plan.mounts().count(), 0);
        let expected_environment = root
            .path()
            .join("materializations/opaque-project-env")
            .canonicalize()
            .unwrap();
        assert_eq!(
            plan.project_environment(),
            Some((expected_environment.as_path(), "rails-app"))
        );
        assert_eq!(plan.tunnel_placement(), InGuestTunnelPlacement::Outer);
    }

    #[cfg(unix)]
    #[test]
    fn accepts_manifest_without_materialization_directory_for_outer_only_lease() {
        let (_root, manifest, _revision) = assignment_fixture(false);
        let loaded = load_assignment(&manifest).unwrap();
        assert!(loaded.mounts.is_empty());
        assert_eq!(loaded.leases[0].state, "outer");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_raw_secret_fields_without_echoing_the_value() {
        const SENTINEL: &str = "never-print-this-secret";
        let (root, manifest, _revision) = assignment_fixture(false);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"][0]["value"] = serde_json::json!(SENTINEL);
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());

        let error = load_assignment(&manifest).unwrap_err().to_string();
        assert!(error.contains("unknown field"));
        assert!(!error.contains(SENTINEL));
        drop(root);
    }

    #[test]
    fn rejects_supervisor_socket_and_assignment_directory_mounts_after_resolution() {
        for source in [
            "/var/run/docker.sock",
            "/run/containerd/containerd.sock",
            "/run/podman/podman.sock",
            "/run/buildkit/buildkitd.sock",
            "/var/lib/docker",
            "/run/agentify-assignment/materializations",
            "/run/agentify-runtime",
        ] {
            let inspection = serde_json::json!([{
                "HostConfig": {"Privileged": false, "PidMode": "", "IpcMode": ""},
                "Mounts": [{"Source": source, "Destination": "/unsafe"}],
                "Config": {"Env": []}
            }]);
            assert!(validate_container_inspection(
                &serde_json::to_vec(&inspection).unwrap(),
                &BTreeSet::new(),
                &BTreeSet::new()
            )
            .is_err());
        }
    }

    #[test]
    fn resolved_configuration_rejects_project_docker_aliases_and_remote_endpoints() {
        for configuration in [
            serde_json::json!({
                "features": {"ghcr.io/devcontainers/features/docker-in-docker:2": {}}
            }),
            serde_json::json!({"containerEnv": {"DOCKER_HOST": "tcp://supervisor:2375"}}),
            serde_json::json!({
                "mounts": ["source=/run/podman/podman.sock,target=/run/podman/podman.sock,type=bind"]
            }),
        ] {
            let output = format!("diagnostic line\n{configuration}\n");
            assert!(
                validate_resolved_configuration(output.as_bytes()).is_err(),
                "unexpectedly accepted: {configuration}"
            );
        }
    }

    #[test]
    fn resolved_configuration_accepts_an_unprivileged_socket_free_facade() {
        let configuration = serde_json::json!({
            "configuration": {
                "privileged": false,
                "containerEnv": {"DB_HOST": "postgres"},
                "features": {"ghcr.io/devcontainers/features/github-cli:1": {}}
            }
        });
        validate_resolved_configuration(format!("{configuration}\n").as_bytes()).unwrap();
    }

    #[test]
    fn running_container_rejects_remote_daemon_environment_and_privilege_reintroduction() {
        for inspection in [
            serde_json::json!([{
                "HostConfig": {"Privileged": false, "PidMode": "", "IpcMode": "private"},
                "Mounts": [],
                "Config": {"Env": ["DOCKER_HOST=tcp://supervisor:2375"]}
            }]),
            serde_json::json!([{
                "HostConfig": {
                    "Privileged": false,
                    "PidMode": "",
                    "IpcMode": "private",
                    "CapAdd": ["SYS_ADMIN"]
                },
                "Mounts": [],
                "Config": {"Env": []}
            }]),
            serde_json::json!([{
                "HostConfig": {
                    "Privileged": false,
                    "PidMode": "",
                    "IpcMode": "private",
                    "NetworkMode": "host"
                },
                "Mounts": [],
                "Config": {"Env": []}
            }]),
            serde_json::json!([{
                "HostConfig": {
                    "Privileged": false,
                    "PidMode": "",
                    "IpcMode": "private",
                    "Devices": [{"PathOnHost": "/dev/kvm"}]
                },
                "Mounts": [],
                "Config": {"Env": []}
            }]),
            serde_json::json!([{
                "HostConfig": {
                    "Privileged": false,
                    "PidMode": "",
                    "IpcMode": "private",
                    "SecurityOpt": ["seccomp=unconfined"]
                },
                "Mounts": [],
                "Config": {"Env": []}
            }]),
        ] {
            assert!(validate_container_inspection(
                &serde_json::to_vec(&inspection).unwrap(),
                &BTreeSet::new(),
                &BTreeSet::new()
            )
            .is_err());
        }
    }

    #[test]
    fn accepts_unprivileged_container_with_no_supervisor_mounts_or_persisted_model_key() {
        let inspection = serde_json::json!([{
            "HostConfig": {"Privileged": false, "PidMode": "", "IpcMode": "private"},
            "Mounts": [{"Source": "/workspace/agentify", "Destination": "/workspaces/agentify"}],
            "Config": {"Env": ["RAILS_ENV=development"]}
        }]);
        validate_container_inspection(
            &serde_json::to_vec(&inspection).unwrap(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .unwrap();
    }

    #[test]
    fn compose_override_requires_override_tag_capable_version() {
        ensure_compose_override_version("2.30.0").unwrap();
        ensure_compose_override_version("v2.40.3").unwrap();
        assert!(ensure_compose_override_version("2.29.9").is_err());
    }

    #[test]
    fn published_port_proxy_targets_only_the_inspected_primary_container() {
        assert!(IN_GUEST_PORT_PROXY_SCRIPT.contains("inspect -f '{{.Name}}' \"$container_id\""));
        assert!(!IN_GUEST_PORT_PROXY_SCRIPT.contains("ps --filter"));
        assert!(!IN_GUEST_PORT_PROXY_SCRIPT.contains("ExposedPorts"));
        assert!(!IN_GUEST_PORT_PROXY_SCRIPT.contains("candidate_id"));
    }

    #[cfg(unix)]
    #[test]
    fn managed_provider_environment_is_exact_consumer_bound_and_never_mounted() {
        let (_root, manifest, provider_secret) = managed_assignment_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        assignment
            .authorize_provider_execution("provider-cli", &["MODEL_ACCESS_TOKEN".to_string()])
            .unwrap();
        assert!(assignment
            .authorize_provider_execution("other-cli", &["MODEL_ACCESS_TOKEN".to_string()],)
            .is_err());
        assert!(assignment
            .authorize_provider_execution("provider-cli", &["UNDECLARED_TOKEN".to_string()],)
            .is_err());
        let bindings = assignment
            .provider_environment("coding-agent")
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].source, provider_secret.canonicalize().unwrap());
        assert!(matches!(
            &bindings[0].target,
            MaterializationTarget::Environment(name) if name == "SOURCE_DELIVERY_TOKEN"
        ));
        let inspection = serde_json::json!([{
            "HostConfig": {"Privileged": false, "PidMode": "", "IpcMode": "private"},
            "Mounts": [],
            "Config": {"Env": ["SOURCE_DELIVERY_TOKEN=must-not-persist"]}
        }]);
        assert!(validate_container_inspection(
            &serde_json::to_vec(&inspection).unwrap(),
            &BTreeSet::new(),
            &assignment.provider_environment_names(),
        )
        .is_err());

        let plan = InGuestFacadePlan {
            manifest_path: manifest,
            tunnel_placement: InGuestTunnelPlacement::Outer,
            published_ports: Vec::new(),
            mounts: assignment.mounts,
        };
        assert_eq!(plan.mounts().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn signed_provider_binding_reaches_only_the_provider_process_environment() {
        const SENTINEL: &str = "non-secret-sentinel";
        let (_root, manifest, provider_secret) = managed_assignment_fixture();
        private_write(&provider_secret, SENTINEL.as_bytes());
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"][1]["materializations"][0]["sha256"] =
            serde_json::json!(file_sha256(&provider_secret).unwrap());
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        let assignment = load_assignment(&manifest).unwrap();
        let binding = assignment
            .provider_environment("coding-agent")
            .next()
            .unwrap();
        let MaterializationTarget::Environment(name) = &binding.target else {
            panic!("expected provider environment target");
        };
        let bindings = vec![ProviderEnvironmentValue {
            name: name.clone(),
            value: read_provider_environment_value(&binding.source, &binding.sha256).unwrap(),
        }];
        let mut child = Command::new("/bin/sh");
        child
            .env("UNRELATED_TOKEN", "must-be-cleared")
            .args([
                "-c",
                "test \"$SOURCE_DELIVERY_TOKEN\" = \"non-secret-sentinel\" && test -z \"${UNRELATED_TOKEN:-}\" && printf received",
            ]);
        apply_provider_process_environment(&mut child, &[], &bindings).unwrap();
        let output = child.output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"received");
        assert!(output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SENTINEL));
    }

    #[cfg(unix)]
    #[test]
    fn legacy_provider_execution_preserves_the_version_one_allowlist() {
        let (_root, manifest, _revision) = assignment_fixture(false);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"] = serde_json::json!([{
            "lease_id": "lease_model",
            "scope": "model-identity",
            "consumer": LEGACY_PROVIDER_EXECUTABLE,
            "expires_at": "2099-01-01T00:00:00Z",
            "materializations": []
        }]);
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        let assignment = load_assignment(&manifest).unwrap();
        assignment
            .authorize_provider_execution(
                LEGACY_PROVIDER_EXECUTABLE,
                &[LEGACY_PROVIDER_ENVIRONMENT.to_string()],
            )
            .unwrap();
        assert!(assignment
            .authorize_provider_execution("other-cli", &[LEGACY_PROVIDER_ENVIRONMENT.to_string()],)
            .is_err());
        assert!(assignment
            .authorize_provider_execution(
                LEGACY_PROVIDER_EXECUTABLE,
                &["OTHER_MODEL_TOKEN".to_string()],
            )
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn provider_environment_digest_is_recomputed_from_the_bytes_used_at_exec() {
        let (_root, manifest, provider_secret) = managed_assignment_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let binding = assignment
            .provider_environment("coding-agent")
            .next()
            .unwrap();
        private_write(&provider_secret, b"rotated-provider-value");
        assert!(read_provider_environment_value(&provider_secret, &binding.sha256).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn managed_provider_environment_rejects_control_names_and_values_without_secret_echo() {
        const SENTINEL: &str = "never-echo-provider-secret";
        let (_root, manifest, provider_secret) = managed_assignment_fixture();
        private_write(&provider_secret, format!("{SENTINEL}\n").as_bytes());
        let error = load_assignment(&manifest).unwrap_err().to_string();
        assert!(!error.contains(SENTINEL));

        private_write(&provider_secret, SENTINEL.as_bytes());
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"][1]["materializations"][0]["environment_name"] =
            serde_json::json!("DOCKER_HOST");
        value["leases"][1]["materializations"][0]["sha256"] =
            serde_json::json!(file_sha256(&provider_secret).unwrap());
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        let error = load_assignment(&manifest).unwrap_err().to_string();
        assert!(!error.contains(SENTINEL));
    }

    #[cfg(unix)]
    #[test]
    fn provider_environment_materialization_cleanup_is_residue_checked() {
        let (root, _manifest, provider_secret) = managed_assignment_fixture();
        let state = ProviderState {
            version: PROVIDER_STATE_VERSION.to_string(),
            manifest_path: root.path().join("branchbox-in-guest.json"),
            worktree_path: root.path().join("workspace/worktree"),
            workspace_paths: Vec::new(),
            config_path: root.path().join("devcontainer.json"),
            run_id: Some("run_123".to_string()),
            outer_runtime_id: Some("vm_123".to_string()),
            materializations: vec![StateMaterialization {
                source_path: provider_secret.clone(),
                sha256: file_sha256(&provider_secret).unwrap(),
            }],
            proxy_names: Vec::new(),
            compose_projects: Vec::new(),
            container_id: None,
        };
        let provider = InGuestRuntimeProvider {
            devcontainer: PathBuf::from("devcontainer"),
            docker: PathBuf::from("docker"),
        };
        let mut residue = Vec::new();
        provider.erase_materializations(&state, &mut residue);
        assert!(!provider_secret.exists());
        assert!(residue.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn project_environment_accepts_canonical_raw_values_without_interpolation() {
        let root = tempfile::tempdir().unwrap();
        let environment = root.path().join("project.env");
        private_write(
            &environment,
            b"ACCOUNT_NAME=Matchup Labs\nADMIN_EMAIL=admin@example.com\nADMIN_PASSWORD=$2b$12# spaced ' \" value\n",
        );
        validate_project_environment_file(&environment).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn project_environment_rejects_reserved_unsorted_and_malformed_entries_without_values() {
        const SECRET: &str = "never-echo-project-secret";
        for content in [
            format!("OPENAI_API_KEY={SECRET}\n"),
            format!("Z_LAST={SECRET}\nA_FIRST=value\n"),
            format!("lowercase={SECRET}\n"),
            format!("MALFORMED:{SECRET}\n"),
            format!("VALUE={SECRET}"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let environment = root.path().join("project.env");
            private_write(&environment, content.as_bytes());
            let error = validate_project_environment_file(&environment)
                .unwrap_err()
                .to_string();
            assert!(!error.contains(SECRET));
        }
    }
}
