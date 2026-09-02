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

    /// A managed request spool is healthy but its atomic final request file is not present yet.
    #[error("Tool request is not pending: {lease_id}/{request_id}")]
    ToolRequestNotPending {
        lease_id: String,
        request_id: String,
    },

    /// A trusted endpoint may have executed the exact request, but its correlated response was
    /// lost before BranchBox could persist it. Only the digest-bound replay record may retry it.
    #[error("Tool request relay needs an exact retry: {lease_id}/{request_id}: {reason}")]
    ToolRequestRelayRetryable {
        lease_id: String,
        request_id: String,
        reason: String,
    },

    /// Worktree contains uncommitted module-managed changes
    #[error("Worktree has module-managed changes: {worktree:?} (dirty entries: {files:?})")]
    WorktreeDirty {
        /// Path to the worktree that is dirty
        worktree: PathBuf,
        /// File paths with module-managed changes that triggered the block
        files: Vec<String>,
    },

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

impl From<dialoguer::Error> for Error {
    fn from(err: dialoguer::Error) -> Self {
        Self::Other(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error() {
        let err = Error::validation("test message");
        assert!(matches!(err, Error::Validation(_)));
        assert_eq!(err.to_string(), "Validation error: test message");
    }

    #[test]
    fn test_git_error() {
        let err = Error::git("git command failed");
        assert!(matches!(err, Error::CommandFailed(_)));
        assert_eq!(
            err.to_string(),
            "Command execution failed: git command failed"
        );
    }

    #[test]
    fn test_module_error() {
        let err = Error::module("module failed");
        assert!(matches!(err, Error::Module(_)));
        assert_eq!(err.to_string(), "Module error: module failed");
    }

    #[test]
    fn test_config_error() {
        let err = Error::config("config invalid");
        assert!(matches!(err, Error::Config(_)));
        assert_eq!(err.to_string(), "Configuration error: config invalid");
    }

    #[test]
    fn test_other_error() {
        let err = Error::other("something else");
        assert!(matches!(err, Error::Other(_)));
        assert_eq!(err.to_string(), "something else");
    }

    #[test]
    fn test_worktree_exists_error() {
        let path = PathBuf::from("/tmp/test");
        let err = Error::WorktreeExists(path.clone());
        assert!(err.to_string().contains("/tmp/test"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }
}
