use anyhow::Result;
use chrono::Local;
use clap::{Args, Subcommand};
use dialoguer::{console::Term, theme::ColorfulTheme, Confirm};
use serde_json::json;
use std::{env, path::PathBuf};
use worktree_core::{
    workflows::feature::{
        FeatureMetadata, FeatureStatus, FeatureWorkflow, ModuleOutcome, ModuleOutcomeRecord,
        ModuleStatus, StartMode, StartRequest, StartSummary, TeardownRequest, TeardownSummary,
    },
    Error as CoreError,
};

#[derive(Subcommand)]
pub enum FeatureCommands {
    /// Create a new feature worktree and run module setup
    #[command(alias = "new")]
    Start(FeatureStartArgs),

    /// Tear down an existing feature worktree
    Teardown(FeatureTeardownArgs),

    /// List known feature worktrees from the registry
    List(FeatureListArgs),
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

    /// Delete the git branch after removing the worktree
    #[arg(long)]
    pub delete_branch: bool,

    /// Force removal even with local changes
    #[arg(long)]
    pub force: bool,

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

pub fn execute(command: FeatureCommands) -> Result<()> {
    match command {
        FeatureCommands::Start(args) => run_start(args),
        FeatureCommands::Teardown(args) => run_teardown(args),
        FeatureCommands::List(args) => run_list(args),
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
        json,
        no_summary,
    } = args;

    let mut mode = if minimal || fast {
        StartMode::Minimal
    } else {
        StartMode::Full
    };

    let fast_mode_enabled = env_flag("BRANCHBOX_ENABLE_FAST_MODE");
    if mode == StartMode::Minimal && !fast_mode_enabled {
        println!(
            "⚠️  Minimal mode disabled. Set BRANCHBOX_ENABLE_FAST_MODE=1 to enable the preview; running full mode instead."
        );
        mode = StartMode::Full;
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

    if json {
        let payload = serde_json::to_string_pretty(&features)?;
        println!("{}", payload);
        return Ok(());
    }

    if features.is_empty() {
        if let Some(filter) = status.as_ref() {
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

    let showing_count = features.len();
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

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(features.len());
    for feature in features {
        let url = feature_url_for_list(&feature);
        let tunnel = tunnel_summary_for_list(&feature);
        let devcontainer = devcontainer_status_for_list(&feature);
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

fn run_teardown(args: FeatureTeardownArgs) -> Result<()> {
    let FeatureTeardownArgs {
        name,
        branch_prefix,
        repo,
        delete_branch,
        force,
        complete_spec,
        telemetry,
    } = args;

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let mut request = TeardownRequest {
        work_feature: name,
        branch_prefix,
        delete_branch,
        force_remove: force,
        complete_spec,
        telemetry,
    };

    let summary = match workflow.teardown(request.clone()) {
        Ok(summary) => summary,
        Err(CoreError::WorktreeDirty { worktree, files }) => {
            println!(
                "⚠️  Detected devcontainer/compose changes inside {}:",
                worktree.display()
            );
            for entry in &files {
                println!("    • {}", entry);
            }
            println!("    (BranchBox refuses to delete dirty module files without --force)");

            if !Term::stdout().is_term() {
                anyhow::bail!(
                    "Devcontainer/compose changes detected; rerun this command with --force to proceed."
                );
            }

            let proceed = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(
                    "Continue teardown with --force? This will discard local module changes.",
                )
                .default(false)
                .interact()?;

            if !proceed {
                anyhow::bail!("Teardown aborted; rerun with --force to skip this prompt.");
            }

            request.force_remove = true;
            workflow.teardown(request)?
        }
        Err(err) => return Err(err.into()),
    };

    print_teardown_summary(&summary);

    Ok(())
}

fn print_start_summary(
    summary: &StartSummary,
    json_output: bool,
    suppress_summary: bool,
) -> Result<()> {
    let prompt_bridge_enabled = env_flag("BRANCHBOX_ENABLE_PROMPT_BRIDGE");

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
        });

        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("🚀 Feature workspace ready ({})", summary.mode);
    println!("  Worktree: {}", summary.worktree_path.display());
    println!("  Branch: {}", summary.branch_name);
    if let Some(color) = summary.color.as_ref() {
        println!("  Workspace color: {}", color);
    }
    if let Some(url) = summary.feature_url.as_ref() {
        println!("  Feature URL: https://{}", url);
    }
    if let Some(compose) = summary.compose_project_name.as_ref() {
        println!("  Compose project: {}", compose);
    }
    if let Some(env_path) = summary.env_path.as_ref() {
        println!("  .env copied to: {}", env_path.display());
    }
    if let Some(adapter) = summary.adapter.as_ref() {
        println!("  Adapter: {}", adapter.name);
        println!("  Service URL: {}", adapter.service_url);
        if !adapter.warnings.is_empty() {
            for warning in &adapter.warnings {
                println!("      ⚠ {}", warning);
            }
        }
    }

    match summary.prompt_seed.as_ref() {
        Some(seed) => {
            let length = seed.chars().count();
            if prompt_bridge_enabled {
                println!("  Prompt seed stored (length: {length} chars)");
            } else {
                println!(
                    "  Prompt seed stored locally (length: {length} chars; prompt bridge disabled)"
                );
            }
        }
        None => println!("  Prompt seed stored: no"),
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

    Ok(())
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
