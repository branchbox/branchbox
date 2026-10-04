use crate::agent::AgentClient;
use crate::json_error;
use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use clap::{Args, Subcommand};
use worktree_core::{humanln, output};

#[derive(Subcommand)]
pub enum AgentCommands {
    /// Show agent/control-plane status
    Status(AgentStatusArgs),
}

#[derive(Args)]
pub struct AgentStatusArgs {
    /// Emit JSON output
    #[arg(long)]
    pub json: bool,
}

impl AgentCommands {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        match self {
            AgentCommands::Status(args) => args.json,
        }
    }
}

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3).
pub const CAPABILITIES: &[&str] = &[];

pub fn execute(command: AgentCommands) -> Result<()> {
    match command {
        AgentCommands::Status(args) => run_status(args),
    }
}

fn run_status(args: AgentStatusArgs) -> Result<()> {
    // `connect` only fails when no socket can be used at all.
    let client = AgentClient::connect().map_err(agent_unreachable)?;
    let status = client.agent_status().map_err(|err| {
        if is_transport_failure(&err) {
            agent_unreachable(err)
        } else {
            err
        }
    })?;
    if args.json {
        output::emit_json(&status)?;
        return Ok(());
    }

    humanln!(
        "Control plane: {}",
        match (
            status.control_plane_configured,
            status.control_plane_connected
        ) {
            (false, _) => "disabled",
            (true, true) => "connected",
            (true, false) => "degraded",
        }
    );
    if let Some(ack) = status.last_ack_event_id {
        humanln!("Last acked event ID: {ack}");
    }
    if let Some(batch) = status.last_sent_batch_id {
        if let Some(cursor) = status.last_sent_event_id {
            humanln!("Last batch sent: #{batch} (through event {cursor})");
        } else {
            humanln!("Last batch sent: #{batch}");
        }
    } else if let Some(cursor) = status.last_sent_event_id {
        humanln!("Last sent cursor: event {cursor}");
    }
    if let Some(ts) = format_timestamp(status.last_sent_at.as_deref()) {
        humanln!("Last send attempt: {ts}");
    }
    if let Some(ts) = format_timestamp(status.last_delivery_at.as_deref()) {
        humanln!("Last delivery: {ts}");
    }
    if let Some(ts) = format_timestamp(status.last_failure_at.as_deref()) {
        humanln!("Last failure: {ts}");
    }
    if let Some(err) = status.last_error.as_deref() {
        humanln!("Last error: {err}");
    }

    Ok(())
}

/// The envelope code for an agent that cannot be reached. The error still prints exactly as
/// before (`Error: failed to connect to BranchBox agent at …`).
fn agent_unreachable(err: anyhow::Error) -> anyhow::Error {
    json_error::recode(err, "agent_unreachable", None)
}

/// Whether talking to the agent failed at the socket (missing socket, refused or dropped
/// connection) rather than in the agent's own reply, which keeps its code.
fn is_transport_failure(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<std::io::Error>())
}

fn format_timestamp(raw: Option<&str>) -> Option<String> {
    let ts = raw?;
    let parsed: DateTime<Utc> = ts.parse().ok()?;
    Some(
        parsed
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    )
}
