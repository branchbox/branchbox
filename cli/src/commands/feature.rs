use crate::json_error;
use anyhow::{anyhow, bail, Result};
use chrono::Local;
use clap::{Args, Subcommand};
use dialoguer::{theme::ColorfulTheme, Confirm};
use serde::Serialize;
use serde_json::json;
use shell_words::split as split_command_line;
use std::{
    env, fmt,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use worktree_core::{
    config::BranchBoxConfig,
    human, humanln, output,
    runtime::{self, RuntimeProviderKind},
    workflows::{
        feature::{
            DevcontainerReusePolicy, FeatureMetadata, FeatureStatus, FeatureTunnelStatus,
            FeatureWorkflow, ModuleOutcome, ModuleOutcomeRecord, ModuleSkipRecord, ModuleStatus,
            StartMode, StartRequest, StartSummary, TeardownRequest, TeardownSummary, WorkspaceMode,
        },
        teardown_plan::{
            BranchAction, BranchSource, TeardownOptions, TeardownPlan, REFUSAL_LISTED_FILES,
        },
    },
    Error as CoreError,
};

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3):
/// `prune-json` is `prune --dry-run --json`, `prune --yes --json` and `--feature` selection
/// (§5.9).
pub const CAPABILITIES: &[&str] = &["prune-json"];

/// `schema_version` of the prune documents.
const PRUNE_SCHEMA_VERSION: u32 = 1;

/// The 0.13 refusal for dirty devcontainer/compose files. Text mode keeps it as the first
/// `Error:` line when module files are among the changes, so scripts that match it still do.
const LEGACY_MODULE_REFUSAL: &str =
    "Devcontainer/compose changes detected; rerun this command with --force to proceed.";

const DEFAULT_MINIMAL_PROMPT: &str = "You are the default BranchBox coding agent operating in minimal mode. Devcontainer, compose, and specs modules were skipped to keep setup lightweight—focus on quick tweaks or documentation updates, and run `branchbox devcontainer sync` later if full provisioning becomes necessary.";

#[derive(Subcommand)]
pub enum FeatureCommands {
    /// Create a new feature worktree and run module setup
    #[command(alias = "new")]
    Start(FeatureStartArgs),

    /// Tear down an existing feature worktree
    Teardown(FeatureTeardownArgs),

    /// List known feature worktrees from the registry
    List(FeatureListArgs),

    /// Tear down all active feature worktrees
    Prune(FeaturePruneArgs),

    /// Execute a command through a feature's runtime provider
    Exec(FeatureExecArgs),

    /// Execute an allowlisted coding provider with name-only environment inheritance
    ExecProvider(FeatureExecProviderArgs),

    /// Dispatch one capability-bound request to a trusted managed tool endpoint
    DispatchTool(FeatureDispatchToolArgs),
}

#[derive(Args)]
pub struct FeatureStartArgs {
    /// Dasherized feature name (e.g., oauth-integration)
    pub name: Option<String>,

    /// Free-form feature title (converted to dasherized name)
    #[arg(long)]
    pub title: Option<String>,

    /// Base branch to branch from (defaults to current HEAD)
    #[arg(long)]
    pub base: Option<String>,

    /// Override branch prefix (defaults to "feature")
    #[arg(long)]
    pub branch_prefix: Option<String>,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Allow reusing an existing worktree directory
    #[arg(long)]
    pub reuse: bool,

    /// Check the feature branch out in the repository instead of a worktree.
    ///
    /// A worktree lets several features share one clone. A caller that clones
    /// per run and discards the clone has nothing to share it with, and pays
    /// for the second checkout identity anyway.
    #[arg(long, conflicts_with = "reuse")]
    pub no_worktree: bool,

    /// Copy-mode conflict policy when reusing a worktree (fail, preserve, overwrite, inspect)
    #[arg(
        long,
        value_name = "POLICY",
        default_value = "fail",
        requires = "reuse"
    )]
    pub devcontainer_reuse: DevcontainerReusePolicy,

    /// Retain a failed SBX runtime and its build cache for inspection or retry
    #[arg(long)]
    pub keep_runtime_on_failure: bool,

    /// Reuse a retained runtime (implies --reuse and --keep-runtime-on-failure)
    #[arg(long, conflicts_with = "reuse")]
    pub reuse_runtime: bool,

    /// Emit verbose telemetry (e.g. Cloudflare operations)
    #[arg(long)]
    pub telemetry: bool,

    /// Skip specific modules during setup (can be specified multiple times)
    /// Available modules: compose, database, tunnel, specs
    #[arg(long = "skip-module", value_name = "MODULE")]
    pub skip_modules: Vec<String>,

    /// Start feature workflow in minimal mode (skips heavyweight modules)
    #[arg(long)]
    pub minimal: bool,

    /// Alias for --minimal (hidden)
    #[arg(long = "fast", hide = true)]
    pub fast: bool,

    /// Provide an optional prompt seed for automation/agent hand-off
    #[arg(long)]
    pub prompt: Option<String>,

    /// Use the default minimal-mode prompt shortcut (only valid with --minimal/--fast)
    #[arg(long = "default-prompt", conflicts_with = "prompt")]
    pub default_prompt: bool,

    /// Emit JSON summary payload instead of human-readable text
    #[arg(long)]
    pub json: bool,

    /// Allow running feature start from inside a containerized environment
    #[arg(long, alias = "no-host-check")]
    pub allow_container: bool,

    /// Suppress summary output (text mode only)
    #[arg(long = "no-summary")]
    pub no_summary: bool,

    /// Workspace isolation runtime (container, sbx, or Linux/KVM local-vm)
    #[arg(long, value_name = "PROVIDER")]
    pub runtime: Option<RuntimeProviderKind>,

    /// Absolute supervisor-authored assignment manifest (required by --runtime in-guest)
    #[arg(long, value_name = "PATH")]
    pub runtime_manifest: Option<PathBuf>,
}

#[derive(Args)]
pub struct FeatureTeardownArgs {
    /// Dasherized feature name to tear down (e.g., oauth-integration)
    pub name: String,

    /// Override branch prefix (defaults to "feature")
    #[arg(long)]
    pub branch_prefix: Option<String>,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Keep the git branch after removing the worktree (default is to delete it)
    #[arg(long)]
    pub keep_branch: bool,

    /// Delete the git branch after removing the worktree
    #[arg(long, conflicts_with = "keep_branch")]
    pub delete_branch: bool,

    /// Remove the worktree whatever its state: discard uncommitted changes, remove a locked or
    /// unreadable worktree (deleting the directory if git cannot), and force-delete the branch
    /// (`git branch -D`, even with unmerged commits) when deleting it. Prefer
    /// --discard-changes, which keeps unmerged commits safe
    #[arg(long)]
    pub force: bool,

    /// Force-delete the git branch even if it is not fully merged (`git branch -D`)
    #[arg(long)]
    pub force_delete_branch: bool,

    /// Discard the worktree's uncommitted changes (modified and untracked files) instead of
    /// refusing. Does not force-delete the branch
    #[arg(long)]
    pub discard_changes: bool,

    /// Print what teardown would do (the teardown plan) and change nothing
    #[arg(long)]
    pub dry_run: bool,

    /// Move spec to completed during teardown
    #[arg(long)]
    pub complete_spec: bool,

    /// Emit verbose telemetry (e.g. Cloudflare operations)
    #[arg(long)]
    pub telemetry: bool,

    /// Allow running feature teardown from inside a containerized environment
    #[arg(long, alias = "no-host-check")]
    pub allow_container: bool,

