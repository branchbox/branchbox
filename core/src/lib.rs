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
//! - [`config`] / [`config_edit`] - Project configuration and its format-preserving editor
//! - [`credentials`] - Cloudflare API credentials for tunnel provisioning
//! - [`doctor`] - Host and project health checks
//! - [`output`] - Human text vs. machine (`--json`) output routing
//! - [`error`] - Error types

#![cfg_attr(test, allow(clippy::disallowed_macros))]

pub mod adapters;
pub(crate) mod atomic_fs;
pub mod bootstrap;
pub mod cloudflare;
pub mod config;
pub mod config_edit;
pub mod credentials;
pub mod devcontainer_runtime;
pub mod doctor;
pub(crate) mod env_placeholders;
pub mod error;
pub mod git;
pub mod modules;
pub mod naming;
pub mod output;
pub mod runtime;
pub mod tunnel;
pub mod validation;
pub mod workflow;
pub mod workflows;

pub use error::{Error, Result};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the machine-readable CLI contract (DESIGN §5). Bumped only on breaking changes;
/// additive keys and new capabilities do not change it.
pub const CONTRACT_VERSION: u32 = 1;

/// The contract capabilities implemented by this library, in contract order. The CLI adds its
/// own (`json-error-envelope` and per-command ones) when it prints `branchbox version --json`.
pub fn capabilities() -> Vec<&'static str> {
    [
        atomic_fs::CAPABILITIES,
        workflows::feature::CAPABILITIES,
        workflows::teardown_plan::CAPABILITIES,
        runtime::CAPABILITIES,
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_aggregate_every_module_without_duplicates() {
        let capabilities = capabilities();
        let expected: Vec<&str> = atomic_fs::CAPABILITIES
            .iter()
            .chain(workflows::feature::CAPABILITIES)
            .chain(workflows::teardown_plan::CAPABILITIES)
            .chain(runtime::CAPABILITIES)
            .copied()
            .collect();
        assert_eq!(capabilities, expected);

        let mut unique = capabilities.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), capabilities.len(), "{capabilities:?}");
        assert!(capabilities.iter().all(|capability| !capability.is_empty()
            && capability
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-')));
    }

    #[test]
    fn capabilities_lead_with_the_registry_guarantees() {
        // Later packages append their own; these two come first, in contract order.
        assert!(
            capabilities().starts_with(&["registry-lock", "write-ahead-start"]),
            "{:?}",
            capabilities()
        );
    }

    #[test]
    fn contract_version_is_one() {
        assert_eq!(CONTRACT_VERSION, 1);
    }
}
