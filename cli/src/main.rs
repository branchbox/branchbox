mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::devcontainer::{self, DevcontainerCommands};
use commands::feature::{self, FeatureCommands};
use commands::init::{self, InitArgs};
use std::path::PathBuf;
use worktree_core::bootstrap::Bootstrap;

#[derive(Parser)]
#[command(name = "branchbox")]
#[command(about = "Isolated development environments for every feature", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize project with devcontainer and BranchBox registry
    #[command(alias = "bootstrap")]
    Init(InitArgs),

    /// Manage devcontainer configuration
    #[command(subcommand)]
    Devcontainer(DevcontainerCommands),

    /// Detect project configuration
    Detect {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Feature name utilities
    #[command(subcommand)]
    Name(NameCommands),

    /// Manage feature worktrees
    #[command(subcommand)]
    Feature(FeatureCommands),
}

#[derive(Subcommand)]
enum NameCommands {
    /// Generate feature name from title
    Generate {
        /// Feature title (e.g., "OAuth Integration")
        title: String,
    },

    /// Validate feature name
    Validate {
        /// Feature name to validate (e.g., "oauth-integration")
        name: String,
    },
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => {
            init::execute(args)?;
        }

        Commands::Devcontainer(devcontainer_cmd) => {
            devcontainer::execute(devcontainer_cmd)?;
        }

        Commands::Detect { path } => {
            let project_path = path.unwrap_or_else(|| PathBuf::from("."));
            let bootstrap = Bootstrap::new(&project_path);
            let stack = bootstrap.detect_stack()?;

            println!("📦 BranchBox Configuration");
            println!();
            println!("Project: {}", project_path.display());
            println!("Stack: {:?}", stack);

            // Show what adapters would be used
            let adapters = worktree_core::adapters::detect_adapter(&project_path)?;
            println!("Adapter: {}", adapters.name());

            // Show what modules would be enabled
            let module_plan = worktree_core::modules::detect_modules(&project_path, &[]);
            println!();
            println!("Enabled modules: {}", module_plan.handles.len());
            for handle in &module_plan.handles {
                println!("  ✓ {}", handle.name);
            }
            if !module_plan.warnings.is_empty() {
                println!();
                println!("Warnings:");
                for warning in module_plan.warnings {
                    println!("  - {}", warning);
                }
            }
        }

        Commands::Feature(feature_cmd) => {
            feature::execute(feature_cmd)?;
        }

        Commands::Name(name_cmd) => match name_cmd {
            NameCommands::Generate { title } => {
                let name = worktree_core::naming::generate_work_feature(&title);
                println!("{}", name);
            }

            NameCommands::Validate { name } => {
                if worktree_core::naming::validate_work_feature(&name) {
                    println!("✓ Valid feature name: {}", name);
                } else {
                    eprintln!("✗ Invalid feature name: {}", name);
                    eprintln!(
                        "  Feature names must be DNS-safe (lowercase a-z, 0-9, hyphens only)"
                    );
                    std::process::exit(1);
                }
            }
        },
    }

    Ok(())
}
