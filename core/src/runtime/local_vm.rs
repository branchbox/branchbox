//! Account-free Firecracker runtime provider.
//!
//! The provider keeps BranchBox's normal devcontainer contract and delegates privileged Linux
//! host setup to the release-bundled `branchbox-local-vm` driver. The driver is a narrow, local
//! process boundary: it directly owns Firecracker, jailer, TAP/NAT, workspace synchronization,
//! guest SSH, and port proxy lifecycle. It never talks to a hosted service or a host Docker daemon.

use super::{
    RuntimeContext, RuntimeExecResult, RuntimeMetadata, RuntimePort, RuntimeProvider,
    RuntimeProviderKind, RuntimeVersionMetadata,
};
use crate::{Error, Result};
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

pub(super) struct LocalVmRuntimeProvider {
    driver: PathBuf,
}

#[derive(Debug, Deserialize)]
struct PrepareOutput {
    runtime_id: String,
    #[serde(default)]
    published_ports: Vec<RuntimePort>,
    monitor: String,
    kernel_sha256: String,
    rootfs_sha256: String,
}

impl LocalVmRuntimeProvider {
    pub(super) fn new() -> Result<Self> {
        let driver = if let Some(path) = std::env::var_os("BRANCHBOX_LOCAL_VM_DRIVER_PATH") {
            PathBuf::from(path)
        } else if let Ok(current) = std::env::current_exe() {
            let adjacent = current
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("branchbox-local-vm");
            if adjacent.is_file() {
                adjacent
            } else {
                which::which("branchbox-local-vm").map_err(|_| Self::missing_driver_error())?
            }
        } else {
            which::which("branchbox-local-vm").map_err(|_| Self::missing_driver_error())?
        };
        Ok(Self { driver })
    }

    fn missing_driver_error() -> Error {
        Error::validation(
            "Runtime provider 'local-vm' requires the BranchBox Firecracker driver `branchbox-local-vm` beside the BranchBox binary or on PATH",
        )
    }

    fn run(&self, args: &[&str]) -> Result<Output> {
        Command::new(&self.driver)
            .args(args)
            .output()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute local-vm driver '{}': {err}",
                    self.driver.display()
                ))
            })
    }

    fn checked(&self, operation: &str, args: &[&str]) -> Result<Output> {
        let output = self.run(args)?;
        if output.status.success() {
            return Ok(output);
        }
        Err(Error::validation(format!(
            "local-vm {operation} failed: {}",
            bounded_detail(&output.stderr)
        )))
    }

    fn runtime_id(metadata: &RuntimeMetadata) -> Result<&str> {
        metadata.runtime_id.as_deref().ok_or_else(|| {
            Error::validation("local-vm runtime metadata is missing its Firecracker VM ID")
        })
    }
}

impl RuntimeProvider for LocalVmRuntimeProvider {
    fn kind(&self) -> RuntimeProviderKind {
        RuntimeProviderKind::LocalVm
    }

    fn validate(&self) -> Result<()> {
        self.checked("preflight", &["validate"]).map(|_| ())
    }

    fn exists(&self, metadata: &RuntimeMetadata) -> Result<bool> {
        let Some(runtime_id) = metadata.runtime_id.as_deref() else {
            return Ok(false);
        };
        Ok(self.run(&["exists", runtime_id])?.status.success())
    }

    fn environment_ready(&self, metadata: &RuntimeMetadata, worktree_path: &Path) -> Result<bool> {
        let runtime_id = Self::runtime_id(metadata)?;
        Ok(self
            .run(&["probe", runtime_id, &worktree_path.to_string_lossy()])?
            .status
            .success())
    }

    fn prepare(&self, context: &RuntimeContext<'_>) -> Result<RuntimeMetadata> {
        let ports = serde_json::to_string(context.published_ports)?;
        let output = self.checked(
            "prepare",
            &[
                "prepare",
                context.runtime_name,
                &context.workspace_mount_path.to_string_lossy(),
                &context.worktree_path.to_string_lossy(),
                &ports,
            ],
        )?;
        let prepared: PrepareOutput = serde_json::from_slice(&output.stdout).map_err(|err| {
            Error::validation(format!(
                "local-vm driver returned invalid prepare metadata: {err}"
            ))
        })?;
        Ok(RuntimeMetadata {
            provider: RuntimeProviderKind::LocalVm,
            runtime_id: Some(prepared.runtime_id),
            published_ports: prepared.published_ports,
            version: Some(RuntimeVersionMetadata {
                monitor: prepared.monitor,
                kernel_sha256: prepared.kernel_sha256,
                rootfs_sha256: prepared.rootfs_sha256,
            }),
        })
    }

