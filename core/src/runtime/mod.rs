//! Workspace isolation runtime providers.
//!
//! Runtime providers define the outer execution boundary for a BranchBox
//! workspace. They intentionally sit above devcontainers: a devcontainer
//! describes the developer environment, while a runtime provider decides where
//! that environment and its Docker/Compose workloads execute.

use crate::{Error, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::str::FromStr;
use std::sync::OnceLock;

/// Runtime implementations selectable for a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeProviderKind {
    /// Existing BranchBox behavior: the worktree and devcontainer run through
    /// the host's normal container tooling.
    #[default]
    Container,
    /// Experimental Docker Sandboxes microVM backend.
    Sbx,
    /// Reserved account-free local microVM backend.
    LocalVm,
}

impl fmt::Display for RuntimeProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Container => "container",
            Self::Sbx => "sbx",
            Self::LocalVm => "local-vm",
        })
    }
}

impl FromStr for RuntimeProviderKind {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "container" | "current" => Ok(Self::Container),
            "sbx" => Ok(Self::Sbx),
            "local-vm" | "local_vm" => Ok(Self::LocalVm),
            other => Err(format!(
                "unknown runtime provider '{other}'; expected container, sbx, or local-vm"
            )),
        }
    }
}

/// Persisted identity for the execution boundary associated with a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMetadata {
    #[serde(default)]
    pub provider: RuntimeProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub published_ports: Vec<RuntimePort>,
}

impl Default for RuntimeMetadata {
    fn default() -> Self {
        Self {
            provider: RuntimeProviderKind::Container,
            runtime_id: None,
            published_ports: Vec::new(),
        }
    }
}

/// Host-to-runtime port mapping owned by the outer isolation boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePort {
    pub host: u16,
    pub runtime: u16,
}

impl fmt::Display for RuntimePort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.runtime)
    }
}

/// Captured output from a command executed through a runtime provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Inputs shared by runtime implementations during workspace provisioning.
#[derive(Debug)]
pub struct RuntimeContext<'a> {
    pub work_feature: &'a str,
    pub worktree_path: &'a Path,
    /// Stable project-scoped name chosen by BranchBox (typically the Compose
    /// project name) to avoid collisions across repositories.
    pub runtime_name: &'a str,
    /// Project-level directory exposed to the runtime. BranchBox devcontainer
    /// templates reference sibling worktrees and shared configuration here.
    pub workspace_mount_path: &'a Path,
    pub published_ports: &'a [RuntimePort],
}

/// Execution-boundary lifecycle contract.
pub trait RuntimeProvider {
    fn kind(&self) -> RuntimeProviderKind;

    /// Validate provider-specific prerequisites before BranchBox mutates the
    /// repository or creates a worktree.
    fn validate(&self) -> Result<()> {
        Ok(())
    }

