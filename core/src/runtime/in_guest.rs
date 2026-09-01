//! Devcontainer runtime inside an orchestrator-owned isolation boundary.
//!
//! This provider deliberately owns no VM and no SSH control plane. The outer orchestrator
//! materializes a signed assignment and opaque lease files in the guest; BranchBox validates only
//! the versioned assignment, paths, consumers, and digests, creates the Git worktree through the
//! normal feature workflow, and operates Docker/devcontainers directly in the current guest.

use super::{
    exec_result, RuntimeContext, RuntimeExecResult, RuntimeMetadata, RuntimePort, RuntimeProvider,
    RuntimeProviderKind, RuntimeResidue, RuntimeTeardownReport, RuntimeToolDispatchResult,
};
use crate::{devcontainer_runtime::DevcontainerConfig, Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const LEGACY_MANIFEST_VERSION: &str = "1";
const MANAGED_MANIFEST_VERSION: &str = "2";
const MANAGED_RUNTIME_ROOT: &str = "/run/branchbox/managed";
const LEASE_TARGET_ROOT: &str = "/run/branchbox/leases";
const PROJECT_ENVIRONMENT_TARGET: &str = "/run/branchbox/leases/project-env";
const SHARED_DIRECTORY_TARGET_ROOT: &str = "/run/branchbox/leases/shared";
const TOOL_ENDPOINT_TARGET_ROOT: &str = "/run/branchbox/leases/tool-endpoints";
const TOOL_REQUEST_TARGET_ROOT: &str = "/run/branchbox/leases/tool-requests";
const MAX_PROVIDER_ENVIRONMENT_BINDINGS: usize = 16;
const MAX_PROVIDER_ENVIRONMENT_VALUE_BYTES: usize = 16 * 1024;
const MAX_TOOL_REQUESTS: usize = 16;
const MAX_TOOL_REQUEST_BYTES: usize = 256 * 1024;
const MAX_TOOL_REQUEST_QUOTA_BYTES: usize = 1024 * 1024;
const MAX_TOOL_RESPONSE_BYTES: usize = 256 * 1024;
const TOOL_RELAY_TIMEOUT: Duration = Duration::from_secs(30);
const LEGACY_PROVIDER_EXECUTABLE: &str = "codex";
const LEGACY_PROVIDER_ENVIRONMENT: &str = "OPENAI_API_KEY";
const REQUIRED_SECCOMP_SECURITY_OPTION: &str = "seccomp=builtin";
const SECURE_EXEC_WRAPPER: &str = "umask 0022; exec \"$@\"";
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
const INITIALIZE_TOOL_REQUEST_SPOOL_SCRIPT: &str = r#"set -eu
root="$1"
uid="$2"
test -d "$root"
test ! -L "$root"
for candidate in "$root"/* "$root"/.[!.]* "$root"/..?*; do
  test -e "$candidate" || test -L "$candidate" || continue
  exit 73
done
chown 0:0 "$root"
chmod 0755 "$root"
umask 077
capability_tmp="$root/.capability.tmp"
binding_tmp="$root/.binding.json.tmp"
trap 'rm -f "$capability_tmp" "$binding_tmp"' EXIT HUP INT TERM
IFS= read -r capability
printf '%s' "$capability" >"$capability_tmp"
cat >"$binding_tmp"
chown 0:0 "$capability_tmp" "$binding_tmp"
chmod 0444 "$capability_tmp" "$binding_tmp"
mv -f "$capability_tmp" "$root/.capability"
mv -f "$binding_tmp" "$root/.binding.json"
trap - EXIT HUP INT TERM
mkdir -p "$root/requests"
chown "$uid:$uid" "$root/requests"
chmod 0700 "$root/requests"
mkdir -p "$root/responses"
chown 0:0 "$root/responses"
chmod 0755 "$root/responses"
mkdir -p "$root/.processing"
chown 0:0 "$root/.processing"
chmod 0700 "$root/.processing"
for directory in "$root/requests" "$root/responses" "$root/.processing"; do
  for candidate in "$directory"/* "$directory"/.[!.]* "$directory"/..?*; do
    test -e "$candidate" || test -L "$candidate" || continue
    exit 74
  done
done
test "$(stat -c '%u:%a' "$root")" = "0:755"
test "$(stat -c '%u:%a' "$root/.capability")" = "0:444"
test "$(stat -c '%u:%a' "$root/.binding.json")" = "0:444"
test "$(stat -c '%u:%a' "$root/requests")" = "$uid:700"
test "$(stat -c '%u:%a' "$root/responses")" = "0:755"
test "$(stat -c '%u:%a' "$root/.processing")" = "0:700""#;
const READ_TOOL_REQUEST_SCRIPT: &str = r#"set -eu
root="$1"
uid="$2"
request_id="$3"
max_count="$4"
max_file_bytes="$5"
max_total_bytes="$6"
test -d "$root"
test ! -L "$root"
test "$(stat -c '%u:%a' "$root")" = "0:755"
test -f "$root/.capability"
test ! -L "$root/.capability"
test "$(stat -c '%u:%a' "$root/.capability")" = "0:444"
test -f "$root/.binding.json"
test ! -L "$root/.binding.json"
test "$(stat -c '%u:%a' "$root/.binding.json")" = "0:444"
requests="$root/requests"
test -d "$requests"
test ! -L "$requests"
test "$(stat -c '%u:%a' "$requests")" = "$uid:700"
request="$requests/$request_id.json"
count=0
total=0
found=0
for candidate in "$requests"/* "$requests"/.[!.]* "$requests"/..?*; do
  test ! -L "$candidate"
  test -e "$candidate" || continue
  base=${candidate##*/}
  test -f "$candidate"
  case "$base" in
    *.json) stem=${base%.json} ;;
    *) exit 71 ;;
  esac
  case "$stem" in
    ""|.*|*[!A-Za-z0-9._:-]*) exit 72 ;;
  esac
  set -- $(stat -c '%u %a %s %h' "$candidate")
  test "$1" = "$uid"
  test "$2" = "600"
  test "$3" -gt 0
  test "$3" -le "$max_file_bytes"
  test "$4" = "1"
  count=$((count + 1))
  total=$((total + $3))
  test "$count" -le "$max_count"
  test "$total" -le "$max_total_bytes"
  test "$candidate" != "$request" || found=1
done
test "$found" = "1" || exit 75
processing="$root/.processing"
test -d "$processing"
test ! -L "$processing"
test "$(stat -c '%u:%a' "$processing")" = "0:700"
staged="$processing/$request_id.json"
test ! -e "$staged"
test ! -L "$staged"
trap 'rm -f "$staged"' EXIT HUP INT TERM
mv "$request" "$staged"
test -f "$staged"
test ! -L "$staged"
set -- $(stat -c '%u %a %s %h' "$staged")
test "$1" = "$uid"
test "$2" = "600"
test "$3" -gt 0
test "$3" -le "$max_file_bytes"
test "$4" = "1"
dd if="$staged" bs=4096 count=65 2>/dev/null
rm -f "$staged"
trap - EXIT HUP INT TERM"#;
const WRITE_TOOL_RESPONSE_SCRIPT: &str = r#"set -eu
root="$1"
uid="$2"
request_id="$3"
responses="$root/responses"
test -d "$responses"
test ! -L "$responses"
test "$(stat -c '%u:%a' "$responses")" = "0:755"
umask 077
temporary="$responses/$request_id.json.tmp"
final="$responses/$request_id.json"
trap 'rm -f "$temporary"' EXIT HUP INT TERM
test ! -e "$temporary"
test ! -L "$temporary"
test ! -e "$final"
test ! -L "$final"
cat >"$temporary"
chown "$uid:$uid" "$temporary"
chmod 0400 "$temporary"
mv -f "$temporary" "$final"
trap - EXIT HUP INT TERM
test "$(stat -c '%u:%a' "$final")" = "$uid:400""#;
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
    tool_request_spools: Vec<ToolRequestSpool>,
    linked_tool_endpoints: BTreeSet<String>,
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
                (
                    LeaseScope::ProjectEnvironment
                    | LeaseScope::ProviderEnvironment
                    | LeaseScope::ToolRequest,
                    _,
                ) => None,
                (LeaseScope::ToolEndpoint, _)
                    if self.linked_tool_endpoints.contains(&mount.lease_id) =>
                {
                    None
                }
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

    pub fn tool_request_spools(&self) -> impl Iterator<Item = (&str, &Path, u32)> {
        self.tool_request_spools.iter().map(|spool| {
            (
                spool.volume_name.as_str(),
                spool.target_path.as_path(),
                spool.consumer_uid,
            )
        })
    }
}

#[derive(Debug, Clone)]
struct InGuestMount {
    lease_id: String,
    source: PathBuf,
    target: MaterializationTarget,
    sha256: Option<String>,
    source_kind: ManagedSourceKind,
    scope: LeaseScope,
    consumer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ToolRequestSpool {
    lease_id: String,
    endpoint_lease_id: String,
    consumer: String,
    consumer_uid: u32,
    target_path: PathBuf,
    volume_name: String,
    capability_source: PathBuf,
    capability_sha256: String,
}

#[derive(Debug, Serialize)]
struct ToolRequestBinding<'a> {
    version: &'static str,
    run_id: &'a str,
    lease_id: &'a str,
    endpoint_lease_id: &'a str,
    consumer: &'a str,
    request_directory: PathBuf,
    response_directory: PathBuf,
    capability_path: PathBuf,
    request_filename: &'static str,
    response_filename: &'static str,
    max_pending_requests: usize,
    max_request_bytes: usize,
    max_spool_bytes: usize,
    max_response_bytes: usize,
}

fn tool_request_binding<'a>(
    run_id: &'a str,
    spool: &'a ToolRequestSpool,
) -> ToolRequestBinding<'a> {
    ToolRequestBinding {
        version: "1",
        run_id,
        lease_id: &spool.lease_id,
        endpoint_lease_id: &spool.endpoint_lease_id,
        consumer: &spool.consumer,
        request_directory: spool.target_path.join("requests"),
        response_directory: spool.target_path.join("responses"),
        capability_path: spool.target_path.join(".capability"),
        request_filename: "<request_id>.json",
        response_filename: "<request_id>.json",
        max_pending_requests: MAX_TOOL_REQUESTS,
        max_request_bytes: MAX_TOOL_REQUEST_BYTES,
        max_spool_bytes: MAX_TOOL_REQUEST_QUOTA_BYTES,
        max_response_bytes: MAX_TOOL_RESPONSE_BYTES,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum ManagedSourceKind {
    #[default]
    File,
    Directory,
    Socket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedMountExpectation {
    ReadOnlyBind {
        source_kind: ManagedSourceKind,
        scope: LeaseScope,
    },
    WritableRequestSpool,
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
    consumer_uid: Option<u32>,
    #[serde(default)]
    endpoint_lease_id: Option<String>,
    #[serde(default)]
    request_spool_target: Option<PathBuf>,
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
    SharedDirectory,
    ToolEndpoint,
    ToolRequest,
    PlatformTunnel,
}

impl LeaseScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::ModelIdentity => "model-identity",
            Self::SourceControlIdentity => "source-control-identity",
            Self::ProjectEnvironment => "project-environment",
            Self::ProviderEnvironment => "provider-environment",
            Self::SharedDirectory => "shared-directory",
            Self::ToolEndpoint => "tool-endpoint",
            Self::ToolRequest => "tool-request",
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
    #[serde(default)]
    sha256: Option<String>,
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
    tool_request_spools: Vec<ToolRequestSpool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_request_ledger_path: Option<PathBuf>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(default)]
    source_kind: ManagedSourceKind,
}

#[derive(Debug, Default)]
struct OwnedDockerIdentity {
    container_ids: BTreeSet<String>,
    compose_projects: BTreeSet<String>,
}

pub(super) struct InGuestRuntimeProvider {
    devcontainer: PathBuf,
    docker: PathBuf,
    timeout: PathBuf,
}

