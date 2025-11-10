use crate::config::ControlPlaneConfig;
use crate::state::PendingEvent;
use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use reqwest::{header, Client};
use serde::Serialize;
use std::path::Path;
use tracing::info;

static USER_AGENT: Lazy<header::HeaderValue> =
    Lazy::new(|| header::HeaderValue::from_static("BranchBox-Agent/0.3"));

#[derive(Clone)]
pub struct ControlPlaneClient {
    config: ControlPlaneConfig,
    http: Client,
}

impl ControlPlaneClient {
    pub fn new(config: ControlPlaneConfig) -> Result<Self> {
        let mut builder = Client::builder();
        if !config.verify_tls {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let http = builder
            .user_agent(USER_AGENT.clone())
            .build()
            .context("Failed to construct reqwest client")?;

        Ok(Self { config, http })
    }

    pub async fn send_events(&self, workspace_root: &Path, events: &[PendingEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let payload = ControlPlanePayload {
            workspace_root: workspace_root.display().to_string(),
            events: events.iter().map(EventPayload::from).collect(),
        };

        let response = self
            .http
            .post(&self.config.endpoint)
            .bearer_auth(&self.config.api_token)
            .json(&payload)
            .send()
            .await
            .context("Failed to send events to control plane")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Control plane responded with {}: {}", status, body));
        }

        info!(
            "Delivered {} queued events to control plane ({})",
            events.len(),
            self.config.endpoint
        );
        Ok(())
    }
}

#[derive(Serialize)]
struct ControlPlanePayload {
    workspace_root: String,
    events: Vec<EventPayload>,
}

#[derive(Serialize)]
struct EventPayload {
    id: i64,
    kind: String,
    queued_at: String,
    payload: serde_json::Value,
}

impl From<&PendingEvent> for EventPayload {
    fn from(event: &PendingEvent) -> Self {
        EventPayload {
            id: event.id,
            kind: event.event_type.clone(),
            queued_at: event.queued_at.to_rfc3339(),
            payload: event.payload.clone(),
        }
    }
}
