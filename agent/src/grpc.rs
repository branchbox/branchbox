use crate::{config::AgentConfig, ops, state::AgentState};
use anyhow::Result;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};
use tracing::info;
use worktree_core::workflows::feature::{
    AdapterSummary, FeatureMetadata, FeatureTunnelState, ModuleOutcome, ModuleOutcomeRecord,
    ModuleTeardownReport, StartMode, StartSummary, TeardownSummary,
};

pub mod proto {
    tonic::include_proto!("branchbox.agent");
}

use proto::feature_service_server::{FeatureService, FeatureServiceServer};
use proto::{
    Adapter, Feature, ListRequest, ListResponse, ModuleOutcome as ProtoModuleOutcome,
    ModuleReport as ProtoModuleReport, SkippedModule as ProtoSkippedModule, StartRequest,
    StartResponse, StartSummary as ProtoStartSummary, TeardownRequest, TeardownResponse,
    TeardownSummary as ProtoTeardownSummary, Tunnel,
};

pub struct GrpcServer {
    config: Arc<AgentConfig>,
    state: AgentState,
}

impl GrpcServer {
    pub fn new(config: Arc<AgentConfig>, state: AgentState) -> Self {
        Self { config, state }
    }

    pub async fn serve(self, addr: SocketAddr) -> Result<()> {
        info!("gRPC server listening on {}", addr);
        Server::builder()
            .add_service(FeatureServiceServer::new(GrpcFeatureService {
                config: Arc::clone(&self.config),
                state: self.state.clone(),
            }))
            .serve(addr)
            .await?;
        Ok(())
    }
}

#[derive(Clone)]
struct GrpcFeatureService {
    config: Arc<AgentConfig>,
    state: AgentState,
}

#[tonic::async_trait]
impl FeatureService for GrpcFeatureService {
    async fn list(&self, request: Request<ListRequest>) -> Result<Response<ListResponse>, Status> {
        let req = request.into_inner();
        let repo_path = parse_repo_path(req.repo_path);
        let features =
            ops::list_features(&self.config, repo_path, req.include_removed).map_err(to_status)?;

        let feature_payloads = features
            .into_iter()
            .map(proto_feature_from_metadata)
            .collect();

        Ok(Response::new(ListResponse {
            features: feature_payloads,
        }))
    }

    async fn start(
        &self,
        request: Request<StartRequest>,
    ) -> Result<Response<StartResponse>, Status> {
        let req = request.into_inner();
        let repo_path = parse_repo_path(req.repo_path);
        let start_req = ops::build_start_request(ops::StartRequestParams {
            name: optional_string(req.name),
            title: optional_string(req.title),
            base_branch: optional_string(req.base_branch),
            branch_prefix: optional_string(req.branch_prefix),
            reuse_existing: req.reuse,
            telemetry: req.telemetry,
            skip_modules: req.skip_modules,
            mode: match req.mode.as_str() {
                "minimal" => StartMode::Minimal,
                _ => StartMode::Full,
            },
            prompt_seed: optional_string(req.prompt_seed),
        });

        let summary = ops::start_feature(&self.config, repo_path, start_req).map_err(to_status)?;
        self.state
            .record_feature_start(&summary)
            .await
            .map_err(to_status)?;

        Ok(Response::new(StartResponse {
            summary: Some(proto_start_summary(summary)),
        }))
    }

    async fn teardown(
        &self,
        request: Request<TeardownRequest>,
    ) -> Result<Response<TeardownResponse>, Status> {
        let req = request.into_inner();
        let repo_path = parse_repo_path(req.repo_path);
        let teardown_req = ops::build_teardown_request(
            req.name,
            optional_string(req.branch_prefix),
            req.delete_branch,
            req.force,
            req.complete_spec,
            req.telemetry,
        );

        let summary =
            ops::teardown_feature(&self.config, repo_path, teardown_req).map_err(to_status)?;
        self.state
            .record_feature_teardown(&summary)
            .await
            .map_err(to_status)?;

        Ok(Response::new(TeardownResponse {
            summary: Some(proto_teardown_summary(summary)),
        }))
    }
}

