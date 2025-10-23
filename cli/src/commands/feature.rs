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
    #[arg(long)]
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
}

#[derive(Args)]
pub struct FeatureTeardownArgs {
    /// Dasherized feature name to tear down (e.g., oauth-integration)
    #[arg(long)]
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
}

#[derive(Args)]
pub struct FeatureListArgs {
    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Filter by status (active, removed)
    #[arg(long)]
    pub status: Option<String>,
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
    } = args;

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let mut request = StartRequest::default();
    request.name = name;
    request.title = title;
    request.base_branch = base;
    request.branch_prefix = branch_prefix;
    request.reuse_existing = reuse;

    let summary = workflow.start(request)?;
    print_start_summary(&summary);

    Ok(())
}

fn run_list(args: FeatureListArgs) -> Result<()> {
    let FeatureListArgs { repo, status } = args;
    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let mut features = workflow.list_features()?;

    if let Some(status_filter) = status {
        let parsed: FeatureStatus = status_filter.parse()?;
        features.retain(|feature| feature.status == parsed);
    }

    if features.is_empty() {
        println!("ℹ️  No features tracked yet. Run `branchbox feature start` to create one.");
        return Ok(());
    }

    println!("📚 Tracked features ({}):", features.len());
    for feature in features {
        println!(
            "- {name} [{status:?}]",
            name = feature.work_feature,
            status = feature.status
        );
        println!("    Branch: {}", feature.branch_name);
        println!("    Worktree: {}", feature.worktree_path.display());
        if let Some(url) = feature.feature_url.as_ref() {
            println!("    URL: https://{}", url);
        }
        println!(
            "    Updated: {}",
            feature
                .updated_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
        );
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
    } = args;

    let repo_path = repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let request = TeardownRequest {
        work_feature: name,
        branch_prefix,
        delete_branch,
        force_remove: force,
    };

    let summary = workflow.teardown(request)?;
    print_teardown_summary(&summary);

    Ok(())
}

fn print_start_summary(summary: &StartSummary) {
    println!("🚀 Feature workspace ready");
    println!("  Worktree: {}", summary.worktree_path.display());
    println!("  Branch: {}", summary.branch_name);
    if let Some(url) = summary.feature_url.as_ref() {
        println!("  Feature URL: https://{}", url);
    }
    if let Some(compose) = summary.compose_project_name.as_ref() {
        println!("  Compose project: {}", compose);
    }
    if let Some(env_path) = summary.env_path.as_ref() {
        println!("  .env copied to: {}", env_path.display());
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
