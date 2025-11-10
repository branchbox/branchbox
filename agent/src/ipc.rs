use crate::{config::AgentConfig, ops, state::AgentState};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use worktree_core::workflows::feature::{
    AdapterSummary, FeatureMetadata, FeatureTunnelState, ModuleOutcome, ModuleOutcomeRecord,
    ModuleStatus, ModuleTeardownReport, StartMode, StartSummary, TeardownSummary,
};

pub struct IpcServer {
    config: Arc<AgentConfig>,
    state: AgentState,
}

impl IpcServer {
    pub fn new(config: Arc<AgentConfig>, state: AgentState) -> Self {
        Self { config, state }
    }

    pub async fn serve(&self, shutdown: CancellationToken) -> Result<()> {
        if self.config.socket_path.exists() {
            fs::remove_file(&self.config.socket_path).with_context(|| {
                format!(
                    "Failed to remove existing socket {}",
                    self.config.socket_path.display()
                )
            })?;
        }

        let listener = UnixListener::bind(&self.config.socket_path).with_context(|| {
            format!(
                "Failed to bind Unix socket {}",
                self.config.socket_path.display()
            )
        })?;

        info!(
            "IPC server listening on {}",
            self.config.socket_path.display()
        );

        loop {
            tokio::select! {
                accept_result = listener.accept() => match accept_result {
                    Ok((stream, _)) => {
                        let config = Arc::clone(&self.config);
                        let state = self.state.clone();
                        tokio::spawn(async move {
                            if let Err(err) = handle_connection(stream, config, state).await {
                                error!("IPC connection failed: {err:?}");
                            }
                        });
                    }
                    Err(err) => {
                        warn!("Failed to accept IPC connection: {err}");
                    }
                },
                _ = shutdown.cancelled() => {
                    info!("IPC server shutting down");
                    break;
                }
            }
        }

        Ok(())
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    config: Arc<AgentConfig>,
    state: AgentState,
) -> Result<()> {
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .context("Failed to read request payload")?;

    if buf.is_empty() {
        debug!("Received empty IPC payload");
        let response = AgentResponse::error("Empty request payload");
        write_response(&mut stream, response).await?;
        return Ok(());
    }

    let request: AgentRequest = match serde_json::from_slice(&buf) {
        Ok(req) => req,
        Err(err) => {
            let response = AgentResponse::error(format!("Invalid request JSON: {}", err));
            write_response(&mut stream, response).await?;
            return Ok(());
        }
    };

    let response = match dispatch(request, &config, &state).await {
        Ok(res) => res,
        Err(err) => AgentResponse::error(err.to_string()),
    };

    write_response(&mut stream, response).await?;
    Ok(())
}

async fn write_response(stream: &mut UnixStream, response: AgentResponse) -> Result<()> {
    let payload = serde_json::to_vec(&response).context("Failed to serialize response")?;
    stream
        .write_all(&payload)
        .await
        .context("Failed to send response")?;
    stream.shutdown().await.context("Failed to close socket")
}

async fn dispatch(
    request: AgentRequest,
    config: &AgentConfig,
    state: &AgentState,
) -> Result<AgentResponse> {
    match request {
        AgentRequest::ListFeatures {
            repo_path,
            include_removed,
        } => {
            let features = ops::list_features(config, repo_path, include_removed)?;
            let records: Vec<FeatureRecord> =
                features.into_iter().map(FeatureRecord::from).collect();
            Ok(AgentResponse::success(json!({ "features": records })))
        }
        AgentRequest::StartFeature {
            repo_path,
            name,
            title,
            base_branch,
            branch_prefix,
            reuse,
            telemetry,
            skip_modules,
            minimal,
            prompt,
        } => {
            let request = ops::build_start_request(ops::StartRequestParams {
                name,
                title,
                base_branch,
                branch_prefix,
                reuse_existing: reuse,
                telemetry,
                skip_modules,
                mode: if minimal {
                    StartMode::Minimal
                } else {
                    StartMode::Full
                },
                prompt_seed: prompt,
            });

            let summary = ops::start_feature(config, repo_path, request)?;
            state.record_feature_start(&summary).await?;
            let payload = StartFeaturePayload::from(summary);
            Ok(AgentResponse::success(json!({ "start": payload })))
        }
        AgentRequest::TeardownFeature {
            repo_path,
            name,
            branch_prefix,
            delete_branch,
            force,
            complete_spec,
            telemetry,
        } => {
            let request = ops::build_teardown_request(
                name,
                branch_prefix,
                delete_branch,
                force,
                complete_spec,
                telemetry,
            );
            let summary = ops::teardown_feature(config, repo_path, request)?;
            state.record_feature_teardown(&summary).await?;
            let payload = TeardownFeaturePayload::from(summary);
            Ok(AgentResponse::success(json!({ "teardown": payload })))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum AgentRequest {
    ListFeatures {
        #[serde(default)]
        repo_path: Option<PathBuf>,
        #[serde(default)]
        include_removed: bool,
    },
    StartFeature {
        #[serde(default)]
        repo_path: Option<PathBuf>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        base_branch: Option<String>,
        #[serde(default)]
        branch_prefix: Option<String>,
        #[serde(default)]
        reuse: bool,
        #[serde(default)]
        telemetry: bool,
        #[serde(default)]
        skip_modules: Vec<String>,
        #[serde(default)]
        minimal: bool,
        #[serde(default)]
        prompt: Option<String>,
    },
    TeardownFeature {
        #[serde(default)]
        repo_path: Option<PathBuf>,
        name: String,
        #[serde(default)]
        branch_prefix: Option<String>,
        #[serde(default)]
        delete_branch: bool,
        #[serde(default)]
        force: bool,
        #[serde(default)]
        complete_spec: bool,
        #[serde(default)]
        telemetry: bool,
    },
}

#[derive(Debug, Serialize)]
struct AgentResponse {
    status: ResponseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl AgentResponse {
    fn success(data: serde_json::Value) -> Self {
        Self {
            status: ResponseStatus::Success,
            data: Some(data),
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            status: ResponseStatus::Error,
            data: None,
            error: Some(message.into()),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ResponseStatus {
    Success,
    Error,
}

#[derive(Debug, Serialize)]
struct FeatureRecord {
    work_feature: String,
    branch_name: String,
    worktree_path: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    feature_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compose_project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tunnel_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tunnel_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tunnel_hostname: Option<String>,
    devcontainer_outdated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_sync_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_seed: Option<String>,
    start_mode: StartMode,
    #[serde(default)]
    module_outcomes: Vec<ModuleOutcomeRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pr_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adapter: Option<AdapterPayload>,
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
            status: meta.status.to_string(),
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

#[derive(Debug, Serialize)]
struct StartFeaturePayload {
    work_feature: String,
    branch_name: String,
    worktree_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    feature_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compose_project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env_path: Option<String>,
    mode: StartMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_seed: Option<String>,
    warnings: Vec<String>,
    skipped_modules: Vec<SkippedModulePayload>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    module_outcomes: Vec<ModuleOutcomePayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adapter: Option<AdapterPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tunnel: Option<TunnelPayload>,
    generated_at: String,
}

impl From<StartSummary> for StartFeaturePayload {
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
            skipped_modules: summary
                .skipped_modules
                .into_iter()
                .map(SkippedModulePayload::from)
                .collect(),
            module_outcomes: summary
                .module_outcomes
                .into_iter()
                .map(ModuleOutcomePayload::from)
                .collect(),
            adapter: summary.adapter.map(AdapterPayload::from),
            tunnel: summary.tunnel.map(TunnelPayload::from),
            generated_at: summary.generated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AdapterPayload {
    name: String,
    service_url: String,
    warnings: Vec<String>,
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

#[derive(Debug, Serialize)]
struct TunnelPayload {
    provider: String,
    hostname: Option<String>,
    status: String,
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

#[derive(Debug, Serialize)]
struct ModuleOutcomePayload {
    module: String,
    status: ModuleStatus,
    duration_ms: u64,
    notes: Vec<String>,
    forced: bool,
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

#[derive(Debug, Serialize)]
struct SkippedModulePayload {
    name: String,
    reason: String,
}

impl From<worktree_core::workflows::feature::ModuleSkipRecord> for SkippedModulePayload {
    fn from(record: worktree_core::workflows::feature::ModuleSkipRecord) -> Self {
        Self {
            name: record.name,
            reason: record.reason.description().to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TeardownFeaturePayload {
    work_feature: String,
    branch_name: String,
    worktree_removed: bool,
    branch_deleted: bool,
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    adapter_warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    module_reports: Vec<ModuleReportPayload>,
}

impl From<TeardownSummary> for TeardownFeaturePayload {
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

#[derive(Debug, Serialize)]
struct ModuleReportPayload {
    name: String,
    teardown_ok: bool,
    errors: Vec<String>,
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
