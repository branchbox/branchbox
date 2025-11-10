use crate::config::ControlPlaneConfig;
use crate::state::PendingEvent;
use anyhow::{anyhow, Context, Result};
use hostname::get;
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

    pub async fn send_events(
        &self,
        workspace_root: &Path,
        agent: &AgentMetadata,
        events: &[PendingEvent],
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let payload = ControlPlanePayload {
            workspace_root: workspace_root.display().to_string(),
            agent,
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
struct ControlPlanePayload<'a> {
    workspace_root: String,
    agent: &'a AgentMetadata,
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

#[derive(Clone, Serialize)]
pub struct AgentMetadata {
    pub version: &'static str,
    pub hostname: String,
    pub os: String,
    pub arch: String,
}

impl AgentMetadata {
    pub fn detect() -> Self {
        let hostname = get()
            .ok()
            .and_then(|value| value.into_string().ok())
            .unwrap_or_else(|| "unknown-host".to_string());

        Self {
            version: env!("CARGO_PKG_VERSION"),
            hostname,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_includes_agent_metadata() {
        let agent = AgentMetadata {
            version: "1.0.0",
            hostname: "devbox".to_string(),
            os: "macos".to_string(),
            arch: "arm64".to_string(),
        };

        let payload = ControlPlanePayload {
            workspace_root: "/tmp/repo".to_string(),
            agent: &agent,
            events: vec![EventPayload {
                id: 1,
                kind: "heartbeat".to_string(),
                queued_at: "2024-05-01T00:00:00Z".to_string(),
                payload: json!({"event": "heartbeat"}),
            }],
        };

        let serialized = serde_json::to_value(&payload).expect("payload serializes");
        assert_eq!(serialized["agent"]["hostname"], "devbox");
        assert_eq!(serialized["workspace_root"], "/tmp/repo");
        assert_eq!(serialized["events"].as_array().unwrap().len(), 1);
    }
}
