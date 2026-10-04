//! Init command implementation
//!
//! `init --json` (DESIGN §5.13) prints the summary as one JSON document and never prompts, as
//! if `--yes` were given; the progress text goes to stderr. The `--op-*` and `--skip-1password`
//! flags record the 1Password setup without the interactive questions.

use crate::json_error::CliError;
use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
use worktree_core::bootstrap::Stack;
use worktree_core::workflows::init::{
    DevcontainerStatus, InitOptions, InitSource, InitSummary, InitWorkflow, OnePasswordSetup,
    RepositoryState,
};
use worktree_core::{human, humanln, output};

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

    /// 1Password reference for the devcontainer's GitHub token (op://vault/item/field), saved
    /// to .devcontainer/.env without prompting
    #[arg(long, value_name = "OP_REF", conflicts_with = "skip_1password")]
    pub op_github_ref: Option<String>,

    /// 1Password reference for the devcontainer's SSH signing key (with --op-github-ref)
    #[arg(long, value_name = "OP_REF", requires = "op_github_ref")]
    pub op_signing_key_ref: Option<String>,

    /// Do not use 1Password for devcontainer credentials (recorded in .devcontainer/.env)
    #[arg(long = "skip-1password")]
    pub skip_1password: bool,

    /// Save the --op-* references without checking them with `op read`
    #[arg(long, requires = "op_github_ref")]
    pub no_verify_op_refs: bool,

    /// Emit the summary as JSON (implies --yes: nothing is asked)
    #[arg(long)]
    pub json: bool,
}

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3).
pub const CAPABILITIES: &[&str] = &["init-json"];

impl InitArgs {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        self.json
    }

    /// The 1Password setup the flags ask for.
    fn onepassword(&self) -> OnePasswordSetup {
        if self.skip_1password {
            OnePasswordSetup::Skip
        } else if let Some(github_ref) = &self.op_github_ref {
            OnePasswordSetup::Configure {
                github_ref: github_ref.trim().to_string(),
                signing_key_ref: self
                    .op_signing_key_ref
                    .as_deref()
                    .map(str::trim)
                    .filter(|reference| !reference.is_empty())
                    .map(str::to_string),
                verify: !self.no_verify_op_refs,
            }
        } else {
            OnePasswordSetup::Unchanged
        }
    }
}

