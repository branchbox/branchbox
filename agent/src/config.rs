use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

const DEFAULT_HEARTBEAT_SECS: u64 = 30;
const DEFAULT_SOCKET_NAME: &str = "branchbox-agent.sock";
const DEFAULT_GRPC_ADDR: &str = "127.0.0.1:50515";
const DEFAULT_EVENT_FLUSH_SECS: u64 = 10;
const DEFAULT_EVENT_BATCH_SIZE: usize = 50;

/// Fully-resolved agent configuration used by the runtime.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub config_path: PathBuf,
    pub workspace_root: PathBuf,
    pub state_dir: PathBuf,
    pub socket_path: PathBuf,
    pub heartbeat_interval: Duration,
    pub grpc_enabled: bool,
    pub grpc_addr: SocketAddr,
    pub event_flush_interval: Duration,
    pub event_batch_size: usize,
    pub event_log_only: bool,
    pub control_plane: Option<ControlPlaneConfig>,
}

#[derive(Debug, Clone)]
pub struct ControlPlaneConfig {
    pub endpoint: String,
    pub api_token: String,
    pub verify_tls: bool,
}

impl AgentConfig {
    /// Load configuration from `BRANCHBOX_AGENT_CONFIG` or the default path.
    pub fn load() -> Result<Self> {
        let config_path = detect_config_path()?;
        let content = fs::read_to_string(&config_path)
            .map(Some)
            .or_else(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(err)
                }
            })
            .with_context(|| format!("Failed reading {}", config_path.display()))?;

        if content.is_none() {
            info!(
                "Config file {} missing; using defaults",
                config_path.display()
            );
        }

        let file_config: FileConfig = content
            .as_deref()
            .map(toml::from_str)
            .unwrap_or_else(|| Ok(FileConfig::default()))?;

        let workspace_root = file_config
            .workspace_root
            .unwrap_or_else(|| current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .clean_path();

        let state_dir = file_config
            .state_dir
            .unwrap_or_else(default_state_dir)
            .clean_path();

        let socket_path = file_config
            .socket_path
            .unwrap_or_else(|| state_dir.join(DEFAULT_SOCKET_NAME));

        let heartbeat_interval = Duration::from_secs(
            file_config
                .heartbeat_interval_secs
                .unwrap_or(DEFAULT_HEARTBEAT_SECS)
                .max(5),
        );

        let grpc_enabled = file_config.grpc_enabled.unwrap_or(true);
        let grpc_addr = parse_grpc_addr(
            file_config
                .grpc_addr
                .or_else(|| env::var("BRANCHBOX_AGENT_GRPC_ADDR").ok()),
        );

        let event_flush_interval = Duration::from_secs(
            file_config
                .event_flush_interval_secs
                .unwrap_or(DEFAULT_EVENT_FLUSH_SECS)
                .max(1),
        );
        let event_batch_size = file_config
            .event_batch_size
            .unwrap_or(DEFAULT_EVENT_BATCH_SIZE)
            .max(1);
        let control_plane = resolve_control_plane(file_config.control_plane.as_ref())?;
        let mut event_log_only = file_config.event_log_only.unwrap_or(true);
        if control_plane.is_some() && file_config.event_log_only.is_none() {
            event_log_only = false;
        }

        fs::create_dir_all(&state_dir).with_context(|| {
            format!(
                "Failed to create agent state directory {}",
                state_dir.display()
            )
        })?;

        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create socket parent directory {}",
                    parent.display()
                )
            })?;
        }

        debug!(
            workspace = %workspace_root.display(),
            state_dir = %state_dir.display(),
            socket = %socket_path.display(),
            heartbeat_secs = heartbeat_interval.as_secs(),
            "Agent configuration loaded"
        );

        Ok(Self {
            config_path,
            workspace_root,
            state_dir,
            socket_path,
            heartbeat_interval,
            grpc_enabled,
            grpc_addr,
            event_flush_interval,
            event_batch_size,
            event_log_only,
            control_plane,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    workspace_root: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    socket_path: Option<PathBuf>,
    heartbeat_interval_secs: Option<u64>,
    grpc_enabled: Option<bool>,
    grpc_addr: Option<String>,
    event_flush_interval_secs: Option<u64>,
    event_batch_size: Option<usize>,
    event_log_only: Option<bool>,
    control_plane: Option<ControlPlaneFileConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct ControlPlaneFileConfig {
    enabled: Option<bool>,
    endpoint: Option<String>,
    api_token: Option<String>,
    verify_tls: Option<bool>,
}

fn detect_config_path() -> Result<PathBuf> {
    if let Ok(path) = env::var("BRANCHBOX_AGENT_CONFIG") {
        return Ok(PathBuf::from(path));
    }

    let default_dir = default_state_dir();
    Ok(default_dir.join("agent.toml"))
}

fn default_state_dir() -> PathBuf {
    if let Some(dir) = env::var_os("BRANCHBOX_AGENT_DIR") {
        return PathBuf::from(dir);
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".branchbox")
        .join("agent")
}

fn parse_grpc_addr(value: Option<String>) -> SocketAddr {
    let addr_str = value.unwrap_or_else(|| DEFAULT_GRPC_ADDR.to_string());
    addr_str.parse().unwrap_or_else(|err| {
        warn!(
            "Invalid gRPC address '{}': {}. Falling back to {}",
            addr_str, err, DEFAULT_GRPC_ADDR
        );
        DEFAULT_GRPC_ADDR
            .parse()
            .expect("default gRPC address parses")
    })
}

fn resolve_control_plane(
    file_cfg: Option<&ControlPlaneFileConfig>,
) -> Result<Option<ControlPlaneConfig>> {
    let env_endpoint = env::var("BRANCHBOX_CP_ENDPOINT").ok();
    let env_token = env::var("BRANCHBOX_CP_TOKEN").ok();
    let env_verify = env::var("BRANCHBOX_CP_VERIFY_TLS").ok();

    let endpoint = env_endpoint.or_else(|| file_cfg.and_then(|cfg| cfg.endpoint.clone()));
    let token = env_token.or_else(|| file_cfg.and_then(|cfg| cfg.api_token.clone()));

    let verify_tls = env_verify
        .as_deref()
        .map(parse_bool_str)
        .or_else(|| file_cfg.and_then(|cfg| cfg.verify_tls))
        .unwrap_or(true);

    let enabled = file_cfg.and_then(|cfg| cfg.enabled).unwrap_or(false)
        || endpoint.is_some()
        || token.is_some();

    if !enabled {
        return Ok(None);
    }

    let Some(endpoint) = endpoint else {
        warn!("Control plane enabled but endpoint missing");
        return Ok(None);
    };

    let Some(api_token) = token else {
        warn!("Control plane enabled but api_token missing");
        return Ok(None);
    };

    Ok(Some(ControlPlaneConfig {
        endpoint,
        api_token,
        verify_tls,
    }))
}

fn parse_bool_str(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn current_dir() -> Result<PathBuf> {
    std::env::current_dir().context("Unable to determine current working directory")
}

trait CleanPath {
    fn clean_path(self) -> PathBuf;
}

impl CleanPath for PathBuf {
    fn clean_path(self) -> PathBuf {
        std::fs::canonicalize(&self).unwrap_or(self)
    }
}
