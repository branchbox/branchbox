#![cfg_attr(test, allow(clippy::disallowed_macros))]

mod agent;
mod commands;
mod json_error;

use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::agent as agent_commands;
use commands::agent::AgentCommands;
use commands::config::{self, ConfigCommands};
use commands::detect::{self, DetectArgs};
use commands::devcontainer::{self, DevcontainerCommands};
use commands::doctor::{self, DoctorArgs};
use commands::feature::{self, FeatureCommands, FeaturePruneArgs};
use commands::init::{self, InitArgs};
use commands::tunnel::{self, TunnelCommands};
use commands::version::{self, VersionArgs};
use json_error::ErrorEnvelope;
use std::ffi::OsStr;
use std::io::IsTerminal;
use std::process::ExitCode;
use worktree_core::{humanln, output};

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

    /// Agent and control-plane helpers
    #[command(subcommand)]
    Agent(AgentCommands),

    /// Detect project configuration
    Detect(DetectArgs),

    /// Feature name utilities
    #[command(subcommand)]
    Name(NameCommands),

    /// Manage feature worktrees
    #[command(subcommand, alias = "features")]
    Feature(FeatureCommands),

    /// Tear down all active feature worktrees
    Prune(FeaturePruneArgs),

    /// Manage tunnels for existing features
    #[command(subcommand)]
    Tunnel(TunnelCommands),

    /// Show the BranchBox version and, with --json, its contract capabilities
    Version(VersionArgs),

    /// Check the host (and optionally a repository) for BranchBox prerequisites
    Doctor(DoctorArgs),

    /// Read and change project configuration (.branchbox/config.json)
    #[command(subcommand)]
    Config(ConfigCommands),
}

impl Commands {
    /// Whether this invocation asked for machine (`--json`) output. Each command module answers
    /// for its own flags, so adding one never touches this file.
    fn wants_json(&self) -> bool {
        match self {
            Commands::Init(args) => args.wants_json(),
            Commands::Devcontainer(command) => command.wants_json(),
            Commands::Agent(command) => command.wants_json(),
            Commands::Detect(args) => args.wants_json(),
            Commands::Name(_) => false,
            Commands::Feature(command) => command.wants_json(),
            Commands::Prune(args) => args.wants_json(),
            Commands::Tunnel(command) => command.wants_json(),
            Commands::Version(args) => args.wants_json(),
            Commands::Doctor(args) => args.wants_json(),
            Commands::Config(command) => command.wants_json(),
        }
    }
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

fn main() -> ExitCode {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(log_colors_enabled(
            std::io::stderr().is_terminal(),
            std::env::var_os("NO_COLOR").as_deref(),
        ))
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Machine mode (DESIGN §5.2): stdout carries exactly one JSON document, human text goes to
    // stderr and nothing prompts.
    output::set_machine_mode(cli.command.wants_json());
    if output::machine_mode() {
        install_machine_mode_panic_hook();
    }

    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report_failure(&err),
    }
}

fn run(command: Commands) -> Result<()> {
    match command {
        Commands::Init(args) => init::execute(args),
        Commands::Devcontainer(devcontainer_cmd) => devcontainer::execute(devcontainer_cmd),
        Commands::Agent(agent_cmd) => agent_commands::execute(agent_cmd),
        Commands::Detect(args) => detect::execute(args),
        Commands::Feature(feature_cmd) => feature::execute(feature_cmd),
        Commands::Prune(args) => feature::run_prune(args),
        Commands::Tunnel(tunnel_cmd) => tunnel::execute(tunnel_cmd),
        Commands::Version(args) => version::execute(args),
        Commands::Doctor(args) => doctor::execute(args),
        Commands::Config(config_cmd) => config::execute(config_cmd),
        Commands::Name(name_cmd) => run_name(name_cmd),
    }
}

fn run_name(command: NameCommands) -> Result<()> {
    match command {
        NameCommands::Generate { title } => {
            let name = worktree_core::naming::generate_work_feature(&title);
            humanln!("{}", name);
        }

        NameCommands::Validate { name } => {
            if worktree_core::naming::validate_work_feature(&name) {
                humanln!("✓ Valid feature name: {}", name);
            } else {
                eprintln!("✗ Invalid feature name: {}", name);
                eprintln!("  Feature names must be DNS-safe (lowercase a-z, 0-9, hyphens only)");
                return Err(worktree_core::Error::InvalidFeatureName(name).into());
            }
        }
    }

    Ok(())
}