    /// Emit deterministic teardown and residue evidence as JSON (with --dry-run, the plan)
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct FeatureListArgs {
    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Filter by status (active, degraded, failed_retained, orphaned, removed)
    #[arg(long)]
    pub status: Option<String>,

    /// Include removed features (retained and orphaned features are shown by default)
    #[arg(long, conflicts_with = "status")]
    pub all: bool,

    /// Emit JSON output instead of human-readable summary
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct FeaturePruneArgs {
    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Show which features would be removed without applying teardown
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Keep git branches after removing worktrees
    #[arg(long)]
    pub keep_branch: bool,

    /// Delete git branches after removing worktrees
    #[arg(long, conflicts_with = "keep_branch")]
    pub delete_branch: bool,

    /// Move specs to completed during teardown
    #[arg(long)]
    pub complete_spec: bool,

    /// Emit verbose telemetry (e.g. Cloudflare operations)
    #[arg(long)]
    pub telemetry: bool,

    /// Allow running feature prune from inside a containerized environment
    #[arg(long, alias = "no-host-check")]
    pub allow_container: bool,

    /// Prune only this feature (repeatable); by default every active or retained feature
    #[arg(long = "feature", value_name = "NAME")]
    pub features: Vec<String>,

    /// Emit the dry-run candidates, or the prune results, as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct FeatureExecArgs {
    /// Dasherized feature name
    pub name: String,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Emit captured command output as JSON
    #[arg(long)]
    pub json: bool,

    /// Command and arguments to execute
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Args)]
pub struct FeatureExecProviderArgs {
    /// Dasherized feature name
    pub name: String,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Exact provider executable declared by the managed runtime assignment
    #[arg(long, value_name = "EXECUTABLE")]
    pub provider: String,

    /// Allowlisted environment name to inherit without transporting its value
    #[arg(long = "inherit-env", value_name = "NAME")]
    pub inherit_env: Vec<String>,

    /// Arguments passed to the fixed provider executable
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Args)]
pub struct FeatureDispatchToolArgs {
    /// Dasherized feature name
    pub name: String,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Exact tool-request lease declared by the managed runtime assignment
    #[arg(long, value_name = "LEASE_ID")]
    pub lease: String,

    /// Exact request identifier and spool filename stem
    #[arg(long, value_name = "REQUEST_ID")]
    pub request_id: String,

    /// Emit the correlated response as JSON
    #[arg(long)]
    pub json: bool,

    /// Wait up to this many seconds for the atomic request file (maximum 300)
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 0,
        value_parser = clap::value_parser!(u64).range(0..=300)
    )]
    pub wait_seconds: u64,
}

impl FeatureCommands {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        match self {
            FeatureCommands::Start(args) => args.json,
            FeatureCommands::Teardown(args) => args.json,
            FeatureCommands::List(args) => args.json,
            FeatureCommands::Prune(args) => args.wants_json(),
            FeatureCommands::Exec(args) => args.json,
            FeatureCommands::ExecProvider(_) => false,
            FeatureCommands::DispatchTool(args) => args.json,
        }
    }
}

impl FeaturePruneArgs {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        self.json
    }
}

pub fn execute(command: FeatureCommands) -> Result<()> {
    match command {
        FeatureCommands::Start(args) => run_start(args),
        FeatureCommands::Teardown(args) => run_teardown(args),
        FeatureCommands::List(args) => run_list(args),
        FeatureCommands::Prune(args) => run_prune(args),
        FeatureCommands::Exec(args) => run_exec(args),
        FeatureCommands::ExecProvider(args) => run_exec_provider(args),
        FeatureCommands::DispatchTool(args) => run_dispatch_tool(args),
    }
}

fn run_dispatch_tool(args: FeatureDispatchToolArgs) -> Result<()> {
    let repo_path = args.repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let deadline = Instant::now() + Duration::from_secs(args.wait_seconds);
    let result = loop {
        match workflow.dispatch_tool_request(&args.name, &args.lease, &args.request_id) {
            Ok(result) => break result,
            Err(
                CoreError::ToolRequestNotPending { .. }
                | CoreError::ToolRequestRelayRetryable { .. },
            ) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(250));
            }
            Err(CoreError::ToolRequestNotPending { .. }) => {
                if args.json {
                    output::emit_json(&json!({
                        "status": "not-pending",
                        "retryable": true,
                        "lease_id": args.lease,
                        "request_id": args.request_id
                    }))?;
                }
                std::process::exit(75);
            }
            Err(err) => return Err(err.into()),
        }
    };
    if args.json {
        output::emit_json(&json!({
            "status": "dispatched",
            "retryable": false,
            "result": result
        }))?;
    } else {
        humanln!("{}", serde_json::to_string(&result.response)?);
    }
    Ok(())
}

fn run_exec_provider(args: FeatureExecProviderArgs) -> Result<()> {
    let repo_path = args.repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let exit_code = workflow.exec_provider_runtime_interactive(
        &args.name,
        &args.provider,
        &args.inherit_env,
        &args.command,
    )?;
    if exit_code != 0 {
        bail!("Managed provider exited with status {exit_code}");
    }
    Ok(())
}

fn run_exec(args: FeatureExecArgs) -> Result<()> {
    let repo_path = args.repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let result = workflow.exec_runtime(&args.name, &args.command)?;

    if args.json {
        // In-band failure (DESIGN §5.2 rule 4): the payload is the document, so a failing inner
        // command adds no envelope.
        output::emit_json(&result)?;
    } else {
        human!("{}", result.stdout);
        eprint!("{}", result.stderr);
    }

    if result.exit_code != 0 {
        bail!("Runtime command exited with status {}", result.exit_code);
    }
    Ok(())
}

fn run_start(args: FeatureStartArgs) -> Result<()> {
    let FeatureStartArgs {
        name,
        title,
        base,
        branch_prefix,
        repo,
        reuse,
        devcontainer_reuse,
        keep_runtime_on_failure,
        reuse_runtime,
        telemetry,
        skip_modules,
        minimal,
        fast,
        prompt,
        default_prompt,
        json,
        allow_container,
        no_summary,
        runtime,
        runtime_manifest,
        no_worktree,
    } = args;

    let mode = if minimal || fast {
        StartMode::Minimal
    } else {
        StartMode::Full
    };

    if default_prompt && mode != StartMode::Minimal {
        // A usage refusal, not a crash: coded validation_failed, with the same text as before.
        return Err(json_error::CliError::new(
            "validation_failed",
            "--default-prompt can only be used with --minimal or --fast",
        )
        .into());
    }

    const PROMPT_MAX_CHARS: usize = 2000;
    let mut prompt_seed = prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut truncation_warning = None;
    if let Some(ref mut seed) = prompt_seed {
        let len = seed.chars().count();
        if len > PROMPT_MAX_CHARS {
            let truncated: String = seed.chars().take(PROMPT_MAX_CHARS).collect();
            let warning =
                format!("Prompt truncated to {PROMPT_MAX_CHARS} characters before storage.");
            humanln!("⚠️  {warning}");
            truncation_warning = Some(warning);
            *seed = truncated;
        }
    }

    if default_prompt && prompt_seed.is_none() {
        prompt_seed = Some(DEFAULT_MINIMAL_PROMPT.to_string());
    }

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let request = StartRequest {
        name,
        title,
        base_branch: base,
        branch_prefix,
        reuse_existing: reuse || reuse_runtime,
        devcontainer_reuse,
        keep_runtime_on_failure: keep_runtime_on_failure || reuse_runtime,
        telemetry,
        skip_modules,
        mode,
        prompt_seed: prompt_seed.clone(),
        runtime,
        runtime_manifest,
        workspace_mode: if no_worktree {
            WorkspaceMode::Repository
        } else {
            WorkspaceMode::Worktree
        },
    };

    let _host_validation_override = HostValidationOverride::new(allow_container);
    let mut summary = workflow.start(request)?;
    // In JSON mode the notice above went to stderr, so the payload carries it as a warning. Text
    // mode already showed it; repeating it under "Warnings:" would change the text output.
    if let Some(warning) = truncation_warning.filter(|_| json) {
        summary.warnings.push(warning);
    }
    print_start_summary(&summary, json, no_summary)?;

    Ok(())
}

struct HostValidationOverride {
    previous: Option<std::ffi::OsString>,
}

impl HostValidationOverride {
    fn new(enabled: bool) -> Option<Self> {
        if !enabled {
            return None;
        }

        let previous = env::var_os("BRANCHBOX_SKIP_HOST_VALIDATION");
        env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");
        Some(Self { previous })
    }
}

