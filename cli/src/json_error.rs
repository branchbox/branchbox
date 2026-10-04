//! The machine-mode error envelope (DESIGN §5.2)
//!
//! In `--json` mode a failing command prints exactly one document on stdout:
//!
//! ```json
//! {"schema_version":1,"error":{"code":"worktree_not_found","message":"…","causes":["…"],"details":{"name":"eta"}}}
//! ```
//!
//! Clients branch on `error.code`. Codes come from [`worktree_core::Error::code`] for core
//! failures and from [`CliError`] (or [`recode`]) for conditions only the CLI knows about;
//! anything else is `internal`. `message` and `causes` are the same text the `Error: {err:?}`
//! stderr report shows.
//!
//! The [`CliError`] constructors are the shared vocabulary of the command modules. Those that no
//! command uses yet carry an item-level `allow(dead_code)` until one does.

use serde::Serialize;
use serde_json::{json, Value};
use std::fmt;
use std::path::Path;

/// `schema_version` of the error envelope.
pub const ENVELOPE_SCHEMA_VERSION: u32 = 1;

/// Contract capabilities implemented by the envelope and the machine-mode rules in `main.rs`.
pub const CAPABILITIES: &[&str] = &["json-error-envelope"];

/// A failure the core library does not model, with its contract code. Return it through
/// `anyhow` (`return Err(CliError::….into())`) and the envelope picks up its code and details.
#[derive(Debug, Clone, PartialEq)]
pub struct CliError {
    pub code: &'static str,
    /// Cause-naming text; this is also what `Error: …` prints on stderr.
    pub message: String,
    pub details: Option<Value>,
}

impl CliError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    /// `unsupported`: `what` (a subcommand or flag) is not implemented by this build.
    #[allow(dead_code)]
    pub fn unsupported(what: &str) -> Self {
        Self::new(
            "unsupported",
            format!(
                "`{what}` is not supported by branchbox {} yet",
                env!("CARGO_PKG_VERSION")
            ),
        )
    }

    /// `feature_not_found`: `name` has no entry in the project's registry file. (The core paths
    /// report [`worktree_core::Error::FeatureNotFound`], which maps to the same code.)
    #[allow(dead_code)]
    pub fn feature_not_found(name: &str, registry: &Path) -> Self {
        Self::new(
            "feature_not_found",
            format!(
                "Feature '{name}' is not registered in {}",
                registry.display()
            ),
        )
        .with_details(json!({ "name": name, "registry": registry.display().to_string() }))
    }

    /// `confirmation_required`: an action on `count` items needs a confirmation that machine
    /// mode cannot prompt for. `message` names the flag that confirms it.
    #[allow(dead_code)]
    pub fn confirmation_required(count: usize, message: impl Into<String>) -> Self {
        Self::new("confirmation_required", message).with_details(json!({ "count": count }))
    }

    /// `devcontainer_source_missing`: the main worktree has no `.devcontainer` at `path`.
    #[allow(dead_code)]
    pub fn devcontainer_source_missing(path: &Path) -> Self {
        Self::new(
            "devcontainer_source_missing",
            format!(
                "Devcontainer source not found: {} does not exist in the main worktree",
                path.display()
            ),
        )
        .with_details(json!({ "path": path.display().to_string() }))
    }

    /// `agent_unreachable`: the BranchBox agent could not be reached; `message` says where.
    /// (`agent status` keeps the client's own error text and uses [`recode`] instead.)
    #[allow(dead_code)]
    pub fn agent_unreachable(message: impl Into<String>) -> Self {
        Self::new("agent_unreachable", message)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

/// Label `err` with a contract `code` (and `details`) without changing what it prints: the
/// message, the causes and so the `Error: {err:?}` report on stderr stay byte-identical. Use it
/// where wrapping the error in a [`CliError`] would change existing text.
pub fn recode(err: anyhow::Error, code: &'static str, details: Option<Value>) -> anyhow::Error {
    anyhow::Error::new(Recoded {
        code,
        details,
        inner: err,
    })
}

/// An error that displays as its inner error's outermost message and continues the inner chain
/// as its `source()`, so anyhow renders the same message and "Caused by:" lines.
struct Recoded {
    code: &'static str,
    details: Option<Value>,
    inner: anyhow::Error,
}

impl fmt::Display for Recoded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // anyhow's plain Display is the outermost message only.
        fmt::Display::fmt(&self.inner, f)
    }
}

