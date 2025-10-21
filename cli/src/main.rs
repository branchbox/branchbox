use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use worktree_core::bootstrap::{Bootstrap, Stack};

#[derive(Parser)]
#[command(name = "worktree")]
#[command(about = "Git worktree and devcontainer orchestration", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate devcontainer configuration for a project
    Bootstrap {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Force specific stack (rails, nodejs, rust, generic)
        #[arg(short, long)]
        stack: Option<String>,
    },

    /// Detect project stack and show configuration
    Detect {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Validate project naming
    ValidateName {
        /// Feature name to validate
        name: String,
    },

    /// Generate feature name from title
    GenerateName {
        /// Feature title
        title: String,
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
        Commands::Bootstrap { path, stack } => {
            let project_path = path.unwrap_or_else(|| PathBuf::from("."));
            let bootstrap = Bootstrap::new(&project_path)?;

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

            println!("Detected stack: {:?}", stack);
            println!("Generating devcontainer files...");

            bootstrap.generate(stack)?;

            println!("✓ Devcontainer configuration generated!");
            println!("  - .devcontainer/devcontainer.json");
            println!("  - .devcontainer/compose.yaml");
            println!("  - .devcontainer/Dockerfile");
            println!("  - .env.sample (if not exists)");
        }

        Commands::Detect { path } => {
            let project_path = path.unwrap_or_else(|| PathBuf::from("."));
            let bootstrap = Bootstrap::new(&project_path)?;
            let stack = bootstrap.detect_stack()?;

            println!("Project: {}", project_path.display());
            println!("Stack: {:?}", stack);

            // Show what adapters would be used
            let adapters = worktree_core::adapters::detect_adapter(&project_path)?;
            println!("Adapter: {}", adapters.name());

            // Show what modules would be enabled
            let modules = worktree_core::modules::detect_modules(&project_path);
            println!("Enabled modules: {}", modules.len());
            for module in modules {
                println!("  - {}", module.name());
            }
        }

        Commands::ValidateName { name } => {
            if worktree_core::naming::validate_work_feature(&name) {
                println!("✓ Valid feature name: {}", name);
            } else {
                eprintln!("✗ Invalid feature name: {}", name);
                eprintln!("  Feature names must be DNS-safe (lowercase a-z, 0-9, hyphens only)");
                std::process::exit(1);
            }
        }

        Commands::GenerateName { title } => {
            let name = worktree_core::naming::generate_work_feature(&title);
            println!("Feature name: {}", name);
        }
    }

    Ok(())
}
