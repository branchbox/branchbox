#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use worktree_core::workflows::feature::{
    AdapterSummary, FeatureMetadata, FeatureStatus, FeatureTunnelState, ModuleOutcome,
    ModuleOutcomeRecord, ModuleSkipRecord, ModuleStatus, ModuleTeardownReport, StartMode,
    StartSummary, TeardownSummary,
};

#[cfg(unix)]
pub use platform::AgentClient;
#[cfg(unix)]
mod platform {
    use super::*;
    use std::env;
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;

    pub struct AgentClient {
        socket_path: PathBuf,
    }

    impl AgentClient {
        pub fn connect() -> Result<Self> {
            let socket_path = detect_socket_path()?;
            Ok(Self { socket_path })
        }

        pub fn list_features(
            &self,
            repo_path: Option<&Path>,
            include_removed: bool,
        ) -> Result<Vec<FeatureRecord>> {
            let request = json!({
                "action": "list_features",
                "repo_path": repo_path.map(|path| path.display().to_string()),
                "include_removed": include_removed,
            });
            let data = self.send(request)?;
            let features = data
                .get("features")
                .ok_or_else(|| anyhow!("agent response missing 'features' field"))?;
            Ok(serde_json::from_value(features.clone())?)
        }

        pub fn start_feature(&self, request: StartFeatureRequest) -> Result<StartFeatureSummary> {
            let payload = json!({
                "action": "start_feature",
                "repo_path": request.repo_path.map(|path| path.display().to_string()),
                "name": request.name,
                "title": request.title,
                "base_branch": request.base_branch,
                "branch_prefix": request.branch_prefix,
                "reuse": request.reuse,
                "telemetry": request.telemetry,
                "skip_modules": request.skip_modules,
                "minimal": request.mode == StartMode::Minimal,
                "prompt": request.prompt_seed,
            });
            let data = self.send(payload)?;
            let summary = data
                .get("start")
                .ok_or_else(|| anyhow!("agent response missing 'start' field"))?;
            Ok(serde_json::from_value(summary.clone())?)
        }

        pub fn teardown_feature(
            &self,
            request: TeardownFeatureRequest,
        ) -> Result<TeardownFeatureSummary> {
            let payload = json!({
                "action": "teardown_feature",
                "repo_path": request.repo_path.map(|path| path.display().to_string()),
                "name": request.name,
                "branch_prefix": request.branch_prefix,
                "delete_branch": request.delete_branch,
                "force": request.force,
                "complete_spec": request.complete_spec,
                "telemetry": request.telemetry,
            });
            let data = self.send(payload)?;
            let summary = data
                .get("teardown")
                .ok_or_else(|| anyhow!("agent response missing 'teardown' field"))?;
            Ok(serde_json::from_value(summary.clone())?)
        }

        fn send(&self, request: serde_json::Value) -> Result<serde_json::Value> {
            let mut stream = UnixStream::connect(&self.socket_path).with_context(|| {
                format!(
                    "failed to connect to BranchBox agent at {}",
                    self.socket_path.display()
                )
            })?;

            let payload = serde_json::to_vec(&request)?;
            stream
                .write_all(&payload)
                .with_context(|| "failed to send request to agent")?;
            stream
                .shutdown(Shutdown::Write)
                .with_context(|| "failed to finalize agent request")?;

            let mut response = Vec::new();
            stream
                .read_to_end(&mut response)
                .with_context(|| "failed to read agent response")?;

            if response.is_empty() {
                return Err(anyhow!("agent returned empty response"));
            }

            let reply: AgentReply = serde_json::from_slice(&response)
                .with_context(|| "failed to parse agent response")?;

            match reply.status {
                ResponseStatus::Success => reply
                    .data
                    .ok_or_else(|| anyhow!("agent response missing data payload")),
                ResponseStatus::Error => Err(anyhow!(
                    "{}",
                    reply
                        .error
                        .unwrap_or_else(|| "agent returned an unknown error".to_string())
                )),
            }
        }

        pub fn agent_status(&self) -> Result<AgentStatus> {
            let payload = json!({
                "action": "agent_status",
            });
            let data = self.send(payload)?;
            let status = data
                .get("status")
                .ok_or_else(|| anyhow!("agent response missing 'status' field"))?;
            Ok(serde_json::from_value(status.clone())?)
        }
    }

