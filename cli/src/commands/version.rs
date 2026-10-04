//! `branchbox version`: the CLI version and, with `--json`, the contract it implements
//! (DESIGN §5.3).
//!
//! Clients call `branchbox version --json` once and enable only the features whose capability
//! is listed. A CLI older than 0.14 has no `version` subcommand (clap exits 2), and clients fall
//! back to parsing `branchbox --version` with an empty capability set.

use super::{agent, config, detect, devcontainer, doctor, feature, init, tunnel};
use crate::json_error;
use anyhow::Result;
use clap::Args;
use serde::Serialize;
use worktree_core::{humanln, output};

#[derive(Args)]
pub struct VersionArgs {
    /// Print the version, contract version and capabilities as JSON
    #[arg(long)]
    pub json: bool,
}

impl VersionArgs {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        self.json
    }
}

/// The `version --json` payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VersionInfo {
    pub version: &'static str,
    pub contract_version: u32,
    pub capabilities: Vec<&'static str>,
}

impl VersionInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            contract_version: worktree_core::CONTRACT_VERSION,
            capabilities: capabilities(),
        }
    }
}

/// Every capability this build implements, in contract order: the envelope, the core library's,
/// then each command module's. A module lists a capability in the same change that implements
/// it.
pub fn capabilities() -> Vec<&'static str> {
    let mut capabilities = json_error::CAPABILITIES.to_vec();
    capabilities.extend(worktree_core::capabilities());
    for module in [
        feature::CAPABILITIES,
        detect::CAPABILITIES,
        devcontainer::CAPABILITIES,
        config::CAPABILITIES,
        tunnel::CAPABILITIES,
        doctor::CAPABILITIES,
        init::CAPABILITIES,
        agent::CAPABILITIES,
    ] {
        capabilities.extend_from_slice(module);
    }
    capabilities
}

pub fn execute(args: VersionArgs) -> Result<()> {
    let info = VersionInfo::current();
    if args.json {
        output::emit_json(&info)?;
    } else {
        // Same text as `branchbox --version`.
        humanln!("branchbox {}", info.version);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_lead_with_the_envelope_and_include_the_core_ones() {
        let capabilities = capabilities();
        assert_eq!(capabilities.first(), Some(&"json-error-envelope"));
        for expected in ["registry-lock", "write-ahead-start"] {
            assert!(
                capabilities.contains(&expected),
                "{expected} missing from {capabilities:?}"
            );
        }
        let core = worktree_core::capabilities();
        assert_eq!(&capabilities[1..=core.len()], core.as_slice());
    }

    #[test]
    fn capabilities_are_unique_contract_strings() {
        let capabilities = capabilities();
        let mut unique = capabilities.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), capabilities.len(), "{capabilities:?}");
        assert!(capabilities.iter().all(|capability| {
            !capability.is_empty()
                && capability
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch == '-')
        }));
    }

    #[test]
    fn payload_has_the_contract_shape() {
        let value = serde_json::to_value(VersionInfo::current()).unwrap();
        let object = value.as_object().unwrap();
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["capabilities", "contract_version", "version"]);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["contract_version"], 1);
        assert!(value["capabilities"].is_array());
    }
}