impl fmt::Debug for Recoded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl std::error::Error for Recoded {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source()
    }
}

/// The document printed on stdout when a `--json` command fails.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorEnvelope {
    pub schema_version: u32,
    pub error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    pub causes: Vec<String>,
    pub details: Option<Value>,
}

impl ErrorEnvelope {
    /// The envelope for a command failure: `message` is the outermost error, `causes` the rest
    /// of the chain (anyhow's "Caused by:" lines), and the code comes from the first coded error.
    pub fn from_error(err: &anyhow::Error) -> Self {
        let (code, details) = classify(err);
        let mut chain = err.chain();
        let message = chain.next().map(ToString::to_string).unwrap_or_default();
        Self::new(
            code,
            message,
            chain.map(ToString::to_string).collect(),
            details,
        )
    }

    /// The envelope the machine-mode panic hook prints.
    pub fn internal_panic(message: impl Into<String>) -> Self {
        Self::new("internal_panic", message.into(), Vec::new(), None)
    }

    /// The `internal_panic` envelope for a panic with `payload` raised at `location`.
    pub fn from_panic(
        payload: &(dyn std::any::Any + Send),
        location: Option<&std::panic::Location<'_>>,
    ) -> Self {
        let what = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("Box<dyn Any>");
        let message = match location {
            Some(location) => format!("branchbox panicked at {location}: {what}"),
            None => format!("branchbox panicked: {what}"),
        };
        Self::internal_panic(message)
    }

    fn new(
        code: &'static str,
        message: String,
        causes: Vec<String>,
        details: Option<Value>,
    ) -> Self {
        Self {
            schema_version: ENVELOPE_SCHEMA_VERSION,
            error: ErrorBody {
                code,
                message,
                causes,
                details,
            },
        }
    }
}

