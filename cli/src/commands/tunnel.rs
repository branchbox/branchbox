//! `branchbox tunnel`: tunnels for existing features, and the Cloudflare credentials used to
//! provision them.

use crate::json_error::CliError;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use worktree_core::credentials::{self, CredentialsSummary, TokenChange};
use worktree_core::workflows::feature::{
    FeatureTunnelStatus, FeatureWorkflow, TunnelOpenRequest, TunnelRemoveRequest,
};
use worktree_core::{humanln, output};

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3).
pub const CAPABILITIES: &[&str] = &["tunnel-credentials"];

/// Most bytes read from standard input for an API token; a token is 40 characters.
const MAX_TOKEN_INPUT: u64 = 64 * 1024;

#[derive(Subcommand)]
pub enum TunnelCommands {
    /// Provision (or re-provision) a tunnel for an existing feature
    Open(TunnelOpenArgs),

    /// Remove tunnel metadata and attempt provider teardown
    Remove(TunnelRemoveArgs),

    /// Manage the Cloudflare API credentials used to provision tunnels
    #[command(subcommand)]
    Credentials(TunnelCredentialsCommands),
}

#[derive(Subcommand)]
pub enum TunnelCredentialsCommands {
    /// Store the Cloudflare account ID and API token (.branchbox/secure/cloudflared.env,
    /// owner-only) and enable automatic tunnel provisioning
    Set(TunnelCredentialsSetArgs),
}

#[derive(Args)]
pub struct TunnelCredentialsSetArgs {
    /// Cloudflare account ID
    #[arg(long, value_name = "ID", required_unless_present = "clear")]
    pub account_id: Option<String>,

    /// Read the API token from standard input (it is never accepted as an argument)
    #[arg(long, required_unless_present = "clear", conflicts_with = "clear")]
    pub api_token_stdin: bool,

    /// Remove the stored API token; tunnels fall back to manual setup instructions
    #[arg(long)]
    pub clear: bool,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Emit JSON output instead of human-readable summary
    #[arg(long)]
    pub json: bool,
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

impl TunnelCommands {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        match self {
            TunnelCommands::Open(args) => args.json,
            TunnelCommands::Remove(args) => args.json,
            TunnelCommands::Credentials(TunnelCredentialsCommands::Set(args)) => args.json,
        }
    }
}

pub fn execute(command: TunnelCommands) -> Result<()> {
    match command {
        TunnelCommands::Open(args) => run_open(args),
        TunnelCommands::Remove(args) => run_remove(args),
        TunnelCommands::Credentials(TunnelCredentialsCommands::Set(args)) => {
            run_credentials_set(args)
        }
    }
}