/// Report a failed command and pick its exit code (1, as `fn main() -> Result<()>` did).
///
/// In machine mode the error envelope is printed on stdout, unless the command already printed
/// its document (an in-band failure such as `feature exec --json`, whose payload is the
/// result). stderr gets the same `Error: {err:?}` report in both modes.
fn report_failure(err: &anyhow::Error) -> ExitCode {
    if output::machine_mode() && !output::document_emitted() {
        if let Err(emit_err) = output::emit_json(&ErrorEnvelope::from_error(err)) {
            tracing::debug!("Failed to print the error envelope: {emit_err}");
        }
    }
    eprintln!("Error: {err:?}");
    ExitCode::FAILURE
}

/// In machine mode a panic on the main thread still leaves one document on stdout: the
/// `internal_panic` envelope, unless the payload was already printed. The default hook then
/// reports the panic on stderr and the process exits 101 as before. A panic on a worker thread
/// that the main thread recovers from prints nothing; one it does not recover from panics the
/// main thread too.
fn install_machine_mode_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let on_main_thread = std::thread::current().name() == Some("main");
        if on_main_thread && !output::document_emitted() {
            let envelope = ErrorEnvelope::from_panic(info.payload(), info.location());
            if let Err(err) = output::emit_json(&envelope) {
                tracing::debug!("Failed to print the panic envelope: {err}");
            }
        }
        default_hook(info);
    }));
}

/// ANSI colours in log lines only when stderr is a terminal and `NO_COLOR` is unset or empty
/// (<https://no-color.org>). Text-mode stdout is unaffected.
fn log_colors_enabled(stderr_is_terminal: bool, no_color: Option<&OsStr>) -> bool {
    stderr_is_terminal && no_color.is_none_or(OsStr::is_empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn wants_json(args: &[&str]) -> bool {
        let mut argv = vec!["branchbox"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv)
            .unwrap_or_else(|err| panic!("{args:?} should parse: {err}"))
            .command
            .wants_json()
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_json_flag_selects_machine_mode() {
        for args in [
            &["feature", "start", "eta", "--json"][..],
            &["feature", "teardown", "eta", "--json"],
            &["feature", "list", "--json"],
            &["feature", "exec", "eta", "--json", "--", "true"],
            &[
                "feature",
                "dispatch-tool",
                "eta",
                "--lease",
                "l",
                "--request-id",
                "r",
                "--json",
            ],
            &["tunnel", "open", "eta", "--json"],
            &["tunnel", "remove", "eta", "--json"],
            &["agent", "status", "--json"],
            &["detect", "--json"],
            &["version", "--json"],
            &["doctor", "--json"],
            &["config", "get", "--json"],
            &["config", "apply", "--file", "-", "--json"],
            &["devcontainer", "up", "--json"],
            &["devcontainer", "exec", "--json", "--", "true"],
            &["devcontainer", "down", "--json"],
            &["devcontainer", "build", "--json"],
            &["devcontainer", "read-configuration", "--json"],
            &["devcontainer", "configure", "--json"],
            &["devcontainer", "detect", "--json"],
            &["devcontainer", "add-tunnel", "--json"],
            &["devcontainer", "inject-agents", "--json"],
            &["devcontainer", "sync", "--json"],
            &["init", "--json"],
            &["config", "get", "runtime.provider", "--json"],
            &["feature", "prune", "--dry-run", "--json"],
            &["prune", "--dry-run", "--json"],
            &[
                "tunnel",
                "credentials",
                "set",
                "--account-id",
                "a",
                "--api-token-stdin",
                "--json",
            ],
        ] {
            assert!(wants_json(args), "{args:?} should select machine mode");
        }
    }

    #[test]
    fn text_invocations_stay_in_text_mode() {
        for args in [
            &["feature", "start", "eta"][..],
            &["feature", "list"],
            &["feature", "prune", "--yes"],
            &["feature", "exec-provider", "eta", "--provider", "codex"],
            &["prune", "--dry-run"],
            &["init", "--yes"],
            &["detect"],
            &["version"],
            &["doctor"],
            &["config", "set", "feature.branch_prefix", "feat"],
            &["config", "unset", "feature.branch_prefix"],
            &["devcontainer", "sync"],
            &["name", "validate", "eta"],
            &["agent", "status"],
            &["tunnel", "open", "eta"],
        ] {
            assert!(!wants_json(args), "{args:?} should stay in text mode");
        }
    }

    #[test]
    fn log_colors_follow_the_terminal_and_no_color() {
        assert!(log_colors_enabled(true, None));
        assert!(log_colors_enabled(true, Some(OsStr::new(""))));
        assert!(!log_colors_enabled(true, Some(OsStr::new("1"))));
        assert!(!log_colors_enabled(false, None));
    }
}