fn parse_repo_path(path: String) -> Option<PathBuf> {
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn optional_string(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn proto_feature_from_metadata(meta: FeatureMetadata) -> Feature {
    Feature {
        work_feature: meta.work_feature,
        branch_name: meta.branch_name,
        worktree_path: meta.worktree_path.display().to_string(),
        status: meta.status.to_string(),
        feature_url: meta.feature_url.unwrap_or_default(),
        env_path: meta
            .env_path
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        compose_project_name: meta.compose_project_name.unwrap_or_default(),
        color: meta.color.unwrap_or_default(),
        tunnel_provider: meta
            .tunnel
            .as_ref()
            .map(|state| state.provider.clone())
            .unwrap_or_default(),
        tunnel_status: meta
            .tunnel
            .as_ref()
            .map(|state| state.status.to_string())
            .unwrap_or_else(|| "none".to_string()),
        tunnel_hostname: meta
            .tunnel
            .and_then(|state| state.hostname)
            .unwrap_or_default(),
        devcontainer_outdated: meta.devcontainer_outdated,
        last_sync_at: meta
            .last_sync_at
            .map(|ts| ts.to_rfc3339())
            .unwrap_or_default(),
        sync_strategy: meta.sync_strategy.unwrap_or_default(),
        created_at: meta.created_at.to_rfc3339(),
        updated_at: meta.updated_at.to_rfc3339(),
        removed_at: meta
            .removed_at
            .map(|ts| ts.to_rfc3339())
            .unwrap_or_default(),
        prompt_seed: meta.prompt_seed.unwrap_or_default(),
        start_mode: meta.start_mode.to_string(),
        module_outcomes: meta
            .module_outcomes
            .into_iter()
            .map(proto_module_outcome_record)
            .collect(),
        pr_number: meta.pr_number.unwrap_or_default(),
    }
}

fn proto_start_summary(summary: StartSummary) -> ProtoStartSummary {
    ProtoStartSummary {
        work_feature: summary.work_feature,
        branch_name: summary.branch_name,
        worktree_path: summary.worktree_path.display().to_string(),
        feature_url: summary.feature_url.unwrap_or_default(),
        compose_project_name: summary.compose_project_name.unwrap_or_default(),
        env_path: summary
            .env_path
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        mode: summary.mode.to_string(),
        color: summary.color.unwrap_or_default(),
        prompt_seed: summary.prompt_seed.unwrap_or_default(),
        warnings: summary.warnings,
        module_outcomes: summary
            .module_outcomes
            .into_iter()
            .map(proto_module_outcome)
            .collect(),
        skipped_modules: summary
            .skipped_modules
            .into_iter()
            .map(proto_skipped_module)
            .collect(),
        adapter: summary.adapter.map(proto_adapter),
        tunnel: summary.tunnel.map(proto_tunnel),
        generated_at: summary.generated_at.to_rfc3339(),
    }
}

fn proto_teardown_summary(summary: TeardownSummary) -> ProtoTeardownSummary {
    ProtoTeardownSummary {
        work_feature: summary.work_feature,
        branch_name: summary.branch_name,
        worktree_removed: summary.worktree_removed,
        branch_deleted: summary.branch_deleted,
        warnings: summary.warnings,
        adapter_warnings: summary.adapter_cleanup_warnings,
        module_reports: summary
            .module_reports
            .into_iter()
            .map(proto_module_report)
            .collect(),
    }
}

fn proto_module_outcome(outcome: ModuleOutcome) -> ProtoModuleOutcome {
    ProtoModuleOutcome {
        module: outcome.module,
        status: outcome.status.to_string(),
        duration_ms: outcome.duration_ms,
        notes: outcome.notes,
        forced: outcome.forced,
    }
}

fn proto_module_outcome_record(record: ModuleOutcomeRecord) -> ProtoModuleOutcome {
    ProtoModuleOutcome {
        module: record.module,
        status: record.status.to_string(),
        duration_ms: record.duration_ms,
        notes: record.notes,
        forced: record.forced,
    }
}

fn proto_skipped_module(
    record: worktree_core::workflows::feature::ModuleSkipRecord,
) -> ProtoSkippedModule {
    ProtoSkippedModule {
        name: record.name,
        reason: record.reason.description().to_string(),
    }
}

fn proto_adapter(summary: AdapterSummary) -> Adapter {
    Adapter {
        name: summary.name,
        service_url: summary.service_url,
        warnings: summary.warnings,
    }
}

fn proto_tunnel(state: FeatureTunnelState) -> Tunnel {
    Tunnel {
        provider: state.provider,
        hostname: state.hostname.unwrap_or_default(),
        status: state.status.to_string(),
    }
}

fn proto_module_report(report: ModuleTeardownReport) -> ProtoModuleReport {
    ProtoModuleReport {
        name: report.name,
        teardown_ok: report.teardown_ok,
        errors: report.errors,
    }
}

fn to_status(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}
