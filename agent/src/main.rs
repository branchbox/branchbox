#[cfg(unix)]
mod config;
#[cfg(unix)]
mod control_plane;
#[cfg(unix)]
mod grpc;
#[cfg(unix)]
mod ipc;
#[cfg(unix)]
mod ops;
#[cfg(unix)]
mod runtime;
#[cfg(unix)]
mod shutdown;
#[cfg(unix)]
mod state;

#[cfg(not(unix))]
fn main() {
    eprintln!("BranchBox agent currently supports Unix-like hosts only.");
}

#[cfg(unix)]
use anyhow::Result;
#[cfg(unix)]
use runtime::AgentRuntime;
#[cfg(unix)]
use tracing::{info, warn};
#[cfg(unix)]
use tracing_subscriber::fmt::try_init;

#[cfg(unix)]
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

#[cfg(unix)]
fn init_tracing() -> Result<()> {
    if try_init().is_err() {
        warn!("Tracing subscriber already initialized; continuing");
    }
    Ok(())
}