pub fn execute(args: InitArgs) -> Result<()> {
    let onepassword = args.onepassword();

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
        // --json never prompts: it takes the same defaults as --yes.
        non_interactive: args.yes || args.json,
        verbose: args.verbose,
        coding_agents: !args.no_coding_agents, // Default is true (coding agents enabled)
        onepassword,
    };

    // Execute workflow
    let mut workflow = InitWorkflow::new(options);
    let summary = workflow.execute()?;

    if args.json {
        output::emit_json(&summary.document())?;
        return Ok(());
    }

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
        _ => Err(CliError::new(
            "validation_failed",
            format!("Unknown stack: {stack_str}\nValid stacks: rails, nodejs, rust, generic"),
        )
        .into()),
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
    humanln!();
    human!("✓ Initialized BranchBox");

    // Add stack info on same line if detected
    if !summary.adapter.is_empty() && summary.stack != worktree_core::bootstrap::Stack::Generic {
        humanln!(" ({:?} project)", summary.stack);
    } else {
        humanln!();
    }

    // Show location only if reorganized
    if summary.reorganized {
        humanln!("  Location: {}", summary.workspace_path.display());
    }

    // Show next step hint
    if !summary.next_steps.is_empty() {
        humanln!();
        humanln!("  Next: {}", summary.next_steps[0]);
    }

    // Show warnings if any (important)
    if !summary.warnings.is_empty() {
        humanln!();
        for warning in &summary.warnings {
            humanln!("  ⚠ {}", warning);
        }
    }

    humanln!();

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

    humanln!();
    humanln!("✓ Initialized BranchBox");

    if summary.reorganized {
        humanln!("  Location: {}", summary.workspace_path.display());
    }

    // Show what was created/updated
    match summary.devcontainer_status {
        DevcontainerStatus::Created => {
            humanln!("  ✓ Created devcontainer configuration");
        }
        DevcontainerStatus::Enhanced { ref changes } => {
            humanln!("  ✓ Enhanced devcontainer configuration");
            for change in changes {
                humanln!("    - {}", change);
            }
        }
        DevcontainerStatus::Valid => {
            humanln!("  ✓ Devcontainer configuration valid");
        }
        DevcontainerStatus::Invalid { ref issues } => {
            humanln!("  ⚠ Devcontainer has issues:");
            for issue in issues {
                humanln!("    - {}", issue);
            }
        }
        DevcontainerStatus::None => {}
    }

    if summary.registry_initialized {
        humanln!("  ✓ Initialized BranchBox registry");
    }

    // Show detected configuration
    if !summary.adapter.is_empty() {
        humanln!();
        humanln!("  Stack: {:?}", summary.stack);
        humanln!("  Adapter: {}", summary.adapter);
        if !summary.modules.is_empty() {
            humanln!("  Modules: {}", summary.modules.join(", "));
        }
    }

    // Show warnings
    if !summary.warnings.is_empty() {
        humanln!();
        for warning in &summary.warnings {
            humanln!("  ⚠ {}", warning);
        }
    }

    // Show next steps
    if !summary.next_steps.is_empty() {
        humanln!();
        humanln!("  Next steps:");
        for step in &summary.next_steps {
            humanln!("    {}", step);
        }
    }

    humanln!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: InitArgs,
    }

    fn parse(args: &[&str]) -> Result<InitArgs, clap::Error> {
        let mut argv = vec!["init"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).map(|cli| cli.args)
    }

    #[test]
    fn json_selects_machine_mode() {
        assert!(parse(&["--json"]).unwrap().wants_json());
        assert!(!parse(&["--yes"]).unwrap().wants_json());
        assert_eq!(CAPABILITIES, ["init-json"]);
    }

    #[test]
    fn onepassword_flags_map_onto_the_setup() {
        assert_eq!(
            parse(&[]).unwrap().onepassword(),
            OnePasswordSetup::Unchanged
        );
        assert_eq!(
            parse(&["--skip-1password"]).unwrap().onepassword(),
            OnePasswordSetup::Skip
        );
        assert_eq!(
            parse(&[
                "--op-github-ref",
                " op://v/github/token ",
                "--op-signing-key-ref",
                "op://v/ssh/key",
                "--no-verify-op-refs",
            ])
            .unwrap()
            .onepassword(),
            OnePasswordSetup::Configure {
                github_ref: "op://v/github/token".to_string(),
                signing_key_ref: Some("op://v/ssh/key".to_string()),
                verify: false,
            }
        );
        assert_eq!(
            parse(&["--op-github-ref", "op://v/github/token"])
                .unwrap()
                .onepassword(),
            OnePasswordSetup::Configure {
                github_ref: "op://v/github/token".to_string(),
                signing_key_ref: None,
                verify: true,
            }
        );
    }

    #[test]
    fn onepassword_flags_that_contradict_each_other_are_usage_errors() {
        assert!(parse(&["--skip-1password", "--op-github-ref", "op://v/i/f"]).is_err());
        assert!(parse(&["--op-signing-key-ref", "op://v/i/f"]).is_err());
        assert!(parse(&["--no-verify-op-refs"]).is_err());
    }

    #[test]
    fn unknown_stacks_are_validation_failures() {
        let err = parse_stack("cobol").unwrap_err();
        let cli = err.downcast_ref::<CliError>().unwrap();
        assert_eq!(cli.code, "validation_failed");
        assert_eq!(
            cli.message,
            "Unknown stack: cobol\nValid stacks: rails, nodejs, rust, generic"
        );
        assert_eq!(parse_stack("Node").unwrap(), Stack::NodeJs);
    }
}