    fn detect_socket_path() -> Result<PathBuf> {
        if let Ok(path) = env::var("BRANCHBOX_AGENT_SOCKET") {
            return Ok(PathBuf::from(path));
        }

        if let Some(dir) = env::var_os("BRANCHBOX_AGENT_DIR") {
            return Ok(PathBuf::from(dir).join("branchbox-agent.sock"));
        }

        let base = dirs::home_dir().map(|home| home.join(".branchbox").join("agent"));

        let Some(dir) = base else {
            return Err(anyhow!(
                "Unable to locate agent socket. Set BRANCHBOX_AGENT_SOCKET."
            ));
        };

        Ok(dir.join("branchbox-agent.sock"))
    }
}

#[cfg(not(unix))]
pub use stub::AgentClient;
#[cfg(not(unix))]
mod stub {
    use super::*;

    pub struct AgentClient;

    impl AgentClient {
        pub fn connect() -> Result<Self> {
            Err(anyhow!(
                "BranchBox agent IPC currently supports Unix-like hosts only. Set BRANCHBOX_CLI_DIRECT=1 to run commands directly."
            ))
        }

        pub fn list_features(
            &self,
            _repo_path: Option<&Path>,
            _include_removed: bool,
        ) -> Result<Vec<FeatureRecord>> {
            Err(anyhow!(
                "BranchBox agent feature list unavailable on this platform. Use CLI direct mode instead."
            ))
        }

        pub fn start_feature(&self, _request: StartFeatureRequest) -> Result<StartFeatureSummary> {
            Err(anyhow!(
                "BranchBox agent start is unavailable on this platform. Use CLI direct mode instead."
            ))
        }

        pub fn teardown_feature(
            &self,
            _request: TeardownFeatureRequest,
        ) -> Result<TeardownFeatureSummary> {
            Err(anyhow!(
                "BranchBox agent teardown is unavailable on this platform. Use CLI direct mode instead."
            ))
        }

        pub fn agent_status(&self) -> Result<AgentStatus> {
            Err(anyhow!(
                "BranchBox agent status is unavailable on this platform. Use CLI direct mode instead."
            ))
        }
    }
}

pub struct StartFeatureRequest {
    pub repo_path: Option<PathBuf>,
    pub name: Option<String>,
    pub title: Option<String>,
    pub base_branch: Option<String>,
    pub branch_prefix: Option<String>,
    pub reuse: bool,
    pub telemetry: bool,
    pub skip_modules: Vec<String>,
    pub mode: StartMode,
    pub prompt_seed: Option<String>,
}

