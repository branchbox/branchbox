use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use clap::{Args, Subcommand};
use dialoguer::{console::Term, theme::ColorfulTheme, Confirm};
use serde::Serialize;
use serde_json::json;
use shell_words::split as split_command_line;
use std::{
    env, fmt,
    path::{Path, PathBuf},
    process::Command,
};
use worktree_core::{
    config::BranchBoxConfig,
    workflows::feature::{
        FeatureMetadata, FeatureStatus, FeatureTunnelStatus, FeatureWorkflow, ModuleOutcome,
        ModuleOutcomeRecord, ModuleSkipRecord, ModuleStatus, StartMode, StartRequest, StartSummary,
        TeardownRequest, TeardownSummary,
    },
    Error as CoreError,
};

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

    /// Suppress summary output (text mode only)
    #[arg(long = "no-summary")]
    pub no_summary: bool,
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

    /// Force removal even with local changes
    #[arg(long)]
    pub force: bool,

    /// Force-delete the git branch even if it is not fully merged (`git branch -D`)
    #[arg(long)]
    pub force_delete_branch: bool,

    /// Move spec to completed during teardown
    #[arg(long)]
    pub complete_spec: bool,

    /// Emit verbose telemetry (e.g. Cloudflare operations)
    #[arg(long)]
    pub telemetry: bool,
}

#[derive(Args)]
pub struct FeatureListArgs {
    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Filter by status (active, removed)
    #[arg(long)]
    pub status: Option<String>,

    /// Include removed features even if --status is not provided
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
}

pub fn execute(command: FeatureCommands) -> Result<()> {
    match command {
        FeatureCommands::Start(args) => run_start(args),
        FeatureCommands::Teardown(args) => run_teardown(args),
        FeatureCommands::List(args) => run_list(args),
        FeatureCommands::Prune(args) => run_prune(args),
    }
}

