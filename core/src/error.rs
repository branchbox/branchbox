//! Error types for worktree-core
//!
//! Every variant maps to a stable machine-readable code ([`Error::code`]) and optional structured
//! details ([`Error::details`]); `--json` failures carry both in the error envelope
//! (DESIGN §5.2). Codes and detail keys are a public contract: add new ones, never rename.

use crate::workflows::teardown_plan::TeardownPlan;
use serde_json::{json, Value};
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

    /// The feature's worktree directory does not exist. Displays exactly as 0.13.4's teardown
    /// reported it (`Worktree not found: <path>`), and also carries the feature name.
    #[error("Worktree not found: {}", .path.display())]
    WorktreeMissing { name: String, path: PathBuf },

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

    /// Worktree contains uncommitted module-managed changes.
    ///
    /// Legacy: kept for callers that predate the plan-first teardown, which reports
    /// [`Error::TeardownRefused`] instead. Both map to the `teardown_refused` code.
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

    /// Teardown refused; `plan` says why. Unless `changed_anything` is set, nothing was changed.
    /// When it is set (new user changes appeared after the runtime and modules were already
    /// stopped), `completed_steps` lists what ran; the worktree and registry entry are kept.
    #[error("{message}")]
    TeardownRefused {
        work_feature: String,
        /// Cause-naming refusal text, including the flags that override it.
        message: String,
        plan: Box<TeardownPlan>,
        changed_anything: bool,
        completed_steps: Vec<String>,
    },

    /// Another process held the `.branchbox` state-directory lock for the whole wait.
    #[error(
        "Timed out after {waited_secs}s waiting for the BranchBox registry lock on {}: another \
         branchbox process is changing this project's registry. The lock is released when that \
         process exits; retry once it has finished.",
        .path.display()
    )]
    RegistryLocked { path: PathBuf, waited_secs: u64 },

    /// The path is not inside a git repository.
    #[error("Validation error: Not a git repository: {}", .0.display())]
    NotAGitRepository(PathBuf),

    /// The feature has no entry in the project's registry.
    #[error("Feature '{name}' is not registered in {}", .registry.display())]
    FeatureNotFound { name: String, registry: PathBuf },

    /// A configuration value or file is invalid; the optional fields locate the problem.
    #[error("Configuration error: {message}")]
    ConfigInvalid {
        message: String,
        key: Option<String>,
        line: Option<usize>,
        column: Option<usize>,
        /// The accepted values or format, e.g. `one of: container, sbx`.
        expected: Option<String>,
    },

    /// A configuration key that BranchBox does not know.
    #[error(
        "Configuration error: unknown key '{key}'. Run `branchbox config get` to list the \
         supported keys."
    )]
    ConfigUnknownKey { key: String },
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

    /// The stable code clients branch on (DESIGN §5.2).
    pub fn code(&self) -> &'static str {
        match self {
            Self::TeardownRefused { .. } | Self::WorktreeDirty { .. } => "teardown_refused",
            Self::WorktreeNotFound(_) | Self::WorktreeMissing { .. } => "worktree_not_found",
            Self::FeatureNotFound { .. } => "feature_not_found",
            Self::InvalidFeatureName(_) => "invalid_feature_name",
            Self::WorktreeExists(_) => "worktree_exists",
            Self::BranchExists(_) => "branch_exists",
            Self::NotAGitRepository(_) => "not_a_git_repository",
            Self::Validation(_) => "validation_failed",
            Self::Config(_) | Self::ConfigInvalid { .. } => "config_invalid",
            Self::ConfigUnknownKey { .. } => "config_unknown_key",
            Self::RegistryLocked { .. } => "registry_locked",
            Self::Git(_) => "git_failed",
            Self::Io(_) => "io_error",
            Self::CommandFailed(_) => "command_failed",
            Self::Module(_) => "module_failed",
            Self::EnvVarNotSet(_) => "env_var_not_set",
            Self::AdapterNotFound => "adapter_not_found",
            Self::Http(_)
            | Self::Json(_)
            | Self::ToolRequestNotPending { .. }
            | Self::ToolRequestRelayRetryable { .. }
            | Self::Other(_) => "internal",
        }
    }

    /// Structured details for the error envelope, or `None` when the code carries none.
    pub fn details(&self) -> Option<Value> {
        match self {
            Self::TeardownRefused {
                plan,
                changed_anything,
                completed_steps,
                ..
            } => Some(json!({
                // A plan only fails to serialize on a non-UTF-8 path; the envelope still goes out.
                "plan": serde_json::to_value(plan.as_ref()).unwrap_or(Value::Null),
                "changed_anything": changed_anything,
                "completed_steps": completed_steps,
            })),
            // The legacy refusal predates plans; it still names the files it refused over.
            Self::WorktreeDirty { worktree, files } => Some(json!({
                "plan": null,
                "changed_anything": false,
                "completed_steps": [],
                "worktree": display_path(worktree),
                "files": files,
            })),
            Self::WorktreeNotFound(name) | Self::InvalidFeatureName(name) => {
                Some(json!({ "name": name }))
            }
            Self::WorktreeMissing { name, path } => {
                Some(json!({ "name": name, "path": display_path(path) }))
            }
            Self::FeatureNotFound { name, registry } => {
                Some(json!({ "name": name, "registry": display_path(registry) }))
            }
            Self::WorktreeExists(path) | Self::NotAGitRepository(path) => {
                Some(json!({ "path": display_path(path) }))
            }
            Self::BranchExists(branch) => Some(json!({ "branch": branch })),
            Self::RegistryLocked { path, waited_secs } => {
                Some(json!({ "path": display_path(path), "waited_secs": waited_secs }))
            }
            Self::ConfigInvalid {
                key,
                line,
                column,
                expected,
                ..
            } => {
                let mut details = serde_json::Map::new();
                if let Some(key) = key {
                    details.insert("key".to_string(), json!(key));
                }
                if let Some(line) = line {
                    details.insert("line".to_string(), json!(line));
                }
                if let Some(column) = column {
                    details.insert("column".to_string(), json!(column));
                }
                if let Some(expected) = expected {
                    details.insert("expected".to_string(), json!(expected));
                }
                Some(Value::Object(details))
            }
            Self::ConfigUnknownKey { key } => Some(json!({ "key": key })),
            _ => None,
        }
    }
}