/// The contract code and details for `err`. Outer errors win: a `CliError` context added over a
/// core error reports the CLI's code.
fn classify(err: &anyhow::Error) -> (&'static str, Option<Value>) {
    // anyhow's own downcast also sees `.context(…)` values and the errors under them.
    if let Some(recoded) = err.downcast_ref::<Recoded>() {
        return (recoded.code, recoded.details.clone());
    }
    if let Some(cli) = err.downcast_ref::<CliError>() {
        return (cli.code, cli.details.clone());
    }
    if let Some(core) = err.downcast_ref::<worktree_core::Error>() {
        return (core.code(), core.details());
    }
    // Errors nested through `source()` (e.g. a wrapper type around a core error).
    for cause in err.chain() {
        if let Some(recoded) = cause.downcast_ref::<Recoded>() {
            return (recoded.code, recoded.details.clone());
        }
        if let Some(cli) = cause.downcast_ref::<CliError>() {
            return (cli.code, cli.details.clone());
        }
        if let Some(core) = cause.downcast_ref::<worktree_core::Error>() {
            return (core.code(), core.details());
        }
        if cause.downcast_ref::<std::io::Error>().is_some() {
            return ("io_error", None);
        }
    }
    ("internal", None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Context};
    use std::path::PathBuf;
    use worktree_core::Error as CoreError;

    fn envelope_of<T>(result: anyhow::Result<T>) -> ErrorEnvelope {
        ErrorEnvelope::from_error(&result.err().expect("an error"))
    }

    #[test]
    fn envelope_serializes_to_the_contract_shape() {
        let err: anyhow::Error = CoreError::WorktreeNotFound("nope".to_string()).into();
        let value = serde_json::to_value(ErrorEnvelope::from_error(&err)).unwrap();
        assert_eq!(
            value,
            json!({
                "schema_version": 1,
                "error": {
                    "code": "worktree_not_found",
                    "message": "Worktree not found: nope",
                    "causes": [],
                    "details": {"name": "nope"}
                }
            })
        );
    }

    #[test]
    fn core_errors_keep_their_code_under_context() {
        let result: anyhow::Result<()> =
            Err(CoreError::NotAGitRepository(PathBuf::from("/nonexistent")))
                .context("Failed to open repository");
        let envelope = envelope_of(result);
        assert_eq!(envelope.error.code, "not_a_git_repository");
        assert_eq!(envelope.error.message, "Failed to open repository");
        assert_eq!(
            envelope.error.causes,
            ["Validation error: Not a git repository: /nonexistent"]
        );
        assert_eq!(
            envelope.error.details,
            Some(json!({"path": "/nonexistent"}))
        );
    }

    #[test]
    fn message_and_causes_mirror_the_stderr_report() {
        let result: anyhow::Result<()> = Err(CoreError::validation("bad input"))
            .context("inner context")
            .context("outer context");
        let err = result.unwrap_err();
        let envelope = ErrorEnvelope::from_error(&err);
        assert_eq!(envelope.error.code, "validation_failed");
        let mut lines = vec![envelope.error.message.clone()];
        lines.extend(envelope.error.causes.iter().cloned());
        let stderr = format!("{err:?}");
        for line in &lines {
            assert!(stderr.contains(line.as_str()), "{line} not in {stderr}");
        }
        assert_eq!(
            lines,
            [
                "outer context",
                "inner context",
                "Validation error: bad input"
            ]
        );
    }

    #[test]
    fn cli_errors_carry_their_code_and_details() {
        let err: anyhow::Error =
            CliError::feature_not_found("nope", Path::new("/r/main/.branchbox/registry.json"))
                .into();
        assert_eq!(
            err.to_string(),
            "Feature 'nope' is not registered in /r/main/.branchbox/registry.json"
        );
        let envelope = ErrorEnvelope::from_error(&err);
        assert_eq!(envelope.error.code, "feature_not_found");
        assert!(envelope.error.causes.is_empty());
        assert_eq!(
            envelope.error.details,
            Some(json!({"name": "nope", "registry": "/r/main/.branchbox/registry.json"}))
        );
    }

    #[test]
    fn an_outer_cli_error_wins_over_the_core_error_it_wraps() {
        let result: anyhow::Result<()> =
            Err(CoreError::WorktreeNotFound("nope".to_string())).context(
                CliError::feature_not_found("nope", Path::new("/r/.branchbox/registry.json")),
            );
        let envelope = envelope_of(result);
        assert_eq!(envelope.error.code, "feature_not_found");
        assert_eq!(envelope.error.causes, ["Worktree not found: nope"]);
    }

    #[test]
    fn core_error_used_as_context_is_still_classified() {
        let io = std::io::Error::other("disk full");
        let result: anyhow::Result<()> =
            Err(io).context(CoreError::BranchExists("feature/eta".to_string()));
        let envelope = envelope_of(result);
        assert_eq!(envelope.error.code, "branch_exists");
        assert_eq!(envelope.error.causes, ["disk full"]);
    }

    #[test]
    fn coded_errors_found_through_source_chains() {
        #[derive(Debug)]
        struct Wrapper(CoreError);
        impl fmt::Display for Wrapper {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("wrapper")
            }
        }
        impl std::error::Error for Wrapper {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let err: anyhow::Error = Wrapper(CoreError::InvalidFeatureName("X".to_string())).into();
        let envelope = ErrorEnvelope::from_error(&err);
        assert_eq!(envelope.error.code, "invalid_feature_name");
        assert_eq!(envelope.error.details, Some(json!({"name": "X"})));

        #[derive(Debug)]
        struct CliWrapper(CliError);
        impl fmt::Display for CliWrapper {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("cli wrapper")
            }
        }
        impl std::error::Error for CliWrapper {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let err: anyhow::Error = CliWrapper(CliError::agent_unreachable("no socket")).into();
        assert_eq!(
            ErrorEnvelope::from_error(&err).error.code,
            "agent_unreachable"
        );
    }

    #[test]
    fn plain_io_errors_are_io_error() {
        let result: anyhow::Result<()> =
            Err(std::io::Error::other("denied")).context("Failed to read /r/.env");
        let envelope = envelope_of(result);
        assert_eq!(envelope.error.code, "io_error");
        assert_eq!(envelope.error.message, "Failed to read /r/.env");
        assert_eq!(envelope.error.details, None);
    }

    #[test]
    fn uncoded_errors_are_internal() {
        let envelope = ErrorEnvelope::from_error(&anyhow!("Runtime exploded"));
        assert_eq!(envelope.error.code, "internal");
        assert_eq!(envelope.error.message, "Runtime exploded");
        assert!(envelope.error.causes.is_empty());
        assert_eq!(envelope.error.details, None);
    }

    #[test]
    fn panic_envelope_has_the_internal_panic_code() {
        let value = serde_json::to_value(ErrorEnvelope::internal_panic("boom")).unwrap();
        assert_eq!(
            value,
            json!({
                "schema_version": 1,
                "error": {"code": "internal_panic", "message": "boom", "causes": [], "details": null}
            })
        );
    }

    #[test]
    fn panic_envelopes_name_the_payload_and_location() {
        let location = std::panic::Location::caller();
        let envelope = ErrorEnvelope::from_panic(&"boom", Some(location));
        assert_eq!(envelope.error.code, "internal_panic");
        assert_eq!(
            envelope.error.message,
            format!("branchbox panicked at {location}: boom")
        );

        let owned: Box<dyn std::any::Any + Send> = Box::new(String::from("owned boom"));
        assert_eq!(
            ErrorEnvelope::from_panic(owned.as_ref(), None)
                .error
                .message,
            "branchbox panicked: owned boom"
        );
        assert_eq!(
            ErrorEnvelope::from_panic(&42_u8, None).error.message,
            "branchbox panicked: Box<dyn Any>"
        );
    }

    #[test]
    fn recode_changes_only_the_code() {
        let original = || {
            Err::<(), _>(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No such file or directory (os error 2)",
            ))
            .context("failed to connect to BranchBox agent at /tmp/agent.sock")
            .unwrap_err()
        };
        let recoded = recode(
            original(),
            "agent_unreachable",
            Some(json!({"socket": "/tmp/agent.sock"})),
        );

        // Under RUST_BACKTRACE=1 each error carries its own capture-site backtrace.
        let without_backtrace = |debug: String| {
            debug
                .split("\n\nStack backtrace:")
                .next()
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(
            without_backtrace(format!("{recoded:?}")),
            without_backtrace(format!("{:?}", original()))
        );
        assert_eq!(recoded.to_string(), original().to_string());
        assert_eq!(format!("{recoded:#}"), format!("{:#}", original()));

        let envelope = ErrorEnvelope::from_error(&recoded);
        assert_eq!(envelope.error.code, "agent_unreachable");
        assert_eq!(
            envelope.error.message,
            "failed to connect to BranchBox agent at /tmp/agent.sock"
        );
        assert_eq!(
            envelope.error.causes,
            ["No such file or directory (os error 2)"]
        );
        assert_eq!(
            envelope.error.details,
            Some(json!({"socket": "/tmp/agent.sock"}))
        );

        // Still recognised under later context, which then supplies the message.
        let wrapped = recoded.context("agent status failed");
        let envelope = ErrorEnvelope::from_error(&wrapped);
        assert_eq!(envelope.error.code, "agent_unreachable");
        assert_eq!(envelope.error.message, "agent status failed");
    }

    #[test]
    fn cli_error_constructors_use_the_contract_codes() {
        let unsupported = CliError::unsupported("detect --json");
        assert_eq!(unsupported.code, "unsupported");
        assert!(unsupported.message.contains("`detect --json`"));
        assert!(unsupported.message.contains(env!("CARGO_PKG_VERSION")));
        assert_eq!(unsupported.details, None);

        let confirm =
            CliError::confirmation_required(3, "Refusing to prune 3 features without --yes");
        assert_eq!(confirm.code, "confirmation_required");
        assert_eq!(confirm.details, Some(json!({"count": 3})));
        assert_eq!(
            confirm.to_string(),
            "Refusing to prune 3 features without --yes"
        );

        let missing = CliError::devcontainer_source_missing(Path::new("/r/main/.devcontainer"));
        assert_eq!(missing.code, "devcontainer_source_missing");
        assert!(missing.message.contains("/r/main/.devcontainer"));
        assert_eq!(
            missing.details,
            Some(json!({"path": "/r/main/.devcontainer"}))
        );

        let agent = CliError::agent_unreachable("Agent socket not found at /tmp/agent.sock");
        assert_eq!(agent.code, "agent_unreachable");
        assert_eq!(agent.details, None);

        let custom = CliError::new("teardown_refused", "Refusing").with_details(json!({"x": 1}));
        assert_eq!(custom.details, Some(json!({"x": 1})));
    }
}
