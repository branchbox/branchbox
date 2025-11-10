use crate::config::AgentConfig;
use anyhow::Result;
use std::path::PathBuf;
use worktree_core::workflows::feature::{
    FeatureStatus, FeatureWorkflow, StartMode, StartRequest, StartSummary, TeardownRequest,
    TeardownSummary,
};

pub fn list_features(
    config: &AgentConfig,
    repo_path: Option<PathBuf>,
    include_removed: bool,
) -> Result<Vec<worktree_core::workflows::feature::FeatureMetadata>> {
    let repo = resolve_repo(repo_path, config);
    let workflow = FeatureWorkflow::new(&repo)?;
    let mut entries = workflow.list_features()?;
    if !include_removed {
        entries.retain(|feature| feature.status == FeatureStatus::Active);
    }
    Ok(entries)
}

pub fn start_feature(
    config: &AgentConfig,
    repo_path: Option<PathBuf>,
    request: StartRequest,
) -> Result<StartSummary> {
    let repo = resolve_repo(repo_path, config);
    let workflow = FeatureWorkflow::new(&repo)?;
    Ok(workflow.start(request)?)
}

pub fn teardown_feature(
    config: &AgentConfig,
    repo_path: Option<PathBuf>,
    request: TeardownRequest,
) -> Result<TeardownSummary> {
    let repo = resolve_repo(repo_path, config);
    let workflow = FeatureWorkflow::new(&repo)?;
    Ok(workflow.teardown(request)?)
}

fn resolve_repo(repo_path: Option<PathBuf>, config: &AgentConfig) -> PathBuf {
    repo_path
        .map(CleanPath::clean_path)
        .unwrap_or_else(|| config.workspace_root.clone())
}

trait CleanPath {
    fn clean_path(self) -> PathBuf;
}

impl CleanPath for PathBuf {
    fn clean_path(self) -> PathBuf {
        std::fs::canonicalize(&self).unwrap_or(self)
    }
}

// Helper constructors used by IPC/GRPC layers.
pub fn build_start_request(
    name: Option<String>,
    title: Option<String>,
    base_branch: Option<String>,
    branch_prefix: Option<String>,
    reuse_existing: bool,
    telemetry: bool,
    skip_modules: Vec<String>,
    mode: StartMode,
    prompt_seed: Option<String>,
) -> StartRequest {
    StartRequest {
        name,
        title,
        base_branch,
        branch_prefix,
        reuse_existing,
        telemetry,
        skip_modules,
        mode,
        prompt_seed,
    }
}

pub fn build_teardown_request(
    work_feature: String,
    branch_prefix: Option<String>,
    delete_branch: bool,
    force_remove: bool,
    complete_spec: bool,
    telemetry: bool,
) -> TeardownRequest {
    TeardownRequest {
        work_feature,
        branch_prefix,
        delete_branch,
        force_remove,
        force_remove_modules: force_remove,
        complete_spec,
        telemetry,
    }
}
