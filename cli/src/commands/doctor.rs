//! `branchbox doctor`: host and project health checks (DESIGN §5.12, capability `doctor`).
//!
//! The report is printed whatever the checks found; the command then exits 1 when a required
//! check is in error. In `--json` mode that is an in-band failure (DESIGN §5.2 rule 4): the
//! payload is the only document on stdout and no error envelope follows it.

use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
use worktree_core::doctor::{
    self, CheckStatus, DoctorCheck, DoctorOptions, DoctorReport, DEFAULT_PROBE_TIMEOUT,
};
use worktree_core::{humanln, output};

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3).
pub const CAPABILITIES: &[&str] = &["doctor"];

#[derive(Args)]
pub struct DoctorArgs {
    /// Also check this repository (git, initialization, config, registry, .gitignore)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Also check that the tools that need credentials are signed in
    #[arg(long)]
    pub check_auth: bool,

    /// Emit the check results as JSON
    #[arg(long)]
    pub json: bool,
}

impl DoctorArgs {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        self.json
    }
}

pub fn execute(args: DoctorArgs) -> Result<()> {
    let report = doctor::run(&DoctorOptions {
        repo: args.repo,
        probe_timeout: DEFAULT_PROBE_TIMEOUT,
        check_auth: args.check_auth,
    });

    if args.json {
        output::emit_json(&report)?;
    } else {
        print_report(&report);
    }

    let failed = report.failed_required();
    if !failed.is_empty() {
        let ids: Vec<&str> = failed.iter().map(|check| check.id).collect();
        anyhow::bail!(
            "{} required check(s) failed: {}",
            failed.len(),
            ids.join(", ")
        );
    }
    Ok(())
}

fn print_report(report: &DoctorReport) {
    humanln!(
        "🩺 BranchBox doctor (branchbox {}, {}/{})",
        report.cli.version,
        report.host.os,
        report.host.arch
    );
    humanln!();
    for check in &report.checks {
        humanln!("{}", check_line(check));
        if let Some(remediation) = check
            .remediation
            .as_deref()
            .filter(|_| matches!(check.status, CheckStatus::Warn | CheckStatus::Error))
        {
            humanln!("      → {}", remediation);
        }
    }
    humanln!();
    humanln!(
        "{} ok, {} warning(s), {} error(s)",
        report.summary.ok,
        report.summary.warn,
        report.summary.error
    );
}

/// `  ✓ Git 2.50.1 (/usr/bin/git)` or `  ✗ Docker daemon: timed out after 3s`.
fn check_line(check: &DoctorCheck) -> String {
    let icon = match check.status {
        CheckStatus::Ok => "✓",
        CheckStatus::Warn => "⚠",
        CheckStatus::Error => "✗",
        CheckStatus::Skipped => "–",
    };
    let mut line = format!("  {icon} {}", check.title);
    if let Some(version) = &check.version {
        line.push(' ');
        line.push_str(version);
    }
    if let Some(path) = &check.path {
        line.push_str(&format!(" ({path})"));
    }
    if let Some(detail) = &check.detail {
        line.push_str(": ");
        line.push_str(detail);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(status: CheckStatus) -> DoctorCheck {
        DoctorCheck {
            id: "docker.daemon",
            title: "Docker daemon",
            required: true,
            status,
            path: None,
            version: None,
            detail: None,
            remediation: None,
        }
    }

    #[test]
    fn check_lines_show_status_version_path_and_detail() {
        let mut ok = check(CheckStatus::Ok);
        ok.title = "Git";
        ok.version = Some("2.50.1".to_string());
        ok.path = Some("/usr/bin/git".to_string());
        assert_eq!(check_line(&ok), "  ✓ Git 2.50.1 (/usr/bin/git)");

        let mut error = check(CheckStatus::Error);
        error.detail = Some("timed out after 3s".to_string());
        assert_eq!(check_line(&error), "  ✗ Docker daemon: timed out after 3s");
        assert_eq!(check_line(&check(CheckStatus::Warn)), "  ⚠ Docker daemon");
        assert_eq!(
            check_line(&check(CheckStatus::Skipped)),
            "  – Docker daemon"
        );
    }

    #[test]
    fn json_flag_selects_machine_mode_and_the_capability_is_listed() {
        let args = DoctorArgs {
            repo: None,
            check_auth: false,
            json: true,
        };
        assert!(args.wants_json());
        assert_eq!(CAPABILITIES, ["doctor"]);
    }
}
