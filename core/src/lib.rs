//! Worktree Core Library
//!
//! Provides core functionality for git worktree and devcontainer orchestration.
//!
//! # Modules
//!
//! - [`naming`] - Generate DNS-safe, dasherized feature names
//! - [`validation`] - Validate environment, git state, and configuration
//! - [`adapters`] - Auto-detect and configure for different stacks
//! - [`modules`] - Composable feature components
//! - [`bootstrap`] - Self-bootstrapping devcontainer system (meta!)
//! - [`git`] - Git worktree operations
//! - [`devcontainer_runtime`] - Devcontainer lifecycle management (up, exec, down, build)
//! - [`error`] - Error types

pub mod adapters;
pub mod bootstrap;
pub mod cloudflare;
pub mod config;
pub mod devcontainer_runtime;
pub mod error;
pub mod git;
pub mod modules;
pub mod naming;
pub mod tunnel;
pub mod validation;
pub mod workflow;
pub mod workflows;

pub use error::{Error, Result};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