impl InGuestRuntimeProvider {
    pub(super) fn new() -> Result<Self> {
        let devcontainer = resolve_binary("BRANCHBOX_DEVCONTAINER_PATH", "devcontainer")?;
        let docker = resolve_binary("BRANCHBOX_DOCKER_PATH", "docker")?;
        let timeout = if let Some(path) = std::env::var_os("BRANCHBOX_TIMEOUT_PATH") {
            let path = PathBuf::from(path);
            if !path.is_file() {
                return Err(Error::validation(
                    "BRANCHBOX_TIMEOUT_PATH does not point to a regular executable file",
                ));
            }
            path
        } else {
            PathBuf::from("timeout")
        };
        Ok(Self {
            devcontainer,
            docker,
            timeout,
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

    fn bounded_docker_command(&self) -> Command {
        let mut command = self.command(&self.timeout);
        command.args(["-s", "KILL", "35s"]).arg(&self.docker);
        command
    }

    fn bounded_docker_output(&self, args: &[&str]) -> Result<Output> {
        self.bounded_docker_command()
            .args(args)
            .output()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute bounded in-guest Docker CLI '{}': {err}",
                    self.docker.display()
                ))
            })
    }

    fn bounded_docker_resource_exists(&self, args: &[&str]) -> Result<bool> {
        let output = self.bounded_docker_output(args)?;
        if output.status.success() {
            return Ok(true);
        }
        if matches!(output.status.code(), Some(124 | 137)) {
            return Err(Error::validation(
                "Timed out while inspecting a managed tool-request Docker resource",
            ));
        }
        Ok(false)
    }

    fn resolve_container_user_identity(
        &self,
        container_id: &str,
        configured_user: Option<&str>,
    ) -> Result<(String, u32)> {
        let user = if let Some(user) = configured_user.filter(|user| !user.is_empty()) {
            user.to_string()
        } else {
            let output = self.bounded_docker_output(&[
                "inspect",
                "--format",
                "{{.Config.User}}",
                container_id,
            ])?;
            if !output.status.success() {
                return Err(Error::validation(format!(
                    "Could not inspect the primary devcontainer user: {}",
                    bounded_failure(&output.stderr)
                )));
            }
            inspected_container_user(&output.stdout)?
        };
        validate_container_user_selector(&user)?;
        let output =
            self.bounded_docker_output(&["exec", "--user", &user, container_id, "id", "-u"])?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Could not resolve the primary devcontainer user UID: {}",
                bounded_failure(&output.stderr)
            )));
        }
        let uid = parse_container_uid(&output.stdout)?;
        Ok((user, uid))
    }

    fn bind_tool_request_consumer_identity(
        &self,
        container_id: &str,
        metadata: &mut RuntimeMetadata,
    ) -> Result<()> {
        let in_guest = metadata.in_guest.as_ref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing assignment identity")
        })?;
        let state = Self::read_state(&in_guest.state_path)?;
        let assignment = load_assignment(&state.manifest_path)?;
        if assignment.tool_request_spools.is_empty() {
            return Ok(());
        }
        let (user, uid) =
            self.resolve_container_user_identity(container_id, metadata.container_user.as_deref())?;
        validate_tool_request_consumer_uid(&assignment.tool_request_spools, uid)?;
        metadata.container_user = Some(user);
        Ok(())
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
        let output = self.devcontainer_output(
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
        )?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "in-guest devcontainer startup failed (diagnostic={}): {}. In-guest startup strips host identity, platform secret hooks, ambient env files, and supervisor Docker access; repository primary commands and container lifecycle hooks must tolerate that boundary",
                devcontainer_start_failure_code(&output.stderr),
                bounded_failure(&output.stderr)
            )));
        }
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
        let (signed_mounts, forbidden_environment) =
            if let Some(identity) = metadata.in_guest.as_ref() {
                let state = Self::read_state(&identity.state_path)?;
                let assignment = load_assignment(&state.manifest_path)?;
                (
                    assignment.signed_mounts(),
                    assignment.provider_environment_names(),
                )
            } else {
                (BTreeMap::new(), BTreeSet::new())
            };
        validate_container_inspection(&output.stdout, &signed_mounts, &forbidden_environment)
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

    fn initialize_tool_request_spools(
        &self,
        container_id: &str,
        metadata: &RuntimeMetadata,
    ) -> Result<()> {
        let in_guest = metadata.in_guest.as_ref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing assignment identity")
        })?;
        let state = Self::read_state(&in_guest.state_path)?;
        let assignment = load_assignment(&state.manifest_path)?;
        if state.tool_request_spools != assignment.tool_request_spools {
            return Err(Error::validation(
                "Tool-request spool state differs from the managed assignment",
            ));
        }
        for spool in &assignment.tool_request_spools {
            let mut capability =
                read_tool_request_capability(&spool.capability_source, &spool.capability_sha256)?;
            let binding = tool_request_binding(&in_guest.run_id, spool);
            let binding = serde_json::to_vec(&binding)?;
            let mut command = self.bounded_docker_command();
            command
                .args([
                    "exec",
                    "--interactive",
                    "--user",
                    "0",
                    container_id,
                    "sh",
                    "-c",
                ])
                .arg(INITIALIZE_TOOL_REQUEST_SPOOL_SCRIPT)
                .arg("branchbox-tool-request-spool")
                .arg(&spool.target_path)
                .arg(spool.consumer_uid.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().map_err(|err| {
                Error::validation(format!(
                    "Failed to initialize a managed tool-request spool: {err}"
                ))
            })?;
            let write_result = (|| -> Result<()> {
                let mut stdin = child.stdin.take().ok_or_else(|| {
                    Error::validation("Managed tool-request initializer has no stdin pipe")
                })?;
                stdin.write_all(capability.as_bytes())?;
                stdin.write_all(b"\n")?;
                stdin.write_all(&binding)?;
                stdin.write_all(b"\n")?;
                Ok(())
            })();
            unsafe { capability.as_bytes_mut() }.fill(0);
            capability.clear();
            let output = child.wait_with_output();
            write_result?;
            let output = output?;
            if !output.status.success() {
                return Err(Error::validation(format!(
                    "Managed tool-request spool initialization failed: {}",
                    bounded_failure(&output.stderr)
                )));
            }
        }
        Ok(())
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
        self.remove_tool_request_volumes(&state.tool_request_spools);
        self.inspect_residue(state, &identity.compose_projects)
    }

    fn remove_tool_request_volumes(&self, spools: &[ToolRequestSpool]) {
        for spool in spools {
            let _ = self.bounded_docker_output(&["volume", "rm", &spool.volume_name]);
        }
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
        let mut request_volumes = Vec::new();
        for spool in &state.tool_request_spools {
            if self.bounded_docker_resource_exists(&["volume", "inspect", &spool.volume_name])? {
                request_volumes.push(spool.volume_name.clone());
            }
        }
        if !request_volumes.is_empty() {
            residue.push(RuntimeResidue {
                kind: "tool-request-volume".to_string(),
                identifiers: request_volumes,
            });
        }
        Ok(residue)
    }

    fn erase_materializations(&self, state: &ProviderState, residue: &mut Vec<RuntimeResidue>) {
        let mut remaining = Vec::new();
        for materialization in &state.materializations {
            let removal = match materialization.source_kind {
                ManagedSourceKind::Directory => fs::remove_dir_all(&materialization.source_path),
                ManagedSourceKind::File | ManagedSourceKind::Socket => {
                    fs::remove_file(&materialization.source_path)
                }
            };
            match removal {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => remaining.push(materialization.source_path.display().to_string()),
            }
            if fs::symlink_metadata(&materialization.source_path).is_ok()
                && !remaining.contains(&materialization.source_path.display().to_string())
            {
                remaining.push(materialization.source_path.display().to_string());
            }
        }
        if !remaining.is_empty() {
            residue.push(RuntimeResidue {
                kind: "lease-materialization".to_string(),
                identifiers: remaining,
            });
        }
        if let Some(ledger) = state.tool_request_ledger_path.as_ref() {
            match fs::remove_dir_all(ledger) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => residue.push(RuntimeResidue {
                    kind: "tool-request-ledger".to_string(),
                    identifiers: vec![ledger.display().to_string()],
                }),
            }
            if ledger.exists()
                && !residue
                    .iter()
                    .any(|entry| entry.kind == "tool-request-ledger")
            {
                residue.push(RuntimeResidue {
                    kind: "tool-request-ledger".to_string(),
                    identifiers: vec![ledger.display().to_string()],
                });
            }
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
        command.arg(container_id);
        append_secure_container_exec(&mut command, "branchbox-in-guest-provider", provider, args);
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
                    value: read_provider_environment_value(
                        &mount.source,
                        mount.sha256.as_deref().ok_or_else(|| {
                            Error::validation("Provider environment binding is missing its digest")
                        })?,
                    )?,
                })
            })
            .collect()
    }

    fn read_spooled_tool_request(
        &self,
        container_id: &str,
        spool: &ToolRequestSpool,
        request_id: &str,
    ) -> Result<Vec<u8>> {
        validate_tool_request_id(request_id)?;
        let mut command = self.bounded_docker_command();
        let output = command
            .args(["exec", "--user", "0", container_id, "sh", "-c"])
            .arg(READ_TOOL_REQUEST_SCRIPT)
            .arg("branchbox-read-tool-request")
            .arg(&spool.target_path)
            .arg(spool.consumer_uid.to_string())
            .arg(request_id)
            .arg(MAX_TOOL_REQUESTS.to_string())
            .arg(MAX_TOOL_REQUEST_BYTES.to_string())
            .arg(MAX_TOOL_REQUEST_QUOTA_BYTES.to_string())
            .output()
            .map_err(|err| {
                Error::validation(format!("Failed to read a managed tool request: {err}"))
            })?;
        if !output.status.success() {
            if output.status.code() == Some(75) {
                return Err(Error::ToolRequestNotPending {
                    lease_id: spool.lease_id.clone(),
                    request_id: request_id.to_string(),
                });
            }
            return Err(Error::validation(format!(
                "Managed tool-request spool failed validation: {}",
                bounded_failure(&output.stderr)
            )));
        }
        if output.stdout.is_empty() || output.stdout.len() > MAX_TOOL_REQUEST_BYTES {
            return Err(Error::validation(
                "Managed tool request exceeds its bounded file size",
            ));
        }
        Ok(output.stdout)
    }

    fn remove_spooled_tool_request(
        &self,
        container_id: &str,
        spool: &ToolRequestSpool,
        request_id: &str,
    ) -> Result<()> {
        validate_tool_request_id(request_id)?;
        let output = self
            .bounded_docker_command()
            .args([
                "exec",
                "--user",
                "0",
                container_id,
                "sh",
                "-c",
                "set -eu; root=$1; request_id=$2; test -d \"$root/requests\"; rm -f \"$root/requests/$request_id.json\"; test ! -e \"$root/requests/$request_id.json\"",
                "branchbox-remove-tool-request",
                &spool.target_path.to_string_lossy(),
                request_id,
            ])
            .output()?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Managed tool-request cleanup failed: {}",
                bounded_failure(&output.stderr)
            )));
        }
        Ok(())
    }

    fn write_spooled_tool_response(
        &self,
        container_id: &str,
        spool: &ToolRequestSpool,
        request_id: &str,
        response: &ToolRelayResponse,
    ) -> Result<()> {
        validate_tool_request_id(request_id)?;
        let mut bytes = serde_json::to_vec(response)?;
        if bytes.is_empty() || bytes.len() > MAX_TOOL_RESPONSE_BYTES {
            return Err(Error::validation(
                "Trusted tool response exceeds its bounded spool size",
            ));
        }
        bytes.push(b'\n');
        let mut command = self.bounded_docker_command();
        command
            .args([
                "exec",
                "--interactive",
                "--user",
                "0",
                container_id,
                "sh",
                "-c",
            ])
            .arg(WRITE_TOOL_RESPONSE_SCRIPT)
            .arg("branchbox-write-tool-response")
            .arg(&spool.target_path)
            .arg(spool.consumer_uid.to_string())
            .arg(request_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|err| {
            Error::validation(format!("Failed to write a managed tool response: {err}"))
        })?;
        let write_result = (|| -> Result<()> {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                Error::validation("Managed tool response writer has no stdin pipe")
            })?;
            stdin.write_all(&bytes)?;
            Ok(())
        })();
        bytes.fill(0);
        let output = child.wait_with_output();
        write_result?;
        let output = output?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Managed tool response spool write failed: {}",
                bounded_failure(&output.stderr)
            )));
        }
        Ok(())
    }

    fn claim_tool_request(
        &self,
        state: &ProviderState,
        lease_id: &str,
        request_id: &str,
    ) -> Result<(PathBuf, PathBuf)> {
        validate_opaque_identifier(lease_id, "tool request lease_id")?;
        validate_tool_request_id(request_id)?;
        let ledger_root = state.tool_request_ledger_path.as_ref().ok_or_else(|| {
            Error::validation("Managed runtime state has no tool-request replay ledger")
        })?;
        create_owner_only_directory(ledger_root)?;
        let lease_root = ledger_root.join(safe_identity(lease_id)?);
        create_owner_only_directory(&lease_root)?;
        let stem = safe_identity(request_id)?;
        let claim = lease_root.join(format!("{stem}.claim"));
        let done = lease_root.join(format!("{stem}.done"));
        if done.exists() {
            return Err(Error::validation(
                "Managed tool request was already dispatched",
            ));
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&claim)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::validation(
                        "Managed tool request is already claimed; automatic replay is denied",
                    )
                } else {
                    Error::validation(format!(
                        "Could not create the managed tool-request replay claim: {err}"
                    ))
                }
            })?;
        drop(file);
        set_owner_only(&claim)?;
        Ok((claim, done))
    }

    fn dispatch_tool_request(
        &self,
        metadata: &RuntimeMetadata,
        lease_id: &str,
        request_id: &str,
    ) -> Result<RuntimeToolDispatchResult> {
        let in_guest = metadata.in_guest.as_ref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing assignment identity")
        })?;
        let container_id = metadata.container_id.as_deref().ok_or_else(|| {
            Error::validation("In-guest runtime metadata is missing the devcontainer ID")
        })?;
        self.verify_untrusted_boundary(container_id, metadata)?;
        let state = Self::read_state(&in_guest.state_path)?;
        let assignment = load_assignment(&state.manifest_path)?;
        let spool = assignment.tool_request_spool(lease_id).ok_or_else(|| {
            Error::validation("Managed assignment has no matching tool-request lease")
        })?;
        if !state.tool_request_spools.contains(spool) {
            return Err(Error::validation(
                "Tool-request spool is not recorded in managed runtime state",
            ));
        }
        let endpoint = assignment
            .tool_endpoint(&spool.endpoint_lease_id)
            .ok_or_else(|| Error::validation("Managed tool endpoint is unavailable"))?;
        let (_, consumer_uid) =
            self.resolve_container_user_identity(container_id, metadata.container_user.as_deref())?;
        validate_tool_request_consumer_uid(std::slice::from_ref(spool), consumer_uid)?;
        let mut bytes = self.read_spooled_tool_request(container_id, spool, request_id)?;
        let request = validate_tool_request_envelope(&bytes, &in_guest.run_id, spool, request_id);
        bytes.fill(0);
        let request = request?;
        let (claim, done) = self.claim_tool_request(&state, lease_id, request_id)?;
        let relay = ToolRelayRequest {
            version: "1",
            run_id: &request.run_id,
            lease_id: &request.lease_id,
            consumer: &request.consumer,
            request_id: &request.request_id,
            payload: &request.payload,
        };
        let response = relay_tool_request(&endpoint.source, &relay)?;
        validate_tool_response_binding(&response, &request)?;
        self.write_spooled_tool_response(container_id, spool, request_id, &response)
            .map_err(|err| {
                Error::validation(format!(
                    "Trusted tool responded but its consumer response could not be committed; replay remains blocked: {err}"
                ))
            })?;
        fs::rename(&claim, &done).map_err(|err| {
            Error::validation(format!(
                "Trusted tool responded but its replay ledger could not be finalized: {err}"
            ))
        })?;
        self.remove_spooled_tool_request(container_id, spool, request_id)
            .map_err(|err| {
                Error::validation(format!(
                    "Trusted tool request was delivered and replay-blocked, but spool cleanup failed: {err}"
                ))
            })?;
        Ok(RuntimeToolDispatchResult {
            run_id: request.run_id.clone(),
            lease_id: request.lease_id.clone(),
            consumer: request.consumer.clone(),
            request_id: request.request_id.clone(),
            response: response.payload,
        })
    }
}