impl Drop for HostValidationOverride {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", previous);
        } else {
            env::remove_var("BRANCHBOX_SKIP_HOST_VALIDATION");
        }
    }
}

fn run_list(args: FeatureListArgs) -> Result<()> {
    let FeatureListArgs {
        repo,
        status,
        all,
        json,
    } = args;
    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let mut features = workflow.list_features()?;
    let total_count = features.len();
    let active_count = features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::Active)
        .count();
    let failed_count = features
        .iter()
        .filter(|feature| {
            matches!(
                feature.status,
                FeatureStatus::Degraded | FeatureStatus::FailedRetained | FeatureStatus::Orphaned
            )
        })
        .count();
    let removed_count = features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::Removed)
        .count();

    if let Some(status_filter) = status.as_ref() {
        let parsed: FeatureStatus = status_filter.parse().map_err(|err| {
            json_error::recode(anyhow::Error::new(err), "validation_failed", None)
        })?;
        features.retain(|feature| feature.status == parsed);
    } else if !all {
        features.retain(|feature| feature.status != FeatureStatus::Removed);
    }

    if total_count == 0 {
        if json {
            output::emit_json(&[] as &[FeatureMetadata])?;
        } else {
            humanln!("ℹ️  No features tracked yet. Run `branchbox feature start` to create one.");
        }
        return Ok(());
    }

    if features.is_empty() {
        if json {
            output::emit_json(&[] as &[FeatureMetadata])?;
        } else if let Some(filter) = status.as_ref() {
            humanln!(
                "ℹ️  No features found with status '{}'.",
                filter.to_ascii_lowercase()
            );
        } else if !all {
            humanln!(
                "ℹ️  No active or retained features. Use `branchbox feature list --all` to include removed entries."
            );
        } else {
            humanln!("ℹ️  No features match the requested filters.");
        }
        return Ok(());
    }

    let agent_config = AgentLaunchConfig::from_env();
    let agent_config_ref = agent_config.as_ref();
    let entries: Vec<(FeatureMetadata, AgentPlan)> = features
        .into_iter()
        .map(|feature| {
            let plan = determine_agent_plan(
                devcontainer_status_from_metadata(&feature),
                agent_config_ref,
            );
            (feature, plan)
        })
        .collect();

    if json {
        #[derive(Serialize)]
        struct FeatureListEntry<'a> {
            #[serde(flatten)]
            feature: &'a FeatureMetadata,
            #[serde(rename = "default_agent")]
            agent: &'a AgentPlan,
        }

        let payload: Vec<_> = entries
            .iter()
            .map(|(feature, plan)| FeatureListEntry {
                feature,
                agent: plan,
            })
            .collect();
        output::emit_json(&payload)?;
        return Ok(());
    }

    let showing_count = entries.len();
    humanln!(
        "📚 Feature registry — {} active · {} retained/orphaned · {} removed (showing {}/{})",
        active_count,
        failed_count,
        removed_count,
        showing_count,
        total_count
    );

    let headers = [
        "Feature",
        "Status",
        "Mode",
        "Runtime",
        "Prompt",
        "Modules",
        "Branch",
        "URL",
        "Tunnel",
        "Devcontainer",
        "Agent",
        "PR",
        "Color",
        "Updated",
    ];
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    let format_ts = |ts: &chrono::DateTime<chrono::Utc>| -> String {
        ts.with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    };

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(entries.len());
    for (feature, agent_plan) in &entries {
        let url = feature_url_for_list(feature);
        let tunnel = tunnel_summary_for_list(feature);
        let devcontainer = devcontainer_status_for_list(feature);
        let agent = summarize_agent_plan(agent_plan);
        let pr = feature
            .pr_number
            .map(|number| format!("#{number}"))
            .unwrap_or_else(|| "—".to_string());
        let color = feature.color.as_deref().unwrap_or("—").to_string();
        let updated = format_ts(&feature.updated_at);
        let prompt = summarize_prompt_seed(feature.prompt_seed.as_ref());
        let module_health = summarize_module_health(&feature.module_outcomes);

        let row = vec![
            feature.work_feature.clone(),
            feature.status.to_string(),
            feature.start_mode.to_string(),
            feature.runtime.provider.to_string(),
            prompt,
            module_health,
            feature.branch_name.clone(),
            url,
            tunnel,
            devcontainer,
            agent,
            pr,
            color,
            updated,
        ];

        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.len());
        }

        rows.push(row);
    }

    let header_line = headers
        .iter()
        .enumerate()
        .map(|(idx, header)| format!("{:<width$}", header, width = widths[idx]))
        .collect::<Vec<_>>()
        .join("  ");
    humanln!("{}", header_line);

    let separator = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("  ");
    humanln!("{}", separator);

    for row in rows {
        let line = row
            .iter()
            .enumerate()
            .map(|(idx, cell)| format!("{:<width$}", cell, width = widths[idx]))
            .collect::<Vec<_>>()
            .join("  ");
        humanln!("{}", line);
    }

    Ok(())
}

fn feature_url_for_list(feature: &FeatureMetadata) -> String {
    feature
        .feature_url
        .as_ref()
        .map(|url| format!("https://{}", url))
        .unwrap_or_else(|| "—".to_string())
}

fn tunnel_summary_for_list(feature: &FeatureMetadata) -> String {
    feature
        .tunnel
        .as_ref()
        .map(|state| {
            let mut label = state.status.to_string();
            if let Some(host) = state.hostname.as_ref() {
                if !host.is_empty() {
                    label = format!("{label} ({host})");
                }
            }
            label
        })
        .unwrap_or_else(|| "—".to_string())
}

fn devcontainer_status_for_list(feature: &FeatureMetadata) -> String {
    if feature.devcontainer_outdated {
        "outdated".to_string()
    } else if let Some(sync_at) = feature.last_sync_at.as_ref() {
        format!(
            "synced {}",
            sync_at.with_timezone(&Local).format("%Y-%m-%d")
        )
    } else {
        "never".to_string()
    }
}

fn summarize_prompt_seed(seed: Option<&String>) -> String {
    match seed {
        Some(value) => format!("seed ({} chars)", value.chars().count()),
        None => "—".to_string(),
    }
}

