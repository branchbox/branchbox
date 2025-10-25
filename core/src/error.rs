//! Error types for worktree-core

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias using our Error type
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for worktree operations
#[derive(Error, Debug)]
pub enum Error {
    /// Git-related errors
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP errors
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Validation errors
    #[error("Validation error: {0}")]
    Validation(String),

    /// Worktree already exists
    #[error("Worktree already exists at: {0}")]
    WorktreeExists(PathBuf),

    /// Worktree not found
    #[error("Worktree not found: {0}")]
    WorktreeNotFound(String),

    /// Branch already exists
    #[error("Branch already exists: {0}")]
    BranchExists(String),

    /// Invalid feature name
    #[error("Invalid feature name: {0}")]
    InvalidFeatureName(String),

    /// Environment variable not set
    #[error("Environment variable not set: {0}")]
    EnvVarNotSet(String),

    /// Command execution failed
    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    /// Adapter not found
    #[error("No adapter found for stack")]
    AdapterNotFound,

    /// Module error
    #[error("Module error: {0}")]
    Module(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Other errors
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Create a git command error
    pub fn git(msg: impl Into<String>) -> Self {
        Self::CommandFailed(msg.into())
    }

    /// Create a validation error
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    /// Create a module error
    pub fn module(msg: impl Into<String>) -> Self {
        Self::Module(msg.into())
    }

    /// Create a config error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create an other error
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
