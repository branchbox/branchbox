//! Init command implementation

use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
use worktree_core::bootstrap::{Bootstrap, Stack};
use worktree_core::devcontainer::ProjectDetector;
use worktree_core::workflows::init::{
    DevcontainerStatus, InitOptions, InitSource, InitSummary, InitWorkflow, RepositoryState,
};

#[derive(Args)]
pub struct InitArgs {
    /// Repository URL or path (defaults to current directory)
    pub source: Option<String>,

    /// Target directory for parent worktree
    #[arg(short, long)]
    pub path: Option<PathBuf>,

    /// Force specific stack (rails, nodejs, rust, python, generic)
    #[arg(short, long)]
    pub stack: Option<String>,

    /// Skip devcontainer setup
    #[arg(long)]
    pub skip_devcontainer: bool,

    /// Skip environment setup
    #[arg(long)]
    pub skip_env: bool,

    /// Force reorganization into worktree structure
    #[arg(long)]
    pub reorganize: bool,

    /// Disable parent structure (keep flat layout instead of container/main/)
    #[arg(long)]
    pub no_parent_structure: bool,

    /// Update existing setup without restructuring
    #[arg(long)]
    pub update: bool,

    /// Validate only (no modifications)
    #[arg(long)]
    pub validate: bool,

    /// Dry run (show what would happen)
    #[arg(long)]
    pub dry_run: bool,

    /// Non-interactive mode (use defaults, answer yes to prompts)
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Disable AI coding agent mounts (.codex, .claude, .gh)
    #[arg(long)]
    pub no_coding_agents: bool,

    /// Project name (detected from project files if not specified)
    #[arg(long)]
    pub name: Option<String>,

    /// Ruby version for Rails projects (detected from .ruby-version if not specified)
    #[arg(long)]
    pub ruby_version: Option<String>,

    /// Node.js version (detected from .nvmrc if not specified)
    #[arg(long)]
    pub node_version: Option<String>,

    /// Python version (detected from .python-version if not specified)
    #[arg(long)]
    pub python_version: Option<String>,

    /// Application port
    #[arg(long)]
    pub port: Option<u16>,

    /// Show detected project settings without making changes
    #[arg(long)]
    pub show_detected: bool,
}

pub fn execute(args: InitArgs) -> Result<()> {
    // Handle --show-detected: display detected values and exit
    if args.show_detected {
        return show_detected_settings(&args);
    }

    // Parse source
    let source = if let Some(source_str) = args.source {
        // Determine if it's a URL or path
        if source_str.starts_with("http://")
            || source_str.starts_with("https://")
            || source_str.starts_with("git@")
            || source_str.starts_with("git://")
        {
            InitSource::Url(source_str)
        } else {
            InitSource::LocalPath(PathBuf::from(source_str))
        }
    } else {
        InitSource::CurrentDirectory
    };

    // Parse stack
    let stack = if let Some(stack_str) = args.stack {
        Some(parse_stack(&stack_str)?)
    } else {
        None
    };

    // Build options
    let options = InitOptions {
        source,
        target_dir: args.path,
        stack,
        skip_devcontainer: args.skip_devcontainer,
        skip_env: args.skip_env,
        reorganize: args.reorganize,
        use_parent_structure: !args.no_parent_structure, // Default is true (parent structure)
        update: args.update,
        validate_only: args.validate,
        dry_run: args.dry_run,
        non_interactive: args.yes,
        verbose: args.verbose,
        coding_agents: !args.no_coding_agents, // Default is true (coding agents enabled)
        project_name: args.name,
        ruby_version: args.ruby_version,
        node_version: args.node_version,
        python_version: args.python_version,
        port: args.port,
    };

    // Execute workflow
    let mut workflow = InitWorkflow::new(options);
    let summary = workflow.execute()?;

    // Print summary (unless validate mode which prints inline)
    if !args.validate {
        if args.verbose {
            print_verbose_summary(&summary)?;
        } else {
            print_summary(&summary)?;
        }
    }

    Ok(())
}

fn parse_stack(stack_str: &str) -> Result<Stack> {
    match stack_str.to_lowercase().as_str() {
        "rails" | "ruby" => Ok(Stack::Rails),
        "nodejs" | "node" | "javascript" | "js" => Ok(Stack::NodeJs),
        "rust" => Ok(Stack::Rust),
        "python" | "django" | "flask" => Ok(Stack::Python),
        "generic" => Ok(Stack::Generic),
        _ => {
            anyhow::bail!(
                "Unknown stack: {}\nValid stacks: rails, nodejs, rust, python, generic",
                stack_str
            );
        }
    }
}

fn print_summary(summary: &InitSummary) -> Result<()> {
    // Don't print anything if already initialized and not updating
    if matches!(
        summary.repository_state,
        RepositoryState::AlreadyInitialized
    ) {
        return Ok(());
    }

    // Minimal output by default (1-2 lines)
    println!();
    print!("✓ Initialized BranchBox");

    // Add stack info on same line if detected
    if !summary.adapter.is_empty() && summary.stack != worktree_core::bootstrap::Stack::Generic {
        println!(" ({:?} project)", summary.stack);
    } else {
        println!();
    }

    // Show location only if reorganized
    if summary.reorganized {
        println!("  Location: {}", summary.workspace_path.display());
    }

    // Show next step hint
    if !summary.next_steps.is_empty() {
        println!();
        println!("  Next: {}", summary.next_steps[0]);
    }

    // Show warnings if any (important)
    if !summary.warnings.is_empty() {
        println!();
        for warning in &summary.warnings {
            println!("  ⚠ {}", warning);
        }
    }

    println!();

    Ok(())
}