fn summarize_module_health(outcomes: &[ModuleOutcomeRecord]) -> String {
    if outcomes.is_empty() {
        return "—".to_string();
    }

    let mut success = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut forced = 0;

    for outcome in outcomes {
        match outcome.status {
            ModuleStatus::Success => success += 1,
            ModuleStatus::Skipped => skipped += 1,
            ModuleStatus::Failed => failed += 1,
        }
        if outcome.forced {
            forced += 1;
        }
    }

    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(format!("{failed} fail"));
    }
    if success > 0 {
        parts.push(format!("{success} ok"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skip"));
    }
    if forced > 0 {
        parts.push(format!("{forced} forced"));
    }

    if parts.is_empty() {
        "pending".to_string()
    } else {
        parts.join(" / ")
    }
}

fn summarize_agent_plan(plan: &AgentPlan) -> String {
    match plan.status {
        AgentPlanStatus::Ready => plan
            .label
            .as_deref()
            .map(|label| format!("ready ({label})"))
            .unwrap_or_else(|| "ready".to_string()),
        _ => plan.status.to_string(),
    }
}

fn run_teardown(args: FeatureTeardownArgs) -> Result<()> {
    let FeatureTeardownArgs {
        name,
        branch_prefix,
        repo,
        keep_branch,
        delete_branch,
        force,
        force_delete_branch,
        discard_changes,
        dry_run,
        complete_spec,
        telemetry,
        allow_container,
        json,
    } = args;

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let _host_validation_override = HostValidationOverride::new(allow_container);
    workflow.validate_host()?;

    let config = BranchBoxConfig::load(workflow.repo_root()).unwrap_or_default();
    let default_delete_branch = config.feature.teardown.delete_branch_by_default;

    let delete_branch = if keep_branch {
        false
    } else if delete_branch {
        true
    } else {
        default_delete_branch
    };
    // No branch prefix unless given: core deletes the branch the registry recorded.
    let mut request = TeardownRequest {
        work_feature: name,
        branch_prefix,
        delete_branch,
        force_delete_branch,
        force_remove: force,
        force_remove_modules: force,
        complete_spec,
        telemetry,
    };
    let mut options = TeardownOptions {
        discard_changes: force || discard_changes,
        require_mergeable_branch: true,
    };

    let mut plan = workflow.plan_teardown(&request, &options)?;
    if !plan.worktree.exists && !force && !dry_run {
        return Err(CoreError::WorktreeMissing {
            name: request.work_feature,
            path: plan.worktree.path,
        }
        .into());
    }
    let interactive = output::is_interactive() && !dry_run;

    // The branch decision first: an unmerged branch that would be deleted.
    if let Some((branch, _)) = plan.unmerged_branch() {
        let branch = branch.to_string();
        if decide_unmerged_branch(&config, &mut request, &branch, interactive)? {
            plan = workflow.plan_teardown(&request, &options)?;
        }
    }

    // Then the dirty decision: user changes nobody agreed to discard.
    if plan.blocks_on_uncommitted_changes() {
        if !dry_run {
            print_uncommitted_banner(&plan);
        }
        if interactive && confirm_discard()? {
            options.discard_changes = true;
            plan = workflow.plan_teardown(&request, &options)?;
        }
    }

    if dry_run {
        if json {
            output::emit_json(&plan)?;
        } else {
            print_teardown_plan(&plan, force);
        }
        return Ok(());
    }

    if plan.is_blocked() {
        return Err(teardown_refusal(plan));
    }

    let summary = workflow.teardown_with_options(request, options)?;
    if json {
        output::emit_json(&summary)?;
    } else {
        print_teardown_summary(&summary);
    }

    Ok(())
}

/// Decide what to do with an unmerged branch that teardown was asked to delete (S5). Returns
/// whether `request` changed.
///
/// - `force_delete_unmerged_by_default` force-deletes it;
/// - interactively, as in 0.13: ask when `prompt_force_delete_unmerged` (yes force-deletes,
///   no keeps the branch), otherwise keep it;
/// - otherwise nothing changes and teardown refuses before touching anything, naming
///   `--keep-branch` and `--force-delete-branch`.
fn decide_unmerged_branch(
    config: &BranchBoxConfig,
    request: &mut TeardownRequest,
    branch: &str,
    interactive: bool,
) -> Result<bool> {
    if config.feature.teardown.force_delete_unmerged_by_default {
        request.force_delete_branch = true;
        return Ok(true);
    }
    if !interactive {
        return Ok(false);
    }

    let force_delete = config.feature.teardown.prompt_force_delete_unmerged
        && Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!(
                "Branch '{}' is not fully merged. Force delete it with -D?",
                branch
            ))
            .default(false)
            .interact()?;
    if force_delete {
        request.force_delete_branch = true;
    } else {
        request.delete_branch = false;
        humanln!("ℹ️  Keeping branch '{}' (not fully merged).", branch);
    }
    Ok(true)
}

/// The dirty-worktree banner. The first line stays the 0.13 text when module files changed:
/// `scripts/manual-cli-e2e.sh` matches "Detected devcontainer/compose changes".
fn print_uncommitted_banner(plan: &TeardownPlan) {
    let what = if plan.has_module_area_changes() {
        "devcontainer/compose changes"
    } else {
        "uncommitted changes"
    };
    humanln!(
        "⚠️  Detected {} inside {}:",
        what,
        plan.worktree.path.display()
    );
    for change in plan.changes.user.iter().take(REFUSAL_LISTED_FILES) {
        humanln!("    • {} ({})", change.path, change.kind.label());
    }
    let unlisted = plan.changes.user.len().saturating_sub(REFUSAL_LISTED_FILES);
    if unlisted > 0 || plan.changes.truncated {
        humanln!("    • … and more");
    }
    humanln!("    (BranchBox refuses to discard them without --discard-changes or --force)");
}

fn confirm_discard() -> Result<bool> {
    let proceed = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Discard these changes and continue teardown?")
        .default(false)
        .interact()?;
    if !proceed {
        bail!("Teardown aborted; nothing was removed. Rerun with --discard-changes to discard the changes.");
    }
    Ok(true)
}

/// The refusal for a blocked plan: `teardown_refused` with the plan in its details. In text
/// mode, module-file changes keep the 0.13 line as the first `Error:` line, with the
/// cause-naming refusal under it.
fn teardown_refusal(plan: TeardownPlan) -> anyhow::Error {
    let legacy_line = !output::machine_mode()
        && plan.blocks_on_uncommitted_changes()
        && plan.has_module_area_changes();
    let refusal = anyhow::Error::new(plan.into_refusal());
    if legacy_line {
        refusal.context(LEGACY_MODULE_REFUSAL)
    } else {
        refusal
    }
}

/// `feature teardown --dry-run` in text mode.
fn print_teardown_plan(plan: &TeardownPlan, force: bool) {
    humanln!(
        "🧭 Teardown plan for '{}' (dry run; nothing was changed)",
        plan.work_feature
    );
    let mut worktree = format!(
        "  Worktree: {} ({})",
        plan.worktree.path.display(),
        if plan.worktree.exists {
            "exists"
        } else {
            "missing"
        }
    );
    if plan.worktree.locked {
        worktree.push_str(", locked");
        if let Some(reason) = &plan.worktree.lock_reason {
            worktree.push_str(&format!(": {reason}"));
        }
    }
    humanln!("{}", worktree);

    if !plan.changes.status_available {
        humanln!("  Uncommitted changes: unknown (git status failed)");
    } else if plan.changes.user.is_empty() {
        humanln!("  Uncommitted changes: none");
    } else {
        let fate = if plan.blocks_on_uncommitted_changes() {
            "teardown refuses"
        } else {
            "discarded"
        };
        humanln!(
            "  Uncommitted changes ({}): {}{}",
            fate,
            plan.changes.user.len(),
            if plan.changes.truncated { "+" } else { "" }
        );
        for change in &plan.changes.user {
            humanln!("    • {} ({})", change.path, change.kind.label());
        }
    }
    if !plan.changes.generated.is_empty() {
        humanln!(
            "  BranchBox-generated files (discarded): {}",
            plan.changes.generated.len()
        );
    }
    for file in &plan.changes.preserved {
        humanln!(
            "  Kept: {} → {} in the main worktree",
            file.path,
            file.destination
        );
    }
    if let Some(branch) = &plan.branch {
        let source = match branch.source {
            BranchSource::ExplicitPrefix => "from --branch-prefix",
            BranchSource::Registry => "from the registry",
            BranchSource::ConfigPrefix => "from the configured prefix",
        };
        let state = if !branch.exists {
            "does not exist".to_string()
        } else if branch.merged {
            format!("merged into {}", branch.reference_name)
        } else {
            format!(
                "{} {} not in {}",
                branch.ahead,
                if branch.ahead == 1 {
                    "commit"
                } else {
                    "commits"
                },
                branch.reference_name
            )
        };
        let action = match branch.action {
            BranchAction::Keep => "keep",
            BranchAction::Delete => "delete (git branch -d)",
            BranchAction::ForceDelete => "force-delete (git branch -D)",
        };
        humanln!(
            "  Branch: {} ({}; {}) → {}",
            branch.name,
            source,
            state,
            action
        );
    }
    if !plan.warnings.is_empty() {
        humanln!("  Warnings:");
        for warning in &plan.warnings {
            humanln!("    - {}", warning);
        }
    }
    if plan.is_blocked() {
        humanln!("✗ Teardown would refuse:");
        for blocker in &plan.blockers {
            humanln!("    - {}", blocker.message());
        }
    } else if !plan.worktree.exists && !force {
        humanln!(
            "✗ Teardown would refuse: the worktree {} is missing; rerun with --force to tear \
             down what is left.",
            plan.worktree.path.display()
        );
    } else {
        humanln!("✓ Teardown would proceed.");
    }
}

/// What `prune` does to every feature (CLI users only): the documented forced teardown, which
/// discards uncommitted changes and force-deletes branches when deleting them.
#[derive(Debug, Clone, Copy, Serialize)]
struct PrunePolicy {
    delete_branch: bool,
    force_delete_branch: bool,
    discard_changes: bool,
    complete_spec: bool,
}

