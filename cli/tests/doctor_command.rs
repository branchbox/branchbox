//! `branchbox doctor` (DESIGN §5.12) against fake tools: each test puts scripted `git`, `docker`
//! and `sbx` stand-ins alone on `PATH`, so the checks see exactly the host the test describes.
//! The golden document lives in `fixtures/contract/commands/`; rerun with
//! `UPDATE_CONTRACT_FIXTURES=1` to rewrite it after an intended change.
#![cfg(unix)]
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};
use support::{assert_fixture, assert_single_json, init_test_repo, normalize_json};
use tempfile::TempDir;

const AREA: &str = "commands";

/// A healthy Docker: client, daemon and the compose plugin all answer.
const DOCKER_OK: &str = r#"case "$1" in
  --version) echo "Docker version 28.3.2, build test" ;;
  version) echo "28.3.2" ;;
  compose) echo "2.38.2" ;;
  *) exit 1 ;;
esac"#;

/// The daemon is down: `docker version` fails the way the real CLI does.
const DOCKER_DAEMON_DOWN: &str = r#"case "$1" in
  --version) echo "Docker version 28.3.2, build test" ;;
  version)
    echo "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?" >&2
    exit 1 ;;
  compose) echo "2.38.2" ;;
  *) exit 1 ;;
esac"#;

/// The daemon is wedged: `docker version` hangs.
const DOCKER_DAEMON_HANGS: &str = r#"case "$1" in
  --version) echo "Docker version 28.3.2, build test" ;;
  version) /bin/sleep 10 ;;
  compose) echo "2.38.2" ;;
  *) exit 1 ;;
esac"#;

/// Docker Sandboxes installed but signed out.
const SBX_SIGNED_OUT: &str = r#"echo "Error: not authenticated; please sign in with 'sbx login'" >&2
exit 1"#;

/// A directory of fake tools that becomes the whole `PATH` of the doctor run.
struct FakeHost {
    temp: TempDir,
}

impl FakeHost {
    /// Fake tools plus a `git` that forwards to the real one (the repository checks run it).
    fn new() -> Self {
        let host = Self {
            temp: TempDir::new().unwrap(),
        };
        fs::create_dir_all(host.bin()).unwrap();
        host.tool("git", &format!("exec '{}' \"$@\"", real_git().display()));
        host
    }

    fn bin(&self) -> PathBuf {
        self.temp.path().join("bin")
    }

    fn tool(&self, name: &str, body: &str) -> &Self {
        let path = self.bin().join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        self
    }

    fn doctor(&self, dir: &Path, args: &[&str]) -> Output {
        branchbox_cmd!(dir, "PATH" => self.bin())
            .arg("doctor")
            .args(args)
            .output()
            .expect("run doctor")
    }
}

/// The real git executable (not a shim that needs `PATH`), from `git --exec-path`.
fn real_git() -> PathBuf {
    let output = Command::new("git")
        .arg("--exec-path")
        .output()
        .expect("git is installed");
    let exec_path = String::from_utf8(output.stdout).unwrap();
    PathBuf::from(exec_path.trim()).join("git")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn check<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == id)
        .unwrap_or_else(|| panic!("no {id} check in {report:#}"))
}

/// Blank what differs by machine: the host, the executable path, and the two checks whose
/// outcome depends on the platform (local-vm is Linux-only; the container check depends on
/// where the tests run), plus the summary counts they feed.
fn platform_neutral(mut report: Value) -> Value {
    report["host"] = json!({"os": "<os>", "arch": "<arch>"});
    report["cli"]["path"] = json!("<branchbox>");
    for check in report["checks"].as_array_mut().unwrap() {
        if check["id"] == "runtime.local_vm" || check["id"] == "host.in_container" {
            for field in ["status", "path", "version", "detail", "remediation"] {
                check[field] = json!("<platform>");
            }
        }
    }
    report["summary"] = json!({"ok": "<n>", "warn": "<n>", "error": "<n>"});
    report
}