struct ProviderEnvironmentValue {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolRequestEnvelope {
    version: String,
    run_id: String,
    lease_id: String,
    consumer: String,
    request_id: String,
    capability: String,
    payload: serde_json::Value,
}

impl Drop for ToolRequestEnvelope {
    fn drop(&mut self) {
        unsafe { self.capability.as_bytes_mut() }.fill(0);
        self.capability.clear();
    }
}

#[derive(Debug, Serialize)]
struct ToolRelayRequest<'a> {
    version: &'static str,
    run_id: &'a str,
    lease_id: &'a str,
    consumer: &'a str,
    request_id: &'a str,
    payload: &'a serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolRelayResponse {
    version: String,
    run_id: String,
    lease_id: String,
    consumer: String,
    request_id: String,
    payload: serde_json::Value,
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

fn validate_tool_request_id(value: &str) -> Result<()> {
    validate_opaque_identifier(value, "tool request request_id")?;
    if value.starts_with('.') {
        return Err(Error::validation(
            "Tool request request_id may not be a hidden filename",
        ));
    }
    Ok(())
}

fn validate_tool_request_envelope(
    bytes: &[u8],
    run_id: &str,
    spool: &ToolRequestSpool,
    request_id: &str,
) -> Result<ToolRequestEnvelope> {
    if bytes.is_empty() || bytes.len() > MAX_TOOL_REQUEST_BYTES {
        return Err(Error::validation(
            "Managed tool request exceeds its bounded file size",
        ));
    }
    let request: ToolRequestEnvelope = serde_json::from_slice(bytes)
        .map_err(|_| Error::validation("Managed tool request is not canonical JSON"))?;
    if request.version != "1"
        || request.run_id != run_id
        || request.lease_id != spool.lease_id
        || request.consumer != spool.consumer
        || request.request_id != request_id
    {
        return Err(Error::validation(
            "Managed tool request does not match its exact run, lease, consumer, and request binding",
        ));
    }
    let mut expected_capability =
        read_tool_request_capability(&spool.capability_source, &spool.capability_sha256)?;
    let capability_matches = constant_time_bytes_equal(
        request.capability.as_bytes(),
        expected_capability.as_bytes(),
    );
    unsafe { expected_capability.as_bytes_mut() }.fill(0);
    expected_capability.clear();
    if !capability_matches {
        return Err(Error::validation(
            "Managed tool request capability is invalid",
        ));
    }
    Ok(request)
}

fn validate_tool_response_binding(
    response: &ToolRelayResponse,
    request: &ToolRequestEnvelope,
) -> Result<()> {
    if response.version != "1"
        || response.run_id != request.run_id
        || response.lease_id != request.lease_id
        || response.consumer != request.consumer
        || response.request_id != request.request_id
    {
        return Err(Error::validation(
            "Trusted tool response does not match the claimed request binding",
        ));
    }
    Ok(())
}

fn create_owner_only_directory(path: &Path) -> Result<()> {
    if path.exists() {
        validate_private_directory(path, "tool-request replay ledger")?;
        return Ok(());
    }
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)?;
    }
    validate_private_directory(path, "tool-request replay ledger")?;
    Ok(())
}

fn constant_time_bytes_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}