    /// Create or reuse the execution boundary for a prepared worktree.
    fn prepare(&self, context: &RuntimeContext<'_>) -> Result<RuntimeMetadata>;

    /// Start the repository-defined developer environment inside the boundary.
    fn start_environment(
        &self,
        context: &RuntimeContext<'_>,
        metadata: &RuntimeMetadata,
    ) -> Result<()>;

    /// Execute an arbitrary command in the workspace boundary.
    fn exec(
        &self,
        metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<RuntimeExecResult>;

    /// Execute a command with inherited stdio for interactive tools such as
    /// coding agents.
    fn exec_interactive(
        &self,
        metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<i32>;

    /// Remove provider-owned state before the BranchBox worktree is removed.
    fn destroy(&self, metadata: &RuntimeMetadata) -> Result<()>;
}

/// Resolve a runtime provider without making optional provider dependencies
/// part of BranchBox's default startup path.
pub fn provider(kind: RuntimeProviderKind) -> Result<Box<dyn RuntimeProvider>> {
    match kind {
        RuntimeProviderKind::Container => Ok(Box::new(ContainerRuntimeProvider)),
        RuntimeProviderKind::Sbx => Ok(Box::new(SbxRuntimeProvider::new()?)),
        RuntimeProviderKind::LocalVm => Err(Error::validation(
            "Runtime provider 'local-vm' is reserved but not implemented yet; use 'container' or experimental 'sbx'",
        )),
    }
}

/// Compatibility provider for today's host container/devcontainer behavior.
struct ContainerRuntimeProvider;

impl RuntimeProvider for ContainerRuntimeProvider {
    fn kind(&self) -> RuntimeProviderKind {
        RuntimeProviderKind::Container
    }

    fn prepare(&self, _context: &RuntimeContext<'_>) -> Result<RuntimeMetadata> {
        Ok(RuntimeMetadata::default())
    }

    fn start_environment(
        &self,
        _context: &RuntimeContext<'_>,
        _metadata: &RuntimeMetadata,
    ) -> Result<()> {
        Ok(())
    }

    fn exec(
        &self,
        _metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<RuntimeExecResult> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| Error::validation("Runtime command cannot be empty"))?;
        let output = Command::new(program)
            .args(args)
            .current_dir(worktree_path)
            .output()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute runtime command '{program}': {err}"
                ))
            })?;
        Ok(exec_result(output))
    }

    fn exec_interactive(
        &self,
        _metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<i32> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| Error::validation("Runtime command cannot be empty"))?;
        let status = Command::new(program)
            .args(args)
            .current_dir(worktree_path)
            .status()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute runtime command '{program}': {err}"
                ))
            })?;
        Ok(status.code().unwrap_or(-1))
    }

    fn destroy(&self, _metadata: &RuntimeMetadata) -> Result<()> {
        Ok(())
    }
}

/// Experimental Docker Sandboxes provider.
struct SbxRuntimeProvider {
    binary: PathBuf,
}

impl SbxRuntimeProvider {
    const MAX_FAILURE_DETAIL_LINES: usize = 12;
    const MAX_FAILURE_DETAIL_BYTES: usize = 2_048;
    const DEVCONTAINER_EXEC: &'static str = "if command -v devcontainer >/dev/null 2>&1; then exec devcontainer exec --workspace-folder . \"$@\"; elif command -v npx >/dev/null 2>&1; then exec env -u NPM_CONFIG_PREFIX npx --yes @devcontainers/cli exec --workspace-folder . \"$@\"; else echo 'BranchBox SBX requires devcontainer or npx inside the sandbox shell image' >&2; exit 127; fi";
    const PORT_PROXY_BOOTSTRAP: &'static str = r#"set -eu
container_id="$1"
runtime_port="$2"
proxy_name="branchbox-port-proxy-${runtime_port}"
network_id=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.NetworkID}}{{end}}' "$container_id")
target_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$container_id")
docker rm -f "$proxy_name" >/dev/null 2>&1 || true
for candidate_id in $(docker ps --filter "network=${network_id}" -q); do
    exposed_ports=$(docker inspect -f '{{json .Config.ExposedPorts}}' "$candidate_id")
    case "$exposed_ports" in
        *\"${runtime_port}/tcp\"*)
            target_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$candidate_id")
            break
            ;;
    esac