    fn start_environment(
        &self,
        context: &RuntimeContext<'_>,
        metadata: &RuntimeMetadata,
    ) -> Result<()> {
        let runtime_id = Self::runtime_id(metadata)?;
        self.checked(
            "devcontainer startup",
            &[
                "start",
                runtime_id,
                &context.worktree_path.to_string_lossy(),
            ],
        )
        .map(|_| ())
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
        let runtime_id = Self::runtime_id(metadata)?;
        let mut args = vec![
            "exec".to_string(),
            runtime_id.to_string(),
            worktree_path.to_string_lossy().into_owned(),
            "--".to_string(),
        ];
        args.extend(command.iter().cloned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = self.run(&refs)?;
        Ok(RuntimeExecResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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
        let runtime_id = Self::runtime_id(metadata)?;
        let status = Command::new(&self.driver)
            .args(["exec-interactive", runtime_id])
            .arg(worktree_path)
            .arg("--")
            .args(command)
            .status()
            .map_err(|err| {
                Error::validation(format!(
                    "Failed to execute local-vm driver '{}': {err}",
                    self.driver.display()
                ))
            })?;
        Ok(status.code().unwrap_or(-1))
    }

    fn destroy(&self, metadata: &RuntimeMetadata) -> Result<()> {
        let Some(runtime_id) = metadata.runtime_id.as_deref() else {
            return Ok(());
        };
        self.checked("teardown", &["destroy", runtime_id])
            .map(|_| ())
    }
}

fn bounded_detail(bytes: &[u8]) -> String {
    const MAX_BYTES: usize = 2_048;
    const MAX_LINES: usize = 12;
    let rendered = String::from_utf8_lossy(bytes);
    let mut lines: Vec<&str> = rendered.lines().rev().take(MAX_LINES).collect();
    lines.reverse();
    let unredacted = lines.join("\n");
    let values = environment_values(&unredacted);
    let mut detail = redact_environment_assignments(&unredacted);
    for value in values {
        if !value.is_empty() {
            detail = detail.replace(&value, "[REDACTED]");
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

fn environment_values(detail: &str) -> Vec<String> {
    static ENV_ASSIGNMENT: OnceLock<Regex> = OnceLock::new();
    let pattern = ENV_ASSIGNMENT.get_or_init(|| {
        Regex::new(
            r#"[\"']?[A-Z_][A-Z0-9_]*[\"']?\s*[:=]\s*(?:\"([^\"]*)\"|'([^']*)'|([^\s,}\]]+))"#,
        )
        .expect("environment value regex must compile")
    });
    let mut values = Vec::new();
    for capture in pattern.captures_iter(detail) {
        if let Some(value) = capture
            .get(1)
            .or_else(|| capture.get(2))
            .or_else(|| capture.get(3))
        {
            if !value.as_str().is_empty() {
                values.push(value.as_str().to_string());
            }
        }
    }
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values.dedup();
    values
}

fn redact_environment_assignments(detail: &str) -> String {
    static ENV_ASSIGNMENT: OnceLock<Regex> = OnceLock::new();
    let pattern = ENV_ASSIGNMENT.get_or_init(|| {
        Regex::new(
            r#"(?P<prefix>[\"']?[A-Z_][A-Z0-9_]*[\"']?\s*[:=]\s*)(?:\"[^\"]*\"|'[^']*'|[^\s,}\]]+)"#,
        )
        .expect("environment redaction regex must compile")
    });
    pattern
        .replace_all(detail, "${prefix}[REDACTED]")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_details_are_bounded() {
        let detail = bounded_detail("secret=no\n".repeat(500).as_bytes());
        assert!(detail.len() <= 2_051);
        assert!(detail.lines().count() <= 12);
    }

    #[test]
    fn failure_details_redact_assigned_and_expanded_secrets() {
        let detail =
            bounded_detail(b"API_TOKEN=local-vm-secret\nError: local-vm-secret was rejected\n");
        assert!(!detail.contains("local-vm-secret"));
        assert!(detail.contains("API_TOKEN=[REDACTED]"));
    }
}
