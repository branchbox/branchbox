mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::feature::{self, FeatureCommands};
use std::path::PathBuf;
use worktree_core::bootstrap::{Bootstrap, Stack};

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
    /// Initialize project with devcontainer (alias: bootstrap)
    #[command(alias = "bootstrap")]
    Init {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Force specific stack (rails, nodejs, rust, generic)
        #[arg(short, long)]
        stack: Option<String>,
    },

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
        Commands::Init { path, stack } => {
            let project_path = path.unwrap_or_else(|| PathBuf::from("."));
            let bootstrap = Bootstrap::new(&project_path);

            let stack = if let Some(stack_str) = stack {
                match stack_str.to_lowercase().as_str() {
                    "rails" => Stack::Rails,
                    "nodejs" => Stack::NodeJs,
                    "rust" => Stack::Rust,
                    "generic" => Stack::Generic,
                    _ => {
                        eprintln!("Unknown stack: {}", stack_str);
                        eprintln!("Valid stacks: rails, nodejs, rust, generic");
                        std::process::exit(1);
                    }
                }
            } else {
                bootstrap.detect_stack()?
            };

            println!("📦 BranchBox - Detected stack: {:?}", stack);
            println!("🔧 Generating devcontainer files...");

            bootstrap.generate(stack)?;

            println!("✓ Project initialized!");
            println!("  - .devcontainer/devcontainer.json");
            println!("  - .devcontainer/compose.yaml");
            println!("  - .devcontainer/Dockerfile");
            println!("  - .env.sample (if not exists)");
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
            let module_plan = worktree_core::modules::detect_modules(&project_path);
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
