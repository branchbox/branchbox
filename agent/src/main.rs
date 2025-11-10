mod config;
mod control_plane;
mod grpc;
mod ipc;
mod ops;
mod runtime;
mod shutdown;
mod state;

use anyhow::Result;
use runtime::AgentRuntime;
use tracing::{info, warn};
use tracing_subscriber::fmt::try_init;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;
    info!("Starting BranchBox agent");

    let config = config::AgentConfig::load()?;
    let runtime = AgentRuntime::new(config);
    runtime.run().await?;

    info!("BranchBox agent shutting down");
    Ok(())
}

fn init_tracing() -> Result<()> {
    if try_init().is_err() {
        warn!("Tracing subscriber already initialized; continuing");
    }
    Ok(())
}
