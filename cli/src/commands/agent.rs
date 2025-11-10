use crate::agent::AgentClient;
use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use clap::{Args, Subcommand};

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

pub fn execute(command: AgentCommands) -> Result<()> {
    match command {
        AgentCommands::Status(args) => run_status(args),
    }
}

fn run_status(args: AgentStatusArgs) -> Result<()> {
    let client = AgentClient::connect()?;
    let status = client.agent_status()?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!(
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
        println!("Last acked event ID: {ack}");
    }
    if let Some(ts) = format_timestamp(status.last_delivery_at.as_deref()) {
        println!("Last delivery: {ts}");
    }
    if let Some(ts) = format_timestamp(status.last_failure_at.as_deref()) {
        println!("Last failure: {ts}");
    }
    if let Some(err) = status.last_error.as_deref() {
        println!("Last error: {err}");
    }

    Ok(())
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
