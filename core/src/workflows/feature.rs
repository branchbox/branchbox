use super::teardown_plan::{
    blocking_changes, branch_action, build_teardown_plan, classify_changes, discards_changes,
    parse_jsonc_object, Blocker, BranchAction, BranchPlan, BranchSource, FsChangeContext,
    PlanInputs, PreservedFile, ResolvedBranch, TeardownDefaults, TeardownOptions, TeardownPlan,
    UserChange, WorktreeState, WorktreeStatus, ENV_FEATURE_SECTION_MARKER, MAX_CLASSIFIED_ENTRIES,
    VSCODE_COLOR_CUSTOMIZATIONS, VSCODE_FEATURE_URL_TASK, VSCODE_PEACOCK_COLOR,
    VSCODE_PEACOCK_REMOTE_COLOR, VSCODE_WINDOW_TITLE,
};
use crate::{
    adapters, atomic_fs,
    config::BranchBoxConfig,
    devcontainer_runtime::DevcontainerConfig,
    git::{
        repository_common_git_dir, require_private_common_git_metadata, GitWorktree, RemovalForce,
        WorktreeRegistration,
    },
    modules::{self, DevcontainerModule, ModuleHandle, SpecStatus},
    naming,
    runtime::{
        self, InGuestFacadePlan, RuntimeContext, RuntimeMetadata, RuntimePort, RuntimeProviderKind,
    },
    tunnel::{
        cloudflared::CloudflaredProvider, ProvisioningIntent, ProvisioningOutcome,
        TunnelDescriptor, TunnelProvider,
    },
    validation::{self, AppUrl},
    Error, Result,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt},
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::time::Instant;
#[cfg(unix)]
use std::{
    ffi::CString,
    os::fd::{AsRawFd, FromRawFd},
};

/// Capabilities of the feature workflow, reported by `branchbox version --json`.
/// `write-ahead-start`: `start` registers the feature as soon as its worktree exists, and
/// `list` reports an unfinished start as `setup.state: interrupted` (DESIGN §5.4).
pub(crate) const CAPABILITIES: &[&str] = &["write-ahead-start"];

/// Feature start execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StartMode {
    #[default]
    Full,
    Minimal,
}

impl fmt::Display for StartMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            StartMode::Full => "full",
            StartMode::Minimal => "minimal",
        };
        f.write_str(label)
    }
}

/// Module execution status for feature workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModuleStatus {
    #[default]
    Success,
    Skipped,
    Failed,
}

impl fmt::Display for ModuleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            ModuleStatus::Success => "success",
            ModuleStatus::Skipped => "skipped",
            ModuleStatus::Failed => "failed",
        };
        f.write_str(label)
    }
}

/// Captures runtime outcome for a module during feature start.
#[derive(Debug, Clone)]
pub struct ModuleOutcome {
    pub module: String,
    pub status: ModuleStatus,
    pub duration_ms: u64,
    pub notes: Vec<String>,
    pub forced: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum ModuleSkipReason {
    Requested,
    MinimalDefault,
    Auto,
    Policy,
}

impl ModuleSkipReason {
    pub fn description(&self) -> &'static str {
        match self {
            ModuleSkipReason::Requested => "Skipped via --skip-module",
            ModuleSkipReason::MinimalDefault => "Skipped by minimal mode defaults",
            ModuleSkipReason::Auto => "Automatically skipped by BranchBox configuration",
            ModuleSkipReason::Policy => "Skipped due to policy enforcement",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleSkipRecord {
    pub name: String,
    pub reason: ModuleSkipReason,
}

fn register_module_skip(
    module_skip: &mut Vec<String>,
    skip_records: &mut Vec<ModuleSkipRecord>,
    name: &str,
    reason: ModuleSkipReason,
) {
    let normalized = name.to_ascii_lowercase();
    if !module_skip
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&normalized))
    {
        module_skip.push(normalized.clone());
    }

    if !skip_records
        .iter()
        .any(|record| record.name.eq_ignore_ascii_case(&normalized))
    {
        skip_records.push(ModuleSkipRecord {
            name: normalized,
            reason,
        });
    }
}

fn load_policy_enforced_modules() -> HashSet<String> {
    let mut modules = HashSet::new();

    if let Ok(raw) = std::env::var("BRANCHBOX_POLICY_ENFORCED_MODULES") {
        for value in raw.split(',') {
            let normalized = value.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                modules.insert(normalized);
            }
        }
    }

    modules
}

/// Manages feature lifecycle workflows (start/teardown) using git worktrees.
#[derive(Debug)]
pub struct FeatureWorkflow {
    repo_root: PathBuf,
    /// Preserve a caller-supplied lexical repository path for exact Docker workspace-label cleanup.
    repo_root_alias: Option<PathBuf>,
    git: GitWorktree,
    state: FeatureStateStore,
    /// Test hook: runs on the worktree right before teardown checks it again and removes it,
    /// to stand in for a change made while the runtime and modules were being stopped.
    #[cfg(test)]
    before_worktree_removal: Option<fn(&Path)>,
}

/// Parameters for starting a feature worktree.
#[derive(Debug, Default)]
pub struct StartRequest {
    pub name: Option<String>,
    pub title: Option<String>,
    pub base_branch: Option<String>,
    pub branch_prefix: Option<String>,
    pub reuse_existing: bool,
    /// How copy-mode devcontainer divergence is handled while reusing a worktree.
    pub devcontainer_reuse: DevcontainerReusePolicy,
    /// Retain a failed outer runtime so a later retry can reuse its build cache and diagnostics.
    pub keep_runtime_on_failure: bool,
    pub telemetry: bool,
    /// List of module names to skip during setup (e.g., "tunnel", "database")
    pub skip_modules: Vec<String>,
    /// Start mode (full or minimal)
    pub mode: StartMode,
    /// Optional prompt seed captured for agent/automation hand-off
    pub prompt_seed: Option<String>,
    /// Optional execution-boundary override. Defaults to project configuration.
    pub runtime: Option<RuntimeProviderKind>,
    /// Absolute supervisor-authored manifest for an in-guest runtime. The CLI transports only the
    /// path; the manifest transports opaque identities and validated materialization paths.
    pub runtime_manifest: Option<PathBuf>,
    /// Where the feature's checkout lives. Defaults to a worktree beside the repository.
    pub workspace_mode: WorkspaceMode,
}

/// Where a feature's checkout lives.
///
/// A worktree lets several features share one clone on a long-lived machine,
/// which is the whole point on a developer's laptop. It costs something too: the
/// checkout gains a second identity, its administrative files live under the
/// repository it was cut from, and both have to be reachable and owned
/// correctly wherever the work runs.
///
/// A caller that clones per run and discards the clone afterwards has no second
/// consumer to isolate from, so it pays that cost for nothing. `Repository`
/// checks the branch out in place instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceMode {
    /// Cut a worktree beside the repository. The default.
    #[default]
    Worktree,
    /// Check the feature branch out in the repository itself.
    Repository,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DevcontainerReusePolicy {
    #[default]
    Fail,
    Preserve,
    Overwrite,
    Inspect,
}

impl fmt::Display for DevcontainerReusePolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Fail => "fail",
            Self::Preserve => "preserve",
            Self::Overwrite => "overwrite",
            Self::Inspect => "inspect",
        })
    }
}

impl FromStr for DevcontainerReusePolicy {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fail" => Ok(Self::Fail),
            "preserve" => Ok(Self::Preserve),
            "overwrite" => Ok(Self::Overwrite),
            "inspect" | "diff" => Ok(Self::Inspect),
            other => Err(format!(
                "unknown devcontainer reuse policy '{other}'; expected fail, preserve, overwrite, or inspect"
            )),
        }
    }
}

/// Result of running feature start workflow.
#[derive(Debug)]
pub struct StartSummary {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub feature_url: Option<String>,
    pub compose_project_name: Option<String>,
    pub env_path: Option<PathBuf>,
    pub tunnel: Option<FeatureTunnelState>,
    pub adapter: Option<AdapterSummary>,
    pub module_reports: Vec<ModuleSetupReport>,
    pub warnings: Vec<String>,
    pub color: Option<String>,
    pub mode: StartMode,
    pub prompt_seed: Option<String>,
    pub runtime: RuntimeMetadata,
    pub module_outcomes: Vec<ModuleOutcome>,
    pub skipped_modules: Vec<ModuleSkipRecord>,
    pub generated_at: DateTime<Utc>,
}

/// Module setup execution report.
#[derive(Debug)]
pub struct ModuleSetupReport {
    pub name: String,
    pub init_ok: bool,
    pub setup_ok: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Default)]
struct ModuleSetupOutcome {
    reports: Vec<ModuleSetupReport>,
    warnings: Vec<String>,
    executions: Vec<ModuleOutcome>,
}

#[derive(Debug, Default)]
struct StashState {
    created: bool,
    reference: Option<String>,
}

/// The write-ahead registry entry of a start in progress (see [`SetupRecord`]).
#[derive(Debug)]
struct ProvisionalStart {
    work_feature: String,
    pid: u32,
    /// The entry the provisional one replaced, restored if the start removes its worktree.
    previous: Option<FeatureMetadata>,
}

/// Parameters for tearing down a feature worktree.
#[derive(Debug, Clone)]
pub struct TeardownRequest {
    pub work_feature: String,
    pub branch_prefix: Option<String>,
    pub delete_branch: bool,
    /// Force branch deletion (`git branch -D`) when the branch is not fully merged.
    pub force_delete_branch: bool,
    pub force_remove: bool,
    /// Override module dirty checks even if --force was not provided.
    pub force_remove_modules: bool,
    pub complete_spec: bool,
    pub telemetry: bool,
}

/// Result of running feature teardown workflow.
///
/// The fields after `warnings` were added in 0.14 (DESIGN §5.6); clients of older CLIs see
/// them missing.
#[derive(Debug, Serialize)]
pub struct TeardownSummary {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub adapter_cleanup_warnings: Vec<String>,
    pub module_reports: Vec<ModuleTeardownReport>,
    pub runtime_teardown: runtime::RuntimeTeardownReport,
    pub warnings: Vec<String>,
    /// The branch step that ran: `keep`, `delete` (`git branch -d`) or `force_delete` (`-D`).
    pub branch_action: BranchAction,
    /// Why deleting the branch failed. Teardown still succeeds: the worktree is gone and the
    /// branch is kept.
    pub branch_delete_error: Option<String>,
    /// User changes discarded with the worktree (`--discard-changes` or `--force`).
    pub discarded_changes: Vec<UserChange>,
    /// Files moved into the main worktree before the worktree was removed (the feature spec).
    pub preserved: Vec<PreservedFile>,
    /// Whether the registry entry was marked removed.
    pub registry_updated: bool,
}

/// Module teardown execution report.
#[derive(Debug, Serialize)]
pub struct ModuleTeardownReport {
    pub name: String,
    pub teardown_ok: bool,
    pub errors: Vec<String>,
}

/// Parameters for opening (provisioning) a tunnel outside feature start.
#[derive(Debug)]
pub struct TunnelOpenRequest {
    pub work_feature: String,
}

/// Result of running a tunnel open/provision operation.
#[derive(Debug)]
pub struct TunnelOpenSummary {
    pub work_feature: String,
    pub state: FeatureTunnelState,
    pub warnings: Vec<String>,
}

/// Parameters for removing a tunnel.
#[derive(Debug)]
pub struct TunnelRemoveRequest {
    pub work_feature: String,
    pub force: bool,
}

/// Result of removing a tunnel for a feature.
#[derive(Debug)]
pub struct TunnelRemoveSummary {
    pub work_feature: String,
    pub previous_state: Option<FeatureTunnelState>,
    pub updated_state: Option<FeatureTunnelState>,
    pub warnings: Vec<String>,
}

/// Adapter orchestration summary.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AdapterSummary {
    pub name: String,
    pub service_url: String,
    pub warnings: Vec<String>,
}

impl FeatureWorkflow {
    /// Create a new workflow instance from a repository path.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let candidate = path.into();
        let repo_root = resolve_repo_root(&candidate)?;
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            std::env::current_dir()?.join(candidate)
        };
        let canonical_root = repo_root.canonicalize()?;
        let canonical_parent = canonical_root.parent();
        let repo_root_alias = candidate
            .ancestors()
            .find(|path| {
                path.canonicalize().ok().as_ref() == Some(&canonical_root)
                    && path
                        .parent()
                        .and_then(|parent| parent.canonicalize().ok())
                        .as_deref()
                        == canonical_parent
            })
            .filter(|path| *path != repo_root)
            .map(Path::to_path_buf);
        validation::validate_git_worktree(&repo_root)?;

        let git = GitWorktree::new(&repo_root)?;
        let state = FeatureStateStore::new(&repo_root);
        Ok(Self {
            repo_root,
            repo_root_alias,
            git,
            state,
            #[cfg(test)]
            before_worktree_removal: None,
        })
    }

    /// Get absolute path to repository root.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Start a feature, creating a new git worktree and running module setup.
    pub fn start(&self, request: StartRequest) -> Result<StartSummary> {
        self.ensure_host_environment()?;

        let work_feature = self.resolve_work_feature(&request)?;
        let config = BranchBoxConfig::load(&self.repo_root).unwrap_or_default();
        let runtime_kind = request.runtime.unwrap_or(config.runtime.provider);
        match (runtime_kind, request.runtime_manifest.as_deref()) {
            (RuntimeProviderKind::InGuest, None) => {
                return Err(Error::validation(
                    "--runtime in-guest requires --runtime-manifest with a trusted guest path",
                ));
            }
            (RuntimeProviderKind::InGuest, Some(path)) if !path.is_absolute() => {
                return Err(Error::validation(
                    "--runtime-manifest must be an absolute path inside the trusted guest",
                ));
            }
            (RuntimeProviderKind::InGuest, Some(_)) | (_, None) => {}
            (_, Some(_)) => {
                return Err(Error::validation(
                    "--runtime-manifest is accepted only with --runtime in-guest",
                ));
            }
        }
        if request.keep_runtime_on_failure && runtime_kind != RuntimeProviderKind::Sbx {
            return Err(Error::validation(
                "--keep-runtime-on-failure and --reuse-runtime are only supported with the SBX runtime",
            ));
        }
        if runtime_kind == RuntimeProviderKind::InGuest
            && request.workspace_mode == WorkspaceMode::Repository
        {
            return Err(Error::validation(
                "Managed in-guest launch requires a separate task worktree so the coding consumer cannot remove provider state from the mounted repository",
            ));
        }
        if runtime_kind == RuntimeProviderKind::InGuest && request.reuse_existing {
            return Err(Error::validation(
                "Managed in-guest launch cannot reuse a branch after the coding consumer had write access to shared Git metadata",
            ));
        }
        if runtime_kind == RuntimeProviderKind::InGuest {
            require_private_common_git_metadata(&self.repo_root)?;
        }
        let runtime_provider = runtime::provider(runtime_kind)?;
        if runtime_kind != RuntimeProviderKind::InGuest {
            runtime_provider.validate()?;
        }
        let branch_prefix = request
            .branch_prefix
            .clone()
            .or_else(|| Some(config.feature.branch_prefix.clone()));
        let branch_name = build_branch_name(branch_prefix.as_deref(), &work_feature);
        let repository_workspace = request.workspace_mode == WorkspaceMode::Repository;
        let worktree_path = if repository_workspace {
            self.repo_root.clone()
        } else {
            self.worktree_path(&work_feature)?
        };
        let mut branch_exists = if runtime_kind == RuntimeProviderKind::InGuest {
            self.git.branch_exists_consumer_writable(&branch_name)?
        } else {
            self.git.branch_exists(&branch_name)?
        };
        // In repository mode the checkout already exists -- it is the repository
        // -- so its presence says nothing about whether this feature was started
        // before, and must not be read as a collision.
        let worktree_exists = !repository_workspace && worktree_path.exists();
        let base_branch = request.base_branch.clone();
        let mut warnings = Vec::new();
        let mut reuse_existing = request.reuse_existing;

        if runtime_kind != RuntimeProviderKind::InGuest
            && !reuse_existing
            && branch_exists
            && !worktree_exists
        {
            if let Ok(Some(metadata)) = self.state.get_feature(&work_feature) {
                if metadata.status == FeatureStatus::Removed || !metadata.worktree_path.exists() {
                    reuse_existing = true;
                    warnings.push(format!(
                        "Branch '{}' already exists; recreating worktree from existing branch.",
                        branch_name
                    ));
                }
            }
        }

        if request.telemetry {
            std::env::set_var("BRANCHBOX_EMIT_TELEMETRY", "1");
        } else {
            std::env::remove_var("BRANCHBOX_EMIT_TELEMETRY");
        }

        if worktree_exists && !reuse_existing {
            return Err(Error::WorktreeExists(worktree_path));
        }

        if reuse_existing && !branch_exists {
            for remote in self.git.list_remotes()? {
                if self.git.remote_branch_exists(&remote, &branch_name)? {
                    self.git.create_branch_from_remote(&branch_name, &remote)?;
                    branch_exists = true;
                    tracing::info!(
                        "Created local branch '{}' from remote '{}' to honor reuse request",
                        branch_name,
                        remote
                    );
                    break;
                }
            }
        }

        let workspace_mount_path = worktree_path.parent().unwrap_or(&worktree_path);
        let repository_revision = resolve_git_object(
            &self.repo_root,
            if reuse_existing {
                &branch_name
            } else {
                base_branch.as_deref().unwrap_or("HEAD")
            },
        )?;
        let in_guest_plan = if runtime_kind == RuntimeProviderKind::InGuest {
            let manifest_path = request
                .runtime_manifest
                .as_deref()
                .expect("validated in-guest manifest");
            runtime::require_secure_in_guest_launch_assignment(manifest_path)?;
            validate_untrusted_checkout_attributes(&self.repo_root, &repository_revision)?;
            Some(runtime::load_in_guest_facade_plan(
                manifest_path,
                &self.repo_root,
                workspace_mount_path,
                &worktree_path,
                &branch_name,
                &repository_revision,
            )?)
        } else {
            None
        };

        if !reuse_existing && branch_exists {
            return Err(Error::BranchExists(branch_name.clone()));
        }

        if reuse_existing && !branch_exists {
            return Err(Error::validation(format!(
                "Cannot reuse feature branch '{}' because it does not exist locally. Fetch or create it before retrying.",
                branch_name
            )));
        }

        if worktree_exists {
            if reuse_existing {
                tracing::info!("Using existing worktree at {}", worktree_path.display());
            } else {
                return Err(Error::WorktreeExists(worktree_path));
            }
        } else if reuse_existing {
            tracing::info!(
                "Recreating missing worktree for existing branch {}",
                branch_name
            );
            if runtime_kind == RuntimeProviderKind::InGuest {
                self.git
                    .attach_existing_branch_without_hooks(&worktree_path, &branch_name)?;
            } else {
                self.git
                    .attach_existing_branch(&worktree_path, &branch_name)?;
            }
        } else if repository_workspace {
            self.git
                .checkout_feature_branch(&branch_name, base_branch.as_deref())?;
        } else if runtime_kind == RuntimeProviderKind::InGuest {
            self.git
                .create_without_hooks(&worktree_path, &branch_name, base_branch.as_deref())?;
        } else {
            self.git
                .create(&worktree_path, &branch_name, base_branch.as_deref())?;
        }

        // Write-ahead (DESIGN §5.4): register the feature as soon as its worktree exists, so a
        // start that dies during setup is listed as interrupted instead of leaving an
        // unregistered worktree behind. The final registry update clears the marker. From here
        // on, an error path that removes the worktree again also discards this entry; every
        // other error path leaves it, and `list` then reports the setup as interrupted.
        let setup = SetupRecord::begin();
        let provisional = self.record_provisional_start(
            FeatureMetadata {
                work_feature: work_feature.clone(),
                branch_name: branch_name.clone(),
                worktree_path: worktree_path.clone(),
                base_branch: base_branch.clone(),
                feature_url: None,
                compose_project_name: None,
                env_path: None,
                status: FeatureStatus::Active,
                created_at: setup.started_at,
                updated_at: setup.started_at,
                removed_at: None,
                tunnel: None,
                color: Some(generate_feature_color(&work_feature)),
                pr_number: None,
                last_commit: None,
                devcontainer_outdated: false,
                last_sync_at: None,
                sync_strategy: None,
                start_mode: request.mode,
                prompt_seed: request.prompt_seed.clone(),
                module_outcomes: Vec::new(),
                last_summary_rendered_at: None,
                adapter: None,
                runtime: RuntimeMetadata {
                    provider: runtime_kind,
                    ..RuntimeMetadata::default()
                },
                setup: Some(setup),
            },
            &mut warnings,
        );

        // Both of these exist to make a worktree reachable from the container:
        // one rewrites the worktree's absolute gitdir pointer, the other projects
        // its administrative files under the main checkout. A repository
        // workspace has neither a pointer to rewrite nor metadata to project.
        if !repository_workspace {
            if let Err(err) = self.fix_git_worktree_path(&worktree_path) {
                tracing::warn!("Failed to fix git worktree path: {}", err);
                warnings.push(format!("Git worktree path fix failed: {}", err));
            }
        }

        // This must be the first repository-content processing step in the trusted guest. It
        // strips host lifecycle hooks and ambient mounts before any Dev Containers command runs.
        if let Some(plan) = in_guest_plan.as_ref() {
            let prepared =
                prepare_in_guest_devcontainer_config(&self.repo_root, &worktree_path, plan)
                    .and_then(|()| runtime_provider.validate());
            if let Err(err) = prepared {
                self.cleanup_failed_in_guest_worktree(
                    &worktree_path,
                    &branch_name,
                    in_guest_plan.as_ref(),
                    provisional.as_ref(),
                );
                return Err(err);
            }
            if !repository_workspace {
                if let Err(err) = self.set_in_guest_git_worktree_path(&worktree_path) {
                    self.cleanup_failed_in_guest_worktree(
                        &worktree_path,
                        &branch_name,
                        in_guest_plan.as_ref(),
                        provisional.as_ref(),
                    );
                    return Err(err);
                }
            }
            let common_git = match repository_common_git_dir(&self.repo_root) {
                Ok(path) => path,
                Err(err) => {
                    self.cleanup_failed_in_guest_worktree(
                        &worktree_path,
                        &branch_name,
                        in_guest_plan.as_ref(),
                        provisional.as_ref(),
                    );
                    return Err(err);
                }
            };
            if let Err(err) = plan.grant_workspace_consumer_access(&worktree_path, &common_git) {
                self.cleanup_failed_in_guest_worktree(
                    &worktree_path,
                    &branch_name,
                    in_guest_plan.as_ref(),
                    provisional.as_ref(),
                );
                return Err(err);
            }
        }

        let stash_state = if runtime_kind != RuntimeProviderKind::InGuest && !reuse_existing {
            match self.capture_stash(&work_feature) {
                Ok(state) => state,
                Err(err) => {
                    warnings.push(format!("Failed to stash local changes: {}", err));
                    StashState::default()
                }
            }
        } else {
            StashState::default()
        };

        if runtime_kind != RuntimeProviderKind::InGuest {
            self.ensure_spec_for_start(&work_feature, &worktree_path, &branch_name, &mut warnings);
        }

        let adapter_copy_allowed = !reuse_existing;
        let adapter_summary = if runtime_kind == RuntimeProviderKind::InGuest {
            None
        } else {
            match self.prepare_adapter(&worktree_path, adapter_copy_allowed) {
                Ok(summary) => Some(summary),
                Err(err) => {
                    warnings.push(err);
                    None
                }
            }
        };

        let mut module_skip: Vec<String> = Vec::new();
        let mut skip_records: Vec<ModuleSkipRecord> = Vec::new();

        for name in &request.skip_modules {
            register_module_skip(
                &mut module_skip,
                &mut skip_records,
                name,
                ModuleSkipReason::Requested,
            );
        }

        if runtime_kind == RuntimeProviderKind::InGuest {
            for name in ["devcontainer", "compose", "database", "tunnel", "specs"] {
                register_module_skip(
                    &mut module_skip,
                    &mut skip_records,
                    name,
                    ModuleSkipReason::Policy,
                );
            }
        }

        if matches!(request.mode, StartMode::Minimal) {
            for default in ["devcontainer", "compose", "specs"] {
                register_module_skip(
                    &mut module_skip,
                    &mut skip_records,
                    default,
                    ModuleSkipReason::MinimalDefault,
                );
            }
        }

        let policy_enforced = load_policy_enforced_modules();
        if runtime_kind != RuntimeProviderKind::InGuest && !policy_enforced.is_empty() {
            skip_records.retain(|record| {
                if policy_enforced.contains(&record.name) {
                    warnings.push(format!(
                        "Module '{}' is policy enforced and will run even if minimal mode skips it.",
                        record.name
                    ));
                    false
                } else {
                    true
                }
            });
            module_skip.retain(|module| !policy_enforced.contains(module));
        }

        let skip_tunnel_provisioning = !policy_enforced.contains("tunnel")
            && request
                .skip_modules
                .iter()
                .any(|name| name.eq_ignore_ascii_case("tunnel"));

        // Tunnel provisioning is handled by the workflow (provider-based) rather than the legacy
        // tunnel module. Avoid surfacing the tunnel module as "skipped" in summaries unless the
        // user explicitly asked to skip it.
        if !policy_enforced.contains("tunnel")
            && !module_skip
                .iter()
                .any(|name| name.eq_ignore_ascii_case("tunnel"))
        {
            module_skip.push("tunnel".to_string());
        }

        let modules::ModulePlan {
            handles,
            warnings: dependency_warnings,
        } = if runtime_kind == RuntimeProviderKind::InGuest {
            modules::ModulePlan::new(Vec::new(), Vec::new())
        } else {
            modules::detect_modules(&self.repo_root, &module_skip)
        };
        if !dependency_warnings.is_empty() {
            warnings.extend(dependency_warnings.clone());
        }

        let forced_modules = if runtime_kind == RuntimeProviderKind::InGuest {
            HashSet::new()
        } else {
            policy_enforced.clone()
        };

        let env_outcome = if runtime_kind == RuntimeProviderKind::InGuest {
            EnvOutcome {
                env_path: None,
                feature_url: None,
                compose_project_name: None,
                skipped: true,
            }
        } else {
            self.prepare_env(
                &worktree_path,
                &work_feature,
                &branch_name,
                reuse_existing,
                &mut warnings,
            )?
        };
        if env_outcome.skipped {
            warnings.push("Skipped .env provisioning (source file not found)".to_string());
        }

        let previous_service_url = std::env::var("SERVICE_URL").ok();
        if let Some(summary) = adapter_summary.as_ref() {
            std::env::set_var("SERVICE_URL", &summary.service_url);
        }
        let previous_reuse_policy = std::env::var_os("BRANCHBOX_DEVCONTAINER_REUSE_POLICY");
        let devcontainer_sync_strategy =
            if std::env::var("BRANCHBOX_DEVCONTAINER_STRATEGY").as_deref() == Ok("symlink") {
                "symlink"
            } else {
                "copy"
            };
        if reuse_existing {
            std::env::set_var(
                "BRANCHBOX_DEVCONTAINER_REUSE_POLICY",
                request.devcontainer_reuse.to_string(),
            );
        } else {
            std::env::remove_var("BRANCHBOX_DEVCONTAINER_REUSE_POLICY");
        }
        let setup_outcome = self.run_module_setup(handles, &worktree_path, &forced_modules);
        match previous_reuse_policy {
            Some(value) => std::env::set_var("BRANCHBOX_DEVCONTAINER_REUSE_POLICY", value),
            None => std::env::remove_var("BRANCHBOX_DEVCONTAINER_REUSE_POLICY"),
        }
        match previous_service_url {
            Some(value) => std::env::set_var("SERVICE_URL", value),
            None => std::env::remove_var("SERVICE_URL"),
        }
        if reuse_existing {
            if let Some(conflict) = setup_outcome.executions.iter().find(|outcome| {
                outcome.module.eq_ignore_ascii_case("devcontainer")
                    && outcome.status == ModuleStatus::Failed
                    && outcome.notes.iter().any(|note| {
                        note.contains("feature-local devcontainer")
                            || note.contains("Feature-local devcontainer")
                    })
            }) {
                return Err(Error::validation(conflict.notes.join("; ")));
            }
        }
        if !setup_outcome.warnings.is_empty() {
            warnings.extend(setup_outcome.warnings.clone());
        }
        let mut module_outcomes = setup_outcome.executions.clone();
        for record in &skip_records {
            if module_outcomes
                .iter()
                .any(|outcome| outcome.module.eq_ignore_ascii_case(&record.name))
            {
                continue;
            }
            module_outcomes.push(ModuleOutcome {
                module: record.name.clone(),
                status: ModuleStatus::Skipped,
                duration_ms: 0,
                notes: vec![record.reason.description().to_string()],
                forced: matches!(record.reason, ModuleSkipReason::Policy),
            });
        }
        let module_reports = setup_outcome.reports;
        if runtime_kind != RuntimeProviderKind::InGuest && !reuse_existing {
            let stash_warnings = self.apply_stash_to_worktree(&stash_state, &worktree_path);
            if !stash_warnings.is_empty() {
                warnings.extend(stash_warnings);
            }
        }

        let env_path = env_outcome.env_path.clone();
        let feature_url = env_outcome.feature_url.clone();
        let compose_project_name = in_guest_plan
            .as_ref()
            .map(|plan| plan.managed_compose_project_name(&worktree_path))
            .or_else(|| env_outcome.compose_project_name.clone());
        let project_name = self
            .repo_root
            .parent()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        let fallback_runtime_name = format!("{project_name}-{work_feature}");
        let runtime_name = compose_project_name
            .as_deref()
            .unwrap_or(&fallback_runtime_name);
        let published_ports = in_guest_plan
            .as_ref()
            .map(|plan| plan.published_ports().to_vec())
            .unwrap_or_else(|| runtime_ports(&worktree_path));

        if runtime_kind == RuntimeProviderKind::Sbx {
            prepare_sbx_devcontainer_config(
                &self.repo_root,
                &worktree_path,
                &config.runtime.sbx.run_services,
            )?;
        }

        // Tunnel credentials are part of the Compose input. Materialize them before creating an
        // outer runtime or asking devcontainers to validate/start the project.
        let service_url_for_tunnel = self
            .resolve_tunnel_service_url(adapter_summary.as_ref().map(|s| s.service_url.as_str()));
        let (tunnel_state, mut tunnel_warnings) = if in_guest_plan.is_some() {
            (
                Some(FeatureTunnelState {
                    provider: "outer".to_string(),
                    hostname: feature_url.clone(),
                    service_url: published_ports
                        .first()
                        .map(|port| format!("http://127.0.0.1:{}", port.host)),
                    status: FeatureTunnelStatus::Pending,
                    descriptor: None,
                    instructions: None,
                    notes: Some(
                        "The managed orchestrator owns the exclusive outer-boundary tunnel connector"
                            .to_string(),
                    ),
                    last_updated: Utc::now(),
                    removed_at: None,
                }),
                Vec::new(),
            )
        } else {
            self.prepare_tunnel_state(
                skip_tunnel_provisioning,
                &work_feature,
                &worktree_path,
                feature_url.as_deref(),
                &service_url_for_tunnel,
            )?
        };
        if !tunnel_warnings.is_empty() {
            warnings.append(&mut tunnel_warnings);
        }
        if runtime_kind == RuntimeProviderKind::Sbx
            && tunnel_state.as_ref().is_some_and(|state| {
                state.status == FeatureTunnelStatus::Disabled
                    && state
                        .notes
                        .as_deref()
                        .is_some_and(|notes| notes.contains("credentials are configured"))
            })
            && devcontainer_requires_cloudflared_env(&worktree_path)
        {
            return Err(Error::validation(
                "SBX devcontainer requires .devcontainer/.cloudflared.env, but Cloudflare credentials are not configured. Set CLOUDFLARE_TUNNEL_TOKEN or configure tunnel.providers.cloudflared before retrying; no sandbox was created."
                    .to_string(),
            ));
        }

        let runtime_context = RuntimeContext {
            work_feature: &work_feature,
            worktree_path: &worktree_path,
            runtime_name,
            workspace_mount_path,
            published_ports: &published_ports,
            runtime_manifest_path: request.runtime_manifest.as_deref(),
        };
        let mut runtime_metadata = match runtime_provider.prepare(&runtime_context) {
            Ok(metadata) => {
                // The runtime now exists (an sbx sandbox or a local VM), and starting the
                // environment is the longest step. Record its identity in the write-ahead entry,
                // so a start killed from here on can still be torn down: destroy needs
                // `runtime_id`, and without it teardown would report the runtime residue-free.
                if runtime_kind != RuntimeProviderKind::InGuest && metadata.runtime_id.is_some() {
                    self.record_provisional_runtime(provisional.as_ref(), &metadata);
                }
                metadata
            }
            Err(err) => {
                self.rollback_prepared_tunnel(tunnel_state.as_ref());
                if runtime_kind == RuntimeProviderKind::InGuest {
                    self.cleanup_failed_in_guest_worktree(
                        &worktree_path,
                        &branch_name,
                        in_guest_plan.as_ref(),
                        provisional.as_ref(),
                    );
                }
                return Err(err);
            }
        };
        let environment_result =
            runtime_provider.start_environment(&runtime_context, &mut runtime_metadata);

        // Keep successful in-guest worktrees pointed at the exact container Git projection while
        // the coding environment is active. Failed starts and other runtime providers need the
        // host-portable pointer immediately; normal in-guest teardown repairs it before status and
        // removal checks.
        if runtime_kind != RuntimeProviderKind::InGuest || environment_result.is_err() {
            if let Err(err) = self.fix_git_worktree_path(&worktree_path) {
                tracing::warn!("Failed to restore git worktree path: {}", err);
                warnings.push(format!("Git worktree path restore failed: {}", err));
            }
        }

        if let Err(err) = environment_result {
            if request.keep_runtime_on_failure {
                let now = Utc::now();
                let module_outcome_records = module_outcomes
                    .iter()
                    .map(|outcome| ModuleOutcomeRecord {
                        module: outcome.module.clone(),
                        status: outcome.status,
                        duration_ms: outcome.duration_ms,
                        notes: outcome.notes.clone(),
                        forced: outcome.forced,
                        recorded_at: Some(now),
                    })
                    .collect();
                self.state.record_start(FeatureMetadata {
                    work_feature: work_feature.clone(),
                    branch_name: branch_name.clone(),
                    worktree_path: worktree_path.clone(),
                    base_branch: base_branch.clone(),
                    feature_url: feature_url.clone(),
                    compose_project_name: compose_project_name.clone(),
                    env_path: env_path.clone(),
                    tunnel: tunnel_state.clone(),
                    status: FeatureStatus::FailedRetained,
                    created_at: now,
                    updated_at: now,
                    removed_at: None,
                    color: Some(generate_feature_color(&work_feature)),
                    pr_number: None,
                    last_commit: get_last_commit_sha(&self.repo_root, &branch_name),
                    devcontainer_outdated: false,
                    last_sync_at: None,
                    sync_strategy: Some(format!("{devcontainer_sync_strategy}:failed-retained")),
                    start_mode: request.mode,
                    prompt_seed: request.prompt_seed.clone(),
                    module_outcomes: module_outcome_records,
                    last_summary_rendered_at: None,
                    adapter: adapter_summary.clone(),
                    runtime: runtime_metadata.clone(),
                    setup: None,
                })?;
                return Err(Error::validation(format!(
                    "{err}. Retained SBX runtime '{}'. Inspect it with `sbx exec {} bash`; retry with `branchbox feature start {} --runtime sbx --reuse-runtime`, or clean it with `branchbox feature teardown {} --force`.",
                    runtime_metadata.runtime_id.as_deref().unwrap_or("unknown"),
                    runtime_metadata.runtime_id.as_deref().unwrap_or("unknown"),
                    work_feature,
                    work_feature
                )));
            }
            match runtime_provider.destroy_worktree(
                &runtime_metadata,
                &self.runtime_ownership_path(runtime_kind, &worktree_path),
            ) {
                Ok(report) if !report.residue_free => tracing::warn!(
                    "Runtime cleanup after startup failure left residue: {:?}",
                    report.residue
                ),
                Ok(_) => {
                    // The runtime is gone, so the write-ahead entry must stop naming it; a later
                    // teardown would otherwise try to remove it again. On residue or an error the
                    // identity stays recorded so that teardown retries the removal.
                    if runtime_kind != RuntimeProviderKind::InGuest
                        && runtime_metadata.runtime_id.is_some()
                    {
                        self.record_provisional_runtime(
                            provisional.as_ref(),
                            &RuntimeMetadata {
                                provider: runtime_kind,
                                ..RuntimeMetadata::default()
                            },
                        );
                    }
                }
                Err(cleanup_err) => tracing::warn!(
                    "Failed to clean up runtime after environment startup failure: {}",
                    cleanup_err
                ),
            }
            self.rollback_prepared_tunnel(tunnel_state.as_ref());
            if runtime_kind == RuntimeProviderKind::InGuest {
                self.cleanup_failed_in_guest_worktree(
                    &worktree_path,
                    &branch_name,
                    in_guest_plan.as_ref(),
                    provisional.as_ref(),
                );
            }
            return Err(err);
        }

        // Record tunnel provisioning as a module outcome so `feature list --json` can surface
        // consistent health signals (and the manual CLI harness can assert expected behavior).
        module_outcomes.retain(|outcome| !outcome.module.eq_ignore_ascii_case("tunnel"));
        let tunnel_outcome_status = match tunnel_state.as_ref() {
            Some(state) if state.status == FeatureTunnelStatus::Active => ModuleStatus::Success,
            _ => ModuleStatus::Skipped,
        };
        module_outcomes.push(ModuleOutcome {
            module: "tunnel".to_string(),
            status: tunnel_outcome_status,
            duration_ms: 0,
            notes: Vec::new(),
            forced: false,
        });

        let color = Some(generate_feature_color(&work_feature));
        let summary_color = color.clone();
        let last_commit = get_last_commit_sha(&self.repo_root, &branch_name);
        let summary_generated_at = Utc::now();
        let devcontainer_skipped = skip_records
            .iter()
            .any(|record| record.name.eq_ignore_ascii_case("devcontainer"));
        let module_outcome_records: Vec<ModuleOutcomeRecord> = module_outcomes
            .iter()
            .map(|outcome| ModuleOutcomeRecord {
                module: outcome.module.clone(),
                status: outcome.status,
                duration_ms: outcome.duration_ms,
                notes: outcome.notes.clone(),
                forced: outcome.forced,
                recorded_at: Some(summary_generated_at),
            })
            .collect();

        if let Err(err) = self.state.record_start(FeatureMetadata {
            work_feature: work_feature.clone(),
            branch_name: branch_name.clone(),
            worktree_path: worktree_path.clone(),
            base_branch,
            feature_url: feature_url.clone(),
            compose_project_name: compose_project_name.clone(),
            env_path: env_path.clone(),
            tunnel: tunnel_state.clone(),
            status: FeatureStatus::Active,
            created_at: summary_generated_at,
            updated_at: summary_generated_at,
            removed_at: None,
            color,
            pr_number: None, // Can be populated later via gh CLI integration
            last_commit,
            devcontainer_outdated: devcontainer_skipped,
            last_sync_at: None,
            sync_strategy: reuse_existing.then(|| {
                format!(
                    "{devcontainer_sync_strategy}:reuse-{}",
                    request.devcontainer_reuse
                )
            }),
            start_mode: request.mode,
            prompt_seed: request.prompt_seed.clone(),
            module_outcomes: module_outcome_records.clone(),
            last_summary_rendered_at: Some(summary_generated_at),
            adapter: adapter_summary.clone(),
            runtime: runtime_metadata.clone(),
            setup: None,
        }) {
            tracing::warn!("Failed to update feature registry: {}", err);
            warnings.push("Failed to update feature registry metadata".to_string());
        }

        // Set up VS Code workspace customization for visual differentiation
        if let Err(err) = self.setup_vscode_workspace(
            &worktree_path,
            &work_feature,
            &summary_color,
            feature_url.as_deref(),
        ) {
            tracing::warn!("Failed to set up VS Code workspace customization: {}", err);
            warnings.push(format!("Failed to set up VS Code workspace: {}", err));
        }

        Ok(StartSummary {
            work_feature,
            branch_name,
            worktree_path,
            feature_url,
            compose_project_name,
            env_path,
            tunnel: tunnel_state,
            adapter: adapter_summary,
            module_reports,
            warnings,
            color: summary_color,
            mode: request.mode,
            prompt_seed: request.prompt_seed.clone(),
            runtime: runtime_metadata,
            module_outcomes,
            skipped_modules: skip_records,
            generated_at: summary_generated_at,
        })
    }

    /// Check that feature worktrees may be changed from here: BranchBox refuses to run inside a
    /// container unless `BRANCHBOX_SKIP_HOST_VALIDATION` is set.
    pub fn validate_host(&self) -> Result<()> {
        self.ensure_host_environment()
    }

    /// What tearing down `request.work_feature` would do, without changing anything
    /// (DESIGN §5.5): the worktree's user changes, the BranchBox-generated files and the spec
    /// teardown keeps, the branch step, and every blocker that makes teardown refuse. `options`
    /// are the ones the teardown would run with.
    ///
    /// A missing worktree is not an error here; the plan reports `worktree.exists: false`.
    pub fn plan_teardown(
        &self,
        request: &TeardownRequest,
        options: &TeardownOptions,
    ) -> Result<TeardownPlan> {
        if !naming::validate_work_feature(&request.work_feature) {
            return Err(Error::InvalidFeatureName(request.work_feature.clone()));
        }
        let recorded = self.state.get_feature(&request.work_feature)?;
        let worktree_path = self.worktree_path(&request.work_feature)?;
        let in_guest = self
            .teardown_runtime_metadata(recorded.as_ref(), &worktree_path)?
            .provider
            == RuntimeProviderKind::InGuest;
        require_forced_in_guest_teardown(in_guest, request)?;
        Ok(self
            .gather_teardown_plan(
                request,
                options,
                recorded.as_ref(),
                &worktree_path,
                in_guest,
            )
            .0)
    }

    /// The runtime a teardown of the feature at `worktree_path` stops: the registry's, or for a
    /// start that never finished (or a feature with no entry), the in-guest assignment recovered
    /// from the provider state.
    fn teardown_runtime_metadata(
        &self,
        recorded: Option<&FeatureMetadata>,
        worktree_path: &Path,
    ) -> Result<RuntimeMetadata> {
        Ok(match recorded {
            // A start that never finished (write-ahead entry) recorded the provider but not the
            // in-guest assignment identity; recover it from the provider state, as for a feature
            // with no entry at all.
            Some(metadata)
                if metadata.runtime.provider == RuntimeProviderKind::InGuest
                    && metadata.runtime.in_guest.is_none() =>
            {
                runtime::recover_in_guest_runtime_metadata(&self.repo_root, worktree_path)?
                    .unwrap_or_else(|| metadata.runtime.clone())
            }
            Some(metadata) => metadata.runtime.clone(),
            None => runtime::recover_in_guest_runtime_metadata(&self.repo_root, worktree_path)?
                .unwrap_or_default(),
        })
    }

    /// Tear down a feature worktree and optionally delete its branch, with the policy of
    /// callers that predate [`TeardownOptions`] (the agent): `--force` also discards user
    /// changes, and no unmerged-branch preflight runs. Without `--force` a worktree with user
    /// changes is refused, never deleted.
    pub fn teardown(&self, request: TeardownRequest) -> Result<TeardownSummary> {
        let options = TeardownOptions::legacy(&request);
        self.teardown_with_options(request, options)
    }

    /// Tear down a feature worktree and optionally delete its branch.
    ///
    /// The plan comes first: any blocker refuses with [`Error::TeardownRefused`] before the
    /// tunnel, modules, spec, adapter or runtime are touched. User changes are discarded only
    /// under `options.discard_changes` or `--force`; otherwise the worktree is checked again
    /// right before removal, and new user changes stop the teardown with the worktree and its
    /// registry entry kept. The worktree is removed with `git worktree remove --force` (what
    /// is left is BranchBox-generated or already moved out), or `--force --force` under
    /// `--force`, which alone may fall back to deleting the directory.
    pub fn teardown_with_options(
        &self,
        request: TeardownRequest,
        options: TeardownOptions,
    ) -> Result<TeardownSummary> {
        self.ensure_host_environment()?;

        if !naming::validate_work_feature(&request.work_feature) {
            return Err(Error::InvalidFeatureName(request.work_feature));
        }

        let worktree_path = self.worktree_path(&request.work_feature)?;
        let recorded_metadata = self.state.get_feature(&request.work_feature)?;
        let runtime_metadata =
            self.teardown_runtime_metadata(recorded_metadata.as_ref(), &worktree_path)?;
        let in_guest_teardown = runtime_metadata.provider == RuntimeProviderKind::InGuest;
        require_forced_in_guest_teardown(in_guest_teardown, &request)?;
        let worktree_exists = worktree_path.exists();
        if !worktree_exists && !request.force_remove {
            return Err(Error::WorktreeMissing {
                name: request.work_feature,
                path: worktree_path,
            });
        }

        // Repository lifecycle hooks can leave an in-guest worktree pointing at the container's
        // view of the shared Git metadata. Repair that pointer before status checks use it.
        // Forced teardown retains the existing best-effort filesystem fallback for irreparable
        // or already-partially-removed worktrees.
        if worktree_exists && in_guest_teardown && !request.force_remove {
            self.fix_git_worktree_path(&worktree_path).map_err(|err| {
                Error::validation(format!(
                    "Cannot restore in-guest Git worktree metadata before teardown: {err}"
                ))
            })?;
        }

        // Nothing has changed yet: a blocked plan refuses here.
        let (plan, branch) = self.gather_teardown_plan(
            &request,
            &options,
            recorded_metadata.as_ref(),
            &worktree_path,
            in_guest_teardown,
        );
        if plan.is_blocked() {
            return Err(plan.into_refusal());
        }

        let work_feature = request.work_feature.clone();
        let discard = discards_changes(&request, &options);

        if request.telemetry {
            std::env::set_var("BRANCHBOX_EMIT_TELEMETRY", "1");
        } else {
            std::env::remove_var("BRANCHBOX_EMIT_TELEMETRY");
        }

        let previous_complete = std::env::var("BRANCHBOX_COMPLETE_SPEC").ok();
        if request.complete_spec {
            std::env::set_var("BRANCHBOX_COMPLETE_SPEC", "1");
        } else {
            std::env::remove_var("BRANCHBOX_COMPLETE_SPEC");
        }

        let mut warnings = Vec::new();
        let mut completed_steps = Vec::new();
        let mut skip_modules: Vec<String> = Vec::new();

        if !in_guest_teardown && recorded_metadata.is_some() {
            match self.tunnel_remove(TunnelRemoveRequest {
                work_feature: work_feature.clone(),
                force: request.force_remove,
            }) {
                Ok(summary) => {
                    warnings.extend(summary.warnings);
                    skip_modules.push("tunnel".to_string());
                    completed_steps.push("Removed the feature tunnel".to_string());
                }
                Err(err) => warnings.push(format!("Automated tunnel teardown failed: {}", err)),
            }
        }

        let modules::ModulePlan {
            handles,
            warnings: dependency_warnings,
        } = if in_guest_teardown {
            modules::ModulePlan::new(Vec::new(), Vec::new())
        } else {
            modules::detect_modules(&self.repo_root, &skip_modules)
        };
        let (module_reports, module_warnings) = if worktree_exists {
            self.run_module_teardown(
                handles,
                &self.runtime_ownership_path(runtime_metadata.provider, &worktree_path),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        if !module_reports.is_empty() {
            let names: Vec<&str> = module_reports
                .iter()
                .map(|report| report.name.as_str())
                .collect();
            completed_steps.push(format!("Tore down modules ({})", names.join(", ")));
        }
        if !dependency_warnings.is_empty() {
            warnings.extend(dependency_warnings);
        }
        if !module_warnings.is_empty() {
            warnings.extend(module_warnings.clone());
        }
        if !worktree_exists {
            warnings.push(format!(
                "Worktree directory '{}' missing; skipping module teardown",
                worktree_path.display()
            ));
        }
        let spec_warnings_from = warnings.len();
        let preserved: Vec<PreservedFile> = if in_guest_teardown {
            Vec::new()
        } else {
            self.handle_spec_on_teardown(
                &work_feature,
                &branch.name,
                &worktree_path,
                request.complete_spec,
                &mut warnings,
            )
            .into_iter()
            .collect()
        };
        for file in &preserved {
            completed_steps.push(format!(
                "Moved {} to {} in the main worktree",
                file.path, file.destination
            ));
        }
        match previous_complete {
            Some(value) => std::env::set_var("BRANCHBOX_COMPLETE_SPEC", value),
            None => std::env::remove_var("BRANCHBOX_COMPLETE_SPEC"),
        }
        // The plan promised to keep the spec: a move that failed stops here, before the runtime
        // goes, even when user changes are discarded. Only --force removes it anyway.
        if worktree_exists && !request.force_remove {
            if let Some(unmoved) = plan.changes.preserved.iter().find(|file| {
                !preserved.iter().any(|moved| moved.path == file.path)
                    && worktree_path.join(&file.path).is_file()
            }) {
                let cause = warnings[spec_warnings_from..]
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "the move did not happen".to_string());
                let mut stopped = plan.clone();
                stopped.blockers = vec![Blocker::spec_not_preserved(
                    &worktree_path,
                    &unmoved.path,
                    cause,
                )];
                stopped.warnings.extend(warnings);
                return Err(stopped.into_stopped(completed_steps));
            }
        }

        let mut runtime_teardown =
            match runtime::provider(runtime_metadata.provider).and_then(|provider| {
                provider.destroy_worktree(
                    &runtime_metadata,
                    &self.runtime_ownership_path(runtime_metadata.provider, &worktree_path),
                )
            }) {
                Ok(report) => report,
                Err(err) => {
                    warnings.push(format!(
                        "Runtime '{}' teardown failed: {}",
                        runtime_metadata.provider, err
                    ));
                    runtime::RuntimeTeardownReport::unverified(
                        runtime_metadata.provider,
                        runtime_metadata.runtime_id.clone(),
                        err.to_string(),
                    )
                }
            };
        // Module failures also invalidate the runtime receipt: e.g. Compose volumes may remain even
        // after a successful empty devcontainer-container probe.
        for report in module_reports.iter().filter(|report| !report.teardown_ok) {
            runtime_teardown.verified = false;
            runtime_teardown.residue_free = false;
            runtime_teardown.residue.push(runtime::RuntimeResidue {
                kind: "module-teardown-error".to_string(),
                identifiers: vec![format!("{}: {}", report.name, report.errors.join("; "))],
            });
        }
        if runtime_teardown.verified && runtime_teardown.residue_free {
            completed_steps.push(format!("Stopped the {} runtime", runtime_metadata.provider));
        } else {
            let cause = runtime_teardown
                .residue
                .iter()
                .map(|item| format!("{}: {}", item.kind, item.identifiers.join(", ")))
                .collect::<Vec<_>>()
                .join("; ");
            warnings.push(format!(
                "Runtime cleanup was not verified residue-free: {cause}"
            ));
            // Bare worktrees with no devcontainer setup or recorded runtime keep the no-Docker lifecycle.
            // A synchronized config can also have been started manually or by the Mac app without an ID.
            let possible_runtime = runtime_metadata.provider != RuntimeProviderKind::Container
                || module_reports.iter().any(|report| !report.teardown_ok)
                || runtime_teardown.residue.iter().any(|item| {
                    matches!(
                        item.kind.as_str(),
                        "container"
                            | "container-cleanup-attempted"
                            | "compose-project"
                            | "compose-ownership-error"
                    )
                })
                || runtime_metadata.runtime_id.is_some()
                || runtime_metadata.container_id.is_some()
                || worktree_path
                    .join(".devcontainer/devcontainer.json")
                    .exists()
                || worktree_path
                    .join(".devcontainer/.devcontainer.json")
                    .exists()
                || worktree_path.join(".devcontainer.json").exists()
                || self
                    .repo_root
                    .join(".devcontainer/devcontainer.json")
                    .exists()
                || self
                    .repo_root
                    .join(".devcontainer/.devcontainer.json")
                    .exists()
                || self.repo_root.join(".devcontainer.json").exists();
            if possible_runtime && !request.force_remove {
                let mut stopped = plan.clone();
                stopped.blockers = vec![Blocker::runtime_cleanup_failed(&worktree_path, cause)];
                stopped.warnings.extend(warnings);
                return Err(stopped.into_stopped(completed_steps));
            }
        }

        let mut worktree_removed = false;
        if worktree_exists {
            #[cfg(test)]
            if let Some(hook) = self.before_worktree_removal {
                hook(&worktree_path);
            }

            if !discard {
                // A spec that was copied (not moved) out is still in the worktree and kept.
                let copied_spec = preserved
                    .iter()
                    .map(|file| file.path.as_str())
                    .find(|path| worktree_path.join(path).is_file());
                self.recheck_before_removal(
                    &request,
                    &options,
                    &plan,
                    &worktree_path,
                    copied_spec,
                    &completed_steps,
                )?;
            }
        }

        // The adapter cleanup deletes caches and build output (tmp/, build/, dist/…). It runs
        // only once the runtime is stopped and the worktree passed its last check, so a teardown
        // that stops keeps them, and nothing written meanwhile is deleted unseen.
        let (adapter_cleanup_warnings, adapter_detection_warning) = if in_guest_teardown {
            (Vec::new(), None)
        } else {
            self.cleanup_adapter(&worktree_path)
        };
        if !in_guest_teardown && worktree_exists {
            completed_steps.push("Ran the adapter cleanup".to_string());
        }
        if let Some(message) = adapter_detection_warning {
            warnings.push(message);
        }

        if worktree_exists {
            // Level 1 is safe without --force: everything left in the worktree was just
            // verified to be BranchBox-generated or moved out, or the caller discards it.
            let level = if request.force_remove {
                RemovalForce::IncludingLocked
            } else {
                RemovalForce::DiscardChanges
            };
            let removal = if in_guest_teardown {
                self.git.remove_consumer_writable(&worktree_path)
            } else {
                self.git.remove_worktree(&worktree_path, level)
            };
            match removal {
                Ok(_) => {
                    worktree_removed = true;
                }
                Err(err) if request.force_remove => {
                    warnings.push(format!("Failed to remove worktree: {}", err));
                    if worktree_path.exists() {
                        match fs::remove_dir_all(&worktree_path) {
                            Ok(_) => {
                                worktree_removed = true;
                                warnings.push(
                                    "Worktree directory removed manually after git removal failed"
                                        .to_string(),
                                );
                            }
                            Err(fs_err) => {
                                warnings.push(format!(
                                    "Failed to remove worktree directory manually: {}",
                                    fs_err
                                ));
                            }
                        }
                    }
                }
                Err(err) => {
                    let mut stopped = plan.clone();
                    stopped.blockers = vec![Blocker::worktree_removal_failed(
                        &worktree_path,
                        error_cause(&err),
                    )];
                    return Err(stopped.into_stopped(completed_steps));
                }
            }
        } else {
            warnings.push(format!(
                "Worktree directory '{}' already removed; skipping git worktree cleanup",
                worktree_path.display()
            ));
        }

        if worktree_removed || !worktree_exists {
            let pruning = if in_guest_teardown {
                self.git.prune_consumer_writable()
            } else {
                self.git.prune()
            };
            if let Err(err) = pruning {
                tracing::warn!("Failed to prune stale worktrees: {}", err);
                warnings.push(format!("Failed to prune stale git worktrees: {}", err));
            }
        }

        let worktree_gone = worktree_removed || !worktree_path.exists();
        if worktree_gone {
            if let Err(err) = DevcontainerModule::remove_baseline(&self.repo_root, &work_feature) {
                warnings.push(err.to_string());
            }
        }

        let branch_action = branch_action(&request);
        let (branch_deleted, branch_delete_error) = self.delete_feature_branch(
            &branch.name,
            branch_action,
            plan.branch.as_ref(),
            &request,
            in_guest_teardown,
            &mut warnings,
        );

        let registry_updated = if worktree_gone {
            match self.state.record_teardown(&work_feature) {
                Ok(updated) => updated,
                Err(err) => {
                    tracing::warn!("Failed to update feature registry: {}", err);
                    warnings.push("Failed to update feature registry metadata".to_string());
                    false
                }
            }
        } else {
            warnings.push(format!(
                "Kept the registry entry of '{}' because its worktree {} still exists",
                work_feature,
                worktree_path.display()
            ));
            false
        };

        if plan.changes.truncated && worktree_removed {
            warnings.push(format!(
                "Discarded more uncommitted changes than discarded_changes lists: only the first \
                 {MAX_CLASSIFIED_ENTRIES} changes were classified"
            ));
        }
        let discarded_changes = if worktree_removed {
            plan.changes.user.clone()
        } else {
            Vec::new()
        };

        Ok(TeardownSummary {
            work_feature,
            branch_name: branch.name,
            worktree_removed,
            branch_deleted,
            adapter_cleanup_warnings,
            module_reports,
            runtime_teardown,
            warnings,
            branch_action,
            branch_delete_error,
            discarded_changes,
            preserved,
            registry_updated,
        })
    }

    /// Gather what a teardown plan is decided from (the registry entry, the configuration,
    /// the worktree's lock and status, the branch's merge state) and build it. Read-only.
    /// Returns the branch teardown acts on alongside, which the plan omits when its merge
    /// state could not be read.
    fn gather_teardown_plan(
        &self,
        request: &TeardownRequest,
        options: &TeardownOptions,
        recorded: Option<&FeatureMetadata>,
        worktree_path: &Path,
        in_guest: bool,
    ) -> (TeardownPlan, ResolvedBranch) {
        let mut warnings = Vec::new();
        let config = BranchBoxConfig::load(&self.repo_root).unwrap_or_else(|err| {
            warnings.push(format!(
                "Using default teardown settings: cannot read .branchbox/config.json: {err}"
            ));
            BranchBoxConfig::default()
        });
        let branch = resolve_teardown_branch(
            request.branch_prefix.as_deref(),
            recorded,
            &config,
            &request.work_feature,
            &mut warnings,
        );

        let exists = worktree_path.exists();
        let mut not_a_worktree = None;
        let (locked, lock_reason) = if exists {
            match self.git.worktree_registration(worktree_path) {
                Ok(WorktreeRegistration::Linked { lock }) => {
                    (lock.is_some(), lock.and_then(|lock| lock.reason))
                }
                Ok(WorktreeRegistration::Main) => {
                    not_a_worktree = Some("it is the repository's main worktree".to_string());
                    (false, None)
                }
                Ok(WorktreeRegistration::NotListed) => {
                    // What a forced teardown whose removal failed halfway leaves behind: the
                    // registered feature's folder, without its `.git` link. `git status` fails
                    // there, so only --force removes it (status_unavailable).
                    let leftover = recorded.is_some()
                        && fs::symlink_metadata(worktree_path.join(".git")).is_err();
                    if !leftover && !self.links_into_repository(worktree_path) {
                        not_a_worktree = Some(format!(
                            "git does not list it as a worktree of {}",
                            self.repo_root.display()
                        ));
                    }
                    (false, None)
                }
                Err(err) => {
                    not_a_worktree = Some(format!(
                        "cannot list the worktrees of {}: {}",
                        self.repo_root.display(),
                        error_cause(&err)
                    ));
                    (false, None)
                }
            }
        } else {
            (false, None)
        };

        let status = if !exists {
            WorktreeStatus::Missing
        } else if in_guest {
            // The coding container can write this worktree, and `git status` may run a
            // repository-configured filter as the runtime; managed in-guest teardown requires
            // --force instead of inspecting it.
            WorktreeStatus::Unavailable(
                "not inspected: managed in-guest worktrees are never scanned with git status"
                    .to_string(),
            )
        } else {
            match self.git.status_entries(worktree_path) {
                Ok(entries) => {
                    let context =
                        self.change_context(worktree_path, &request.work_feature, &mut warnings);
                    // R2: the spec teardown moves out is the one handle_spec_on_teardown picks.
                    // An in-guest teardown moves none.
                    let kept_spec = if in_guest {
                        None
                    } else {
                        self.determine_feature_spec(worktree_path, &request.work_feature)
                            .ok()
                            .flatten()
                            .map(|(spec, _)| self.spec_display_path(&spec, worktree_path))
                    };
                    WorktreeStatus::Classified(classify_changes(
                        &entries,
                        &context,
                        &request.work_feature,
                        request.complete_spec,
                        kept_spec.as_deref(),
                    ))
                }
                Err(err) => WorktreeStatus::Unavailable(error_cause(&err)),
            }
        };

        let merge_state = match self.git.branch_merge_state(&branch.name) {
            Ok(state) => Some(state),
            Err(err) => {
                warnings.push(format!(
                    "Cannot tell whether branch '{}' is merged: {}",
                    branch.name,
                    error_cause(&err)
                ));
                None
            }
        };

        let plan = build_teardown_plan(PlanInputs {
            request,
            options: *options,
            recorded,
            worktree: WorktreeState {
                path: worktree_path.to_path_buf(),
                exists,
                locked,
                lock_reason,
            },
            status,
            branch: branch.clone(),
            merge_state,
            defaults: TeardownDefaults {
                delete_branch_by_default: config.feature.teardown.delete_branch_by_default,
                force_delete_unmerged_by_default: config
                    .feature
                    .teardown
                    .force_delete_unmerged_by_default,
            },
            warnings,
        });
        let mut plan = plan;
        if let Some(cause) = not_a_worktree {
            // First, so a client that reads the first blocker names this cause.
            plan.blockers
                .insert(0, Blocker::not_a_worktree(worktree_path, cause));
        }
        (plan, branch)
    }

    /// Whether `worktree_path/.git` is a gitdir link into this repository's `worktrees/`
    /// administrative directory, directly or through the container view
    /// `/workspaces/main/.git/worktrees` that in-guest hooks can leave behind. Such a directory
    /// is a worktree of this repository even when `git worktree list` shows another path for it.
    fn links_into_repository(&self, worktree_path: &Path) -> bool {
        let git_file = worktree_path.join(".git");
        if !fs::symlink_metadata(&git_file).is_ok_and(|meta| meta.is_file()) {
            return false;
        }
        let Ok(content) = fs::read_to_string(&git_file) else {
            return false;
        };
        let Some(target) = content
            .lines()
            .find_map(|line| line.strip_prefix("gitdir:"))
            .map(str::trim)
        else {
            return false;
        };
        let Ok(worktrees) = self
            .repository_worktrees_dir()
            .and_then(|dir| fs::canonicalize(dir).map_err(Error::from))
        else {
            return false;
        };
        let target = Path::new(target);
        let candidate = match target.strip_prefix("/workspaces/main/.git/worktrees") {
            Ok(relative) => worktrees.join(relative),
            Err(_) if target.is_absolute() => target.to_path_buf(),
            Err(_) => worktree_path.join(target),
        };
        fs::canonicalize(candidate)
            .is_ok_and(|resolved| resolved.starts_with(&worktrees) && resolved != worktrees)
    }

    /// The classifier's view of `worktree_path`, with the feature's devcontainer baseline.
    fn change_context(
        &self,
        worktree_path: &Path,
        work_feature: &str,
        warnings: &mut Vec<String>,
    ) -> FsChangeContext {
        let baseline = DevcontainerModule::read_baseline(&self.repo_root, work_feature)
            .unwrap_or_else(|err| {
                warnings.push(format!("Ignoring the devcontainer sync baseline: {err}"));
                None
            });
        FsChangeContext::new(worktree_path, &self.repo_root, baseline)
    }

    /// Check the worktree again right before removing it, when user changes are not to be
    /// discarded: the runtime and modules have been stopped meanwhile, so a change made since
    /// the plan must stop the teardown instead of being deleted. Deleting a tracked file loses
    /// nothing (its content is in `HEAD`), so that alone does not stop it; adapter cleanups
    /// remove such files.
    fn recheck_before_removal(
        &self,
        request: &TeardownRequest,
        options: &TeardownOptions,
        plan: &TeardownPlan,
        worktree_path: &Path,
        copied_spec: Option<&str>,
        completed_steps: &[String],
    ) -> Result<()> {
        let mut stopped = plan.clone();
        let blocker = match self.git.status_entries(worktree_path) {
            Ok(entries) => {
                let mut ignored = Vec::new();
                let context =
                    self.change_context(worktree_path, &request.work_feature, &mut ignored);
                // The spec was already moved out; only a copy left behind is still kept.
                let classification = classify_changes(
                    &entries,
                    &context,
                    &request.work_feature,
                    request.complete_spec,
                    copied_spec,
                );
                let blocking = blocking_changes(classification.content_changes(), request, options);
                let unlisted = classification.unlisted_user_changes;
                if blocking.is_empty() && unlisted == 0 {
                    return Ok(());
                }
                let blocker = Blocker::uncommitted_changes(worktree_path, &blocking, unlisted);
                stopped.changes = classification.changes;
                blocker
            }
            Err(err) => {
                stopped.changes.status_available = false;
                Blocker::status_unavailable(worktree_path, error_cause(&err))
            }
        };
        stopped.blockers = vec![blocker];
        Err(stopped.into_stopped(completed_steps.to_vec()))
    }

    /// The branch step. A missing branch is skipped with a warning; a failed delete is
    /// reported in the summary (`branch_delete_error`) and does not fail the teardown.
    fn delete_feature_branch(
        &self,
        branch: &str,
        action: BranchAction,
        planned: Option<&BranchPlan>,
        request: &TeardownRequest,
        in_guest: bool,
        warnings: &mut Vec<String>,
    ) -> (bool, Option<String>) {
        if action == BranchAction::Keep {
            return (false, None);
        }
        if matches!(self.git.local_branch_exists(branch), Ok(false)) {
            warnings.push(format!("Branch '{branch}' not found; nothing to delete"));
            return (false, None);
        }
        let force = action == BranchAction::ForceDelete;
        let deletion = if in_guest {
            self.git.delete_branch_consumer_writable(branch, force)
        } else {
            self.git.delete_branch(branch, force)
        };
        match deletion {
            Ok(_) => {
                // D-27: --force still means -D. Say what that cost when it deleted commits.
                if force && request.force_remove && !request.force_delete_branch {
                    if let Some(planned) =
                        planned.filter(|planned| planned.exists && !planned.merged)
                    {
                        warnings.push(format!(
                            "Force-deleted unmerged branch {branch} ({} {}); use \
                             --discard-changes to discard files without deleting unmerged commits",
                            planned.ahead,
                            if planned.ahead == 1 {
                                "commit"
                            } else {
                                "commits"
                            }
                        ));
                    }
                }
                (true, None)
            }
            Err(err) => {
                warnings.push(format!("Failed to delete branch '{}': {}", branch, err));
                (false, Some(error_cause(&err)))
            }
        }
    }

    /// List feature metadata from the registry, sorted by most recently updated.
    ///
    /// Statuses and setup states are reconciled with the machine, never persisted:
    /// - an unfinished start is reported as `setup.state: interrupted` once its process has
    ///   exited or it is more than 24 hours old;
    /// - an active or retained feature whose worktree directory is gone is `orphaned`;
    /// - otherwise the runtime provider decides between `orphaned` (runtime gone) and `degraded`
    ///   (environment not ready). An unfinished start skips that check: its runtime identity is
    ///   recorded only when the start completes.
    pub fn list_features(&self) -> Result<Vec<FeatureMetadata>> {
        let mut entries = self.state.list_features()?;
        let now = Utc::now();
        for entry in &mut entries {
            if let Some(setup) = entry.setup.as_mut() {
                setup.state = setup.observed_state(now);
            }
            if !matches!(
                entry.status,
                FeatureStatus::Active | FeatureStatus::FailedRetained
            ) {
                continue;
            }
            // Only a definite "not found" counts: an unreadable parent is not a missing worktree.
            if matches!(entry.worktree_path.try_exists(), Ok(false)) {
                entry.status = FeatureStatus::Orphaned;
                continue;
            }
            if entry.setup.is_some() {
                continue;
            }
            if let Ok(provider) = runtime::provider(entry.runtime.provider) {
                let exists = provider.exists(&entry.runtime);
                if exists.is_ok_and(|exists| !exists) {
                    entry.status = FeatureStatus::Orphaned;
                } else if entry.status == FeatureStatus::Active
                    && provider
                        .environment_ready(&entry.runtime, &entry.worktree_path)
                        .is_ok_and(|ready| !ready)
                {
                    entry.status = FeatureStatus::Degraded;
                }
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
        Ok(entries)
    }

    /// Execute a command through the runtime associated with an active feature.
    pub fn exec_runtime(
        &self,
        work_feature: &str,
        command: &[String],
    ) -> Result<runtime::RuntimeExecResult> {
        if command.is_empty() {
            return Err(Error::validation("Runtime command cannot be empty"));
        }
        let metadata = self
            .state
            .get_feature(work_feature)?
            .ok_or_else(|| Error::WorktreeNotFound(work_feature.to_string()))?;
        if metadata.status != FeatureStatus::Active {
            return Err(Error::validation(format!(
                "Feature '{work_feature}' is not active"
            )));
        }
        let provider = runtime::provider(metadata.runtime.provider)?;
        provider.exec(&metadata.runtime, &metadata.worktree_path, command)
    }

    /// Execute an interactive command through an active feature's runtime.
    pub fn exec_runtime_interactive(&self, work_feature: &str, command: &[String]) -> Result<i32> {
        if command.is_empty() {
            return Err(Error::validation("Runtime command cannot be empty"));
        }
        let metadata = self
            .state
            .get_feature(work_feature)?
            .ok_or_else(|| Error::WorktreeNotFound(work_feature.to_string()))?;
        if metadata.status != FeatureStatus::Active {
            return Err(Error::validation(format!(
                "Feature '{work_feature}' is not active"
            )));
        }
        let provider = runtime::provider(metadata.runtime.provider)?;
        provider.exec_interactive(&metadata.runtime, &metadata.worktree_path, command)
    }

    /// Execute the exact provider entrypoint and environment declared by the managed assignment.
    pub fn exec_provider_runtime_interactive(
        &self,
        work_feature: &str,
        coding_provider: &str,
        inherited_environment: &[String],
        args: &[String],
    ) -> Result<i32> {
        let metadata = self
            .state
            .get_feature(work_feature)?
            .ok_or_else(|| Error::WorktreeNotFound(work_feature.to_string()))?;
        if metadata.status != FeatureStatus::Active {
            return Err(Error::validation(format!(
                "Feature '{work_feature}' is not active"
            )));
        }
        let provider = runtime::provider(metadata.runtime.provider)?;
        provider.exec_provider_interactive(
            &metadata.runtime,
            coding_provider,
            inherited_environment,
            args,
        )
    }

    /// Dispatch one capability-bound consumer request to its managed trusted endpoint.
    pub fn dispatch_tool_request(
        &self,
        work_feature: &str,
        lease_id: &str,
        request_id: &str,
    ) -> Result<runtime::RuntimeToolDispatchResult> {
        let metadata = self
            .state
            .get_feature(work_feature)?
            .ok_or_else(|| Error::WorktreeNotFound(work_feature.to_string()))?;
        if metadata.status != FeatureStatus::Active {
            return Err(Error::validation(format!(
                "Feature '{work_feature}' is not active"
            )));
        }
        let provider = runtime::provider(metadata.runtime.provider)?;
        provider.dispatch_tool_request(&metadata.runtime, lease_id, request_id)
    }

    /// Open (provision) a tunnel for an existing feature.
    pub fn tunnel_open(&self, request: TunnelOpenRequest) -> Result<TunnelOpenSummary> {
        let mut warnings = Vec::new();
        let metadata = self
            .state
            .get_feature(&request.work_feature)?
            .ok_or_else(|| self.state.feature_not_found(&request.work_feature))?;

        if metadata.status == FeatureStatus::Removed {
            return Err(Error::validation(format!(
                "Feature '{}' has been removed. Recreate the feature before provisioning a tunnel.",
                request.work_feature
            )));
        }

        let mut config = BranchBoxConfig::load(&self.repo_root)?;
        config.tunnel.ensure_defaults();
        if !config.tunnel.enabled {
            return Err(Error::validation(
                "Tunnel provisioning is disabled in project configuration",
            ));
        }

        let configured_provider = config
            .tunnel
            .default_provider
            .clone()
            .unwrap_or_else(|| "cloudflared".to_string());
        let configured_provider = Self::normalize_tunnel_provider_name(&configured_provider)
            .unwrap_or(configured_provider.as_str())
            .to_string();

        let stored_provider = metadata
            .tunnel
            .as_ref()
            .map(|state| state.provider.trim().to_string())
            .filter(|provider| !provider.is_empty());

        let provider_name = stored_provider
            .as_deref()
            .and_then(Self::normalize_tunnel_provider_name)
            .map(str::to_string)
            .unwrap_or_else(|| configured_provider.clone());

        if let Some(stored) = stored_provider.as_ref() {
            if Self::normalize_tunnel_provider_name(stored).is_none() {
                warnings.push(format!(
                    "Stored tunnel provider '{}' is unsupported; using configured provider '{}'",
                    stored, configured_provider
                ));
            }
        }

        let hostname = self.resolve_tunnel_hostname(&metadata)?;
        let service_url = self.resolve_service_url();

        let (mut state, mut provider_warnings, provision_token) = self.invoke_tunnel_provider(
            &provider_name,
            &metadata.work_feature,
            &hostname,
            &service_url,
            &config,
        )?;
        warnings.append(&mut provider_warnings);

        if state.hostname.is_none() {
            state.hostname = Some(hostname.clone());
        }

        if state.descriptor.is_some() {
            match self.persist_tunnel_credentials(
                &metadata.worktree_path,
                &hostname,
                provision_token.as_deref(),
                state
                    .descriptor
                    .as_ref()
                    .and_then(|stored| stored.token_path.as_ref()),
            ) {
                Ok(_) => {
                    state.status = FeatureTunnelStatus::Active;
                }
                Err(err) => warnings.push(format!("Failed to write tunnel credentials: {}", err)),
            }
        }

        let cloned_state = state.clone();
        let updated = self
            .state
            .update_feature(&metadata.work_feature, |feature| {
                feature.tunnel = Some(cloned_state.clone());
                feature.updated_at = cloned_state.last_updated;
            })?;

        Ok(TunnelOpenSummary {
            work_feature: updated.work_feature,
            state,
            warnings,
        })
    }

    /// Remove tunnel metadata (and attempt teardown) for a feature.
    pub fn tunnel_remove(&self, request: TunnelRemoveRequest) -> Result<TunnelRemoveSummary> {
        let mut warnings = Vec::new();
        let metadata = self
            .state
            .get_feature(&request.work_feature)?
            .ok_or_else(|| self.state.feature_not_found(&request.work_feature))?;

        let previous_state = metadata.tunnel.clone();
        if previous_state.is_none() {
            return Err(Error::validation(format!(
                "Feature '{}' has no tunnel metadata recorded",
                request.work_feature
            )));
        }

        let mut config = BranchBoxConfig::load(&self.repo_root)?;
        config.tunnel.ensure_defaults();

        if let Some(ref tunnel_state) = previous_state {
            if let Some(ref descriptor) = tunnel_state.descriptor {
                let runtime_descriptor =
                    self.stored_descriptor_to_runtime(tunnel_state, descriptor);
                if let Err(err) = self.invoke_tunnel_teardown(
                    &tunnel_state.provider,
                    &runtime_descriptor,
                    &config,
                ) {
                    if request.force {
                        warnings.push(format!(
                            "Failed to tear down tunnel via provider '{}': {}",
                            tunnel_state.provider, err
                        ));
                    } else {
                        return Err(err);
                    }
                }

                if let Some(path) = descriptor.token_path.as_ref() {
                    if let Err(err) = fs::remove_file(path) {
                        warnings.push(format!(
                            "Failed to remove stored tunnel token {}: {}",
                            path.display(),
                            err
                        ));
                    }
                }
            } else {
                warnings.push("Tunnel descriptor missing; skipping provider teardown".to_string());
            }
        }

        let env_file = metadata
            .worktree_path
            .join(".devcontainer")
            .join(".cloudflared.env");
        if env_file.exists() {
            if let Err(err) = fs::remove_file(&env_file) {
                warnings.push(format!(
                    "Failed to remove tunnel environment file {}: {}",
                    env_file.display(),
                    err
                ));
            }
        }

        let now = Utc::now();
        let mut updated_state = previous_state.clone().unwrap();
        updated_state.status = FeatureTunnelStatus::Disabled;
        updated_state.instructions = None;
        updated_state.descriptor = None;
        updated_state.notes = Some("Tunnel removed via CLI".to_string());
        updated_state.last_updated = now;
        updated_state.removed_at = Some(now);

        let updated = self
            .state
            .update_feature(&metadata.work_feature, |feature| {
                feature.tunnel = Some(updated_state.clone());
                feature.updated_at = now;
            })?;

        Ok(TunnelRemoveSummary {
            work_feature: updated.work_feature,
            previous_state,
            updated_state: updated.tunnel.clone(),
            warnings,
        })
    }

    /// Record the outcome of a devcontainer sync attempt for a feature worktree.
    pub fn record_devcontainer_sync(
        &self,
        work_feature: &str,
        strategy: Option<&str>,
        success: bool,
    ) -> Result<()> {
        self.state
            .record_devcontainer_sync(work_feature, strategy, success)
    }

    fn resolve_work_feature(&self, request: &StartRequest) -> Result<String> {
        if let Some(name) = request.name.as_ref() {
            // If already valid, use it directly
            if naming::validate_work_feature(name) {
                return Ok(name.clone());
            }
            // Otherwise, treat the input as a human-readable title and auto-generate
            let generated = naming::generate_work_feature(name);
            if generated.is_empty() {
                return Err(Error::InvalidFeatureName(name.clone()));
            }
            return Ok(generated);
        }

        if let Some(title) = request.title.as_ref() {
            let generated = naming::generate_work_feature(title);
            if generated.is_empty() {
                return Err(Error::validation(
                    "Generated feature name is empty".to_string(),
                ));
            }
            return Ok(generated);
        }

        Err(Error::validation(
            "Feature name or title is required for feature start".to_string(),
        ))
    }

    fn worktree_path(&self, work_feature: &str) -> Result<PathBuf> {
        let parent = self.repo_root.parent().ok_or_else(|| {
            Error::validation("Repository root has no parent directory".to_string())
        })?;
        Ok(parent.join(work_feature))
    }

    fn runtime_ownership_path(
        &self,
        provider: RuntimeProviderKind,
        worktree_path: &Path,
    ) -> PathBuf {
        if provider == RuntimeProviderKind::Container {
            if let (Some(parent), Some(name)) = (
                self.repo_root_alias.as_ref().and_then(|root| root.parent()),
                worktree_path.file_name(),
            ) {
                // A symlink to the repository alone does not alias its sibling worktrees. Only
                // a verified alias of the worktree's parent can authorize the lexical label.
                if matches!((parent.canonicalize().ok(), worktree_path.parent().and_then(|path| path.canonicalize().ok())),
                    (Some(alias), Some(actual)) if alias == actual)
                {
                    return parent.join(name);
                }
            }
        }
        worktree_path.to_path_buf()
    }

    fn main_worktree_name(&self) -> String {
        self.repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "main".to_string())
    }

    fn resolve_app_slug(&self, env_path: &Path) -> Result<String> {
        if let Ok(vars) = validation::parse_env_file(env_path) {
            if let Some(raw) = vars
                .get("APP_NAME")
                .or_else(|| vars.get("APP_SLUG"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                let slug = naming::generate_work_feature(raw);
                if !slug.is_empty() {
                    return Ok(slug);
                }
            }
        }

        let parent = self.repo_root.parent().ok_or_else(|| {
            Error::validation("Repository root has no parent directory".to_string())
        })?;

        let app_name = parent
            .file_name()
            .ok_or_else(|| Error::validation("Repository parent has no name".to_string()))?
            .to_string_lossy()
            .to_string();

        let slug = naming::generate_work_feature(&app_name);
        if slug.is_empty() {
            return Err(Error::validation(
                "Unable to derive application name for compose project".to_string(),
            ));
        }

        Ok(slug)
    }

    /// Resolve the service URL for tunnel ingress.
    ///
    /// Priority: cloudflared config > adapter detection > default
    fn resolve_tunnel_service_url(&self, adapter_service_url: Option<&str>) -> String {
        // Try cloudflared config first (centralized source of truth)
        if let Ok(config) = BranchBoxConfig::load(&self.repo_root) {
            if let Some(cloudflared) = &config.tunnel.providers.cloudflared {
                if let Some(url) = &cloudflared.service_url {
                    if !url.is_empty() {
                        tracing::info!("Using service URL from cloudflared config: {}", url);
                        return url.clone();
                    }
                }
            }
        }

        // Fall back to adapter detection
        if let Some(url) = adapter_service_url {
            tracing::info!("Using service URL from adapter: {}", url);
            return url.to_string();
        }

        // Default fallback
        "web:3000".to_string()
    }

    /// Derive the feature URL from cloudflared config (preferred) or APP_URL (fallback).
    ///
    /// When cloudflared is configured with `dns_zone` and `tunnel_name_prefix`, the feature
    /// URL is derived as `{tunnel_name_prefix}-{work_feature}.{dns_zone}`.
    ///
    /// Falls back to parsing APP_URL from .env and using `naming::generate_feature_url()`.
    fn derive_feature_url(&self, source_env: &Path, work_feature: &str) -> Option<String> {
        // Try cloudflared config first (centralized source of truth)
        if let Ok(config) = BranchBoxConfig::load(&self.repo_root) {
            if let Some(cloudflared) = &config.tunnel.providers.cloudflared {
                if let (Some(prefix), Some(zone)) =
                    (&cloudflared.tunnel_name_prefix, &cloudflared.dns_zone)
                {
                    if !prefix.is_empty() && !zone.is_empty() {
                        let url = format!("{}-{}.{}", prefix, work_feature, zone);
                        tracing::info!("Derived feature URL from cloudflared config: {}", url);
                        return Some(url);
                    }
                }
            }
        }

        // Fallback to APP_URL from .env
        match AppUrl::from_env_file(source_env) {
            Ok(app_url) => {
                let url = naming::generate_feature_url(&app_url.url, work_feature);
                tracing::info!("Derived feature URL from APP_URL: {}", url);
                Some(url)
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to derive feature URL from {}: {}",
                    source_env.display(),
                    err
                );
                None
            }
        }
    }

    fn prepare_env(
        &self,
        worktree_path: &Path,
        work_feature: &str,
        branch_name: &str,
        reuse_existing: bool,
        warnings: &mut Vec<String>,
    ) -> Result<EnvOutcome> {
        let source_env = self.repo_root.join(".env");
        let dest_env = worktree_path.join(".env");
        let source_env_exists = source_env.exists();

        let mut feature_url = None;
        let app_slug = self.resolve_app_slug(&source_env)?;
        let raw_compose_name = format!("{}-{}", app_slug, work_feature);
        let compose_name = sanitize_compose_project_name(&raw_compose_name);
        std::env::set_var("BASE_PREFIX", &app_slug);
        std::env::set_var("COMPOSE_PROJECT_NAME", &compose_name);
        std::env::set_var("DEVCONTAINER_NAME", &compose_name);

        if source_env_exists {
            let (base_env, _previous_section) = split_feature_section(&source_env)?;

            if reuse_existing && dest_env.exists() {
                if let Ok((_, existing_section)) = split_feature_section(&dest_env) {
                    if let Some(section) = existing_section {
                        let trimmed = section.trim();
                        if !trimmed.is_empty() {
                            warnings.push(format!(
                                "Existing feature-specific configuration replaced in {}",
                                dest_env.display()
                            ));
                        }
                    } else {
                        warnings.push(format!(
                            "Existing {} will be refreshed from main .env (no branchbox block found)",
                            dest_env.display()
                        ));
                    }
                }
            }

            write_secure_file(&dest_env, &base_env)?;

            // Try to derive feature URL from cloudflared config first (centralized source of truth)
            // Falls back to APP_URL if cloudflared dns_zone is not configured
            let url = self.derive_feature_url(&source_env, work_feature);
            let mut file = open_append_secure(&dest_env)?;
            ensure_trailing_newline(&mut file)?;
            writeln!(
                file,
                "# Feature-specific configuration (managed by branchbox)"
            )?;
            writeln!(
                file,
                "WORK_FEATURE={}",
                sanitize_identifier_env_value(work_feature)
            )?;
            if let Some(url) = url {
                let sanitized_url = sanitize_url_env_value(&url);
                writeln!(file, "APP_URL={}", quote_env_value(&sanitized_url))?;
                feature_url = Some(sanitized_url);
            }
            writeln!(file, "COMPOSE_PROJECT_NAME={}", compose_name)?;
            writeln!(file, "DEVCONTAINER_NAME={}", compose_name)?;
            writeln!(
                file,
                "GIT_BRANCH={}",
                sanitize_git_branch_env_value(branch_name)
            )?;

            self.link_env_into_devcontainer(worktree_path)?;
        } else {
            tracing::warn!("No .env found at {}", source_env.display());
        }

        let main_name = self.main_worktree_name();
        self.write_branchbox_env(
            worktree_path,
            work_feature,
            branch_name,
            feature_url.as_deref(),
            Some(&compose_name),
            Some(&compose_name),
            &main_name,
        )?;

        Ok(EnvOutcome {
            env_path: source_env_exists.then_some(dest_env),
            feature_url,
            compose_project_name: Some(compose_name),
            skipped: !source_env_exists,
        })
    }

    fn prepare_adapter(
        &self,
        worktree_path: &Path,
        copy_secrets: bool,
    ) -> std::result::Result<AdapterSummary, String> {
        match adapters::detect_adapter(&self.repo_root) {
            Ok(adapter) => {
                let name = adapter.name().to_string();
                tracing::info!("Detected adapter: {}", name);
                let service_url = adapter.service_url();
                let mut summary = AdapterSummary {
                    name,
                    service_url,
                    warnings: Vec::new(),
                };

                if copy_secrets {
                    if let Err(err) = adapter.copy_secrets(&self.repo_root, worktree_path) {
                        summary
                            .warnings
                            .push(format!("Failed to copy secrets: {}", err));
                    }
                }

                Ok(summary)
            }
            Err(err) => Err(format!("Failed to detect adapter: {}", err)),
        }
    }

    fn cleanup_adapter(&self, worktree_path: &Path) -> (Vec<String>, Option<String>) {
        if !worktree_path.exists() {
            return (Vec::new(), None);
        }

        match adapters::detect_adapter(worktree_path) {
            Ok(adapter) => match adapter.cleanup(worktree_path) {
                Ok(_) => (Vec::new(), None),
                Err(err) => (vec![format!("Adapter cleanup failed: {}", err)], None),
            },
            Err(err) => (
                Vec::new(),
                Some(format!("Failed to detect adapter for cleanup: {}", err)),
            ),
        }
    }

    /// Record that this process is starting the feature (the write-ahead entry). A registry that
    /// cannot be updated costs only that guarantee, so the start goes on with a warning and the
    /// final registry update tries again.
    fn record_provisional_start(
        &self,
        metadata: FeatureMetadata,
        warnings: &mut Vec<String>,
    ) -> Option<ProvisionalStart> {
        let work_feature = metadata.work_feature.clone();
        let pid = metadata
            .setup
            .as_ref()
            .map_or_else(std::process::id, |setup| setup.pid);
        match self.state.record_setup_started(metadata) {
            Ok(previous) => Some(ProvisionalStart {
                work_feature,
                pid,
                previous,
            }),
            Err(err) => {
                tracing::warn!(
                    "Failed to record the in-progress start in the feature registry: {err}"
                );
                warnings.push(format!(
                    "Failed to record the in-progress start in the feature registry: {err}"
                ));
                None
            }
        }
    }

    /// Record the runtime a start is setting up in its write-ahead entry. Like the entry itself
    /// this is best effort: a registry that cannot be updated costs only the guarantee that an
    /// interrupted start's runtime can be found again, so it only warns.
    fn record_provisional_runtime(
        &self,
        provisional: Option<&ProvisionalStart>,
        runtime: &RuntimeMetadata,
    ) {
        let Some(provisional) = provisional else {
            return;
        };
        if let Err(err) =
            self.state
                .record_setup_runtime(&provisional.work_feature, provisional.pid, runtime)
        {
            tracing::warn!(
                "Failed to record the runtime of the in-progress start of '{}' in the feature registry: {}",
                provisional.work_feature,
                err
            );
        }
    }

    /// Take back the write-ahead entry of a start that removed its worktree again, restoring
    /// whatever the registry held for the feature before.
    fn discard_provisional_start(&self, provisional: Option<&ProvisionalStart>) {
        let Some(provisional) = provisional else {
            return;
        };
        if let Err(err) = self.state.discard_setup(
            &provisional.work_feature,
            provisional.pid,
            provisional.previous.clone(),
        ) {
            tracing::warn!(
                "Failed to remove the in-progress start of '{}' from the feature registry: {}",
                provisional.work_feature,
                err
            );
        }
    }

    fn cleanup_failed_in_guest_worktree(
        &self,
        worktree_path: &Path,
        branch_name: &str,
        plan: Option<&InGuestFacadePlan>,
        provisional: Option<&ProvisionalStart>,
    ) {
        if let Some(plan) = plan {
            if let Err(err) = plan.remove_private_compose_stage(worktree_path) {
                tracing::warn!(
                    "Failed to remove private Compose inputs after startup failure: {err}"
                );
            }
        }
        // A repository workspace is the repository. Removing it is never the
        // right cleanup for a failed start, and the branch is left in place for
        // the same reason a failed run leaves its clone: the caller owns it.
        if worktree_path == self.repo_root {
            tracing::info!("Repository workspace retained after startup failure");
            return;
        }
        if worktree_path.exists() {
            if let Err(err) = self.fix_git_worktree_path(worktree_path) {
                tracing::warn!(
                    "Failed to restore in-guest Git metadata before startup cleanup: {}",
                    err
                );
            }
            if let Err(err) = self.git.remove_consumer_writable(worktree_path) {
                tracing::warn!(
                    "Failed to remove in-guest worktree after startup failure: {}",
                    err
                );
                if let Err(fs_err) = fs::remove_dir_all(worktree_path) {
                    tracing::warn!(
                        "Failed to remove in-guest worktree directory after startup failure: {}",
                        fs_err
                    );
                }
            }
        }
        if let Err(err) = self.git.prune_consumer_writable() {
            tracing::warn!(
                "Failed to prune in-guest worktree metadata after startup failure: {}",
                err
            );
        }
        if self
            .git
            .branch_exists_consumer_writable(branch_name)
            .unwrap_or(false)
        {
            if let Err(err) = self.git.delete_branch_consumer_writable(branch_name, true) {
                tracing::warn!(
                    "Failed to delete in-guest task branch '{}' after startup failure: {}",
                    branch_name,
                    err
                );
            }
        }
        // A worktree that could not be removed keeps its entry, which then lists as interrupted.
        if !worktree_path.exists() {
            self.discard_provisional_start(provisional);
        }
    }

    fn capture_stash(&self, work_feature: &str) -> Result<StashState> {
        let stash_before = self.current_stash_oid()?;

        let status = Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=no"])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|err| Error::git(format!("Failed to check git status: {}", err)))?;

        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            return Err(Error::git(format!(
                "git status exited with error: {}",
                stderr.trim()
            )));
        }

        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(StashState::default());
        }

        let message = format!("feature-start: changes for {}", work_feature);
        let mut push_cmd = Command::new("git");
        push_cmd
            .arg("stash")
            .arg("push")
            .arg("--no-include-untracked")
            .arg("-m")
            .arg(&message)
            .arg("--")
            .arg(".");
        for pathspec in Self::stash_excluded_pathspecs() {
            push_cmd.arg(pathspec);
        }
        let push = push_cmd
            .current_dir(&self.repo_root)
            .output()
            .map_err(|err| Error::git(format!("Failed to invoke git stash: {}", err)))?;

        if !push.status.success() {
            let stderr = String::from_utf8_lossy(&push.stderr);
            return Err(Error::git(format!(
                "git stash push failed: {}",
                stderr.trim()
            )));
        }

        let stash_after = self.current_stash_oid()?;
        if stash_after.is_none() || stash_after == stash_before {
            return Ok(StashState::default());
        }

        let list = Command::new("git")
            .args(["stash", "list", "-n", "1", "--format=%gd"])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|err| Error::git(format!("Failed to list stashes: {}", err)))?;

        if !list.status.success() {
            let stderr = String::from_utf8_lossy(&list.stderr);
            return Err(Error::git(format!(
                "git stash list failed: {}",
                stderr.trim()
            )));
        }

        let reference = String::from_utf8_lossy(&list.stdout)
            .lines()
            .next()
            .map(|line| line.trim().to_string())
            .or_else(|| Some("stash@{0}".to_string()));

        Ok(StashState {
            created: true,
            reference,
        })
    }

    fn current_stash_oid(&self) -> Result<Option<String>> {
        let output = Command::new("git")
            .args(["rev-parse", "-q", "--verify", "refs/stash"])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|err| Error::git(format!("Failed to read refs/stash: {}", err)))?;

        if output.status.success() {
            let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if oid.is_empty() {
                Ok(None)
            } else {
                Ok(Some(oid))
            }
        } else if output.stderr.is_empty() {
            Ok(None)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::git(format!(
                "git rev-parse refs/stash failed: {}",
                stderr.trim()
            )))
        }
    }

    fn apply_stash_to_worktree(&self, stash: &StashState, worktree_path: &Path) -> Vec<String> {
        if !stash.created {
            return Vec::new();
        }

        let Some(reference) = stash.reference.as_deref() else {
            return vec![
                "Failed to apply stashed changes to feature worktree: stash reference unavailable. Stash remains available."
                    .to_string(),
            ];
        };

        match Command::new("git")
            .args(["stash", "pop", reference])
            .current_dir(worktree_path)
            .output()
        {
            Ok(output) if output.status.success() => Vec::new(),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let mut warning = format!(
                    "Failed to apply stashed changes to feature worktree: {}",
                    stderr.trim()
                );
                warning.push_str(&format!(" Stash {} remains available.", reference));
                vec![warning]
            }
            Err(err) => {
                let mut warning = format!(
                    "Failed to apply stashed changes to feature worktree: {}",
                    err
                );
                warning.push_str(&format!(" Stash {} remains available.", reference));
                vec![warning]
            }
        }
    }

    fn stash_excluded_pathspecs() -> [&'static str; 2] {
        [
            ":(exclude).branchbox/config.json",
            ":(exclude).branchbox/registry.json",
        ]
    }

    fn link_env_into_devcontainer(&self, worktree_path: &Path) -> Result<()> {
        let dev_dir = worktree_path.join(".devcontainer");
        if !dev_dir.exists() {
            fs::create_dir_all(&dev_dir)?;
        }

        let link_path = dev_dir.join(".env");
        if link_path.exists() {
            fs::remove_file(&link_path)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            if let Err(err) = symlink("../.env", &link_path) {
                tracing::warn!("Failed to create symlink for devcontainer .env: {}", err);
                fs::copy(worktree_path.join(".env"), &link_path)?;
            }
        }

        #[cfg(windows)]
        {
            if let Err(err) = std::os::windows::fs::symlink_file("..\\.env", &link_path) {
                tracing::warn!("Failed to create symlink for devcontainer .env: {}", err);
                fs::copy(worktree_path.join(".env"), &link_path)?;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn write_branchbox_env(
        &self,
        worktree_path: &Path,
        work_feature: &str,
        branch_name: &str,
        feature_url: Option<&str>,
        compose_project_name: Option<&str>,
        devcontainer_name: Option<&str>,
        main_name: &str,
    ) -> Result<()> {
        let dev_dir = worktree_path.join(".devcontainer");
        if !dev_dir.exists() {
            fs::create_dir_all(&dev_dir)?;
        }

        let managed_env = dev_dir.join(".branchbox.env");
        let retained_projects = modules::compose::retained_teardown_projects(worktree_path)?;
        let mut file = Vec::new();
        writeln!(
            file,
            "# BranchBox-managed overrides (auto-generated, do not edit)"
        )?;
        writeln!(
            file,
            "WORK_FEATURE={}",
            sanitize_identifier_env_value(work_feature)
        )?;
        writeln!(
            file,
            "BRANCHBOX_MAIN_NAME={}",
            sanitize_identifier_env_value(main_name)
        )?;
        writeln!(
            file,
            "GIT_BRANCH={}",
            sanitize_git_branch_env_value(branch_name)
        )?;

        if let Some(url) = feature_url {
            let sanitized_url = sanitize_url_env_value(url);
            writeln!(file, "APP_URL={}", quote_env_value(&sanitized_url))?;
        }
        if let Some(compose) = compose_project_name {
            writeln!(
                file,
                "COMPOSE_PROJECT_NAME={}",
                sanitize_compose_project_name(compose)
            )?;
        }
        if let Some(devcontainer) = devcontainer_name {
            writeln!(
                file,
                "DEVCONTAINER_NAME={}",
                sanitize_identifier_env_value(devcontainer)
            )?;
        }
        file.extend_from_slice(
            modules::compose::teardown_identity_lines(worktree_path, &retained_projects)?
                .as_bytes(),
        );
        atomic_fs::write_atomic(&managed_env, &file, 0o600)
    }

    fn run_module_setup(
        &self,
        handles: Vec<ModuleHandle>,
        feature_dir: &Path,
        forced_modules: &HashSet<String>,
    ) -> ModuleSetupOutcome {
        let mut reports = Vec::with_capacity(handles.len());
        let mut warnings = Vec::new();
        let mut executions = Vec::with_capacity(handles.len());
        let mut successful: Vec<ModuleHandle> = Vec::new();
        let mut aborted = false;

        for mut handle in handles.into_iter() {
            let normalized_name = handle.name.to_ascii_lowercase();
            let forced = forced_modules.contains(&normalized_name);
            let mut report = ModuleSetupReport {
                name: handle.name.clone(),
                init_ok: false,
                setup_ok: false,
                errors: Vec::new(),
            };
            let mut outcome = ModuleOutcome {
                module: handle.name.clone(),
                status: ModuleStatus::Success,
                duration_ms: 0,
                notes: Vec::new(),
                forced,
            };

            if forced {
                outcome
                    .notes
                    .push("Policy enforced module executed".to_string());
            }

            let start_instant = Instant::now();

            match handle.module.init(&self.repo_root, feature_dir) {
                Ok(_) => {
                    report.init_ok = true;
                    match handle.module.setup(&self.repo_root, feature_dir) {
                        Ok(_) => {
                            report.setup_ok = true;
                            successful.push(handle);
                        }
                        Err(err) => {
                            let message = err.to_string();
                            report.errors.push(message.clone());
                            outcome.status = ModuleStatus::Failed;
                            outcome.notes.push(message);
                            // Attempt best-effort cleanup for the module that failed during setup.
                            if let Err(teardown_err) =
                                handle.module.teardown(&self.repo_root, feature_dir)
                            {
                                warnings.push(format!(
                                    "Rollback for module '{}' failed: {}",
                                    report.name, teardown_err
                                ));
                            }
                            aborted = true;
                            reports.push(report);
                            outcome.duration_ms =
                                start_instant.elapsed().as_millis().min(u64::MAX as u128) as u64;
                            executions.push(outcome);
                            break;
                        }
                    }
                }
                Err(err) => {
                    let message = err.to_string();
                    report.errors.push(message.clone());
                    outcome.status = ModuleStatus::Failed;
                    outcome.notes.push(message);
                    aborted = true;
                    reports.push(report);
                    outcome.duration_ms =
                        start_instant.elapsed().as_millis().min(u64::MAX as u128) as u64;
                    executions.push(outcome);
                    break;
                }
            }

            reports.push(report);
            if !aborted {
                outcome.duration_ms =
                    start_instant.elapsed().as_millis().min(u64::MAX as u128) as u64;
                executions.push(outcome);
            }
        }

        if aborted {
            while let Some(handle) = successful.pop() {
                if let Err(err) = handle.module.teardown(&self.repo_root, feature_dir) {
                    warnings.push(format!(
                        "Rollback for module '{}' failed: {}",
                        handle.name, err
                    ));
                }
            }
            if !warnings.iter().any(|w| w.contains("Module setup aborted")) {
                warnings.push("Module setup aborted due to earlier failures".to_string());
            }
        }

        ModuleSetupOutcome {
            reports,
            warnings,
            executions,
        }
    }

    fn prepare_tunnel_state(
        &self,
        skip_tunnel: bool,
        work_feature: &str,
        feature_dir: &Path,
        hostname: Option<&str>,
        service_url: &str,
    ) -> Result<(Option<FeatureTunnelState>, Vec<String>)> {
        let mut warnings = Vec::new();

        let service_url = if service_url.trim().is_empty() {
            "web:3000"
        } else {
            service_url
        };

        if skip_tunnel {
            warnings.push("Tunnel provisioning skipped (--skip-module tunnel)".to_string());
            let state =
                FeatureTunnelState::disabled(None, "Tunnel module skipped for this feature");
            return Ok((Some(state), warnings));
        }

        let config = match BranchBoxConfig::load(&self.repo_root) {
            Ok(config) => config,
            Err(err) => {
                warnings.push(format!("Failed to load tunnel configuration: {}", err));
                return Ok((None, warnings));
            }
        };

        if !config.tunnel.enabled {
            let state = FeatureTunnelState::disabled(
                config.tunnel.default_provider.clone(),
                "Tunnel provisioning disabled in project configuration",
            );
            return Ok((Some(state), warnings));
        }

        let provider_name = config
            .tunnel
            .default_provider
            .clone()
            .unwrap_or_else(|| "cloudflared".to_string());

        // Check if cloudflared is configured (via env var or config file) before checking hostname
        let has_env_token = provider_name.eq_ignore_ascii_case("cloudflared")
            && std::env::var("CLOUDFLARE_TUNNEL_TOKEN")
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false);

        let has_api_config = if provider_name.eq_ignore_ascii_case("cloudflared") {
            config
                .tunnel
                .providers
                .cloudflared
                .as_ref()
                .map(|cloudflared| {
                    !cloudflared.manual_instructions
                        && cloudflared.api_token_path.is_some()
                        && cloudflared
                            .account_id
                            .as_ref()
                            .map(|value| !value.trim().is_empty())
                            .unwrap_or(false)
                })
                .unwrap_or(false)
        } else {
            false
        };

        // If cloudflared provider has no credentials at all, return disabled early
        if provider_name.eq_ignore_ascii_case("cloudflared") && !has_env_token && !has_api_config {
            let state = FeatureTunnelState::disabled(
                Some(provider_name.clone()),
                "Tunnel provisioning disabled until Cloudflare credentials are configured",
            );
            return Ok((Some(state), warnings));
        }

        let hostname = match hostname {
            Some(value) if !value.is_empty() => value,
            _ => {
                warnings.push(
                    "Tunnel provisioning skipped; feature hostname unavailable. Configure APP_URL before enabling tunnels."
                        .to_string(),
                );
                let state = FeatureTunnelState::disabled(
                    Some(provider_name.clone()),
                    "Feature hostname unavailable; configure APP_URL to enable tunnels.",
                );
                return Ok((Some(state), warnings));
            }
        };

        // If env token is present, use it directly
        if has_env_token {
            let token = std::env::var("CLOUDFLARE_TUNNEL_TOKEN").unwrap();
            self.write_feature_tunnel_env(feature_dir, hostname, token.trim())?;
            let state = FeatureTunnelState {
                provider: provider_name.clone(),
                hostname: Some(hostname.to_string()),
                service_url: Some(service_url.to_string()),
                status: FeatureTunnelStatus::Active,
                descriptor: None,
                instructions: None,
                notes: Some(
                    "Tunnel token provided via CLOUDFLARE_TUNNEL_TOKEN; skipping API provisioning"
                        .to_string(),
                ),
                last_updated: Utc::now(),
                removed_at: None,
            };
            return Ok((Some(state), warnings));
        }

        let (mut state, mut provider_warnings, provision_token) = self.invoke_tunnel_provider(
            &provider_name,
            work_feature,
            hostname,
            service_url,
            &config,
        )?;
        warnings.append(&mut provider_warnings);

        if state.descriptor.is_some() {
            match self.persist_tunnel_credentials(
                feature_dir,
                hostname,
                provision_token.as_deref(),
                state
                    .descriptor
                    .as_ref()
                    .and_then(|stored| stored.token_path.as_ref()),
            ) {
                Ok(_) => {
                    state.status = FeatureTunnelStatus::Active;
                }
                Err(err) => warnings.push(format!("Failed to write tunnel credentials: {}", err)),
            }
        }

        Ok((Some(state), warnings))
    }

    fn rollback_prepared_tunnel(&self, state: Option<&FeatureTunnelState>) {
        let Some(state) = state else {
            return;
        };
        let Some(descriptor) = state.descriptor.as_ref() else {
            return;
        };
        let Ok(config) = BranchBoxConfig::load(&self.repo_root) else {
            tracing::warn!("Could not load tunnel configuration for failed-start rollback");
            return;
        };
        let runtime_descriptor = self.stored_descriptor_to_runtime(state, descriptor);
        if let Err(err) = self.invoke_tunnel_teardown(&state.provider, &runtime_descriptor, &config)
        {
            tracing::warn!("Failed to roll back tunnel after environment startup failure: {err}");
        }
    }

    fn resolve_tunnel_hostname(&self, metadata: &FeatureMetadata) -> Result<String> {
        if let Some(host) = metadata
            .tunnel
            .as_ref()
            .and_then(|state| state.hostname.clone())
            .filter(|host| !host.is_empty())
        {
            return Ok(host);
        }

        if let Some(host) = metadata.feature_url.as_ref() {
            if !host.is_empty() {
                return Ok(host.clone());
            }
        }

        let env_path = self.repo_root.join(".env");
        let app_url = AppUrl::from_env_file(&env_path).map_err(|err| {
            let cause = match err {
                Error::Validation(message) => message,
                other => other.to_string(),
            };
            Error::validation(format!(
                "Cannot derive a tunnel hostname for '{}' from {}: {cause}. Set APP_URL in that \
                 file and retry.",
                metadata.work_feature,
                env_path.display()
            ))
        })?;
        Ok(naming::generate_feature_url(
            &app_url.url,
            &metadata.work_feature,
        ))
    }

    fn resolve_service_url(&self) -> String {
        match adapters::detect_adapter(&self.repo_root) {
            Ok(adapter) => adapter.service_url(),
            Err(err) => {
                tracing::debug!(
                    "Falling back to default service URL during tunnel provisioning: {}",
                    err
                );
                "web:3000".to_string()
            }
        }
    }

    fn persist_tunnel_credentials(
        &self,
        feature_dir: &Path,
        hostname: &str,
        token: Option<&str>,
        token_path: Option<&PathBuf>,
    ) -> Result<()> {
        let token_value = if let Some(value) = token {
            value.trim().to_string()
        } else if let Some(path) = token_path {
            match self.read_connector_token(path)? {
                Some(value) => value,
                None => {
                    return Err(Error::validation(format!(
                        "Tunnel token not found in {}",
                        path.display()
                    )))
                }
            }
        } else {
            return Err(Error::validation(
                "Tunnel token not provided by tunnel provider".to_string(),
            ));
        };

        if token_value.is_empty() {
            return Err(Error::validation("Tunnel token is empty".to_string()));
        }

        self.write_feature_tunnel_env(feature_dir, hostname, &token_value)?;
        Ok(())
    }

    fn write_feature_tunnel_env(
        &self,
        feature_dir: &Path,
        hostname: &str,
        token: &str,
    ) -> Result<PathBuf> {
        let dev_dir = feature_dir.join(".devcontainer");
        fs::create_dir_all(&dev_dir)?;
        let env_file = dev_dir.join(".cloudflared.env");
        let content = format!("TUNNEL_TOKEN={token}\nDEV_HOSTNAME={hostname}\n");
        write_secure_file(&env_file, &content)?;
        Ok(env_file)
    }

    fn read_connector_token(&self, path: &Path) -> Result<Option<String>> {
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(path)?;
        for line in content.lines() {
            if let Some(value) = line.strip_prefix("TUNNEL_TOKEN=") {
                let cleaned = value.trim().trim_matches(|c| matches!(c, '"' | '\''));
                return Ok(Some(cleaned.to_string()));
            }
        }

        Ok(None)
    }

    fn normalize_tunnel_provider_name(provider: &str) -> Option<&'static str> {
        if provider.eq_ignore_ascii_case("cloudflared") {
            Some("cloudflared")
        } else {
            None
        }
    }

    fn invoke_tunnel_provider(
        &self,
        provider_name: &str,
        work_feature: &str,
        hostname: &str,
        service_url: &str,
        config: &BranchBoxConfig,
    ) -> Result<(FeatureTunnelState, Vec<String>, Option<String>)> {
        let mut warnings = Vec::new();

        match provider_name {
            "cloudflared" => {
                let provider_config = config
                    .tunnel
                    .providers
                    .cloudflared
                    .clone()
                    .unwrap_or_default();
                let provider = CloudflaredProvider::new(&provider_config, &self.repo_root);
                let intent = ProvisioningIntent {
                    workspace_root: &self.repo_root,
                    feature_name: work_feature,
                    hostname,
                    service_url,
                };

                match provider.provision(&intent) {
                    Ok(ProvisioningOutcome::Automated { descriptor, token }) => {
                        let state = FeatureTunnelState::automated(
                            descriptor,
                            Some(service_url.to_string()),
                        );
                        Ok((state, warnings, token))
                    }
                    Ok(ProvisioningOutcome::Manual(instructions)) => {
                        warnings.push(format!(
                            "Tunnel requires manual setup: {}",
                            instructions.reason
                        ));
                        let state = FeatureTunnelState::manual(
                            provider.name().to_string(),
                            Some(hostname.to_string()),
                            Some(service_url.to_string()),
                            instructions.reason,
                            instructions.steps,
                        );
                        Ok((state, warnings, None))
                    }
                    Ok(ProvisioningOutcome::Disabled(reason)) => {
                        warnings.push(format!("Tunnel provisioning disabled: {}", reason));
                        let state =
                            FeatureTunnelState::disabled(Some(provider.name().to_string()), reason);
                        Ok((state, warnings, None))
                    }
                    Err(err) => {
                        warnings.push(format!(
                            "Tunnel provider error ({}): {}",
                            provider.name(),
                            err
                        ));
                        let state = FeatureTunnelState::manual(
                            provider.name().to_string(),
                            Some(hostname.to_string()),
                            Some(service_url.to_string()),
                            "Tunnel provisioning failed; follow manual setup instructions.",
                            Vec::new(),
                        );
                        Ok((state, warnings, None))
                    }
                }
            }
            _ => {
                warnings.push(format!(
                    "Tunnel provider '{}' not recognized; skipping provisioning",
                    provider_name
                ));
                let state = FeatureTunnelState::disabled(
                    Some(provider_name.to_string()),
                    "Unsupported tunnel provider",
                );
                Ok((state, warnings, None))
            }
        }
    }

    fn invoke_tunnel_teardown(
        &self,
        provider_name: &str,
        descriptor: &TunnelDescriptor,
        config: &BranchBoxConfig,
    ) -> Result<()> {
        match provider_name {
            "cloudflared" => {
                let provider_config = config
                    .tunnel
                    .providers
                    .cloudflared
                    .clone()
                    .unwrap_or_default();
                let provider = CloudflaredProvider::new(&provider_config, &self.repo_root);
                provider.teardown(descriptor)
            }
            _ => Err(Error::validation(format!(
                "Unsupported tunnel provider '{}'",
                provider_name
            ))),
        }
    }

    fn stored_descriptor_to_runtime(
        &self,
        tunnel: &FeatureTunnelState,
        stored: &StoredTunnelDescriptor,
    ) -> TunnelDescriptor {
        TunnelDescriptor {
            provider: tunnel.provider.clone(),
            tunnel_name: stored.tunnel_name.clone(),
            tunnel_id: stored.tunnel_id.clone(),
            hostname: tunnel.hostname.clone().unwrap_or_default(),
            token_path: stored.token_path.clone(),
        }
    }

    fn run_module_teardown(
        &self,
        handles: Vec<ModuleHandle>,
        feature_dir: &Path,
    ) -> (Vec<ModuleTeardownReport>, Vec<String>) {
        let mut reports = Vec::with_capacity(handles.len());
        let mut warnings = Vec::new();

        for mut handle in handles.into_iter().rev() {
            let mut report = ModuleTeardownReport {
                name: handle.name.clone(),
                teardown_ok: false,
                errors: Vec::new(),
            };

            match handle.module.init(&self.repo_root, feature_dir) {
                Ok(_) => match handle.module.teardown(&self.repo_root, feature_dir) {
                    Ok(_) => {
                        report.teardown_ok = true;
                    }
                    Err(err) => {
                        let message = err.to_string();
                        report.errors.push(message.clone());
                        warnings.push(format!(
                            "Module '{}' teardown reported an error: {}",
                            report.name, message
                        ));
                    }
                },
                Err(err) => {
                    let message = err.to_string();
                    report.errors.push(message.clone());
                    warnings.push(format!(
                        "Module '{}' initialization failed during teardown: {}",
                        report.name, message
                    ));
                }
            }

            reports.push(report);
        }

        reports.reverse();
        (reports, warnings)
    }

    fn determine_feature_spec(
        &self,
        base_dir: &Path,
        work_feature: &str,
    ) -> Result<Option<(PathBuf, SpecStatus)>> {
        let features_dir = base_dir.join("docs/features");
        if !features_dir.exists() {
            return Ok(None);
        }

        let spec_name = format!("{}.md", work_feature);
        for status in [
            SpecStatus::InProgress,
            SpecStatus::Backlog,
            SpecStatus::Completed,
        ] {
            let candidate = features_dir.join(status.as_str()).join(&spec_name);
            if candidate.exists() {
                return Ok(Some((candidate, status)));
            }
        }

        Ok(None)
    }

    fn transfer_spec(
        &self,
        source: &Path,
        destination: &Path,
        warnings: &mut Vec<String>,
        context: &str,
        preserve_source: bool,
    ) -> bool {
        if let Some(parent) = destination.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                warnings.push(format!(
                    "Failed to prepare directory '{}' for {}: {}",
                    parent.display(),
                    context,
                    err
                ));
                return false;
            }
        }

        if !preserve_source {
            match fs::rename(source, destination) {
                Ok(_) => return true,
                Err(rename_err) => match fs::copy(source, destination) {
                    Ok(_) => {
                        if let Err(remove_err) = fs::remove_file(source) {
                            warnings.push(format!(
                                "Copied {} but failed to delete source '{}': {}",
                                context,
                                source.display(),
                                remove_err
                            ));
                        }
                        return true;
                    }
                    Err(copy_err) => {
                        warnings.push(format!(
                                "Failed to move {} from '{}' to '{}' (rename error: {}, copy error: {})",
                                context,
                                source.display(),
                                destination.display(),
                                rename_err,
                                copy_err
                            ));
                        return false;
                    }
                },
            }
        }

        match fs::copy(source, destination) {
            Ok(_) => true,
            Err(err) => {
                warnings.push(format!(
                    "Failed to copy {} from '{}' to '{}': {}",
                    context,
                    source.display(),
                    destination.display(),
                    err
                ));
                false
            }
        }
    }

    fn ensure_spec_for_start(
        &self,
        work_feature: &str,
        worktree_path: &Path,
        branch_name: &str,
        warnings: &mut Vec<String>,
    ) -> Option<PathBuf> {
        let features_dir = worktree_path.join("docs/features");
        if let Err(err) = fs::create_dir_all(features_dir.join("in-progress")) {
            warnings.push(format!("Failed to ensure specs directory exists: {}", err));
            return None;
        }

        let spec_name = format!("{}.md", work_feature);
        let in_progress = features_dir.join("in-progress").join(&spec_name);

        let worktree_spec = match self.determine_feature_spec(worktree_path, work_feature) {
            Ok(spec) => spec,
            Err(err) => {
                warnings.push(format!(
                    "Failed to inspect existing feature spec in worktree for '{}': {}",
                    work_feature, err
                ));
                None
            }
        };

        let repo_spec = match self.determine_feature_spec(&self.repo_root, work_feature) {
            Ok(spec) => spec,
            Err(err) => {
                warnings.push(format!(
                    "Failed to inspect repository feature spec for '{}': {}",
                    work_feature, err
                ));
                None
            }
        };

        let mut spec_path: Option<PathBuf> = None;

        if let Some((existing, status)) = worktree_spec {
            if matches!(status, SpecStatus::Backlog) && existing != in_progress {
                if self.transfer_spec(
                    &existing,
                    &in_progress,
                    warnings,
                    "feature spec inside worktree",
                    false,
                ) {
                    spec_path = Some(in_progress.clone());
                } else {
                    spec_path = Some(existing);
                }
            } else {
                spec_path = Some(existing);
            }
        } else if let Some((source, status)) = repo_spec {
            let preserve_source = matches!(status, SpecStatus::Completed);
            let transferred = self.transfer_spec(
                &source,
                &in_progress,
                warnings,
                "feature spec into worktree",
                preserve_source,
            );

            if transferred || in_progress.exists() {
                spec_path = Some(in_progress.clone());
            } else if preserve_source {
                spec_path = Some(source);
            }
        }

        if spec_path.is_none() {
            let content = format!(
                "---\nworktree: {}\nbranch: {}\nwork_feature: {}\nstatus: in-progress\ncreated: {}\n---\n\n# {}\n\n## Overview\n\nTODO: Describe the feature scope.\n",
                worktree_path.display(),
                branch_name,
                work_feature,
                Utc::now().date_naive(),
                feature_title_from_work_feature(work_feature)
            );

            if let Err(err) = write_text_file(&in_progress, &content) {
                warnings.push(format!(
                    "Failed to create feature spec '{}': {}",
                    in_progress.display(),
                    err
                ));
                return None;
            }
            spec_path = Some(in_progress.clone());
        }

        let updates = vec![
            ("worktree".to_string(), worktree_path.display().to_string()),
            ("branch".to_string(), branch_name.to_string()),
            ("work_feature".to_string(), work_feature.to_string()),
            ("status".to_string(), "in-progress".to_string()),
        ];

        let spec_path = spec_path.unwrap();
        if let Err(err) = update_spec_frontmatter(&spec_path, &updates, &[]) {
            warnings.push(format!(
                "Failed to update feature spec frontmatter '{}': {}",
                spec_path.display(),
                err
            ));
        }

        Some(spec_path)
    }

    /// Move the feature spec into the main worktree before the worktree goes: to
    /// `docs/features/completed/` with `mark_complete`, else back to `docs/features/backlog/`.
    /// Returns the move it made, if any; problems become warnings.
    fn handle_spec_on_teardown(
        &self,
        work_feature: &str,
        branch_name: &str,
        worktree_path: &Path,
        mark_complete: bool,
        warnings: &mut Vec<String>,
    ) -> Option<PreservedFile> {
        let worktree_spec = match self.determine_feature_spec(worktree_path, work_feature) {
            Ok(spec) => spec,
            Err(err) => {
                warnings.push(format!(
                    "Failed to inspect worktree spec for '{}': {}",
                    work_feature, err
                ));
                None
            }
        };
        let repo_spec = match self.determine_feature_spec(&self.repo_root, work_feature) {
            Ok(spec) => spec,
            Err(err) => {
                warnings.push(format!(
                    "Failed to inspect repository spec for '{}': {}",
                    work_feature, err
                ));
                None
            }
        };

        if mark_complete {
            let Some((source_spec, _)) = worktree_spec.as_ref().or(repo_spec.as_ref()) else {
                warnings.push(format!(
                    "Unable to locate feature spec '{}' during teardown",
                    work_feature
                ));
                return None;
            };

            let features_dir = self.repo_root.join("docs/features");
            let completed_dir = features_dir.join(SpecStatus::Completed.as_str());
            if let Err(err) = fs::create_dir_all(&completed_dir) {
                warnings.push(format!(
                    "Failed to prepare completed specs directory: {}",
                    err
                ));
                return None;
            }

            let target = completed_dir.join(format!("{}.md", work_feature));
            if !self.transfer_spec(
                source_spec,
                &target,
                warnings,
                "feature spec to completed",
                false,
            ) {
                return None;
            }
            let moved = PreservedFile {
                path: self.spec_display_path(source_spec, worktree_path),
                destination: self.spec_display_path(&target, worktree_path),
            };

            for status_dir in [SpecStatus::Backlog, SpecStatus::InProgress] {
                let candidate = features_dir
                    .join(status_dir.as_str())
                    .join(format!("{}.md", work_feature));
                if candidate.exists() && candidate != target {
                    if let Err(err) = fs::remove_file(&candidate) {
                        warnings.push(format!(
                            "Failed to remove '{}' spec at {}: {}",
                            status_dir.as_str(),
                            candidate.display(),
                            err
                        ));
                    }
                }
            }

            let mut updates = vec![
                ("status".to_string(), "completed".to_string()),
                ("branch".to_string(), branch_name.to_string()),
                ("worktree".to_string(), worktree_path.display().to_string()),
            ];
            updates.push(("completed".to_string(), Utc::now().date_naive().to_string()));

            if let Err(err) = update_spec_frontmatter(&target, &updates, &[]) {
                warnings.push(format!(
                    "Failed to update completed spec frontmatter '{}': {}",
                    target.display(),
                    err
                ));
            }
            Some(moved)
        } else {
            let (source_spec, status) = worktree_spec.or(repo_spec)?;

            let features_dir = self.repo_root.join("docs/features");
            let backlog_dir = features_dir.join(SpecStatus::Backlog.as_str());
            if let Err(err) = fs::create_dir_all(&backlog_dir) {
                warnings.push(format!(
                    "Failed to prepare backlog specs directory: {}",
                    err
                ));
                return None;
            }

            let target = backlog_dir.join(format!("{}.md", work_feature));

            let mut moved = None;
            if source_spec != target {
                let preserve_source = matches!(status, SpecStatus::Completed);
                if !self.transfer_spec(
                    &source_spec,
                    &target,
                    warnings,
                    "feature spec back to repository",
                    preserve_source,
                ) {
                    return None;
                }
                moved = Some(PreservedFile {
                    path: self.spec_display_path(&source_spec, worktree_path),
                    destination: self.spec_display_path(&target, worktree_path),
                });
            }

            if let Err(err) = update_spec_frontmatter(
                &target,
                &[("status".to_string(), "backlog".to_string())],
                &["worktree", "branch", "completed"],
            ) {
                warnings.push(format!(
                    "Failed to refresh spec frontmatter '{}': {}",
                    target.display(),
                    err
                ));
            }
            moved
        }
    }

    /// `spec` relative to the feature worktree or the main worktree it lives in, `/`-separated.
    fn spec_display_path(&self, spec: &Path, worktree_path: &Path) -> String {
        spec.strip_prefix(worktree_path)
            .or_else(|_| spec.strip_prefix(&self.repo_root))
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| spec.display().to_string())
    }

    fn ensure_host_environment(&self) -> Result<()> {
        if std::env::var("BRANCHBOX_SKIP_HOST_VALIDATION").is_ok() {
            tracing::debug!(
                "Skipping host environment validation via BRANCHBOX_SKIP_HOST_VALIDATION"
            );
            return Ok(());
        }

        validation::validate_host_environment()
    }

    /// Set up VS Code workspace customization for visual differentiation.
    fn setup_vscode_workspace(
        &self,
        worktree_path: &Path,
        work_feature: &str,
        color: &Option<String>,
        feature_url: Option<&str>,
    ) -> Result<()> {
        let vscode_dir = worktree_path.join(".vscode");
        fs::create_dir_all(&vscode_dir)?;

        // Set up Peacock extension color and window title
        // Only the settings in VSCODE_MANAGED_SETTINGS are written: teardown recognizes the file
        // as BranchBox-generated by comparing everything else with the committed file.
        let settings_path = vscode_dir.join("settings.json");
        let mut settings = if settings_path.exists() {
            let content = fs::read(&settings_path)?;
            match parse_jsonc_object(&content) {
                Some(object) => serde_json::Value::Object(object),
                None => {
                    tracing::debug!(
                        "Failed to parse existing settings.json at {} as a JSON object. Using \
                         empty object.",
                        settings_path.display()
                    );
                    serde_json::json!({})
                }
            }
        } else {
            serde_json::json!({})
        };

        if let Some(color_value) = color {
            settings[VSCODE_PEACOCK_COLOR] = serde_json::json!(color_value);
            settings[VSCODE_PEACOCK_REMOTE_COLOR] = serde_json::json!(color_value);
            if let Some(customizations) = build_color_customizations(color_value) {
                settings[VSCODE_COLOR_CUSTOMIZATIONS] = customizations;
            }
        }

        // Customize window title to show feature name
        settings[VSCODE_WINDOW_TITLE] = serde_json::json!(format!(
            "${{rootName}} [{}] - ${{activeEditorShort}}",
            work_feature
        ));

        let settings_json = serde_json::to_string_pretty(&settings)
            .map_err(|e| Error::config(format!("Failed to serialize VS Code settings: {}", e)))?;
        write_text_file(&settings_path, &settings_json)?;

        // Set up quick access task for feature URL
        if let Some(url) = feature_url {
            let normalized_url = sanitize_url_env_value(
                url.trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://"),
            );
            if !normalized_url.is_empty() {
                let full_url = format!("https://{}", normalized_url);
                let tasks_path = vscode_dir.join("tasks.json");
                let tasks = serde_json::json!({
                    "version": "2.0.0",
                    "tasks": [
                        {
                            "label": VSCODE_FEATURE_URL_TASK,
                            "type": "process",
                            "command": "xdg-open",
                            "args": [full_url.clone()],
                            "problemMatcher": [],
                            "presentation": {
                                "reveal": "silent",
                                "panel": "shared"
                            },
                            "osx": {
                                "command": "open",
                                "args": [full_url.clone()]
                            },
                            "windows": {
                                "command": "explorer",
                                "args": [full_url]
                            }
                        }
                    ]
                });

                let tasks_json = serde_json::to_string_pretty(&tasks).map_err(|e| {
                    Error::config(format!("Failed to serialize VS Code tasks: {}", e))
                })?;
                write_text_file(&tasks_path, &tasks_json)?;
            }
        }

        Ok(())
    }

    /// Fix git worktree path to use relative paths for devcontainer compatibility.
    ///
    /// When a worktree is created, git stores an absolute path to the main repo's
    /// .git/worktrees/ directory. This breaks in devcontainers where paths are mounted
    /// differently. We convert absolute paths to relative paths.
    fn fix_git_worktree_path(&self, worktree_path: &Path) -> Result<()> {
        let git_file = worktree_path.join(".git");

        if !git_file.exists() {
            return Err(Error::validation(format!(
                "No .git file found in worktree at {}",
                worktree_path.display()
            )));
        }

        // Read the .git file
        let content = fs::read_to_string(&git_file)?;

        // Parse the gitdir line
        let gitdir_line = content
            .lines()
            .find(|line| line.starts_with("gitdir:"))
            .ok_or_else(|| Error::validation("No gitdir: line found in .git file".to_string()))?;

        let current_path = gitdir_line.strip_prefix("gitdir:").unwrap_or("").trim();

        // If already relative, nothing to do
        if !current_path.starts_with('/') {
            tracing::debug!("Git worktree path is already relative: {}", current_path);
            return Ok(());
        }

        let repository_worktrees = self.repository_worktrees_dir()?;
        let current_target = Path::new(current_path);
        let container_worktrees = Path::new("/workspaces/main/.git/worktrees");
        let target_to_validate = match current_target.strip_prefix(container_worktrees) {
            Ok(relative) => {
                let components: Vec<_> = relative.components().collect();
                if components.len() != 1
                    || !matches!(components[0], std::path::Component::Normal(_))
                {
                    return Err(Error::validation(format!(
                        "Container gitdir target '{}' is not a single repository worktree entry; preserving the original worktree pointer",
                        current_path
                    )));
                }
                repository_worktrees.join(relative)
            }
            Err(_) => current_target.to_path_buf(),
        };

        let authoritative_target = fs::canonicalize(&target_to_validate).map_err(|err| {
            Error::validation(format!(
                "Cannot validate gitdir target '{}' as '{}'; preserving the original worktree pointer: {err}",
                current_path,
                target_to_validate.display()
            ))
        })?;
        if !authoritative_target.starts_with(&repository_worktrees) {
            return Err(Error::validation(format!(
                "Absolute gitdir target '{}' does not belong to repository '{}'; preserving the original worktree pointer",
                authoritative_target.display(),
                self.repo_root.display()
            )));
        }

        let canonical_worktree = fs::canonicalize(worktree_path).map_err(|err| {
            Error::validation(format!(
                "Cannot validate worktree directory '{}': {err}",
                worktree_path.display()
            ))
        })?;
        let relative_path = relative_path_between(&canonical_worktree, &authoritative_target)
            .ok_or_else(|| {
                Error::validation(format!(
                    "Cannot construct a relative gitdir path from '{}' to '{}'; preserving the original worktree pointer",
                    canonical_worktree.display(),
                    authoritative_target.display()
                ))
            })?;

        // Convert to string for gitdir format (git expects forward slashes)
        let relative_path_str = relative_path
            .to_str()
            .ok_or_else(|| Error::validation("Invalid UTF-8 in worktree path".to_string()))?
            .replace('\\', "/"); // Ensure forward slashes on Windows

        let resolved_target = canonical_worktree.join(&relative_path);
        let resolved_target = fs::canonicalize(&resolved_target).map_err(|err| {
            Error::validation(format!(
                "Rewritten gitdir target '{}' could not be validated; preserving the original worktree pointer: {err}",
                resolved_target.display()
            ))
        })?;
        if resolved_target != authoritative_target {
            return Err(Error::validation(format!(
                "Rewritten gitdir target '{}' does not match authoritative target '{}'; preserving the original worktree pointer",
                resolved_target.display(),
                authoritative_target.display()
            )));
        }

        let new_gitdir_line = format!("gitdir: {relative_path_str}");
        let new_content = content.replacen(gitdir_line, &new_gitdir_line, 1);
        write_text_file(&git_file, &new_content)?;

        tracing::info!(
            "Fixed git worktree path from absolute to relative: {}",
            relative_path_str
        );

        Ok(())
    }

    /// Point the active worktree at the exact Git metadata projection exposed inside a managed
    /// in-guest devcontainer. The target entry is derived from validated host metadata; repository
    /// content cannot select another worktree or escape the shared Git directory.
    fn set_in_guest_git_worktree_path(&self, worktree_path: &Path) -> Result<()> {
        let git_file = worktree_path.join(".git");
        if !git_file.is_file() {
            return Err(Error::validation(format!(
                "No .git file found in worktree at {}",
                worktree_path.display()
            )));
        }

        let content = fs::read_to_string(&git_file)?;
        let gitdir_line = content
            .lines()
            .find(|line| line.starts_with("gitdir:"))
            .ok_or_else(|| Error::validation("No gitdir: line found in .git file".to_string()))?;
        let current_path = gitdir_line.strip_prefix("gitdir:").unwrap_or("").trim();
        let repository_worktrees = self.repository_worktrees_dir()?;
        let container_worktrees = Path::new("/workspaces/main/.git/worktrees");

        let target_to_validate =
            if let Ok(relative) = Path::new(current_path).strip_prefix(container_worktrees) {
                repository_worktrees.join(relative)
            } else if Path::new(current_path).is_absolute() {
                PathBuf::from(current_path)
            } else {
                worktree_path.join(current_path)
            };
        let authoritative_target = fs::canonicalize(&target_to_validate).map_err(|err| {
            Error::validation(format!(
                "Cannot validate in-guest gitdir target '{}' as '{}': {err}",
                current_path,
                target_to_validate.display()
            ))
        })?;
        let entry = authoritative_target
            .strip_prefix(&repository_worktrees)
            .ok()
            .and_then(|relative| {
                let components: Vec<_> = relative.components().collect();
                (components.len() == 1 && matches!(components[0], std::path::Component::Normal(_)))
                    .then_some(components[0].as_os_str())
            })
            .ok_or_else(|| {
                Error::validation(format!(
                    "Git metadata target '{}' is not one exact repository worktree entry",
                    authoritative_target.display()
                ))
            })?;
        let entry = entry
            .to_str()
            .ok_or_else(|| Error::validation("Invalid UTF-8 in Git worktree entry".to_string()))?;
        let container_target = container_worktrees.join(entry);
        let new_gitdir_line = format!("gitdir: {}", container_target.display());
        let new_content = content.replacen(gitdir_line, &new_gitdir_line, 1);
        write_text_file(&git_file, &new_content)?;

        tracing::info!(
            "Projected Git worktree metadata into managed devcontainer: {}",
            container_target.display()
        );
        Ok(())
    }

    /// Resolve the repository's authoritative shared Git metadata. `repo_root` may itself be a
    /// linked worktree, so canonicalizing `<repo_root>/.git` is insufficient: it resolves to that
    /// worktree's entry instead of the common directory that owns all sibling worktrees.
    fn repository_worktrees_dir(&self) -> Result<PathBuf> {
        Ok(repository_common_git_dir(&self.repo_root)?.join("worktrees"))
    }
}

fn relative_path_between(from: &Path, to: &Path) -> Option<PathBuf> {
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }

    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &to_components[common..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

/// Helper to ensure the file ends with a newline before appending.
fn ensure_trailing_newline(file: &mut File) -> io::Result<()> {
    file.write_all(b"\n")
}

fn devcontainer_requires_cloudflared_env(worktree_path: &Path) -> bool {
    let devcontainer_dir = worktree_path.join(".devcontainer");
    let Ok(entries) = fs::read_dir(devcontainer_dir) else {
        return false;
    };
    entries.filter_map(std::result::Result::ok).any(|entry| {
        let path = entry.path();
        let supported = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "json" | "yaml" | "yml"));
        supported
            && fs::read_to_string(path)
                .map(|content| content.contains(".cloudflared.env"))
                .unwrap_or(false)
    })
}

const SBX_DEVCONTAINER_CONFIG: &str = ".devcontainer.json";
const SBX_COMPOSE_OVERRIDE: &str = ".branchbox-sbx-compose.yaml";
/// Managed in-guest worktrees are writable by the coding container, so even planning a teardown
/// must not inspect them with Git: a repository-configured filter or hook would run as the
/// runtime. Only a forced teardown, which never scans them, is allowed.
fn require_forced_in_guest_teardown(in_guest: bool, request: &TeardownRequest) -> Result<()> {
    if in_guest && !request.force_remove {
        return Err(Error::validation(
            "Managed in-guest teardown requires --force: checking consumer-writable Git worktree changes could execute a repository-configured filter as the BranchBox runtime",
        ));
    }
    Ok(())
}

const SBX_COMPOSE_INPUT_PREFIX: &str = ".branchbox-sbx-compose-input-";
const SBX_COMPOSE_INPUT_MARKER: &str = "# Generated by BranchBox for in-guest Compose.\n";
const IN_GUEST_OMITTED_CONNECTOR_IMAGE: &str = "busybox:1.36";
const MAX_IN_GUEST_INPUT_BYTES: u64 = 8 * 1024 * 1024;
const IN_GUEST_SHM_SIZE: &str = "1gb";
const CONTAINER_MAIN_GIT_TARGET: &str = "/workspaces/main/.git";
const IN_GUEST_SECCOMP_SECURITY_OPTION: &str = "seccomp=builtin";

fn prepare_in_guest_devcontainer_config(
    repo_root: &Path,
    worktree_path: &Path,
    plan: &InGuestFacadePlan,
) -> Result<()> {
    let devcontainer_dir = worktree_path.join(".devcontainer");
    let private_stage = plan.private_compose_stage_dir(worktree_path)?;
    if let Some(stage) = private_stage.as_deref() {
        runtime::validate_private_compose_stage_git_mount(stage, repo_root)?;
    }
    let output_dir = private_stage.as_deref().unwrap_or(&devcontainer_dir);
    let directory_metadata = fs::symlink_metadata(&devcontainer_dir).map_err(|err| {
        Error::validation(format!(
            "Could not inspect in-guest devcontainer directory '{}': {err}",
            devcontainer_dir.display()
        ))
    })?;
    if !directory_metadata.is_dir() || directory_metadata.file_type().is_symlink() {
        return Err(Error::validation(
            "In-guest .devcontainer must be a real directory inside the task worktree",
        ));
    }
    let config_path = devcontainer_dir.join("devcontainer.json");
    let source = read_in_guest_worktree_file(worktree_path, &config_path)?;
    let mut value = jsonc_parser::parse_to_serde_value(&source, &Default::default())
        .map_err(|err| Error::validation(format!("Failed to parse devcontainer JSONC: {err:?}")))?
        .ok_or_else(|| Error::validation("Devcontainer configuration is empty"))?;
    let config: DevcontainerConfig = serde_json::from_value(value.clone()).map_err(|err| {
        Error::validation(format!("Could not inspect in-guest devcontainer: {err}"))
    })?;
    let compose_references: Vec<String> = match config.docker_compose_file.as_ref() {
        Some(reference) => reference.to_vec(),
        None => [
            "compose.yaml",
            "compose.yml",
            "docker-compose.yaml",
            "docker-compose.yml",
        ]
        .iter()
        .filter(|path| devcontainer_dir.join(path).exists())
        .map(|path| (*path).to_string())
        .collect(),
    };
    if config.docker_compose_file.is_some() && compose_references.is_empty() {
        return Err(Error::validation(
            "In-guest dockerComposeFile must name at least one file; an empty list lets the Dev Containers CLI load COMPOSE_FILE from the worktree .env",
        ));
    }
    let compose_files: Vec<PathBuf> = compose_references
        .iter()
        .map(|path| devcontainer_dir.join(path))
        .collect();
    for compose_file in &compose_files {
        let name = compose_file.file_name().and_then(|name| name.to_str());
        if name == Some(SBX_COMPOSE_OVERRIDE)
            || name.is_some_and(|name| name.starts_with(SBX_COMPOSE_INPUT_PREFIX))
        {
            return Err(Error::validation(format!(
                "In-guest Compose source '{}' conflicts with a reserved BranchBox facade name",
                compose_file.display()
            )));
        }
    }
    let preloaded_image_mode = !plan.service_images().is_empty();
    let security_option_in_devcontainer = config.service.is_none() || compose_files.is_empty();
    sanitize_in_guest_devcontainer_json(
        &mut value,
        security_option_in_devcontainer,
        preloaded_image_mode,
    )?;
    let mut compose_documents = Vec::with_capacity(compose_files.len());
    for compose_file in &compose_files {
        canonical_in_guest_compose_parent(
            worktree_path,
            compose_file
                .parent()
                .ok_or_else(|| Error::validation("In-guest Compose source has no parent"))?,
        )?;
        let document = read_in_guest_compose_document(worktree_path, compose_file)?;
        reject_supervisor_socket_references_yaml(&document)?;
        validate_in_guest_compose_security(
            &document,
            config.service.as_deref(),
            &devcontainer_dir,
            worktree_path,
            plan.service_images(),
        )?;
        compose_documents.push(document);
    }

    if let Some(stage) = private_stage.as_deref() {
        validate_private_in_guest_generated_path(stage, &stage.join(SBX_COMPOSE_OVERRIDE))?;
    } else {
        validate_in_guest_generated_path(worktree_path, &output_dir.join(SBX_COMPOSE_OVERRIDE))?;
    }
    prepare_sbx_compose_override(
        repo_root,
        output_dir,
        config.service.as_deref(),
        &compose_files,
        Some(&compose_documents),
        Some(worktree_path),
        private_stage.as_deref(),
    )?;
    let project_environment = plan.project_environment();
    if let Some((_, consumer)) = project_environment {
        if config.service.as_deref() != Some(consumer) {
            return Err(Error::validation(format!(
                "Project-environment consumer '{consumer}' must match the primary devcontainer service"
            )));
        }
    }
    let workspace_folder = effective_in_guest_workspace_folder(&config, worktree_path)?;
    let lease_mounts: Vec<(PathBuf, PathBuf)> = plan
        .mounts()
        .map(|(source, target)| (source.to_path_buf(), target.to_path_buf()))
        .collect();
    let spool_volumes: Vec<(String, PathBuf)> = plan
        .tool_request_spools()
        .map(|(volume, target, _consumer_uid)| (volume.to_string(), target.to_path_buf()))
        .collect();
    let assignment = InGuestComposeAssignment {
        project_environment: project_environment.map(|(source, _)| source),
        service_images: plan.service_images(),
        workspace_consumer: plan.workspace_consumer().is_some(),
        private_stage: private_stage.as_deref(),
        lease_mounts: &lease_mounts,
        spool_volumes: &spool_volumes,
    };
    let omitted_services = prepare_outer_tunnel_compose_override(
        repo_root,
        worktree_path,
        output_dir,
        config.service.as_deref(),
        &workspace_folder,
        &compose_documents,
        &assignment,
    )?;

    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::validation("Devcontainer configuration must be a JSON object"))?;
    let mounts = object
        .remove("mounts")
        .and_then(|mounts| mounts.as_array().cloned())
        .unwrap_or_default();
    // Read-only lease binds are carried by the generated Compose facade, which
    // preserves their read-only mode; declaring them here as well would have the
    // same bind resolved twice on the inspected container.
    // Tool-request spool volumes are carried by the generated Compose facade, which
    // pins their exact name; declaring them here instead lets Compose namespace the
    // volume under the project and the signed name no longer matches.
    if !mounts.is_empty() {
        object.insert("mounts".to_string(), serde_json::Value::Array(mounts));
    }
    object.insert(
        "workspaceFolder".to_string(),
        serde_json::Value::String(workspace_folder),
    );

    object.remove("runServices");
    if let Some(primary) = config.service.as_deref() {
        if omitted_services.contains(primary) {
            return Err(Error::validation(
                "The primary devcontainer service cannot be the outer cloudflared connector",
            ));
        }
        // Compose starts the validated dependency closure of the primary service. Do not let a
        // repository opt independent or connector services into the in-guest startup set.
        object.insert("runServices".to_string(), serde_json::json!([primary]));
    }

    let previous_inputs =
        previous_in_guest_compose_inputs(worktree_path, output_dir, private_stage.as_deref())?;
    let mut current_inputs = HashSet::new();
    if output_dir.join(SBX_COMPOSE_OVERRIDE).is_file() {
        // Compose interpolates each input before it merges the final !override facade.
        // It must never read repository mount/publication entries, even when they will
        // eventually be overridden (or contain required ${VAR:?} expressions).
        // Connector identity is collected across all inputs: one file may supply
        // its image while another supplies a tokenized command or entrypoint.
        let mut references = prepare_in_guest_compose_inputs(
            worktree_path,
            &compose_references,
            &compose_files,
            compose_documents,
            &InGuestComposeSanitization {
                primary_service: config.service.as_deref(),
                service_images: plan.service_images(),
                omitted_services: &omitted_services,
            },
            private_stage.as_deref(),
        )?;
        for reference in &references {
            let path = output_dir.join(reference);
            if private_stage.is_some() {
                current_inputs.insert(path);
                continue;
            }
            let parent = canonical_in_guest_compose_parent(
                worktree_path,
                path.parent()
                    .ok_or_else(|| Error::validation("In-guest Compose input has no parent"))?,
            )?;
            current_inputs.insert(
                parent.join(
                    path.file_name().ok_or_else(|| {
                        Error::validation("In-guest Compose input has no filename")
                    })?,
                ),
            );
        }
        references.push(SBX_COMPOSE_OVERRIDE.to_string());
        object.insert(
            "dockerComposeFile".to_string(),
            serde_json::json!(references),
        );
    }

    let generated = output_dir.join(SBX_DEVCONTAINER_CONFIG);
    if generated == config_path {
        return Err(Error::validation(
            "In-guest runtime overlays for a top-level .devcontainer.json are not supported; move the source config to .devcontainer/devcontainer.json",
        ));
    }
    write_managed_in_guest_generated_text_file(
        worktree_path,
        private_stage.as_deref(),
        &generated,
        &format!("{}\n", serde_json::to_string_pretty(&value)?),
    )?;
    for stale in previous_inputs {
        if let Some(stage) = private_stage.as_deref() {
            validate_private_in_guest_generated_path(stage, &stale)?;
        } else {
            validate_in_guest_generated_path(worktree_path, &stale)?;
        }
        if !current_inputs.contains(&stale)
            && read_in_guest_worktree_file(
                private_stage.as_deref().unwrap_or(worktree_path),
                &stale,
            )?
            .starts_with(SBX_COMPOSE_INPUT_MARKER)
        {
            remove_in_guest_generated_file(
                private_stage.as_deref().unwrap_or(worktree_path),
                &stale,
            )?;
        }
    }
    Ok(())
}

fn previous_in_guest_compose_inputs(
    worktree_path: &Path,
    output_dir: &Path,
    private_stage: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    let generated = output_dir.join(SBX_DEVCONTAINER_CONFIG);
    match fs::symlink_metadata(&generated) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
        Ok(_) => {
            if let Some(stage) = private_stage {
                validate_private_in_guest_generated_path(stage, &generated)?;
            } else {
                validate_in_guest_generated_path(worktree_path, &generated)?;
            }
        }
    }
    let source = read_in_guest_worktree_file(private_stage.unwrap_or(worktree_path), &generated)?;
    let Ok(previous) = serde_json::from_str::<serde_json::Value>(&source) else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for reference in previous["dockerComposeFile"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter(|reference| {
            Path::new(reference)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(SBX_COMPOSE_INPUT_PREFIX))
        })
    {
        if private_stage.is_some() && Path::new(reference).components().count() != 1 {
            continue;
        }
        let path = output_dir.join(reference);
        if !path.exists() {
            continue;
        }
        if private_stage.is_some() {
            paths.push(path);
            continue;
        }
        let parent = canonical_in_guest_compose_parent(
            worktree_path,
            path.parent()
                .ok_or_else(|| Error::validation("In-guest Compose input has no parent"))?,
        )?;
        paths.push(
            parent.join(
                path.file_name()
                    .ok_or_else(|| Error::validation("In-guest Compose input has no filename"))?,
            ),
        );
    }
    Ok(paths)
}

fn read_in_guest_compose_document(worktree_path: &Path, path: &Path) -> Result<serde_yaml::Value> {
    let source = read_in_guest_worktree_file(worktree_path, path)?;
    let mut document: serde_yaml::Value = serde_yaml::from_str(&source).map_err(|err| {
        Error::validation(format!(
            "Could not parse in-guest Compose file '{}': {err}",
            path.display()
        ))
    })?;
    // Compose applies YAML merges before interpreting service fields. Apply them
    // here too, so an anchored volume cannot survive the source sanitization.
    document.apply_merge().map_err(|err| {
        Error::validation(format!(
            "Could not resolve YAML merges in in-guest Compose file '{}': {err}",
            path.display()
        ))
    })?;
    reject_unsupported_in_guest_compose_tags(&document)?;
    Ok(document)
}

/// Read an untrusted worktree input only after proving it resolves to a bounded
/// regular file inside the task worktree. Direct file symlinks are allowed when
/// their canonical target stays inside the worktree; the canonical target is
/// opened so a symlink swap cannot redirect the read to an ambient path.
fn read_in_guest_worktree_file(worktree_path: &Path, path: &Path) -> Result<String> {
    let worktree = fs::canonicalize(worktree_path).map_err(|err| {
        Error::validation(format!(
            "Cannot resolve in-guest task worktree '{}': {err}",
            worktree_path.display()
        ))
    })?;
    let canonical = fs::canonicalize(path).map_err(|err| {
        Error::validation(format!(
            "Cannot resolve in-guest input '{}': {err}",
            path.display()
        ))
    })?;
    let parent = fs::canonicalize(
        path.parent()
            .ok_or_else(|| Error::validation("In-guest input has no parent directory"))?,
    )?;
    if !canonical.starts_with(&worktree) || !parent.starts_with(&worktree) {
        return Err(Error::validation(format!(
            "In-guest input '{}' must resolve inside the task worktree",
            path.display()
        )));
    }
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() || metadata.len() > MAX_IN_GUEST_INPUT_BYTES {
        return Err(Error::validation(format!(
            "In-guest input '{}' must be a regular file no larger than {} bytes",
            path.display(),
            MAX_IN_GUEST_INPUT_BYTES
        )));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(&canonical)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > MAX_IN_GUEST_INPUT_BYTES {
        return Err(Error::validation(format!(
            "In-guest input '{}' changed or exceeds the file size limit",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
            return Err(Error::validation(format!(
                "In-guest input '{}' changed while it was opened",
                path.display()
            )));
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_IN_GUEST_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IN_GUEST_INPUT_BYTES {
        return Err(Error::validation(format!(
            "In-guest input '{}' exceeds the file size limit",
            path.display()
        )));
    }
    String::from_utf8(bytes).map_err(|err| {
        Error::validation(format!(
            "In-guest input '{}' must be UTF-8 text: {err}",
            path.display()
        ))
    })
}

/// Generated Compose inputs are referenced through their source directory.
/// Keep every parent component real so that reference cannot be redirected
/// through a repository-controlled directory symlink after it is generated.
/// The generated-file operations also pin this parent through directory file
/// descriptors so a concurrent rename cannot redirect a later write or delete.
fn canonical_in_guest_compose_parent(worktree_path: &Path, parent: &Path) -> Result<PathBuf> {
    let worktree = fs::canonicalize(worktree_path)?;
    let relative = parent
        .strip_prefix(worktree_path)
        .or_else(|_| parent.strip_prefix(&worktree))
        .map_err(|_| {
            Error::validation(format!(
                "In-guest Compose parent '{}' must be under the task worktree",
                parent.display()
            ))
        })?;
    let mut current = worktree;
    let mut depth = 0usize;
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if depth > 0 => {
                current.pop();
                depth -= 1;
            }
            std::path::Component::Normal(name) => {
                current.push(name);
                depth += 1;
                let metadata = fs::symlink_metadata(&current)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(Error::validation(format!(
                        "In-guest Compose parent '{}' cannot contain a directory symlink or special file",
                        parent.display()
                    )));
                }
            }
            _ => {
                return Err(Error::validation(format!(
                    "In-guest Compose parent '{}' escapes the task worktree",
                    parent.display()
                )));
            }
        }
    }
    let canonical = fs::canonicalize(parent)?;
    if canonical != current {
        return Err(Error::validation(format!(
            "In-guest Compose parent '{}' changed or contains a directory symlink",
            parent.display()
        )));
    }
    if !canonical.starts_with(fs::canonicalize(worktree_path)?) {
        return Err(Error::validation(format!(
            "In-guest Compose parent '{}' must stay inside the task worktree",
            parent.display()
        )));
    }
    Ok(canonical)
}

fn validate_in_guest_generated_path(worktree_path: &Path, path: &Path) -> Result<()> {
    let worktree = fs::canonicalize(worktree_path)?;
    let parent = fs::canonicalize(
        path.parent()
            .ok_or_else(|| Error::validation("In-guest generated path has no parent"))?,
    )?;
    if !parent.starts_with(worktree) {
        return Err(Error::validation(format!(
            "In-guest generated path '{}' must stay inside the task worktree",
            path.display()
        )));
    }
    let tracked = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
        ])
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(worktree_path)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(path)
        .output()?;
    if tracked.status.success() {
        return Err(Error::validation(format!(
            "In-guest generated path '{}' is tracked by Git and cannot be replaced",
            path.display()
        )));
    }
    if tracked.status.code() != Some(1) {
        return Err(Error::validation(format!(
            "Could not verify in-guest generated path '{}' against Git: {}",
            path.display(),
            tracked.status
        )));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(Error::validation(format!(
            "In-guest generated path '{}' is not a regular file",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn write_in_guest_generated_text_file(
    worktree_path: &Path,
    path: &Path,
    contents: &str,
) -> Result<()> {
    let parent = canonical_in_guest_compose_parent(
        worktree_path,
        path.parent()
            .ok_or_else(|| Error::validation("In-guest generated path has no parent"))?,
    )?;
    let canonical_path = parent.join(
        path.file_name()
            .ok_or_else(|| Error::validation("In-guest generated path has no filename"))?,
    );
    validate_in_guest_generated_path(worktree_path, &canonical_path)?;
    #[cfg(unix)]
    {
        let directory = pin_in_guest_compose_parent(worktree_path, &parent)?;
        return write_in_guest_generated_text_file_at(
            &directory,
            canonical_path
                .file_name()
                .ok_or_else(|| Error::validation("In-guest generated path has no filename"))?,
            contents,
        );
    }
    #[cfg(not(unix))]
    {
        let mut temporary = tempfile::Builder::new()
            .prefix(SBX_COMPOSE_INPUT_PREFIX)
            .tempfile_in(&parent)?;
        temporary.write_all(contents.as_bytes())?;
        temporary
            .persist(&canonical_path)
            .map_err(|err| err.error)?;
        Ok(())
    }
}

fn validate_private_in_guest_generated_path(stage: &Path, path: &Path) -> Result<()> {
    if path.parent() != Some(stage) {
        return Err(Error::validation(
            "Private in-guest generated file must be directly inside its signed stage",
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(Error::validation(format!(
            "Private in-guest generated path '{}' is not a regular file",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn write_private_in_guest_generated_text_file(
    stage: &Path,
    path: &Path,
    contents: &str,
) -> Result<()> {
    validate_private_in_guest_generated_path(stage, path)?;
    #[cfg(unix)]
    {
        let directory = pin_in_guest_compose_parent(stage, stage)?;
        return write_in_guest_generated_text_file_at(
            &directory,
            path.file_name()
                .ok_or_else(|| Error::validation("Private generated path has no filename"))?,
            contents,
        );
    }
    #[cfg(not(unix))]
    {
        let _ = (stage, path, contents);
        Err(Error::validation(
            "Private in-guest Compose staging requires Unix directory handles",
        ))
    }
}

fn write_managed_in_guest_generated_text_file(
    worktree_path: &Path,
    private_stage: Option<&Path>,
    path: &Path,
    contents: &str,
) -> Result<()> {
    if let Some(stage) = private_stage {
        write_private_in_guest_generated_text_file(stage, path, contents)
    } else {
        write_in_guest_generated_text_file(worktree_path, path, contents)
    }
}

#[cfg(unix)]
fn pin_in_guest_compose_parent(worktree_path: &Path, parent: &Path) -> Result<File> {
    let worktree = fs::canonicalize(worktree_path)?;
    let canonical_parent = canonical_in_guest_compose_parent(worktree_path, parent)?;
    let expected = fs::metadata(&canonical_parent)?;
    let relative = canonical_parent.strip_prefix(&worktree).map_err(|_| {
        Error::validation("In-guest Compose parent must stay inside the task worktree")
    })?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut directory = options.open(&worktree)?;
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(Error::validation(
                "In-guest Compose parent has an unsafe component",
            ));
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| Error::validation("In-guest Compose parent contains a NUL byte"))?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error().into());
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    let actual = directory.metadata()?;
    if actual.dev() != expected.dev() || actual.ino() != expected.ino() {
        return Err(Error::validation(format!(
            "In-guest Compose parent '{}' changed while it was opened",
            parent.display()
        )));
    }
    Ok(directory)
}

#[cfg(unix)]
fn write_in_guest_generated_text_file_at(
    directory: &File,
    filename: &std::ffi::OsStr,
    contents: &str,
) -> Result<()> {
    let target = CString::new(filename.as_bytes())
        .map_err(|_| Error::validation("In-guest generated filename contains a NUL byte"))?;
    let temporary = CString::new(format!(
        "{SBX_COMPOSE_INPUT_PREFIX}{}-tmp",
        uuid::Uuid::new_v4()
    ))
    .expect("generated temporary filename has no NUL bytes");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| {
        file.write_all(contents.as_bytes())?;
        let moved = unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                target.as_ptr(),
            )
        };
        if moved < 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    }
    result
}

fn remove_in_guest_generated_file(worktree_path: &Path, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .ok_or_else(|| Error::validation("In-guest generated path has no parent"))?;
        let directory = pin_in_guest_compose_parent(worktree_path, parent)?;
        let filename = path
            .file_name()
            .ok_or_else(|| Error::validation("In-guest generated path has no filename"))?;
        let filename = CString::new(filename.as_bytes())
            .map_err(|_| Error::validation("In-guest generated filename contains a NUL byte"))?;
        if unsafe { libc::unlinkat(directory.as_raw_fd(), filename.as_ptr(), 0) } < 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = worktree_path;
        fs::remove_file(path)?;
        Ok(())
    }
}

fn reject_unsupported_in_guest_compose_tags(value: &serde_yaml::Value) -> Result<()> {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            for (key, value) in mapping {
                reject_unsupported_in_guest_compose_tags(key)?;
                reject_unsupported_in_guest_compose_tags(value)?;
            }
        }
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                reject_unsupported_in_guest_compose_tags(value)?;
            }
        }
        serde_yaml::Value::Tagged(tagged) => {
            if !matches!(tagged.tag.to_string().as_str(), "!reset" | "!override") {
                return Err(Error::validation(format!(
                    "In-guest Compose rejects unsupported YAML tag '{}' because it could load an unsanitized source",
                    tagged.tag
                )));
            }
            reject_unsupported_in_guest_compose_tags(&tagged.value)?;
        }
        _ => {}
    }
    Ok(())
}

struct InGuestComposeSanitization<'a> {
    primary_service: Option<&'a str>,
    service_images: &'a BTreeMap<String, String>,
    omitted_services: &'a BTreeSet<String>,
}

fn prepare_in_guest_compose_inputs(
    worktree_path: &Path,
    references: &[String],
    compose_files: &[PathBuf],
    documents: Vec<serde_yaml::Value>,
    sanitization: &InGuestComposeSanitization<'_>,
    private_stage: Option<&Path>,
) -> Result<Vec<String>> {
    if references.len() != compose_files.len() || references.len() != documents.len() {
        return Err(Error::config(
            "In-guest Compose sources and validated documents must have the same length",
        ));
    }
    let worktree = fs::canonicalize(worktree_path).map_err(|err| {
        Error::validation(format!(
            "Cannot resolve in-guest task worktree '{}': {err}",
            worktree_path.display()
        ))
    })?;
    let mut sanitized_references = Vec::with_capacity(references.len());
    for (index, ((reference, compose_file), mut document)) in references
        .iter()
        .zip(compose_files)
        .zip(documents)
        .enumerate()
    {
        // The generated names were excluded before any facade file was written.
        let canonical_source = fs::canonicalize(compose_file).map_err(|err| {
            Error::validation(format!(
                "Cannot resolve in-guest Compose source '{}': {err}",
                compose_file.display()
            ))
        })?;
        let canonical_parent = canonical_in_guest_compose_parent(
            worktree_path,
            compose_file
                .parent()
                .ok_or_else(|| Error::validation("In-guest Compose source has no parent"))?,
        )
        .map_err(|err| {
            Error::validation(format!(
                "Cannot resolve parent of in-guest Compose source '{}': {err}",
                compose_file.display()
            ))
        })?;
        if !canonical_source.starts_with(&worktree) || !canonical_parent.starts_with(&worktree) {
            return Err(Error::validation(format!(
                "In-guest Compose source '{}' must be inside the task worktree",
                compose_file.display()
            )));
        }
        sanitize_in_guest_compose_source(&mut document, sanitization)?;
        let generated_name = format!("{SBX_COMPOSE_INPUT_PREFIX}{index}.yaml");
        let generated_path = private_stage
            .unwrap_or(&canonical_parent)
            .join(&generated_name);
        let generated_reference = if private_stage.is_some() {
            PathBuf::from(&generated_name)
        } else {
            Path::new(reference).with_file_name(&generated_name)
        };
        if let Some(stage) = private_stage {
            validate_private_in_guest_generated_path(stage, &generated_path)?;
        } else {
            validate_in_guest_generated_path(worktree_path, &generated_path)?;
        }
        if generated_path.exists() {
            let previous = read_in_guest_worktree_file(
                private_stage.unwrap_or(worktree_path),
                &generated_path,
            )?;
            if !previous.starts_with(SBX_COMPOSE_INPUT_MARKER) {
                return Err(Error::validation(format!(
                    "In-guest Compose facade path '{}' contains a non-BranchBox file",
                    generated_path.display()
                )));
            }
        }
        let rendered = serde_yaml::to_string(&document).map_err(|err| {
            Error::config(format!(
                "Could not serialize sanitized in-guest Compose input: {err}"
            ))
        })?;
        // The pinned directory keeps the copy inside the validated worktree
        // even if a workspace consumer swaps this path concurrently.
        write_managed_in_guest_generated_text_file(
            worktree_path,
            private_stage,
            &generated_path,
            &format!("{SBX_COMPOSE_INPUT_MARKER}{rendered}"),
        )?;
        sanitized_references.push(generated_reference.to_string_lossy().into_owned());
    }
    Ok(sanitized_references)
}

fn sanitize_in_guest_compose_source(
    document: &mut serde_yaml::Value,
    sanitization: &InGuestComposeSanitization<'_>,
) -> Result<()> {
    let root = document
        .as_mapping_mut()
        .ok_or_else(|| Error::validation("In-guest Compose source must be a YAML mapping"))?;
    for key in ["volumes", "secrets", "configs"] {
        root.remove(serde_yaml::Value::String(key.to_string()));
    }
    if let Some(services) = root.get_mut(serde_yaml::Value::String("services".to_string())) {
        let services = services
            .as_mapping_mut()
            .ok_or_else(|| Error::validation("In-guest Compose services must be a YAML mapping"))?;
        for (name, service) in services {
            let name_string = name.as_str().unwrap_or("<invalid name>");
            if sanitization.omitted_services.contains(name_string) {
                // The connector is disabled by the final facade. Keep only a
                // harmless image so Compose accepts its profiled-out service;
                // none of its repository command, entrypoint, image, or other
                // fields may be interpolated before that facade is merged.
                let mut inert = serde_yaml::Mapping::new();
                inert.insert(
                    serde_yaml::Value::String("image".to_string()),
                    serde_yaml::Value::String(IN_GUEST_OMITTED_CONNECTOR_IMAGE.to_string()),
                );
                *service = serde_yaml::Value::Mapping(inert);
                continue;
            }
            let service = service.as_mapping_mut().ok_or_else(|| {
                Error::validation(format!(
                    "In-guest Compose service '{}' must be a YAML mapping",
                    name_string
                ))
            })?;
            // Every repository value that could mount a host path, read a host
            // env file, or publish a port is removed before Compose interpolates
            // this file. The generated final facade supplies only signed mounts.
            for key in [
                "volumes",
                "ports",
                "expose",
                "env_file",
                "label_file",
                "develop",
                "credential_spec",
                "devices",
                "secrets",
                "configs",
            ] {
                service.remove(serde_yaml::Value::String(key.to_string()));
            }
            if sanitization.primary_service == Some(name_string) {
                service.remove(serde_yaml::Value::String("environment".to_string()));
            }
            if let Some(environment) = service.get("environment") {
                reject_in_guest_compose_environment_passthrough(environment)?;
            }
            if !sanitization.service_images.contains_key(name_string) {
                if let Some(args) = service
                    .get("build")
                    .map(untag_in_guest_compose_value)
                    .and_then(|build| build.get("args"))
                {
                    reject_in_guest_compose_build_arg_passthrough(args)?;
                }
            }
            if let Some(dependencies) =
                service.get_mut(serde_yaml::Value::String("depends_on".to_string()))
            {
                // Compose merges dependencies across the ordered input files. Filter
                // disabled connectors in each copy instead of deriving a final
                // replacement from one service definition (which may only add a
                // command in a later file).
                filter_in_guest_omitted_dependencies(dependencies, sanitization.omitted_services);
            }
            if sanitization.service_images.contains_key(name_string) {
                for key in ["image", "build", "pull_policy"] {
                    service.remove(serde_yaml::Value::String(key.to_string()));
                }
            }
        }
    }
    remove_compose_extensions(document);
    // Compose interpolates each source before merging the signed facade. Even
    // when mount-related fields are gone, a retained dependency environment,
    // label, or command could still copy a runtime-process secret into Docker.
    reject_ambient_compose_interpolation(document)?;
    Ok(())
}

fn reject_ambient_compose_interpolation(value: &serde_yaml::Value) -> Result<()> {
    match value {
        serde_yaml::Value::String(value) if contains_unescaped_compose_variable(value) => {
            // cause-withheld: the untrusted scalar can contain credential material.
            Err(Error::validation(
                "In-guest Compose rejects ambient variable interpolation in a Compose field; use a fixed value or escape '$' as '$$' for container-side expansion",
            ))
        }
        serde_yaml::Value::Mapping(mapping) => {
            for (key, value) in mapping {
                reject_ambient_compose_interpolation(key)?;
                reject_ambient_compose_interpolation(value)?;
            }
            Ok(())
        }
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                reject_ambient_compose_interpolation(value)?;
            }
            Ok(())
        }
        serde_yaml::Value::Tagged(tagged) => reject_ambient_compose_interpolation(&tagged.value),
        _ => Ok(()),
    }
}

fn untag_in_guest_compose_value(mut value: &serde_yaml::Value) -> &serde_yaml::Value {
    while let serde_yaml::Value::Tagged(tagged) = value {
        value = &tagged.value;
    }
    value
}

fn reject_in_guest_compose_environment_passthrough(value: &serde_yaml::Value) -> Result<()> {
    reject_in_guest_compose_passthrough(value, "environment")
}

fn reject_in_guest_compose_build_arg_passthrough(value: &serde_yaml::Value) -> Result<()> {
    reject_in_guest_compose_passthrough(value, "build.args")
}

fn reject_in_guest_compose_passthrough(value: &serde_yaml::Value, field: &str) -> Result<()> {
    let passthrough = match untag_in_guest_compose_value(value) {
        serde_yaml::Value::Mapping(entries) => entries.values().any(serde_yaml::Value::is_null),
        serde_yaml::Value::Sequence(entries) => entries
            .iter()
            .filter_map(serde_yaml::Value::as_str)
            .any(|entry| !entry.contains('=')),
        _ => false,
    };
    if passthrough {
        // cause-withheld: an untrusted variable name can identify credential material.
        return Err(Error::validation(format!(
            "In-guest Compose rejects valueless {field} entries because Compose can inherit ambient host variables"
        )));
    }
    Ok(())
}

fn contains_unescaped_compose_variable(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'$' {
            if bytes.get(index + 1) == Some(&b'$') {
                index += 2;
                continue;
            }
            if bytes
                .get(index + 1)
                .is_some_and(|next| *next == b'{' || next.is_ascii_alphabetic() || *next == b'_')
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn filter_in_guest_omitted_dependencies(
    dependencies: &mut serde_yaml::Value,
    omitted: &BTreeSet<String>,
) {
    match dependencies {
        serde_yaml::Value::Sequence(values) => {
            values.retain(|value| value.as_str().is_none_or(|name| !omitted.contains(name)));
        }
        serde_yaml::Value::Mapping(values) => {
            values.retain(|name, _| name.as_str().is_none_or(|name| !omitted.contains(name)));
        }
        serde_yaml::Value::Tagged(tagged) => {
            filter_in_guest_omitted_dependencies(&mut tagged.value, omitted);
        }
        _ => {}
    }
}

fn remove_compose_extensions(value: &mut serde_yaml::Value) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            mapping.retain(|key, _| !key.as_str().is_some_and(|key| key.starts_with("x-")));
            for value in mapping.values_mut() {
                remove_compose_extensions(value);
            }
        }
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                remove_compose_extensions(value);
            }
        }
        serde_yaml::Value::Tagged(tagged) => remove_compose_extensions(&mut tagged.value),
        _ => {}
    }
}

fn sanitize_in_guest_devcontainer_json(
    value: &mut serde_json::Value,
    security_option_in_devcontainer: bool,
    preloaded_image_mode: bool,
) -> Result<()> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::validation("Devcontainer configuration must be a JSON object"))?;
    object.remove("initializeCommand");
    if let Some(environment) = object
        .get_mut("containerEnv")
        .and_then(serde_json::Value::as_object_mut)
    {
        environment.retain(|name, value| {
            value
                .as_str()
                .is_some_and(|value| is_safe_nonsecret_container_environment(name, value))
        });
    }
    object.remove("remoteEnv");
    object.remove("workspaceMount");
    object.remove("mounts");
    object.remove("appPort");
    object.remove("forwardPorts");
    object.remove("portsAttributes");
    object.remove("otherPortsAttributes");
    object.remove("capAdd");
    if security_option_in_devcontainer {
        object.insert(
            "securityOpt".to_string(),
            serde_json::json!([IN_GUEST_SECCOMP_SECURITY_OPTION]),
        );
    } else {
        // Compose devcontainers receive the same mandatory option from the generated service
        // facade. Repeating it in devcontainer.json makes the CLI generate a later Compose
        // fragment with the same list entry, which Compose rejects as a duplicate.
        object.remove("securityOpt");
    }
    object.insert("privileged".to_string(), serde_json::Value::Bool(false));

    if preloaded_image_mode {
        // The assigned image already contains the complete devcontainer toolchain. Features,
        // Dockerfile configuration, and UID rewriting make the Dev Containers CLI derive a new
        // image, which would violate the no-build assignment contract.
        object.remove("features");
        object.remove("build");
        object.insert(
            "updateRemoteUserUID".to_string(),
            serde_json::Value::Bool(false),
        );
    }

    if let Some(features) = object
        .get_mut("features")
        .and_then(serde_json::Value::as_object_mut)
    {
        features.retain(|feature, _| !is_project_docker_feature(feature));
    }
    if let Some(build) = object
        .get_mut("build")
        .and_then(serde_json::Value::as_object_mut)
    {
        build.remove("args");
        for forbidden in ["ssh", "secrets", "privileged", "entitlements"] {
            if build.contains_key(forbidden) {
                return Err(Error::validation(format!(
                    "In-guest devcontainer rejects build.{forbidden}"
                )));
            }
        }
    }
    if let Some(run_args) = object
        .get_mut("runArgs")
        .and_then(serde_json::Value::as_array_mut)
    {
        let mut sanitized = Vec::new();
        let mut index = 0;
        while index < run_args.len() {
            let argument = run_args[index].as_str().unwrap_or_default();
            if matches!(
                argument,
                "--env-file"
                    | "--volume"
                    | "-v"
                    | "--mount"
                    | "--shm-size"
                    | "--device"
                    | "--cap-add"
                    | "--security-opt"
                    | "--network"
                    | "--network-mode"
                    | "--pid"
                    | "--ipc"
                    | "--cgroupns"
            ) {
                index += 2;
                continue;
            }
            let normalized = argument.to_ascii_lowercase();
            if normalized.starts_with("--env-file=")
                || normalized.starts_with("--volume=")
                || normalized.starts_with("--mount=")
                || normalized.starts_with("--shm-size=")
                || normalized.starts_with("--device=")
                || normalized.starts_with("--cap-add=")
                || normalized.starts_with("--security-opt=")
                || normalized.starts_with("--network=")
                || normalized.starts_with("--network-mode=")
                || normalized.starts_with("--pid=")
                || normalized.starts_with("--ipc=")
                || normalized.starts_with("--cgroupns=")
                || normalized == "--ipc=host"
                || normalized == "--pid=host"
                || normalized == "--privileged"
                || contains_supervisor_reference(argument)
            {
                index += 1;
                continue;
            }
            sanitized.push(run_args[index].clone());
            index += 1;
        }
        *run_args = sanitized;
    }
    Ok(())
}

fn is_project_docker_feature(feature: &str) -> bool {
    let normalized = feature.to_ascii_lowercase();
    [
        "docker-outside-of-docker",
        "docker-in-docker",
        "docker-from-docker",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn is_safe_nonsecret_container_environment(name: &str, value: &str) -> bool {
    let name = name.to_ascii_uppercase();
    let connectivity_key = name == "HOST"
        || name == "PORT"
        || name.ends_with("_HOST")
        || name.ends_with("_PORT")
        || matches!(
            name.as_str(),
            "RAILS_ENV" | "RACK_ENV" | "NODE_ENV" | "APP_ENV"
        );
    let platform_or_secret = [
        "TOKEN",
        "PASSWORD",
        "SECRET",
        "CREDENTIAL",
        "AUTH",
        "API_KEY",
        "PRIVATE_KEY",
        "TUNNEL",
        "DEV_HOSTNAME",
        "DOCKER",
    ]
    .iter()
    .any(|fragment| name.contains(fragment));
    connectivity_key
        && !platform_or_secret
        && !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
        })
}

fn reject_supervisor_socket_references_yaml(value: &serde_yaml::Value) -> Result<()> {
    match value {
        serde_yaml::Value::String(value) => {
            if contains_supervisor_socket_reference(value) {
                return Err(Error::validation(
                    "In-guest Compose may not reference a Docker or containerd supervisor socket",
                ));
            }
            Ok(())
        }
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                reject_supervisor_socket_references_yaml(value)?;
            }
            Ok(())
        }
        serde_yaml::Value::Mapping(values) => {
            for (key, value) in values {
                if key
                    .as_str()
                    .is_some_and(contains_supervisor_socket_reference)
                {
                    return Err(Error::validation(
                        "In-guest Compose may not configure container supervisor authority",
                    ));
                }
                reject_supervisor_socket_references_yaml(value)?;
            }
            Ok(())
        }
        serde_yaml::Value::Tagged(value) => reject_supervisor_socket_references_yaml(&value.value),
        _ => Ok(()),
    }
}

fn validate_in_guest_compose_security(
    document: &serde_yaml::Value,
    primary_service: Option<&str>,
    devcontainer_dir: &Path,
    worktree_path: &Path,
    service_images: &BTreeMap<String, String>,
) -> Result<()> {
    if document.get("include").is_some() {
        return Err(Error::validation(
            "In-guest Compose rejects include because it can load unsanitized service definitions",
        ));
    }
    if document
        .get("volumes")
        .and_then(serde_yaml::Value::as_mapping)
        .is_some_and(|volumes| {
            volumes.values().any(|volume| {
                volume
                    .get("external")
                    .and_then(serde_yaml::Value::as_bool)
                    .unwrap_or(false)
            })
        })
    {
        return Err(Error::validation(
            "In-guest Compose may not attach external volumes shared with the supervisor",
        ));
    }
    let Some(services) = document
        .get("services")
        .and_then(serde_yaml::Value::as_mapping)
    else {
        return Ok(());
    };
    for (name, service) in services {
        let name = name.as_str().unwrap_or_default();
        let connector = is_platform_connector(name, service);
        if service.get("provider").is_some() {
            return Err(Error::validation(format!(
                "In-guest Compose rejects service '{name}' provider because Compose runs its binary on the guest host"
            )));
        }
        if service.get("extends").is_some() {
            return Err(Error::validation(format!(
                "In-guest Compose rejects service '{name}' extends because inherited host mounts cannot be proven safe"
            )));
        }
        if service.get("volumes_from").is_some() {
            return Err(Error::validation(format!(
                "In-guest Compose rejects service '{name}' volumes_from because it can inherit host mounts"
            )));
        }
        // A signed preloaded image makes repository build instructions unreachable:
        // the sanitized input removes build before Compose interpolation, and the
        // final facade resets it. Other services still require full build checks.
        if let Some(build) = service
            .get("build")
            .and_then(serde_yaml::Value::as_mapping)
            .filter(|_| !service_images.contains_key(name))
        {
            for forbidden in [
                "ssh",
                "secrets",
                "privileged",
                "entitlements",
                "additional_contexts",
            ] {
                if build.contains_key(serde_yaml::Value::String(forbidden.to_string())) {
                    return Err(Error::validation(format!(
                        "In-guest Compose rejects build.{forbidden} for service '{name}'"
                    )));
                }
            }
            if build
                .get("network")
                .and_then(serde_yaml::Value::as_str)
                .is_some_and(|network| network == "host")
            {
                return Err(Error::validation(format!(
                    "In-guest Compose rejects host-network builds for service '{name}'"
                )));
            }
            if build
                .get("context")
                .and_then(serde_yaml::Value::as_str)
                .is_some_and(|context| {
                    if context.starts_with('/') || context.contains("${") {
                        return true;
                    }
                    match (
                        fs::canonicalize(devcontainer_dir.join(context)),
                        fs::canonicalize(worktree_path.parent().unwrap_or(worktree_path)),
                    ) {
                        (Ok(resolved), Ok(workspace)) => !resolved.starts_with(workspace),
                        _ => true,
                    }
                })
            {
                return Err(Error::validation(format!(
                    "In-guest Compose rejects ambient/absolute build context for service '{name}'"
                )));
            }
        }
        if !connector {
            let privileged = service
                .get("privileged")
                .and_then(serde_yaml::Value::as_bool)
                .unwrap_or(false);
            let host_namespace = ["pid", "ipc", "network_mode", "cgroup"].iter().any(|key| {
                service
                    .get(*key)
                    .and_then(serde_yaml::Value::as_str)
                    .is_some_and(|value| value == "host")
            });
            let elevated_capability = service
                .get("cap_add")
                .and_then(serde_yaml::Value::as_sequence)
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
            if privileged || host_namespace || elevated_capability {
                return Err(Error::validation(format!(
                    "In-guest Compose rejects privileged host authority for service '{name}'"
                )));
            }
            if service.get("secrets").is_some() || service.get("configs").is_some() {
                return Err(Error::validation(format!(
                    "In-guest Compose requires service '{name}' secrets/configs to use manifest materializations"
                )));
            }
            if service
                .get("volumes")
                .and_then(serde_yaml::Value::as_sequence)
                .is_some_and(|volumes| {
                    volumes.iter().any(|volume| {
                        volume.as_str().is_some_and(|volume| {
                            let source = volume.split(':').next().unwrap_or_default();
                            source.starts_with('/') || source.starts_with('~')
                        }) || volume.as_mapping().is_some_and(|volume| {
                            volume
                                .get(serde_yaml::Value::String("source".to_string()))
                                .and_then(serde_yaml::Value::as_str)
                                .is_some_and(|source| {
                                    source.starts_with('/') || source.starts_with('~')
                                })
                        })
                    })
                })
                && primary_service != Some(name)
            {
                return Err(Error::validation(format!(
                    "In-guest Compose rejects ambient host paths for secondary service '{name}'"
                )));
            }
        }
    }
    Ok(())
}

fn contains_supervisor_socket_reference(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    [
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

fn contains_supervisor_reference(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    contains_supervisor_socket_reference(&normalized)
        || normalized.contains("/run/agentify-assignment")
        || normalized.contains("/run/agentify-runtime")
        || normalized.contains("/run/branchbox/managed")
}

fn effective_in_guest_workspace_folder(
    config: &DevcontainerConfig,
    worktree_path: &Path,
) -> Result<String> {
    let basename = worktree_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let folder = config.effective_workspace_folder(basename);
    if contains_unescaped_compose_variable(&folder) {
        // cause-withheld: the untrusted folder can contain credential material.
        return Err(Error::validation(
            "In-guest workspaceFolder rejects ambient variable interpolation because Compose expands generated mount targets",
        ));
    }
    let path = Path::new(&folder);
    if !path.is_absolute()
        || path == Path::new("/workspaces")
        || !path.starts_with("/workspaces")
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || contains_supervisor_reference(&folder)
    {
        return Err(Error::validation(
            "In-guest workspaceFolder must be a normalized path below the platform-owned /workspaces root",
        ));
    }
    Ok(folder)
}

struct InGuestComposeAssignment<'a> {
    project_environment: Option<&'a Path>,
    service_images: &'a BTreeMap<String, String>,
    /// Signed read-only lease binds. These are placed in the generated Compose
    /// facade rather than devcontainer.json `mounts`, because the Dev Containers
    /// CLI does not carry the `readonly` property into a Compose project, and the
    /// running container is then inspected for a read-only bind and fails closed.
    lease_mounts: &'a [(PathBuf, PathBuf)],
    /// Signed tool-request spool volumes, as (volume name, target). Like the lease
    /// binds these belong in the Compose facade: declared through devcontainer.json
    /// `mounts`, the Dev Containers CLI hands Compose an ordinary volume, which
    /// Compose then namespaces with the project name. The container is afterwards
    /// inspected for the signed volume name and fails closed on the prefixed one.
    spool_volumes: &'a [(String, PathBuf)],
    /// A signed version 3 assignment group-shares the task worktree with a consumer that does
    /// not own it, so the primary service needs the matching Git ownership exceptions.
    workspace_consumer: bool,
    private_stage: Option<&'a Path>,
}

fn prepare_outer_tunnel_compose_override(
    repo_root: &Path,
    worktree_path: &Path,
    output_dir: &Path,
    primary_service: Option<&str>,
    workspace_folder: &str,
    compose_documents: &[serde_yaml::Value],
    assignment: &InGuestComposeAssignment<'_>,
) -> Result<std::collections::BTreeSet<String>> {
    use serde_yaml::value::{Tag, TaggedValue};

    let mut definitions = BTreeMap::new();
    let mut omitted = std::collections::BTreeSet::new();
    // This must use the same validated snapshot that is later sanitized into CLI inputs.
    // A running workspace consumer can rewrite source files during preparation; rereading
    // here could approve a different service set than the one staged for Compose.
    for document in compose_documents {
        if !assignment.service_images.is_empty() && document.get("include").is_some() {
            return Err(Error::validation(
                "Preloaded service image assignments reject Compose include because indirect services cannot be bound exactly",
            ));
        }
        let Some(services) = document
            .get("services")
            .and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };
        for (name, service) in services {
            let Some(name) = name.as_str() else {
                continue;
            };
            definitions.insert(name.to_string(), service.clone());
            if is_platform_connector(name, service) {
                omitted.insert(name.to_string());
            }
        }
    }
    validate_preloaded_service_image_coverage(
        &definitions,
        &omitted,
        primary_service,
        assignment.service_images,
    )?;
    let override_path = output_dir.join(SBX_COMPOSE_OVERRIDE);
    let mut document = if override_path.is_file() {
        serde_yaml::from_str::<serde_yaml::Value>(&read_in_guest_worktree_file(
            assignment.private_stage.unwrap_or(worktree_path),
            &override_path,
        )?)
        .map_err(|err| Error::config(format!("Invalid generated Compose facade: {err}")))?
    } else {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    };
    let document_mapping = document
        .as_mapping_mut()
        .ok_or_else(|| Error::config("Generated Compose facade must be a mapping"))?;
    let services_key = serde_yaml::Value::String("services".to_string());
    let services = document_mapping
        .entry(services_key)
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
        .as_mapping_mut()
        .ok_or_else(|| Error::config("Generated Compose facade services must be a mapping"))?;

    for name in &omitted {
        let service = services
            .entry(serde_yaml::Value::String(name.clone()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            .as_mapping_mut()
            .ok_or_else(|| Error::config("Generated Compose service facade must be a mapping"))?;
        service.insert(
            serde_yaml::Value::String("profiles".to_string()),
            serde_yaml::Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("!override"),
                value: serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                    "branchbox-outer-tunnel-disabled".to_string(),
                )]),
            })),
        );
        for key in ["env_file", "volumes", "devices", "cap_add"] {
            service.insert(
                serde_yaml::Value::String(key.to_string()),
                serde_yaml::Value::Tagged(Box::new(TaggedValue {
                    tag: Tag::new("!override"),
                    value: serde_yaml::Value::Sequence(Vec::new()),
                })),
            );
        }
        service.insert(
            serde_yaml::Value::String("environment".to_string()),
            serde_yaml::Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("!override"),
                value: serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
            })),
        );
    }

    for name in definitions.keys() {
        let service = services
            .entry(serde_yaml::Value::String(name.clone()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            .as_mapping_mut()
            .ok_or_else(|| Error::config("Generated Compose service facade must be a mapping"))?;
        for key in [
            "env_file",
            "volumes",
            "ports",
            "expose",
            "devices",
            "secrets",
            "configs",
            "volumes_from",
        ] {
            service.insert(
                serde_yaml::Value::String(key.to_string()),
                serde_yaml::Value::Tagged(Box::new(TaggedValue {
                    tag: Tag::new("!override"),
                    value: serde_yaml::Value::Sequence(Vec::new()),
                })),
            );
        }
    }

    for (name, image) in assignment.service_images {
        let service = services
            .entry(serde_yaml::Value::String(name.clone()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            .as_mapping_mut()
            .ok_or_else(|| Error::config("Generated Compose service facade must be a mapping"))?;
        service.insert(
            serde_yaml::Value::String("image".to_string()),
            serde_yaml::Value::String(image.clone()),
        );
        service.insert(
            serde_yaml::Value::String("build".to_string()),
            serde_yaml::Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("!reset"),
                value: serde_yaml::Value::Null,
            })),
        );
        service.insert(
            serde_yaml::Value::String("pull_policy".to_string()),
            serde_yaml::Value::String("never".to_string()),
        );
    }

    if let Some(primary_service) = primary_service {
        let primary = services
            .entry(serde_yaml::Value::String(primary_service.to_string()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            .as_mapping_mut()
            .ok_or_else(|| Error::config("Generated primary service facade must be a mapping"))?;
        primary.insert(
            serde_yaml::Value::String("environment".to_string()),
            serde_yaml::Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("!override"),
                value: serde_yaml::Value::Mapping(in_guest_primary_environment(
                    assignment.workspace_consumer,
                    workspace_folder,
                )),
            })),
        );
        primary.insert(
            serde_yaml::Value::String("shm_size".to_string()),
            serde_yaml::Value::String(IN_GUEST_SHM_SIZE.to_string()),
        );
        primary.insert(
            serde_yaml::Value::String("security_opt".to_string()),
            serde_yaml::Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("!override"),
                value: serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                    IN_GUEST_SECCOMP_SECURITY_OPTION.to_string(),
                )]),
            })),
        );
        let env_files = assignment
            .project_environment
            .map(|source| {
                let mut entry = serde_yaml::Mapping::new();
                entry.insert(
                    serde_yaml::Value::String("path".to_string()),
                    serde_yaml::Value::String(source.to_string_lossy().into_owned()),
                );
                entry.insert(
                    serde_yaml::Value::String("required".to_string()),
                    serde_yaml::Value::Bool(true),
                );
                entry.insert(
                    serde_yaml::Value::String("format".to_string()),
                    serde_yaml::Value::String("raw".to_string()),
                );
                vec![serde_yaml::Value::Mapping(entry)]
            })
            .unwrap_or_default();
        primary.insert(
            serde_yaml::Value::String("env_file".to_string()),
            serde_yaml::Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("!override"),
                value: serde_yaml::Value::Sequence(env_files),
            })),
        );
        let mut safe_volumes =
            in_guest_primary_volumes(repo_root, worktree_path, workspace_folder)?;
        safe_volumes.extend(assignment.lease_mounts.iter().map(|(source, target)| {
            let mut mount = serde_yaml::Mapping::new();
            mount.insert(
                serde_yaml::Value::String("type".to_string()),
                serde_yaml::Value::String("bind".to_string()),
            );
            mount.insert(
                serde_yaml::Value::String("source".to_string()),
                serde_yaml::Value::String(source.to_string_lossy().into_owned()),
            );
            mount.insert(
                serde_yaml::Value::String("target".to_string()),
                serde_yaml::Value::String(target.to_string_lossy().into_owned()),
            );
            mount.insert(
                serde_yaml::Value::String("read_only".to_string()),
                serde_yaml::Value::Bool(true),
            );
            serde_yaml::Value::Mapping(mount)
        }));
        safe_volumes.extend(assignment.spool_volumes.iter().map(|(volume, target)| {
            let mut mount = serde_yaml::Mapping::new();
            mount.insert(
                serde_yaml::Value::String("type".to_string()),
                serde_yaml::Value::String("volume".to_string()),
            );
            mount.insert(
                serde_yaml::Value::String("source".to_string()),
                serde_yaml::Value::String(volume.clone()),
            );
            mount.insert(
                serde_yaml::Value::String("target".to_string()),
                serde_yaml::Value::String(target.to_string_lossy().into_owned()),
            );
            serde_yaml::Value::Mapping(mount)
        }));
        primary.insert(
            serde_yaml::Value::String("volumes".to_string()),
            serde_yaml::Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("!override"),
                value: serde_yaml::Value::Sequence(safe_volumes),
            })),
        );
    }

    if !assignment.spool_volumes.is_empty() {
        // A top-level volume carrying an explicit `name` is created under exactly
        // that name; without it Compose prefixes the project name and the signed
        // volume no longer matches what the container reports.
        let document_mapping = document
            .as_mapping_mut()
            .ok_or_else(|| Error::config("Generated Compose facade must be a mapping"))?;
        let volumes = document_mapping
            .entry(serde_yaml::Value::String("volumes".to_string()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            .as_mapping_mut()
            .ok_or_else(|| Error::config("Generated Compose facade volumes must be a mapping"))?;
        for (volume, _target) in assignment.spool_volumes {
            let mut definition = serde_yaml::Mapping::new();
            definition.insert(
                serde_yaml::Value::String("name".to_string()),
                serde_yaml::Value::String(volume.clone()),
            );
            volumes.insert(
                serde_yaml::Value::String(volume.clone()),
                serde_yaml::Value::Mapping(definition),
            );
        }
    }
    // Paths and names supplied by the signed assignment or the task checkout
    // also become Compose values. Refuse interpolation in this generated file,
    // even when every repository source was sanitized.
    reject_ambient_compose_interpolation(&document)?;
    let rendered = serde_yaml::to_string(&document)
        .map_err(|err| Error::config(format!("Failed to serialize Compose facade: {err}")))?;
    write_managed_in_guest_generated_text_file(
        worktree_path,
        assignment.private_stage,
        &override_path,
        &rendered,
    )?;
    Ok(omitted)
}

fn validate_preloaded_service_image_coverage(
    definitions: &BTreeMap<String, serde_yaml::Value>,
    omitted: &std::collections::BTreeSet<String>,
    primary_service: Option<&str>,
    service_images: &BTreeMap<String, String>,
) -> Result<()> {
    if service_images.is_empty() {
        return Ok(());
    }
    let primary = primary_service.ok_or_else(|| {
        Error::validation(
            "Preloaded service images require a Compose-backed primary devcontainer service",
        )
    })?;
    let runnable = definitions
        .keys()
        .filter(|name| !omitted.contains(*name))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if !runnable.contains(primary) {
        return Err(Error::validation(format!(
            "Preloaded service image assignment cannot resolve primary Compose service '{primary}'"
        )));
    }
    let assigned = service_images
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let missing = runnable.difference(&assigned).cloned().collect::<Vec<_>>();
    let unknown = assigned.difference(&runnable).cloned().collect::<Vec<_>>();
    if !missing.is_empty() || !unknown.is_empty() {
        let mut details = Vec::new();
        if !missing.is_empty() {
            details.push(format!("missing: {}", missing.join(", ")));
        }
        if !unknown.is_empty() {
            details.push(format!("unknown or disabled: {}", unknown.join(", ")));
        }
        return Err(Error::validation(format!(
            "Preloaded service images must bind every runnable Compose service exactly ({})",
            details.join("; ")
        )));
    }
    Ok(())
}

fn is_platform_connector(name: &str, service: &serde_yaml::Value) -> bool {
    let name = name.to_ascii_lowercase();
    let image = service
        .get("image")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let requires_tun = service.get("devices").is_some_and(|devices| {
        serde_yaml::to_string(devices).is_ok_and(|rendered| rendered.contains("/dev/net/tun"))
    });
    name == "cloudflared"
        || name == "tailscale"
        || name == "ngrok"
        || name == "tunnel"
        || name.ends_with("-tunnel")
        || image.contains("cloudflare/cloudflared")
        || image.contains("tailscale/tailscale")
        || image.contains("ngrok/ngrok")
        || requires_tun
}

/// The primary service's environment always replaces the repository's. A signed workspace
/// consumer additionally receives the two exact Git ownership exceptions it needs: the task
/// worktree stays owned by the unprivileged BranchBox runtime UID and is only group-shared, so
/// Git would otherwise refuse every command with `detected dubious ownership`. Only these exact
/// platform-owned paths are excepted, never a wildcard, and BranchBox alone can set them because
/// project and provider environments both reserve the `GIT_CONFIG_` prefix.
fn in_guest_primary_environment(
    workspace_consumer: bool,
    workspace_folder: &str,
) -> serde_yaml::Mapping {
    let mut environment = serde_yaml::Mapping::new();
    if !workspace_consumer {
        return environment;
    }
    let exceptions = [workspace_folder, CONTAINER_MAIN_GIT_TARGET];
    environment.insert(
        serde_yaml::Value::String("GIT_CONFIG_COUNT".to_string()),
        serde_yaml::Value::String(exceptions.len().to_string()),
    );
    for (index, path) in exceptions.iter().enumerate() {
        environment.insert(
            serde_yaml::Value::String(format!("GIT_CONFIG_KEY_{index}")),
            serde_yaml::Value::String("safe.directory".to_string()),
        );
        environment.insert(
            serde_yaml::Value::String(format!("GIT_CONFIG_VALUE_{index}")),
            serde_yaml::Value::String((*path).to_string()),
        );
    }
    environment
}

fn in_guest_primary_volumes(
    repo_root: &Path,
    worktree_path: &Path,
    workspace_folder: &str,
) -> Result<Vec<serde_yaml::Value>> {
    let authoritative_worktree = fs::canonicalize(worktree_path).map_err(|err| {
        Error::validation(format!(
            "Cannot prepare in-guest task worktree facade from '{}': {err}",
            worktree_path.display()
        ))
    })?;
    let mut workspace = serde_yaml::Mapping::new();
    workspace.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("bind".to_string()),
    );
    workspace.insert(
        serde_yaml::Value::String("source".to_string()),
        serde_yaml::Value::String(authoritative_worktree.to_string_lossy().into_owned()),
    );
    workspace.insert(
        serde_yaml::Value::String("target".to_string()),
        serde_yaml::Value::String(workspace_folder.to_string()),
    );

    let authoritative_git = repository_common_git_dir(repo_root)?;
    let mut git = serde_yaml::Mapping::new();
    git.insert(
        serde_yaml::Value::String("type".to_string()),
        serde_yaml::Value::String("bind".to_string()),
    );
    git.insert(
        serde_yaml::Value::String("source".to_string()),
        serde_yaml::Value::String(authoritative_git.to_string_lossy().into_owned()),
    );
    git.insert(
        serde_yaml::Value::String("target".to_string()),
        serde_yaml::Value::String(CONTAINER_MAIN_GIT_TARGET.to_string()),
    );
    Ok(vec![
        serde_yaml::Value::Mapping(workspace),
        serde_yaml::Value::Mapping(git),
    ])
}

fn prepare_sbx_devcontainer_config(
    repo_root: &Path,
    worktree_path: &Path,
    run_services: &[String],
) -> Result<()> {
    let (config, config_path) = DevcontainerConfig::load(worktree_path)
        .map_err(|err| Error::validation(format!("Could not inspect SBX devcontainer: {err}")))?;
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
        .filter(|path| devcontainer_dir.join(path).exists())
        .map(|path| (*path).to_string())
        .collect(),
    };
    let compose_files: Vec<PathBuf> = compose_references
        .iter()
        .map(|path| devcontainer_dir.join(path))
        .collect();
    let mut unsupported_services = Vec::new();
    for compose_file in &compose_files {
        let Ok(content) = fs::read_to_string(compose_file) else {
            continue;
        };
        let Ok(document) = serde_yaml::from_str::<serde_yaml::Value>(&content) else {
            continue;
        };
        let Some(services) = document
            .get("services")
            .and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };
        for (name, service) in services {
            let Some(name) = name.as_str() else {
                continue;
            };
            let requires_tun = service
                .get("devices")
                .and_then(serde_yaml::Value::as_sequence)
                .is_some_and(|devices| {
                    devices.iter().any(|device| {
                        device
                            .as_str()
                            .is_some_and(|value| value.contains("/dev/net/tun"))
                            || device.as_mapping().is_some_and(|mapping| {
                                ["source", "target", "path"].iter().any(|key| {
                                    mapping
                                        .get(serde_yaml::Value::String((*key).to_string()))
                                        .and_then(serde_yaml::Value::as_str)
                                        .is_some_and(|value| value.contains("/dev/net/tun"))
                                })
                            })
                    })
                });
            if requires_tun {
                unsupported_services.push(name.to_string());
            }
        }
    }
    unsupported_services.sort();
    unsupported_services.dedup();

    if !unsupported_services.is_empty() && run_services.is_empty() {
        return Err(Error::validation(format!(
            "SBX does not expose /dev/net/tun required by Compose service(s): {}. Declare runtime.sbx.run_services in .branchbox/config.json with the primary devcontainer service; Compose will still start its required dependencies.",
            unsupported_services.join(", ")
        )));
    }
    if let Some(primary) = config.service.as_deref() {
        if !run_services.is_empty() && !run_services.iter().any(|service| service == primary) {
            return Err(Error::validation(format!(
                "runtime.sbx.run_services must include primary devcontainer service '{primary}'"
            )));
        }
    }
    let selected_unsupported: Vec<_> = unsupported_services
        .iter()
        .filter(|service| run_services.iter().any(|selected| selected == *service))
        .collect();
    if !selected_unsupported.is_empty() {
        return Err(Error::validation(format!(
            "runtime.sbx.run_services selects SBX-incompatible /dev/net/tun service(s): {}",
            selected_unsupported
                .iter()
                .map(|service| service.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let compose_override = prepare_sbx_compose_override(
        repo_root,
        devcontainer_dir,
        config.service.as_deref(),
        &compose_files,
        None,
        None,
        None,
    )?;
    let generated = devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG);
    if run_services.is_empty() && compose_override.is_none() {
        if generated.exists() && generated != config_path {
            fs::remove_file(generated)?;
        }
        return Ok(());
    }

    let source = fs::read_to_string(&config_path)?;
    let mut value = jsonc_parser::parse_to_serde_value(&source, &Default::default())
        .map_err(|err| Error::validation(format!("Failed to parse devcontainer JSONC: {err:?}")))?
        .ok_or_else(|| Error::validation("Devcontainer configuration is empty"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::validation("Devcontainer configuration must be a JSON object"))?;
    if !run_services.is_empty() {
        object.insert("runServices".to_string(), serde_json::json!(run_services));
    }
    if let Some(override_name) = compose_override {
        let mut references = compose_references;
        references.retain(|reference| reference != &override_name);
        references.push(override_name);
        object.insert(
            "dockerComposeFile".to_string(),
            serde_json::json!(references),
        );
    }
    let rendered = serde_json::to_string_pretty(&value)?;
    if generated == config_path {
        return Err(Error::validation(
            "SBX runtime overlays for a top-level .devcontainer.json are not yet supported; move the source config to .devcontainer/devcontainer.json"
                .to_string(),
        ));
    }
    write_text_file(&generated, &format!("{rendered}\n"))?;
    Ok(())
}

fn prepare_sbx_compose_override(
    repo_root: &Path,
    devcontainer_dir: &Path,
    primary_service: Option<&str>,
    compose_files: &[PathBuf],
    validated_documents: Option<&[serde_yaml::Value]>,
    in_guest_worktree: Option<&Path>,
    private_stage: Option<&Path>,
) -> Result<Option<String>> {
    let override_path = devcontainer_dir.join(SBX_COMPOSE_OVERRIDE);
    let Some(primary_service) = primary_service else {
        if override_path.exists() {
            fs::remove_file(override_path)?;
        }
        return Ok(None);
    };

    let mut primary_found = false;
    let mut replace_main_git_mount = false;
    let documents: Vec<serde_yaml::Value> = if let Some(documents) = validated_documents {
        documents.to_vec()
    } else {
        compose_files
            .iter()
            .filter_map(|compose_file| fs::read_to_string(compose_file).ok())
            .filter_map(|source| serde_yaml::from_str::<serde_yaml::Value>(&source).ok())
            .collect()
    };
    for document in &documents {
        let Some(service) = document
            .get("services")
            .and_then(|services| services.get(primary_service))
        else {
            continue;
        };
        primary_found = true;
        replace_main_git_mount |= service
            .get("volumes")
            .and_then(serde_yaml::Value::as_sequence)
            .is_some_and(|volumes| volumes.iter().any(compose_volume_targets_main_git));
    }

    if !primary_found {
        if override_path.exists() {
            fs::remove_file(override_path)?;
        }
        return Ok(None);
    }

    let mut service = serde_yaml::Mapping::new();
    service.insert(
        serde_yaml::Value::String("restart".to_string()),
        serde_yaml::Value::String("unless-stopped".to_string()),
    );
    if replace_main_git_mount {
        let authoritative_git = repository_common_git_dir(repo_root)?;
        let mut volume = serde_yaml::Mapping::new();
        volume.insert(
            serde_yaml::Value::String("type".to_string()),
            serde_yaml::Value::String("bind".to_string()),
        );
        volume.insert(
            serde_yaml::Value::String("source".to_string()),
            serde_yaml::Value::String(authoritative_git.to_string_lossy().into_owned()),
        );
        volume.insert(
            serde_yaml::Value::String("target".to_string()),
            serde_yaml::Value::String(CONTAINER_MAIN_GIT_TARGET.to_string()),
        );
        service.insert(
            serde_yaml::Value::String("volumes".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(volume)]),
        );
    }

    let mut services = serde_yaml::Mapping::new();
    services.insert(
        serde_yaml::Value::String(primary_service.to_string()),
        serde_yaml::Value::Mapping(service),
    );
    let mut document = serde_yaml::Mapping::new();
    document.insert(
        serde_yaml::Value::String("services".to_string()),
        serde_yaml::Value::Mapping(services),
    );
    let rendered = serde_yaml::to_string(&serde_yaml::Value::Mapping(document)).map_err(|err| {
        Error::config(format!("Failed to serialize the SBX Compose facade: {err}"))
    })?;
    if let Some(worktree_path) = in_guest_worktree {
        write_managed_in_guest_generated_text_file(
            worktree_path,
            private_stage,
            &override_path,
            &rendered,
        )?;
    } else {
        write_text_file(&override_path, &rendered)?;
    }
    Ok(Some(SBX_COMPOSE_OVERRIDE.to_string()))
}

fn compose_volume_targets_main_git(volume: &serde_yaml::Value) -> bool {
    if let Some(short) = volume.as_str() {
        return short
            .split(':')
            .any(|component| component == CONTAINER_MAIN_GIT_TARGET);
    }
    volume.as_mapping().is_some_and(|mapping| {
        mapping
            .get(serde_yaml::Value::String("target".to_string()))
            .and_then(serde_yaml::Value::as_str)
            == Some(CONTAINER_MAIN_GIT_TARGET)
    })
}

fn runtime_ports(worktree_path: &Path) -> Vec<RuntimePort> {
    let Ok((config, _)) = DevcontainerConfig::load(worktree_path) else {
        return Vec::new();
    };

    let mut ports: Vec<RuntimePort> = config
        .forward_ports
        .unwrap_or_default()
        .into_iter()
        .filter_map(|port| port.runtime_mapping())
        .map(|(host, runtime)| RuntimePort { host, runtime })
        .collect();
    ports.sort_unstable_by_key(|port| (port.host, port.runtime));
    ports.dedup();
    ports
}

fn split_feature_section(path: &Path) -> Result<(String, Option<String>)> {
    let content = fs::read_to_string(path)?;
    if let Some(pos) = content.find(ENV_FEATURE_SECTION_MARKER) {
        let base = content[..pos].trim_end().to_string();
        let mut base_with_newline = base;
        if !base_with_newline.ends_with('\n') {
            base_with_newline.push('\n');
        }
        Ok((base_with_newline, Some(content[pos..].to_string())))
    } else {
        let mut base = content.trim_end().to_string();
        if !base.is_empty() && !base.ends_with('\n') {
            base.push('\n');
        }
        Ok((base, None))
    }
}

fn sanitize_identifier_env_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/' | '='))
        .collect()
}

fn sanitize_git_branch_env_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/'))
        .collect()
}

fn sanitize_compose_project_name(value: &str) -> String {
    let normalized: String = value
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if matches!(ch, '-' | '_') {
                Some(ch)
            } else {
                None
            }
        })
        .collect();
    let trimmed = normalized.trim_start_matches(['-', '_']);
    if trimmed.is_empty() {
        "app".to_string()
    } else {
        trimmed.to_string()
    }
}

fn quote_env_value(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn sanitize_url_env_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '.' | '-'
                        | '_'
                        | '/'
                        | ':'
                        | '='
                        | '?'
                        | '&'
                        | '#'
                        | '%'
                        | '+'
                        | '~'
                        | '@'
                        | ','
                        | '['
                        | ']'
                        | '!'
                        | '*'
                        | '('
                        | ')'
                        | ';'
                )
        })
        .collect()
}

fn ensure_not_symlink(path: &Path) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Refusing to write through symlink: {}", path.display()),
            ));
        }
    }
    Ok(())
}

/// The crate-wide entry point to [`ensure_not_symlink`] for writers outside this module
/// (`atomic_fs::write_atomic`). The guard itself stays a private `fn` here, where
/// `scripts/review-preflight.sh` checks for it.
pub(crate) fn refuse_symlink_target(path: &Path) -> io::Result<()> {
    ensure_not_symlink(path)
}

fn write_text_file(path: &Path, contents: &str) -> io::Result<()> {
    ensure_not_symlink(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }
}

fn create_secure_file(path: &Path) -> io::Result<File> {
    ensure_not_symlink(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
    }
}

fn write_secure_file(path: &Path, contents: &str) -> io::Result<()> {
    let mut file = create_secure_file(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

fn open_append_secure(path: &Path) -> io::Result<File> {
    ensure_not_symlink(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let file = OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new().append(true).open(path)
    }
}

fn feature_title_from_work_feature(work_feature: &str) -> String {
    let title = work_feature
        .split('-')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    if title.is_empty() {
        work_feature.to_string()
    } else {
        title
    }
}

fn update_spec_frontmatter(
    path: &Path,
    updates: &[(String, String)],
    removals: &[&str],
) -> Result<()> {
    let raw = fs::read_to_string(path)?;
    let trimmed = raw.trim_start();

    let (frontmatter, body_start) = if let Some(rest) = trimmed.strip_prefix("---") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some((front, remainder)) = rest.split_once("\n---") {
            let body = remainder.strip_prefix('\n').unwrap_or(remainder);
            (Some(front), Some(body))
        } else {
            (Some(rest), None)
        }
    } else {
        (None, Some(trimmed))
    };

    let mut entries = BTreeMap::new();
    if let Some(front) = frontmatter {
        for line in front.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                entries.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }

    for key in removals {
        entries.remove(*key);
    }

    for (key, value) in updates {
        entries.insert(key.clone(), value.clone());
    }

    let mut frontmatter_text = String::from("---\n");
    for (key, value) in &entries {
        frontmatter_text.push_str(key);
        frontmatter_text.push(':');
        frontmatter_text.push(' ');
        frontmatter_text.push_str(value);
        frontmatter_text.push('\n');
    }
    frontmatter_text.push_str("---\n");

    let mut body_text = String::new();
    if let Some(body) = body_start {
        body_text.push_str(body.trim_start_matches('\n'));
    }

    write_text_file(path, &format!("{}{}", frontmatter_text, body_text))?;
    Ok(())
}

fn build_branch_name(prefix: Option<&str>, work_feature: &str) -> String {
    let sanitized_prefix = sanitize_git_branch_env_value(prefix.unwrap_or("feature"));
    let prefix = sanitized_prefix.trim_end_matches('/');
    let work_feature = sanitize_git_branch_env_value(work_feature);
    if prefix.is_empty() {
        work_feature
    } else {
        format!("{}/{}", prefix, work_feature)
    }
}

/// The branch teardown acts on (C10c): an explicit `--branch-prefix` wins, then the branch the
/// registry recorded when the feature started, then the configured prefix. A recorded name
/// that is not a plain branch name is ignored with a warning, so a hand-edited registry cannot
/// smuggle an option into `git branch -d`.
fn resolve_teardown_branch(
    explicit_prefix: Option<&str>,
    recorded: Option<&FeatureMetadata>,
    config: &BranchBoxConfig,
    work_feature: &str,
    warnings: &mut Vec<String>,
) -> ResolvedBranch {
    if let Some(prefix) = explicit_prefix {
        return ResolvedBranch {
            name: build_branch_name(Some(prefix), work_feature),
            source: BranchSource::ExplicitPrefix,
        };
    }
    if let Some(recorded) = recorded
        .map(|metadata| metadata.branch_name.as_str())
        .filter(|name| !name.is_empty())
    {
        if is_plain_branch_name(recorded) {
            return ResolvedBranch {
                name: recorded.to_string(),
                source: BranchSource::Registry,
            };
        }
        warnings.push(format!(
            "Ignoring the registry's branch name '{recorded}': it is not a plain branch name"
        ));
    }
    ResolvedBranch {
        name: build_branch_name(Some(&config.feature.branch_prefix), work_feature),
        source: BranchSource::ConfigPrefix,
    }
}

/// A branch name made of the characters BranchBox itself uses, that git cannot read as an
/// option or a range.
fn is_plain_branch_name(name: &str) -> bool {
    !name.starts_with('-')
        && !name.contains("..")
        && !name.ends_with('/')
        && sanitize_git_branch_env_value(name) == name
}

/// The part of `err` worth showing as a cause: a failed command's own message, without the
/// "Command execution failed:" prefix.
fn error_cause(err: &Error) -> String {
    match err {
        Error::CommandFailed(message) => message.trim().to_string(),
        other => other.to_string(),
    }
}

fn resolve_git_object(repo_root: &Path, revision: &str) -> Result<String> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
            "rev-parse",
            "--verify",
            &format!("{revision}^{{commit}}"),
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|err| {
            Error::git(format!(
                "Failed to resolve Git revision '{revision}': {err}"
            ))
        })?;
    if !output.status.success() {
        return Err(Error::git(format!(
            "Git revision '{revision}' could not be resolved to a commit"
        )));
    }
    let digest = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !matches!(digest.len(), 40 | 64)
        || !digest
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(Error::git(format!(
            "Git revision '{revision}' resolved to an invalid object digest"
        )));
    }
    Ok(digest)
}

fn validate_untrusted_checkout_attributes(repo_root: &Path, revision: &str) -> Result<()> {
    let listing = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            revision,
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|err| Error::git(format!("Failed to inspect untrusted Git attributes: {err}")))?;
    if !listing.status.success() {
        return Err(Error::git(
            "Failed to enumerate untrusted repository attributes",
        ));
    }
    for path in listing
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = String::from_utf8_lossy(path);
        if !path.ends_with(".gitattributes") {
            continue;
        }
        let object = format!("{revision}:{path}");
        let content = Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.attributesFile=/dev/null",
                "show",
                &object,
            ])
            .current_dir(repo_root)
            .output()
            .map_err(|err| {
                Error::git(format!("Failed to inspect untrusted attribute file: {err}"))
            })?;
        if !content.status.success() {
            return Err(Error::git(format!(
                "Failed to read untrusted attribute file '{path}'"
            )));
        }
        let defines_filter = String::from_utf8_lossy(&content.stdout)
            .lines()
            .any(|line| {
                let line = line.split('#').next().unwrap_or_default();
                line.split_ascii_whitespace().any(|attribute| {
                    attribute == "filter"
                        || attribute == "-filter"
                        || attribute.starts_with("filter=")
                })
            });
        if defines_filter {
            return Err(Error::validation(format!(
                "In-guest worktree checkout rejects filter attributes in '{path}' because smudge/process filters execute in the trusted guest"
            )));
        }
    }
    Ok(())
}

fn resolve_repo_root(path: &Path) -> Result<PathBuf> {
    // First attempt to ask git where the repository lives to cover nested directories.
    let cwd = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    if let Ok(output) = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
            "rev-parse",
            "--show-toplevel",
        ])
        .current_dir(cwd)
        .output()
    {
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !root.is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
    }

    // Fallback: walk up the directory tree and look for a .git entry.
    let mut current = if path.exists() {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };

    if current.is_file() {
        current = current
            .parent()
            .ok_or_else(|| Error::validation("Not a git repository".to_string()))?
            .to_path_buf();
    }

    let mut cursor = Some(current);
    while let Some(dir) = cursor {
        let git_path = dir.join(".git");
        if git_path.is_dir() {
            return Ok(dir);
        }

        if git_path.is_file() {
            let git_file = fs::read_to_string(&git_path)?;
            let gitdir = git_file
                .strip_prefix("gitdir:")
                .map(|s| s.trim())
                .ok_or_else(|| Error::validation("Invalid .git file format".to_string()))?;

            let gitdir_path = if Path::new(gitdir).is_absolute() {
                PathBuf::from(gitdir)
            } else {
                dir.join(gitdir)
            };
            let canonical_gitdir = gitdir_path.canonicalize()?;
            let repo_root = canonical_gitdir
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .ok_or_else(|| {
                    Error::validation("Unable to resolve repository root".to_string())
                })?;
            return Ok(repo_root.to_path_buf());
        }

        cursor = dir.parent().map(|p| p.to_path_buf());
    }

    Err(Error::NotAGitRepository(path.to_path_buf()))
}

#[derive(Clone)]
struct EnvOutcome {
    env_path: Option<PathBuf>,
    feature_url: Option<String>,
    compose_project_name: Option<String>,
    skipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatus {
    Active,
    Degraded,
    FailedRetained,
    Orphaned,
    Removed,
}

impl fmt::Display for FeatureStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeatureStatus::Active => write!(f, "active"),
            FeatureStatus::Degraded => write!(f, "degraded"),
            FeatureStatus::FailedRetained => write!(f, "failed_retained"),
            FeatureStatus::Orphaned => write!(f, "orphaned"),
            FeatureStatus::Removed => write!(f, "removed"),
        }
    }
}

impl FromStr for FeatureStatus {
    type Err = ParseFeatureStatusError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "active" => Ok(FeatureStatus::Active),
            "degraded" => Ok(FeatureStatus::Degraded),
            "failed_retained" | "failed-retained" => Ok(FeatureStatus::FailedRetained),
            "orphaned" => Ok(FeatureStatus::Orphaned),
            "removed" => Ok(FeatureStatus::Removed),
            _ => Err(ParseFeatureStatusError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParseFeatureStatusError(String);

impl fmt::Display for ParseFeatureStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
                "invalid feature status '{}'; expected active, degraded, failed_retained, orphaned, or removed",
            self.0
        )
    }
}

impl std::error::Error for ParseFeatureStatusError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureTunnelStatus {
    Pending,
    Active,
    Manual,
    Disabled,
}

impl fmt::Display for FeatureTunnelStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            FeatureTunnelStatus::Pending => "degraded",
            FeatureTunnelStatus::Active => "online",
            FeatureTunnelStatus::Manual => "manual",
            FeatureTunnelStatus::Disabled => "disabled",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTunnelDescriptor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureTunnelState {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Service URL the tunnel routes to (e.g., `http://app:5001`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_url: Option<String>,
    pub status: FeatureTunnelStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<StoredTunnelDescriptor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub last_updated: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<DateTime<Utc>>,
}

impl FeatureTunnelState {
    fn manual(
        provider: impl Into<String>,
        hostname: Option<String>,
        service_url: Option<String>,
        reason: impl Into<String>,
        steps: Vec<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            hostname,
            service_url,
            status: FeatureTunnelStatus::Manual,
            descriptor: None,
            instructions: if steps.is_empty() { None } else { Some(steps) },
            notes: Some(reason.into()),
            last_updated: Utc::now(),
            removed_at: None,
        }
    }

    fn disabled(provider: Option<String>, reason: impl Into<String>) -> Self {
        Self {
            provider: provider.unwrap_or_else(|| "manual".to_string()),
            hostname: None,
            service_url: None,
            status: FeatureTunnelStatus::Disabled,
            descriptor: None,
            instructions: None,
            notes: Some(reason.into()),
            last_updated: Utc::now(),
            removed_at: None,
        }
    }

    fn automated(descriptor: TunnelDescriptor, service_url: Option<String>) -> Self {
        Self {
            provider: descriptor.provider.clone(),
            hostname: Some(descriptor.hostname.clone()),
            service_url,
            status: FeatureTunnelStatus::Pending,
            descriptor: Some(StoredTunnelDescriptor {
                tunnel_name: descriptor.tunnel_name.clone(),
                tunnel_id: descriptor.tunnel_id.clone(),
                token_path: descriptor.token_path.clone(),
            }),
            instructions: None,
            notes: None,
            last_updated: Utc::now(),
            removed_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleOutcomeRecord {
    pub module: String,
    #[serde(default)]
    pub status: ModuleStatus,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default)]
    pub forced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureMetadata {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub base_branch: Option<String>,
    pub feature_url: Option<String>,
    pub compose_project_name: Option<String>,
    pub env_path: Option<PathBuf>,
    pub status: FeatureStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub removed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<FeatureTunnelState>,
    /// Workspace color for visual differentiation.
    ///
    /// Hex color code (e.g., "#3498db") generated deterministically from the feature name.
    /// Used by Peacock extension and VS Code to visually distinguish workspaces.
    /// Limited to 12 colors from a predefined palette, so features may share colors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// GitHub pull request number if one exists for this feature.
    ///
    /// Can be populated via `gh` CLI integration when a PR is created for this branch.
    /// Useful for tracking feature status and linking worktrees to PRs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u32>,
    /// Last commit SHA on this branch.
    ///
    /// Captured at worktree creation time using `git rev-parse <branch>`.
    /// Useful for tracking branch state and detecting stale worktrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_commit: Option<String>,
    /// Indicates if the devcontainer configuration is out of date for this worktree.
    #[serde(default)]
    pub devcontainer_outdated: bool,
    /// Last time the devcontainer configuration was synchronized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<DateTime<Utc>>,
    /// Last devcontainer sync strategy that was applied (copy/symlink).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_strategy: Option<String>,
    /// Mode used when starting the feature (full/minimal).
    #[serde(default)]
    pub start_mode: StartMode,
    /// Optional prompt seed captured during feature start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_seed: Option<String>,
    /// Recorded module outcomes for the feature start operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_outcomes: Vec<ModuleOutcomeRecord>,
    /// Timestamp when the CLI rendered the summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_summary_rendered_at: Option<DateTime<Utc>>,
    /// Adapter information captured at start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter: Option<AdapterSummary>,
    /// Execution boundary used for this workspace. Missing data from older
    /// registries defaults to the compatibility `container` provider.
    #[serde(default)]
    pub runtime: RuntimeMetadata,
    /// Present only while a `feature start` has not completed (DESIGN §5.4): written as soon as
    /// the worktree exists and cleared by the final registry update, so a start that died midway
    /// stays visible. Older CLIs ignore the key.
    #[serde(
        default,
        deserialize_with = "deserialize_setup_record",
        skip_serializing_if = "Option::is_none"
    )]
    pub setup: Option<SetupRecord>,
}

/// How long an unfinished start may stay `in_progress` before it is reported as `interrupted`
/// even though its pid is alive (the pid may have been reused by an unrelated process).
const SETUP_STALE_AFTER_HOURS: i64 = 24;

/// Progress of a `feature start` that has not completed.
///
/// Only `in_progress` is ever written. `interrupted` is computed by
/// [`FeatureWorkflow::list_features`] from [`SetupRecord::observed_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupState {
    InProgress,
    Interrupted,
}

impl<'de> Deserialize<'de> for SetupState {
    /// A state this version does not know (written by a newer BranchBox) reads as
    /// `in_progress`, so the liveness and age rules still decide what to report.
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        Ok(match String::deserialize(deserializer)?.as_str() {
            "interrupted" => Self::Interrupted,
            _ => Self::InProgress,
        })
    }
}

/// The write-ahead marker of an unfinished start: which process is running it, and since when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupRecord {
    pub state: SetupState,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

impl SetupRecord {
    /// A record for a start that this process begins now.
    fn begin() -> Self {
        Self {
            state: SetupState::InProgress,
            pid: std::process::id(),
            started_at: Utc::now(),
        }
    }

    /// The state to report at `now`: `interrupted` once the recording process has exited or the
    /// start is more than 24 hours old, otherwise the recorded state.
    pub fn observed_state(&self, now: DateTime<Utc>) -> SetupState {
        let stale = now.signed_duration_since(self.started_at)
            > chrono::Duration::hours(SETUP_STALE_AFTER_HOURS);
        if self.state == SetupState::Interrupted || stale || !process_is_alive(self.pid) {
            SetupState::Interrupted
        } else {
            SetupState::InProgress
        }
    }
}

/// Whether `pid` names a live process. Only a definite "no such process" counts as dead: a
/// process owned by another user (EPERM) is alive. Pid 0 and values outside `pid_t` are never a
/// recorded start, so they count as dead rather than probing the process group.
#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 delivers nothing; kill(2) only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Without kill(2) the liveness of a pid is unknown, so only the 24-hour rule applies.
#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

/// Read `setup` leniently: a record this version cannot parse (a newer BranchBox may change its
/// shape) is dropped instead of failing the whole registry load.
fn deserialize_setup_record<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<SetupRecord>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let Some(value) = Option::<serde_json::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match serde_json::from_value(value) {
        Ok(record) => Ok(Some(record)),
        Err(err) => {
            tracing::debug!("Ignoring unreadable feature setup record: {err}");
            Ok(None)
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FeatureRegistry {
    #[serde(deserialize_with = "deserialize_registry_version")]
    version: String,
    features: Vec<FeatureMetadata>,
}

impl Default for FeatureRegistry {
    fn default() -> Self {
        Self {
            version: STATE_VERSION.to_string(),
            features: Vec::new(),
        }
    }
}

/// The project's feature registry (`<repo>/.branchbox/registry.json`).
///
/// Every change is a locked read-modify-write through [`FeatureStateStore::mutate`], so
/// concurrent BranchBox processes (CLI runs, the macOS app, the agent) never lose each other's
/// updates, and every write replaces the file atomically. Readers take no lock: they always see
/// a complete document.
#[derive(Debug)]
struct FeatureStateStore {
    state_dir: PathBuf,
    path: PathBuf,
    legacy_path: PathBuf,
    /// How long a change waits for another process to release the registry lock.
    lock_timeout: std::time::Duration,
    /// Test hook: sleep this long while holding the lock, between reading and writing the
    /// registry, to widen the window in which an unlocked writer would lose an update.
    #[cfg(test)]
    hold_while_locked: Option<std::time::Duration>,
}

impl FeatureStateStore {
    fn new(repo_root: &Path) -> Self {
        let state_dir = repo_root.join(".branchbox");
        let path = state_dir.join("registry.json");
        let legacy_path = state_dir.join("feature.json");
        Self {
            state_dir,
            path,
            legacy_path,
            lock_timeout: atomic_fs::LOCK_TIMEOUT,
            #[cfg(test)]
            hold_while_locked: None,
        }
    }

    /// Apply `change` to the registry under the `.branchbox` lock and save the result. Nothing is
    /// written when `change` fails.
    fn mutate<T>(&self, change: impl FnOnce(&mut FeatureRegistry) -> Result<T>) -> Result<T> {
        let _lock = atomic_fs::lock_state_dir(&self.state_dir, self.lock_timeout)?;
        let mut registry = self.load_registry()?;
        #[cfg(test)]
        if let Some(hold) = self.hold_while_locked {
            std::thread::sleep(hold);
        }
        let value = change(&mut registry)?;
        self.save_registry(&registry)?;
        Ok(value)
    }

    fn record_start(&self, metadata: FeatureMetadata) -> Result<()> {
        self.mutate(|registry| {
            upsert_started_feature(registry, metadata);
            Ok(())
        })
    }

    /// The write-ahead half of `start`: record that this process is setting the feature up.
    ///
    /// A feature with no entry, or only a removed one, gets `provisional` (merged like any
    /// [`Self::record_start`]). A live entry (a `--reuse` start) keeps everything it recorded,
    /// in particular its runtime identity, and only gains the setup marker. Returns the entry
    /// that was there before, which [`Self::discard_setup`] restores.
    fn record_setup_started(
        &self,
        provisional: FeatureMetadata,
    ) -> Result<Option<FeatureMetadata>> {
        self.mutate(|registry| {
            let existing = registry
                .features
                .iter_mut()
                .find(|item| item.work_feature == provisional.work_feature);
            let previous = existing.as_deref().cloned();
            match existing {
                Some(existing) if existing.status != FeatureStatus::Removed => {
                    existing.setup = provisional.setup;
                    existing.updated_at = provisional.updated_at;
                }
                _ => upsert_started_feature(registry, provisional),
            }
            Ok(previous)
        })
    }

    /// Undo [`Self::record_setup_started`] after a failed start removed its worktree again:
    /// restore the previous entry, or drop the feature if it had none. An entry that no longer
    /// carries this start's marker (`pid`) belongs to someone else and is left alone.
    fn discard_setup(
        &self,
        work_feature: &str,
        pid: u32,
        previous: Option<FeatureMetadata>,
    ) -> Result<()> {
        self.mutate(|registry| {
            let Some(index) = registry.features.iter().position(|item| {
                item.work_feature == work_feature
                    && item.setup.as_ref().is_some_and(|setup| setup.pid == pid)
            }) else {
                return Ok(());
            };
            match previous {
                Some(previous) => registry.features[index] = previous,
                None => {
                    registry.features.remove(index);
                }
            }
            Ok(())
        })
    }

    /// Record the runtime an unfinished start prepared in its write-ahead entry, so teardown can
    /// remove that runtime if the start never completes. Only the entry that still carries this
    /// start's marker (`pid`) is changed. An entry that already names the same runtime keeps
    /// everything it recorded (a `--reuse-runtime` start of a live entry).
    fn record_setup_runtime(
        &self,
        work_feature: &str,
        pid: u32,
        runtime: &RuntimeMetadata,
    ) -> Result<()> {
        self.mutate(|registry| {
            let Some(entry) = registry.features.iter_mut().find(|item| {
                item.work_feature == work_feature
                    && item.setup.as_ref().is_some_and(|setup| setup.pid == pid)
            }) else {
                return Ok(());
            };
            if entry.runtime.provider == runtime.provider
                && entry.runtime.runtime_id == runtime.runtime_id
            {
                return Ok(());
            }
            entry.runtime = runtime.clone();
            entry.updated_at = Utc::now();
            Ok(())
        })
    }

    /// Mark `work_feature` removed. Returns whether the registry had an entry to update.
    fn record_teardown(&self, work_feature: &str) -> Result<bool> {
        self.mutate(|registry| {
            if let Some(existing) = registry
                .features
                .iter_mut()
                .find(|item| item.work_feature == work_feature)
            {
                let now = Utc::now();
                existing.status = FeatureStatus::Removed;
                existing.updated_at = now;
                existing.removed_at = Some(now);
                existing.setup = None;
                if let Some(tunnel) = existing.tunnel.as_mut() {
                    tunnel.status = FeatureTunnelStatus::Disabled;
                    tunnel.last_updated = now;
                    tunnel.removed_at = Some(now);
                    tunnel.descriptor = None;
                    tunnel.instructions = None;
                    if tunnel.notes.is_none() {
                        tunnel.notes = Some("Tunnel removed during teardown".to_string());
                    }
                }
                Ok(true)
            } else {
                tracing::debug!(
                    "Feature '{}' not present in registry during teardown",
                    work_feature
                );
                Ok(false)
            }
        })
    }

    fn get_feature(&self, work_feature: &str) -> Result<Option<FeatureMetadata>> {
        let registry = self.load_registry()?;
        Ok(registry
            .features
            .into_iter()
            .find(|item| item.work_feature == work_feature))
    }

    fn update_feature<F>(&self, work_feature: &str, mut update: F) -> Result<FeatureMetadata>
    where
        F: FnMut(&mut FeatureMetadata),
    {
        self.mutate(|registry| {
            let feature = registry
                .features
                .iter_mut()
                .find(|item| item.work_feature == work_feature)
                .ok_or_else(|| self.feature_not_found(work_feature))?;
            update(feature);
            Ok(feature.clone())
        })
    }

    fn record_devcontainer_sync(
        &self,
        work_feature: &str,
        strategy: Option<&str>,
        success: bool,
    ) -> Result<()> {
        self.mutate(|registry| {
            let Some(existing) = registry
                .features
                .iter_mut()
                .find(|item| item.work_feature == work_feature)
            else {
                return Err(Error::validation(format!(
                    "Feature '{}' not present in registry during devcontainer sync",
                    work_feature
                )));
            };
            let now = Utc::now();
            existing.devcontainer_outdated = !success;
            existing.last_sync_at = Some(now);
            if let Some(value) = strategy {
                existing.sync_strategy = Some(value.to_string());
            }
            existing.updated_at = now;
            Ok(())
        })
    }

    /// The error for a feature this registry has no entry for, naming the registry file.
    fn feature_not_found(&self, work_feature: &str) -> Error {
        Error::FeatureNotFound {
            name: work_feature.to_string(),
            registry: self.path.clone(),
        }
    }

    fn list_features(&self) -> Result<Vec<FeatureMetadata>> {
        let registry = self.load_registry()?;
        Ok(registry.features)
    }

    fn load_registry(&self) -> Result<FeatureRegistry> {
        if self.path.exists() {
            return self.read_registry_from(&self.path);
        }

        if self.legacy_path.exists() {
            return self.read_registry_from(&self.legacy_path);
        }

        Ok(FeatureRegistry::default())
    }

    /// Only [`Self::mutate`] calls this, with the registry lock held.
    fn save_registry(&self, registry: &FeatureRegistry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let serialized = serde_json::to_string_pretty(registry).map_err(|err| {
            Error::config(format!("Failed to serialize feature registry: {}", err))
        })?;

        atomic_fs::write_atomic(&self.path, serialized.as_bytes(), 0o644)?;
        if self.legacy_path.exists() && self.legacy_path != self.path {
            let _ = fs::remove_file(&self.legacy_path);
        }
        Ok(())
    }

    fn read_registry_from(&self, path: &Path) -> Result<FeatureRegistry> {
        let data = fs::read_to_string(path)?;
        if data.trim().is_empty() {
            return Ok(FeatureRegistry::default());
        }

        serde_json::from_str(&data)
            .map_err(|err| Error::config(format!("Failed to parse feature registry: {}", err)))
    }
}

/// Insert `metadata` for a started feature, or replace the existing entry while keeping what the
/// new record does not know yet (creation time, tunnel, sync and module history).
fn upsert_started_feature(registry: &mut FeatureRegistry, mut metadata: FeatureMetadata) {
    let now = metadata.updated_at;

    if let Some(existing) = registry
        .features
        .iter_mut()
        .find(|item| item.work_feature == metadata.work_feature)
    {
        if metadata.tunnel.is_none() {
            metadata.tunnel = existing.tunnel.clone();
        }
        metadata.created_at = existing.created_at;
        metadata.updated_at = now;
        if metadata.status != FeatureStatus::Removed {
            metadata.removed_at = None;
        }
        if metadata.last_sync_at.is_none() {
            metadata.last_sync_at = existing.last_sync_at;
        }
        if metadata.sync_strategy.is_none() {
            metadata.sync_strategy = existing.sync_strategy.clone();
        }
        if metadata.module_outcomes.is_empty() && !existing.module_outcomes.is_empty() {
            metadata.module_outcomes = existing.module_outcomes.clone();
        }
        if metadata.last_summary_rendered_at.is_none() {
            metadata.last_summary_rendered_at = existing.last_summary_rendered_at;
        }
        if metadata.prompt_seed.is_none() {
            metadata.prompt_seed = existing.prompt_seed.clone();
        }
        if metadata.adapter.is_none() {
            metadata.adapter = existing.adapter.clone();
        }
        *existing = metadata;
    } else {
        registry.features.push(metadata);
    }
}

const STATE_VERSION: &str = "1";

fn deserialize_registry_version<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum VersionHelper {
        String(String),
        Number(u32),
    }

    match VersionHelper::deserialize(deserializer)? {
        VersionHelper::String(value) => Ok(value),
        VersionHelper::Number(value) => Ok(value.to_string()),
    }
}

/// Generate a deterministic color from a feature name for visual differentiation.
///
/// Uses a hash of the feature name to select from a predefined palette of 12 distinguishable
/// colors. The same feature name will always generate the same color, making it easy to
/// identify workspaces visually in VS Code with the Peacock extension.
///
/// # Color Collisions
///
/// With only 12 colors available, projects with more than 12 concurrent features will
/// experience color collisions (multiple features sharing the same color). This is acceptable
/// for most workflows as it's unlikely to have >12 feature branches active simultaneously.
///
/// # Returns
///
/// A hex color code string in the format `#RRGGBB` (e.g., "#3498db").
///
/// # Examples
///
/// ```text
/// // Internal function - not exposed in public API
/// let color = generate_feature_color("oauth-integration");
/// assert_eq!(color.len(), 7); // #RRGGBB format
/// assert!(color.starts_with('#'));
///
/// // Same name always produces same color
/// let color2 = generate_feature_color("oauth-integration");
/// assert_eq!(color, color2);
/// ```
fn generate_feature_color(work_feature: &str) -> String {
    // Predefined palette of 12 distinguishable colors
    // Selected for good visual contrast and accessibility
    let palette = [
        "#3498db", // blue
        "#e74c3c", // red
        "#2ecc71", // green
        "#f39c12", // orange
        "#9b59b6", // purple
        "#1abc9c", // turquoise
        "#e67e22", // dark orange
        "#16a085", // dark turquoise
        "#27ae60", // dark green
        "#2980b9", // dark blue
        "#8e44ad", // dark purple
        "#c0392b", // dark red
    ];

    let mut hasher = DefaultHasher::new();
    work_feature.hash(&mut hasher);
    let hash = hasher.finish();
    let index = (hash % palette.len() as u64) as usize;

    palette[index].to_string()
}

fn build_color_customizations(base_color: &str) -> Option<serde_json::Value> {
    let rgb = parse_hex_color(base_color)?;
    let lighter = format_rgb(lighten_rgb(rgb, 0.20));
    let lighter_strong = format_rgb(lighten_rgb(rgb, 0.35));
    let badge_background = format_rgb(lighten_rgb(rgb, 0.15));
    let hover = format_rgb(darken_rgb(rgb, 0.25));
    let inactive_background = format!("{}99", base_color);
    let text_primary = "#15202b";
    let text_muted = "#15202b99";
    let badge_foreground = "#e7e7e7";

    Some(serde_json::json!({
        "activityBar.activeBackground": lighter_strong,
        "activityBar.background": lighter,
        "activityBar.foreground": text_primary,
        "activityBar.inactiveForeground": text_muted,
        "activityBarBadge.background": badge_background,
        "activityBarBadge.foreground": badge_foreground,
        "commandCenter.border": lighter_strong,
        "sash.hoverBorder": lighter_strong,
        "statusBar.background": base_color,
        "statusBar.foreground": text_primary,
        "statusBarItem.hoverBackground": hover,
        "statusBarItem.remoteBackground": base_color,
        "statusBarItem.remoteForeground": text_primary,
        "titleBar.activeBackground": base_color,
        "titleBar.activeForeground": text_primary,
        "titleBar.inactiveBackground": inactive_background,
        "titleBar.inactiveForeground": text_muted,
    }))
}

fn parse_hex_color(color: &str) -> Option<(u8, u8, u8)> {
    if !(color.starts_with('#') && color.len() == 7) {
        return None;
    }

    let r = u8::from_str_radix(&color[1..3], 16).ok()?;
    let g = u8::from_str_radix(&color[3..5], 16).ok()?;
    let b = u8::from_str_radix(&color[5..7], 16).ok()?;
    Some((r, g, b))
}

fn lighten_rgb((r, g, b): (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    let adjust = |value: u8| -> u8 {
        let value_f = value as f32;
        let adjusted = value_f + (255.0 - value_f) * factor;
        adjusted.clamp(0.0, 255.0).round() as u8
    };

    (adjust(r), adjust(g), adjust(b))
}

fn darken_rgb((r, g, b): (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    let adjust = |value: u8| -> u8 {
        let value_f = value as f32;
        let adjusted = value_f - (value_f * factor);
        adjusted.clamp(0.0, 255.0).round() as u8
    };

    (adjust(r), adjust(g), adjust(b))
}

fn format_rgb((r, g, b): (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// Get the last commit SHA for a branch.
fn get_last_commit_sha(repo_root: &Path, branch: &str) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
            "rev-parse",
            branch,
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if sha.is_empty() {
            None
        } else {
            Some(sha)
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CloudflaredConfig;
    use crate::workflows::teardown_plan::{ChangeArea, ChangeKind};
    use serde_json::Value;
    use serde_yaml::Value as YamlValue;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;
    use walkdir::WalkDir;

    #[test]
    fn test_build_branch_name_with_default_prefix() {
        assert_eq!(build_branch_name(None, "oauth"), "feature/oauth");
    }

    #[test]
    fn test_build_branch_name_with_custom_prefix() {
        assert_eq!(
            build_branch_name(Some("bugfix"), "issue-123"),
            "bugfix/issue-123"
        );
    }

    #[test]
    fn test_build_branch_name_with_trailing_slash() {
        assert_eq!(build_branch_name(Some("feature/"), "test"), "feature/test");
    }

    #[test]
    fn test_build_branch_name_with_empty_prefix() {
        assert_eq!(build_branch_name(Some(""), "test"), "test");
    }

    #[test]
    fn test_build_branch_name_strips_crlf() {
        assert_eq!(
            build_branch_name(Some("feature/\n"), "test\r"),
            "feature/test"
        );
    }

    #[test]
    fn test_build_branch_name_strips_equals() {
        assert_eq!(
            build_branch_name(Some("feature=unsafe"), "name=unsafe"),
            "featureunsafe/nameunsafe"
        );
    }

    #[test]
    fn test_sanitize_identifier_env_value_strips_shell_metacharacters() {
        assert_eq!(
            sanitize_identifier_env_value("https://dev.example.com/$(touch /tmp/pwn) & echo hi"),
            "https//dev.example.com/touch/tmp/pwnechohi"
        );
        assert_eq!(
            sanitize_identifier_env_value("feature/$USER;rm -rf *"),
            "feature/USERrm-rf"
        );
        assert_eq!(
            sanitize_identifier_env_value("feature:with:colons"),
            "featurewithcolons"
        );
    }

    #[test]
    fn test_sanitize_git_branch_env_value_strips_shell_metacharacters() {
        assert_eq!(
            sanitize_git_branch_env_value("feature/(ui)\r\nINJECTED=1"),
            "feature/uiINJECTED1"
        );
    }

    #[test]
    fn test_sanitize_compose_project_name_enforces_compose_charset() {
        assert_eq!(sanitize_compose_project_name("My_App-01"), "my_app-01");
        assert_eq!(sanitize_compose_project_name("my.app"), "myapp");
        assert_eq!(sanitize_compose_project_name("__bad\nname!"), "badname");
        assert_eq!(sanitize_compose_project_name("...lead"), "lead");
        assert_eq!(sanitize_compose_project_name("___"), "app");
    }

    #[test]
    fn test_quote_env_value_wraps_with_single_quotes() {
        assert_eq!(
            quote_env_value("https://dev.example.com?a=1&b=2"),
            "'https://dev.example.com?a=1&b=2'"
        );
        assert_eq!(quote_env_value("it'works"), "'it'\\''works'");
    }

    #[test]
    fn test_sanitize_url_env_value_preserves_query_and_fragment() {
        assert_eq!(
            sanitize_url_env_value("https://dev.example.com/path?a=1&b=2#frag"),
            "https://dev.example.com/path?a=1&b=2#frag"
        );
        assert_eq!(
            sanitize_url_env_value("https://dev.example.com/path;one!(two)*three?a=1"),
            "https://dev.example.com/path;one!(two)*three?a=1"
        );
        assert_eq!(
            sanitize_url_env_value("https://dev.example.com/$(touch /tmp/pwn)?a=1\r\nINJECT=1"),
            "https://dev.example.com/(touch/tmp/pwn)?a=1INJECT=1"
        );
    }

    #[test]
    fn test_feature_title_from_work_feature() {
        assert_eq!(
            feature_title_from_work_feature("oauth-integration"),
            "Oauth Integration"
        );
        assert_eq!(
            feature_title_from_work_feature("fix-bug-123"),
            "Fix Bug 123"
        );
        assert_eq!(feature_title_from_work_feature("a"), "A");
        assert_eq!(feature_title_from_work_feature(""), "");
    }

    #[test]
    fn test_feature_status_display() {
        assert_eq!(FeatureStatus::Active.to_string(), "active");
        assert_eq!(FeatureStatus::Removed.to_string(), "removed");
    }

    #[test]
    fn test_feature_status_from_str() {
        assert_eq!(
            FeatureStatus::from_str("active").unwrap(),
            FeatureStatus::Active
        );
        assert_eq!(
            FeatureStatus::from_str("removed").unwrap(),
            FeatureStatus::Removed
        );
        assert_eq!(
            FeatureStatus::from_str("ACTIVE").unwrap(),
            FeatureStatus::Active
        );
        assert_eq!(
            FeatureStatus::from_str(" removed ").unwrap(),
            FeatureStatus::Removed
        );
    }

    #[test]
    fn test_feature_status_from_str_invalid() {
        let result = FeatureStatus::from_str("invalid");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid"));
        assert!(err.to_string().contains("active"));
        assert!(err.to_string().contains("removed"));
    }

    #[test]
    fn test_update_spec_frontmatter_creates_new() {
        let temp = TempDir::new().unwrap();
        let spec_path = temp.path().join("spec.md");
        fs::write(&spec_path, "# Feature\n\nSome content\n").unwrap();

        let updates = vec![
            ("status".to_string(), "in-progress".to_string()),
            ("branch".to_string(), "feature/test".to_string()),
        ];
        update_spec_frontmatter(&spec_path, &updates, &[]).unwrap();

        let content = fs::read_to_string(&spec_path).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("branch: feature/test"));
        assert!(content.contains("status: in-progress"));
        assert!(content.contains("# Feature"));
    }

    #[test]
    fn test_update_spec_frontmatter_updates_existing() {
        let temp = TempDir::new().unwrap();
        let spec_path = temp.path().join("spec.md");
        fs::write(
            &spec_path,
            "---\nstatus: backlog\nold_key: old_value\n---\n# Feature\n",
        )
        .unwrap();

        let updates = vec![("status".to_string(), "in-progress".to_string())];
        update_spec_frontmatter(&spec_path, &updates, &[]).unwrap();

        let content = fs::read_to_string(&spec_path).unwrap();
        assert!(content.contains("status: in-progress"));
        assert!(content.contains("old_key: old_value"));
    }

    #[test]
    fn test_split_feature_section_no_marker() {
        let temp = TempDir::new().unwrap();
        let env_path = temp.path().join(".env");
        fs::write(&env_path, "FOO=bar\nBAZ=qux").unwrap();

        let (base, feature) = split_feature_section(&env_path).unwrap();
        assert_eq!(base.trim(), "FOO=bar\nBAZ=qux");
        assert!(feature.is_none());
    }

    #[test]
    fn test_split_feature_section_with_marker() {
        let temp = TempDir::new().unwrap();
        let env_path = temp.path().join(".env");
        fs::write(
            &env_path,
            "FOO=bar\n# Feature-specific configuration\nWORK_FEATURE=test\n",
        )
        .unwrap();

        let (base, feature) = split_feature_section(&env_path).unwrap();
        assert_eq!(base.trim(), "FOO=bar");
        assert!(feature.is_some());
        assert!(feature.unwrap().contains("WORK_FEATURE=test"));
    }

    #[test]
    fn test_resolve_repo_root_direct_repo() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path();

        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        let resolved = resolve_repo_root(repo_path).unwrap();
        assert_eq!(
            resolved.canonicalize().unwrap(),
            repo_path.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_resolve_repo_root_not_a_repo() {
        let temp = TempDir::new().unwrap();
        let result = resolve_repo_root(temp.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Not a git repository"));
    }

    #[test]
    fn test_state_store_load_empty() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());
        let registry = store.load_registry().unwrap();
        assert_eq!(registry.features.len(), 0);
    }

    #[test]
    fn test_state_store_record_start() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let metadata = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("main".to_string()),
            feature_url: Some("test.example.com".to_string()),
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            tunnel: None,
            color: None,
            pr_number: None,
            last_commit: None,
            devcontainer_outdated: false,
            last_sync_at: None,
            sync_strategy: None,
            start_mode: StartMode::Full,
            prompt_seed: None,
            module_outcomes: Vec::new(),
            last_summary_rendered_at: None,
            adapter: None,
            runtime: RuntimeMetadata::default(),
            setup: None,
        };

        store.record_start(metadata.clone()).unwrap();

        let features = store.list_features().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].work_feature, "test");
        assert_eq!(features[0].status, FeatureStatus::Active);
    }

    #[test]
    fn test_state_store_record_teardown() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let metadata = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("main".to_string()),
            feature_url: Some("test.example.com".to_string()),
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            tunnel: None,
            color: None,
            pr_number: None,
            last_commit: None,
            devcontainer_outdated: false,
            last_sync_at: None,
            sync_strategy: None,
            start_mode: StartMode::Full,
            prompt_seed: None,
            module_outcomes: Vec::new(),
            last_summary_rendered_at: None,
            adapter: None,
            runtime: RuntimeMetadata::default(),
            setup: None,
        };

        store.record_start(metadata).unwrap();
        store.record_teardown("test").unwrap();

        let features = store.list_features().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].status, FeatureStatus::Removed);
        assert!(features[0].removed_at.is_some());
    }

    #[test]
    fn test_state_store_record_start_updates_existing() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let now = Utc::now();
        let metadata1 = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("main".to_string()),
            feature_url: Some("test.example.com".to_string()),
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: now,
            updated_at: now,
            removed_at: None,
            tunnel: None,
            color: None,
            pr_number: None,
            last_commit: None,
            devcontainer_outdated: false,
            last_sync_at: None,
            sync_strategy: None,
            start_mode: StartMode::Full,
            prompt_seed: None,
            module_outcomes: Vec::new(),
            last_summary_rendered_at: None,
            adapter: None,
            runtime: RuntimeMetadata::default(),
            setup: None,
        };

        store.record_start(metadata1).unwrap();

        // Record start again with updated data
        let metadata2 = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("develop".to_string()), // Changed
            feature_url: Some("new.example.com".to_string()), // Changed
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: Utc::now(), // This should be ignored
            updated_at: Utc::now(),
            removed_at: None,
            tunnel: None,
            color: None,
            pr_number: None,
            last_commit: None,
            devcontainer_outdated: false,
            last_sync_at: None,
            sync_strategy: None,
            start_mode: StartMode::Full,
            prompt_seed: None,
            module_outcomes: Vec::new(),
            last_summary_rendered_at: None,
            adapter: None,
            runtime: RuntimeMetadata::default(),
            setup: None,
        };

        store.record_start(metadata2).unwrap();

        let features = store.list_features().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].work_feature, "test");
        assert_eq!(features[0].base_branch, Some("develop".to_string()));
        assert_eq!(features[0].feature_url, Some("new.example.com".to_string()));
        // created_at should be preserved from first record
        assert_eq!(features[0].created_at, now);
    }

    #[test]
    fn test_state_store_record_devcontainer_sync_success() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let metadata = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("main".to_string()),
            feature_url: None,
            compose_project_name: None,
            env_path: None,
            status: FeatureStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            tunnel: None,
            color: None,
            pr_number: None,
            last_commit: None,
            devcontainer_outdated: false,
            last_sync_at: None,
            sync_strategy: None,
            start_mode: StartMode::Full,
            prompt_seed: None,
            module_outcomes: Vec::new(),
            last_summary_rendered_at: None,
            adapter: None,
            runtime: RuntimeMetadata::default(),
            setup: None,
        };

        store.record_start(metadata).unwrap();
        store
            .record_devcontainer_sync("test", Some("copy"), true)
            .unwrap();

        let features = store.list_features().unwrap();
        assert_eq!(features.len(), 1);
        let feature = &features[0];
        assert!(!feature.devcontainer_outdated);
        assert!(feature.last_sync_at.is_some());
        assert_eq!(feature.sync_strategy.as_deref(), Some("copy"));
    }

    #[test]
    fn test_state_store_record_devcontainer_sync_missing_feature() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let result = store.record_devcontainer_sync("missing", None, true);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err
            .to_string()
            .contains("Feature 'missing' not present in registry"));
    }

    fn setup_test_repo() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        fs::write(repo_path.join("README.md"), "# Test Repo\n").unwrap();
        fs::write(
            repo_path.join(".gitignore"),
            ".env\n.devcontainer/.branchbox.env\n.devcontainer/.cloudflared.env\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "README.md", ".gitignore"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        temp_dir
    }

    fn copy_repo_devcontainer(repo_path: &Path) {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("core crate has repo root");
        let source = repo_root.join(".devcontainer");
        let dest = repo_path.join(".devcontainer");
        for entry in WalkDir::new(&source) {
            let entry = entry.unwrap();
            let rel = entry.path().strip_prefix(&source).unwrap();
            if rel.as_os_str().is_empty() {
                continue;
            }
            let target = dest.join(rel);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target).unwrap();
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::copy(entry.path(), &target).unwrap();
            }
        }

        Command::new("git")
            .args(["add", ".devcontainer"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        let diff_status = Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(repo_path)
            .status()
            .unwrap();

        if !diff_status.success() {
            Command::new("git")
                .args(["commit", "-m", "Add devcontainer scaffolding"])
                .current_dir(repo_path)
                .output()
                .unwrap();
        }
    }

    #[test]
    fn tunnel_open_falls_back_to_configured_provider_when_stored_provider_is_unsupported() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();
        fs::create_dir_all(repo_path.join(".branchbox/secure")).unwrap();
        fs::write(
            repo_path.join(".branchbox/secure/cloudflared.env"),
            "CLOUDFLARE_API_TOKEN=test-token\n",
        )
        .unwrap();

        let mut config = BranchBoxConfig::default();
        config.tunnel.default_provider = Some("cloudflared".to_string());
        config.tunnel.providers.cloudflared = Some(CloudflaredConfig {
            account_id: Some("${CLOUDFLARE_ACCOUNT_ID}".to_string()),
            api_token_path: Some(PathBuf::from(".branchbox/secure/cloudflared.env")),
            dns_zone: Some("example.com".to_string()),
            manual_instructions: false,
            ..Default::default()
        });
        config.save(repo_path).unwrap();

        let store = FeatureStateStore::new(repo_path);
        let now = Utc::now();
        let worktree_path = repo_path.parent().unwrap().join("lashdesk-videos");
        fs::create_dir_all(&worktree_path).unwrap();

        store
            .record_start(FeatureMetadata {
                work_feature: "lashdesk-videos".to_string(),
                branch_name: "feature/lashdesk-videos".to_string(),
                worktree_path: worktree_path.clone(),
                base_branch: Some("main".to_string()),
                feature_url: Some("amidship-lashdesk-videos.example.com".to_string()),
                compose_project_name: Some("amidship-lashdesk-videos".to_string()),
                env_path: Some(worktree_path.join(".env")),
                status: FeatureStatus::Active,
                created_at: now,
                updated_at: now,
                removed_at: None,
                tunnel: Some(FeatureTunnelState::disabled(
                    Some("manual".to_string()),
                    "Tunnel requires manual setup",
                )),
                color: None,
                pr_number: None,
                last_commit: None,
                devcontainer_outdated: false,
                last_sync_at: None,
                sync_strategy: None,
                start_mode: StartMode::Full,
                prompt_seed: None,
                module_outcomes: Vec::new(),
                last_summary_rendered_at: None,
                adapter: None,
                runtime: RuntimeMetadata::default(),
                setup: None,
            })
            .unwrap();

        std::env::remove_var("CLOUDFLARE_ACCOUNT_ID");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .tunnel_open(TunnelOpenRequest {
                work_feature: "lashdesk-videos".to_string(),
            })
            .unwrap();

        assert_eq!(summary.state.provider, "cloudflared");
        assert_eq!(summary.state.status, FeatureTunnelStatus::Manual);
        assert!(
            summary.warnings.iter().any(|warning| warning.contains(
                "Stored tunnel provider 'manual' is unsupported; using configured provider 'cloudflared'"
            )),
            "expected unsupported-provider fallback warning, got {:?}",
            summary.warnings
        );
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning.contains("unresolved placeholder")),
            "expected unresolved-placeholder warning, got {:?}",
            summary.warnings
        );
    }

    #[test]
    fn feature_start_creates_worktree_and_env() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();
        copy_repo_devcontainer(repo_path);

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let worktree_path = repo_path.parent().unwrap().join("test-feature");
        if worktree_path.exists() {
            fs::remove_dir_all(&worktree_path).unwrap();
        }

        // Create uncommitted change to verify stash handling
        fs::write(repo_path.join("README.md"), "# Test Repo\nlocal change\n").unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("test-feature".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        assert!(summary.worktree_path.exists());
        let env_path = summary.worktree_path.join(".env");
        assert!(env_path.exists());
        let env_content = fs::read_to_string(&env_path).unwrap();
        assert!(env_content.contains("WORK_FEATURE=test-feature"));
        assert!(env_content.contains("APP_URL='dev-test-feature.example.com'"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let env_mode = fs::metadata(&env_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(env_mode, 0o600);
        }

        let managed_env_path = summary.worktree_path.join(".devcontainer/.branchbox.env");
        assert!(managed_env_path.exists());
        let managed_env = fs::read_to_string(&managed_env_path).unwrap();
        assert!(managed_env.contains("WORK_FEATURE=test-feature"));
        assert!(managed_env.contains(&format!("GIT_BRANCH={}", summary.branch_name)));
        let main_name = repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main");
        assert!(managed_env.contains(&format!("BRANCHBOX_MAIN_NAME={}", main_name)));
        if let Some(compose) = summary.compose_project_name.as_deref() {
            assert!(managed_env.contains(&format!("COMPOSE_PROJECT_NAME={}", compose)));
            assert!(managed_env.contains(&format!("DEVCONTAINER_NAME={}", compose)));
        }
        if let Some(url) = summary.feature_url.as_deref() {
            assert!(managed_env.contains(&format!("APP_URL='{}'", url)));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let managed_mode = fs::metadata(&managed_env_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(managed_mode, 0o600);
        }
        assert_eq!(
            summary.feature_url.as_deref(),
            Some("dev-test-feature.example.com")
        );
        let adapter = summary.adapter.as_ref().expect("adapter summary");
        assert_eq!(adapter.name, "Generic");
        assert_eq!(adapter.service_url, "http://dev:3000");
        assert!(adapter.warnings.is_empty());
        let tunnel = summary.tunnel.as_ref().expect("tunnel state present");
        assert_eq!(tunnel.status, FeatureTunnelStatus::Disabled);
        assert_eq!(tunnel.provider, "cloudflared");
        assert!(
            tunnel
                .notes
                .as_deref()
                .unwrap_or_default()
                .contains("disabled until Cloudflare credentials are configured"),
            "expected disabled tunnel notes, got {:?}",
            tunnel.notes
        );
        assert!(
            summary.module_outcomes.iter().any(
                |outcome| outcome.module == "tunnel" && outcome.status == ModuleStatus::Skipped
            ),
            "expected tunnel module outcome skipped, got {:?}",
            summary
                .module_outcomes
                .iter()
                .map(|outcome| (&outcome.module, outcome.status))
                .collect::<Vec<_>>()
        );

        // The generated devcontainer compose file must keep the canonical workspace mount and shared config volumes.
        let compose_path = summary.worktree_path.join(".devcontainer/compose.yaml");
        let compose_contents = fs::read_to_string(&compose_path).unwrap();
        let compose_yaml: YamlValue = serde_yaml::from_str(&compose_contents).unwrap();
        let volume_entries = compose_yaml
            .get("services")
            .and_then(|services| services.get("rust-dev"))
            .and_then(|service| service.get("volumes"))
            .and_then(|volumes| volumes.as_sequence())
            .expect("rust-dev volumes list present");
        let volumes: Vec<&str> = volume_entries
            .iter()
            .filter_map(|entry| entry.as_str())
            .collect();
        assert!(
            volumes.contains(&"../..:/workspaces:cached"),
            "compose.yaml missing canonical /workspaces bind: {:?}",
            volumes
        );
        for shared in [
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/codex:/home/vscode/.codex",
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/claude:/home/vscode/.claude",
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/claude.json:/home/vscode/.claude.json",
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/gh:/home/vscode/.config/gh",
        ] {
            assert!(
                volumes.contains(&shared),
                "compose.yaml missing shared config volume {shared}: {:?}",
                volumes
            );
        }

        // The devcontainer.json must continue to cd into the workspace before running helper scripts.
        let devcontainer_json_path = summary
            .worktree_path
            .join(".devcontainer/devcontainer.json");
        let devcontainer_json: Value =
            serde_json::from_str(&fs::read_to_string(&devcontainer_json_path).unwrap()).unwrap();
        assert_eq!(
            devcontainer_json
                .get("postStartCommand")
                .and_then(|value| value.as_str()),
            Some(
                "bash -c 'cd \"${WORKSPACE_FOLDER:-.}\" && bash .devcontainer/scripts/ensure-gitdir.sh && bash .devcontainer/scripts/setup-git.sh'"
            ),
            "devcontainer.json missing canonical postStartCommand"
        );

        // Stashed README changes should be applied to the new worktree only
        let worktree_readme = fs::read_to_string(summary.worktree_path.join("README.md")).unwrap();
        assert!(worktree_readme.contains("local change"));
        let main_readme = fs::read_to_string(repo_path.join("README.md")).unwrap();
        assert_eq!(main_readme, "# Test Repo\n");

        let spec_path = summary
            .worktree_path
            .join("docs/features/in-progress/test-feature.md");
        assert!(spec_path.exists());
        let spec_content = fs::read_to_string(&spec_path).unwrap();
        assert!(spec_content.contains("status: in-progress"));
        assert!(spec_content.contains("branch: feature/test-feature"));
        assert!(spec_content.contains("worktree:"));
        assert!(!repo_path
            .join("docs/features/in-progress/test-feature.md")
            .exists());

        let stash_list = Command::new("git")
            .args(["stash", "list"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&stash_list.stdout)
            .trim()
            .is_empty());

        let registry_path = repo_path.join(".branchbox/registry.json");
        assert!(registry_path.exists());
        let registry_data = fs::read_to_string(&registry_path).unwrap();
        let registry: Value = serde_json::from_str(&registry_data).unwrap();
        let features = registry
            .get("features")
            .and_then(|features| features.as_array())
            .expect("features is array");
        assert_eq!(features.len(), 1);
        let entry = features.first().unwrap();
        assert_eq!(entry.get("work_feature").unwrap(), "test-feature");
        assert_eq!(entry.get("status").unwrap(), "active");

        let teardown_summary = workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature.clone(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
        assert!(teardown_summary.adapter_cleanup_warnings.is_empty());
    }

    #[test]
    fn feature_start_ignores_untracked_changes_without_stash_warnings() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();
        copy_repo_devcontainer(repo_path);

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");
        fs::write(repo_path.join("scratch-notes.txt"), "untracked changes\n").unwrap();
        let worktree_path = repo_path.parent().unwrap().join("stash-untracked");
        if worktree_path.exists() {
            fs::remove_dir_all(&worktree_path).unwrap();
        }

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("stash-untracked".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        assert!(
            !summary
                .warnings
                .iter()
                .any(|warning| warning.contains("Failed to apply stashed changes")),
            "unexpected stash warning(s): {:?}",
            summary.warnings
        );
        assert!(!summary.worktree_path.join("scratch-notes.txt").exists());
        assert!(repo_path.join("scratch-notes.txt").exists());

        let stash_list = Command::new("git")
            .args(["stash", "list"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&stash_list.stdout)
            .trim()
            .is_empty());
    }

    #[test]
    fn feature_start_keeps_branchbox_config_changes_in_main_worktree() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();
        copy_repo_devcontainer(repo_path);

        let mut config = BranchBoxConfig::default();
        config.tunnel.providers.cloudflared = Some(CloudflaredConfig {
            account_id: Some("${CLOUDFLARE_ACCOUNT_ID}".to_string()),
            api_token_path: Some(PathBuf::from(".branchbox/secure/cloudflared.env")),
            dns_zone: Some("example.com".to_string()),
            manual_instructions: false,
            ..Default::default()
        });
        config.save(repo_path).unwrap();

        Command::new("git")
            .args(["add", ".branchbox/config.json"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Track BranchBox config"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Simulate `init --update` rewriting account_id to a literal before feature start.
        config.tunnel.providers.cloudflared = Some(CloudflaredConfig {
            account_id: Some("acct-123".to_string()),
            api_token_path: Some(PathBuf::from(".branchbox/secure/cloudflared.env")),
            dns_zone: Some("example.com".to_string()),
            manual_instructions: false,
            ..Default::default()
        });
        config.save(repo_path).unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");
        let worktree_path = repo_path.parent().unwrap().join("stash-config-main");
        if worktree_path.exists() {
            fs::remove_dir_all(&worktree_path).unwrap();
        }

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("stash-config-main".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let main_config = BranchBoxConfig::load(repo_path).unwrap();
        assert_eq!(
            main_config
                .tunnel
                .providers
                .cloudflared
                .as_ref()
                .and_then(|cloudflared| cloudflared.account_id.as_deref()),
            Some("acct-123"),
            "main worktree config should retain updated literal account id"
        );

        let feature_config = BranchBoxConfig::load(&summary.worktree_path).unwrap();
        assert_eq!(
            feature_config
                .tunnel
                .providers
                .cloudflared
                .as_ref()
                .and_then(|cloudflared| cloudflared.account_id.as_deref()),
            Some("${CLOUDFLARE_ACCOUNT_ID}"),
            "feature worktree should keep committed config state"
        );
    }

    #[test]
    fn feature_start_keeps_untracked_devcontainer_templates_in_main_worktree() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");
        let worktree_path = repo_path.parent().unwrap().join("preserve-devcontainer");
        if worktree_path.exists() {
            fs::remove_dir_all(&worktree_path).unwrap();
        }

        let devcontainer_dir = repo_path.join(".devcontainer");
        fs::create_dir_all(devcontainer_dir.join("scripts")).unwrap();
        fs::write(devcontainer_dir.join("Dockerfile"), "FROM ubuntu:22.04\n").unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  dev:\n    image: ubuntu:22.04\n",
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            "{\"service\":\"dev\"}\n",
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("scripts/init-host.sh"),
            "#!/usr/bin/env bash\necho init\n",
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("scripts/setup-git.sh"),
            "#!/usr/bin/env bash\necho setup\n",
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join(".github-token.env"),
            "GITHUB_TOKEN=\n",
        )
        .unwrap();
        fs::write(devcontainer_dir.join(".git-signing-key"), "\n").unwrap();
        fs::write(devcontainer_dir.join(".gitconfig.env"), "\n").unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("preserve-devcontainer".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        for path in [
            ".devcontainer/Dockerfile",
            ".devcontainer/compose.yaml",
            ".devcontainer/devcontainer.json",
            ".devcontainer/scripts/init-host.sh",
            ".devcontainer/scripts/setup-git.sh",
        ] {
            assert!(
                repo_path.join(path).exists(),
                "expected main worktree to keep {}",
                path
            );
            assert!(
                summary.worktree_path.join(path).exists(),
                "expected feature worktree to contain {}",
                path
            );
        }
    }

    #[test]
    fn feature_start_moves_backlog_spec_into_worktree() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");
        copy_repo_devcontainer(repo_path);

        let backlog_dir = repo_path.join("docs/features/backlog");
        fs::create_dir_all(&backlog_dir).unwrap();
        fs::write(
            backlog_dir.join("backlog-spec.md"),
            "---\nstatus: backlog\ntitle: Backlog Spec\n---\n\n# Backlog Spec\n",
        )
        .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("backlog-spec".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let worktree_spec = summary
            .worktree_path
            .join("docs/features/in-progress/backlog-spec.md");
        assert!(worktree_spec.exists());
        assert!(!backlog_dir.join("backlog-spec.md").exists());

        let contents = fs::read_to_string(&worktree_spec).unwrap();
        assert!(contents.contains("status: in-progress"));
        assert!(contents.contains("branch: feature/backlog-spec"));

        workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature,
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
    }

    #[test]
    fn feature_start_without_source_env_still_sets_compose_identity() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");
        copy_repo_devcontainer(repo_path);

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("no-env-feature".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        assert!(summary.env_path.is_none());
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning.contains("Skipped .env provisioning")),
            "expected skipped env warning, got {:?}",
            summary.warnings
        );

        let compose_name = summary
            .compose_project_name
            .clone()
            .expect("compose project name should be set");
        assert!(compose_name.ends_with("-no-env-feature"));

        let branchbox_env_path = summary.worktree_path.join(".devcontainer/.branchbox.env");
        assert!(branchbox_env_path.exists());
        let branchbox_env = fs::read_to_string(&branchbox_env_path).unwrap();
        assert!(branchbox_env.contains(&format!("COMPOSE_PROJECT_NAME={}", compose_name)));
        assert!(branchbox_env.contains(&format!("DEVCONTAINER_NAME={}", compose_name)));

        workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature,
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
    }

    #[test]
    fn feature_teardown_removes_worktree_and_branch() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        workflow
            .start(StartRequest {
                name: Some("cleanup".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let worktree_path = repo_path.parent().unwrap().join("cleanup");
        assert!(worktree_path.exists());

        let summary = workflow
            .teardown(TeardownRequest {
                work_feature: "cleanup".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();

        assert!(summary.worktree_removed);
        assert!(summary.branch_deleted);
        assert!(!worktree_path.exists());
        assert!(summary.adapter_cleanup_warnings.is_empty());

        let output = Command::new("git")
            .args(["branch", "--list", "feature/cleanup"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());

        let registry_path = repo_path.join(".branchbox/registry.json");
        let registry_data = fs::read_to_string(&registry_path).unwrap();
        let registry: Value = serde_json::from_str(&registry_data).unwrap();
        let entry = registry["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item.get("work_feature").unwrap() == "cleanup")
            .unwrap();
        assert_eq!(entry.get("status").unwrap(), "removed");
    }

    #[test]
    fn feature_start_reuses_existing_branch_after_teardown_without_delete() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let first = workflow
            .start(StartRequest {
                name: Some("restart".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let torn_down = workflow
            .teardown(TeardownRequest {
                work_feature: "restart".to_string(),
                branch_prefix: None,
                delete_branch: false,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
        assert!(torn_down.worktree_removed);
        assert!(!torn_down.branch_deleted);

        let resumed = workflow
            .start(StartRequest {
                name: Some("restart".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        assert_eq!(resumed.branch_name, first.branch_name);
        assert!(resumed.worktree_path.exists());
        assert!(
            resumed
                .warnings
                .iter()
                .any(|warn| warn.contains("recreating worktree")),
            "expected warning about recreating worktree from existing branch, got {:?}",
            resumed.warnings
        );

        workflow
            .teardown(TeardownRequest {
                work_feature: "restart".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
    }

    #[test]
    fn list_features_returns_sorted_entries() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow
            .start(StartRequest {
                name: Some("feature-one".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        thread::sleep(Duration::from_millis(10));

        workflow
            .start(StartRequest {
                name: Some("feature-two".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let features = workflow.list_features().unwrap();
        assert_eq!(features.len(), 2);
        assert_eq!(features[0].work_feature, "feature-two");

        workflow
            .teardown(TeardownRequest {
                work_feature: "feature-one".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();

        let features = workflow.list_features().unwrap();
        let removed = features
            .iter()
            .find(|item| item.work_feature == "feature-one")
            .unwrap();
        assert_eq!(removed.status, FeatureStatus::Removed);

        workflow
            .teardown(TeardownRequest {
                work_feature: "feature-two".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
    }

    #[test]
    fn feature_teardown_prunes_stale_worktree_entries() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let worktree_path = repo_path.parent().unwrap().join("dirty");
        if worktree_path.exists() {
            fs::remove_dir_all(&worktree_path).unwrap();
        }
        workflow
            .start(StartRequest {
                name: Some("dirty".to_string()),
                mode: StartMode::Minimal,
                ..StartRequest::default()
            })
            .unwrap();

        assert!(worktree_path.exists());

        // An untracked user file: 0.13 deleted it through a remove_dir_all fallback (BUG-04).
        fs::write(worktree_path.join("dirty.txt"), "local changes").unwrap();

        let request = TeardownRequest {
            work_feature: "dirty".to_string(),
            branch_prefix: None,
            delete_branch: true,
            force_delete_branch: false,
            force_remove: false,
            force_remove_modules: false,
            complete_spec: false,
            telemetry: false,
        };
        let err = workflow
            .teardown(request.clone())
            .expect_err("teardown must refuse to delete the untracked file");
        match err {
            Error::TeardownRefused {
                plan,
                changed_anything,
                ..
            } => {
                assert!(!changed_anything);
                let user: Vec<&str> = plan
                    .changes
                    .user
                    .iter()
                    .map(|change| change.path.as_str())
                    .collect();
                assert_eq!(user, ["dirty.txt"]);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(worktree_path.join("dirty.txt")).unwrap(),
            "local changes"
        );

        // Discarding is explicit; the stale git registration goes with the worktree.
        let summary = workflow
            .teardown_with_options(
                request,
                TeardownOptions {
                    discard_changes: true,
                    require_mergeable_branch: true,
                },
            )
            .unwrap();
        assert!(summary.worktree_removed);
        assert!(summary
            .warnings
            .iter()
            .all(|warning| !warning.contains("removed manually")));
        assert_eq!(summary.discarded_changes.len(), 1);
        assert!(!worktree_path.exists());

        let git = GitWorktree::new(repo_path).unwrap();
        let worktrees = git.list().unwrap();
        assert!(worktrees.into_iter().all(|info| info.path != worktree_path));
    }

    #[test]
    fn feature_teardown_detects_module_changes_without_force() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        workflow
            .start(StartRequest {
                name: Some("dirty-devcontainer".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let worktree_path = repo_path.parent().unwrap().join("dirty-devcontainer");
        let dev_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&dev_dir).unwrap();
        fs::write(dev_dir.join("compose.yaml"), "services: {}\n").unwrap();

        let err = workflow
            .teardown(TeardownRequest {
                work_feature: "dirty-devcontainer".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: false,
                force_remove_modules: false,
                complete_spec: false,
                telemetry: false,
            })
            .expect_err("expected teardown to block on dirty module files");

        match err {
            Error::TeardownRefused { plan, .. } => {
                assert!(plan.blocks_on_uncommitted_changes());
                assert!(plan.has_module_area_changes());
                let change = plan
                    .changes
                    .user
                    .iter()
                    .find(|change| change.path == ".devcontainer/compose.yaml")
                    .unwrap_or_else(|| panic!("compose.yaml not listed: {:?}", plan.changes));
                assert_eq!(change.area, ChangeArea::Devcontainer);
                assert_eq!(change.kind, ChangeKind::Untracked);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
        assert!(dev_dir.join("compose.yaml").exists());

        // Force removal to cleanup for test completion.
        workflow
            .teardown(TeardownRequest {
                work_feature: "dirty-devcontainer".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
    }

    /// A repository at `<temp>/main` without a `.gitignore`, so feature worktrees land inside
    /// the temp dir and BranchBox's own files show up as untracked.
    fn nested_test_repo() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("main");
        fs::create_dir(&repo).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "test@example.com"],
            &["config", "user.name", "Test User"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git_in(&repo, args);
        }
        fs::write(repo.join("README.md"), "# Test Repo\n").unwrap();
        git_in(&repo, &["add", "README.md"]);
        git_in(&repo, &["commit", "-q", "-m", "Initial commit"]);
        fs::write(repo.join(".env"), "APP_URL=dev.example.com\n").unwrap();
        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");
        (temp, repo)
    }

    fn git_in(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn start_minimal(workflow: &FeatureWorkflow, name: &str, prefix: Option<&str>) -> PathBuf {
        workflow
            .start(StartRequest {
                name: Some(name.to_string()),
                branch_prefix: prefix.map(str::to_string),
                mode: StartMode::Minimal,
                ..StartRequest::default()
            })
            .unwrap()
            .worktree_path
    }

    fn delete_request(name: &str) -> TeardownRequest {
        TeardownRequest {
            work_feature: name.to_string(),
            branch_prefix: None,
            delete_branch: true,
            force_delete_branch: false,
            force_remove: false,
            force_remove_modules: false,
            complete_spec: false,
            telemetry: false,
        }
    }

    fn strict_options() -> TeardownOptions {
        TeardownOptions {
            discard_changes: false,
            require_mergeable_branch: true,
        }
    }

    fn branch_exists(repo: &Path, branch: &str) -> bool {
        Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn refused_teardown_changes_nothing() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "eta", None);
        fs::write(worktree.join("README.md"), "edited\n").unwrap();
        fs::write(worktree.join("notes.txt"), "notes\n").unwrap();
        let registry_before = fs::read(repo.join(".branchbox/registry.json")).unwrap();

        let err = workflow.teardown(delete_request("eta")).unwrap_err();
        let Error::TeardownRefused {
            plan,
            changed_anything,
            completed_steps,
            message,
            ..
        } = err
        else {
            panic!("unexpected error {err:?}");
        };
        assert!(!changed_anything && completed_steps.is_empty());
        assert!(
            message.contains("README.md (modified), notes.txt (untracked)"),
            "{message}"
        );
        assert!(plan
            .changes
            .generated
            .iter()
            .any(|file| file.path == ".env"));
        assert_eq!(plan.changes.preserved.len(), 1);

        assert_eq!(
            fs::read_to_string(worktree.join("notes.txt")).unwrap(),
            "notes\n"
        );
        assert!(worktree.join("docs/features/in-progress/eta.md").exists());
        assert!(!repo.join("docs/features/backlog/eta.md").exists());
        assert_eq!(
            fs::read(repo.join(".branchbox/registry.json")).unwrap(),
            registry_before,
            "the registry is untouched"
        );
        assert!(branch_exists(&repo, "feature/eta"));

        let summary = workflow
            .teardown_with_options(
                delete_request("eta"),
                TeardownOptions {
                    discard_changes: true,
                    require_mergeable_branch: true,
                },
            )
            .unwrap();
        assert!(summary.worktree_removed && summary.branch_deleted && summary.registry_updated);
        assert_eq!(summary.branch_action, BranchAction::Delete);
        let discarded: Vec<&str> = summary
            .discarded_changes
            .iter()
            .map(|change| change.path.as_str())
            .collect();
        assert_eq!(discarded, ["README.md", "notes.txt"]);
        assert_eq!(
            summary.preserved,
            [PreservedFile {
                path: "docs/features/in-progress/eta.md".to_string(),
                destination: "docs/features/backlog/eta.md".to_string(),
            }]
        );
        assert!(repo.join("docs/features/backlog/eta.md").exists());
        assert!(!branch_exists(&repo, "feature/eta"));
    }

    #[test]
    fn a_fresh_feature_tears_down_without_discarding_anything() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "fresh", None);
        let plan = workflow
            .plan_teardown(&delete_request("fresh"), &strict_options())
            .unwrap();
        assert!(!plan.is_blocked(), "{:?}", plan.blockers);
        assert!(plan.changes.user.is_empty(), "{:?}", plan.changes.user);
        assert!(plan.registered);
        assert_eq!(plan.branch.as_ref().unwrap().source, BranchSource::Registry);

        let summary = workflow
            .teardown_with_options(delete_request("fresh"), strict_options())
            .unwrap();
        assert!(summary.worktree_removed && summary.branch_deleted && summary.registry_updated);
        assert!(summary.discarded_changes.is_empty());
        assert!(!worktree.exists());
        assert!(!DevcontainerModule::baseline_path(&repo, "fresh").exists());
    }

    #[test]
    fn new_user_changes_during_teardown_stop_it_before_removal() {
        let (_temp, repo) = nested_test_repo();
        let mut workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "late", None);
        workflow.before_worktree_removal = Some(|worktree: &Path| {
            fs::write(worktree.join("late.txt"), "written during teardown").unwrap();
        });

        let err = workflow.teardown(delete_request("late")).unwrap_err();
        let Error::TeardownRefused {
            plan,
            changed_anything,
            completed_steps,
            message,
            ..
        } = err
        else {
            panic!("unexpected error {err:?}");
        };
        assert!(changed_anything);
        assert!(
            completed_steps
                .iter()
                .any(|step| step.starts_with("Moved docs/features/in-progress/late.md")),
            "{completed_steps:?}"
        );
        assert!(
            message.starts_with("Stopped tearing down 'late'"),
            "{message}"
        );
        assert!(message.contains("late.txt"), "{message}");
        assert!(plan.blocks_on_uncommitted_changes());
        assert!(worktree.join("late.txt").exists());
        let entry = workflow.state.get_feature("late").unwrap().unwrap();
        assert_eq!(
            entry.status,
            FeatureStatus::Active,
            "the registry entry is kept"
        );
        assert!(branch_exists(&repo, "feature/late"));
    }

    #[test]
    fn files_written_to_adapter_cache_dirs_during_teardown_stop_it_and_survive() {
        let (_temp, repo) = nested_test_repo();
        let mut workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "cache", None);
        // The generic adapter cleanup deletes tmp/; it runs only after the last check, so work
        // written there after the plan stops the teardown instead of being deleted unseen.
        workflow.before_worktree_removal = Some(|worktree: &Path| {
            fs::create_dir_all(worktree.join("tmp")).unwrap();
            fs::write(worktree.join("tmp/agent-output.txt"), "late work\n").unwrap();
        });
        let err = workflow.teardown(delete_request("cache")).unwrap_err();
        let Error::TeardownRefused {
            changed_anything,
            completed_steps,
            ..
        } = err
        else {
            panic!("unexpected error {err:?}");
        };
        assert!(changed_anything);
        assert!(
            !completed_steps
                .iter()
                .any(|step| step.contains("adapter cleanup")),
            "{completed_steps:?}"
        );
        assert_eq!(
            fs::read_to_string(worktree.join("tmp/agent-output.txt")).unwrap(),
            "late work\n"
        );
    }

    #[test]
    fn deleting_tracked_files_during_teardown_does_not_stop_it() {
        let (_temp, repo) = nested_test_repo();
        let mut workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "cleanup", None);
        // Adapter cleanups delete directories such as tmp/; a deleted tracked file loses nothing.
        workflow.before_worktree_removal = Some(|worktree: &Path| {
            fs::remove_file(worktree.join("README.md")).unwrap();
        });
        let summary = workflow.teardown(delete_request("cleanup")).unwrap();
        assert!(summary.worktree_removed);
        assert!(!worktree.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_git_removal_keeps_the_worktree_without_force() {
        use std::os::unix::fs::PermissionsExt;

        let (_temp, repo) = nested_test_repo();
        let mut workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "stuck", None);
        // Generated files git cannot delete: removal fails after the re-check passed.
        workflow.before_worktree_removal = Some(|worktree: &Path| {
            fs::set_permissions(worktree.join(".vscode"), fs::Permissions::from_mode(0o555))
                .unwrap();
        });

        let err = workflow.teardown(delete_request("stuck")).unwrap_err();
        fs::set_permissions(worktree.join(".vscode"), fs::Permissions::from_mode(0o755)).unwrap();
        let Error::TeardownRefused {
            plan,
            changed_anything,
            ..
        } = err
        else {
            panic!("unexpected error {err:?}");
        };
        assert!(changed_anything);
        assert!(
            matches!(plan.blockers.as_slice(), [Blocker::WorktreeRemovalFailed { cause, .. }] if cause.contains("Permission denied")),
            "{:?}",
            plan.blockers
        );
        assert!(
            worktree.exists(),
            "no remove_dir_all fallback without --force"
        );
        let entry = workflow.state.get_feature("stuck").unwrap().unwrap();
        assert_eq!(entry.status, FeatureStatus::Active);

        workflow.before_worktree_removal = None;
        let mut forced = delete_request("stuck");
        forced.force_remove = true;
        assert!(workflow.teardown(forced).unwrap().worktree_removed);
    }

    #[test]
    fn a_status_failure_at_the_recheck_stops_teardown() {
        let (_temp, repo) = nested_test_repo();
        let mut workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "unreadable", None);
        workflow.before_worktree_removal = Some(|worktree: &Path| {
            fs::write(
                worktree.join(".git"),
                "gitdir: /nonexistent/worktrees/unreadable\n",
            )
            .unwrap();
        });

        let err = workflow.teardown(delete_request("unreadable")).unwrap_err();
        let Error::TeardownRefused {
            plan,
            changed_anything,
            ..
        } = err
        else {
            panic!("unexpected error {err:?}");
        };
        assert!(changed_anything);
        assert!(!plan.changes.status_available);
        assert!(
            matches!(plan.blockers.as_slice(), [Blocker::StatusUnavailable { cause, .. }] if cause.contains("not a git repository")),
            "{:?}",
            plan.blockers
        );
        assert!(worktree.exists());
    }

    #[cfg(unix)]
    #[test]
    fn forced_teardown_keeps_the_registry_entry_when_the_directory_survives() {
        use std::os::unix::fs::PermissionsExt;

        let (_temp, repo) = nested_test_repo();
        let mut workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "survivor", None);
        workflow.before_worktree_removal = Some(|worktree: &Path| {
            fs::set_permissions(worktree.join(".vscode"), fs::Permissions::from_mode(0o555))
                .unwrap();
        });
        let mut forced = delete_request("survivor");
        forced.force_remove = true;
        let summary = workflow.teardown(forced.clone()).unwrap();
        fs::set_permissions(worktree.join(".vscode"), fs::Permissions::from_mode(0o755)).unwrap();

        assert!(!summary.worktree_removed);
        assert!(!summary.registry_updated);
        assert!(summary
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Failed to remove worktree directory manually")));
        assert!(summary
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Kept the registry entry of 'survivor'")));
        assert!(summary.discarded_changes.is_empty());
        let entry = workflow.state.get_feature("survivor").unwrap().unwrap();
        assert_eq!(entry.status, FeatureStatus::Active);

        workflow.before_worktree_removal = None;
        assert!(workflow.teardown(forced).unwrap().registry_updated);
    }

    #[test]
    fn unreadable_settings_files_become_plan_warnings() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        start_minimal(&workflow, "warned", None);
        fs::write(repo.join(".branchbox/config.json"), "{ not json").unwrap();
        fs::create_dir_all(repo.join(".branchbox/devcontainer-sync")).unwrap();
        fs::write(repo.join(".branchbox/devcontainer-sync/warned.json"), "[]").unwrap();

        let plan = workflow
            .plan_teardown(&delete_request("warned"), &strict_options())
            .unwrap();
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.starts_with("Using default teardown settings")),
            "{:?}",
            plan.warnings
        );
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Ignoring the devcontainer sync baseline")));
        assert!(plan.defaults.delete_branch_by_default);
        assert!(!plan.is_blocked());
    }

    #[test]
    fn discarding_more_changes_than_are_listed_says_so() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "many", None);
        fs::create_dir(worktree.join("many")).unwrap();
        for index in 0..=MAX_CLASSIFIED_ENTRIES {
            fs::write(worktree.join(format!("many/{index}.txt")), "x").unwrap();
        }
        let summary = workflow
            .teardown_with_options(
                delete_request("many"),
                TeardownOptions {
                    discard_changes: true,
                    require_mergeable_branch: true,
                },
            )
            .unwrap();
        assert!(summary.worktree_removed);
        assert!(summary.discarded_changes.len() < MAX_CLASSIFIED_ENTRIES + 1);
        assert!(summary
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Discarded more uncommitted changes")));
    }

    #[test]
    fn teardown_deletes_the_branch_the_registry_recorded() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        start_minimal(&workflow, "zeta", Some("spike"));
        assert!(branch_exists(&repo, "spike/zeta"));

        let summary = workflow
            .teardown_with_options(delete_request("zeta"), strict_options())
            .unwrap();
        assert_eq!(summary.branch_name, "spike/zeta");
        assert!(summary.branch_deleted);
        assert!(!branch_exists(&repo, "spike/zeta"));
    }

    #[test]
    fn a_missing_branch_is_skipped_with_a_warning() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        start_minimal(&workflow, "nobranch", None);
        let mut request = delete_request("nobranch");
        request.branch_prefix = Some("other".to_string());
        let summary = workflow
            .teardown_with_options(request, strict_options())
            .unwrap();
        assert_eq!(summary.branch_name, "other/nobranch");
        assert!(!summary.branch_deleted);
        assert_eq!(summary.branch_delete_error, None);
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning == "Branch 'other/nobranch' not found; nothing to delete"),
            "{:?}",
            summary.warnings
        );
        // The feature's own branch is untouched.
        assert!(branch_exists(&repo, "feature/nobranch"));
    }

    #[test]
    fn force_names_the_unmerged_commits_it_deleted() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        let worktree = start_minimal(&workflow, "unmerged", None);
        fs::write(worktree.join("work.txt"), "work").unwrap();
        git_in(&worktree, &["add", "work.txt"]);
        git_in(&worktree, &["commit", "-q", "-m", "work"]);

        let err = workflow
            .teardown_with_options(delete_request("unmerged"), strict_options())
            .unwrap_err();
        assert!(err.to_string().contains("--force-delete-branch"), "{err}");
        assert!(worktree.exists());

        let mut forced = delete_request("unmerged");
        forced.force_remove = true;
        let summary = workflow.teardown(forced).unwrap();
        assert_eq!(summary.branch_action, BranchAction::ForceDelete);
        assert!(summary.branch_deleted);
        assert!(summary.warnings.iter().any(|warning| {
            warning
            == "Force-deleted unmerged branch feature/unmerged (1 commit); use --discard-changes \
                to discard files without deleting unmerged commits"
        }));
    }

    #[test]
    fn plan_teardown_reports_unregistered_and_invalid_features() {
        let (_temp, repo) = nested_test_repo();
        let workflow = FeatureWorkflow::new(&repo).unwrap();
        let plan = workflow
            .plan_teardown(&delete_request("ghost"), &strict_options())
            .unwrap();
        assert!(!plan.registered && !plan.worktree.exists);
        assert_eq!(plan.status, None);
        let branch = plan.branch.as_ref().unwrap();
        assert_eq!(branch.source, BranchSource::ConfigPrefix);
        assert_eq!(branch.name, "feature/ghost");
        assert!(!branch.exists);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("needs --force")));

        let err = workflow
            .plan_teardown(&delete_request("Bad Name"), &strict_options())
            .unwrap_err();
        assert!(matches!(err, Error::InvalidFeatureName(_)));
        let err = workflow
            .teardown_with_options(delete_request("Bad Name"), strict_options())
            .unwrap_err();
        assert!(matches!(err, Error::InvalidFeatureName(_)));
        let err = workflow
            .teardown_with_options(delete_request("ghost"), strict_options())
            .unwrap_err();
        assert!(matches!(err, Error::WorktreeMissing { .. }), "{err:?}");
    }

    #[test]
    fn teardown_branch_resolution_prefers_explicit_then_registry_then_config() {
        let config = BranchBoxConfig::default();
        let mut warnings = Vec::new();
        let explicit = resolve_teardown_branch(Some("spike/"), None, &config, "eta", &mut warnings);
        assert_eq!(
            explicit,
            ResolvedBranch {
                name: "spike/eta".to_string(),
                source: BranchSource::ExplicitPrefix
            }
        );

        let mut metadata: FeatureMetadata = serde_json::from_value(serde_json::json!({
            "work_feature": "eta", "branch_name": "custom/eta", "worktree_path": "/r/eta",
            "status": "active", "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap();
        let recorded =
            resolve_teardown_branch(None, Some(&metadata), &config, "eta", &mut warnings);
        assert_eq!(recorded.name, "custom/eta");
        assert_eq!(recorded.source, BranchSource::Registry);
        assert!(warnings.is_empty());

        for unsafe_name in ["-D", "a..b", "a b", "x/"] {
            metadata.branch_name = unsafe_name.to_string();
            let fallback =
                resolve_teardown_branch(None, Some(&metadata), &config, "eta", &mut warnings);
            assert_eq!(fallback.name, "feature/eta", "{unsafe_name}");
            assert_eq!(fallback.source, BranchSource::ConfigPrefix);
        }
        assert_eq!(warnings.len(), 4);

        metadata.branch_name = String::new();
        let empty = resolve_teardown_branch(None, Some(&metadata), &config, "eta", &mut warnings);
        assert_eq!(empty.source, BranchSource::ConfigPrefix);
        assert_eq!(
            warnings.len(),
            4,
            "an empty recorded name is not worth a warning"
        );
    }

    #[test]
    fn error_causes_drop_the_command_failed_prefix() {
        assert_eq!(error_cause(&Error::git("git said no\n")), "git said no");
        assert_eq!(
            error_cause(&Error::validation("bad")),
            "Validation error: bad"
        );
    }

    #[test]
    fn record_teardown_reports_whether_an_entry_was_updated() {
        let (_temp, repo) = nested_test_repo();
        let store = FeatureStateStore::new(&repo);
        assert!(!store.record_teardown("nobody").unwrap());
    }

    #[test]
    fn feature_teardown_force_handles_missing_worktree() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        workflow
            .start(StartRequest {
                name: Some("ghost".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let worktree_path = repo_path.parent().unwrap().join("ghost");
        assert!(worktree_path.exists());
        fs::remove_dir_all(&worktree_path).unwrap();
        assert!(!worktree_path.exists());

        let summary = workflow
            .teardown(TeardownRequest {
                work_feature: "ghost".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();

        assert!(!summary.worktree_removed);
        assert!(summary.branch_deleted);
        assert!(summary
            .warnings
            .iter()
            .any(|warning| warning.contains("already removed")));

        let branch_check = Command::new("git")
            .args(["branch", "--list", "feature/ghost"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_check.stdout)
                .trim()
                .is_empty(),
            "expected feature branch to be deleted"
        );

        let registry_path = repo_path.join(".branchbox/registry.json");
        let registry_data = fs::read_to_string(&registry_path).unwrap();
        let registry: Value = serde_json::from_str(&registry_data).unwrap();
        let entry = registry["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item.get("work_feature").unwrap() == "ghost")
            .unwrap();
        assert_eq!(entry.get("status").unwrap(), "removed");
    }

    #[test]
    fn feature_teardown_moves_spec_to_completed_when_requested() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("complete-me".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let in_progress_spec = summary
            .worktree_path
            .join("docs/features/in-progress/complete-me.md");
        assert!(in_progress_spec.exists());
        assert!(!repo_path
            .join("docs/features/in-progress/complete-me.md")
            .exists());

        workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature.clone(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: true,
                telemetry: false,
            })
            .unwrap();

        let completed_spec = repo_path.join("docs/features/completed/complete-me.md");
        assert!(completed_spec.exists());
        assert!(!in_progress_spec.exists());
        let spec_body = fs::read_to_string(completed_spec).unwrap();
        assert!(spec_body.contains("status: completed"));
        assert!(spec_body.contains("completed:"));
    }

    #[test]
    fn feature_teardown_returns_spec_to_main_when_not_completed() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();
        copy_repo_devcontainer(repo_path);

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let backlog_dir = repo_path.join("docs/features/backlog");
        fs::create_dir_all(&backlog_dir).unwrap();
        fs::write(
            backlog_dir.join("rehydrate.md"),
            "---\nstatus: backlog\n---\n\n# Rehydrate\n",
        )
        .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("rehydrate".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let worktree_spec = summary
            .worktree_path
            .join("docs/features/in-progress/rehydrate.md");
        assert!(worktree_spec.exists());
        assert!(!backlog_dir.join("rehydrate.md").exists());

        workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature.clone(),
                branch_prefix: None,
                delete_branch: true,
                force_delete_branch: false,
                force_remove: true,
                force_remove_modules: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();

        let repo_backlog = repo_path.join("docs/features/backlog/rehydrate.md");
        assert!(repo_backlog.exists());
        let spec_body = fs::read_to_string(repo_backlog).unwrap();
        assert!(spec_body.contains("status: backlog"));
        assert!(!spec_body.contains("branch:"));
        assert!(!spec_body.contains("worktree:"));
    }

    #[test]
    fn feature_start_reuses_existing_env_with_warning() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let worktree_path = repo_path.parent().unwrap().join("reuse-env");
        fs::create_dir_all(worktree_path.join(".devcontainer")).unwrap();
        fs::write(
            worktree_path.join(".env"),
            "FOO=bar\n# Feature-specific configuration (managed by branchbox)\nOLD_VALUE=1\n",
        )
        .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "feature/reuse-env"])
            .status()
            .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("reuse-env".to_string()),
                reuse_existing: true,
                ..StartRequest::default()
            })
            .unwrap();

        assert!(summary
            .warnings
            .iter()
            .any(|w| w.contains("Existing feature-specific configuration replaced")));

        let env_content = fs::read_to_string(summary.worktree_path.join(".env")).unwrap();
        assert!(env_content.contains("WORK_FEATURE=reuse-env"));
        assert!(!env_content.contains("OLD_VALUE"));
    }

    #[test]
    fn feature_start_reuse_requires_existing_branch() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let err = workflow
            .start(StartRequest {
                name: Some("missing-branch".to_string()),
                reuse_existing: true,
                ..StartRequest::default()
            })
            .unwrap_err();

        assert!(format!("{}", err).contains("feature/missing-branch"));
    }

    #[test]
    fn feature_start_reuse_bootstraps_branch_from_remote() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let remote = TempDir::new().unwrap();
        Command::new("git")
            .current_dir(remote.path())
            .args(["init", "--bare"])
            .status()
            .unwrap();

        let remote_name = "upstream";

        Command::new("git")
            .current_dir(repo_path)
            .args([
                "remote",
                "add",
                remote_name,
                remote.path().to_str().unwrap(),
            ])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["push", remote_name, "main"])
            .status()
            .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", "-b", "feature/remote"])
            .status()
            .unwrap();
        fs::write(repo_path.join("REMOTE.md"), "remote branch").unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["add", "REMOTE.md"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["commit", "-m", "Remote branch state"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["push", remote_name, "feature/remote"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", "main"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "-D", "feature/remote"])
            .status()
            .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("remote".to_string()),
                reuse_existing: true,
                ..StartRequest::default()
            })
            .unwrap();

        let branches = Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "--list", "feature/remote"])
            .output()
            .unwrap();
        assert!(!branches.stdout.is_empty());
        assert_eq!(summary.branch_name, "feature/remote");
    }

    #[test]
    fn test_fix_git_worktree_path_converts_absolute() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        // Use complete Git metadata. Git may prune a fabricated worktree entry
        // between fixture setup and the canonical target check below.
        let worktree_path = repo_path.join("feature-test");
        let added = Command::new("git")
            .current_dir(repo_path)
            .args(["worktree", "add", "--detach"])
            .arg(&worktree_path)
            .arg("HEAD")
            .output()
            .unwrap();
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        let git_file = worktree_path.join(".git");
        let metadata_path = repo_path.join(".git/worktrees/feature-test");
        assert!(metadata_path.is_dir());
        assert!(fs::read_to_string(&git_file)
            .unwrap()
            .starts_with("gitdir: /"));

        workflow.fix_git_worktree_path(&worktree_path).unwrap();

        // Verify the path was converted to relative
        let content = fs::read_to_string(&git_file).unwrap();
        assert!(content.starts_with("gitdir: ../.git/worktrees/feature-test"));
        assert!(!content.contains(repo_path.to_str().unwrap()));
    }

    #[test]
    fn test_fix_git_worktree_path_skips_relative() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();

        // Write a .git file with already-relative path
        let git_file = worktree_path.join(".git");
        let original_content = "gitdir: ../main/.git/worktrees/feature-test\n";
        fs::write(&git_file, original_content).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow.fix_git_worktree_path(&worktree_path).unwrap();

        // Verify the path was not changed
        let content = fs::read_to_string(&git_file).unwrap();
        assert_eq!(content, original_content);
    }

    #[test]
    fn test_fix_git_worktree_path_handles_missing_git_file() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();
        // Don't create .git file

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        let result = workflow.fix_git_worktree_path(&worktree_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No .git file found"));
    }

    #[test]
    fn test_fix_git_worktree_path_handles_invalid_gitdir() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();

        // Write an invalid .git file (no gitdir: line)
        let git_file = worktree_path.join(".git");
        fs::write(&git_file, "invalid content\n").unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        let result = workflow.fix_git_worktree_path(&worktree_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No gitdir: line found"));
    }

    #[test]
    fn test_fix_git_worktree_path_ignores_unrelated_sibling_main() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("agentify");
        fs::create_dir_all(&repo_path).unwrap();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        fs::write(repo_path.join("README.md"), "# Agentify\n").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        fs::create_dir_all(temp_dir.path().join("main/.git")).unwrap();

        let worktree_path = temp_dir.path().join("sandbox-devcontainer");
        let created = Command::new("git")
            .args(["worktree", "add", "-b", "feature/sandbox-devcontainer"])
            .arg(&worktree_path)
            .current_dir(&repo_path)
            .output()
            .unwrap();
        assert!(created.status.success());
        let git_file = worktree_path.join(".git");

        let workflow = FeatureWorkflow::new(&repo_path).unwrap();

        workflow.fix_git_worktree_path(&worktree_path).unwrap();

        let content = fs::read_to_string(&git_file).unwrap();
        assert_eq!(
            content,
            "gitdir: ../agentify/.git/worktrees/sandbox-devcontainer\n"
        );
        assert!(!content.contains("../main/"));
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "repaired worktree is unusable: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    #[test]
    fn test_fix_git_worktree_path_translates_container_main_metadata() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let metadata_path = repo_path.join(".git/worktrees/feature-test");
        fs::create_dir_all(&metadata_path).unwrap();
        let git_file = worktree_path.join(".git");
        fs::write(
            &git_file,
            "gitdir: /workspaces/main/.git/worktrees/feature-test\n",
        )
        .unwrap();

        workflow.fix_git_worktree_path(&worktree_path).unwrap();

        assert_eq!(
            fs::read_to_string(&git_file).unwrap(),
            "gitdir: ../.git/worktrees/feature-test\n"
        );
    }

    #[test]
    fn test_in_guest_git_worktree_projection_round_trips_when_repo_is_a_worktree() {
        let temp_dir = TempDir::new().unwrap();
        let main_repo = temp_dir.path().join("agentify");
        fs::create_dir_all(&main_repo).unwrap();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&main_repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&main_repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&main_repo)
            .output()
            .unwrap();
        fs::write(main_repo.join("README.md"), "# Agentify\n").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&main_repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&main_repo)
            .output()
            .unwrap();

        let source_worktree = temp_dir.path().join("coding-demo-e2e-integrated");
        let source_created = Command::new("git")
            .args(["worktree", "add", "-b", "feature/source"])
            .arg(&source_worktree)
            .current_dir(&main_repo)
            .output()
            .unwrap();
        assert!(source_created.status.success());

        let task_worktree = temp_dir.path().join("aex-nested-source");
        let task_created = Command::new("git")
            .args(["worktree", "add", "-b", "feature/aex-nested-source"])
            .arg(&task_worktree)
            .current_dir(&source_worktree)
            .output()
            .unwrap();
        assert!(task_created.status.success());

        let workflow = FeatureWorkflow::new(&source_worktree).unwrap();
        let volumes = in_guest_primary_volumes(
            &source_worktree,
            &task_worktree,
            "/workspaces/aex-nested-source",
        )
        .unwrap();
        assert_eq!(
            volumes[1].get("source").and_then(serde_yaml::Value::as_str),
            Some(
                fs::canonicalize(main_repo.join(".git"))
                    .unwrap()
                    .to_str()
                    .unwrap()
            )
        );
        workflow
            .set_in_guest_git_worktree_path(&task_worktree)
            .unwrap();
        assert_eq!(
            fs::read_to_string(task_worktree.join(".git")).unwrap(),
            "gitdir: /workspaces/main/.git/worktrees/aex-nested-source\n"
        );

        workflow.fix_git_worktree_path(&task_worktree).unwrap();

        assert_eq!(
            fs::read_to_string(task_worktree.join(".git")).unwrap(),
            "gitdir: ../agentify/.git/worktrees/aex-nested-source\n"
        );
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&task_worktree)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "repaired nested worktree is unusable: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    #[test]
    fn test_fix_git_worktree_path_rejects_nested_container_metadata() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();
        let git_file = worktree_path.join(".git");
        let original = "gitdir: /workspaces/main/.git/worktrees/../config\n";
        fs::write(&git_file, original).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let error = workflow
            .fix_git_worktree_path(&worktree_path)
            .unwrap_err()
            .to_string();

        assert!(error.contains("not a single repository worktree entry"));
        assert_eq!(fs::read_to_string(&git_file).unwrap(), original);
    }

    #[test]
    fn test_fix_git_worktree_path_preserves_original_when_target_is_missing() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();
        let git_file = worktree_path.join(".git");
        let original = format!(
            "gitdir: {}\n",
            repo_path.join(".git/worktrees/missing").display()
        );
        fs::write(&git_file, &original).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let error = workflow
            .fix_git_worktree_path(&worktree_path)
            .unwrap_err()
            .to_string();

        assert!(error.contains("preserving the original worktree pointer"));
        assert_eq!(fs::read_to_string(&git_file).unwrap(), original);
    }

    #[test]
    fn test_prepare_sbx_devcontainer_adds_git_and_restart_compose_facade() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let devcontainer_dir = repo_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{
  "dockerComposeFile": "compose.yaml",
  "service": "app"
}
"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    image: alpine:3.19
    environment: {HOST_AUTH: "${HOST_AUTH_ENV:?discarded-primary-environment}"}
    volumes:
      - ../../main/.git:/workspaces/main/.git
"#,
        )
        .unwrap();

        prepare_sbx_devcontainer_config(repo_path, repo_path, &["app".to_string()]).unwrap();

        let generated: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG)).unwrap(),
        )
        .unwrap();
        assert_eq!(generated["runServices"], serde_json::json!(["app"]));
        assert_eq!(
            generated["dockerComposeFile"],
            serde_json::json!(["compose.yaml", SBX_COMPOSE_OVERRIDE])
        );

        let facade: serde_yaml::Value = serde_yaml::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap(),
        )
        .unwrap();
        let app = &facade["services"]["app"];
        assert_eq!(app["restart"].as_str(), Some("unless-stopped"));
        assert_eq!(
            app["volumes"][0]["source"].as_str(),
            Some(
                fs::canonicalize(repo_path.join(".git"))
                    .unwrap()
                    .to_str()
                    .unwrap()
            )
        );
        assert_eq!(
            app["volumes"][0]["target"].as_str(),
            Some(CONTAINER_MAIN_GIT_TARGET)
        );
        assert_eq!(app["volumes"][0]["type"].as_str(), Some("bind"));
    }

    #[test]
    fn test_prepare_sbx_devcontainer_restarts_primary_without_git_facade() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let devcontainer_dir = repo_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  app:\n    image: alpine:3.19\n",
        )
        .unwrap();

        prepare_sbx_devcontainer_config(repo_path, repo_path, &[]).unwrap();

        let facade: serde_yaml::Value = serde_yaml::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            facade["services"]["app"]["restart"].as_str(),
            Some("unless-stopped")
        );
        assert!(facade["services"]["app"].get("volumes").is_none());
    }

    #[test]
    fn test_prepare_sbx_devcontainer_removes_stale_runtime_facade() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let devcontainer_dir = repo_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        let source_config = devcontainer_dir.join("devcontainer.json");
        fs::write(
            &source_config,
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  app:\n    image: alpine:3.19\n",
        )
        .unwrap();
        prepare_sbx_devcontainer_config(repo_path, repo_path, &[]).unwrap();
        assert!(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG).exists());
        assert!(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE).exists());

        fs::write(&source_config, r#"{"image":"alpine:3.19"}"#).unwrap();
        prepare_sbx_devcontainer_config(repo_path, repo_path, &[]).unwrap();

        assert!(!devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG).exists());
        assert!(!devcontainer_dir.join(SBX_COMPOSE_OVERRIDE).exists());
        assert_eq!(
            fs::read_to_string(source_config).unwrap(),
            r#"{"image":"alpine:3.19"}"#
        );
    }

    #[test]
    fn test_in_guest_sanitizer_removes_host_hooks_ambient_env_and_supervisor_docker_feature() {
        let mut config = serde_json::json!({
            "initializeCommand": "op read secret",
            "features": {
                "ghcr.io/devcontainers/features/docker-outside-of-docker:1": {},
                "ghcr.io/devcontainers/features/docker-in-docker:2": {},
                "ghcr.io/devcontainers-contrib/features/docker-from-docker:1": {},
                "ghcr.io/devcontainers/features/node:1": {}
            },
            "containerEnv": {"OPENAI_API_KEY": "${localEnv:OPENAI_API_KEY}"},
            "remoteEnv": {"TOKEN": "ambient"},
            "mounts": [
                "source=${localEnv:HOME}/.ssh/id.pub,target=/tmp/id.pub,type=bind",
                "source=project-data,target=/project-data,type=volume"
            ],
            "appPort": ["127.0.0.1:2222:22"],
            "forwardPorts": [3000, 5432],
            "portsAttributes": {"3000": {"label": "Repository app"}},
            "otherPortsAttributes": {"onAutoForward": "openBrowser"},
            "securityOpt": ["seccomp=unconfined"],
            "runArgs": [
                "--env-file", "../.env", "--shm-size", "8g", "--shm-size=16g",
                "--ipc=host", "--network", "host", "--device=/dev/kvm",
                "--cap-add", "SYS_ADMIN", "--privileged", "--init"
            ],
            "postCreateCommand": "bin/setup"
        });

        sanitize_in_guest_devcontainer_json(&mut config, true, false).unwrap();

        assert!(config.get("initializeCommand").is_none());
        assert!(config["containerEnv"].as_object().unwrap().is_empty());
        assert!(config.get("remoteEnv").is_none());
        assert!(config["features"]
            .get("ghcr.io/devcontainers/features/docker-outside-of-docker:1")
            .is_none());
        assert!(config["features"]
            .get("ghcr.io/devcontainers/features/docker-in-docker:2")
            .is_none());
        assert!(config["features"]
            .get("ghcr.io/devcontainers-contrib/features/docker-from-docker:1")
            .is_none());
        assert!(config["features"]
            .get("ghcr.io/devcontainers/features/node:1")
            .is_some());
        assert!(config.get("mounts").is_none());
        assert!(config.get("appPort").is_none());
        assert!(config.get("forwardPorts").is_none());
        assert!(config.get("portsAttributes").is_none());
        assert!(config.get("otherPortsAttributes").is_none());
        assert_eq!(
            config["securityOpt"],
            serde_json::json!([IN_GUEST_SECCOMP_SECURITY_OPTION])
        );
        assert_eq!(config["runArgs"], serde_json::json!(["--init"]));
        assert_eq!(config["postCreateCommand"], "bin/setup");
    }

    #[test]
    fn test_in_guest_sanitizer_preserves_only_safe_literal_connectivity_environment() {
        let mut config = serde_json::json!({
            "image": "alpine",
            "containerEnv": {
                "DB_HOST": "postgres",
                "PORT": "3000",
                "RAILS_ENV": "development",
                "TUNNEL_HOST": "edge",
                "OPENAI_API_KEY": "secret",
                "DB_PASSWORD": "postgres",
                "ENV_FILE": ".env",
                "REDIS_HOST": "${localEnv:REDIS_HOST}"
            }
        });
        sanitize_in_guest_devcontainer_json(&mut config, true, false).unwrap();
        assert_eq!(
            config["containerEnv"],
            serde_json::json!({
                "DB_HOST": "postgres",
                "PORT": "3000",
                "RAILS_ENV": "development"
            })
        );
    }

    #[test]
    fn test_in_guest_compose_security_option_is_owned_only_by_the_compose_facade() {
        let mut config = serde_json::json!({
            "dockerComposeFile": "compose.yaml",
            "service": "app",
            "securityOpt": ["seccomp=unconfined"]
        });

        sanitize_in_guest_devcontainer_json(&mut config, false, false).unwrap();

        assert!(config.get("securityOpt").is_none());
    }

    #[test]
    fn test_preloaded_image_mode_disables_devcontainer_image_derivation() {
        let mut config = serde_json::json!({
            "build": {"dockerfile": "Dockerfile"},
            "features": {"ghcr.io/devcontainers/features/rust:1": {}},
            "updateRemoteUserUID": true,
            "postCreateCommand": "bin/setup"
        });

        sanitize_in_guest_devcontainer_json(&mut config, false, true).unwrap();

        assert!(config.get("build").is_none());
        assert!(config.get("features").is_none());
        assert_eq!(config["updateRemoteUserUID"], false);
        assert_eq!(config["postCreateCommand"], "bin/setup");
    }

    #[test]
    fn repository_workspace_checks_out_in_place_and_leaves_no_worktree() {
        // A caller that clones per run has no second worktree to isolate from,
        // so the branch belongs in the repository. Nothing beside it should be
        // created, and the repository must never be removed as cleanup.
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        let git = GitWorktree::new(repo_path).unwrap();
        git.checkout_feature_branch("feature/in-place", None)
            .expect("branch checks out in the repository");

        assert!(
            git.branch_exists("feature/in-place").unwrap(),
            "the feature branch exists"
        );
        // Ask Git what worktrees this repository has rather than counting
        // entries beside it: the repository is created in the system temporary
        // directory, which every other test shares, so a directory another test
        // happened to create between two readings looked like a worktree here.
        let worktrees = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&worktrees.stdout);
        assert_eq!(
            listed
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count(),
            1,
            "no worktree was created beside it: {listed}"
        );
        assert!(repo_path.join(".git").exists(), "the repository is intact");
    }

    #[test]
    fn test_spool_volume_keeps_its_signed_name_under_a_compose_project() {
        // Compose namespaces a volume with the project name unless the volume
        // declares an explicit `name`. The running container is inspected for the
        // signed volume name, so a namespaced volume fails closed as unsigned.
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        let compose = devcontainer_dir.join("compose.yaml");
        fs::write(
            &compose,
            r#"services:
  app:
    image: app:mutable
"#,
        )
        .unwrap();
        let images =
            BTreeMap::from([("app".to_string(), format!("app@sha256:{}", "c".repeat(64)))]);
        let spool_volumes = vec![(
            "branchbox-tool-requests-abc".to_string(),
            PathBuf::from("/run/branchbox/leases/tool-requests/browser-verification"),
        )];

        prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("app"),
            "/workspaces/coding-demo",
            &[read_in_guest_compose_document(&worktree_path, &compose).unwrap()],
            &InGuestComposeAssignment {
                project_environment: None,
                service_images: &images,
                workspace_consumer: false,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &spool_volumes,
            },
        )
        .unwrap();

        let facade: serde_yaml::Value = serde_yaml::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap(),
        )
        .unwrap();
        // Pinned at the top level, so Compose creates it under exactly this name.
        assert_eq!(
            facade["volumes"]["branchbox-tool-requests-abc"]["name"].as_str(),
            Some("branchbox-tool-requests-abc")
        );
        let mounted = facade["services"]["app"]["volumes"]
            .as_sequence()
            .expect("primary service carries its volumes")
            .iter()
            .any(|mount| {
                mount["type"].as_str() == Some("volume")
                    && mount["source"].as_str() == Some("branchbox-tool-requests-abc")
                    && mount["target"].as_str()
                        == Some("/run/branchbox/leases/tool-requests/browser-verification")
            });
        assert!(mounted, "spool volume is attached to the primary service");
    }

    #[test]
    fn test_preloaded_images_replace_builds_and_disable_pulls_for_all_runnable_services() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        let compose = devcontainer_dir.join("compose.yaml");
        fs::write(
            &compose,
            r#"services:
  app:
    build: {context: ..}
    depends_on: [database, tunnel]
  database:
    image: database:mutable
  tunnel:
    image: tunnel:mutable
"#,
        )
        .unwrap();
        let digest = "c".repeat(64);
        let images = BTreeMap::from([
            (
                "app".to_string(),
                format!("registry.example/team/app@sha256:{digest}"),
            ),
            (
                "database".to_string(),
                format!("registry.example/team/database@sha256:{digest}"),
            ),
        ]);

        let omitted = prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("app"),
            "/workspaces/coding-demo",
            &[read_in_guest_compose_document(&worktree_path, &compose).unwrap()],
            &InGuestComposeAssignment {
                project_environment: None,
                service_images: &images,
                workspace_consumer: false,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &[],
            },
        )
        .unwrap();
        assert_eq!(omitted, ["tunnel".to_string()].into_iter().collect());

        let rendered = fs::read_to_string(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap();
        assert!(rendered.contains("build: !reset null"));
        let facade: serde_yaml::Value = serde_yaml::from_str(&rendered).unwrap();
        for (service, image) in &images {
            assert_eq!(
                facade["services"][service]["image"].as_str(),
                Some(image.as_str())
            );
            assert_eq!(
                facade["services"][service]["pull_policy"].as_str(),
                Some("never")
            );
            let build = &facade["services"][service]["build"];
            assert!(matches!(
                build,
                serde_yaml::Value::Tagged(tagged)
                    if tagged.tag == serde_yaml::value::Tag::new("!reset")
                        && tagged.value.is_null()
            ));
        }

        let incomplete = BTreeMap::from([(
            "app".to_string(),
            format!("registry.example/team/app@sha256:{digest}"),
        )]);
        let error = prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("app"),
            "/workspaces/coding-demo",
            &[read_in_guest_compose_document(&worktree_path, &compose).unwrap()],
            &InGuestComposeAssignment {
                project_environment: None,
                service_images: &incomplete,
                workspace_consumer: false,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &[],
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("missing: database"));

        fs::write(
            &compose,
            "include: [sidecars.yaml]\nservices:\n  app:\n    image: app:mutable\n",
        )
        .unwrap();
        let error = prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("app"),
            "/workspaces/coding-demo",
            &[read_in_guest_compose_document(&worktree_path, &compose).unwrap()],
            &InGuestComposeAssignment {
                project_environment: None,
                service_images: &BTreeMap::from([("app".to_string(), images["app"].clone())]),
                workspace_consumer: false,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &[],
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("reject Compose include"));
    }

    #[test]
    fn test_in_guest_preloaded_coverage_uses_the_sanitized_source_snapshot() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        let compose = devcontainer_dir.join("compose.yaml");
        fs::write(
            &compose,
            "services:\n  app:\n    image: app:mutable\n    depends_on: [worker]\n  worker:\n    image: worker:mutable\n",
        )
        .unwrap();
        let parsed = read_in_guest_compose_document(&worktree_path, &compose).unwrap();
        // A prior coding process can rewrite its source after BranchBox parses it.
        // Image coverage must be checked against the bytes that become CLI inputs.
        fs::write(&compose, "services:\n  app:\n    image: app:mutable\n").unwrap();
        let images =
            BTreeMap::from([("app".to_string(), format!("app@sha256:{}", "a".repeat(64)))]);
        let error = prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("app"),
            "/workspaces/coding-demo",
            &[parsed],
            &InGuestComposeAssignment {
                project_environment: None,
                service_images: &images,
                workspace_consumer: false,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &[],
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing: worker"));
    }

    #[test]
    fn test_in_guest_compose_facade_omits_platform_connectors_and_ambient_authority() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            worktree_path.join(".gitignore"),
            ".devcontainer/.1password-service-account-token\n",
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join(".1password-service-account-token"),
            "ignored-credential-must-not-be-mounted\n",
        )
        .unwrap();
        let compose = devcontainer_dir.join("compose.yaml");
        fs::write(
            &compose,
            r#"services:
  tailscale:
    image: tailscale/tailscale:stable
    environment:
      TS_STATE_DIR: /var/lib/tailscale
      TS_USERSPACE: "false"
    volumes: [tailscale-state:/var/lib/tailscale]
    devices: [/dev/net/tun:/dev/net/tun]
    cap_add: [net_admin, net_raw]
    network_mode: service:rails-app
    depends_on: [rails-app]
  rails-app:
    build:
      context: ..
      dockerfile: .devcontainer/Dockerfile
    volumes:
      - ../..:/workspaces:cached
      - ../../main/.git:/workspaces/main/.git:cached
      - agentify_codex_auth:/home/vscode/.codex
      - ${HOME}/.ssh/id.pub:/tmp/id.pub:ro
      - ./.1password-service-account-token:/run/secrets/agentify-op-service-account-token:ro
    ports: ["127.0.0.1:2222:22"]
    expose: ["3000"]
    env_file: [.rails-app.env]
    depends_on: [postgres, cloudflared, tailscale]
  postgres:
    image: postgres:16
    volumes:
      - ./ignored-db-credential:/run/secrets/ignored-db-credential:ro
      - postgres-data:/var/lib/postgresql/data
    ports: ["5432:5432"]
    expose: ["5432"]
    env_file: [.postgres.env]
  cloudflared:
    image: cloudflare/cloudflared:latest
    env_file: [.cloudflared.env]
volumes:
  agentify_codex_auth:
  postgres-data:
  tailscale-state:
"#,
        )
        .unwrap();

        prepare_sbx_compose_override(
            repo_path,
            &devcontainer_dir,
            Some("rails-app"),
            std::slice::from_ref(&compose),
            None,
            None,
            None,
        )
        .unwrap();
        let project_environment = repo_path.join("project-environment.env");
        fs::write(
            &project_environment,
            "ACCOUNT_NAME=Matchup\nADMIN_PASSWORD=never-render-this-value\n",
        )
        .unwrap();
        let omitted = prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("rails-app"),
            "/workspaces/coding-demo",
            &[read_in_guest_compose_document(&worktree_path, &compose).unwrap()],
            &InGuestComposeAssignment {
                project_environment: Some(&project_environment),
                service_images: &BTreeMap::new(),
                workspace_consumer: false,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &[],
            },
        )
        .unwrap();
        assert_eq!(
            omitted,
            ["cloudflared".to_string(), "tailscale".to_string()]
                .into_iter()
                .collect()
        );
        let rendered = fs::read_to_string(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap();
        assert!(rendered.contains("!override"));
        assert!(!rendered.contains("agentify_codex_auth"));
        assert!(!rendered.contains("${HOME}"));
        assert!(!rendered.contains(".1password-service-account-token"));
        assert!(!rendered.contains("ignored-db-credential"));
        assert!(!rendered.contains("postgres-data"));
        assert!(!rendered.contains("127.0.0.1:2222:22"));
        assert!(!rendered.contains("never-render-this-value"));
        assert!(rendered.contains("format: raw"));
        assert!(rendered.contains(&project_environment.to_string_lossy().into_owned()));
        let facade: serde_yaml::Value = serde_yaml::from_str(&rendered).unwrap();
        let rails = &facade["services"]["rails-app"];
        let rails_volumes = match &rails["volumes"] {
            serde_yaml::Value::Tagged(tagged) => tagged.value.as_sequence().unwrap(),
            other => panic!("expected overridden rails volumes, got {other:?}"),
        };
        assert_eq!(rails_volumes.len(), 2);
        assert_eq!(
            rails_volumes[0]["source"].as_str(),
            Some(fs::canonicalize(&worktree_path).unwrap().to_str().unwrap())
        );
        assert_eq!(
            rails_volumes[0]["target"].as_str(),
            Some("/workspaces/coding-demo")
        );
        assert_eq!(rails_volumes[0]["type"].as_str(), Some("bind"));
        assert_eq!(
            rails_volumes[1]["source"].as_str(),
            Some(
                fs::canonicalize(repo_path.join(".git"))
                    .unwrap()
                    .to_str()
                    .unwrap()
            )
        );
        assert_eq!(
            rails_volumes[1]["target"].as_str(),
            Some(CONTAINER_MAIN_GIT_TARGET)
        );
        assert!(matches!(rails["env_file"], serde_yaml::Value::Tagged(_)));
        // Dependencies are filtered in each sanitized source before Compose
        // merges the ordered inputs; the facade must not replace that merge.
        assert!(rails.get("depends_on").is_none());
        assert_eq!(rails["shm_size"].as_str(), Some(IN_GUEST_SHM_SIZE));
        let security_options = match &rails["security_opt"] {
            serde_yaml::Value::Tagged(tagged) => tagged.value.as_sequence().unwrap(),
            other => panic!("expected overridden primary security options, got {other:?}"),
        };
        assert_eq!(
            security_options,
            &[serde_yaml::Value::String(
                IN_GUEST_SECCOMP_SECURITY_OPTION.into()
            )]
        );
        assert!(rails.get("ipc").is_none());
        for name in ["rails-app", "postgres", "cloudflared", "tailscale"] {
            for key in ["ports", "expose"] {
                let value = &facade["services"][name][key];
                assert!(matches!(value, serde_yaml::Value::Tagged(_)));
                let sequence = match value {
                    serde_yaml::Value::Tagged(tagged) => tagged.value.as_sequence().unwrap(),
                    _ => unreachable!(),
                };
                assert!(sequence.is_empty(), "{name}.{key} retained publication");
            }
        }
        for name in ["postgres", "cloudflared", "tailscale"] {
            let value = &facade["services"][name]["volumes"];
            let sequence = match value {
                serde_yaml::Value::Tagged(tagged) => tagged.value.as_sequence().unwrap(),
                other => panic!("expected overridden {name} volumes, got {other:?}"),
            };
            assert!(sequence.is_empty(), "{name} retained a repository volume");
        }
        assert!(matches!(
            facade["services"]["cloudflared"]["profiles"],
            serde_yaml::Value::Tagged(_)
        ));
        assert!(matches!(
            facade["services"]["tailscale"]["devices"],
            serde_yaml::Value::Tagged(_)
        ));
        assert!(matches!(
            facade["services"]["tailscale"]["cap_add"],
            serde_yaml::Value::Tagged(_)
        ));
        assert!(facade["services"]["postgres"].get("profiles").is_none());
        let unsigned_environment = match &rails["environment"] {
            serde_yaml::Value::Tagged(tagged) => tagged.value.as_mapping().unwrap(),
            other => panic!("expected overridden rails environment, got {other:?}"),
        };
        assert!(
            unsigned_environment.is_empty(),
            "an assignment without a workspace consumer must not add Git ownership exceptions"
        );
    }

    #[test]
    fn test_in_guest_compose_interpolation_is_removed_from_cli_inputs_before_merge() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":["compose.yaml","compose.extra.yaml"],"service":"app","workspaceFolder":"/workspaces/coding-demo"}"#,
        )
        .unwrap();
        let first = r#"x-database: &database
  volumes:
    - ${HOST_SECRET:?must-not-be-evaluated}:/run/secrets/db:ro
services:
  app:
    image: alpine:3.19
    volumes:
      - ${HOST_AUTH:?must-not-be-evaluated}:/run/secrets/auth:ro
    ports: ["${HOST_PORT:?must-not-be-evaluated}:3000"]
    env_file: ["${HOST_ENV_FILE:?must-not-be-evaluated}"]
    depends_on: [database]
  database:
    <<: *database
    image: postgres:16
    environment: {POSTGRES_USER: kept-for-dependency}
  cloudflared:
    image: "${TUNNEL_IMAGE:?discarded-connector-image}"
    environment: {TUNNEL_TOKEN: "${TUNNEL_TOKEN:?discarded-connector-environment}"}
    command: ["tunnel", "--token", "${TUNNEL_COMMAND_TOKEN:?discarded-connector-command}"]
    entrypoint: ["${TUNNEL_ENTRYPOINT:?discarded-connector-entrypoint}"]
    secrets: [repo-secret]
secrets:
  repo-secret:
    file: ./ignored-repo-secret
"#;
        let second = r#"services:
  database:
    volumes:
      - ${DB_VOLUME:-postgres-data}:${DB_TARGET:-/var/lib/postgresql/data}
      - ./ignored-secret:/run/secrets/ignored-secret:ro
    expose: ["5432"]
volumes:
  postgres-data:
    driver: local
    driver_opts: {type: none, device: /etc, o: bind}
"#;
        fs::write(devcontainer_dir.join("compose.yaml"), first).unwrap();
        fs::write(devcontainer_dir.join("compose.extra.yaml"), second).unwrap();

        prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap();

        let generated: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG)).unwrap(),
        )
        .unwrap();
        let references = generated["dockerComposeFile"].as_array().unwrap();
        assert_eq!(references.len(), 3);
        assert_eq!(references[2].as_str(), Some(SBX_COMPOSE_OVERRIDE));
        for reference in &references[..2] {
            let reference = reference.as_str().unwrap();
            assert!(reference.starts_with(SBX_COMPOSE_INPUT_PREFIX));
            let path = devcontainer_dir.join(reference);
            let source = fs::read_to_string(&path).unwrap();
            assert!(source.starts_with(SBX_COMPOSE_INPUT_MARKER));
            assert!(
                !source.contains("${"),
                "interpolation reached Compose: {source}"
            );
            assert!(!source.contains("/run/secrets/"));
            assert!(!source.contains("ignored-secret"));
            assert!(!source.contains("ignored-repo-secret"));
            assert!(!source.contains("/etc"));
            let document: serde_yaml::Value = serde_yaml::from_str(&source).unwrap();
            assert!(document.get("x-database").is_none());
            assert!(document.get("volumes").is_none());
            assert!(document.get("secrets").is_none());
            for service in document["services"].as_mapping().unwrap().values() {
                for key in [
                    "volumes", "ports", "expose", "env_file", "devices", "secrets", "configs",
                ] {
                    assert!(
                        service.get(key).is_none(),
                        "{key} reached Compose: {source}"
                    );
                }
            }
            if document["services"]["database"]
                .get("environment")
                .is_some()
            {
                assert_eq!(
                    document["services"]["database"]["environment"]["POSTGRES_USER"].as_str(),
                    Some("kept-for-dependency")
                );
            }
            assert!(document["services"]["app"].get("environment").is_none());
            assert!(document["services"]["cloudflared"]
                .get("environment")
                .is_none());
            if !document["services"]["cloudflared"].is_null() {
                assert_eq!(
                    document["services"]["cloudflared"]["image"].as_str(),
                    Some(IN_GUEST_OMITTED_CONNECTOR_IMAGE)
                );
                assert!(document["services"]["cloudflared"].get("command").is_none());
                assert!(document["services"]["cloudflared"]
                    .get("entrypoint")
                    .is_none());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        let facade = fs::read_to_string(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap();
        assert!(!facade.contains("${"));
        assert!(!facade.contains("ignored-secret"));
        assert!(!facade.contains("HOST_SECRET"));
        assert_eq!(
            fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap(),
            first
        );
        assert_eq!(
            fs::read_to_string(devcontainer_dir.join("compose.extra.yaml")).unwrap(),
            second
        );
        // A resumed in-guest start may regenerate the same private inputs.
        prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap();

        // Opt-in local Compose proof uses the exact paths handed to the Dev
        // Containers CLI. No Docker daemon or image pull is needed for `config`.
        if std::env::var_os("BRANCHBOX_VERIFY_COMPOSE_CONFIG").is_some() {
            let mut command = Command::new("docker");
            command.arg("compose");
            for reference in references {
                command
                    .arg("-f")
                    .arg(devcontainer_dir.join(reference.as_str().unwrap()));
            }
            let output = command
                .args(["config", "--format", "json"])
                .env("COMPOSE_PROJECT_NAME", "branchbox-security-test")
                .env_remove("HOST_SECRET")
                .env_remove("HOST_AUTH")
                .env_remove("HOST_PORT")
                .env_remove("HOST_ENV_FILE")
                .env_remove("HOST_AUTH_ENV")
                .env_remove("TUNNEL_TOKEN")
                .env_remove("TUNNEL_IMAGE")
                .env_remove("TUNNEL_COMMAND_TOKEN")
                .env_remove("TUNNEL_ENTRYPOINT")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Compose rejected sanitized inputs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let effective: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            for (name, service) in effective["services"].as_object().unwrap() {
                assert!(service.get("ports").is_none(), "{name} published a port");
                assert!(service.get("secrets").is_none(), "{name} retained a secret");
                assert!(service.get("devices").is_none(), "{name} retained a device");
                if name != "app" {
                    assert!(service.get("volumes").is_none(), "{name} retained a mount");
                }
            }
            let volumes = effective["services"]["app"]["volumes"].as_array().unwrap();
            assert_eq!(volumes.len(), 2, "only signed primary binds may survive");
            assert_eq!(
                volumes[0]["source"].as_str(),
                Some(fs::canonicalize(&worktree_path).unwrap().to_str().unwrap())
            );
            assert_eq!(
                volumes[1]["source"].as_str(),
                Some(
                    fs::canonicalize(repo_path.join(".git"))
                        .unwrap()
                        .to_str()
                        .unwrap()
                )
            );
        }
        // Shrinking the source list removes the now-unreferenced, marker-owned
        // copy, even when an earlier start generated it in a different file.
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":["compose.yaml"],"service":"app","workspaceFolder":"/workspaces/coding-demo"}"#,
        )
        .unwrap();
        prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap();
        assert!(!devcontainer_dir
            .join(format!("{SBX_COMPOSE_INPUT_PREFIX}1.yaml"))
            .exists());
    }

    #[test]
    fn test_in_guest_split_connector_is_inert_in_every_compose_input() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":["compose.yaml","compose.extra.yaml"],"service":"app"}"#,
        )
        .unwrap();
        let first = r#"services:
  app: {image: alpine:3.19}
  proxy:
    image: cloudflare/cloudflared:latest
    environment: {TOKEN: "${FIRST_TOKEN:?discarded-connector-environment}"}
"#;
        let second = r#"services:
  proxy:
    command: ["tunnel", "--token", "${TUNNEL_TOKEN:?discarded-connector-command}"]
    entrypoint: ["${TUNNEL_ENTRYPOINT:?discarded-connector-entrypoint}"]
"#;
        fs::write(devcontainer_dir.join("compose.yaml"), first).unwrap();
        fs::write(devcontainer_dir.join("compose.extra.yaml"), second).unwrap();

        prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap();
        let generated: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG)).unwrap(),
        )
        .unwrap();
        let references = generated["dockerComposeFile"].as_array().unwrap();
        assert_eq!(references.len(), 3);
        for reference in &references[..2] {
            let source =
                fs::read_to_string(devcontainer_dir.join(reference.as_str().unwrap())).unwrap();
            assert!(!source.contains("${"), "{source}");
            let document: serde_yaml::Value = serde_yaml::from_str(&source).unwrap();
            let proxy = document["services"]["proxy"].as_mapping().unwrap();
            assert_eq!(proxy.len(), 1, "discarded proxy field survived: {source}");
            assert_eq!(
                document["services"]["proxy"]["image"].as_str(),
                Some(IN_GUEST_OMITTED_CONNECTOR_IMAGE)
            );
        }
        assert_eq!(
            fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap(),
            first
        );
        assert_eq!(
            fs::read_to_string(devcontainer_dir.join("compose.extra.yaml")).unwrap(),
            second
        );

        if std::env::var_os("BRANCHBOX_VERIFY_COMPOSE_CONFIG").is_some() {
            let mut command = Command::new("docker");
            command.arg("compose");
            for reference in references {
                command
                    .arg("-f")
                    .arg(devcontainer_dir.join(reference.as_str().unwrap()));
            }
            let output = command
                .args(["config", "--format", "json"])
                .env("COMPOSE_PROJECT_NAME", "branchbox-split-connector-test")
                .env_remove("FIRST_TOKEN")
                .env_remove("TUNNEL_TOKEN")
                .env_remove("TUNNEL_ENTRYPOINT")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Compose rejected sanitized inputs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let effective: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let proxy = &effective["services"]["proxy"];
            assert!(proxy.get("command").is_none());
            assert!(proxy.get("entrypoint").is_none());
            assert!(proxy.get("environment").is_none());
        }
    }

    #[test]
    fn test_in_guest_split_compose_dependencies_exclude_disabled_connector() {
        for (name, first_dependencies, second_dependencies, expected) in [
            (
                "short-command-only",
                "[database, cloudflared]",
                "",
                vec!["database"],
            ),
            (
                "short-later-dependency",
                "[database, cloudflared]",
                "    depends_on: [cache]\n",
                vec!["database", "cache"],
            ),
            (
                "short-later-override",
                "[database, cloudflared]",
                "    depends_on: !override [cache, cloudflared]\n",
                vec!["cache"],
            ),
            (
                "long-command-only",
                "{database: {condition: service_started}, cloudflared: {condition: service_started}}",
                "",
                vec!["database"],
            ),
        ] {
            let temp_dir = setup_test_repo();
            let repo_path = temp_dir.path();
            let worktree_path = repo_path.join("coding-demo");
            let devcontainer_dir = worktree_path.join(".devcontainer");
            fs::create_dir_all(&devcontainer_dir).unwrap();
            fs::write(
                devcontainer_dir.join("devcontainer.json"),
                r#"{"dockerComposeFile":["compose.yaml","compose.extra.yaml"],"service":"app"}"#,
            )
            .unwrap();
            let first = format!(
                "services:\n  app:\n    image: alpine:3.19\n    depends_on: {first_dependencies}\n  database:\n    image: alpine:3.19\n  cache:\n    image: alpine:3.19\n  cloudflared:\n    image: cloudflare/cloudflared:latest\n"
            );
            let second = format!(
                "services:\n  app:\n    command: [sleep, infinity]\n{second_dependencies}"
            );
            fs::write(devcontainer_dir.join("compose.yaml"), &first).unwrap();
            fs::write(devcontainer_dir.join("compose.extra.yaml"), &second).unwrap();

            prepare_in_guest_devcontainer_config(
                repo_path,
                &worktree_path,
                &InGuestFacadePlan::empty_for_tests(),
            )
            .unwrap();
            let generated: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG)).unwrap(),
            )
            .unwrap();
            let references = generated["dockerComposeFile"].as_array().unwrap();
            assert_eq!(references.len(), 3, "{name}");
            let sanitized_first: serde_yaml::Value = serde_yaml::from_str(
                &fs::read_to_string(devcontainer_dir.join(references[0].as_str().unwrap()))
                    .unwrap(),
            )
            .unwrap();
            let dependencies = &sanitized_first["services"]["app"]["depends_on"];
            match dependencies {
                serde_yaml::Value::Sequence(values) => {
                    assert_eq!(values, &[serde_yaml::Value::String("database".into())], "{name}");
                }
                serde_yaml::Value::Mapping(values) => {
                    assert_eq!(values.len(), 1, "{name}");
                    assert!(values.contains_key("database"), "{name}");
                }
                other => panic!("{name}: unexpected dependencies {other:?}"),
            }
            assert_eq!(fs::read_to_string(devcontainer_dir.join("compose.yaml")).unwrap(), first);
            assert_eq!(
                fs::read_to_string(devcontainer_dir.join("compose.extra.yaml")).unwrap(),
                second
            );

            if std::env::var_os("BRANCHBOX_VERIFY_COMPOSE_CONFIG").is_some() {
                let mut command = Command::new("docker");
                command.arg("compose");
                for reference in references {
                    command.arg("-f").arg(devcontainer_dir.join(reference.as_str().unwrap()));
                }
                let output = command
                    .args(["config", "--format", "json"])
                    .env("COMPOSE_PROJECT_NAME", format!("branchbox-dependencies-{name}"))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{name}: Compose rejected sanitized inputs: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let effective: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                let dependencies = effective["services"]["app"]["depends_on"]
                    .as_object()
                    .unwrap();
                assert_eq!(dependencies.len(), expected.len(), "{name}");
                for dependency in expected {
                    assert!(dependencies.contains_key(dependency), "{name}: missing {dependency}");
                }
                assert!(effective["services"]["cloudflared"].is_null(), "{name}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_in_guest_workspace_consumer_uses_private_cli_inputs() {
        use std::os::unix::fs::symlink;
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":["compose.yaml","compose.extra.yaml"],"service":"app"}"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  app:\n    image: '${REPO_IMAGE:?discarded}'\n    build: {context: .}\n    label_file: '${HOST_LABEL_FILE:?discarded}'\n    develop: {watch: [{path: /etc, action: sync, target: /tmp/host}] }\n  proxy:\n    image: cloudflare/cloudflared:latest\n",
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.extra.yaml"),
            "services:\n  proxy:\n    command: ['tunnel', '--token', '${TUNNEL_TOKEN:?discarded}']\n",
        )
        .unwrap();
        let private_run = tempfile::tempdir().unwrap();
        fs::set_permissions(private_run.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let manifest = private_run.path().join("assignment.json");
        let runtime_uid = unsafe { libc::geteuid() };
        let consumer_uid = if runtime_uid == 1000 { 1001 } else { 1000 };
        let revision = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repo_path)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let revision = revision.trim();
        let assigned_image = format!("app@sha256:{}", "a".repeat(64));
        let assignment = serde_json::json!({
            "version": "3",
            "run_id": "run_private_compose",
            "lease_id": "assignment_private_compose",
            "outer_runtime_id": "outer_private_compose",
            "workspace": repo_path,
            "repository": {"path": repo_path, "revision": revision},
            "task_branch": "feature/coding-demo",
            "tunnel_placement": "outer",
            "published_ports": [],
            "service_images": {"app": assigned_image.clone()},
            "workspace_consumer": {"uid": consumer_uid, "gid": consumer_uid},
            "leases": [{
                "lease_id": "outer_tunnel",
                "scope": "platform-tunnel",
                "consumer": "outer-connector",
                "materializations": []
            }]
        });
        fs::write(&manifest, serde_json::to_vec(&assignment).unwrap()).unwrap();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
        let plan = runtime::load_in_guest_facade_plan(
            &manifest,
            repo_path,
            repo_path,
            &worktree_path,
            "feature/coding-demo",
            revision,
        )
        .unwrap();
        assert_eq!(
            plan.workspace_consumer(),
            Some((consumer_uid, consumer_uid))
        );

        prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan).unwrap();
        let stage = plan
            .private_compose_stage_dir(&worktree_path)
            .unwrap()
            .unwrap();
        assert!(!stage.starts_with(&worktree_path));
        assert_eq!(
            fs::metadata(&stage).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(!devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG).exists());
        assert!(!devcontainer_dir.join(SBX_COMPOSE_OVERRIDE).exists());
        let generated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(stage.join(SBX_DEVCONTAINER_CONFIG)).unwrap())
                .unwrap();
        let references = generated["dockerComposeFile"].as_array().unwrap();
        assert_eq!(references.len(), 3);
        for reference in references {
            let reference = reference.as_str().unwrap();
            assert_eq!(Path::new(reference).components().count(), 1);
            let input = stage.join(reference);
            assert!(input.is_file());
            assert_eq!(
                fs::metadata(&input).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert!(!fs::read_to_string(&input).unwrap().contains("${"));
        }
        // An existing coding process can replace repository paths, but the
        // Dev Containers CLI references only immutable runner-owned inputs.
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  app:\n    volumes: ['/etc:/host']\n",
        )
        .unwrap();
        assert!(
            !fs::read_to_string(stage.join(references[0].as_str().unwrap()))
                .unwrap()
                .contains("/etc:/host")
        );
        let held_source = worktree_path.join("held-devcontainer");
        let attacker_dir = tempfile::tempdir().unwrap();
        fs::rename(&devcontainer_dir, &held_source).unwrap();
        symlink(attacker_dir.path(), &devcontainer_dir).unwrap();
        assert!(!attacker_dir.path().join(SBX_DEVCONTAINER_CONFIG).exists());

        let verify_devcontainer = std::env::var_os("BRANCHBOX_VERIFY_DEVCONTAINER_CLI").is_some();
        let verify_compose = std::env::var_os("BRANCHBOX_VERIFY_COMPOSE_CONFIG").is_some();
        let keep_mutating = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let mutator = (verify_devcontainer || verify_compose).then(|| {
            let keep_mutating = std::sync::Arc::clone(&keep_mutating);
            let original = held_source.join("compose.yaml");
            let redirected = attacker_dir.path().join("compose.yaml");
            let (ready, started) = std::sync::mpsc::channel();
            let handle = thread::spawn(move || {
                let mut first_write = true;
                while keep_mutating.load(std::sync::atomic::Ordering::Relaxed) {
                    let hostile = "services:\n  app:\n    volumes: ['/etc:/host']\n    environment: {HOST_AUTH: '${HOST_AUTH:?unsafe}'}\n";
                    fs::write(&original, hostile).unwrap();
                    fs::write(&redirected, hostile).unwrap();
                    if first_write {
                        ready.send(()).unwrap();
                        first_write = false;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            });
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            handle
        });
        let devcontainer_output = verify_devcontainer.then(|| {
            Command::new("devcontainer")
                .args(["read-configuration", "--workspace-folder"])
                .arg(&worktree_path)
                .arg("--config")
                .arg(stage.join(SBX_DEVCONTAINER_CONFIG))
                .env_remove("REPO_IMAGE")
                .env_remove("TUNNEL_TOKEN")
                .env_remove("HOST_LABEL_FILE")
                .output()
        });
        let compose_output = verify_compose.then(|| {
            let mut command = Command::new("docker");
            command.arg("compose");
            for reference in references {
                command
                    .arg("-f")
                    .arg(stage.join(reference.as_str().unwrap()));
            }
            command
                .args(["config", "--format", "json"])
                .env_remove("REPO_IMAGE")
                .env_remove("TUNNEL_TOKEN")
                .env_remove("HOST_LABEL_FILE")
                .output()
        });
        keep_mutating.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Some(mutator) = mutator {
            mutator.join().unwrap();
        }

        if let Some(output) = devcontainer_output {
            let output = output.unwrap();
            assert!(
                output.status.success(),
                "Dev Containers rejected private inputs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let inspected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let configuration = &inspected["configuration"];
            assert_eq!(configuration["service"].as_str(), Some("app"));
            assert_eq!(
                configuration["dockerComposeFile"],
                generated["dockerComposeFile"]
            );
        }

        if let Some(output) = compose_output {
            let output = output.unwrap();
            assert!(
                output.status.success(),
                "Compose rejected private inputs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let effective: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(effective["services"]["app"].get("build").is_none());
            assert_eq!(effective["services"]["app"]["image"], assigned_image);
            assert!(effective["services"]["app"].get("ports").is_none());
            let volumes = effective["services"]["app"]["volumes"].as_array().unwrap();
            assert_eq!(volumes.len(), 2, "only signed primary binds may survive");
            assert_eq!(
                volumes[0]["source"].as_str(),
                fs::canonicalize(&worktree_path).unwrap().to_str()
            );
            assert_eq!(
                volumes[1]["source"].as_str(),
                fs::canonicalize(repo_path.join(".git")).unwrap().to_str()
            );
            assert!(effective["services"]["app"].get("secrets").is_none());
            assert!(effective["services"]["proxy"].get("command").is_none());
            let rendered = effective.to_string();
            assert!(!rendered.contains("HOST_AUTH"));
            assert!(!rendered.contains("TUNNEL_TOKEN"));
            assert!(!rendered.contains("/etc:/host"));
        }

        let no_images = InGuestFacadePlan::empty_for_tests().with_private_compose_stage_for_tests(
            private_run.path().join("assignment.json"),
            consumer_uid,
        );
        let error = no_images
            .private_compose_stage_dir(&worktree_path)
            .unwrap_err();
        assert!(error.to_string().contains("signed preloaded images"));
        let same_uid = InGuestFacadePlan::empty_for_tests()
            .with_service_images_for_tests(BTreeMap::from([(
                "app".to_string(),
                format!("app@sha256:{}", "a".repeat(64)),
            )]))
            .with_private_compose_stage_for_tests(
                private_run.path().join("assignment.json"),
                runtime_uid,
            );
        let error = same_uid
            .private_compose_stage_dir(&worktree_path)
            .unwrap_err();
        assert!(error.to_string().contains("must differ"));

        fs::remove_file(&devcontainer_dir).unwrap();
        fs::rename(&held_source, &devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":["compose.yaml"],"service":"app"}"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  app: {image: alpine:3.19}\n",
        )
        .unwrap();
        prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan).unwrap();
        assert!(!stage
            .join(format!("{SBX_COMPOSE_INPUT_PREFIX}1.yaml"))
            .exists());
        plan.remove_private_compose_stage(&worktree_path).unwrap();
        assert!(
            !stage.exists(),
            "failed starts must release private run inputs"
        );
    }

    #[test]
    fn test_in_guest_preloaded_images_strip_replaced_interpolation() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            r#"services:
  app:
    image: "${REPO_IMAGE:?signed-image-replaces-this}"
    build: {context: "${REPO_BUILD_CONTEXT:?signed-image-replaces-this}", dockerfile: "${REPO_DOCKERFILE:?signed-image-replaces-this}", args: [HOST_SECRET]}
    pull_policy: "${REPO_PULL_POLICY:?signed-image-replaces-this}"
    environment: {HOST_AUTH: "${HOST_AUTH:?signed-environment-replaces-this}"}
    depends_on: [database]
  database:
    image: "${REPO_DATABASE_IMAGE:?signed-image-replaces-this}"
    environment: {POSTGRES_USER: kept-for-dependency}
"#,
        )
        .unwrap();
        let images = BTreeMap::from([
            ("app".to_string(), "alpine:3.19".to_string()),
            ("database".to_string(), "postgres:16".to_string()),
        ]);
        let plan = InGuestFacadePlan::empty_for_tests().with_service_images_for_tests(images);
        prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan).unwrap();

        let generated: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG)).unwrap(),
        )
        .unwrap();
        let references = generated["dockerComposeFile"].as_array().unwrap();
        let sanitized =
            fs::read_to_string(devcontainer_dir.join(references[0].as_str().unwrap())).unwrap();
        assert!(!sanitized.contains("${"), "{sanitized}");
        assert!(!sanitized.contains("HOST_SECRET"), "{sanitized}");
        let document: serde_yaml::Value = serde_yaml::from_str(&sanitized).unwrap();
        for name in ["app", "database"] {
            for key in ["image", "build", "pull_policy"] {
                assert!(document["services"][name].get(key).is_none(), "{sanitized}");
            }
        }
        assert!(document["services"]["app"].get("environment").is_none());
        assert_eq!(
            document["services"]["database"]["environment"]["POSTGRES_USER"].as_str(),
            Some("kept-for-dependency")
        );

        if std::env::var_os("BRANCHBOX_VERIFY_COMPOSE_CONFIG").is_some() {
            let mut command = Command::new("docker");
            command.arg("compose");
            for reference in references {
                command
                    .arg("-f")
                    .arg(devcontainer_dir.join(reference.as_str().unwrap()));
            }
            let output = command
                .args(["config", "--format", "json"])
                .env("COMPOSE_PROJECT_NAME", "branchbox-preloaded-image-test")
                .env_remove("REPO_IMAGE")
                .env_remove("REPO_BUILD_CONTEXT")
                .env_remove("REPO_DOCKERFILE")
                .env_remove("REPO_PULL_POLICY")
                .env_remove("REPO_DATABASE_IMAGE")
                .env_remove("HOST_AUTH")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Compose rejected sanitized inputs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let effective: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(effective["services"]["app"]["image"], "alpine:3.19");
            assert_eq!(effective["services"]["database"]["image"], "postgres:16");
            assert!(effective["services"]["app"].get("build").is_none());
            assert_eq!(
                effective["services"]["database"]["environment"]["POSTGRES_USER"],
                "kept-for-dependency"
            );
        }
    }

    #[test]
    fn test_in_guest_rejects_retained_compose_interpolation_without_losing_static_dependency_env() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        let compose_path = devcontainer_dir.join("compose.yaml");
        let images = BTreeMap::from([
            ("app".to_string(), "alpine:3.19".to_string()),
            ("database".to_string(), "postgres:16".to_string()),
        ]);
        let plan = InGuestFacadePlan::empty_for_tests().with_service_images_for_tests(images);

        for unsafe_source in [
            "services:\n  app: {image: alpine:3.19, depends_on: [database]}\n  database:\n    image: postgres:16\n    environment: {EXPOSED_FROM_GUEST: '${HOST_SECRET:?ambient-secret}'}\n",
            "services:\n  app:\n    image: alpine:3.19\n    labels: {exposed.from.guest: '${HOST_SECRET:?ambient-secret}'}\n  database: {image: postgres:16}\n",
        ] {
            fs::write(&compose_path, unsafe_source).unwrap();
            let error = prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan)
                .unwrap_err();
            assert!(
                error.to_string().contains("ambient variable interpolation"),
                "{error}"
            );
            assert!(!error.to_string().contains("HOST_SECRET"));
            assert!(!devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG).exists());
        }

        fs::write(
            &compose_path,
            "services:\n  app: {image: alpine:3.19, depends_on: [database]}\n  database:\n    image: postgres:16\n    environment: {POSTGRES_USER: kept-for-dependency}\n    command: ['sh', '-c', 'echo $$POSTGRES_USER']\n",
        )
        .unwrap();
        prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan).unwrap();
        let generated: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG)).unwrap(),
        )
        .unwrap();
        let sanitized = fs::read_to_string(
            devcontainer_dir.join(generated["dockerComposeFile"][0].as_str().unwrap()),
        )
        .unwrap();
        let document: serde_yaml::Value = serde_yaml::from_str(&sanitized).unwrap();
        assert_eq!(
            document["services"]["database"]["environment"]["POSTGRES_USER"].as_str(),
            Some("kept-for-dependency")
        );
        assert!(sanitized.contains("$$POSTGRES_USER"));

        if std::env::var_os("BRANCHBOX_VERIFY_COMPOSE_CONFIG").is_some() {
            let references = generated["dockerComposeFile"].as_array().unwrap();
            let mut command = Command::new("docker");
            command.arg("compose");
            for reference in references {
                command
                    .arg("-f")
                    .arg(devcontainer_dir.join(reference.as_str().unwrap()));
            }
            let output = command
                .args(["config", "--format", "json"])
                .env(
                    "COMPOSE_PROJECT_NAME",
                    "branchbox-ambient-interpolation-test",
                )
                .env("HOST_SECRET", "synthetic-private-value")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Compose rejected sanitized inputs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let effective: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                effective["services"]["database"]["environment"]["POSTGRES_USER"],
                "kept-for-dependency"
            );
            assert!(!output
                .stdout
                .windows(23)
                .any(|part| part == b"synthetic-private-value"));
        }
    }

    #[test]
    fn test_in_guest_rejects_implicit_compose_host_environment_passthrough() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        let compose_path = devcontainer_dir.join("compose.yaml");
        let plan = InGuestFacadePlan::empty_for_tests();

        for (field, dependency) in [
            ("environment", "environment: [HOST_SECRET]"),
            ("environment", "environment: {HOST_SECRET: null}"),
            ("build.args", "build: {context: ., args: [HOST_SECRET]}"),
            (
                "build.args",
                "build: {context: ., args: {HOST_SECRET: null}}",
            ),
        ] {
            fs::write(
                &compose_path,
                format!(
                    "services:\n  app: {{image: alpine:3.19, depends_on: [database]}}\n  database:\n    image: postgres:16\n    {dependency}\n"
                ),
            )
            .unwrap();
            let error =
                prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan).unwrap_err();
            assert!(
                error.to_string().contains(&format!("valueless {field}")),
                "{error}"
            );
            assert!(!error.to_string().contains("HOST_SECRET"));
            assert!(!devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG).exists());
        }

        fs::write(
            &compose_path,
            "services:\n  app: {image: alpine:3.19, depends_on: [database]}\n  database:\n    image: postgres:16\n    environment: [HOST_SECRET=fixed, EMPTY=]\n    build: {context: ., args: [RELEASE_CHANNEL=stable, LITERAL=$$HOST_SECRET]}\n",
        )
        .unwrap();
        prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan).unwrap();
        let generated: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG)).unwrap(),
        )
        .unwrap();
        let references = generated["dockerComposeFile"].as_array().unwrap();
        let sanitized =
            fs::read_to_string(devcontainer_dir.join(references[0].as_str().unwrap())).unwrap();
        assert!(sanitized.contains("HOST_SECRET=fixed"));
        assert!(sanitized.contains("LITERAL=$$HOST_SECRET"));

        if std::env::var_os("BRANCHBOX_VERIFY_COMPOSE_CONFIG").is_some() {
            let mut command = Command::new("docker");
            command.arg("compose");
            for reference in references {
                command
                    .arg("-f")
                    .arg(devcontainer_dir.join(reference.as_str().unwrap()));
            }
            let output = command
                .args(["config", "--format", "json"])
                .env("COMPOSE_PROJECT_NAME", "branchbox-implicit-env-test")
                .env("HOST_SECRET", "synthetic-private-value")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Compose rejected sanitized inputs: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let effective: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let database = &effective["services"]["database"];
            assert_eq!(database["environment"]["HOST_SECRET"], "fixed");
            assert_eq!(database["environment"]["EMPTY"], "");
            assert_eq!(database["build"]["args"]["RELEASE_CHANNEL"], "stable");
            assert_eq!(database["build"]["args"]["LITERAL"], "$$HOST_SECRET");
            assert!(!output
                .stdout
                .windows(23)
                .any(|part| part == b"synthetic-private-value"));
        }
    }

    #[test]
    fn test_in_guest_rejects_empty_compose_list_before_cli_dotenv_fallback() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":[],"service":"app"}"#,
        )
        .unwrap();
        let outside = repo_path.join("outside.yaml");
        fs::write(
            &outside,
            "services:\n  app:\n    provider: {type: unsafe}\n",
        )
        .unwrap();
        fs::write(
            worktree_path.join(".env"),
            format!("COMPOSE_FILE={}\n", outside.display()),
        )
        .unwrap();

        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("empty list lets the Dev Containers CLI load COMPOSE_FILE"));
        assert!(!devcontainer_dir.join(SBX_DEVCONTAINER_CONFIG).exists());
        assert!(!devcontainer_dir.join(SBX_COMPOSE_OVERRIDE).exists());
    }

    #[test]
    fn test_in_guest_generated_compose_facade_rejects_path_interpolation() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("${HOST_SECRET}");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        let source: serde_yaml::Value =
            serde_yaml::from_str("services:\n  app: {image: alpine:3.19}\n").unwrap();
        let error = prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("app"),
            "/workspaces/coding-demo",
            &[source],
            &InGuestComposeAssignment {
                project_environment: None,
                service_images: &BTreeMap::new(),
                workspace_consumer: false,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &[],
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("ambient variable interpolation"));
        assert!(!error.to_string().contains("HOST_SECRET"));
        assert!(!devcontainer_dir.join(SBX_COMPOSE_OVERRIDE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_in_guest_rejects_unsafe_input_paths_before_read_or_write() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;

        fn make_fifo(path: &Path) {
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        }

        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        let config_path = devcontainer_dir.join("devcontainer.json");
        fs::remove_file(&config_path).unwrap();
        symlink("/dev/zero", &config_path).unwrap();
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("inside the task worktree"));
        fs::remove_file(&config_path).unwrap();
        make_fifo(&config_path);
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("regular file"));
        fs::remove_file(&config_path).unwrap();
        fs::write(
            &config_path,
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        let compose = devcontainer_dir.join("compose.yaml");
        let outside = repo_path.join("outside.yaml");
        fs::write(&outside, "services:\n  app: {image: alpine}\n").unwrap();
        symlink(&outside, &compose).unwrap();
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("inside the task worktree"));
        assert!(!devcontainer_dir.join(SBX_COMPOSE_OVERRIDE).exists());

        fs::remove_file(&compose).unwrap();
        symlink("/dev/zero", &compose).unwrap();
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("inside the task worktree"));

        fs::remove_file(&compose).unwrap();
        make_fifo(&compose);
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("regular file"));

        fs::remove_file(&compose).unwrap();
        fs::write(&compose, vec![b'x'; MAX_IN_GUEST_INPUT_BYTES as usize + 1]).unwrap();
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("no larger than"));
        fs::remove_file(&compose).unwrap();
        fs::write(&compose, "services:\n  app: {image: alpine}\n").unwrap();
        let override_path = devcontainer_dir.join(SBX_COMPOSE_OVERRIDE);
        make_fifo(&override_path);
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn test_in_guest_accepts_in_worktree_compose_symlink_but_rejects_symlinked_config_dir() {
        use std::os::unix::fs::symlink;

        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        let source = worktree_path.join("compose.yaml");
        let original = "services:\n  app: {image: alpine}\n";
        fs::write(&source, original).unwrap();
        symlink("../compose.yaml", devcontainer_dir.join("compose.yaml")).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&source).unwrap(), original);
        assert!(devcontainer_dir
            .join(format!("{SBX_COMPOSE_INPUT_PREFIX}0.yaml"))
            .exists());

        let nested = worktree_path.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("compose.yaml"), original).unwrap();
        symlink("../nested", devcontainer_dir.join("linked-parent")).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"linked-parent/compose.yaml","service":"app"}"#,
        )
        .unwrap();
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("directory symlink"));
        assert!(!nested
            .join(format!("{SBX_COMPOSE_INPUT_PREFIX}0.yaml"))
            .exists());

        fs::remove_dir_all(&devcontainer_dir).unwrap();
        let outside_dir = repo_path.join("outside-devcontainer");
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(outside_dir.join("devcontainer.json"), "{}").unwrap();
        symlink(&outside_dir, &devcontainer_dir).unwrap();
        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("real directory"));
        assert!(!outside_dir.join(SBX_COMPOSE_OVERRIDE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_in_guest_generated_write_stays_in_pinned_directory_after_path_swap() {
        use std::os::unix::fs::symlink;

        let temp_dir = setup_test_repo();
        let worktree_path = temp_dir.path().join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        let held_dir = worktree_path.join("held-devcontainer");
        let outside_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(&devcontainer_dir).unwrap();

        // The workspace consumer can rename a writable directory after the
        // canonical-path check. Pin its inode before simulating that swap.
        let pinned = pin_in_guest_compose_parent(&worktree_path, &devcontainer_dir).unwrap();
        fs::rename(&devcontainer_dir, &held_dir).unwrap();
        symlink(outside_dir.path(), &devcontainer_dir).unwrap();
        write_in_guest_generated_text_file_at(
            &pinned,
            std::ffi::OsStr::new(SBX_COMPOSE_OVERRIDE),
            "safe generated contents",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(held_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap(),
            "safe generated contents"
        );
        assert!(!outside_dir.path().join(SBX_COMPOSE_OVERRIDE).exists());
        assert!(pin_in_guest_compose_parent(&worktree_path, &devcontainer_dir).is_err());
    }

    #[test]
    fn test_in_guest_does_not_replace_tracked_generated_facade() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  app: {image: alpine}\n",
        )
        .unwrap();
        let facade = devcontainer_dir.join(SBX_COMPOSE_OVERRIDE);
        fs::write(&facade, "tracked repository content\n").unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["add", "-f", "--"])
            .arg(&facade)
            .status()
            .unwrap()
            .success());

        let error = prepare_in_guest_devcontainer_config(
            repo_path,
            &worktree_path,
            &InGuestFacadePlan::empty_for_tests(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("tracked by Git"));
        assert_eq!(
            fs::read_to_string(facade).unwrap(),
            "tracked repository content\n"
        );
    }

    #[test]
    fn test_in_guest_compose_rejects_indirect_unsanitized_sources() {
        for source in [
            "include: [sidecar.yaml]\nservices:\n  app: {image: alpine}\n",
            "services:\n  app: {image: alpine, extends: {file: sidecar.yaml, service: app}}\n",
            "services:\n  app: {image: alpine, volumes_from: [host-service]}\n",
            "services:\n  app: {image: alpine, provider: {type: /bin/sh}}\n",
        ] {
            let document: serde_yaml::Value = serde_yaml::from_str(source).unwrap();
            let temp = tempfile::tempdir().unwrap();
            let devcontainer = temp.path().join("repo/.devcontainer");
            fs::create_dir_all(&devcontainer).unwrap();
            assert!(
                validate_in_guest_compose_security(
                    &document,
                    Some("app"),
                    &devcontainer,
                    &temp.path().join("repo"),
                    &BTreeMap::new()
                )
                .is_err(),
                "unexpectedly accepted: {source}"
            );
        }
        let temp = tempfile::tempdir().unwrap();
        let tagged = temp.path().join("compose.yaml");
        fs::write(&tagged, "services: !include sidecar.yaml\n").unwrap();
        assert!(read_in_guest_compose_document(temp.path(), &tagged)
            .unwrap_err()
            .to_string()
            .contains("unsupported YAML tag"));
    }

    #[cfg(unix)]
    #[test]
    fn test_in_guest_rejects_compose_host_provider_before_private_staging() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        fs::write(
            devcontainer_dir.join("devcontainer.json"),
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#,
        )
        .unwrap();
        fs::write(
            devcontainer_dir.join("compose.yaml"),
            "services:\n  app:\n    image: alpine\n    provider: {type: /bin/sh}\n",
        )
        .unwrap();
        let run = tempfile::tempdir().unwrap();
        fs::set_permissions(run.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let runtime_uid = unsafe { libc::geteuid() };
        let consumer_uid = if runtime_uid == 1000 { 1001 } else { 1000 };
        let plan = InGuestFacadePlan::empty_for_tests()
            .with_service_images_for_tests(BTreeMap::from([(
                "app".to_string(),
                format!("app@sha256:{}", "a".repeat(64)),
            )]))
            .with_private_compose_stage_for_tests(run.path().join("assignment.json"), consumer_uid);

        let error =
            prepare_in_guest_devcontainer_config(repo_path, &worktree_path, &plan).unwrap_err();
        assert!(error.to_string().contains("provider"));
        let stage = plan
            .private_compose_stage_dir(&worktree_path)
            .unwrap()
            .unwrap();
        assert!(!stage.join(SBX_DEVCONTAINER_CONFIG).exists());
        assert!(!stage.join(SBX_COMPOSE_OVERRIDE).exists());
    }

    #[test]
    fn in_guest_workspace_consumer_receives_exact_git_ownership_exceptions() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("coding-demo");
        let devcontainer_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir).unwrap();
        let compose = devcontainer_dir.join("compose.yaml");
        fs::write(
            &compose,
            r#"services:
  rails-app:
    image: registry.example/team/app:mutable
    environment:
      GIT_CONFIG_COUNT: "9"
      GIT_CONFIG_KEY_0: core.hooksPath
      GIT_CONFIG_VALUE_0: /tmp/repository-hooks
"#,
        )
        .unwrap();
        prepare_outer_tunnel_compose_override(
            repo_path,
            &worktree_path,
            &devcontainer_dir,
            Some("rails-app"),
            "/workspaces/coding-demo",
            &[read_in_guest_compose_document(&worktree_path, &compose).unwrap()],
            &InGuestComposeAssignment {
                project_environment: None,
                service_images: &BTreeMap::new(),
                workspace_consumer: true,
                private_stage: None,
                lease_mounts: &[],
                spool_volumes: &[],
            },
        )
        .unwrap();

        let rendered = fs::read_to_string(devcontainer_dir.join(SBX_COMPOSE_OVERRIDE)).unwrap();
        // The repository's own Git configuration is replaced, never merged.
        assert!(!rendered.contains("core.hooksPath"));
        assert!(!rendered.contains("/tmp/repository-hooks"));
        let facade: serde_yaml::Value = serde_yaml::from_str(&rendered).unwrap();
        let environment = match &facade["services"]["rails-app"]["environment"] {
            serde_yaml::Value::Tagged(tagged) => tagged.value.as_mapping().unwrap().clone(),
            other => panic!("expected overridden primary environment, got {other:?}"),
        };
        let value = |name: &str| {
            environment
                .get(serde_yaml::Value::String(name.to_string()))
                .and_then(serde_yaml::Value::as_str)
                .map(str::to_string)
        };
        assert_eq!(value("GIT_CONFIG_COUNT"), Some("2".to_string()));
        assert_eq!(
            value("GIT_CONFIG_KEY_0"),
            Some("safe.directory".to_string())
        );
        assert_eq!(
            value("GIT_CONFIG_VALUE_0"),
            Some("/workspaces/coding-demo".to_string())
        );
        assert_eq!(
            value("GIT_CONFIG_KEY_1"),
            Some("safe.directory".to_string())
        );
        assert_eq!(
            value("GIT_CONFIG_VALUE_1"),
            Some(CONTAINER_MAIN_GIT_TARGET.to_string())
        );
        // Only the exact platform-owned task paths are excepted; never a wildcard, and never a
        // second Git setting that could redirect execution.
        assert_eq!(environment.len(), 5);
        assert!(!rendered.contains('*'));
    }

    #[test]
    fn test_in_guest_workspace_folder_is_expanded_and_normalized() {
        let config = DevcontainerConfig {
            workspace_folder: Some("/workspaces/${localWorkspaceFolderBasename}".to_string()),
            ..DevcontainerConfig::default()
        };
        assert_eq!(
            effective_in_guest_workspace_folder(&config, Path::new("/workspace/coding-demo"))
                .unwrap(),
            "/workspaces/coding-demo"
        );

        for folder in [
            "relative",
            "/",
            "/workspaces",
            "/home/vscode/.ssh",
            "/etc/task",
            "/run/agentify-runtime/task",
            "/run/branchbox/leases/code",
            "/workspaces/${HOST_SECRET}",
            "/workspaces/$HOST_SECRET",
        ] {
            let config = DevcontainerConfig {
                workspace_folder: Some(folder.to_string()),
                ..DevcontainerConfig::default()
            };
            assert!(
                effective_in_guest_workspace_folder(&config, Path::new("/workspace/coding-demo"))
                    .is_err(),
                "unexpectedly accepted {folder}"
            );
        }
    }

    #[test]
    fn test_in_guest_compose_rejects_extends_privilege_build_authority_and_secondary_host_paths() {
        for source in [
            "services:\n  app:\n    extends: {file: base.yaml, service: app}\n",
            "services:\n  app:\n    privileged: true\n",
            "services:\n  app:\n    build: {context: ., ssh: default}\n",
            "services:\n  app:\n    build: {context: /run/agentify-assignment}\n",
            "services:\n  app:\n    secrets: [project-token]\n",
            "services:\n  app:\n    image: alpine\n  worker:\n    image: alpine\n    volumes: [/etc:/host-etc:ro]\n",
        ] {
            let document: serde_yaml::Value = serde_yaml::from_str(source).unwrap();
            let temp = tempfile::tempdir().unwrap();
            let devcontainer = temp.path().join("repo/.devcontainer");
            fs::create_dir_all(&devcontainer).unwrap();
            assert!(
                validate_in_guest_compose_security(
                    &document,
                    Some("app"),
                    &devcontainer,
                    &temp.path().join("repo"),
                    &BTreeMap::new()
                )
                .is_err(),
                "unexpectedly accepted: {source}"
            );
        }
    }

    #[test]
    fn test_in_guest_compose_rejects_alternate_supervisor_endpoints() {
        for source in [
            "services:\n  app:\n    environment: {DOCKER_HOST: tcp://supervisor:2375}\n",
            "services:\n  app:\n    volumes: [/run/podman/podman.sock:/run/podman/podman.sock]\n",
            "services:\n  app:\n    volumes: [/run/buildkit:/run/buildkit]\n",
        ] {
            let document: serde_yaml::Value = serde_yaml::from_str(source).unwrap();
            assert!(
                reject_supervisor_socket_references_yaml(&document).is_err(),
                "unexpectedly accepted: {source}"
            );
        }
    }

    #[test]
    fn test_generate_feature_color_deterministic() {
        // Same feature name should always generate same color
        let color1 = generate_feature_color("oauth-integration");
        let color2 = generate_feature_color("oauth-integration");
        assert_eq!(color1, color2);
    }

    #[test]
    fn test_generate_feature_color_different_names() {
        // Different names should (likely) generate different colors
        let color1 = generate_feature_color("oauth-integration");
        let color2 = generate_feature_color("payment-system");
        // Not guaranteed to be different with only 12 colors, but likely
        // This test mainly ensures the function works
        assert!(color1.starts_with('#'));
        assert!(color2.starts_with('#'));
        assert_eq!(color1.len(), 7); // #RRGGBB format
        assert_eq!(color2.len(), 7);
    }

    #[test]
    fn test_setup_vscode_workspace_creates_settings() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let color = Some("#3498db".to_string());

        workflow
            .setup_vscode_workspace(&worktree_path, "test-feature", &color, None)
            .unwrap();

        // Check that .vscode/settings.json was created
        let settings_path = worktree_path.join(".vscode/settings.json");
        assert!(settings_path.exists());

        // Verify content
        let content = fs::read_to_string(&settings_path).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            settings.get("peacock.color").and_then(|v| v.as_str()),
            Some("#3498db")
        );
        assert_eq!(
            settings.get("peacock.remoteColor").and_then(|v| v.as_str()),
            Some("#3498db")
        );
        assert_eq!(
            settings.get("window.title").and_then(|v| v.as_str()),
            Some("${rootName} [test-feature] - ${activeEditorShort}")
        );

        let customizations = settings
            .get("workbench.colorCustomizations")
            .and_then(|v| v.as_object())
            .expect("color customizations present");
        assert_eq!(
            customizations
                .get("statusBar.background")
                .and_then(|v| v.as_str()),
            Some("#3498db")
        );
        assert_eq!(
            customizations
                .get("activityBar.background")
                .and_then(|v| v.as_str()),
            Some("#5dade2")
        );
        assert_eq!(
            customizations
                .get("activityBar.activeBackground")
                .and_then(|v| v.as_str()),
            Some("#7bbce8")
        );
        assert_eq!(
            customizations
                .get("statusBarItem.hoverBackground")
                .and_then(|v| v.as_str()),
            Some("#2772a4")
        );
        assert_eq!(
            customizations
                .get("titleBar.inactiveBackground")
                .and_then(|v| v.as_str()),
            Some("#3498db99")
        );
    }

    #[test]
    fn test_setup_vscode_workspace_creates_tasks() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow
            .setup_vscode_workspace(
                &worktree_path,
                "test-feature",
                &None,
                Some("https://test-feature.example.com"),
            )
            .unwrap();

        // Check that .vscode/tasks.json was created
        let tasks_path = worktree_path.join(".vscode/tasks.json");
        assert!(tasks_path.exists());

        // Verify content
        let content = fs::read_to_string(&tasks_path).unwrap();
        assert!(content.contains("Open Feature URL"));
        assert!(content.contains("https://test-feature.example.com"));
        assert!(!content.contains("https://https://test-feature.example.com"));

        let tasks: serde_json::Value = serde_json::from_str(&content).unwrap();
        let first_task = tasks
            .get("tasks")
            .and_then(|value| value.as_array())
            .and_then(|value| value.first())
            .expect("first task present");
        assert_eq!(
            first_task.get("type").and_then(|value| value.as_str()),
            Some("process")
        );
        assert_eq!(
            first_task.get("command").and_then(|value| value.as_str()),
            Some("xdg-open")
        );
        let windows_task = first_task.get("windows").expect("windows override present");
        assert_eq!(
            windows_task.get("command").and_then(|value| value.as_str()),
            Some("explorer")
        );
        let windows_url_arg = windows_task
            .get("args")
            .and_then(|value| value.as_array())
            .and_then(|value| value.first())
            .and_then(|value| value.as_str())
            .expect("windows url arg present");
        assert_eq!(windows_url_arg, "https://test-feature.example.com");
    }

    #[test]
    fn test_setup_vscode_workspace_strips_crlf_from_feature_url() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow
            .setup_vscode_workspace(
                &worktree_path,
                "test-feature",
                &None,
                Some("test-feature.example.com\nINJECTED=value\r"),
            )
            .unwrap();

        let tasks_path = worktree_path.join(".vscode/tasks.json");
        assert!(tasks_path.exists());
        let content = fs::read_to_string(tasks_path).unwrap();
        let tasks: serde_json::Value = serde_json::from_str(&content).unwrap();
        let first_task = tasks
            .get("tasks")
            .and_then(|value| value.as_array())
            .and_then(|value| value.first())
            .expect("first task present");
        let url_arg = first_task
            .get("args")
            .and_then(|value| value.as_array())
            .and_then(|value| value.first())
            .and_then(|value| value.as_str())
            .expect("url arg present");
        assert_eq!(url_arg, "https://test-feature.example.comINJECTED=value");
        assert!(!url_arg.contains('\n'));
        assert!(!url_arg.contains('\r'));
    }

    #[test]
    fn test_setup_vscode_workspace_no_url() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow
            .setup_vscode_workspace(&worktree_path, "test-feature", &None, None)
            .unwrap();

        // Should still create settings
        let settings_path = worktree_path.join(".vscode/settings.json");
        assert!(settings_path.exists());

        // But no tasks.json without URL
        let tasks_path = worktree_path.join(".vscode/tasks.json");
        assert!(!tasks_path.exists());

        let content = fs::read_to_string(settings_path).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(settings.get("workbench.colorCustomizations").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_setup_vscode_workspace_rejects_symlink_tasks_file() {
        use std::os::unix::fs::symlink;

        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        let vscode_dir = worktree_path.join(".vscode");
        fs::create_dir_all(&vscode_dir).unwrap();

        let protected_file = repo_path.join("protected.txt");
        fs::write(&protected_file, "original").unwrap();
        symlink(&protected_file, vscode_dir.join("tasks.json")).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let result = workflow.setup_vscode_workspace(
            &worktree_path,
            "test-feature",
            &None,
            Some("test-feature.example.com"),
        );

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&protected_file).unwrap(), "original");
    }

    #[test]
    fn test_write_branchbox_env_sanitizes_values() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(worktree_path.join(".devcontainer")).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        workflow
            .write_branchbox_env(
                &worktree_path,
                "test-feature",
                "feature/test\nINJECTED_BRANCH=1",
                Some("dev-test-feature.example.com\r\nINJECTED_URL=1"),
                Some("compose\nname"),
                Some("devcontainer\rname"),
                "main\r\nINJECTED_MAIN=1",
            )
            .unwrap();

        let managed_env = fs::read_to_string(worktree_path.join(".devcontainer/.branchbox.env"))
            .expect("managed env should exist");
        assert!(managed_env.contains("GIT_BRANCH=feature/testINJECTED_BRANCH1"));
        assert!(managed_env.contains("APP_URL='dev-test-feature.example.comINJECTED_URL=1'"));
        assert!(managed_env.contains("COMPOSE_PROJECT_NAME=composename"));
        assert!(managed_env.contains("DEVCONTAINER_NAME=devcontainername"));
        assert!(managed_env.contains("BRANCHBOX_MAIN_NAME=mainINJECTED_MAIN=1"));
        assert!(!managed_env.lines().any(|line| line == "INJECTED_BRANCH=1"));
        assert!(!managed_env.lines().any(|line| line == "INJECTED_URL=1"));
        assert!(!managed_env.lines().any(|line| line == "INJECTED_MAIN=1"));
    }

    #[test]
    fn managed_env_rewrites_preserve_only_same_workspace_cleanup_identity() {
        let temp_dir = setup_test_repo();
        let workspace = temp_dir.path().join("test-feature");
        fs::create_dir_all(workspace.join(".devcontainer")).unwrap();
        let projects = BTreeSet::from(["vsc-actual-feature".to_string()]);
        modules::compose::persist_teardown_projects(&workspace, &projects).unwrap();
        let workflow = FeatureWorkflow::new(temp_dir.path()).unwrap();
        workflow
            .write_branchbox_env(
                &workspace,
                "test-feature",
                "feature/test-feature",
                None,
                Some("generated-feature"),
                None,
                "main",
            )
            .unwrap();
        assert_eq!(
            modules::compose::retained_teardown_projects(&workspace).unwrap(),
            projects
        );

        let other = temp_dir.path().join("other-feature");
        fs::create_dir_all(other.join(".devcontainer")).unwrap();
        let copied = fs::read(workspace.join(".devcontainer/.branchbox.env")).unwrap();
        let other_env = other.join(".devcontainer/.branchbox.env");
        fs::write(&other_env, &copied).unwrap();
        assert!(workflow
            .write_branchbox_env(
                &other,
                "other-feature",
                "feature/other-feature",
                None,
                Some("generated-other"),
                None,
                "main"
            )
            .is_err());
        assert_eq!(fs::read(other_env).unwrap(), copied);
    }

    #[test]
    fn setup_vscode_workspace_writes_only_managed_settings_over_jsonc() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("jsonc-feature");
        let vscode_dir = worktree_path.join(".vscode");
        fs::create_dir_all(&vscode_dir).unwrap();
        fs::write(
            vscode_dir.join("settings.json"),
            "{\n  // the team's settings\n  \"editor.tabSize\": 2,\n}\n",
        )
        .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        workflow
            .setup_vscode_workspace(
                &worktree_path,
                "jsonc-feature",
                &Some("#3498db".to_string()),
                Some("app.example.com"),
            )
            .unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(vscode_dir.join("settings.json")).unwrap())
                .unwrap();
        let settings = settings.as_object().unwrap();
        assert_eq!(
            settings["editor.tabSize"], 2,
            "settings with comments are kept"
        );
        for key in settings.keys().filter(|key| *key != "editor.tabSize") {
            assert!(
                crate::workflows::teardown_plan::VSCODE_MANAGED_SETTINGS.contains(&key.as_str()),
                "{key} is written by start but not recognized by teardown"
            );
        }
        assert_eq!(
            settings.len(),
            1 + crate::workflows::teardown_plan::VSCODE_MANAGED_SETTINGS.len()
        );
    }

    #[test]
    fn test_setup_vscode_workspace_preserves_existing_settings() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        let vscode_dir = worktree_path.join(".vscode");
        fs::create_dir_all(&vscode_dir).unwrap();

        // Create existing settings with custom value
        let existing_settings = serde_json::json!({
            "custom.setting": "value",
            "peacock.color": "#old-color"
        });
        fs::write(
            vscode_dir.join("settings.json"),
            serde_json::to_string_pretty(&existing_settings).unwrap(),
        )
        .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let color = Some("#3498db".to_string());

        workflow
            .setup_vscode_workspace(&worktree_path, "test-feature", &color, None)
            .unwrap();

        // Check that custom setting is preserved
        let content = fs::read_to_string(vscode_dir.join("settings.json")).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            settings.get("custom.setting").and_then(|v| v.as_str()),
            Some("value")
        );
        assert_eq!(
            settings.get("peacock.color").and_then(|v| v.as_str()),
            Some("#3498db")
        );
        assert_ne!(
            settings.get("peacock.color").and_then(|v| v.as_str()),
            Some("#old-color")
        );
        assert!(settings.get("workbench.colorCustomizations").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_secure_file_rejects_symlink_target() {
        use std::os::unix::fs::symlink;

        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let protected_file = repo_path.join("protected.txt");
        fs::write(&protected_file, "original").unwrap();

        let symlink_path = repo_path.join("linked.txt");
        symlink(&protected_file, &symlink_path).unwrap();

        let err = write_secure_file(&symlink_path, "mutated").unwrap_err();
        assert!(err.to_string().contains("symlink"));
        assert_eq!(fs::read_to_string(&protected_file).unwrap(), "original");
    }

    #[test]
    fn test_resolve_work_feature_auto_generates_from_title_like_input() {
        let temp = setup_test_repo();
        let repo_path = temp.path();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        // Table-driven test cases: (input, expected_output)
        let test_cases = vec![
            // Valid name should be used directly
            ("oauth-integration", "oauth-integration"),
            // Title-like input (uppercase, spaces, dots) should be auto-generated
            ("Rails 8.1.2", "rails-812"),
            // Another title-like input with uppercase
            ("OAuth Integration", "oauth"),
            // Input with spaces
            ("fix bug 123", "fix-bug-123"),
        ];

        for (input, expected) in test_cases {
            let request = StartRequest {
                name: Some(input.to_string()),
                ..Default::default()
            };
            let result = workflow.resolve_work_feature(&request).unwrap();
            assert_eq!(result, expected, "Failed on input: {}", input);
        }
    }

    /// Registry integrity (DESIGN §10.1, S2), the write-ahead start record (D-14) and list
    /// reconciliation (C10d).
    mod registry_integrity {
        use super::*;
        use std::sync::{Arc, Barrier};
        use std::time::Instant;

        fn sample_metadata(repo: &Path, name: &str) -> FeatureMetadata {
            let now = Utc::now();
            FeatureMetadata {
                work_feature: name.to_string(),
                branch_name: format!("feature/{name}"),
                worktree_path: repo.join(name),
                base_branch: None,
                feature_url: None,
                compose_project_name: None,
                env_path: None,
                status: FeatureStatus::Active,
                created_at: now,
                updated_at: now,
                removed_at: None,
                tunnel: None,
                color: None,
                pr_number: None,
                last_commit: None,
                devcontainer_outdated: false,
                last_sync_at: None,
                sync_strategy: None,
                start_mode: StartMode::Minimal,
                prompt_seed: None,
                module_outcomes: Vec::new(),
                last_summary_rendered_at: None,
                adapter: None,
                runtime: RuntimeMetadata::default(),
                setup: None,
            }
        }

        fn setup_by(pid: u32) -> SetupRecord {
            SetupRecord {
                state: SetupState::InProgress,
                pid,
                started_at: Utc::now(),
            }
        }

        /// The pid of a process that has exited and been reaped.
        #[cfg(unix)]
        fn dead_pid() -> u32 {
            let mut child = Command::new("true").spawn().unwrap();
            let pid = child.id();
            child.wait().unwrap();
            pid
        }

        fn names(store: &FeatureStateStore) -> Vec<String> {
            let mut names: Vec<String> = store
                .list_features()
                .unwrap()
                .into_iter()
                .map(|feature| feature.work_feature)
                .collect();
            names.sort();
            names
        }

        /// A repository at `<temp>/main`, so feature worktrees land inside the temp dir.
        fn nested_test_repo(temp: &TempDir) -> PathBuf {
            let repo = temp.path().join("main");
            fs::create_dir(&repo).unwrap();
            for args in [
                vec!["init", "-b", "main"],
                vec!["config", "user.email", "test@example.com"],
                vec!["config", "user.name", "Test User"],
            ] {
                assert!(Command::new("git")
                    .args(&args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success());
            }
            fs::write(repo.join("README.md"), "# Test Repo\n").unwrap();
            fs::write(
                repo.join(".gitignore"),
                ".env\n.devcontainer/.branchbox.env\n.branchbox/\n",
            )
            .unwrap();
            for args in [
                vec!["add", "README.md", ".gitignore"],
                vec!["commit", "-q", "-m", "Initial commit"],
            ] {
                assert!(Command::new("git")
                    .args(&args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success());
            }
            repo
        }

        #[test]
        fn two_stores_holding_the_lock_both_persist_their_entries() {
            let temp = TempDir::new().unwrap();
            let repo = temp.path().to_path_buf();
            let barrier = Arc::new(Barrier::new(2));
            let started = Instant::now();
            let handles: Vec<_> = ["alpha", "beta"]
                .into_iter()
                .map(|name| {
                    let repo = repo.clone();
                    let barrier = Arc::clone(&barrier);
                    thread::spawn(move || {
                        let mut store = FeatureStateStore::new(&repo);
                        // Both read the registry, then sleep before writing: without the lock
                        // each would write back a registry missing the other's entry.
                        store.hold_while_locked = Some(Duration::from_millis(200));
                        barrier.wait();
                        store.record_start(sample_metadata(&repo, name)).unwrap();
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }

            assert!(
                started.elapsed() >= Duration::from_millis(400),
                "the two read-modify-write cycles must not overlap"
            );
            assert_eq!(names(&FeatureStateStore::new(&repo)), ["alpha", "beta"]);
        }

        #[test]
        fn a_held_lock_times_out_with_registry_locked_naming_the_path() {
            let temp = TempDir::new().unwrap();
            let repo = temp.path().to_path_buf();
            let held = atomic_fs::lock_state_dir(&repo.join(".branchbox"), atomic_fs::LOCK_TIMEOUT)
                .unwrap();

            let contender = repo.clone();
            let err = thread::spawn(move || {
                let mut store = FeatureStateStore::new(&contender);
                store.lock_timeout = Duration::from_millis(100);
                store
                    .record_start(sample_metadata(&contender, "alpha"))
                    .unwrap_err()
            })
            .join()
            .unwrap();

            assert!(matches!(err, Error::RegistryLocked { .. }), "{err:?}");
            assert_eq!(err.code(), "registry_locked");
            assert!(err.to_string().contains(".branchbox"), "{err}");
            assert!(!repo.join(".branchbox/registry.json").exists());
            drop(held);

            FeatureStateStore::new(&repo)
                .record_start(sample_metadata(&repo, "alpha"))
                .unwrap();
        }

        #[cfg(unix)]
        #[test]
        fn registry_writes_are_atomic_and_keep_the_file_mode() {
            use std::os::unix::fs::PermissionsExt;

            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            let mode = || fs::metadata(&store.path).unwrap().permissions().mode() & 0o777;

            store
                .record_start(sample_metadata(temp.path(), "alpha"))
                .unwrap();
            assert_eq!(mode(), 0o644);

            fs::set_permissions(&store.path, fs::Permissions::from_mode(0o600)).unwrap();
            store.record_teardown("alpha").unwrap();
            assert_eq!(mode(), 0o600);

            let leftovers: Vec<_> = fs::read_dir(&store.state_dir)
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(leftovers, ["registry.json"]);
        }

        #[cfg(unix)]
        #[test]
        fn registry_refuses_to_write_through_a_symlink() {
            let temp = TempDir::new().unwrap();
            let outside = temp.path().join("outside.json");
            let original = "{\"version\":\"1\",\"features\":[]}";
            fs::write(&outside, original).unwrap();
            fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
            std::os::unix::fs::symlink(&outside, temp.path().join(".branchbox/registry.json"))
                .unwrap();

            let store = FeatureStateStore::new(temp.path());
            let err = store
                .record_start(sample_metadata(temp.path(), "alpha"))
                .unwrap_err();
            assert!(
                err.to_string()
                    .contains("Refusing to write through symlink"),
                "{err}"
            );
            assert_eq!(fs::read_to_string(&outside).unwrap(), original);
        }

        #[test]
        fn a_failed_change_writes_nothing_and_names_the_registry() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());

            let err = store.update_feature("missing", |_| {}).unwrap_err();
            match &err {
                Error::FeatureNotFound { name, registry } => {
                    assert_eq!(name, "missing");
                    assert_eq!(registry, &temp.path().join(".branchbox/registry.json"));
                }
                other => panic!("expected FeatureNotFound, got {other:?}"),
            }
            assert_eq!(err.code(), "feature_not_found");
            assert!(!store.path.exists());
        }

        #[test]
        fn write_ahead_entry_for_a_new_feature_is_discarded_with_its_worktree() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            store
                .record_start(sample_metadata(temp.path(), "other"))
                .unwrap();

            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.setup = Some(setup_by(4242));
            assert!(store.record_setup_started(provisional).unwrap().is_none());
            let recorded = store.get_feature("alpha").unwrap().unwrap();
            assert_eq!(recorded.status, FeatureStatus::Active);
            assert_eq!(recorded.setup.as_ref().map(|setup| setup.pid), Some(4242));

            // Someone else's marker is left alone.
            store.discard_setup("alpha", 1, None).unwrap();
            assert!(store.get_feature("alpha").unwrap().is_some());

            store.discard_setup("alpha", 4242, None).unwrap();
            assert_eq!(names(&store), ["other"]);
        }

        #[test]
        fn write_ahead_on_a_live_entry_keeps_its_runtime_identity() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            let mut live = sample_metadata(temp.path(), "alpha");
            live.status = FeatureStatus::FailedRetained;
            live.feature_url = Some("alpha.example.com".to_string());
            live.runtime.provider = RuntimeProviderKind::Sbx;
            live.runtime.runtime_id = Some("sbx-alpha".to_string());
            store.record_start(live).unwrap();
            let before = store.get_feature("alpha").unwrap().unwrap();

            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.runtime.provider = RuntimeProviderKind::Sbx;
            provisional.setup = Some(setup_by(4242));
            let previous = store.record_setup_started(provisional).unwrap();

            let during = store.get_feature("alpha").unwrap().unwrap();
            assert_eq!(during.status, FeatureStatus::FailedRetained);
            assert_eq!(during.runtime.runtime_id.as_deref(), Some("sbx-alpha"));
            assert_eq!(during.feature_url.as_deref(), Some("alpha.example.com"));
            assert_eq!(during.setup.as_ref().map(|setup| setup.pid), Some(4242));

            store.discard_setup("alpha", 4242, previous).unwrap();
            let after = store.get_feature("alpha").unwrap().unwrap();
            assert!(after.setup.is_none());
            assert_eq!(after.updated_at, before.updated_at);
            assert_eq!(after.runtime.runtime_id.as_deref(), Some("sbx-alpha"));
        }

        #[test]
        fn write_ahead_records_the_prepared_runtime_of_its_own_start_only() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.runtime.provider = RuntimeProviderKind::Sbx;
            provisional.setup = Some(setup_by(4242));
            store.record_setup_started(provisional).unwrap();

            let prepared = RuntimeMetadata {
                provider: RuntimeProviderKind::Sbx,
                runtime_id: Some("branchbox-alpha".to_string()),
                ..RuntimeMetadata::default()
            };
            // Another start's marker: nothing changes.
            store.record_setup_runtime("alpha", 1, &prepared).unwrap();
            assert!(store
                .get_feature("alpha")
                .unwrap()
                .unwrap()
                .runtime
                .runtime_id
                .is_none());

            store
                .record_setup_runtime("alpha", 4242, &prepared)
                .unwrap();
            let recorded = store.get_feature("alpha").unwrap().unwrap();
            assert_eq!(
                recorded.runtime.runtime_id.as_deref(),
                Some("branchbox-alpha")
            );
            assert_eq!(recorded.setup.as_ref().map(|setup| setup.pid), Some(4242));
            assert_eq!(
                recorded.setup.as_ref().map(|setup| setup.state),
                Some(SetupState::InProgress)
            );

            // A runtime destroyed again after a failed environment start is forgotten.
            let gone = RuntimeMetadata {
                provider: RuntimeProviderKind::Sbx,
                ..RuntimeMetadata::default()
            };
            store.record_setup_runtime("alpha", 4242, &gone).unwrap();
            assert!(store
                .get_feature("alpha")
                .unwrap()
                .unwrap()
                .runtime
                .runtime_id
                .is_none());

            // No entry at all (the write-ahead record failed): nothing is created.
            store.record_setup_runtime("beta", 4242, &prepared).unwrap();
            assert!(store.get_feature("beta").unwrap().is_none());
        }

        #[test]
        fn write_ahead_runtime_on_a_live_entry_keeps_its_recorded_identity() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            let mut live = sample_metadata(temp.path(), "alpha");
            live.status = FeatureStatus::FailedRetained;
            live.runtime.provider = RuntimeProviderKind::Sbx;
            live.runtime.runtime_id = Some("branchbox-alpha".to_string());
            live.runtime.container_id = Some("container-1".to_string());
            store.record_start(live).unwrap();
            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.setup = Some(setup_by(4242));
            store.record_setup_started(provisional).unwrap();

            // `--reuse-runtime` woke the same sandbox: what the entry recorded stays.
            let prepared = RuntimeMetadata {
                provider: RuntimeProviderKind::Sbx,
                runtime_id: Some("branchbox-alpha".to_string()),
                ..RuntimeMetadata::default()
            };
            store
                .record_setup_runtime("alpha", 4242, &prepared)
                .unwrap();
            let recorded = store.get_feature("alpha").unwrap().unwrap();
            assert_eq!(
                recorded.runtime.container_id.as_deref(),
                Some("container-1")
            );
            assert_eq!(recorded.status, FeatureStatus::FailedRetained);
        }

        #[test]
        fn write_ahead_replaces_a_removed_entry_and_restores_it_on_discard() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            let mut removed = sample_metadata(temp.path(), "alpha");
            removed.created_at = Utc::now() - chrono::Duration::days(3);
            removed.runtime.runtime_id = Some("stale".to_string());
            store.record_start(removed.clone()).unwrap();
            store.record_teardown("alpha").unwrap();

            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.setup = Some(setup_by(4242));
            let previous = store.record_setup_started(provisional).unwrap();
            assert_eq!(
                previous.as_ref().map(|entry| entry.status.clone()),
                Some(FeatureStatus::Removed)
            );

            let during = store.get_feature("alpha").unwrap().unwrap();
            assert_eq!(during.status, FeatureStatus::Active);
            assert!(during.removed_at.is_none());
            assert!(
                during.runtime.runtime_id.is_none(),
                "stale runtime is dropped"
            );
            assert_eq!(during.created_at, removed.created_at);

            store.discard_setup("alpha", 4242, previous).unwrap();
            assert_eq!(
                store.get_feature("alpha").unwrap().unwrap().status,
                FeatureStatus::Removed
            );
        }

        #[test]
        fn the_final_start_record_and_teardown_clear_the_setup_marker() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.setup = Some(setup_by(4242));
            store.record_setup_started(provisional).unwrap();

            store
                .record_start(sample_metadata(temp.path(), "alpha"))
                .unwrap();
            assert!(store.get_feature("alpha").unwrap().unwrap().setup.is_none());

            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.setup = Some(setup_by(4242));
            store.record_setup_started(provisional).unwrap();
            store.record_teardown("alpha").unwrap();
            let removed = store.get_feature("alpha").unwrap().unwrap();
            assert_eq!(removed.status, FeatureStatus::Removed);
            assert!(removed.setup.is_none());
        }

        #[test]
        fn setup_state_is_interrupted_once_the_process_is_gone_or_the_start_is_stale() {
            let now = Utc::now();
            let ours = SetupRecord::begin();
            assert_eq!(ours.pid, std::process::id());
            assert_eq!(ours.observed_state(now), SetupState::InProgress);

            let stale = SetupRecord {
                started_at: now - chrono::Duration::hours(25),
                ..ours.clone()
            };
            assert_eq!(stale.observed_state(now), SetupState::Interrupted);

            for pid in [0, u32::MAX] {
                assert_eq!(
                    setup_by(pid).observed_state(now),
                    SetupState::Interrupted,
                    "pid {pid}"
                );
            }
            let recorded = SetupRecord {
                state: SetupState::Interrupted,
                ..ours
            };
            assert_eq!(recorded.observed_state(now), SetupState::Interrupted);
        }

        #[cfg(unix)]
        #[test]
        fn setup_by_an_exited_process_is_interrupted() {
            assert_eq!(
                setup_by(dead_pid()).observed_state(Utc::now()),
                SetupState::Interrupted
            );
        }

        #[test]
        fn registries_without_setup_still_load_and_omit_the_key() {
            let registry: FeatureRegistry = serde_json::from_value(serde_json::json!({
                "version": 1,
                "features": [{
                    "work_feature": "alpha",
                    "branch_name": "feature/alpha",
                    "worktree_path": "/r/alpha",
                    "base_branch": null,
                    "feature_url": null,
                    "compose_project_name": null,
                    "env_path": null,
                    "status": "active",
                    "created_at": "2026-10-01T22:50:29.222458Z",
                    "updated_at": "2026-10-01T22:50:29.222458Z",
                    "removed_at": null
                }]
            }))
            .unwrap();
            assert!(registry.features[0].setup.is_none());

            let written = serde_json::to_value(&registry).unwrap();
            assert!(written["features"][0].get("setup").is_none(), "{written}");
        }

        /// `FeatureMetadata` exactly as BranchBox 0.13.4 declared it, before `setup` existed.
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct FeatureMetadataV0134 {
            work_feature: String,
            branch_name: String,
            worktree_path: PathBuf,
            base_branch: Option<String>,
            feature_url: Option<String>,
            compose_project_name: Option<String>,
            env_path: Option<PathBuf>,
            status: FeatureStatus,
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
            removed_at: Option<DateTime<Utc>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tunnel: Option<FeatureTunnelState>,
            #[serde(skip_serializing_if = "Option::is_none")]
            color: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            pr_number: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            last_commit: Option<String>,
            #[serde(default)]
            devcontainer_outdated: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            last_sync_at: Option<DateTime<Utc>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            sync_strategy: Option<String>,
            #[serde(default)]
            start_mode: StartMode,
            #[serde(skip_serializing_if = "Option::is_none")]
            prompt_seed: Option<String>,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            module_outcomes: Vec<ModuleOutcomeRecord>,
            #[serde(skip_serializing_if = "Option::is_none")]
            last_summary_rendered_at: Option<DateTime<Utc>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            adapter: Option<AdapterSummary>,
            #[serde(default)]
            runtime: RuntimeMetadata,
        }

        #[derive(Debug, Deserialize)]
        struct FeatureRegistryV0134 {
            #[serde(deserialize_with = "deserialize_registry_version")]
            #[allow(dead_code)]
            version: String,
            features: Vec<FeatureMetadataV0134>,
        }

        #[test]
        fn a_registry_with_setup_loads_in_0_13_4() {
            let temp = TempDir::new().unwrap();
            let store = FeatureStateStore::new(temp.path());
            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.setup = Some(setup_by(48211));
            store.record_setup_started(provisional).unwrap();

            let written = fs::read_to_string(&store.path).unwrap();
            let raw: Value = serde_json::from_str(&written).unwrap();
            assert_eq!(raw["features"][0]["setup"]["state"], "in_progress");
            assert_eq!(raw["features"][0]["setup"]["pid"], 48211);

            let legacy: FeatureRegistryV0134 = serde_json::from_str(&written).unwrap();
            assert_eq!(legacy.features[0].work_feature, "alpha");
            assert_eq!(legacy.features[0].status, FeatureStatus::Active);
        }

        #[test]
        fn unreadable_setup_records_never_break_the_registry() {
            let entry = |setup: Value| {
                serde_json::json!({
                    "work_feature": "alpha",
                    "branch_name": "feature/alpha",
                    "worktree_path": "/r/alpha",
                    "base_branch": null,
                    "feature_url": null,
                    "compose_project_name": null,
                    "env_path": null,
                    "status": "active",
                    "created_at": "2026-10-01T22:50:29Z",
                    "updated_at": "2026-10-01T22:50:29Z",
                    "removed_at": null,
                    "setup": setup
                })
            };
            let parse = |setup: Value| -> Option<SetupRecord> {
                serde_json::from_value::<FeatureMetadata>(entry(setup))
                    .unwrap()
                    .setup
            };

            let newer = parse(serde_json::json!({
                "state": "resuming", "pid": 7, "started_at": "2026-10-01T22:50:29Z"
            }))
            .expect("an unknown state still reads");
            assert_eq!(newer.state, SetupState::InProgress);
            assert_eq!(newer.pid, 7);
            assert!(parse(serde_json::json!({"state": "in_progress"})).is_none());
            assert!(parse(serde_json::json!("in_progress")).is_none());
            assert!(parse(Value::Null).is_none());
        }

        #[cfg(unix)]
        #[test]
        fn list_reconciles_missing_worktrees_and_unfinished_starts() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            let workflow = FeatureWorkflow::new(&repo).unwrap();
            let store = FeatureStateStore::new(&repo);
            let record =
                |name: &str, status: FeatureStatus, present: bool, setup: Option<SetupRecord>| {
                    let mut metadata = sample_metadata(temp.path(), name);
                    if present {
                        fs::create_dir_all(&metadata.worktree_path).unwrap();
                    }
                    metadata.status = status;
                    metadata.setup = setup;
                    store.record_start(metadata).unwrap();
                };
            record("healthy", FeatureStatus::Active, true, None);
            record("gone", FeatureStatus::Active, false, None);
            record("retained-gone", FeatureStatus::FailedRetained, false, None);
            record("removed", FeatureStatus::Removed, false, None);
            record(
                "starting",
                FeatureStatus::Active,
                true,
                Some(SetupRecord::begin()),
            );
            record(
                "crashed",
                FeatureStatus::Active,
                true,
                Some(setup_by(dead_pid())),
            );

            let listed: BTreeMap<String, FeatureMetadata> = workflow
                .list_features()
                .unwrap()
                .into_iter()
                .map(|feature| (feature.work_feature.clone(), feature))
                .collect();
            let status = |name: &str| listed[name].status.clone();
            let setup_state = |name: &str| listed[name].setup.as_ref().map(|setup| setup.state);

            assert_eq!(status("healthy"), FeatureStatus::Active);
            assert_eq!(status("gone"), FeatureStatus::Orphaned);
            assert_eq!(status("retained-gone"), FeatureStatus::Orphaned);
            assert_eq!(status("removed"), FeatureStatus::Removed);
            assert_eq!(status("starting"), FeatureStatus::Active);
            assert_eq!(setup_state("starting"), Some(SetupState::InProgress));
            assert_eq!(status("crashed"), FeatureStatus::Active);
            assert_eq!(setup_state("crashed"), Some(SetupState::Interrupted));
            assert_eq!(setup_state("healthy"), None);

            // Reconciliation is computed at list time, never written back.
            let stored = store.get_feature("crashed").unwrap().unwrap();
            assert_eq!(stored.status, FeatureStatus::Active);
            assert_eq!(stored.setup.unwrap().state, SetupState::InProgress);
            assert_eq!(
                store.get_feature("gone").unwrap().unwrap().status,
                FeatureStatus::Active
            );
        }

        #[cfg(unix)]
        #[test]
        fn a_start_that_fails_after_creating_its_worktree_stays_registered() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            // A committed `.env` symlink makes the worktree's env provisioning refuse to write
            // through it: a failure after the worktree exists that leaves the worktree behind.
            std::os::unix::fs::symlink("README.md", repo.join(".env")).unwrap();
            for args in [
                vec!["add", "-f", ".env"],
                vec!["commit", "-q", "-m", "Link .env"],
            ] {
                assert!(Command::new("git")
                    .args(&args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success());
            }
            std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

            let workflow = FeatureWorkflow::new(&repo).unwrap();
            let err = workflow
                .start(StartRequest {
                    name: Some("half-done".to_string()),
                    mode: StartMode::Minimal,
                    ..StartRequest::default()
                })
                .unwrap_err();
            assert!(
                err.to_string()
                    .contains("Refusing to write through symlink"),
                "{err}"
            );

            let worktree = temp.path().join("half-done");
            assert!(worktree.exists());
            let listed = workflow.list_features().unwrap();
            assert_eq!(listed.len(), 1);
            let entry = &listed[0];
            assert_eq!(entry.work_feature, "half-done");
            assert_eq!(entry.status, FeatureStatus::Active);
            assert_eq!(
                entry.worktree_path.canonicalize().unwrap(),
                worktree.canonicalize().unwrap()
            );
            let setup = entry.setup.as_ref().expect("write-ahead marker kept");
            assert_eq!(setup.pid, std::process::id());
            // This process is still alive; once it exits the start lists as interrupted.
            assert_eq!(setup.state, SetupState::InProgress);
            assert_eq!(
                setup.observed_state(Utc::now() + chrono::Duration::hours(25)),
                SetupState::Interrupted
            );
        }

        #[test]
        fn a_completed_start_leaves_no_setup_marker() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            fs::write(repo.join(".env"), "APP_URL=dev.example.com\n").unwrap();
            std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

            let workflow = FeatureWorkflow::new(&repo).unwrap();
            workflow
                .start(StartRequest {
                    name: Some("done".to_string()),
                    mode: StartMode::Minimal,
                    skip_modules: vec!["tunnel".to_string()],
                    ..StartRequest::default()
                })
                .unwrap();

            let entry = workflow.state.get_feature("done").unwrap().unwrap();
            assert_eq!(entry.status, FeatureStatus::Active);
            assert!(entry.setup.is_none());
            assert!(entry.last_summary_rendered_at.is_some());
            let raw: Value =
                serde_json::from_str(&fs::read_to_string(&workflow.state.path).unwrap()).unwrap();
            assert!(raw["features"][0].get("setup").is_none());
        }

        #[test]
        fn failed_in_guest_cleanup_discards_the_write_ahead_entry_with_the_worktree() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            let workflow = FeatureWorkflow::new(&repo).unwrap();
            let worktree = temp.path().join("guest");
            workflow
                .git
                .create(&worktree, "feature/guest", None)
                .unwrap();

            let mut provisional = sample_metadata(temp.path(), "guest");
            provisional.setup = Some(SetupRecord::begin());
            let mut warnings = Vec::new();
            let recorded = workflow
                .record_provisional_start(provisional, &mut warnings)
                .expect("write-ahead entry recorded");
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(workflow.state.get_feature("guest").unwrap().is_some());

            workflow.cleanup_failed_in_guest_worktree(
                &worktree,
                "feature/guest",
                None,
                Some(&recorded),
            );
            assert!(!worktree.exists());
            assert!(workflow.state.get_feature("guest").unwrap().is_none());
        }

        #[test]
        fn failed_cleanup_of_a_repository_workspace_keeps_the_write_ahead_entry() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            let workflow = FeatureWorkflow::new(&repo).unwrap();
            let mut provisional = sample_metadata(temp.path(), "in-place");
            provisional.worktree_path = repo.clone();
            provisional.setup = Some(SetupRecord::begin());
            let recorded = workflow
                .record_provisional_start(provisional, &mut Vec::new())
                .unwrap();

            let root = workflow.repo_root.clone();
            workflow.cleanup_failed_in_guest_worktree(
                &root,
                "feature/in-place",
                None,
                Some(&recorded),
            );
            assert!(repo.exists());
            assert!(workflow.state.get_feature("in-place").unwrap().is_some());
        }

        #[test]
        fn an_unrecordable_write_ahead_entry_only_warns() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            fs::create_dir_all(repo.join(".branchbox")).unwrap();
            fs::write(repo.join(".branchbox/registry.json"), "{ not json").unwrap();
            let workflow = FeatureWorkflow::new(&repo).unwrap();

            let mut provisional = sample_metadata(temp.path(), "alpha");
            provisional.setup = Some(SetupRecord::begin());
            let mut warnings = Vec::new();
            assert!(workflow
                .record_provisional_start(provisional, &mut warnings)
                .is_none());
            assert_eq!(warnings.len(), 1);
            assert!(
                warnings[0].contains("Failed to record the in-progress start"),
                "{warnings:?}"
            );
            assert!(warnings[0].contains("Failed to parse feature registry"));
            // Discarding nothing is a no-op.
            workflow.discard_provisional_start(None);
        }

        #[test]
        fn tunnel_commands_name_the_registry_for_an_unregistered_feature() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            let workflow = FeatureWorkflow::new(&repo).unwrap();
            let registry = workflow.repo_root.join(".branchbox/registry.json");

            let errors = [
                workflow
                    .tunnel_open(TunnelOpenRequest {
                        work_feature: "nope".to_string(),
                    })
                    .unwrap_err(),
                workflow
                    .tunnel_remove(TunnelRemoveRequest {
                        work_feature: "nope".to_string(),
                        force: false,
                    })
                    .unwrap_err(),
            ];
            for err in errors {
                assert_eq!(err.code(), "feature_not_found", "{err:?}");
                assert_eq!(
                    err.to_string(),
                    format!("Feature 'nope' is not registered in {}", registry.display())
                );
            }
        }

        #[test]
        fn tunnel_open_names_the_env_file_it_could_not_read() {
            let temp = TempDir::new().unwrap();
            let repo = nested_test_repo(&temp);
            let workflow = FeatureWorkflow::new(&repo).unwrap();
            workflow
                .state
                .record_start(sample_metadata(temp.path(), "alpha"))
                .unwrap();

            let err = workflow
                .tunnel_open(TunnelOpenRequest {
                    work_feature: "alpha".to_string(),
                })
                .unwrap_err();
            let env_path = workflow.repo_root.join(".env");
            let message = err.to_string();
            assert!(
                message.starts_with(&format!(
                    "Validation error: Cannot derive a tunnel hostname for 'alpha' from {}: \
                     Failed to read .env file:",
                    env_path.display()
                )),
                "{message}"
            );
            assert!(
                message.ends_with("Set APP_URL in that file and retry."),
                "{message}"
            );
        }
    }
}