#[test]
fn healthy_host_reports_every_check_and_exits_0() {
    let host = FakeHost::new();
    host.tool("docker", DOCKER_OK);
    let output = host.doctor(host.temp.path(), &["--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));

    let report = assert_single_json(&output);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["cli"]["contract_version"], 1);
    assert_eq!(report["cli"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(check(&report, "docker.daemon")["status"], "ok");
    assert_eq!(check(&report, "docker.daemon")["version"], "28.3.2");
    assert_eq!(check(&report, "docker.compose")["version"], "2.38.2");
    assert_eq!(check(&report, "devcontainer.cli")["status"], "warn");
    assert_eq!(check(&report, "runtime.sbx")["status"], "skipped");
    assert_eq!(check(&report, "op")["status"], "skipped");
    assert_eq!(check(&report, "gh")["status"], "skipped");

    let summary = &report["summary"];
    let counted = ["ok", "warn", "error"]
        .iter()
        .map(|key| summary[key].as_u64().unwrap())
        .sum::<u64>();
    let skipped = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|check| check["status"] == "skipped")
        .count() as u64;
    assert_eq!(
        counted + skipped,
        report["checks"].as_array().unwrap().len() as u64
    );

    let normalized = platform_neutral(normalize_json(report, &[host.temp.path()]));
    assert_fixture(AREA, "doctor_healthy", &normalized);
}

#[test]
fn a_failing_docker_daemon_is_a_required_error_and_exits_1() {
    let host = FakeHost::new();
    host.tool("docker", DOCKER_DAEMON_DOWN);
    let output = host.doctor(host.temp.path(), &["--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));

    // In-band: the report is the only document; no envelope follows it.
    let report = assert_single_json(&output);
    assert!(report.get("error").is_none());
    let daemon = check(&report, "docker.daemon");
    assert_eq!(daemon["status"], "error");
    assert_eq!(daemon["required"], true);
    assert!(daemon["detail"]
        .as_str()
        .unwrap()
        .starts_with("Cannot connect to the Docker daemon"));
    assert_eq!(
        daemon["remediation"],
        "Start Docker Desktop (or the Docker daemon) and retry"
    );
    assert_eq!(check(&report, "docker.cli")["status"], "ok");
    assert!(report["summary"]["error"].as_u64().unwrap() >= 1);
    assert!(
        stderr_of(&output).contains("Error: 1 required check(s) failed: docker.daemon"),
        "{}",
        stderr_of(&output)
    );

    // Text mode exits 1 too, after the report.
    let text = host.doctor(host.temp.path(), &[]);
    assert_eq!(text.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&text.stdout);
    let daemon_line = stdout
        .lines()
        .find(|line| line.starts_with("  ✗ Docker daemon ("))
        .unwrap_or_else(|| panic!("no failed daemon line in\n{stdout}"));
    assert!(
        daemon_line.ends_with(
            "): Cannot connect to the Docker daemon at \
             unix:///var/run/docker.sock. Is the docker daemon running?"
        ),
        "{daemon_line}"
    );
    assert!(stdout.contains("→ Start Docker Desktop"), "{stdout}");
}

#[test]
fn a_hung_docker_daemon_times_out_within_the_probe_deadline() {
    let host = FakeHost::new();
    host.tool("docker", DOCKER_DAEMON_HANGS);
    let started = Instant::now();
    let output = host.doctor(host.temp.path(), &["--json"]);
    let elapsed = started.elapsed();

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let report = assert_single_json(&output);
    let daemon = check(&report, "docker.daemon");
    assert_eq!(daemon["status"], "error");
    assert_eq!(daemon["detail"], "timed out after 3s");
    assert!(
        elapsed < Duration::from_secs(5),
        "doctor took {elapsed:?}; the 3 s probe deadline should bound it"
    );
}

#[test]
fn a_signed_out_sandboxes_cli_is_a_warning() {
    let host = FakeHost::new();
    host.tool("docker", DOCKER_OK).tool("sbx", SBX_SIGNED_OUT);
    let output = host.doctor(host.temp.path(), &["--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));

    let report = normalize_json(assert_single_json(&output), &[host.temp.path()]);
    let sbx = check(&report, "runtime.sbx");
    assert_eq!(sbx["status"], "warn");
    assert_eq!(sbx["path"], "<repo>/bin/sbx");
    assert!(sbx["detail"]
        .as_str()
        .unwrap()
        .starts_with("Not signed in:"));
    assert_eq!(sbx["remediation"], "Run: sbx login");
}

#[test]
fn check_auth_asks_op_and_gh_whether_they_are_signed_in() {
    let host = FakeHost::new();
    host.tool("docker", DOCKER_OK)
        .tool(
            "op",
            r#"case "$1" in --version) echo "2.31.1" ;; whoami) echo "not signed in" >&2; exit 1 ;; esac"#,
        )
        .tool(
            "gh",
            r#"case "$1" in --version) echo "gh version 2.76.0 (2025-07-17)" ;; auth) echo "Logged in" ;; esac"#,
        );

    let presence = assert_single_json(&host.doctor(host.temp.path(), &["--json"]));
    assert_eq!(check(&presence, "op")["status"], "ok");
    assert_eq!(check(&presence, "op")["version"], "2.31.1");
    assert!(check(&presence, "op")["detail"]
        .as_str()
        .unwrap()
        .contains("--check-auth"));

    let output = host.doctor(host.temp.path(), &["--json", "--check-auth"]);
    assert!(output.status.success(), "optional checks never fail doctor");
    let report = assert_single_json(&output);
    assert_eq!(check(&report, "op")["status"], "warn");
    assert_eq!(check(&report, "op")["remediation"], "Run: op signin");
    assert_eq!(check(&report, "gh")["status"], "ok");
    assert_eq!(check(&report, "gh")["version"], "2.76.0");
    assert_eq!(check(&report, "gh")["detail"], "Signed in");
}

#[test]
fn repo_on_an_uninitialized_repository_warns_that_branchbox_is_not_set_up() {
    let host = FakeHost::new();
    host.tool("docker", DOCKER_OK);
    let repo = init_test_repo();
    let output = host.doctor(repo.root(), &["--json", "--repo", "main"]);
    assert!(output.status.success(), "{}", stderr_of(&output));

    let report = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(check(&report, "repo.git")["status"], "ok");
    assert_eq!(check(&report, "repo.git")["path"], "<repo>/main");
    let initialized = check(&report, "repo.initialized");
    assert_eq!(initialized["status"], "warn");
    assert_eq!(initialized["remediation"], "Run: branchbox init");
    assert_eq!(check(&report, "repo.config")["status"], "ok");
    assert_eq!(check(&report, "repo.registry")["status"], "skipped");
    assert_eq!(check(&report, "repo.gitignore")["status"], "skipped");
}

#[test]
fn repo_that_is_not_a_repository_is_a_required_error() {
    let host = FakeHost::new();
    host.tool("docker", DOCKER_OK);
    let output = host.doctor(host.temp.path(), &["--json", "--repo", "."]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let report = assert_single_json(&output);
    let git = check(&report, "repo.git");
    assert_eq!(git["status"], "error");
    assert!(git["detail"]
        .as_str()
        .unwrap()
        .starts_with("Not a git repository"));
    assert_eq!(check(&report, "repo.initialized")["status"], "skipped");
}