fn run_open(args: TunnelOpenArgs) -> Result<()> {
    let repo_path = args.repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;
    let summary = workflow.tunnel_open(TunnelOpenRequest {
        work_feature: args.name,
    })?;

    if args.json {
        output::emit_json(&json!({
            "work_feature": summary.work_feature,
            "state": summary.state,
            "warnings": summary.warnings,
        }))?;
        return Ok(());
    }

    humanln!("🌐 Tunnel");
    humanln!("  Feature: {}", summary.work_feature);
    humanln!(
        "  Status: {}",
        match summary.state.status {
            FeatureTunnelStatus::Active => "online",
            FeatureTunnelStatus::Pending => "degraded",
            FeatureTunnelStatus::Manual => "manual",
            FeatureTunnelStatus::Disabled => "disabled",
        }
    );
    if let Some(hostname) = summary.state.hostname.as_deref().filter(|h| !h.is_empty()) {
        humanln!("  Hostname: {}", hostname);
    }
    if !summary.warnings.is_empty() {
        humanln!();
        humanln!("Warnings:");
        for warning in &summary.warnings {
            humanln!("  - {}", warning);
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
        output::emit_json(&json!({
            "work_feature": summary.work_feature,
            "previous_state": summary.previous_state,
            "updated_state": summary.updated_state,
            "warnings": summary.warnings,
        }))?;
        return Ok(());
    }

    humanln!("🧹 Tunnel removed");
    humanln!("  Feature: {}", summary.work_feature);
    if !summary.warnings.is_empty() {
        humanln!();
        humanln!("Warnings:");
        for warning in &summary.warnings {
            humanln!("  - {}", warning);
        }
    }

    Ok(())
}

/// `tunnel credentials set` (DESIGN §5.11). The token comes from standard input only, so it
/// never appears in argv, and nothing printed (text, JSON or errors) contains it.
fn run_credentials_set(args: TunnelCredentialsSetArgs) -> Result<()> {
    let repo_path = args.repo.unwrap_or_else(|| PathBuf::from("."));
    let workflow = FeatureWorkflow::new(&repo_path)?;

    let token = if args.clear {
        None
    } else {
        Some(read_api_token()?)
    };
    let change = match token.as_deref() {
        Some(token) => TokenChange::Set(token),
        None => TokenChange::Clear,
    };
    let summary = credentials::set_cloudflare_credentials(
        workflow.repo_root(),
        args.account_id.as_deref(),
        change,
    )?;

    if args.json {
        output::emit_json(&summary)?;
    } else {
        print_credentials_summary(&summary, args.clear);
    }
    Ok(())
}

/// The API token from standard input: piped, or typed at a hidden prompt when stdin is a
/// terminal (so it is never echoed). Validation (empty, whitespace) happens in core.
fn read_api_token() -> Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        if !output::is_interactive() {
            return Err(CliError::new(
                "validation_failed",
                "--api-token-stdin reads the token from standard input, which is a terminal; \
                 pipe the token in (for example from a password manager)",
            )
            .into());
        }
        return dialoguer::Password::new()
            .with_prompt("Cloudflare API token")
            .allow_empty_password(true)
            .interact()
            .context("Failed to read the Cloudflare API token");
    }

    let mut bytes = Vec::new();
    stdin
        .lock()
        .take(MAX_TOKEN_INPUT + 1)
        .read_to_end(&mut bytes)
        .context("Failed to read the Cloudflare API token from standard input")?;
    if bytes.len() as u64 > MAX_TOKEN_INPUT {
        return Err(CliError::new(
            "validation_failed",
            format!(
                "Refusing the Cloudflare API token: standard input holds more than \
                 {MAX_TOKEN_INPUT} bytes. Nothing was changed."
            ),
        )
        .into());
    }
    // cause-withheld: the UTF-8 error carries the input bytes, which are the token.
    String::from_utf8(bytes).map_err(|_| {
        CliError::new(
            "validation_failed",
            "Refusing the Cloudflare API token: standard input is not UTF-8 text. Nothing was \
             changed.",
        )
        .into()
    })
}

fn print_credentials_summary(summary: &CredentialsSummary, cleared: bool) {
    if cleared {
        humanln!("🔐 Cloudflare API token removed");
    } else {
        humanln!("🔐 Cloudflare credentials saved");
    }
    humanln!(
        "  File: {} (owner-only)",
        summary.credentials_path.display()
    );
    humanln!(
        "  Account ID: {}",
        summary.account_id.as_deref().unwrap_or("(not set)")
    );
    humanln!(
        "  API token: {}",
        if summary.token_present {
            "stored"
        } else {
            "not stored (tunnels use manual setup instructions)"
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: TunnelCommands,
    }

    fn parse(args: &[&str]) -> Result<TunnelCommands, clap::Error> {
        let mut argv = vec!["tunnel"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).map(|cli| cli.command)
    }

    #[test]
    fn credentials_set_requires_an_account_and_stdin_unless_clearing() {
        let command = parse(&[
            "credentials",
            "set",
            "--account-id",
            "acct",
            "--api-token-stdin",
            "--json",
        ])
        .unwrap();
        assert!(command.wants_json());

        assert!(parse(&["credentials", "set", "--account-id", "acct"]).is_err());
        assert!(parse(&["credentials", "set", "--api-token-stdin"]).is_err());
        assert!(parse(&["credentials", "set", "--clear", "--api-token-stdin"]).is_err());
        let clear = parse(&["credentials", "set", "--clear"]).unwrap();
        assert!(!clear.wants_json());
    }

    #[test]
    fn the_token_is_never_an_argument() {
        assert!(parse(&[
            "credentials",
            "set",
            "--account-id",
            "acct",
            "--api-token",
            "secret"
        ])
        .is_err());
    }

    #[test]
    fn open_and_remove_report_their_json_flag() {
        assert!(parse(&["open", "eta", "--json"]).unwrap().wants_json());
        assert!(!parse(&["remove", "eta"]).unwrap().wants_json());
        assert_eq!(CAPABILITIES, ["tunnel-credentials"]);
    }
}
