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
//! - [`git`] - Git worktree operations
//! - [`error`] - Error types

pub mod adapters;
pub mod error;
pub mod git;
pub mod modules;
pub mod naming;
pub mod validation;

pub use error::{Error, Result};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
