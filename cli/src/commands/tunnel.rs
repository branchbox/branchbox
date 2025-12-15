use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::json;
use std::path::PathBuf;
use worktree_core::workflows::feature::{
    FeatureTunnelStatus, FeatureWorkflow, TunnelOpenRequest, TunnelRemoveRequest,
};

#[derive(Subcommand)]
pub enum TunnelCommands {
    /// Provision (or re-provision) a tunnel for an existing feature
    Open(TunnelOpenArgs),

    /// Remove tunnel metadata and attempt provider teardown
    Remove(TunnelRemoveArgs),
}

#[derive(Args)]
pub struct TunnelOpenArgs {
    /// Dasherized feature name (e.g., oauth-integration)
    pub name: String,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Emit JSON output instead of human-readable summary
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct TunnelRemoveArgs {
    /// Dasherized feature name (e.g., oauth-integration)
    pub name: String,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Continue even if provider teardown fails
    #[arg(long)]
    pub force: bool,

    /// Emit JSON output instead of human-readable summary
    #[arg(long)]
    pub json: bool,
}

pub fn execute(command: TunnelCommands) -> Result<()> {
    match command {
        TunnelCommands::Open(args) => run_open(args),
        TunnelCommands::Remove(args) => run_remove(args),
    }
}

fn run_open(args: TunnelOpenArgs) -> Result<()> {
    let repo_path = args.repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let summary = workflow.tunnel_open(TunnelOpenRequest {
        work_feature: args.name,
    })?;

    if args.json {
        println!(
            "{}",
            json!({
                "work_feature": summary.work_feature,
                "state": summary.state,
                "warnings": summary.warnings,
            })
        );
        return Ok(());
    }

    println!("🌐 Tunnel");
    println!("  Feature: {}", summary.work_feature);
    println!(
        "  Status: {}",
        match summary.state.status {
            FeatureTunnelStatus::Active => "online",
            FeatureTunnelStatus::Pending => "degraded",
            FeatureTunnelStatus::Manual => "manual",
            FeatureTunnelStatus::Disabled => "disabled",
        }
    );
    if let Some(hostname) = summary.state.hostname.as_deref().filter(|h| !h.is_empty()) {
        println!("  Hostname: {}", hostname);
    }
    if !summary.warnings.is_empty() {
        println!();
        println!("Warnings:");
        for warning in &summary.warnings {
            println!("  - {}", warning);
        }
    }

    Ok(())
}

fn run_remove(args: TunnelRemoveArgs) -> Result<()> {
    let repo_path = args.repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let summary = workflow.tunnel_remove(TunnelRemoveRequest {
        work_feature: args.name,
        force: args.force,
    })?;

    if args.json {
        println!(
            "{}",
            json!({
                "work_feature": summary.work_feature,
                "previous_state": summary.previous_state,
                "updated_state": summary.updated_state,
                "warnings": summary.warnings,
            })
        );
        return Ok(());
    }

    println!("🧹 Tunnel removed");
    println!("  Feature: {}", summary.work_feature);
    if !summary.warnings.is_empty() {
        println!();
        println!("Warnings:");
        for warning in &summary.warnings {
            println!("  - {}", warning);
        }
    }

    Ok(())
}
