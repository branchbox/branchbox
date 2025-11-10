use crate::{
    config::AgentConfig,
    control_plane::{AgentMetadata, ControlPlaneClient},
    grpc::GrpcServer,
    ipc::IpcServer,
    shutdown::Shutdown,
    state::AgentState,
};
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub struct AgentRuntime {
    config: Arc<AgentConfig>,
}

impl AgentRuntime {
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub async fn run(self) -> Result<()> {
        info!(
            workspace = %self.config.workspace_root.display(),
            state_dir = %self.config.state_dir.display(),
            socket = %self.config.socket_path.display(),
            config = %self.config.config_path.display(),
            heartbeat_secs = self.config.heartbeat_interval.as_secs(),
            "Agent runtime initialized"
        );

        let state = AgentState::initialize(&self.config.state_dir)?;
        let shutdown = Shutdown::new();
        let ipc_server = IpcServer::new(Arc::clone(&self.config), state.clone());
        let shutdown_token = shutdown.subscribe();
        let heartbeat_token = shutdown.subscribe();
        let event_token = shutdown.subscribe();

        let cp_client = match self
            .config
            .control_plane
            .clone()
            .map(ControlPlaneClient::new)
            .transpose()
        {
            Ok(client) => client,
            Err(err) => {
                warn!("Control plane configuration invalid: {err:#}");
                None
            }
        };

        let agent_meta = Arc::new(AgentMetadata::detect());
        let heartbeat_state = state.clone();
        let heartbeat_handle = tokio::spawn(heartbeat_loop(
            heartbeat_state,
            Arc::clone(&self.config),
            heartbeat_token,
        ));

        let event_state = state.clone();
        let event_handle = tokio::spawn(event_loop(
            event_state,
            Arc::clone(&self.config),
            cp_client.clone(),
            Arc::clone(&agent_meta),
            event_token,
        ));

        let grpc_handle = if self.config.grpc_enabled {
            let grpc_server = GrpcServer::new(Arc::clone(&self.config), state.clone());
            let addr = self.config.grpc_addr;
            Some(tokio::spawn(async move {
                if let Err(err) = grpc_server.serve(addr).await {
                    warn!("gRPC server exited with error: {err:#}");
                }
            }))
        } else {
            None
        };

        let mut server_handle: JoinHandle<Result<()>> =
            tokio::spawn(async move { ipc_server.serve(shutdown_token).await });

        tokio::select! {
            result = &mut server_handle => {
                shutdown.cancel();
                result??;
                info!("IPC server exited");
                if let Err(err) = heartbeat_handle.await {
                    if err.is_cancelled() {
                        warn!("Heartbeat loop cancelled");
                    } else if err.is_panic() {
                        warn!("Heartbeat loop panicked: {err:?}");
                    }
                }
                if let Err(err) = event_handle.await {
                    if err.is_cancelled() {
                        warn!("Event loop cancelled");
                    } else if err.is_panic() {
                        warn!("Event loop panicked: {err:?}");
                    }
                }
                if let Some(handle) = grpc_handle {
                    handle.abort();
                }
                return Ok(());
            }
            _ = shutdown.wait() => {
                info!("Shutdown signal received");
            }
        }

        match server_handle.await {
            Ok(result) => result.context("IPC server failed during shutdown")?,
            Err(err) => {
                if err.is_cancelled() {
                    warn!("IPC server task cancelled");
                } else {
                    return Err(err).context("IPC server panicked");
                }
            }
        }

        if let Err(err) = heartbeat_handle.await {
            if err.is_cancelled() {
                warn!("Heartbeat loop cancelled");
            } else if err.is_panic() {
                warn!("Heartbeat loop panicked: {err:?}");
            }
        }

        if let Err(err) = event_handle.await {
            if err.is_cancelled() {
                warn!("Event loop cancelled");
            } else if err.is_panic() {
                warn!("Event loop panicked: {err:?}");
            }
        }

        if let Some(handle) = grpc_handle {
            handle.abort();
        }

        info!("Agent runtime stopped");
        Ok(())
    }
}

async fn heartbeat_loop(
    state: AgentState,
    config: Arc<AgentConfig>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let mut ticker = interval(config.heartbeat_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    info!(
        "Heartbeat loop started (interval={}s)",
        config.heartbeat_interval.as_secs()
    );

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                info!("Heartbeat loop shutting down");
                break;
            }
            _ = ticker.tick() => {
                if let Err(err) = state.enqueue_heartbeat(&config.workspace_root).await {
                    warn!("Failed to enqueue heartbeat: {err:#}");
                }
            }
        }
    }
}

async fn event_loop(
    state: AgentState,
    config: Arc<AgentConfig>,
    cp_client: Option<ControlPlaneClient>,
    agent_meta: Arc<AgentMetadata>,
    shutdown: CancellationToken,
) {
    let mut ticker = interval(config.event_flush_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    info!(
        "Event loop started (interval={}s batch={} log_only={})",
        config.event_flush_interval.as_secs(),
        config.event_batch_size,
        config.event_log_only
    );

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                info!("Event loop shutting down");
                break;
            }
            _ = ticker.tick() => {
                match state.dequeue_events(config.event_batch_size).await {
                    Ok(events) => {
                        if events.is_empty() {
                            continue;
                        }

                        if config.event_log_only || cp_client.is_none() {
                            for event in &events {
                                info!(
                                    "Pending event [{}] kind={} queued_at={} payload={}",
                                    event.id,
                                    event.event_type,
                                    event.queued_at,
                                    event.payload
                                );
                            }
                        }

                        let mut delivered = false;
                        if let Some(client) = cp_client.as_ref() {
                            match client
                                .send_events(&config.workspace_root, agent_meta.as_ref(), &events)
                                .await
                            {
                                Ok(_) => {
                                    let ids: Vec<i64> =
                                        events.iter().map(|event| event.id).collect();
                                    if let Err(err) = state.mark_events_delivered(&ids).await {
                                        warn!("Failed to mark events delivered: {err:#}");
                                    } else {
                                        delivered = true;
                                    }
                                }
                                Err(err) => {
                                    warn!("Failed to deliver events to control plane: {err:#}");
                                }
                            }
                        }

                        if !delivered {
                            if config.event_log_only || cp_client.is_none() {
                                let ids: Vec<i64> =
                                    events.iter().map(|event| event.id).collect();
                                if let Err(err) = state.mark_events_delivered(&ids).await {
                                    warn!("Failed to mark logged events delivered: {err:#}");
                                }
                            } else {
                                debug!(
                                    "Retaining {} events pending control-plane delivery",
                                    events.len()
                                );
                            }
                        }
                    }
                    Err(err) => {
                        warn!("Failed to dequeue events: {err:#}");
                    }
                }
            }
        }
    }
}