fn print_verbose_summary(summary: &InitSummary) -> Result<()> {
    // Don't print anything if already initialized and not updating
    if matches!(
        summary.repository_state,
        RepositoryState::AlreadyInitialized
    ) {
        return Ok(());
    }

    println!();
    println!("✓ Initialized BranchBox");

    if summary.reorganized {
        println!("  Location: {}", summary.workspace_path.display());
    }

    // Show what was created/updated
    match summary.devcontainer_status {
        DevcontainerStatus::Created => {
            println!("  ✓ Created devcontainer configuration");
        }
        DevcontainerStatus::Enhanced { ref changes } => {
            println!("  ✓ Enhanced devcontainer configuration");
            for change in changes {
                println!("    - {}", change);
            }
        }
        DevcontainerStatus::Valid => {
            println!("  ✓ Devcontainer configuration valid");
        }
        DevcontainerStatus::Invalid { ref issues } => {
            println!("  ⚠ Devcontainer has issues:");
            for issue in issues {
                println!("    - {}", issue);
            }
        }
        DevcontainerStatus::None => {}
    }

    if summary.registry_initialized {
        println!("  ✓ Initialized BranchBox registry");
    }

    // Show detected configuration
    if !summary.adapter.is_empty() {
        println!();
        println!("  Stack: {:?}", summary.stack);
        println!("  Adapter: {}", summary.adapter);
        if !summary.modules.is_empty() {
            println!("  Modules: {}", summary.modules.join(", "));
        }
    }

    // Show warnings
    if !summary.warnings.is_empty() {
        println!();
        for warning in &summary.warnings {
            println!("  ⚠ {}", warning);
        }
    }

    // Show next steps
    if !summary.next_steps.is_empty() {
        println!();
        println!("  Next steps:");
        for step in &summary.next_steps {
            println!("    {}", step);
        }
    }

    println!();

    Ok(())
}

/// Show detected project settings without making changes
fn show_detected_settings(args: &InitArgs) -> Result<()> {
    // Determine project path
    let project_path = if let Some(ref source) = args.source {
        if !source.starts_with("http://")
            && !source.starts_with("https://")
            && !source.starts_with("git@")
            && !source.starts_with("git://")
        {
            PathBuf::from(source)
        } else {
            anyhow::bail!("Cannot detect settings from URL. Clone the repository first.");
        }
    } else {
        std::env::current_dir()?
    };

    if !project_path.exists() {
        anyhow::bail!("Path does not exist: {}", project_path.display());
    }

    // Detect stack
    let bootstrap = Bootstrap::new(&project_path);
    let stack = bootstrap.detect_stack()?;

    // Detect project settings
    let detector = ProjectDetector::new(&project_path);
    let detected = detector.detect_all();

    println!();
    println!("Detected Project Settings");
    println!("==========================");
    println!();

    // Stack
    println!("  Stack:        {:?}", stack);

    // Project name
    if let Some(ref name) = detected.name {
        println!("  Project name: {} (detected)", name);
    } else {
        let fallback = project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        println!("  Project name: {} (from directory)", fallback);
    }

    // Stack-specific versions
    match stack {
        Stack::Rails => {
            if let Some(ref version) = detected.ruby_version {
                println!("  Ruby version: {} (detected)", version);
            } else {
                println!("  Ruby version: 3.3 (default)");
            }
        }
        Stack::NodeJs => {
            if let Some(ref version) = detected.node_version {
                println!("  Node version: {} (detected)", version);
            } else {
                println!("  Node version: 20 (default)");
            }
        }
        Stack::Python => {
            if let Some(ref version) = detected.python_version {
                println!("  Python version: {} (detected)", version);
            } else {
                println!("  Python version: 3.12 (default)");
            }
        }
        Stack::Rust => {
            if let Some(ref version) = detected.rust_version {
                println!("  Rust version: {} (detected)", version);
            } else {
                println!("  Rust version: stable (default)");
            }
        }
        Stack::Generic => {}
    }

    // Port
    if let Some(port) = detected.default_port {
        println!("  Default port: {} (detected)", port);
    } else {
        let default_port = match stack {
            Stack::Rails => 3000,
            Stack::NodeJs => 3000,
            Stack::Python => 5000,
            Stack::Rust => 8080,
            Stack::Generic => 3000,
        };
        println!("  Default port: {} (stack default)", default_port);
    }

    // Check for existing devcontainer
    let devcontainer_exists = project_path.join(".devcontainer").exists();
    println!();
    if devcontainer_exists {
        println!("  ℹ Existing .devcontainer/ found");
        println!("    Run `branchbox init` to enhance with BranchBox features");
    } else {
        println!("  ℹ No .devcontainer/ found");
        println!("    Run `branchbox init` to generate one");
    }

    // Show detection sources
    println!();
    println!("Detection Sources:");
    println!("------------------");

    let sources = [
        (".ruby-version", project_path.join(".ruby-version").exists()),
        ("Gemfile", project_path.join("Gemfile").exists()),
        (".nvmrc", project_path.join(".nvmrc").exists()),
        (".node-version", project_path.join(".node-version").exists()),
        ("package.json", project_path.join("package.json").exists()),
        (".python-version", project_path.join(".python-version").exists()),
        ("pyproject.toml", project_path.join("pyproject.toml").exists()),
        (".tool-versions", project_path.join(".tool-versions").exists()),
        ("Cargo.toml", project_path.join("Cargo.toml").exists()),
        ("rust-toolchain.toml", project_path.join("rust-toolchain.toml").exists()),
    ];

    let found_sources: Vec<_> = sources.iter().filter(|(_, exists)| *exists).collect();
    if found_sources.is_empty() {
        println!("  (no version files found, using defaults)");
    } else {
        for (name, _) in found_sources {
            println!("  ✓ {}", name);
        }
    }

    println!();

    Ok(())
}