done
exec docker run -d --name "$proxy_name" --restart unless-stopped --network "$network_id" -p "${runtime_port}:${runtime_port}" alpine/socat -dd "TCP-LISTEN:${runtime_port},fork,reuseaddr" "TCP:${target_ip}:${runtime_port}""#;

    fn new() -> Result<Self> {
        let binary = match std::env::var_os("BRANCHBOX_SBX_PATH") {
            Some(path) => PathBuf::from(path),
            None => which::which("sbx").map_err(|_| {
                Error::validation(
                    "Runtime provider 'sbx' requires the Docker Sandboxes CLI; install 'sbx' or select --runtime container",
                )
            })?,
        };
        Ok(Self { binary })
    }

    fn run(&self, args: &[&str]) -> Result<Output> {
        Command::new(&self.binary)
            .args(args)
            .output()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute Docker Sandboxes CLI '{}': {err}",
                    self.binary.display()
                ))
            })
    }

    fn ensure_available(&self) -> Result<Vec<String>> {
        let output = self.run(&["ls", "--quiet"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.trim();
            let guidance = if detail.to_ascii_lowercase().contains("auth")
                || detail.to_ascii_lowercase().contains("sign in")
            {
                "Docker Sandboxes authentication is unavailable; run 'sbx login', or select --runtime container (normal BranchBox use does not require SBX authentication)"
            } else {
                "Docker Sandboxes is unavailable for this workspace"
            };
            return Err(Error::validation(if detail.is_empty() {
                guidance.to_string()
            } else {
                format!("{guidance}: {detail}")
            }));
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    fn sandbox_name(runtime_name: &str) -> String {
        let normalized: String = runtime_name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '-') {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let normalized = normalized.trim_matches('-');
        format!("branchbox-{normalized}")
    }

    fn runtime_id<'a>(&self, metadata: &'a RuntimeMetadata) -> Result<&'a str> {
        metadata.runtime_id.as_deref().ok_or_else(|| {
            Error::validation("Docker Sandboxes runtime metadata is missing its sandbox ID")
        })
    }

    fn publish_port(&self, sandbox_name: &str, port: RuntimePort) -> Result<()> {
        let mapping = port.to_string();
        let output = self.run(&["ports", sandbox_name, "--publish", &mapping])?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Docker Sandboxes could not publish port {mapping} for '{sandbox_name}': {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    fn published_ports(&self, sandbox_name: &str) -> Result<Vec<RuntimePort>> {
        let output = self.run(&["ports", sandbox_name, "--json"])?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Docker Sandboxes could not inspect published ports for '{sandbox_name}': {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let entries: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).map_err(|err| {
                Error::validation(format!(
                    "Docker Sandboxes returned invalid port metadata for '{sandbox_name}': {err}"
                ))
            })?;
        let mut ports = Vec::new();
        for entry in entries {
            let Some(host) = entry.get("host_port").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let Some(runtime) = entry
                .get("sandbox_port")
                .and_then(serde_json::Value::as_u64)
            else {
                continue;
            };
            let Ok(host) = u16::try_from(host) else {
                continue;
            };
            let Ok(runtime) = u16::try_from(runtime) else {
                continue;
            };
            let mapping = RuntimePort { host, runtime };
            if !ports.contains(&mapping) {
                ports.push(mapping);
            }
        }
        Ok(ports)
    }

    fn wake_sandbox(&self, sandbox_name: &str) -> Result<()> {
        let output = self.run(&["exec", sandbox_name, "true"])?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Docker Sandboxes could not start existing runtime '{sandbox_name}': {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    fn resolve_published_ports(ports: &[RuntimePort]) -> Result<Vec<RuntimePort>> {
        ports
            .iter()
            .map(|port| {
                if TcpListener::bind(("127.0.0.1", port.host)).is_ok() {
                    return Ok(*port);
                }

                let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|err| {
                    Error::validation(format!(
                        "Could not allocate a host port for sandbox port {}: {err}",
                        port.runtime
                    ))
                })?;
                let host = listener.local_addr().map_err(|err| {
                    Error::validation(format!(
                        "Could not inspect the allocated host port for sandbox port {}: {err}",
                        port.runtime
                    ))
                })?;
                Ok(RuntimePort {
                    host: host.port(),
                    runtime: port.runtime,
                })
            })
            .collect()
    }

    fn devcontainer_id(output: &Output) -> Result<String> {
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
                Error::validation(
                    "Docker Sandboxes devcontainer startup did not return a container ID",
                )
            })
    }

    fn devcontainer_failure_detail(output: &Output) -> String {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let environment_values = Self::expanded_environment_values(&stderr);
        let mut actionable: Vec<&str> = stderr
            .lines()
            .filter(|line| Self::is_actionable_failure_line(line))
            .rev()
            .take(Self::MAX_FAILURE_DETAIL_LINES)
            .collect();
        actionable.reverse();

        let mut detail = Self::redact_environment_assignments(&actionable.join("\n"));
        for value in environment_values {
            if !value.is_empty() {
                detail = detail.replace(&value, "[REDACTED]");
            }
        }
        if detail.len() > Self::MAX_FAILURE_DETAIL_BYTES {
            let mut start = detail.len() - Self::MAX_FAILURE_DETAIL_BYTES;
            while !detail.is_char_boundary(start) {
                start += 1;
            }
            detail = format!("…{}", &detail[start..]);
        }

        let status = output
            .status
            .code()
            .map_or_else(|| "signal".to_string(), |code| code.to_string());
        if detail.trim().is_empty() {
            format!(
                "devcontainer command failed with exit status {status}; inspect sandbox-local devcontainer logs for details"
            )
        } else {
            format!("devcontainer command failed with exit status {status}: {detail}")
        }
    }

    fn is_actionable_failure_line(line: &str) -> bool {
        let trimmed = line.trim_start();
        let normalized = trimmed.to_ascii_lowercase();
        let has_failure_marker = [
            "error",
            "failed",
            "failure",
            "fatal",
            "denied",
            "invalid",
            "not found",
            "cannot",
            "could not",
            "exit code",
            "exited with",
        ]
        .iter()
        .any(|marker| normalized.contains(marker));
        let explicitly_actionable = [
            "error",
            "failed",
            "failure",
            "fatal",
            "docker:",
            "devcontainer",
            "unable",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix));

        has_failure_marker
            && (trimmed.starts_with('[') || (line.len() == trimmed.len() && explicitly_actionable))
    }

    fn expanded_environment_values(stderr: &str) -> Vec<String> {
        let mut values = Vec::new();
        let mut environment_indent = None;
        for line in stderr.lines() {
            let indentation = line.len() - line.trim_start().len();
            let trimmed = line.trim_start();
            if environment_indent.is_some_and(|indent| !trimmed.is_empty() && indentation <= indent)
            {
                environment_indent = None;
            }
            if trimmed == "environment:" || trimmed.starts_with("\"environment\": {") {
                environment_indent = Some(indentation);
                continue;
            }
            let within_environment = environment_indent
                .is_some_and(|indent| !trimmed.is_empty() && indentation > indent);

            for (index, delimiter) in line.match_indices([':', '=']) {
                let before = &line[..index];
                let key = before
                    .trim_end()
                    .rsplit(|character: char| {
                        character.is_ascii_whitespace() || matches!(character, ',' | '{' | '[')
                    })
                    .next()
                    .unwrap_or_default()
                    .trim_matches(['\'', '"']);
                if key.is_empty()
                    || !key
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
                    || !key.chars().next().is_some_and(|character| {
                        character.is_ascii_alphabetic() || character == '_'
                    })
                    || (delimiter != "=" && !within_environment)
                {
                    continue;
                }

                let remainder = line[index + delimiter.len()..].trim_start();
                let value = if let Some(quoted) = remainder.strip_prefix('"') {
                    quoted.split('"').next().unwrap_or_default()
                } else if let Some(quoted) = remainder.strip_prefix('\'') {
                    quoted.split('\'').next().unwrap_or_default()
                } else if within_environment && delimiter == ":" {
                    remainder.trim_end_matches([',', '}', ']'])
                } else {
                    remainder
                        .split(|character: char| {
                            character.is_ascii_whitespace() || matches!(character, ',' | '}' | ']')
                        })
                        .next()
                        .unwrap_or_default()
                };
                if !value.is_empty() && !values.iter().any(|existing| existing == value) {
                    values.push(value.to_string());
                }
            }
        }
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        values
    }

    fn redact_environment_assignments(detail: &str) -> String {
        static ENV_ASSIGNMENT: OnceLock<Regex> = OnceLock::new();
        let pattern = ENV_ASSIGNMENT.get_or_init(|| {
            Regex::new(
                r#"(?P<prefix>["']?[A-Z_][A-Z0-9_]*["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,}\]]+)"#,
            )
            .expect("environment redaction regex must compile")
        });
        pattern
            .replace_all(detail, "${prefix}[REDACTED]")
            .into_owned()
    }

    fn start_port_proxy(
        &self,
        runtime_id: &str,
        container_id: &str,
        port: RuntimePort,
    ) -> Result<()> {
        let runtime_port = port.runtime.to_string();
        let output = self.run(&[
            "exec",
            runtime_id,
            "bash",
            "-lc",
            Self::PORT_PROXY_BOOTSTRAP,
            "branchbox-port-proxy",
            container_id,
            &runtime_port,
        ])?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Docker Sandboxes could not bridge runtime port {} in '{runtime_id}': {}",
                port.runtime,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }
}

