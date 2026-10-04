//! `branchbox detect`: the stack, adapter and modules BranchBox would use for a project.
//!
//! `--json` prints the [`ProjectDetection`] payload (DESIGN §5.7, capability `detect-json`);
//! the text output is the one 0.13 printed.

use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
use worktree_core::workflows::detect::{detect_project, ProjectDetection};
use worktree_core::{humanln, output};

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3).
pub const CAPABILITIES: &[&str] = &["detect-json"];

#[derive(Args)]
pub struct DetectArgs {
    /// Project directory (defaults to current directory)
    #[arg(short, long)]
    pub path: Option<PathBuf>,

    /// Emit JSON output instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

impl DetectArgs {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        self.json
    }
}

pub fn execute(args: DetectArgs) -> Result<()> {
    let project_path = args.path.unwrap_or_else(|| PathBuf::from("."));
    let detection = detect_project(&project_path)?;

    if args.json {
        output::emit_json(&detection)?;
        return Ok(());
    }

    print_text(&project_path, &detection);
    Ok(())
}

/// The 0.13 text report. `Project:` shows the path as given and `Stack:` the stack's Rust name
/// (`Rust`, `NodeJs`), as before.
fn print_text(project_path: &std::path::Path, detection: &ProjectDetection) {
    humanln!("📦 BranchBox Configuration");
    humanln!();
    humanln!("Project: {}", project_path.display());
    humanln!("Stack: {:?}", detection.stack);
    humanln!("Adapter: {}", detection.adapter_name);

    humanln!();
    humanln!("Enabled modules: {}", detection.modules.len());
    for module in &detection.modules {
        humanln!("  ✓ {}", module);
    }
    if !detection.warnings.is_empty() {
        humanln!();
        humanln!("Warnings:");
        for warning in &detection.warnings {
            humanln!("  - {}", warning);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_flag_selects_machine_mode() {
        let args = DetectArgs {
            path: None,
            json: true,
        };
        assert!(args.wants_json());
        assert!(!DetectArgs {
            path: None,
            json: false
        }
        .wants_json());
    }

    #[test]
    fn advertises_the_detect_json_capability() {
        assert_eq!(CAPABILITIES, ["detect-json"]);
    }
}