#[cfg(unix)]
fn relay_tool_request(
    endpoint: &Path,
    request: &ToolRelayRequest<'_>,
) -> Result<ToolRelayResponse> {
    let mut stream = UnixStream::connect(endpoint).map_err(|err| {
        Error::validation(format!("Trusted tool endpoint rejected dispatch: {err}"))
    })?;
    stream.set_read_timeout(Some(TOOL_RELAY_TIMEOUT))?;
    stream.set_write_timeout(Some(TOOL_RELAY_TIMEOUT))?;
    let mut bytes = serde_json::to_vec(request)?;
    if bytes.len() > MAX_TOOL_REQUEST_BYTES {
        return Err(Error::validation(
            "Canonical tool request exceeds its bounded relay size",
        ));
    }
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    bytes.fill(0);
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut response = Vec::new();
    stream
        .take((MAX_TOOL_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)?;
    if response.is_empty() || response.len() > MAX_TOOL_RESPONSE_BYTES {
        return Err(Error::validation(
            "Trusted tool response exceeds its bounded relay size",
        ));
    }
    serde_json::from_slice(&response)
        .map_err(|_| Error::validation("Trusted tool returned an invalid response envelope"))
}

#[cfg(not(unix))]
fn relay_tool_request(
    _endpoint: &Path,
    _request: &ToolRelayRequest<'_>,
) -> Result<ToolRelayResponse> {
    Err(Error::validation(
        "Trusted Unix tool dispatch is supported only inside a Unix guest",
    ))
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
        let container_user = configured_container_user(&config);
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
        let tool_request_ledger_path = (!assignment.tool_request_spools.is_empty())
            .then(|| state_path.with_extension("tool-request-ledger"));
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
                    source_kind: mount.source_kind,
                })
                .collect(),
            tool_request_spools: assignment.tool_request_spools.clone(),
            tool_request_ledger_path,
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
            container_user,
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
            self.bind_tool_request_consumer_identity(&container_id, metadata)?;
            self.initialize_tool_request_spools(&container_id, metadata)?;
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
        process.args([
            "exec",
            "--workspace-folder",
            &worktree_path.to_string_lossy(),
            "--config",
            &config.to_string_lossy(),
        ]);
        append_secure_container_exec(
            &mut process,
            "branchbox-in-guest-exec",
            &command[0],
            &command[1..],
        );
        process.current_dir(worktree_path);
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
        process.args([
            "exec",
            "--workspace-folder",
            &worktree_path.to_string_lossy(),
            "--config",
            &config.to_string_lossy(),
        ]);
        append_secure_container_exec(
            &mut process,
            "branchbox-in-guest-exec",
            &command[0],
            &command[1..],
        );
        let status = process.current_dir(worktree_path).status().map_err(|err| {
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

    fn dispatch_tool_request(
        &self,
        metadata: &RuntimeMetadata,
        lease_id: &str,
        request_id: &str,
    ) -> Result<RuntimeToolDispatchResult> {
        InGuestRuntimeProvider::dispatch_tool_request(self, metadata, lease_id, request_id)
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
    tool_request_spools: Vec<ToolRequestSpool>,
    linked_tool_endpoints: BTreeSet<String>,
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

    fn signed_mounts(&self) -> BTreeMap<(PathBuf, PathBuf), ManagedMountExpectation> {
        let mut signed = self
            .mounts
            .iter()
            .filter_map(|mount| {
                let MaterializationTarget::File(target) = &mount.target else {
                    return None;
                };
                if !matches!(
                    mount.scope,
                    LeaseScope::SharedDirectory | LeaseScope::ToolEndpoint
                ) || (mount.scope == LeaseScope::ToolEndpoint
                    && self.linked_tool_endpoints.contains(&mount.lease_id))
                {
                    return None;
                }
                Some((
                    (mount.source.clone(), target.clone()),
                    ManagedMountExpectation::ReadOnlyBind {
                        source_kind: mount.source_kind,
                        scope: mount.scope,
                    },
                ))
            })
            .collect::<BTreeMap<_, _>>();
        signed.extend(self.tool_request_spools.iter().map(|spool| {
            (
                (PathBuf::from(&spool.volume_name), spool.target_path.clone()),
                ManagedMountExpectation::WritableRequestSpool,
            )
        }));
        signed
    }

    fn tool_request_spool(&self, lease_id: &str) -> Option<&ToolRequestSpool> {
        self.tool_request_spools
            .iter()
            .find(|spool| spool.lease_id == lease_id)
    }

    fn tool_endpoint(&self, lease_id: &str) -> Option<&InGuestMount> {
        self.mounts.iter().find(|mount| {
            mount.lease_id == lease_id
                && mount.scope == LeaseScope::ToolEndpoint
                && mount.source_kind == ManagedSourceKind::Socket
        })
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
        tool_request_spools: assignment.tool_request_spools,
        linked_tool_endpoints: assignment.linked_tool_endpoints,
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
    let has_file_materializations = manifest.leases.iter().any(|lease| {
        !lease.materializations.is_empty()
            && !matches!(
                lease.scope,
                LeaseScope::SharedDirectory | LeaseScope::ToolEndpoint
            )
    });
    let materialization_root = if has_file_materializations {
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
    let mut tool_request_consumers = BTreeSet::new();
    let mut tool_request_spools = Vec::new();
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
                || matches!(
                    lease.scope,
                    LeaseScope::ProviderEnvironment
                        | LeaseScope::SharedDirectory
                        | LeaseScope::ToolEndpoint
                        | LeaseScope::ToolRequest
                ))
        {
            return Err(Error::validation(
                "Version 1 assignments cannot declare managed v2 bindings",
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
        if lease.scope != LeaseScope::ToolRequest
            && (lease.consumer_uid.is_some()
                || lease.endpoint_lease_id.is_some()
                || lease.request_spool_target.is_some())
        {
            return Err(Error::validation(
                "Only tool-request leases may declare a consumer UID, endpoint link, or request spool target",
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
        if matches!(
            lease.scope,
            LeaseScope::SharedDirectory | LeaseScope::ToolEndpoint
        ) && (lease.materializations.len() != 1
            || lease
                .expires_at
                .is_none_or(|expires_at| expires_at <= Utc::now()))
        {
            return Err(Error::validation(
                "Managed directory and tool-endpoint leases require one live materialization",
            ));
        }
        if lease.scope == LeaseScope::ToolRequest {
            tool_request_consumers.insert(lease.consumer.clone());
            let consumer_uid = lease.consumer_uid.ok_or_else(|| {
                Error::validation("Tool-request leases require one non-root consumer UID")
            })?;
            if consumer_uid == 0 || consumer_uid > i32::MAX as u32 {
                return Err(Error::validation(
                    "Tool-request consumer UID must be a non-root Linux UID",
                ));
            }
            let endpoint_lease_id = lease.endpoint_lease_id.as_deref().ok_or_else(|| {
                Error::validation("Tool-request leases require an exact endpoint lease binding")
            })?;
            validate_opaque_identifier(endpoint_lease_id, "lease.endpoint_lease_id")?;
            let request_spool_target = lease.request_spool_target.as_deref().ok_or_else(|| {
                Error::validation("Tool-request leases require an exact request spool target")
            })?;
            validate_managed_mount_target(request_spool_target, TOOL_REQUEST_TARGET_ROOT)?;
            if lease.materializations.len() != 1
                || lease
                    .expires_at
                    .is_none_or(|expires_at| expires_at <= Utc::now())
            {
                return Err(Error::validation(
                    "Tool-request leases require one live capability materialization",
                ));
            }
        }
        for materialization in &lease.materializations {
            let (source, source_kind) = if matches!(
                lease.scope,
                LeaseScope::SharedDirectory | LeaseScope::ToolEndpoint
            ) {
                if materialization.sha256.is_some() {
                    return Err(Error::validation(
                        "Managed directory and tool-endpoint leases must not declare a file digest",
                    ));
                }
                validate_run_owned_managed_source(
                    &manifest_path,
                    &manifest.run_id,
                    &materialization.source_path,
                    lease.scope,
                )?
            } else {
                let source = validate_private_regular_file(
                    &materialization.source_path,
                    "lease materialization",
                )?;
                if !source.starts_with(materialization_root.as_ref().expect("validated above")) {
                    return Err(Error::validation(
                        "Lease materialization source escapes the assignment materializations directory",
                    ));
                }
                (source, ManagedSourceKind::File)
            };
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
                    if lease.scope == LeaseScope::SharedDirectory {
                        validate_managed_mount_target(target, SHARED_DIRECTORY_TARGET_ROOT)?;
                    }
                    if lease.scope == LeaseScope::ToolEndpoint {
                        validate_managed_mount_target(target, TOOL_ENDPOINT_TARGET_ROOT)?;
                    }
                    if lease.scope == LeaseScope::ToolRequest {
                        let spool = lease
                            .request_spool_target
                            .as_ref()
                            .expect("validated above");
                        if target != &spool.join(".capability") {
                            return Err(Error::validation(
                                "Tool-request capability target must be .capability below its exact request spool target",
                            ));
                        }
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
            if let Some(expected_sha256) = materialization.sha256.as_deref() {
                validate_sha256(expected_sha256)?;
                let actual = file_sha256(&source)?;
                if actual != expected_sha256 {
                    return Err(Error::validation(format!(
                        "Lease materialization digest mismatch for lease '{}'",
                        lease.lease_id
                    )));
                }
            } else if !matches!(
                lease.scope,
                LeaseScope::SharedDirectory | LeaseScope::ToolEndpoint
            ) {
                return Err(Error::validation(
                    "File materializations require a SHA-256 digest",
                ));
            }
            if sources.iter().any(|existing: &PathBuf| {
                source.starts_with(existing) || existing.starts_with(&source)
            }) || !sources.insert(source.clone())
            {
                return Err(Error::validation(
                    "Lease materialization sources must be unique and non-overlapping",
                ));
            }
            if lease.scope == LeaseScope::ProjectEnvironment {
                validate_project_environment_file(&source)?;
            }
            if lease.scope == LeaseScope::ProviderEnvironment {
                validate_provider_environment_value_file(
                    &source,
                    materialization.sha256.as_deref().expect("validated above"),
                )?;
            }
            if lease.scope == LeaseScope::ToolRequest {
                validate_tool_request_capability_file(
                    &source,
                    materialization.sha256.as_deref().expect("validated above"),
                )?;
                tool_request_spools.push(ToolRequestSpool {
                    lease_id: lease.lease_id.clone(),
                    endpoint_lease_id: lease.endpoint_lease_id.clone().expect("validated above"),
                    consumer: lease.consumer.clone(),
                    consumer_uid: lease.consumer_uid.expect("validated above"),
                    target_path: lease.request_spool_target.clone().expect("validated above"),
                    volume_name: tool_request_volume_name(&manifest.run_id, &lease.lease_id),
                    capability_source: source.clone(),
                    capability_sha256: materialization.sha256.clone().expect("validated above"),
                });
            }
            mounts.push(InGuestMount {
                lease_id: lease.lease_id.clone(),
                source,
                target,
                sha256: materialization.sha256.clone(),
                source_kind,
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
            } else if lease.scope == LeaseScope::SharedDirectory {
                "primary-read-only-directory".to_string()
            } else if lease.scope == LeaseScope::ToolEndpoint {
                "primary-read-only-endpoint".to_string()
            } else if lease.scope == LeaseScope::ToolRequest {
                "consumer-request-spool".to_string()
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
    if !tool_request_consumers.is_subset(&model_consumers) {
        return Err(Error::validation(
            "Tool-request consumers require an exact model-identity binding",
        ));
    }
    let mut linked_tool_endpoints = BTreeSet::new();
    for spool in &tool_request_spools {
        let endpoint_lease = manifest
            .leases
            .iter()
            .find(|lease| lease.lease_id == spool.endpoint_lease_id)
            .ok_or_else(|| {
                Error::validation("Tool-request lease references an unknown endpoint lease")
            })?;
        if endpoint_lease.scope != LeaseScope::ToolEndpoint
            || endpoint_lease.consumer != spool.consumer
            || endpoint_lease.expires_at.is_none_or(|expires_at| {
                expires_at
                    < manifest
                        .leases
                        .iter()
                        .find(|lease| lease.lease_id == spool.lease_id)
                        .and_then(|lease| lease.expires_at)
                        .expect("tool-request expiry validated")
            })
        {
            return Err(Error::validation(
                "Tool-request lease must bind one live socket endpoint for the same consumer and lifetime",
            ));
        }
        let endpoint_mounts = mounts
            .iter()
            .filter(|mount| mount.lease_id == endpoint_lease.lease_id)
            .collect::<Vec<_>>();
        if endpoint_mounts.len() != 1 || endpoint_mounts[0].source_kind != ManagedSourceKind::Socket
        {
            return Err(Error::validation(
                "Tool-request lease must bind one owner-only Unix socket endpoint",
            ));
        }
        if !linked_tool_endpoints.insert(spool.endpoint_lease_id.clone()) {
            return Err(Error::validation(
                "A trusted tool endpoint may back only one request spool",
            ));
        }
    }
    for lease in &mut leases {
        if linked_tool_endpoints.contains(&lease.lease_id) {
            lease.state = "trusted-relay-endpoint".to_string();
        }
    }
    Ok(LoadedAssignment {
        manifest_path,
        manifest,
        mounts,
        leases,
        tool_request_spools,
        linked_tool_endpoints,
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

fn validate_run_owned_managed_source(
    manifest_path: &Path,
    run_id: &str,
    source_path: &Path,
    scope: LeaseScope,
) -> Result<(PathBuf, ManagedSourceKind)> {
    let run_root = manifest_path.parent().ok_or_else(|| {
        Error::validation("Managed assignment manifest must have a run directory")
    })?;
    if run_root.file_name().and_then(|name| name.to_str()) != Some(run_id) {
        return Err(Error::validation(
            "Managed mount assignment directory must exactly match run_id",
        ));
    }
    if !source_path.is_absolute()
        || source_path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::validation(
            "Managed mount source must be an exact path below its assignment run directory",
        ));
    }
    let source = fs::canonicalize(source_path)?;
    let canonical_run_root = fs::canonicalize(run_root)?;
    if !source.starts_with(&canonical_run_root) || source == canonical_run_root {
        return Err(Error::validation(
            "Managed mount source must be an exact path below its assignment run directory",
        ));
    }
    if contains_ambient_authority_reference(&source.to_string_lossy()) {
        return Err(Error::validation(
            "Managed mount source may not expose ambient supervisor authority or secret paths",
        ));
    }

    let manifest_owner = path_owner(&fs::metadata(manifest_path)?);
    let run_owner = validate_private_directory(run_root, "assignment run directory")?;
    if manifest_owner != run_owner {
        return Err(Error::validation(
            "Managed assignment manifest and run directory must have the same owner",
        ));
    }

    let source_parent = source.parent().ok_or_else(|| {
        Error::validation("Managed mount source must have a private parent directory")
    })?;
    let relative_parent = source_parent
        .strip_prefix(&canonical_run_root)
        .map_err(|_| {
            Error::validation("Managed mount source escapes its assignment run directory")
        })?;
    let mut current = canonical_run_root.clone();
    for component in relative_parent.components() {
        let Component::Normal(component) = component else {
            return Err(Error::validation(
                "Managed mount source path is not normalized",
            ));
        };
        current.push(component);
        let owner = validate_private_directory(&current, "managed source directory")?;
        if owner != run_owner {
            return Err(Error::validation(
                "Managed source directories must retain exact run ownership",
            ));
        }
    }

    let metadata = fs::symlink_metadata(&source)
        .map_err(|err| Error::validation(format!("Cannot inspect managed mount source: {err}")))?;
    if path_owner(&metadata) != run_owner {
        return Err(Error::validation(
            "Managed mount source must retain exact run ownership",
        ));
    }
    let source_kind = managed_source_kind(&metadata).ok_or_else(|| {
        Error::validation("Managed mount source must be a non-symlink directory or Unix socket")
    })?;
    if scope == LeaseScope::SharedDirectory && source_kind != ManagedSourceKind::Directory {
        return Err(Error::validation(
            "Shared-directory leases require an owner-only source directory",
        ));
    }
    validate_managed_source_kind(&source, source_kind, scope)?;
    Ok((source, source_kind))
}

fn validate_private_directory(path: &Path, description: &str) -> Result<Option<u32>> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| Error::validation(format!("Cannot inspect {description}: {err}")))?;
    if !metadata.file_type().is_dir() {
        return Err(Error::validation(format!(
            "In-guest {description} must be a non-symlink directory"
        )));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(Error::validation(format!(
            "In-guest {description} must use owner-only 0700 permissions"
        )));
    }
    Ok(path_owner(&metadata))
}

fn path_owner(metadata: &fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        Some(metadata.uid())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn managed_source_kind(metadata: &fs::Metadata) -> Option<ManagedSourceKind> {
    if metadata.file_type().is_dir() {
        return Some(ManagedSourceKind::Directory);
    }
    #[cfg(unix)]
    if metadata.file_type().is_socket() {
        return Some(ManagedSourceKind::Socket);
    }
    None
}

fn validate_managed_source_kind(
    path: &Path,
    expected: ManagedSourceKind,
    scope: LeaseScope,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| Error::validation(format!("Cannot inspect managed mount source: {err}")))?;
    if managed_source_kind(&metadata) != Some(expected) {
        return Err(Error::validation(
            "Managed mount source type changed after assignment validation",
        ));
    }
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        let safe = match expected {
            ManagedSourceKind::Directory if scope == LeaseScope::SharedDirectory => mode == 0o755,
            ManagedSourceKind::Directory => mode == 0o700,
            ManagedSourceKind::Socket => mode & 0o077 == 0 && mode & 0o600 == 0o600,
            ManagedSourceKind::File => false,
        };
        if !safe {
            return Err(Error::validation(
                "Managed mount source no longer has its scope-compatible permissions",
            ));
        }
    }
    Ok(())
}

fn validate_lease_target(path: &Path) -> Result<()> {
    let root = Path::new(LEASE_TARGET_ROOT);
    if !path.is_absolute()
        || !path.starts_with(root)
        || path == root
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::validation(format!(
            "Lease target must be an individual path below {LEASE_TARGET_ROOT}"
        )));
    }
    Ok(())
}

fn validate_managed_mount_target(path: &Path, root: &str) -> Result<()> {
    let root = Path::new(root);
    if path == root || !path.starts_with(root) {
        return Err(Error::validation(format!(
            "Managed mount target must be an individual path below {}",
            root.display()
        )));
    }
    if contains_ambient_authority_reference(&path.to_string_lossy()) {
        return Err(Error::validation(
            "Managed mount target may not expose ambient supervisor authority or secret paths",
        ));
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

fn validate_tool_request_capability_file(path: &Path, expected_sha256: &str) -> Result<()> {
    read_tool_request_capability(path, expected_sha256).map(|mut value| {
        unsafe { value.as_bytes_mut() }.fill(0);
        value.clear();
    })
}

fn read_tool_request_capability(path: &Path, expected_sha256: &str) -> Result<String> {
    let bytes = fs::read(path)?;
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    if !(32..=128).contains(&bytes.len())
        || actual_sha256 != expected_sha256
        || bytes
            .iter()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(*byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::validation(
            "Tool-request capability materialization is invalid",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| Error::validation("Tool-request capability materialization is invalid"))
}

fn tool_request_volume_name(run_id: &str, lease_id: &str) -> String {
    let digest = Sha256::digest(format!("{run_id}\0{lease_id}").as_bytes());
    format!("branchbox-tool-requests-{:x}", digest)[..56].to_string()
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

fn contains_ambient_authority_reference(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    contains_project_docker_reference(&normalized)
        || [
            "/.gnupg",
            "/.ssh",
            "/run/secrets",
            "/var/run/secrets",
            "ssh_auth_sock",
            "serviceaccount",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn validate_container_inspection(
    source: &[u8],
    signed_mounts: &BTreeMap<(PathBuf, PathBuf), ManagedMountExpectation>,
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
    let required_seccomp = host
        .get("SecurityOpt")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|options| {
            options
                .iter()
                .any(|option| option.as_str() == Some(REQUIRED_SECCOMP_SECURITY_OPTION))
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
        || !required_seccomp
    {
        return Err(Error::validation(
            "In-guest devcontainer resolved to privileged host authority",
        ));
    }
    let allowed_sources = signed_mounts
        .iter()
        .filter_map(|((source, _), expectation)| {
            matches!(expectation, ManagedMountExpectation::ReadOnlyBind { .. })
                .then_some(source.clone())
        })
        .collect::<BTreeSet<_>>();
    let mut observed_signed_mounts = BTreeSet::new();
    if let Some(mounts) = container
        .get("Mounts")
        .and_then(serde_json::Value::as_array)
    {
        for mount in mounts {
            let mount_type = mount
                .get("Type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let source = mount
                .get("Source")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let signed_source = if mount_type == "volume" {
                mount
                    .get("Name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
            } else {
                source
            };
            let destination = mount
                .get("Destination")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let key = (PathBuf::from(signed_source), PathBuf::from(destination));
            let signed_expectation = signed_mounts.get(&key);
            let enters_managed_namespace = Path::new(source).starts_with(MANAGED_RUNTIME_ROOT)
                || Path::new(destination).starts_with(LEASE_TARGET_ROOT);
            let mut exact_request_volume = false;
            if let Some(expectation) = signed_expectation {
                let valid = match expectation {
                    ManagedMountExpectation::ReadOnlyBind { source_kind, scope } => {
                        mount_type == "bind"
                            && mount.get("RW").and_then(serde_json::Value::as_bool) == Some(false)
                            && validate_managed_source_kind(Path::new(source), *source_kind, *scope)
                                .is_ok()
                    }
                    ManagedMountExpectation::WritableRequestSpool => {
                        exact_request_volume = mount_type == "volume"
                            && !signed_source.is_empty()
                            && mount.get("RW").and_then(serde_json::Value::as_bool) == Some(true);
                        exact_request_volume
                    }
                };
                if !valid || !observed_signed_mounts.insert(key) {
                    return Err(Error::validation(
                        "Managed lease mount lacks its exact signed access evidence",
                    ));
                }
            } else if enters_managed_namespace {
                return Err(Error::validation(
                    "In-guest devcontainer resolved an unsigned managed lease mount",
                ));
            }
            if (!exact_request_volume && contains_supervisor_mount(source, &allowed_sources))
                || contains_supervisor_mount(destination, &BTreeSet::new())
            {
                return Err(Error::validation(
                    "In-guest devcontainer resolved a supervisor socket or credential-directory mount",
                ));
            }
        }
    }
    if observed_signed_mounts.len() != signed_mounts.len() {
        return Err(Error::validation(
            "Managed lease mount is missing from the primary devcontainer inspection",
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
        || ((normalized == MANAGED_RUNTIME_ROOT
            || normalized.starts_with(&format!("{MANAGED_RUNTIME_ROOT}/")))
            && !allowed_sources.contains(Path::new(value)))
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

fn configured_container_user(config: &DevcontainerConfig) -> Option<String> {
    config
        .remote_user
        .as_deref()
        .or(config.container_user.as_deref())
        .map(ToOwned::to_owned)
}

fn inspected_container_user(source: &[u8]) -> Result<String> {
    let inspected = std::str::from_utf8(source)
        .map_err(|_| Error::validation("Docker returned a non-UTF-8 container user"))?
        .trim();
    if inspected.is_empty() {
        Ok("0".to_string())
    } else {
        Ok(inspected.to_string())
    }
}

fn validate_container_user_selector(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.starts_with('-')
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | ':' | '-')
        })
    {
        return Err(Error::validation(
            "Primary devcontainer user is not a bounded Docker user selector",
        ));
    }
    Ok(())
}

fn parse_container_uid(source: &[u8]) -> Result<u32> {
    let source = std::str::from_utf8(source)
        .map_err(|_| Error::validation("Container UID resolution returned non-UTF-8 output"))?;
    let source = source.trim();
    if source.is_empty() || !source.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::validation(
            "Container UID resolution did not return one numeric UID",
        ));
    }
    let uid = source
        .parse::<u32>()
        .map_err(|_| Error::validation("Container UID is outside the supported Linux UID range"))?;
    if uid > i32::MAX as u32 {
        return Err(Error::validation(
            "Container UID is outside the supported Linux UID range",
        ));
    }
    Ok(uid)
}

fn validate_tool_request_consumer_uid(spools: &[ToolRequestSpool], actual_uid: u32) -> Result<()> {
    if spools.is_empty() {
        return Ok(());
    }
    if actual_uid == 0 {
        return Err(Error::validation(
            "Tool-request consumers must execute as a non-root devcontainer user",
        ));
    }
    if spools.iter().any(|spool| spool.consumer_uid != actual_uid) {
        return Err(Error::validation(
            "Tool-request consumer UID does not match the resolved devcontainer provider UID",
        ));
    }
    Ok(())
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

fn append_secure_container_exec(
    command: &mut Command,
    marker: &str,
    executable: &str,
    arguments: &[String],
) {
    command
        .args(["/bin/sh", "-lc", SECURE_EXEC_WRAPPER, marker, executable])
        .args(arguments);
}

fn devcontainer_start_failure_code(stderr: &[u8]) -> &'static str {
    let normalized = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if normalized.contains("security_opt items at") && normalized.contains("are equal") {
        "devcontainer_compose_duplicate_security_option"
    } else if normalized.contains("postcreatecommand") {
        "devcontainer_post_create_failed"
    } else if normalized.contains("docker compose") {
        "devcontainer_compose_start_failed"
    } else if normalized.contains("failed to build") || normalized.contains("buildx") {
        "devcontainer_image_build_failed"
    } else if normalized.contains("permission denied") {
        "devcontainer_permission_denied"
    } else {
        "devcontainer_start_failed"
    }
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
    fn private_directory(path: &Path) {
        fs::create_dir_all(path).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn devcontainer_start_failures_have_content_free_diagnostic_codes() {
        assert_eq!(
            devcontainer_start_failure_code(
                b"service rails-app.security_opt items at 0 and 1 are equal /private/path"
            ),
            "devcontainer_compose_duplicate_security_option"
        );
        assert_eq!(
            devcontainer_start_failure_code(b"postCreateCommand failed: token=private"),
            "devcontainer_post_create_failed"
        );
        assert_eq!(
            devcontainer_start_failure_code(b"unexpected secret-only failure"),
            "devcontainer_start_failed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn runtime_exec_restores_a_non_writable_default_mask_without_shell_interpolation() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("created-by-provider");
        let arguments = vec![output.to_string_lossy().to_string()];
        let mut process = Command::new("/bin/sh");
        process.args(["-c", "umask 0000; exec \"$@\"", "branchbox-outer-test"]);
        append_secure_container_exec(
            &mut process,
            "branchbox-inner-test",
            "/usr/bin/touch",
            &arguments,
        );

        assert!(process.status().unwrap().success());
        assert_eq!(fs::metadata(output).unwrap().mode() & 0o777, 0o644);
        assert_eq!(SECURE_EXEC_WRAPPER, "umask 0022; exec \"$@\"");
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
    fn managed_mount_assignment_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf)
    {
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let repository = workspace.join("repository");
        private_directory(&repository);
        let run_root = root.path().join("run_123");
        private_directory(&run_root);
        let shared_parent = run_root.join("shared");
        let endpoint_parent = run_root.join("tool-endpoints");
        private_directory(&shared_parent);
        private_directory(&endpoint_parent);
        let shared = shared_parent.join("exchange");
        let endpoint_directory = endpoint_parent.join("requests");
        let endpoint_socket = endpoint_parent.join("request-stream");
        private_directory(&shared);
        let mut shared_permissions = fs::metadata(&shared).unwrap().permissions();
        shared_permissions.set_mode(0o755);
        fs::set_permissions(&shared, shared_permissions).unwrap();
        private_directory(&endpoint_directory);
        let listener = UnixListener::bind(&endpoint_socket).unwrap();
        let mut permissions = fs::metadata(&endpoint_socket).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&endpoint_socket, permissions).unwrap();
        drop(listener);

        let manifest = serde_json::json!({
            "version": "2",
            "run_id": "run_123",
            "lease_id": "assignment_123",
            "outer_runtime_id": "runtime_123",
            "workspace": workspace,
            "repository": {
                "path": repository,
                "revision": "a".repeat(40)
            },
            "task_branch": "feature/coding-demo",
            "tunnel_placement": "outer",
            "published_ports": [],
            "leases": [
                {
                    "lease_id": "shared_exchange",
                    "scope": "shared-directory",
                    "consumer": "primary-tool",
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": [{
                        "source_path": shared.clone(),
                        "target_path": "/run/branchbox/leases/shared/exchange"
                    }]
                },
                {
                    "lease_id": "request_directory",
                    "scope": "tool-endpoint",
                    "consumer": "primary-tool",
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": [{
                        "source_path": endpoint_directory.clone(),
                        "target_path": "/run/branchbox/leases/tool-endpoints/requests"
                    }]
                },
                {
                    "lease_id": "request_socket",
                    "scope": "tool-endpoint",
                    "consumer": "primary-tool",
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": [{
                        "source_path": endpoint_socket.clone(),
                        "target_path": "/run/branchbox/leases/tool-endpoints/request-stream"
                    }]
                }
            ]
        });
        let manifest_path = run_root.join("assignment.json");
        private_write(
            &manifest_path,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        (
            root,
            manifest_path,
            shared,
            endpoint_directory,
            endpoint_socket,
        )
    }

    #[cfg(unix)]
    fn managed_tool_request_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, String) {
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let repository = workspace.join("repository");
        private_directory(&repository);
        let run_root = root.path().join("run_123");
        private_directory(&run_root);
        let materializations = run_root.join("materializations");
        let endpoint_parent = run_root.join("tool-endpoints");
        private_directory(&materializations);
        private_directory(&endpoint_parent);
        let capability = "request-capability-abcdefghijklmnopqrstuvwxyz012345".to_string();
        let capability_path = materializations.join("request-capability");
        private_write(&capability_path, capability.as_bytes());
        let endpoint_socket = endpoint_parent.join("delivery.sock");
        let listener = UnixListener::bind(&endpoint_socket).unwrap();
        let mut permissions = fs::metadata(&endpoint_socket).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&endpoint_socket, permissions).unwrap();
        drop(listener);
        let manifest = serde_json::json!({
            "version": "2",
            "run_id": "run_123",
            "lease_id": "assignment_123",
            "outer_runtime_id": "runtime_123",
            "workspace": workspace,
            "repository": {
                "path": repository,
                "revision": "a".repeat(40)
            },
            "task_branch": "feature/coding-demo",
            "tunnel_placement": "outer",
            "published_ports": [],
            "leases": [
                {
                    "lease_id": "model_identity",
                    "scope": "model-identity",
                    "consumer": "coding-agent",
                    "executable": "provider-cli",
                    "inherited_environment": [],
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": []
                },
                {
                    "lease_id": "delivery_endpoint",
                    "scope": "tool-endpoint",
                    "consumer": "coding-agent",
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": [{
                        "source_path": endpoint_socket.clone(),
                        "target_path": "/run/branchbox/leases/tool-endpoints/delivery.sock"
                    }]
                },
                {
                    "lease_id": "delivery_requests",
                    "scope": "tool-request",
                    "consumer": "coding-agent",
                    "consumer_uid": 1000,
                    "endpoint_lease_id": "delivery_endpoint",
                    "request_spool_target": "/run/branchbox/leases/tool-requests/delivery",
                    "expires_at": "2099-01-01T00:00:00Z",
                    "materializations": [{
                        "source_path": capability_path.clone(),
                        "target_path": "/run/branchbox/leases/tool-requests/delivery/.capability",
                        "sha256": file_sha256(&capability_path).unwrap()
                    }]
                }
            ]
        });
        let manifest_path = run_root.join("assignment.json");
        private_write(
            &manifest_path,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        (
            root,
            manifest_path,
            endpoint_socket,
            capability_path,
            capability,
        )
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
    fn managed_mount_leases_bind_only_signed_run_owned_directories_and_sockets() {
        let (_root, manifest, shared, endpoint_directory, endpoint_socket) =
            managed_mount_assignment_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let signed = assignment.signed_mounts();
        let plan = InGuestFacadePlan {
            manifest_path: manifest,
            tunnel_placement: InGuestTunnelPlacement::Outer,
            published_ports: Vec::new(),
            mounts: assignment.mounts.clone(),
            tool_request_spools: Vec::new(),
            linked_tool_endpoints: BTreeSet::new(),
        };

        assert_eq!(signed.len(), 3);
        assert_eq!(plan.mounts().count(), 3);
        assert_eq!(
            signed.get(&(
                shared.canonicalize().unwrap(),
                PathBuf::from("/run/branchbox/leases/shared/exchange")
            )),
            Some(&ManagedMountExpectation::ReadOnlyBind {
                source_kind: ManagedSourceKind::Directory,
                scope: LeaseScope::SharedDirectory
            })
        );
        assert_eq!(
            signed.get(&(
                endpoint_directory.canonicalize().unwrap(),
                PathBuf::from("/run/branchbox/leases/tool-endpoints/requests")
            )),
            Some(&ManagedMountExpectation::ReadOnlyBind {
                source_kind: ManagedSourceKind::Directory,
                scope: LeaseScope::ToolEndpoint
            })
        );
        assert_eq!(
            signed.get(&(
                endpoint_socket.canonicalize().unwrap(),
                PathBuf::from("/run/branchbox/leases/tool-endpoints/request-stream")
            )),
            Some(&ManagedMountExpectation::ReadOnlyBind {
                source_kind: ManagedSourceKind::Socket,
                scope: LeaseScope::ToolEndpoint
            })
        );
        assert!(assignment
            .leases
            .iter()
            .all(|lease| lease.state.starts_with("primary-read-only-")));
    }

    #[cfg(unix)]
    #[test]
    fn tool_request_spool_is_the_only_rw_mount_and_private_endpoint_is_never_mounted() {
        let (_root, manifest, endpoint, _capability_path, _capability) =
            managed_tool_request_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let spool = assignment.tool_request_spool("delivery_requests").unwrap();
        let signed = assignment.signed_mounts();
        let plan = InGuestFacadePlan {
            manifest_path: manifest,
            tunnel_placement: InGuestTunnelPlacement::Outer,
            published_ports: Vec::new(),
            mounts: assignment.mounts.clone(),
            tool_request_spools: assignment.tool_request_spools.clone(),
            linked_tool_endpoints: assignment.linked_tool_endpoints.clone(),
        };

        assert_eq!(signed.len(), 1);
        assert_eq!(plan.mounts().count(), 0);
        assert_eq!(plan.tool_request_spools().count(), 1);
        assert!(!signed.keys().any(|(source, _)| source == &endpoint));
        assert_eq!(
            signed.get(&(
                PathBuf::from(&spool.volume_name),
                PathBuf::from("/run/branchbox/leases/tool-requests/delivery")
            )),
            Some(&ManagedMountExpectation::WritableRequestSpool)
        );
        assert_eq!(
            assignment
                .leases
                .iter()
                .find(|lease| lease.lease_id == "delivery_endpoint")
                .unwrap()
                .state,
            "trusted-relay-endpoint"
        );

        let binding = serde_json::to_value(tool_request_binding("run_123", spool)).unwrap();
        assert_eq!(binding["run_id"], "run_123");
        assert_eq!(binding["lease_id"], "delivery_requests");
        assert_eq!(binding["consumer"], "coding-agent");
        assert_eq!(
            binding["request_directory"],
            "/run/branchbox/leases/tool-requests/delivery/requests"
        );
        assert_eq!(
            binding["response_directory"],
            "/run/branchbox/leases/tool-requests/delivery/responses"
        );
        assert!(INITIALIZE_TOOL_REQUEST_SPOOL_SCRIPT
            .contains("chown 0:0 \"$capability_tmp\" \"$binding_tmp\""));
        assert!(INITIALIZE_TOOL_REQUEST_SPOOL_SCRIPT.contains("chmod 0444"));
        assert!(!INITIALIZE_TOOL_REQUEST_SPOOL_SCRIPT.contains("chmod 0777"));
    }

    #[cfg(unix)]
    #[test]
    fn tool_request_manifest_fails_closed_for_uid_link_target_and_capability_drift() {
        let (_root, manifest, _endpoint, _capability_path, _capability) =
            managed_tool_request_fixture();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"][2]["consumer_uid"] = serde_json::json!(0);
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());

        let (_root, manifest, _endpoint, _capability_path, _capability) =
            managed_tool_request_fixture();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"][2]["endpoint_lease_id"] = serde_json::json!("unknown_endpoint");
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());

        let (_root, manifest, _endpoint, _capability_path, _capability) =
            managed_tool_request_fixture();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"][2]["request_spool_target"] = serde_json::json!("/tmp/requests");
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());

        let (_root, manifest, _endpoint, capability_path, _capability) =
            managed_tool_request_fixture();
        private_write(
            &capability_path,
            b"changed-capability-abcdefghijklmnopqrstuvwxyz",
        );
        assert!(load_assignment(&manifest).is_err());

        let (root, manifest, _endpoint, _capability_path, capability) =
            managed_tool_request_fixture();
        let misplaced_capability = root
            .path()
            .join("run_123/tool-endpoints/request-capability");
        private_write(&misplaced_capability, capability.as_bytes());
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["leases"][2]["materializations"][0]["source_path"] =
            serde_json::json!(misplaced_capability);
        value["leases"][2]["materializations"][0]["sha256"] =
            serde_json::json!(file_sha256(&misplaced_capability).unwrap());
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn tool_request_consumer_uid_must_match_the_resolved_non_root_provider_user() {
        let (_root, manifest, _endpoint, _capability_path, _capability) =
            managed_tool_request_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let config = DevcontainerConfig::default();
        assert_eq!(configured_container_user(&config), None);
        assert_eq!(inspected_container_user(b"vscode\n").unwrap(), "vscode");
        assert_eq!(inspected_container_user(b"\n").unwrap(), "0");
        assert_eq!(parse_container_uid(b"1000\n").unwrap(), 1000);
        assert!(parse_container_uid(b"vscode\n").is_err());
        validate_tool_request_consumer_uid(&assignment.tool_request_spools, 1000).unwrap();
        assert!(validate_tool_request_consumer_uid(&assignment.tool_request_spools, 0).is_err());
        assert!(validate_tool_request_consumer_uid(&assignment.tool_request_spools, 1001).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn final_inspection_accepts_only_the_exact_writable_request_volume() {
        let (_root, manifest, endpoint, _capability_path, _capability) =
            managed_tool_request_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let signed = assignment.signed_mounts();
        let ((volume, target), _) = signed.first_key_value().unwrap();
        let inspection = serde_json::json!([{
            "HostConfig": {
                "Privileged": false,
                "PidMode": "",
                "IpcMode": "private",
                "SecurityOpt": ["seccomp=builtin"]
            },
            "Mounts": [{
                "Type": "volume",
                "Name": volume,
                "Source": format!("/var/lib/docker/volumes/{}/_data", volume.display()),
                "Destination": target,
                "RW": true
            }],
            "Config": {"Env": []}
        }]);
        let rendered = serde_json::to_vec(&inspection).unwrap();
        assert!(!String::from_utf8_lossy(&rendered).contains(endpoint.to_string_lossy().as_ref()));
        validate_container_inspection(&rendered, &signed, &BTreeSet::new()).unwrap();

        for (field, value) in [
            ("RW", serde_json::json!(false)),
            ("Type", serde_json::json!("bind")),
            ("Name", serde_json::json!("branchbox-tool-requests-wrong")),
        ] {
            let mut invalid = inspection.clone();
            invalid[0]["Mounts"][0][field] = value;
            assert!(validate_container_inspection(
                &serde_json::to_vec(&invalid).unwrap(),
                &signed,
                &BTreeSet::new()
            )
            .is_err());
        }
        let mut missing_name = inspection;
        missing_name[0]["Mounts"][0]
            .as_object_mut()
            .unwrap()
            .remove("Name");
        assert!(validate_container_inspection(
            &serde_json::to_vec(&missing_name).unwrap(),
            &signed,
            &BTreeSet::new()
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn tool_request_envelope_and_response_require_exact_capability_and_correlation() {
        let (_root, manifest, _endpoint, _capability_path, capability) =
            managed_tool_request_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let spool = assignment.tool_request_spool("delivery_requests").unwrap();
        let envelope = serde_json::json!({
            "version": "1",
            "run_id": "run_123",
            "lease_id": "delivery_requests",
            "consumer": "coding-agent",
            "request_id": "artifact-publish",
            "capability": capability,
            "payload": {"operation": "opaque"}
        });
        let request = validate_tool_request_envelope(
            &serde_json::to_vec(&envelope).unwrap(),
            "run_123",
            spool,
            "artifact-publish",
        )
        .unwrap();
        let response = ToolRelayResponse {
            version: "1".to_string(),
            run_id: "run_123".to_string(),
            lease_id: "delivery_requests".to_string(),
            consumer: "coding-agent".to_string(),
            request_id: "artifact-publish".to_string(),
            payload: serde_json::json!({"accepted": true}),
        };
        validate_tool_response_binding(&response, &request).unwrap();

        for field in ["run_id", "lease_id", "consumer", "request_id", "capability"] {
            let mut invalid = envelope.clone();
            invalid[field] = serde_json::json!("wrong-binding");
            assert!(validate_tool_request_envelope(
                &serde_json::to_vec(&invalid).unwrap(),
                "run_123",
                spool,
                "artifact-publish",
            )
            .is_err());
        }
        let mut wrong_response = response;
        wrong_response.request_id = "wrong-request".to_string();
        assert!(validate_tool_response_binding(&wrong_response, &request).is_err());

        let declined_response = ToolRelayResponse {
            version: "1".to_string(),
            run_id: "run_123".to_string(),
            lease_id: "delivery_requests".to_string(),
            consumer: "coding-agent".to_string(),
            request_id: "artifact-publish".to_string(),
            payload: serde_json::json!({
                "ok": false,
                "fallback": "durable-artifact-link",
                "reason": "tool policy declined the optional upload"
            }),
        };
        validate_tool_response_binding(&declined_response, &request).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn trusted_relay_uses_one_eof_terminated_frame_and_never_forwards_the_capability() {
        use std::os::unix::net::UnixListener;
        use std::thread;

        let (_root, manifest, endpoint, _capability_path, capability) =
            managed_tool_request_fixture();
        fs::remove_file(&endpoint).unwrap();
        let listener = UnixListener::bind(&endpoint).unwrap();
        let mut permissions = fs::metadata(&endpoint).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&endpoint, permissions).unwrap();
        let capability_for_server = capability.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut frame = Vec::new();
            stream.read_to_end(&mut frame).unwrap();
            assert_eq!(frame.iter().filter(|byte| **byte == b'\n').count(), 1);
            assert_eq!(frame.last(), Some(&b'\n'));
            assert!(!frame
                .windows(capability_for_server.len())
                .any(|window| window == capability_for_server.as_bytes()));
            let request: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            assert!(request.get("capability").is_none());
            assert_eq!(request["request_id"], "artifact-delivery");
            let response = serde_json::json!({
                "version": "1",
                "run_id": request["run_id"],
                "lease_id": request["lease_id"],
                "consumer": request["consumer"],
                "request_id": request["request_id"],
                "payload": {"accepted": true}
            });
            stream
                .write_all(&serde_json::to_vec(&response).unwrap())
                .unwrap();
        });

        let assignment = load_assignment(&manifest).unwrap();
        let spool = assignment.tool_request_spool("delivery_requests").unwrap();
        let envelope = serde_json::json!({
            "version": "1",
            "run_id": "run_123",
            "lease_id": "delivery_requests",
            "consumer": "coding-agent",
            "request_id": "artifact-delivery",
            "capability": capability,
            "payload": {"operation": "opaque"}
        });
        let request = validate_tool_request_envelope(
            &serde_json::to_vec(&envelope).unwrap(),
            "run_123",
            spool,
            "artifact-delivery",
        )
        .unwrap();
        let relay = ToolRelayRequest {
            version: "1",
            run_id: &request.run_id,
            lease_id: &request.lease_id,
            consumer: &request.consumer,
            request_id: &request.request_id,
            payload: &request.payload,
        };
        let response = relay_tool_request(&endpoint, &relay).unwrap();
        validate_tool_response_binding(&response, &request).unwrap();
        assert_eq!(response.payload, serde_json::json!({"accepted": true}));
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn provider_state_and_client_descriptor_never_serialize_capability_bytes() {
        let (root, manifest, _endpoint, _capability_path, capability) =
            managed_tool_request_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let spool = assignment.tool_request_spool("delivery_requests").unwrap();
        let state = ProviderState {
            version: PROVIDER_STATE_VERSION.to_string(),
            manifest_path: manifest,
            worktree_path: root.path().join("worktree"),
            workspace_paths: Vec::new(),
            config_path: root.path().join("devcontainer.json"),
            run_id: Some("run_123".to_string()),
            outer_runtime_id: Some("runtime_123".to_string()),
            materializations: Vec::new(),
            tool_request_spools: assignment.tool_request_spools.clone(),
            tool_request_ledger_path: Some(root.path().join("ledger")),
            proxy_names: Vec::new(),
            compose_projects: Vec::new(),
            container_id: Some("container_123".to_string()),
        };
        let state_bytes = serde_json::to_vec(&state).unwrap();
        let binding_bytes = serde_json::to_vec(&tool_request_binding("run_123", spool)).unwrap();
        for bytes in [&state_bytes, &binding_bytes] {
            assert!(!bytes
                .windows(capability.len())
                .any(|window| window == capability.as_bytes()));
        }
        assert!(!INITIALIZE_TOOL_REQUEST_SPOOL_SCRIPT.contains(&capability));
        assert!(!READ_TOOL_REQUEST_SCRIPT.contains(&capability));
        assert!(!WRITE_TOOL_RESPONSE_SCRIPT.contains(&capability));
    }

    #[cfg(unix)]
    #[test]
    fn only_exit_75_from_a_validated_spool_is_classified_as_retryable_absence() {
        fn executable_script(path: &Path, source: &str) {
            fs::write(path, source).unwrap();
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(path, permissions).unwrap();
        }

        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("absent-timeout");
        let malformed = root.path().join("malformed-timeout");
        executable_script(&absent, "#!/bin/sh\nexit 75\n");
        executable_script(&malformed, "#!/bin/sh\nexit 71\n");
        let spool = ToolRequestSpool {
            lease_id: "delivery_requests".to_string(),
            endpoint_lease_id: "delivery_endpoint".to_string(),
            consumer: "coding-agent".to_string(),
            consumer_uid: 1000,
            target_path: PathBuf::from("/run/branchbox/leases/tool-requests/delivery"),
            volume_name: "branchbox-tool-requests-test".to_string(),
            capability_source: root.path().join("capability"),
            capability_sha256: "0".repeat(64),
        };
        let mut provider = InGuestRuntimeProvider {
            devcontainer: PathBuf::from("devcontainer"),
            docker: PathBuf::from("docker"),
            timeout: absent,
        };
        assert!(matches!(
            provider.read_spooled_tool_request("container", &spool, "artifact-delivery"),
            Err(Error::ToolRequestNotPending { .. })
        ));
        provider.timeout = malformed;
        assert!(matches!(
            provider.read_spooled_tool_request("container", &spool, "artifact-delivery"),
            Err(Error::Validation(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn replay_ledger_denies_pending_and_completed_request_reuse() {
        let root = tempfile::tempdir().unwrap();
        let ledger = root.path().join("ledger");
        let state = ProviderState {
            version: PROVIDER_STATE_VERSION.to_string(),
            manifest_path: root.path().join("assignment.json"),
            worktree_path: root.path().join("worktree"),
            workspace_paths: Vec::new(),
            config_path: root.path().join("devcontainer.json"),
            run_id: Some("run_123".to_string()),
            outer_runtime_id: Some("runtime_123".to_string()),
            materializations: Vec::new(),
            tool_request_spools: Vec::new(),
            tool_request_ledger_path: Some(ledger),
            proxy_names: Vec::new(),
            compose_projects: Vec::new(),
            container_id: None,
        };
        let provider = InGuestRuntimeProvider {
            devcontainer: PathBuf::from("devcontainer"),
            docker: PathBuf::from("docker"),
            timeout: PathBuf::from("timeout"),
        };
        let (claim, done) = provider
            .claim_tool_request(&state, "delivery_requests", "request_123")
            .unwrap();
        assert!(provider
            .claim_tool_request(&state, "delivery_requests", "request_123")
            .is_err());
        fs::rename(claim, done).unwrap();
        assert!(provider
            .claim_tool_request(&state, "delivery_requests", "request_123")
            .is_err());
        let mut residue = Vec::new();
        provider.erase_materializations(&state, &mut residue);
        assert!(!state.tool_request_ledger_path.as_ref().unwrap().exists());
        assert!(residue.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn tool_request_teardown_erases_capability_endpoint_and_replay_ledger() {
        let (root, manifest, endpoint, capability_path, _capability) =
            managed_tool_request_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let ledger = root.path().join("tool-request-ledger");
        let state = ProviderState {
            version: PROVIDER_STATE_VERSION.to_string(),
            manifest_path: manifest,
            worktree_path: root.path().join("worktree"),
            workspace_paths: Vec::new(),
            config_path: root.path().join("devcontainer.json"),
            run_id: Some("run_123".to_string()),
            outer_runtime_id: Some("runtime_123".to_string()),
            materializations: assignment
                .mounts
                .iter()
                .map(|mount| StateMaterialization {
                    source_path: mount.source.clone(),
                    sha256: mount.sha256.clone(),
                    source_kind: mount.source_kind,
                })
                .collect(),
            tool_request_spools: assignment.tool_request_spools,
            tool_request_ledger_path: Some(ledger.clone()),
            proxy_names: Vec::new(),
            compose_projects: Vec::new(),
            container_id: None,
        };
        let volume_name = state.tool_request_spools[0].volume_name.clone();
        let removal_log = root.path().join("volume-removal.log");
        let fake_timeout = root.path().join("fake-timeout");
        fs::write(
            &fake_timeout,
            format!(
                "#!/bin/sh\nshift 3\nprintf '%s\\n' \"$*\" > '{}'\n",
                removal_log.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&fake_timeout).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake_timeout, permissions).unwrap();
        let provider = InGuestRuntimeProvider {
            devcontainer: PathBuf::from("devcontainer"),
            docker: PathBuf::from("docker"),
            timeout: fake_timeout,
        };
        provider
            .claim_tool_request(&state, "delivery_requests", "artifact-delivery")
            .unwrap();
        provider.remove_tool_request_volumes(&state.tool_request_spools);
        assert_eq!(
            fs::read_to_string(&removal_log).unwrap(),
            format!("docker volume rm {volume_name}\n")
        );
        let mut residue = Vec::new();
        provider.erase_materializations(&state, &mut residue);
        assert!(fs::symlink_metadata(endpoint).is_err());
        assert!(fs::symlink_metadata(capability_path).is_err());
        assert!(fs::symlink_metadata(ledger).is_err());
        assert!(residue.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn managed_mount_leases_require_exact_run_target_type_and_scope_compatible_paths() {
        let (_root, manifest, shared, _endpoint_directory, endpoint_socket) =
            managed_mount_assignment_fixture();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();

        value["run_id"] = serde_json::json!("different_run");
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());

        value["run_id"] = serde_json::json!("run_123");
        value["leases"][0]["materializations"][0]["target_path"] =
            serde_json::json!("/tmp/exchange");
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());

        value["leases"][0]["materializations"][0]["target_path"] =
            serde_json::json!("/run/branchbox/leases/shared/exchange");
        let mut permissions = fs::metadata(&shared).unwrap().permissions();
        permissions.set_mode(0o777);
        fs::set_permissions(&shared, permissions).unwrap();
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());

        let mut permissions = fs::metadata(&shared).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shared, permissions).unwrap();
        fs::remove_file(&endpoint_socket).unwrap();
        private_write(&endpoint_socket, b"not-a-socket");
        private_write(&manifest, &serde_json::to_vec(&value).unwrap());
        assert!(load_assignment(&manifest).is_err());

        let (_root, legacy_manifest, _shared, _endpoint_directory, _endpoint_socket) =
            managed_mount_assignment_fixture();
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&legacy_manifest).unwrap()).unwrap();
        legacy["version"] = serde_json::json!("1");
        private_write(&legacy_manifest, &serde_json::to_vec(&legacy).unwrap());
        assert!(load_assignment(&legacy_manifest).is_err());

        let (outside_root, outside_manifest, _shared, _endpoint_directory, _endpoint_socket) =
            managed_mount_assignment_fixture();
        let outside = outside_root.path().join("outside-exchange");
        private_directory(&outside);
        let mut outside_value: serde_json::Value =
            serde_json::from_slice(&fs::read(&outside_manifest).unwrap()).unwrap();
        outside_value["leases"][0]["materializations"][0]["source_path"] =
            serde_json::json!(outside);
        private_write(
            &outside_manifest,
            &serde_json::to_vec(&outside_value).unwrap(),
        );
        assert!(load_assignment(&outside_manifest).is_err());

        let (_ambient_root, ambient_manifest, shared, _endpoint_directory, _endpoint_socket) =
            managed_mount_assignment_fixture();
        let ambient = shared.parent().unwrap().join(".ssh");
        fs::rename(&shared, &ambient).unwrap();
        let mut ambient_value: serde_json::Value =
            serde_json::from_slice(&fs::read(&ambient_manifest).unwrap()).unwrap();
        ambient_value["leases"][0]["materializations"][0]["source_path"] =
            serde_json::json!(ambient);
        private_write(
            &ambient_manifest,
            &serde_json::to_vec(&ambient_value).unwrap(),
        );
        assert!(load_assignment(&ambient_manifest).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn final_inspection_requires_every_exact_signed_bind_to_be_read_only() {
        let (_root, manifest, _shared, _endpoint_directory, _endpoint_socket) =
            managed_mount_assignment_fixture();
        let assignment = load_assignment(&manifest).unwrap();
        let signed = assignment.signed_mounts();
        let mount_values = signed
            .keys()
            .map(|(source, target)| {
                serde_json::json!({
                    "Type": "bind",
                    "Source": source,
                    "Destination": target,
                    "RW": false
                })
            })
            .collect::<Vec<_>>();
        let inspection = serde_json::json!([{
            "HostConfig": {
                "Privileged": false,
                "PidMode": "",
                "IpcMode": "private",
                "SecurityOpt": ["seccomp=builtin"]
            },
            "Mounts": mount_values,
            "Config": {"Env": []}
        }]);
        validate_container_inspection(
            &serde_json::to_vec(&inspection).unwrap(),
            &signed,
            &BTreeSet::new(),
        )
        .unwrap();

        let mut writable = inspection.clone();
        writable[0]["Mounts"][0]["RW"] = serde_json::json!(true);
        assert!(validate_container_inspection(
            &serde_json::to_vec(&writable).unwrap(),
            &signed,
            &BTreeSet::new(),
        )
        .is_err());

        let mut wrong_target = inspection.clone();
        wrong_target[0]["Mounts"][0]["Destination"] =
            serde_json::json!("/run/branchbox/leases/tool-endpoints/unsigned");
        assert!(validate_container_inspection(
            &serde_json::to_vec(&wrong_target).unwrap(),
            &signed,
            &BTreeSet::new(),
        )
        .is_err());

        let mut missing = inspection;
        missing[0]["Mounts"].as_array_mut().unwrap().pop();
        assert!(validate_container_inspection(
            &serde_json::to_vec(&missing).unwrap(),
            &signed,
            &BTreeSet::new(),
        )
        .is_err());
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
                &BTreeMap::new(),
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
                &BTreeMap::new(),
                &BTreeSet::new()
            )
            .is_err());
        }
    }

    #[test]
    fn accepts_unprivileged_container_with_no_supervisor_mounts_or_persisted_model_key() {
        let inspection = serde_json::json!([{
            "HostConfig": {
                "Privileged": false,
                "PidMode": "",
                "IpcMode": "private",
                "SecurityOpt": ["seccomp=builtin"]
            },
            "Mounts": [{"Source": "/workspace/agentify", "Destination": "/workspaces/agentify"}],
            "Config": {"Env": ["RAILS_ENV=development"]}
        }]);
        validate_container_inspection(
            &serde_json::to_vec(&inspection).unwrap(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_a_primary_container_without_the_managed_seccomp_profile() {
        let inspection = serde_json::json!([{
            "HostConfig": {"Privileged": false, "PidMode": "", "IpcMode": "private"},
            "Mounts": [],
            "Config": {"Env": []}
        }]);
        assert!(validate_container_inspection(
            &serde_json::to_vec(&inspection).unwrap(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .is_err());
    }

    #[test]
    #[ignore = "requires Docker and the cross-architecture Python conformance image"]
    fn builtin_seccomp_allows_unix_and_inet_but_denies_vsock() {
        let script = r#"import errno, socket
for family in (socket.AF_UNIX, socket.AF_INET):
    value = socket.socket(family, socket.SOCK_STREAM, 0)
    value.close()
try:
    socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM, 0)
except OSError as error:
    if error.errno == errno.EPERM:
        raise SystemExit(0)
    raise
raise SystemExit("AF_VSOCK unexpectedly opened")
"#;
        let status = Command::new("docker")
            .args([
                "run",
                "--rm",
                "--security-opt",
                REQUIRED_SECCOMP_SECURITY_OPTION,
                "python:3.13-alpine",
                "python3",
                "-c",
                script,
            ])
            .status()
            .expect("start seccomp conformance container");
        assert!(status.success());
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires Docker and the Alpine conformance image"]
    fn shared_directory_is_readable_but_not_writable_for_a_non_root_container_user() {
        struct DockerVolumeCleanup(String);

        impl Drop for DockerVolumeCleanup {
            fn drop(&mut self) {
                let _ = Command::new("docker")
                    .args(["volume", "rm", "--force", &self.0])
                    .output();
            }
        }

        let unique = tempfile::Builder::new()
            .prefix("branchbox-shared-evidence-")
            .tempdir()
            .unwrap();
        let suffix = unique
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace('_', "-");
        let volume = format!("branchbox-shared-evidence-{suffix}");
        let _cleanup = DockerVolumeCleanup(volume.clone());
        assert!(Command::new("docker")
            .args(["volume", "create", &volume])
            .status()
            .unwrap()
            .success());

        let writable_mount = format!("type=volume,src={volume},dst=/evidence");
        assert!(Command::new("docker")
            .args([
                "run",
                "--rm",
                "--network",
                "none",
                "--cap-drop",
                "ALL",
                "--security-opt",
                REQUIRED_SECCOMP_SECURITY_OPTION,
                "--mount",
                &writable_mount,
                "alpine:3.22",
                "sh",
                "-c",
                "printf 'finalized-evidence\\n' >/evidence/proof.txt && chmod 0444 /evidence/proof.txt && chmod 0755 /evidence",
            ])
            .status()
            .unwrap()
            .success());

        let readonly_mount =
            format!("type=volume,src={volume},dst=/run/branchbox/leases/shared/evidence,readonly");
        let status = Command::new("docker")
            .args([
                "run",
                "--rm",
                "--network",
                "none",
                "--cap-drop",
                "ALL",
                "--security-opt",
                REQUIRED_SECCOMP_SECURITY_OPTION,
                "--user",
                "1000:1000",
                "--mount",
            ])
            .arg(readonly_mount)
            .args([
                "alpine:3.22",
                "sh",
                "-c",
                "test \"$(cat /run/branchbox/leases/shared/evidence/proof.txt)\" = finalized-evidence && ! touch /run/branchbox/leases/shared/evidence/mutated && test ! -e /var/run/docker.sock && test ! -e /dev/vsock",
            ])
            .status()
            .expect("start shared-directory conformance container");
        assert!(status.success());
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires Docker and the Alpine conformance image"]
    fn tool_request_spool_enforces_consumer_and_dispatcher_filesystem_boundaries() {
        struct DockerCleanup {
            container: String,
            volume: String,
        }

        impl Drop for DockerCleanup {
            fn drop(&mut self) {
                let _ = Command::new("docker")
                    .args(["rm", "-f", &self.container])
                    .output();
                let _ = Command::new("docker")
                    .args(["volume", "rm", &self.volume])
                    .output();
            }
        }

        fn docker_exec(container: &str, user: &str, script: &str, args: &[&str]) -> Output {
            let mut command = Command::new("docker");
            command
                .args(["exec", "--user", user, container, "sh", "-c", script])
                .arg("branchbox-tool-request-conformance")
                .args(args)
                .output()
                .expect("run tool-request conformance command")
        }

        let unique = tempfile::Builder::new()
            .prefix("branchbox-tool-request-")
            .tempdir()
            .unwrap();
        let suffix = unique
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace('_', "-");
        let container = format!("branchbox-tool-request-{suffix}");
        let volume = format!("branchbox-tool-request-{suffix}");
        let _cleanup = DockerCleanup {
            container: container.clone(),
            volume: volume.clone(),
        };
        assert!(Command::new("docker")
            .args(["volume", "create", &volume])
            .status()
            .unwrap()
            .success());
        let mount = format!("type=volume,src={volume},dst=/spool");
        assert!(Command::new("docker")
            .args([
                "run",
                "--detach",
                "--name",
                &container,
                "--network",
                "none",
                "--user",
                "1000:1000",
                "--mount",
                &mount,
                "alpine:3.22",
                "sleep",
                "300",
            ])
            .status()
            .unwrap()
            .success());

        let passthrough_timeout = unique.path().join("passthrough-timeout");
        fs::write(&passthrough_timeout, b"#!/bin/sh\nshift 3\nexec \"$@\"\n").unwrap();
        let mut permissions = fs::metadata(&passthrough_timeout).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&passthrough_timeout, permissions).unwrap();
        let provider = InGuestRuntimeProvider {
            devcontainer: PathBuf::from("devcontainer"),
            docker: PathBuf::from("docker"),
            timeout: passthrough_timeout,
        };
        let (resolved_user, resolved_uid) = provider
            .resolve_container_user_identity(&container, None)
            .unwrap();
        assert_eq!(resolved_user, "1000:1000");
        assert_eq!(resolved_uid, 1000);

        let capability = "request-capability-abcdefghijklmnopqrstuvwxyz012345";
        let binding = serde_json::to_string(&serde_json::json!({
            "version": "1",
            "run_id": "run_123",
            "lease_id": "delivery_requests",
            "consumer": "coding-agent"
        }))
        .unwrap();
        let mut initialize = Command::new("docker")
            .args([
                "exec",
                "--interactive",
                "--user",
                "0",
                &container,
                "sh",
                "-c",
                INITIALIZE_TOOL_REQUEST_SPOOL_SCRIPT,
                "branchbox-tool-request-init",
                "/spool",
                "1000",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let stdin = initialize.stdin.as_mut().unwrap();
            writeln!(stdin, "{capability}").unwrap();
            writeln!(stdin, "{binding}").unwrap();
        }
        assert!(initialize.wait_with_output().unwrap().status.success());

        let consumer_check = docker_exec(
            &container,
            "1000:1000",
            "set -eu; root=$1; test \"$(cat \"$root/.capability\")\" = \"$2\"; test -s \"$root/.binding.json\"; ! printf changed >\"$root/.binding.json\"; ! rm \"$root/.binding.json\"; test ! -r \"$root/.processing\"",
            &["/spool", capability],
        );
        assert!(consumer_check.status.success());

        let envelope = serde_json::to_string(&serde_json::json!({
            "version": "1",
            "run_id": "run_123",
            "lease_id": "delivery_requests",
            "consumer": "coding-agent",
            "request_id": "artifact-delivery",
            "capability": capability,
            "payload": {"operation": "opaque"}
        }))
        .unwrap();
        let create_request = docker_exec(
            &container,
            "1000:1000",
            "set -eu; root=$1; request_id=$2; payload=$3; temporary=\"$root/requests/$request_id.tmp\"; final=\"$root/requests/$request_id.json\"; umask 077; printf '%s' \"$payload\" >\"$temporary\"; chmod 0600 \"$temporary\"; mv \"$temporary\" \"$final\"",
            &["/spool", "artifact-delivery", &envelope],
        );
        assert!(create_request.status.success());
        let request = docker_exec(
            &container,
            "0",
            READ_TOOL_REQUEST_SCRIPT,
            &[
                "/spool",
                "1000",
                "artifact-delivery",
                "16",
                "262144",
                "1048576",
            ],
        );
        assert!(request.status.success());
        assert_eq!(request.stdout, envelope.as_bytes());

        let response = serde_json::to_string(&serde_json::json!({
            "version": "1",
            "run_id": "run_123",
            "lease_id": "delivery_requests",
            "consumer": "coding-agent",
            "request_id": "artifact-delivery",
            "payload": {"accepted": true}
        }))
        .unwrap();
        let mut write_response = Command::new("docker")
            .args([
                "exec",
                "--interactive",
                "--user",
                "0",
                &container,
                "sh",
                "-c",
                WRITE_TOOL_RESPONSE_SCRIPT,
                "branchbox-tool-response-write",
                "/spool",
                "1000",
                "artifact-delivery",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write_response
            .stdin
            .as_mut()
            .unwrap()
            .write_all(response.as_bytes())
            .unwrap();
        assert!(write_response.wait_with_output().unwrap().status.success());
        let response_check = docker_exec(
            &container,
            "1000:1000",
            "set -eu; root=$1; expected=$2; test \"$(cat \"$root/responses/artifact-delivery.json\")\" = \"$expected\"; ! printf changed >\"$root/responses/artifact-delivery.json\"; ! rm \"$root/responses/artifact-delivery.json\"",
            &["/spool", &response],
        );
        assert!(response_check.status.success());

        let symlink = docker_exec(
            &container,
            "1000:1000",
            "ln -s /spool/.capability /spool/requests/symlink.json",
            &[],
        );
        assert!(symlink.status.success());
        let symlink_read = docker_exec(
            &container,
            "0",
            READ_TOOL_REQUEST_SCRIPT,
            &["/spool", "1000", "symlink", "16", "262144", "1048576"],
        );
        assert!(!symlink_read.status.success());
        assert_ne!(symlink_read.status.code(), Some(75));

        assert!(docker_exec(
            &container,
            "1000:1000",
            "set -eu; rm /spool/requests/symlink.json; printf x >/spool/requests/wrong-mode.json; chmod 0644 /spool/requests/wrong-mode.json",
            &[],
        )
        .status
        .success());
        let wrong_mode = docker_exec(
            &container,
            "0",
            READ_TOOL_REQUEST_SCRIPT,
            &["/spool", "1000", "wrong-mode", "16", "262144", "1048576"],
        );
        assert!(!wrong_mode.status.success());
        assert_ne!(wrong_mode.status.code(), Some(75));

        assert!(docker_exec(
            &container,
            "1000:1000",
            "set -eu; rm /spool/requests/wrong-mode.json; umask 077; dd if=/dev/zero of=/spool/requests/too-large.json bs=1024 count=257 2>/dev/null; chmod 0600 /spool/requests/too-large.json",
            &[],
        )
        .status
        .success());
        let too_large = docker_exec(
            &container,
            "0",
            READ_TOOL_REQUEST_SCRIPT,
            &["/spool", "1000", "too-large", "16", "262144", "1048576"],
        );
        assert!(!too_large.status.success());
        assert_ne!(too_large.status.code(), Some(75));

        assert!(docker_exec(
            &container,
            "1000:1000",
            "set -eu; rm /spool/requests/too-large.json; umask 077; for index in 1 2 3 4 5; do dd if=/dev/zero of=\"/spool/requests/quota-$index.json\" bs=1024 count=240 2>/dev/null; chmod 0600 \"/spool/requests/quota-$index.json\"; done",
            &[],
        )
        .status
        .success());
        let over_quota = docker_exec(
            &container,
            "0",
            READ_TOOL_REQUEST_SCRIPT,
            &["/spool", "1000", "quota-1", "16", "262144", "1048576"],
        );
        assert!(!over_quota.status.success());
        assert_ne!(over_quota.status.code(), Some(75));

        assert!(docker_exec(
            &container,
            "1000:1000",
            "set -eu; rm -f /spool/requests/*; umask 077; index=1; while test \"$index\" -le 17; do printf x >\"/spool/requests/count-$index.json\"; chmod 0600 \"/spool/requests/count-$index.json\"; index=$((index + 1)); done",
            &[],
        )
        .status
        .success());
        let over_count = docker_exec(
            &container,
            "0",
            READ_TOOL_REQUEST_SCRIPT,
            &["/spool", "1000", "count-1", "16", "262144", "1048576"],
        );
        assert!(!over_count.status.success());
        assert_ne!(over_count.status.code(), Some(75));

        assert!(docker_exec(
            &container,
            "0",
            "set -eu; rm -f /spool/requests/*; ln -s /spool/.capability /spool/responses/symlink-response.json",
            &[],
        )
        .status
        .success());
        let mut symlink_response = Command::new("docker")
            .args([
                "exec",
                "--interactive",
                "--user",
                "0",
                &container,
                "sh",
                "-c",
                WRITE_TOOL_RESPONSE_SCRIPT,
                "branchbox-tool-response-symlink",
                "/spool",
                "1000",
                "symlink-response",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        symlink_response
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"{}")
            .unwrap();
        assert!(!symlink_response
            .wait_with_output()
            .unwrap()
            .status
            .success());
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
            &BTreeMap::new(),
            &assignment.provider_environment_names(),
        )
        .is_err());

        let plan = InGuestFacadePlan {
            manifest_path: manifest,
            tunnel_placement: InGuestTunnelPlacement::Outer,
            published_ports: Vec::new(),
            mounts: assignment.mounts,
            tool_request_spools: Vec::new(),
            linked_tool_endpoints: BTreeSet::new(),
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
            value: read_provider_environment_value(
                &binding.source,
                binding.sha256.as_deref().unwrap(),
            )
            .unwrap(),
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
        assert!(read_provider_environment_value(
            &provider_secret,
            binding.sha256.as_deref().unwrap()
        )
        .is_err());
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
                sha256: Some(file_sha256(&provider_secret).unwrap()),
                source_kind: ManagedSourceKind::File,
            }],
            tool_request_spools: Vec::new(),
            tool_request_ledger_path: None,
            proxy_names: Vec::new(),
            compose_projects: Vec::new(),
            container_id: None,
        };
        let provider = InGuestRuntimeProvider {
            devcontainer: PathBuf::from("devcontainer"),
            docker: PathBuf::from("docker"),
            timeout: PathBuf::from("timeout"),
        };
        let mut residue = Vec::new();
        provider.erase_materializations(&state, &mut residue);
        assert!(!provider_secret.exists());
        assert!(residue.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn managed_directory_and_socket_cleanup_is_residue_checked() {
        let (root, manifest, shared, endpoint_directory, endpoint_socket) =
            managed_mount_assignment_fixture();
        private_write(&shared.join("outer-result"), b"result");
        private_write(&endpoint_directory.join("request"), b"request");
        let assignment = load_assignment(&manifest).unwrap();
        let state = ProviderState {
            version: PROVIDER_STATE_VERSION.to_string(),
            manifest_path: manifest,
            worktree_path: root.path().join("workspace/worktree"),
            workspace_paths: Vec::new(),
            config_path: root.path().join("devcontainer.json"),
            run_id: Some("run_123".to_string()),
            outer_runtime_id: Some("runtime_123".to_string()),
            materializations: assignment
                .mounts
                .iter()
                .map(|mount| StateMaterialization {
                    source_path: mount.source.clone(),
                    sha256: mount.sha256.clone(),
                    source_kind: mount.source_kind,
                })
                .collect(),
            tool_request_spools: Vec::new(),
            tool_request_ledger_path: None,
            proxy_names: Vec::new(),
            compose_projects: Vec::new(),
            container_id: None,
        };
        let provider = InGuestRuntimeProvider {
            devcontainer: PathBuf::from("devcontainer"),
            docker: PathBuf::from("docker"),
            timeout: PathBuf::from("timeout"),
        };
        let mut residue = Vec::new();
        provider.erase_materializations(&state, &mut residue);

        assert!(fs::symlink_metadata(shared).is_err());
        assert!(fs::symlink_metadata(endpoint_directory).is_err());
        assert!(fs::symlink_metadata(endpoint_socket).is_err());
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