fn exec_result(output: Output) -> RuntimeExecResult {
    RuntimeExecResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

impl RuntimeProvider for SbxRuntimeProvider {
    fn kind(&self) -> RuntimeProviderKind {
        RuntimeProviderKind::Sbx
    }

    fn validate(&self) -> Result<()> {
        self.ensure_available().map(|_| ())
    }

    fn prepare(&self, context: &RuntimeContext<'_>) -> Result<RuntimeMetadata> {
        let sandbox_name = Self::sandbox_name(context.runtime_name);
        let existing = self.ensure_available()?;
        let created = !existing.iter().any(|name| name == &sandbox_name);
        if !created {
            self.wake_sandbox(&sandbox_name)?;
        }
        let mut published_ports = if created {
            Self::resolve_published_ports(context.published_ports)?
        } else {
            self.published_ports(&sandbox_name)?
        };
        if created {
            let workspace = context.workspace_mount_path.to_string_lossy();
            let output = self.run(&[
                "create",
                "shell",
                workspace.as_ref(),
                "--name",
                &sandbox_name,
                "--quiet",
            ])?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(Error::validation(format!(
                    "Docker Sandboxes could not provision runtime '{sandbox_name}': {}",
                    stderr.trim()
                )));
            }
            for port in &published_ports {
                if let Err(err) = self.publish_port(&sandbox_name, *port) {
                    let _ = self.run(&["rm", "--force", &sandbox_name]);
                    return Err(err);
                }
            }
        } else {
            for requested in context.published_ports {
                if published_ports
                    .iter()
                    .any(|published| published.runtime == requested.runtime)
                {
                    continue;
                }
                let resolved = Self::resolve_published_ports(&[*requested])?[0];
                self.publish_port(&sandbox_name, resolved)?;
                published_ports.push(resolved);
            }
        }

        Ok(RuntimeMetadata {
            provider: self.kind(),
            runtime_id: Some(sandbox_name),
            published_ports,
        })
    }

    fn start_environment(
        &self,
        context: &RuntimeContext<'_>,
        metadata: &RuntimeMetadata,
    ) -> Result<()> {
        let runtime_id = self.runtime_id(metadata)?;
        let worktree = context.worktree_path.to_string_lossy();
        let bootstrap = "if command -v devcontainer >/dev/null 2>&1; then exec devcontainer up --workspace-folder .; elif command -v npx >/dev/null 2>&1; then exec env -u NPM_CONFIG_PREFIX npx --yes @devcontainers/cli up --workspace-folder .; else echo 'BranchBox SBX requires devcontainer or npx inside the sandbox shell image' >&2; exit 127; fi";
        let output = self.run(&[
            "exec",
            "--workdir",
            worktree.as_ref(),
            runtime_id,
            "bash",
            "-lc",
            bootstrap,
        ])?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Docker Sandboxes could not start the devcontainer in '{runtime_id}': {}",
                Self::devcontainer_failure_detail(&output)
            )));
        }
        let container_id = Self::devcontainer_id(&output)?;
        for port in &metadata.published_ports {
            self.start_port_proxy(runtime_id, &container_id, *port)?;
        }
        Ok(())
    }

    fn exec(
        &self,
        metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<RuntimeExecResult> {
        let runtime_id = self.runtime_id(metadata)?;
        let worktree = worktree_path.to_string_lossy();
        if command.is_empty() {
            return Err(Error::validation("Runtime command cannot be empty"));
        }
        let mut args = vec![
            "exec".to_string(),
            "--workdir".to_string(),
            worktree.into_owned(),
            runtime_id.to_string(),
            "bash".to_string(),
            "-lc".to_string(),
            Self::DEVCONTAINER_EXEC.to_string(),
            "branchbox-devcontainer-exec".to_string(),
        ];
        args.extend(command.iter().cloned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&refs).map(exec_result)
    }

    fn exec_interactive(
        &self,
        metadata: &RuntimeMetadata,
        worktree_path: &Path,
        command: &[String],
    ) -> Result<i32> {
        let runtime_id = self.runtime_id(metadata)?;
        if command.is_empty() {
            return Err(Error::validation("Runtime command cannot be empty"));
        }
        let worktree = worktree_path.to_string_lossy();
        let status = Command::new(&self.binary)
            .args(["exec", "--workdir", worktree.as_ref(), runtime_id])
            .args([
                "bash",
                "-lc",
                Self::DEVCONTAINER_EXEC,
                "branchbox-devcontainer-exec",
            ])
            .args(command)
            .status()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute Docker Sandboxes CLI '{}': {err}",
                    self.binary.display()
                ))
            })?;
        Ok(status.code().unwrap_or(-1))
    }

    fn destroy(&self, metadata: &RuntimeMetadata) -> Result<()> {
        let Some(runtime_id) = metadata.runtime_id.as_deref() else {
            return Ok(());
        };
        let output = self.run(&["rm", "--force", runtime_id])?;
        if !output.status.success() {
            return Err(Error::validation(format!(
                "Docker Sandboxes could not remove runtime '{runtime_id}': {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    fn fake_sbx(script: &str) -> (tempfile::TempDir, SbxRuntimeProvider) {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("sbx");
        std::fs::write(&binary, script).unwrap();
        let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&binary, permissions).unwrap();
        let provider = SbxRuntimeProvider {
            binary: binary.clone(),
        };
        (temp, provider)
    }

    #[test]
    fn runtime_provider_kind_accepts_supported_aliases() {
        assert_eq!(
            "container".parse::<RuntimeProviderKind>().unwrap(),
            RuntimeProviderKind::Container
        );
        assert_eq!(
            "current".parse::<RuntimeProviderKind>().unwrap(),
            RuntimeProviderKind::Container
        );
        assert_eq!(
            "local_vm".parse::<RuntimeProviderKind>().unwrap(),
            RuntimeProviderKind::LocalVm
        );
    }

    #[test]
    fn sbx_sandbox_names_are_provider_safe() {
        assert_eq!(
            SbxRuntimeProvider::sandbox_name("Storefront_OAuth Flow"),
            "branchbox-storefront-oauth-flow"
        );
    }

    #[test]
    fn legacy_runtime_metadata_defaults_to_container() {
        let metadata: RuntimeMetadata = serde_json::from_str("{}").unwrap();
        assert_eq!(metadata, RuntimeMetadata::default());
    }

    #[cfg(unix)]
    #[test]
    fn sbx_authentication_failure_is_provider_scoped() {
        let (_temp, provider) =
            fake_sbx("#!/bin/sh\necho 'ERROR: Not authenticated to Docker' >&2\nexit 1\n");

        let error = provider.validate().unwrap_err().to_string();
        assert!(error.contains("sbx login"));
        assert!(error.contains("normal BranchBox use does not require SBX authentication"));
    }

    #[cfg(unix)]
    #[test]
    fn sbx_prepare_reuses_named_sandbox() {
        let (_temp, provider) = fake_sbx(
            "#!/bin/sh\ncase \"$1\" in\n  ls) printf '%s\\n' 'branchbox-storefront-oauth-flow' ;;\n  ports) printf '%s\\n' '[{\"host_ip\":\"127.0.0.1\",\"host_port\":49123,\"sandbox_port\":3000,\"protocol\":\"tcp\"}]' ;;\nesac\n",
        );
        let workspace = tempfile::tempdir().unwrap();
        let ports = [RuntimePort {
            host: 3000,
            runtime: 3000,
        }];

        let metadata = provider
            .prepare(&RuntimeContext {
                work_feature: "oauth-flow",
                worktree_path: workspace.path(),
                runtime_name: "storefront-oauth-flow",
                workspace_mount_path: workspace.path(),
                published_ports: &ports,
            })
            .unwrap();

        assert_eq!(metadata.provider, RuntimeProviderKind::Sbx);
        assert_eq!(
            metadata.runtime_id.as_deref(),
            Some("branchbox-storefront-oauth-flow")
        );
        assert_eq!(
            metadata.published_ports,
            [RuntimePort {
                host: 49123,
                runtime: 3000,
            }]
        );
    }

    #[test]
    fn sbx_selects_an_available_host_port_when_preferred_port_is_busy() {
        let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();
        let mappings = SbxRuntimeProvider::resolve_published_ports(&[RuntimePort {
            host: occupied_port,
            runtime: 3000,
        }])
        .unwrap();

        assert_ne!(mappings[0].host, occupied_port);
        assert_eq!(mappings[0].runtime, 3000);
    }

    #[cfg(unix)]
    #[test]
    fn sbx_devcontainer_startup_errors_do_not_expose_expanded_environment_values() {
        const SENTINEL: &str = "branchbox-sentinel-secret-7f9c";
        let script = format!(
            r#"#!/bin/sh
cat >&2 <<'EOF'
services:
  app:
    environment:
      arbitraryCredential: {SENTINEL}
      TUNNEL_TOKEN: another-sensitive-value
      ERROR_REPORTING_DSN: third-sensitive-value
[2026-08-20T00:00:00Z] Error: docker compose up failed because {SENTINEL} was rejected; TUNNEL_TOKEN=another-sensitive-value
EOF
exit 42
"#
        );
        let (_temp, provider) = fake_sbx(&script);
        let workspace = tempfile::tempdir().unwrap();
        let metadata = RuntimeMetadata {
            provider: RuntimeProviderKind::Sbx,
            runtime_id: Some("branchbox-redaction-test".to_string()),
            published_ports: Vec::new(),
        };

        let error = provider
            .start_environment(
                &RuntimeContext {
                    work_feature: "redaction-test",
                    worktree_path: workspace.path(),
                    runtime_name: "redaction-test",
                    workspace_mount_path: workspace.path(),
                    published_ports: &[],
                },
                &metadata,
            )
            .unwrap_err()
            .to_string();

        assert!(!error.contains(SENTINEL), "secret leaked in error: {error}");
        assert!(!error.contains("another-sensitive-value"));
        assert!(!error.contains("third-sensitive-value"));
        assert!(!error.contains("services:"));
        assert!(!error.contains("arbitraryCredential:"));
        assert!(!error.contains("ERROR_REPORTING_DSN:"));
        assert!(error.contains("exit status 42"));
        assert!(error.contains("docker compose up failed"));
    }
}