pub struct TeardownFeatureRequest {
    pub repo_path: Option<PathBuf>,
    pub name: String,
    pub branch_prefix: Option<String>,
    pub delete_branch: bool,
    pub force: bool,
    pub complete_spec: bool,
    pub telemetry: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FeatureRecord {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub status: FeatureStatus,
    #[serde(default)]
    pub feature_url: Option<String>,
    #[serde(default)]
    pub env_path: Option<String>,
    #[serde(default)]
    pub compose_project_name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub tunnel_provider: Option<String>,
    #[serde(default)]
    pub tunnel_status: Option<String>,
    #[serde(default)]
    pub tunnel_hostname: Option<String>,
    #[serde(default)]
    pub devcontainer_outdated: bool,
    #[serde(default)]
    pub last_sync_at: Option<String>,
    #[serde(default)]
    pub sync_strategy: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub removed_at: Option<String>,
    #[serde(default)]
    pub prompt_seed: Option<String>,
    pub start_mode: StartMode,
    #[serde(default)]
    pub module_outcomes: Vec<ModuleOutcomeRecord>,
    #[serde(default)]
    pub pr_number: Option<u32>,
    #[serde(default)]
    pub adapter: Option<AdapterPayload>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StartFeatureSummary {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_path: String,
    #[serde(default)]
    pub feature_url: Option<String>,
    #[serde(default)]
    pub compose_project_name: Option<String>,
    #[serde(default)]
    pub env_path: Option<String>,
    pub mode: StartMode,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub prompt_seed: Option<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub module_outcomes: Vec<ModuleOutcomePayload>,
    #[serde(default)]
    pub skipped_modules: Vec<SkippedModulePayload>,
    #[serde(default)]
    pub adapter: Option<AdapterPayload>,
    #[serde(default)]
    pub tunnel: Option<TunnelPayload>,
    pub generated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TeardownFeatureSummary {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub adapter_warnings: Vec<String>,
    #[serde(default)]
    pub module_reports: Vec<ModuleReportPayload>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentStatus {
    pub control_plane_configured: bool,
    pub control_plane_connected: bool,
    #[serde(default)]
    pub last_delivery_at: Option<String>,
    #[serde(default)]
    pub last_failure_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_ack_event_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModuleOutcomePayload {
    pub module: String,
    pub status: ModuleStatus,
    pub duration_ms: u64,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub forced: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SkippedModulePayload {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdapterPayload {
    pub name: String,
    pub service_url: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TunnelPayload {
    pub provider: String,
    #[serde(default)]
    pub hostname: Option<String>,
    pub status: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModuleReportPayload {
    pub name: String,
    #[serde(default)]
    pub teardown_ok: bool,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AgentReply {
    status: ResponseStatus,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ResponseStatus {
    Success,
    Error,
}

impl From<FeatureMetadata> for FeatureRecord {
    fn from(meta: FeatureMetadata) -> Self {
        let tunnel_provider = meta.tunnel.as_ref().map(|state| state.provider.clone());
        let tunnel_status = meta.tunnel.as_ref().map(|state| state.status.to_string());
        let tunnel_hostname = meta
            .tunnel
            .as_ref()
            .and_then(|state| state.hostname.clone());

        Self {
            work_feature: meta.work_feature,
            branch_name: meta.branch_name,
            worktree_path: meta.worktree_path.display().to_string(),
            status: meta.status,
            feature_url: meta.feature_url,
            env_path: meta.env_path.map(|p| p.display().to_string()),
            compose_project_name: meta.compose_project_name,
            color: meta.color,
            tunnel_provider,
            tunnel_status,
            tunnel_hostname,
            devcontainer_outdated: meta.devcontainer_outdated,
            last_sync_at: meta.last_sync_at.map(|ts| ts.to_rfc3339()),
            sync_strategy: meta.sync_strategy,
            created_at: Some(meta.created_at.to_rfc3339()),
            updated_at: Some(meta.updated_at.to_rfc3339()),
            removed_at: meta.removed_at.map(|ts| ts.to_rfc3339()),
            prompt_seed: meta.prompt_seed,
            start_mode: meta.start_mode,
            module_outcomes: meta.module_outcomes,
            pr_number: meta.pr_number,
            adapter: meta.adapter.map(AdapterPayload::from),
        }
    }
}

impl From<StartSummary> for StartFeatureSummary {
    fn from(summary: StartSummary) -> Self {
        Self {
            work_feature: summary.work_feature,
            branch_name: summary.branch_name,
            worktree_path: summary.worktree_path.display().to_string(),
            feature_url: summary.feature_url,
            compose_project_name: summary.compose_project_name,
            env_path: summary.env_path.map(|p| p.display().to_string()),
            mode: summary.mode,
            color: summary.color,
            prompt_seed: summary.prompt_seed,
            warnings: summary.warnings,
            module_outcomes: summary
                .module_outcomes
                .into_iter()
                .map(ModuleOutcomePayload::from)
                .collect(),
            skipped_modules: summary
                .skipped_modules
                .into_iter()
                .map(SkippedModulePayload::from)
                .collect(),
            adapter: summary.adapter.map(AdapterPayload::from),
            tunnel: summary.tunnel.map(TunnelPayload::from),
            generated_at: summary.generated_at.to_rfc3339(),
        }
    }
}

impl From<TeardownSummary> for TeardownFeatureSummary {
    fn from(summary: TeardownSummary) -> Self {
        Self {
            work_feature: summary.work_feature,
            branch_name: summary.branch_name,
            worktree_removed: summary.worktree_removed,
            branch_deleted: summary.branch_deleted,
            warnings: summary.warnings,
            adapter_warnings: summary.adapter_cleanup_warnings,
            module_reports: summary
                .module_reports
                .into_iter()
                .map(ModuleReportPayload::from)
                .collect(),
        }
    }
}

impl From<ModuleOutcome> for ModuleOutcomePayload {
    fn from(outcome: ModuleOutcome) -> Self {
        Self {
            module: outcome.module,
            status: outcome.status,
            duration_ms: outcome.duration_ms,
            notes: outcome.notes,
            forced: outcome.forced,
        }
    }
}

impl From<ModuleSkipRecord> for SkippedModulePayload {
    fn from(record: ModuleSkipRecord) -> Self {
        Self {
            name: record.name,
            reason: record.reason.description().to_string(),
        }
    }
}

impl From<AdapterSummary> for AdapterPayload {
    fn from(summary: AdapterSummary) -> Self {
        Self {
            name: summary.name,
            service_url: summary.service_url,
            warnings: summary.warnings,
        }
    }
}

impl From<FeatureTunnelState> for TunnelPayload {
    fn from(state: FeatureTunnelState) -> Self {
        Self {
            provider: state.provider,
            hostname: state.hostname,
            status: state.status.to_string(),
        }
    }
}

impl From<ModuleTeardownReport> for ModuleReportPayload {
    fn from(report: ModuleTeardownReport) -> Self {
        Self {
            name: report.name,
            teardown_ok: report.teardown_ok,
            errors: report.errors,
        }
    }
}