impl PrunePolicy {
    fn request(&self, feature: &FeatureMetadata, telemetry: bool) -> TeardownRequest {
        TeardownRequest {
            work_feature: feature.work_feature.clone(),
            // Core deletes the branch the registry recorded.
            branch_prefix: None,
            delete_branch: self.delete_branch,
            force_delete_branch: self.force_delete_branch,
            force_remove: true,
            force_remove_modules: true,
            complete_spec: self.complete_spec,
            telemetry,
        }
    }

    fn options(&self) -> TeardownOptions {
        TeardownOptions {
            discard_changes: self.discard_changes,
            require_mergeable_branch: false,
        }
    }
}

/// One feature `prune` would tear down, with its plan under the prune policy.
#[derive(Debug, Serialize)]
struct PruneCandidate {
    work_feature: String,
    status: FeatureStatus,
    branch_name: String,
    worktree_path: PathBuf,
    plan: Option<TeardownPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_error: Option<String>,
}

impl PruneCandidate {
    /// What pruning this feature destroys, for the text listing.
    fn losses(&self) -> Vec<String> {
        let Some(plan) = &self.plan else {
            return vec![format!(
                "plan unavailable: {}",
                self.plan_error.as_deref().unwrap_or("unknown error")
            )];
        };
        let mut losses = Vec::new();
        let changes = plan.changes.user.len();
        if changes > 0 {
            losses.push(format!(
                "{}{} uncommitted {} discarded",
                changes,
                if plan.changes.truncated { "+" } else { "" },
                if changes == 1 { "change" } else { "changes" }
            ));
        }
        if let Some(branch) = unmerged_force_delete(plan) {
            losses.push(format!(
                "branch {} force-deleted with {} unmerged {}",
                branch.0,
                branch.1,
                if branch.1 == 1 { "commit" } else { "commits" }
            ));
        }
        losses
    }
}

/// The branch and unmerged commit count a forced prune would delete, if any.
fn unmerged_force_delete(plan: &TeardownPlan) -> Option<(&str, u32)> {
    plan.branch
        .as_ref()
        .filter(|branch| {
            branch.action == BranchAction::ForceDelete && branch.exists && !branch.merged
        })
        .map(|branch| (branch.name.as_str(), branch.ahead))
}

#[derive(Debug, Serialize)]
struct PruneDryRun<'a> {
    schema_version: u32,
    dry_run: bool,
    policy: PrunePolicy,
    candidates: &'a [PruneCandidate],
    at_risk: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct PruneOutcome {
    work_feature: String,
    outcome: &'static str,
    summary: Option<TeardownSummary>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct PruneReport {
    schema_version: u32,
    dry_run: bool,
    results: Vec<PruneOutcome>,
    pruned: usize,
    failed: usize,
}

/// What a prune would destroy: uncommitted changes and unmerged commits, per feature.
fn prune_at_risk(candidates: &[PruneCandidate]) -> serde_json::Value {
    let mut uncommitted = Vec::new();
    let mut unmerged = Vec::new();
    for candidate in candidates {
        let Some(plan) = &candidate.plan else {
            continue;
        };
        if !plan.changes.user.is_empty() {
            uncommitted.push(json!({
                "work_feature": candidate.work_feature,
                "count": plan.changes.user.len(),
                "truncated": plan.changes.truncated,
                "paths": plan.changes.user.iter().map(|change| &change.path).collect::<Vec<_>>(),
            }));
        }
        if let Some((branch, ahead)) = unmerged_force_delete(plan) {
            unmerged.push(json!({
                "work_feature": candidate.work_feature,
                "branch": branch,
                "ahead": ahead,
            }));
        }
    }
    json!({ "uncommitted_changes": uncommitted, "unmerged_commits": unmerged })
}

pub fn run_prune(args: FeaturePruneArgs) -> Result<()> {
    let FeaturePruneArgs {
        repo,
        dry_run,
        yes,
        keep_branch,
        delete_branch,
        complete_spec,
        telemetry,
        allow_container,
        features: selected,
        json,
    } = args;

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let mut features = workflow.list_features()?;
    features.retain(|feature| feature.status != FeatureStatus::Removed);
    if !selected.is_empty() {
        let registry = workflow.repo_root().join(".branchbox/registry.json");
        if let Some(unknown) = selected.iter().find(|name| {
            !features
                .iter()
                .any(|feature| &feature.work_feature == *name)
        }) {
            return Err(json_error::CliError::new(
                "feature_not_found",
                format!(
                    "Feature '{unknown}' is not an active or retained feature in {}; nothing was \
                     pruned",
                    registry.display()
                ),
            )
            .with_details(json!({
                "name": unknown,
                "registry": registry.display().to_string(),
            }))
            .into());
        }
        features.retain(|feature| selected.contains(&feature.work_feature));
    }

    let config = BranchBoxConfig::load(workflow.repo_root()).unwrap_or_default();
    let should_delete_branch = if keep_branch {
        false
    } else if delete_branch {
        true
    } else {
        config.feature.teardown.delete_branch_by_default
    };
    let policy = PrunePolicy {
        delete_branch: should_delete_branch,
        force_delete_branch: should_delete_branch,
        discard_changes: true,
        complete_spec,
    };

    let candidates: Vec<PruneCandidate> = features
        .iter()
        .map(|feature| {
            let plan =
                workflow.plan_teardown(&policy.request(feature, telemetry), &policy.options());
            PruneCandidate {
                work_feature: feature.work_feature.clone(),
                status: feature.status.clone(),
                branch_name: feature.branch_name.clone(),
                worktree_path: feature.worktree_path.clone(),
                plan_error: plan.as_ref().err().map(ToString::to_string),
                plan: plan.ok(),
            }
        })
        .collect();

    if candidates.is_empty() {
        humanln!("ℹ️  No active or retained features to prune.");
    } else {
        humanln!(
            "🧹 Preparing to prune {} active or retained feature worktree(s):",
            candidates.len()
        );
        for candidate in &candidates {
            let losses = candidate.losses();
            if losses.is_empty() {
                humanln!("  - {}", candidate.work_feature);
            } else {
                humanln!("  - {} ({})", candidate.work_feature, losses.join("; "));
            }
        }
    }

    if dry_run {
        if json {
            output::emit_json(&PruneDryRun {
                schema_version: PRUNE_SCHEMA_VERSION,
                dry_run: true,
                policy,
                at_risk: prune_at_risk(&candidates),
                candidates: &candidates,
            })?;
        } else if !candidates.is_empty() {
            humanln!("ℹ️  Dry run only; no teardown executed.");
        }
        return Ok(());
    }

    if candidates.is_empty() {
        if json {
            output::emit_json(&PruneReport {
                schema_version: PRUNE_SCHEMA_VERSION,
                dry_run: false,
                results: Vec::new(),
                pruned: 0,
                failed: 0,
            })?;
        }
        return Ok(());
    }

    if !yes {
        if !output::is_interactive() {
            return Err(json_error::CliError::confirmation_required(
                candidates.len(),
                "Refusing to prune in non-interactive mode without --yes. Rerun with --yes to confirm.",
            )
            .into());
        }

        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!(
                "Tear down all {} active features now?",
                candidates.len()
            ))
            .default(false)
            .interact()?;
        if !proceed {
            bail!("Prune aborted.");
        }
    }

    let _host_validation_override = HostValidationOverride::new(allow_container);
    workflow.validate_host()?;
    let mut results = Vec::new();
    let total = candidates.len();

    for feature in &features {
        humanln!();
        humanln!("→ Pruning {}", feature.work_feature);

        match workflow.teardown_with_options(policy.request(feature, telemetry), policy.options()) {
            Ok(summary) => {
                print_teardown_summary(&summary);
                results.push(PruneOutcome {
                    work_feature: feature.work_feature.clone(),
                    outcome: "removed",
                    summary: Some(summary),
                    error: None,
                });
            }
            Err(err) => {
                eprintln!("✗ Failed to prune '{}': {}", feature.work_feature, err);
                results.push(PruneOutcome {
                    work_feature: feature.work_feature.clone(),
                    outcome: "failed",
                    summary: None,
                    error: Some(json!({"code": err.code(), "message": err.to_string()})),
                });
            }
        }
    }

    let failures: Vec<&PruneOutcome> = results
        .iter()
        .filter(|result| result.outcome == "failed")
        .collect();
    let failed = failures.len();
    humanln!();
    if failed == 0 {
        humanln!("✓ Pruned {} feature(s).", total);
    } else {
        eprintln!(
            "Completed prune with failures ({}/{} failed):",
            failed, total
        );
        for failure in &failures {
            let reason = failure
                .error
                .as_ref()
                .and_then(|error| error["message"].as_str())
                .unwrap_or_default();
            eprintln!("  - {}: {}", failure.work_feature, reason);
        }
    }

    if json {
        // In-band failure (DESIGN §5.2 rule 4): the report is the document, even with failures.
        output::emit_json(&PruneReport {
            schema_version: PRUNE_SCHEMA_VERSION,
            dry_run: false,
            pruned: total - failed,
            failed,
            results,
        })?;
    }
    if failed > 0 {
        bail!("Prune completed with failures.");
    }
    Ok(())
}

