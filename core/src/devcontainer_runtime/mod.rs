//! Devcontainer Runtime
//!
//! This module provides functionality equivalent to the `@devcontainers/cli` npm package,
//! allowing you to create, manage, and execute commands in devcontainers from Rust.
//!
//! # Overview
//!
//! The devcontainer runtime supports three types of container configurations:
//!
//! - **Image-based**: Uses a pre-built Docker image (`image` field)
//! - **Dockerfile-based**: Builds from a Dockerfile (`build.dockerfile` field)
//! - **Docker Compose**: Uses Docker Compose for multi-container setups (`dockerComposeFile` field)
//!
//! # Example
//!
//! ```no_run
//! use worktree_core::devcontainer_runtime::{DevcontainerRuntime, UpOptions};
//! use std::path::Path;
//!
//! // Create a runtime for a workspace
//! let runtime = DevcontainerRuntime::new(Path::new("/path/to/workspace")).unwrap();
//!
//! // Start the devcontainer
//! let result = runtime.up(UpOptions::default()).unwrap();
//! println!("Container ID: {}", result.container_id);
//!
//! // Execute a command
//! let exec_result = runtime.exec(
//!     &["cargo".to_string(), "build".to_string()],
//!     Default::default()
//! ).unwrap();
//! println!("Exit code: {}", exec_result.exit_code);
//!
//! // Stop the devcontainer
//! runtime.down(Default::default()).unwrap();
//! ```
//!
//! # Configuration
//!
//! The runtime reads `devcontainer.json` from:
//! 1. `.devcontainer/devcontainer.json` (preferred)
//! 2. `.devcontainer.json` (fallback)
//!
//! # Environment Variables
//!
//! - `DOCKER_PATH`: Path to the Docker CLI (default: `docker`)
//! - `DOCKER_COMPOSE_PATH`: Path to Docker Compose CLI (default: `docker compose`)

pub mod config;
pub mod docker;
pub mod runtime;

pub use config::{
    BuildConfig, ComposeFileRef, DevcontainerConfig, DevcontainerType, LifecycleCommand,
    MountConfig, PortForward,
};
pub use docker::{ContainerInfo, ContainerState, Docker, DockerOutput};
pub use runtime::{
    BuildResult, DevcontainerRuntime, DownOptions, DownResult, ExecOptions, ExecResult,
    ReadConfigResult, UpOptions, UpResult,
};
