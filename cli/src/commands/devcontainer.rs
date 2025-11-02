//! Devcontainer commands
//!
//! Commands for managing devcontainer configuration across feature worktrees

use anyhow::{Context, Result};
use clap::Subcommand;
use std::io::Write;
use std::path::PathBuf;
use worktree_core::modules::{DevcontainerModule, Module};
use worktree_core::workflows::feature::{FeatureStatus, FeatureWorkflow};

#[derive(Subcommand)]
pub enum DevcontainerCommands {
    /// Sync devcontainer configuration to all feature worktrees
    Sync {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Sync strategy (copy or symlink)
        #[arg(short, long)]
        strategy: Option<String>,

        /// Dry run - show what would be synced without making changes
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
}

pub fn execute(cmd: DevcontainerCommands) -> Result<()> {
    match cmd {
        DevcontainerCommands::Sync {
            path,
            strategy,
            dry_run,
        } => sync(path, strategy, dry_run),
    }
}

fn sync(path: Option<PathBuf>, strategy: Option<String>, dry_run: bool) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    // Set strategy env var if provided
    if let Some(strat) = strategy {
        std::env::set_var("BRANCHBOX_DEVCONTAINER_STRATEGY", strat);
    }

    // Load workflow to find all active worktrees
    let workflow = FeatureWorkflow::new(&project_path)?;
    let features = workflow
        .list_features()
        .context("Failed to list features")?;

    let active_features: Vec<_> = features
        .into_iter()
        .filter(|f| f.status == FeatureStatus::Active)
        .collect();

    if active_features.is_empty() {
        println!("No active feature worktrees found");
        return Ok(());
    }

    println!(
        "🔄 Syncing devcontainer configuration to {} feature worktree(s)",
        active_features.len()
    );
    println!();

    if dry_run {
        println!("DRY RUN - no changes will be made");
        println!();
    }

    let mut synced_count = 0;
    let mut errors = Vec::new();
    let module = if dry_run {
        None
    } else {
        let mut module = DevcontainerModule::new();
        module
            .init(&project_path, project_path.as_path())
            .context("Failed to initialize devcontainer module")?;
        Some(module)
    };

    for feature in active_features {
        print!("  {} ... ", feature.work_feature);
        std::io::stdout().flush()?;

        // Check if worktree still exists
        if !feature.worktree_path.exists() {
            println!(
                "⚠️  worktree not found at {}",
                feature.worktree_path.display()
            );
            continue;
        }

        if dry_run {
            println!("would sync");
            synced_count += 1;
            continue;
        }

        let module_ref = module
            .as_ref()
            .expect("Devcontainer module should be initialised when not running in dry-run mode");
        let result = module_ref.sync_to(&feature.worktree_path);

        match result {
            Ok(outcome) => {
                let file_count = outcome.synced_files.len();
                let strategy = outcome.strategy;
                println!("✓ synced {} files ({})", file_count, strategy.as_str());
                synced_count += 1;
                if let Err(err) = workflow.record_devcontainer_sync(
                    &feature.work_feature,
                    Some(strategy.as_str()),
                    true,
                ) {
                    println!("    ⚠️ failed to update registry: {}", err);
                }
            }
            Err(e) => {
                println!("✗ failed: {}", e);
                errors.push((feature.work_feature.clone(), e.to_string()));
                if let Err(err) = workflow.record_devcontainer_sync(
                    &feature.work_feature,
                    Some(module_ref.strategy().as_str()),
                    false,
                ) {
                    println!("    ⚠️ failed to update registry: {}", err);
                }
            }
        }
    }

    println!();
    println!("✓ Successfully synced {} feature worktree(s)", synced_count);

    if !errors.is_empty() {
        println!();
        println!("⚠️  {} error(s) occurred:", errors.len());
        for (feature, error) in errors {
            println!("  - {}: {}", feature, error);
        }
    }

    Ok(())
}