fn print_start_summary(
    summary: &StartSummary,
    json_output: bool,
    suppress_summary: bool,
) -> Result<()> {
    let prompt_bridge_enabled = env_flag("BRANCHBOX_ENABLE_PROMPT_BRIDGE");
    let agent_config = AgentLaunchConfig::from_env();
    let agent_plan = determine_agent_plan(
        devcontainer_status_from_summary(summary),
        agent_config.as_ref(),
    );

    if json_output {
        let module_outcomes_json: Vec<_> = summary
            .module_outcomes
            .iter()
            .map(|outcome| {
                json!({
                    "module": outcome.module,
                    "status": outcome.status.to_string(),
                    "duration_ms": outcome.duration_ms,
                    "notes": outcome.notes,
                    "forced": outcome.forced,
                })
            })
            .collect();

        let skipped_modules_json: Vec<_> = summary
            .skipped_modules
            .iter()
            .map(|record| {
                json!({
                    "module": record.name,
                    "reason": record.reason.description(),
                })
            })
            .collect();

        let adapter_json = summary.adapter.as_ref().map(|adapter| {
            json!({
                "name": adapter.name,
                "service_url": adapter.service_url,
                "warnings": adapter.warnings,
            })
        });

        let tunnel_json = match summary.tunnel.as_ref() {
            Some(tunnel) => Some(serde_json::to_value(tunnel)?),
            None => None,
        };

        let default_agent_json = serde_json::to_value(&agent_plan)?;
        let payload = json!({
            "work_feature": summary.work_feature,
            "branch_name": summary.branch_name,
            "worktree_path": summary.worktree_path.display().to_string(),
            "mode": summary.mode.to_string(),
            "prompt_seed": summary.prompt_seed.as_ref(),
            "feature_url": summary.feature_url.as_ref(),
            "compose_project_name": summary.compose_project_name.as_ref(),
            "runtime": &summary.runtime,
            "env_path": summary.env_path.as_ref().map(|path| path.display().to_string()),
            "color": summary.color.as_ref(),
            "module_outcomes": module_outcomes_json,
            "skipped_modules": skipped_modules_json,
            "warnings": &summary.warnings,
            "adapter": adapter_json,
            "tunnel": tunnel_json,
            "prompt_bridge_enabled": prompt_bridge_enabled,
            "generated_at": summary.generated_at.to_rfc3339(),
            "default_agent": default_agent_json,
        });

        output::emit_json(&payload)?;
        return Ok(());
    }

    humanln!("🚀 Feature workspace ready ({})", summary.mode);
    humanln!("  Feature: {}", summary.work_feature);
    humanln!();
    print_start_checklist(summary, prompt_bridge_enabled, &agent_plan);

    if let Some(adapter) = summary.adapter.as_ref() {
        if !adapter.warnings.is_empty() {
            humanln!();
            humanln!("Adapter warnings:");
            for warning in &adapter.warnings {
                humanln!("  ⚠ {}", warning);
            }
        }
    }

    if !suppress_summary {
        if !summary.module_outcomes.is_empty() {
            humanln!();
            print_module_outcome_table(&summary.module_outcomes);
        }

        if !summary.skipped_modules.is_empty() {
            humanln!();
            humanln!("Skipped modules:");
            for record in &summary.skipped_modules {
                humanln!("  - {} ({})", record.name, record.reason.description());
            }
        }

        if summary.mode == StartMode::Minimal && !summary.skipped_modules.is_empty() {
            humanln!();
            humanln!(
                "Next: run `branchbox devcontainer sync` or targeted module commands when you're ready to fully provision."
            );
        }

        if !summary.warnings.is_empty() {
            humanln!();
            humanln!("Warnings:");
            for warning in &summary.warnings {
                humanln!("  - {}", warning);
            }
        }
    }

    maybe_launch_default_agent(summary, &agent_plan, json_output);

    Ok(())
}

fn print_start_checklist(
    summary: &StartSummary,
    prompt_bridge_enabled: bool,
    agent_plan: &AgentPlan,
) {
    let mut rows = Vec::new();

    rows.push(ChecklistRow::new(
        "Worktree",
        "✅",
        "ready",
        summary.worktree_path.display().to_string(),
    ));
    rows.push(ChecklistRow::new(
        "Branch",
        "✅",
        "ready",
        summary.branch_name.clone(),
    ));
    rows.push(ChecklistRow::new(
        "Runtime",
        "✅",
        "ready",
        summary.runtime.provider.to_string(),
    ));

    if let Some(color) = summary.color.as_ref() {
        rows.push(ChecklistRow::new(
            "Workspace color",
            "✅",
            "applied",
            color.clone(),
        ));
    }

    rows.push(build_adapter_row(summary));
    rows.push(build_feature_url_row(summary));
    rows.push(build_compose_row(summary));
    rows.push(build_env_row(summary));
    rows.push(build_prompt_row(summary, prompt_bridge_enabled));
    rows.push(build_tunnel_row(summary));
    rows.push(build_modules_row(summary));

    if let Some(skipped) = build_skipped_modules_detail(&summary.skipped_modules) {
        rows.push(ChecklistRow::new(
            "Skipped modules",
            "⏭",
            "recorded",
            skipped,
        ));
    }

    rows.push(build_agent_row(agent_plan));

    render_checklist(&rows);
}

fn build_adapter_row(summary: &StartSummary) -> ChecklistRow {
    match summary.adapter.as_ref() {
        Some(adapter) => {
            let mut details = adapter.name.clone();
            if !adapter.service_url.is_empty() {
                details.push_str(&format!(" · {}", adapter.service_url));
            }
            ChecklistRow::new("Adapter", "✅", "detected", details)
        }
        None => ChecklistRow::new(
            "Adapter",
            "ℹ️",
            "generic",
            "No stack-specific adapter detected; using generic workflow",
        ),
    }
}

fn build_feature_url_row(summary: &StartSummary) -> ChecklistRow {
    match summary.feature_url.as_ref() {
        Some(url) => ChecklistRow::new("Feature URL", "✅", "ready", format!("https://{}", url)),
        None => ChecklistRow::new(
            "Feature URL",
            "…",
            "not set",
            "Populate APP_URL in the main .env to pre-seed feature URLs",
        ),
    }
}

fn build_compose_row(summary: &StartSummary) -> ChecklistRow {
    match summary.compose_project_name.as_ref() {
        Some(name) => ChecklistRow::new("Compose project", "✅", "isolated", name.clone()),
        None => ChecklistRow::new(
            "Compose project",
            "ℹ️",
            "default",
            "Compose isolation not required for this stack",
        ),
    }
}

fn build_env_row(summary: &StartSummary) -> ChecklistRow {
    match summary.env_path.as_ref() {
        Some(path) => ChecklistRow::new(".env", "✅", "copied", path.display().to_string()),
        None => ChecklistRow::new(
            ".env",
            "⚠️",
            "missing",
            "Source .env not found; workspace kept untouched",
        ),
    }
}