/// Paths in details are display strings: serde rejects non-UTF-8 paths, and an envelope must not
/// fail to serialize.
fn display_path(path: &std::path::Path) -> String {
    path.display().to_string()
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

    fn sample_plan() -> TeardownPlan {
        use crate::workflows::teardown_plan::{ChangeSet, TeardownDefaults, WorktreeState};
        TeardownPlan {
            schema_version: 1,
            work_feature: "eta".to_string(),
            registered: true,
            status: None,
            worktree: WorktreeState {
                path: PathBuf::from("/r/eta"),
                exists: true,
                locked: false,
                lock_reason: None,
            },
            changes: ChangeSet {
                status_available: true,
                truncated: false,
                user: Vec::new(),
                generated: Vec::new(),
                preserved: Vec::new(),
            },
            branch: None,
            defaults: TeardownDefaults {
                delete_branch_by_default: true,
                force_delete_unmerged_by_default: false,
            },
            runtime: None,
            tunnel: None,
            blockers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn not_a_git_repository_display_matches_the_legacy_validation_text() {
        let path = PathBuf::from("/tmp/not a repo");
        let legacy = Error::validation(format!("Not a git repository: {}", path.display()));
        let err = Error::NotAGitRepository(path);
        assert_eq!(err.to_string(), legacy.to_string());
        assert_eq!(err.code(), "not_a_git_repository");
        assert_eq!(err.details(), Some(json!({"path": "/tmp/not a repo"})));
    }

    #[test]
    fn registry_locked_names_the_path_and_how_it_clears() {
        let err = Error::RegistryLocked {
            path: PathBuf::from("/r/main/.branchbox"),
            waited_secs: 30,
        };
        let message = err.to_string();
        assert!(message.contains("/r/main/.branchbox"), "{message}");
        assert!(message.contains("30s"), "{message}");
        assert!(
            message.contains("released when that process exits"),
            "{message}"
        );
        assert_eq!(err.code(), "registry_locked");
        assert_eq!(
            err.details(),
            Some(json!({"path": "/r/main/.branchbox", "waited_secs": 30}))
        );
    }

    #[test]
    fn teardown_refused_displays_its_message_and_carries_the_plan() {
        let err = Error::TeardownRefused {
            work_feature: "eta".to_string(),
            message: "Refusing to tear down 'eta'; nothing was removed.".to_string(),
            plan: Box::new(sample_plan()),
            changed_anything: false,
            completed_steps: Vec::new(),
        };
        assert_eq!(
            err.to_string(),
            "Refusing to tear down 'eta'; nothing was removed."
        );
        assert_eq!(err.code(), "teardown_refused");
        let details = err.details().unwrap();
        assert_eq!(
            details["plan"],
            serde_json::to_value(sample_plan()).unwrap()
        );
        assert_eq!(details["changed_anything"], json!(false));
        assert_eq!(details["completed_steps"], json!([]));
    }

    #[test]
    fn legacy_worktree_dirty_is_a_teardown_refusal_without_a_plan() {
        let err = Error::WorktreeDirty {
            worktree: PathBuf::from("/r/eta"),
            files: vec![".devcontainer/devcontainer.json".to_string()],
        };
        assert_eq!(err.code(), "teardown_refused");
        assert_eq!(
            err.details(),
            Some(json!({
                "plan": null,
                "changed_anything": false,
                "completed_steps": [],
                "worktree": "/r/eta",
                "files": [".devcontainer/devcontainer.json"],
            }))
        );
    }

    #[test]
    fn worktree_missing_keeps_the_legacy_text_and_names_the_feature() {
        let err = Error::WorktreeMissing {
            name: "eta".to_string(),
            path: PathBuf::from("/r/eta"),
        };
        assert_eq!(
            err.to_string(),
            Error::WorktreeNotFound("/r/eta".to_string()).to_string()
        );
        assert_eq!(err.code(), "worktree_not_found");
        assert_eq!(
            err.details(),
            Some(json!({"name": "eta", "path": "/r/eta"}))
        );
    }

    #[test]
    fn feature_not_found_names_the_registry() {
        let err = Error::FeatureNotFound {
            name: "nope".to_string(),
            registry: PathBuf::from("/r/main/.branchbox/registry.json"),
        };
        assert_eq!(
            err.to_string(),
            "Feature 'nope' is not registered in /r/main/.branchbox/registry.json"
        );
        assert_eq!(err.code(), "feature_not_found");
        assert_eq!(
            err.details(),
            Some(json!({"name": "nope", "registry": "/r/main/.branchbox/registry.json"}))
        );
    }

    #[test]
    fn config_errors_carry_only_the_known_location_fields() {
        let err = Error::ConfigInvalid {
            message: "runtime.provider must be one of: container, sbx".to_string(),
            key: Some("runtime.provider".to_string()),
            line: None,
            column: None,
            expected: Some("container, sbx".to_string()),
        };
        assert_eq!(
            err.to_string(),
            "Configuration error: runtime.provider must be one of: container, sbx"
        );
        assert_eq!(err.code(), "config_invalid");
        assert_eq!(
            err.details(),
            Some(json!({"key": "runtime.provider", "expected": "container, sbx"}))
        );

        let located = Error::ConfigInvalid {
            message: "comments are not supported".to_string(),
            key: None,
            line: Some(3),
            column: Some(5),
            expected: None,
        };
        assert_eq!(located.details(), Some(json!({"line": 3, "column": 5})));

        let unknown = Error::ConfigUnknownKey {
            key: "runtime.nope".to_string(),
        };
        assert!(unknown.to_string().contains("'runtime.nope'"));
        assert_eq!(unknown.code(), "config_unknown_key");
        assert_eq!(unknown.details(), Some(json!({"key": "runtime.nope"})));

        assert_eq!(Error::config("bad").code(), "config_invalid");
        assert_eq!(Error::config("bad").details(), None);
    }

    #[test]
    fn every_variant_maps_to_its_contract_code() {
        let json_err = serde_json::from_str::<Value>("{").unwrap_err();
        let cases: Vec<(Error, &str, Option<Value>)> = vec![
            (
                Error::Git(git2::Error::from_str("boom")),
                "git_failed",
                None,
            ),
            (Error::Io(std::io::Error::other("disk")), "io_error", None),
            (Error::Json(json_err), "internal", None),
            (Error::validation("bad"), "validation_failed", None),
            (
                Error::WorktreeExists(PathBuf::from("/r/eta")),
                "worktree_exists",
                Some(json!({"path": "/r/eta"})),
            ),
            (
                Error::WorktreeNotFound("eta".to_string()),
                "worktree_not_found",
                Some(json!({"name": "eta"})),
            ),
            (
                Error::ToolRequestNotPending {
                    lease_id: "l".to_string(),
                    request_id: "r".to_string(),
                },
                "internal",
                None,
            ),
            (
                Error::ToolRequestRelayRetryable {
                    lease_id: "l".to_string(),
                    request_id: "r".to_string(),
                    reason: "lost".to_string(),
                },
                "internal",
                None,
            ),
            (
                Error::BranchExists("feature/eta".to_string()),
                "branch_exists",
                Some(json!({"branch": "feature/eta"})),
            ),
            (
                Error::InvalidFeatureName("Bad Name".to_string()),
                "invalid_feature_name",
                Some(json!({"name": "Bad Name"})),
            ),
            (
                Error::EnvVarNotSet("APP_URL".to_string()),
                "env_var_not_set",
                None,
            ),
            (Error::git("exit 128"), "command_failed", None),
            (Error::AdapterNotFound, "adapter_not_found", None),
            (Error::module("compose"), "module_failed", None),
            (Error::other("misc"), "internal", None),
        ];
        for (err, code, details) in cases {
            assert_eq!(err.code(), code, "{err}");
            assert_eq!(err.details(), details, "{err}");
        }
    }
}