fn run_start(args: FeatureStartArgs) -> Result<()> {
    let FeatureStartArgs {
        name,
        title,
        base,
        branch_prefix,
        repo,
        reuse,
        telemetry,
        skip_modules,
        minimal,
        fast,
        prompt,
        default_prompt,
        json,
        no_summary,
    } = args;

    let mode = if minimal || fast {
        StartMode::Minimal
    } else {
        StartMode::Full
    };

    if default_prompt && mode != StartMode::Minimal {
        bail!("--default-prompt can only be used with --minimal or --fast");
    }

    const PROMPT_MAX_CHARS: usize = 2000;
    let mut prompt_seed = prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(ref mut seed) = prompt_seed {
        let len = seed.chars().count();
        if len > PROMPT_MAX_CHARS {
            let truncated: String = seed.chars().take(PROMPT_MAX_CHARS).collect();
            println!("⚠️  Prompt truncated to {PROMPT_MAX_CHARS} characters before storage.");
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
        reuse_existing: reuse,
        telemetry,
        skip_modules,
        mode,
        prompt_seed: prompt_seed.clone(),
    };

    let summary = workflow.start(request)?;
    print_start_summary(&summary, json, no_summary)?;

    Ok(())
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
    let removed_count = total_count.saturating_sub(active_count);

    if let Some(status_filter) = status.as_ref() {
        let parsed: FeatureStatus = status_filter.parse()?;
        features.retain(|feature| feature.status == parsed);
    } else if !all {
        features.retain(|feature| feature.status == FeatureStatus::Active);
    }

    if total_count == 0 {
        if json {
            println!("[]");
        } else {
            println!("ℹ️  No features tracked yet. Run `branchbox feature start` to create one.");
        }
        return Ok(());
    }

    if features.is_empty() {
        if json {
            println!("[]");
        } else if let Some(filter) = status.as_ref() {
            println!(
                "ℹ️  No features found with status '{}'.",
                filter.to_ascii_lowercase()
            );
        } else if !all {
            println!(
                "ℹ️  No active features. Use `branchbox feature list --all` to include removed entries."
            );
        } else {
            println!("ℹ️  No features match the requested filters.");
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
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let showing_count = entries.len();
    println!(
        "📚 Feature registry — {} active · {} removed (showing {}/{})",
        active_count, removed_count, showing_count, total_count
    );

    let headers = [
        "Feature",
        "Status",
        "Mode",
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
    println!("{}", header_line);

    let separator = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{}", separator);

    for row in rows {
        let line = row
            .iter()
            .enumerate()
            .map(|(idx, cell)| format!("{:<width$}", cell, width = widths[idx]))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{}", line);
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
        complete_spec,
        telemetry,
    } = args;

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let config = BranchBoxConfig::load(workflow.repo_root()).unwrap_or_default();
    let default_delete_branch = config.feature.teardown.delete_branch_by_default;

    let delete_branch = if keep_branch {
        false
    } else if delete_branch {
        true
    } else {
        default_delete_branch
    };
    let mut request = TeardownRequest {
        work_feature: name,
        branch_prefix: branch_prefix.or_else(|| Some(config.feature.branch_prefix.clone())),
        delete_branch,
        force_delete_branch,
        force_remove: force,
        force_remove_modules: force,
        complete_spec,
        telemetry,
    };

    // Only prompt on interactive shells; in non-interactive contexts we surface a failure if the
    // branch cannot be deleted without force so callers can retry with `--force`.
    if Term::stdout().is_term() {
        maybe_prompt_force_delete_branch(&repo_path, &config, &mut request)?;
    }

    let summary = match workflow.teardown(request.clone()) {
        Ok(summary) => summary,
        Err(err) => handle_teardown_error(err, &workflow, &mut request)?,
    };

    if !Term::stdout().is_term() && request.delete_branch && !summary.branch_deleted {
        let branch_name =
            build_branch_name(request.branch_prefix.as_deref(), &request.work_feature);
        if local_branch_exists(&repo_path, &branch_name)
            && !request.force_remove
            && !request.force_delete_branch
        {
            bail!(
                "Branch '{}' could not be deleted without force; rerun with `--force-delete-branch` (or `--force`).",
                branch_name
            );
        }
    }

    print_teardown_summary(&summary);

    Ok(())
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
    } = args;

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let mut features = workflow.list_features()?;
    features.retain(|feature| feature.status == FeatureStatus::Active);

    if features.is_empty() {
        println!("ℹ️  No active features to prune.");
        return Ok(());
    }

    println!(
        "🧹 Preparing to prune {} active feature worktree(s):",
        features.len()
    );
    for feature in &features {
        println!("  - {}", feature.work_feature);
    }

    if dry_run {
        println!("ℹ️  Dry run only; no teardown executed.");
        return Ok(());
    }

    if !yes {
        if !Term::stdout().is_term() {
            bail!(
                "Refusing to prune in non-interactive mode without --yes. Rerun with --yes to confirm."
            );
        }

        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!(
                "Tear down all {} active features now?",
                features.len()
            ))
            .default(false)
            .interact()?;
        if !proceed {
            bail!("Prune aborted.");
        }
    }

    let config = BranchBoxConfig::load(workflow.repo_root()).unwrap_or_default();
    let default_delete_branch = config.feature.teardown.delete_branch_by_default;
    let should_delete_branch = if keep_branch {
        false
    } else if delete_branch {
        true
    } else {
        default_delete_branch
    };

    let mut failed = Vec::new();
    let total = features.len();

    for feature in features {
        println!();
        println!("→ Pruning {}", feature.work_feature);

        let request = TeardownRequest {
            work_feature: feature.work_feature.clone(),
            branch_prefix: derive_branch_prefix(&feature),
            delete_branch: should_delete_branch,
            force_delete_branch: should_delete_branch,
            force_remove: true,
            force_remove_modules: true,
            complete_spec,
            telemetry,
        };

        match workflow.teardown(request) {
            Ok(summary) => print_teardown_summary(&summary),
            Err(err) => {
                eprintln!("✗ Failed to prune '{}': {}", feature.work_feature, err);
                failed.push((feature.work_feature, err.to_string()));
            }
        }
    }

    println!();
    if failed.is_empty() {
        println!("✓ Pruned {} feature(s).", total);
        return Ok(());
    }

    eprintln!(
        "Completed prune with failures ({}/{} failed):",
        failed.len(),
        total
    );
    for (feature, reason) in failed {
        eprintln!("  - {}: {}", feature, reason);
    }
    bail!("Prune completed with failures.")
}

fn handle_teardown_error(
    err: CoreError,
    workflow: &FeatureWorkflow,
    request: &mut TeardownRequest,
) -> Result<TeardownSummary> {
    match err {
        CoreError::WorktreeDirty { worktree, files } => {
            handle_dirty_worktree(workflow, request, &worktree, &files)
        }
        other => Err(other.into()),
    }
}

fn handle_dirty_worktree(
    workflow: &FeatureWorkflow,
    request: &mut TeardownRequest,
    worktree: &Path,
    files: &[String],
) -> Result<TeardownSummary> {
    println!(
        "⚠️  Detected devcontainer/compose changes inside {}:",
        worktree.display()
    );
    for entry in files {
        println!("    • {}", entry);
    }
    println!("    (BranchBox refuses to delete dirty module files without --force)");

    if !Term::stdout().is_term() {
        anyhow::bail!(
            "Devcontainer/compose changes detected; rerun this command with --force to proceed."
        );
    }

    let proceed = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Continue teardown with --force? This will discard local module changes.")
        .default(false)
        .interact()?;

    if !proceed {
        anyhow::bail!("Teardown aborted; rerun with --force to skip this prompt.");
    }

    request.force_remove = true;
    request.force_remove_modules = true;
    Ok(workflow.teardown(request.clone())?)
}

fn build_branch_name(prefix: Option<&str>, work_feature: &str) -> String {
    let prefix = prefix.unwrap_or("feature").trim_end_matches('/');
    if prefix.is_empty() {
        work_feature.to_string()
    } else {
        format!("{}/{}", prefix, work_feature)
    }
}

fn derive_branch_prefix(feature: &FeatureMetadata) -> Option<String> {
    let suffix = format!("/{}", feature.work_feature);
    if let Some(prefix) = feature.branch_name.strip_suffix(&suffix) {
        return Some(prefix.to_string());
    }

    if feature.branch_name == feature.work_feature {
        return Some(String::new());
    }

    None
}

fn local_branch_exists(repo_root: &Path, branch_name: &str) -> bool {
    let ref_name = format!("refs/heads/{}", branch_name);
    Command::new("git")
        .current_dir(repo_root)
        .args(["show-ref", "--verify", "--quiet", &ref_name])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn local_branch_is_merged(repo_root: &Path, branch_name: &str) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["branch", "--merged"])
        .output()
        .context("failed to run `git branch --merged`")?;

    if !output.status.success() {
        bail!(
            "git branch --merged failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let merged = stdout.lines().any(|line| {
        let normalized = line.trim().trim_start_matches('*').trim();
        normalized == branch_name
    });
    Ok(merged)
}

fn maybe_prompt_force_delete_branch(
    repo_path: &Path,
    config: &BranchBoxConfig,
    request: &mut TeardownRequest,
) -> Result<()> {
    if !request.delete_branch || request.force_delete_branch {
        return Ok(());
    }

    let force_by_default = config.feature.teardown.force_delete_unmerged_by_default;
    let prompt = config.feature.teardown.prompt_force_delete_unmerged;

    if !Term::stdout().is_term() {
        // Non-interactive: we cannot prompt, so only force-delete when explicitly allowed.
        let branch_name =
            build_branch_name(request.branch_prefix.as_deref(), &request.work_feature);
        if local_branch_exists(repo_path, &branch_name)
            && !local_branch_is_merged(repo_path, &branch_name)?
        {
            if request.force_remove || force_by_default {
                request.force_delete_branch = true;
                return Ok(());
            }

            bail!(
                "Branch '{}' is not fully merged; rerun with `--force-delete-branch` (or `--force`) to delete it.",
                branch_name
            );
        }
        return Ok(());
    }

    let branch_name = build_branch_name(request.branch_prefix.as_deref(), &request.work_feature);
    if !local_branch_exists(repo_path, &branch_name) {
        return Ok(());
    }

    if local_branch_is_merged(repo_path, &branch_name)? {
        return Ok(());
    }

    if force_by_default {
        request.force_delete_branch = true;
        return Ok(());
    }

    if !prompt {
        request.delete_branch = false;
        println!("ℹ️  Keeping branch '{}' (not fully merged).", branch_name);
        return Ok(());
    }

    let proceed = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Branch '{}' is not fully merged. Force delete it with -D?",
            branch_name
        ))
        .default(false)
        .interact()?;

    if proceed {
        request.force_delete_branch = true;
        return Ok(());
    }

    request.delete_branch = false;
    println!("ℹ️  Keeping branch '{}' (not fully merged).", branch_name);
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

        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("🚀 Feature workspace ready ({})", summary.mode);
    println!("  Feature: {}", summary.work_feature);
    println!();
    print_start_checklist(summary, prompt_bridge_enabled, &agent_plan);

    if let Some(adapter) = summary.adapter.as_ref() {
        if !adapter.warnings.is_empty() {
            println!();
            println!("Adapter warnings:");
            for warning in &adapter.warnings {
                println!("  ⚠ {}", warning);
            }
        }
    }

    if !suppress_summary {
        if !summary.module_outcomes.is_empty() {
            println!();
            print_module_outcome_table(&summary.module_outcomes);
        }

        if !summary.skipped_modules.is_empty() {
            println!();
            println!("Skipped modules:");
            for record in &summary.skipped_modules {
                println!("  - {} ({})", record.name, record.reason.description());
            }
        }

        if summary.mode == StartMode::Minimal && !summary.skipped_modules.is_empty() {
            println!();
            println!(
                "Next: run `branchbox devcontainer sync` or targeted module commands when you're ready to fully provision."
            );
        }

        if !summary.warnings.is_empty() {
            println!();
            println!("Warnings:");
            for warning in &summary.warnings {
                println!("  - {}", warning);
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

    println!("{border}");
    println!(
        "| {step:<step_w$} | {result:<result_w$} | {detail:<detail_w$} |",
        step = "Step",
        result = "Result",
        detail = "Details",
        step_w = step_w,
        result_w = result_w,
        detail_w = detail_w,
    );
    println!("{border}");

    for row in rows {
        println!(
            "| {step:<step_w$} | {result:<result_w$} | {detail:<detail_w$} |",
            step = row.step,
            result = row.result,
            detail = row.details,
            step_w = step_w,
            result_w = result_w,
            detail_w = detail_w,
        );
    }

    println!("{border}");
}

fn print_teardown_summary(summary: &TeardownSummary) {
    println!("🧹 Feature teardown finished");
    println!(
        "  Worktree removed: {}",
        if summary.worktree_removed {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "  Branch deleted: {}",
        if summary.branch_deleted { "yes" } else { "no" }
    );
    if !summary.branch_deleted {
        println!(
            "  Branch kept: {} (delete manually with `git branch -d {}` or `git branch -D {}`)",
            summary.branch_name, summary.branch_name, summary.branch_name
        );
    }
    if !summary.adapter_cleanup_warnings.is_empty() {
        println!();
        println!("Adapter:");
        for warning in &summary.adapter_cleanup_warnings {
            println!("  ⚠ {}", warning);
        }
    }

    if !summary.module_reports.is_empty() {
        println!();
        println!("Modules:");
        for report in &summary.module_reports {
            let status = if report.teardown_ok { "ok" } else { "warn" };
            println!("  - {} ({})", report.name, status);
            for error in &report.errors {
                println!("      • {}", error);
            }
        }
    }

    if !summary.warnings.is_empty() {
        println!();
        println!("Warnings:");
        for warning in &summary.warnings {
            println!("  - {}", warning);
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

    println!("{border}");
    println!("{header}");
    println!("{border}");

    for (name, status, duration, notes) in rows {
        println!(
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

    println!("{border}");
    if outcomes.iter().any(|outcome| outcome.forced) {
        println!("(*) Forced module executed due to policy requirements.");
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
            println!();
            println!(
                "🤖 Launching {} via `{}` (cwd: {})",
                label,
                command,
                summary.worktree_path.display()
            );
            match launch_agent_process(command, &summary.worktree_path) {
                Ok(()) => println!("✅ Agent session completed successfully."),
                Err(err) => println!("⚠️  Default coding agent command failed: {err}"),
            }
        }
        AgentPlanStatus::Waiting | AgentPlanStatus::Blocked => {
            if let Some(message) = plan.followup.as_ref() {
                println!();
                println!("{message}");
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

fn launch_agent_process(command_line: &str, cwd: &Path) -> Result<()> {
    let parts = split_command_line(command_line)
        .map_err(|err| anyhow!("failed to parse BRANCHBOX_DEFAULT_AGENT_CMD: {}", err))?;

    if parts.is_empty() {
        bail!("BRANCHBOX_DEFAULT_AGENT_CMD must include an executable name");
    }

    let mut command = Command::new(&parts[0]);
    if parts.len() > 1 {
        command.args(&parts[1..]);
    }
    command.current_dir(cwd);

    let status = command
        .status()
        .with_context(|| format!("failed to launch {}", parts[0]))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("command exited with status {}", status))
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