fn build_prompt_row(summary: &StartSummary, prompt_bridge_enabled: bool) -> ChecklistRow {
    match summary.prompt_seed.as_ref() {
        Some(seed) => {
            let len = seed.chars().count();
            let bridge = if prompt_bridge_enabled {
                "bridge enabled"
            } else {
                "bridge disabled"
            };
            ChecklistRow::new(
                "Prompt seed",
                "✅",
                "stored",
                format!("{len} chars ({bridge})"),
            )
        }
        None => ChecklistRow::new(
            "Prompt seed",
            "…",
            "not set",
            "Add --prompt or --default-prompt to capture agent context",
        ),
    }
}

fn build_tunnel_row(summary: &StartSummary) -> ChecklistRow {
    match summary.tunnel.as_ref() {
        Some(state) => {
            let mut detail = state.provider.clone();
            // Show routing info: hostname → service_url
            match (state.hostname.as_ref(), state.service_url.as_ref()) {
                (Some(host), Some(svc)) => {
                    detail = format!("{detail} ({host} → {svc})");
                }
                (Some(host), None) => {
                    detail = format!("{detail} ({host})");
                }
                _ => {}
            }
            match state.status {
                FeatureTunnelStatus::Active => ChecklistRow::new("Tunnel", "✅", "online", detail),
                FeatureTunnelStatus::Pending => ChecklistRow::new("Tunnel", "…", "pending", detail),
                FeatureTunnelStatus::Manual => ChecklistRow::new("Tunnel", "⚠️", "manual", detail),
                FeatureTunnelStatus::Disabled => {
                    ChecklistRow::new("Tunnel", "⏭", "disabled", detail)
                }
            }
        }
        None => ChecklistRow::new(
            "Tunnel",
            "ℹ️",
            "not configured",
            "Enable the tunnel module to sync review links",
        ),
    }
}

fn build_modules_row(summary: &StartSummary) -> ChecklistRow {
    if summary.module_outcomes.is_empty() {
        return ChecklistRow::new("Modules", "…", "pending", "Module detection pending");
    }

    let mut success = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for outcome in &summary.module_outcomes {
        match outcome.status {
            ModuleStatus::Success => success += 1,
            ModuleStatus::Skipped => skipped += 1,
            ModuleStatus::Failed => failed += 1,
        }
    }

    let mut parts = Vec::new();
    if success > 0 {
        parts.push(format!("{success} ok"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skip"));
    }
    if failed > 0 {
        parts.push(format!("{failed} fail"));
    }

    let detail = if parts.is_empty() {
        "No modules detected".to_string()
    } else {
        parts.join(" / ")
    };

    if failed > 0 {
        ChecklistRow::new("Modules", "❌", "failed", detail)
    } else if success == 0 && skipped > 0 {
        ChecklistRow::new("Modules", "⏭", "skipped", detail)
    } else if skipped > 0 {
        ChecklistRow::new("Modules", "⚠️", "partial", detail)
    } else {
        ChecklistRow::new("Modules", "✅", "ready", detail)
    }
}

fn build_skipped_modules_detail(records: &[ModuleSkipRecord]) -> Option<String> {
    if records.is_empty() {
        return None;
    }

    let descriptions: Vec<String> = records
        .iter()
        .map(|record| format!("{} ({})", record.name, record.reason.description()))
        .collect();

    Some(descriptions.join(", "))
}

fn build_agent_row(plan: &AgentPlan) -> ChecklistRow {
    let (icon, label) = match plan.status {
        AgentPlanStatus::Disabled => ("⏭", "disabled"),
        AgentPlanStatus::Waiting => ("…", "waiting"),
        AgentPlanStatus::Blocked => ("⚠️", "blocked"),
        AgentPlanStatus::Ready => ("✅", "ready"),
    };

    ChecklistRow::new("Default agent", icon, label, plan.detail.clone())
}

fn render_checklist(rows: &[ChecklistRow]) {
    if rows.is_empty() {
        return;
    }

    let mut step_w = "Step".len();
    let mut result_w = "Result".len();
    let mut detail_w = "Details".len();

    for row in rows {
        step_w = step_w.max(row.step.len());
        result_w = result_w.max(row.result.len());
        detail_w = detail_w.max(row.details.len());
    }

    let border = format!(
        "+-{step}-+-{result}-+-{detail}-+",
        step = "-".repeat(step_w),
        result = "-".repeat(result_w),
        detail = "-".repeat(detail_w),
    );

    humanln!("{border}");
    humanln!(
        "| {step:<step_w$} | {result:<result_w$} | {detail:<detail_w$} |",
        step = "Step",
        result = "Result",
        detail = "Details",
        step_w = step_w,
        result_w = result_w,
        detail_w = detail_w,
    );
    humanln!("{border}");

    for row in rows {
        humanln!(
            "| {step:<step_w$} | {result:<result_w$} | {detail:<detail_w$} |",
            step = row.step,
            result = row.result,
            detail = row.details,
            step_w = step_w,
            result_w = result_w,
            detail_w = detail_w,
        );
    }

    humanln!("{border}");
}

fn print_teardown_summary(summary: &TeardownSummary) {
    humanln!("🧹 Feature teardown finished");
    humanln!(
        "  Worktree removed: {}",
        if summary.worktree_removed {
            "yes"
        } else {
            "no"
        }
    );
    humanln!(
        "  Branch deleted: {}",
        if summary.branch_deleted { "yes" } else { "no" }
    );
    humanln!(
        "  Runtime cleanup verified: {} (residue free: {})",
        if summary.runtime_teardown.verified {
            "yes"
        } else {
            "no"
        },
        if summary.runtime_teardown.residue_free {
            "yes"
        } else {
            "no"
        }
    );
    if !summary.branch_deleted {
        humanln!(
            "  Branch kept: {} (delete manually with `git branch -d {}` or `git branch -D {}`)",
            summary.branch_name,
            summary.branch_name,
            summary.branch_name
        );
    }
    if let Some(error) = &summary.branch_delete_error {
        humanln!("  Branch delete failed: {}", error);
    }
    if !summary.discarded_changes.is_empty() {
        let names: Vec<&str> = summary
            .discarded_changes
            .iter()
            .take(REFUSAL_LISTED_FILES)
            .map(|change| change.path.as_str())
            .collect();
        humanln!(
            "  Discarded changes: {} ({}{})",
            summary.discarded_changes.len(),
            names.join(", "),
            if summary.discarded_changes.len() > names.len() {
                ", …"
            } else {
                ""
            }
        );
    }
    for file in &summary.preserved {
        humanln!(
            "  Kept: {} → {} in the main worktree",
            file.path,
            file.destination
        );
    }
    if !summary.adapter_cleanup_warnings.is_empty() {
        humanln!();
        humanln!("Adapter:");
        for warning in &summary.adapter_cleanup_warnings {
            humanln!("  ⚠ {}", warning);
        }
    }

    if !summary.module_reports.is_empty() {
        humanln!();
        humanln!("Modules:");
        for report in &summary.module_reports {
            let status = if report.teardown_ok { "ok" } else { "warn" };
            humanln!("  - {} ({})", report.name, status);
            for error in &report.errors {
                humanln!("      • {}", error);
            }
        }
    }

    if !summary.warnings.is_empty() {
        humanln!();
        humanln!("Warnings:");
        for warning in &summary.warnings {
            humanln!("  - {}", warning);
        }
    }
}

fn print_module_outcome_table(outcomes: &[ModuleOutcome]) {
    let mut rows: Vec<(String, String, String, String)> = Vec::with_capacity(outcomes.len());
    let mut name_w = "Module".len();
    let mut status_w = "Status".len();
    let mut duration_w = "Duration".len();
    let mut notes_w = "Notes".len();

    for outcome in outcomes {
        let mut status = outcome.status.to_string();
        if outcome.forced {
            status.push('*');
        }
        let duration = if outcome.status == ModuleStatus::Skipped {
            "—".to_string()
        } else {
            format_duration(outcome.duration_ms)
        };
        let notes = if outcome.notes.is_empty() {
            String::new()
        } else {
            outcome.notes.join("; ")
        };

        name_w = name_w.max(outcome.module.len());
        status_w = status_w.max(status.len());
        duration_w = duration_w.max(duration.len());
        notes_w = notes_w.max(notes.len());

        rows.push((outcome.module.clone(), status, duration, notes));
    }

    let notes_w = notes_w.max(1);

    let border = format!(
        "+-{name}-+-{status}-+-{duration}-+-{notes}-+",
        name = "-".repeat(name_w),
        status = "-".repeat(status_w),
        duration = "-".repeat(duration_w),
        notes = "-".repeat(notes_w),
    );
    let header = format!(
        "| {name:<name_w$} | {status:<status_w$} | {duration:<duration_w$} | {notes:<notes_w$} |",
        name = "Module",
        status = "Status",
        duration = "Duration",
        notes = "Notes",
        name_w = name_w,
        status_w = status_w,
        duration_w = duration_w,
        notes_w = notes_w,
    );

    humanln!("{border}");
    humanln!("{header}");
    humanln!("{border}");

    for (name, status, duration, notes) in rows {
        humanln!(
            "| {name:<name_w$} | {status:<status_w$} | {duration:<duration_w$} | {notes:<notes_w$} |",
            name = name,
            status = status,
            duration = duration,
            notes = notes,
            name_w = name_w,
            status_w = status_w,
            duration_w = duration_w,
            notes_w = notes_w,
        );
    }

    humanln!("{border}");
    if outcomes.iter().any(|outcome| outcome.forced) {
        humanln!("(*) Forced module executed due to policy requirements.");
    }
}

#[derive(Debug)]
struct ChecklistRow {
    step: String,
    result: String,
    details: String,
}

impl ChecklistRow {
    fn new(
        step: impl Into<String>,
        icon: &str,
        label: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        let label = label.into();
        let result = if label.is_empty() {
            icon.to_string()
        } else {
            format!("{icon} {label}")
        };

        Self {
            step: step.into(),
            result,
            details: details.into(),
        }
    }
}

#[derive(Clone, Debug)]
struct AgentLaunchConfig {
    command: String,
    label: Option<String>,
}

impl AgentLaunchConfig {
    fn from_env() -> Option<Self> {
        let command = env::var("BRANCHBOX_DEFAULT_AGENT_CMD")
            .ok()?
            .trim()
            .to_string();
        if command.is_empty() {
            return None;
        }

        let label = env::var("BRANCHBOX_DEFAULT_AGENT_NAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Some(Self { command, label })
    }

    fn display_label(&self) -> &str {
        self.label.as_deref().unwrap_or("default coding agent")
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum AgentPlanStatus {
    Ready,
    Waiting,
    Blocked,
    Disabled,
}

impl AgentPlanStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AgentPlanStatus::Ready => "ready",
            AgentPlanStatus::Waiting => "waiting",
            AgentPlanStatus::Blocked => "blocked",
            AgentPlanStatus::Disabled => "disabled",
        }
    }
}

impl fmt::Display for AgentPlanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Serialize)]
struct AgentPlan {
    status: AgentPlanStatus,
    label: Option<String>,
    command: Option<String>,
    detail: String,
    followup: Option<String>,
}

