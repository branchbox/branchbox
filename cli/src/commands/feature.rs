use anyhow::Result;
use chrono::Local;
use clap::{Args, Subcommand};
use std::path::PathBuf;
use worktree_core::workflows::feature::{
    FeatureStatus, FeatureWorkflow, StartRequest, StartSummary, TeardownRequest, TeardownSummary,
};

#[derive(Subcommand)]
pub enum FeatureCommands {
    /// Create a new feature worktree and run module setup
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
    } = args;

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
    };

    let summary = workflow.start(request)?;
    print_start_summary(&summary);

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
        let url = feature
            .feature_url
            .as_ref()
            .map(|url| format!("https://{}", url))
            .unwrap_or_else(|| "—".to_string());

        let tunnel = feature
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
            .unwrap_or_else(|| "—".to_string());

        let devcontainer = if feature.devcontainer_outdated {
            "outdated".to_string()
        } else if let Some(sync_at) = feature.last_sync_at.as_ref() {
            format!(
                "synced {}",
                sync_at.with_timezone(&Local).format("%Y-%m-%d")
            )
        } else {
            "never".to_string()
        };

        let pr = feature
            .pr_number
            .map(|number| format!("#{number}"))
            .unwrap_or_else(|| "—".to_string());

        let color = feature.color.as_deref().unwrap_or("—").to_string();
        let updated = format_ts(&feature.updated_at);

        let row = vec![
            feature.work_feature,
            feature.status.to_string(),
            feature.branch_name,
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

    let request = TeardownRequest {
        work_feature: name,
        branch_prefix,
        delete_branch,
        force_remove: force,
        complete_spec,
        telemetry,
    };

    let summary = workflow.teardown(request)?;
    print_teardown_summary(&summary);

    Ok(())
}

fn print_start_summary(summary: &StartSummary) {
    println!("🚀 Feature workspace ready");
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

    if !summary.module_reports.is_empty() {
        println!();
        println!("Modules:");
        for report in &summary.module_reports {
            let status = match (report.init_ok, report.setup_ok) {
                (true, true) => "ok",
                (true, false) => "partial",
                _ => "failed",
            };
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
