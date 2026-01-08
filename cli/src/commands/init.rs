//! Init command implementation

use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
use worktree_core::bootstrap::Stack;
use worktree_core::config::{GitHubAuthStrategy, SshAgentProvider};
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

    /// Force specific stack (rails, nodejs, rust, generic)
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

    /// GitHub authentication strategy: ssh (default), gh-cli, or none
    #[arg(long, value_name = "STRATEGY")]
    pub github_auth: Option<String>,

    /// Use SSH key authentication (default, same as --github-auth=ssh)
    #[arg(long, conflicts_with_all = ["gh_auth", "no_github_auth"])]
    pub ssh_auth: bool,

    /// Use gh CLI token authentication (legacy, same as --github-auth=gh-cli)
    #[arg(long, conflicts_with_all = ["ssh_auth", "no_github_auth"])]
    pub gh_auth: bool,

    /// Disable GitHub authentication mounts entirely
    #[arg(long, conflicts_with_all = ["ssh_auth", "gh_auth"])]
    pub no_github_auth: bool,

    /// Use 1Password SSH agent instead of system agent
    #[arg(long = "1password", visible_alias = "op")]
    pub onepassword: bool,
}

pub fn execute(args: InitArgs) -> Result<()> {
    // Parse GitHub auth settings first (before moving fields from args)
    let github_auth = parse_github_auth(&args)?;

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
        github_auth,
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
        "rails" => Ok(Stack::Rails),
        "nodejs" | "node" => Ok(Stack::NodeJs),
        "rust" => Ok(Stack::Rust),
        "generic" => Ok(Stack::Generic),
        _ => {
            anyhow::bail!(
                "Unknown stack: {}\nValid stacks: rails, nodejs, rust, generic",
                stack_str
            );
        }
    }
}

fn parse_github_auth(
    args: &InitArgs,
) -> Result<Option<worktree_core::config::GitHubAuthSettings>> {
    use worktree_core::config::GitHubAuthSettings;

    // Determine strategy from flags
    let strategy = if args.no_github_auth {
        GitHubAuthStrategy::None
    } else if args.gh_auth {
        GitHubAuthStrategy::GhCli
    } else if args.ssh_auth {
        GitHubAuthStrategy::Ssh
    } else if let Some(ref strategy_str) = args.github_auth {
        strategy_str.parse()?
    } else {
        // No explicit flag - return None to use defaults
        if !args.onepassword {
            return Ok(None);
        }
        GitHubAuthStrategy::Ssh
    };

    // Build settings
    let mut settings = GitHubAuthSettings::default();
    settings.strategy = strategy;

    // Handle 1Password flag
    if args.onepassword {
        if strategy != GitHubAuthStrategy::Ssh {
            anyhow::bail!("--1password requires SSH auth (remove --gh-auth or --no-github-auth)");
        }
        settings.ssh_agent_provider = SshAgentProvider::OnePassword;
    }

    Ok(Some(settings))
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