impl AgentPlan {
    fn disabled() -> Self {
        Self {
            status: AgentPlanStatus::Disabled,
            label: None,
            command: None,
            detail: "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent"
                .to_string(),
            followup: None,
        }
    }

    fn ready(config: &AgentLaunchConfig) -> Self {
        Self {
            status: AgentPlanStatus::Ready,
            label: Some(config.display_label().to_string()),
            command: Some(config.command.clone()),
            detail: format!(
                "Will launch {} via `{}`",
                config.display_label(),
                config.command
            ),
            followup: None,
        }
    }

    fn waiting(
        detail: impl Into<String>,
        followup: impl Into<String>,
        config: &AgentLaunchConfig,
    ) -> Self {
        Self {
            status: AgentPlanStatus::Waiting,
            label: Some(config.display_label().to_string()),
            command: Some(config.command.clone()),
            detail: detail.into(),
            followup: Some(followup.into()),
        }
    }

    fn blocked(
        detail: impl Into<String>,
        followup: impl Into<String>,
        config: &AgentLaunchConfig,
    ) -> Self {
        Self {
            status: AgentPlanStatus::Blocked,
            label: Some(config.display_label().to_string()),
            command: Some(config.command.clone()),
            detail: detail.into(),
            followup: Some(followup.into()),
        }
    }
}

fn determine_agent_plan(
    dev_status: Option<ModuleStatus>,
    config: Option<&AgentLaunchConfig>,
) -> AgentPlan {
    let Some(config) = config else {
        return AgentPlan::disabled();
    };

    match dev_status {
        Some(ModuleStatus::Success) => AgentPlan::ready(config),
        Some(ModuleStatus::Failed) => AgentPlan::blocked(
            "Devcontainer failed; fix provisioning before auto-launching",
            "⚠️  Skipping default coding agent launch because the devcontainer module failed.",
            config,
        ),
        Some(ModuleStatus::Skipped) => AgentPlan::waiting(
            "Devcontainer skipped (minimal mode); run `branchbox devcontainer sync` first",
            "ℹ️  Default coding agent launch skipped (devcontainer not provisioned yet). Run `branchbox devcontainer sync` first.",
            config,
        ),
        None => AgentPlan::waiting(
            "Devcontainer module not detected; launch deferred",
            "ℹ️  Default coding agent launch skipped (devcontainer module not detected).",
            config,
        ),
    }
}

fn maybe_launch_default_agent(summary: &StartSummary, plan: &AgentPlan, json_output: bool) {
    if json_output {
        return;
    }

    match plan.status {
        AgentPlanStatus::Ready => {
            let Some(command) = plan.command.as_ref() else {
                return;
            };
            let label = plan.label.as_deref().unwrap_or("default coding agent");
            humanln!();
            humanln!(
                "🤖 Launching {} via `{}` (cwd: {})",
                label,
                command,
                summary.worktree_path.display()
            );
            match launch_agent_process(command, summary) {
                Ok(()) => humanln!("✅ Agent session completed successfully."),
                Err(err) => humanln!("⚠️  Default coding agent command failed: {err}"),
            }
        }
        AgentPlanStatus::Waiting | AgentPlanStatus::Blocked => {
            if let Some(message) = plan.followup.as_ref() {
                humanln!();
                humanln!("{message}");
            }
        }
        AgentPlanStatus::Disabled => {}
    }
}

macro_rules! devcontainer_status {
    ($outcomes:expr) => {
        $outcomes
            .iter()
            .find(|outcome| outcome.module.eq_ignore_ascii_case("devcontainer"))
            .map(|outcome| outcome.status)
    };
}

fn devcontainer_status_from_summary(summary: &StartSummary) -> Option<ModuleStatus> {
    devcontainer_status!(&summary.module_outcomes)
}

fn devcontainer_status_from_metadata(feature: &FeatureMetadata) -> Option<ModuleStatus> {
    devcontainer_status!(&feature.module_outcomes)
}

fn launch_agent_process(command_line: &str, summary: &StartSummary) -> Result<()> {
    let parts = split_command_line(command_line)
        .map_err(|err| anyhow!("failed to parse BRANCHBOX_DEFAULT_AGENT_CMD: {}", err))?;

    if parts.is_empty() {
        bail!("BRANCHBOX_DEFAULT_AGENT_CMD must include an executable name");
    }

    let provider = runtime::provider(summary.runtime.provider)?;
    let exit_code = provider.exec_interactive(&summary.runtime, &summary.worktree_path, &parts)?;

    if exit_code == 0 {
        Ok(())
    } else {
        Err(anyhow!("command exited with status {}", exit_code))
    }
}

fn format_duration(duration_ms: u64) -> String {
    if duration_ms == 0 {
        "0.00s".to_string()
    } else {
        format!("{:.2}s", duration_ms as f64 / 1000.0)
    }
}

fn env_flag(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}
