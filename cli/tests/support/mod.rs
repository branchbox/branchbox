//! Shared helpers for the CLI integration tests, in particular the JSON contract tests
//! (DESIGN §5, §10.1).
//!
//! Use it from a test file with `#[macro_use] mod support;`. This module is frozen after
//! wave 1: later packages add helpers in their own test files rather than change these.
//!
//! - [`branchbox_cmd!`] builds a hermetic `branchbox` invocation.
//! - [`init_test_repo`] creates a repository whose commits hash the same on every run.
//! - [`assert_single_json`] checks that stdout carries exactly one JSON document.
//! - [`normalize_json`] blanks out what legitimately differs between runs.
//! - [`assert_fixture`] compares a document with `cli/tests/fixtures/contract/<area>/<name>.json`
//!   and rewrites the fixture when `UPDATE_CONTRACT_FIXTURES=1`.

// Each test binary compiles its own copy and uses a subset.
#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

/// Environment variables that change what a command prints; the tests run without them.
pub const SCRUBBED_ENV: &[&str] = &[
    "BRANCHBOX_DEFAULT_AGENT_CMD",
    "BRANCHBOX_DEFAULT_AGENT_NAME",
    "BRANCHBOX_ENABLE_PROMPT_BRIDGE",
    "BRANCHBOX_DEVCONTAINER_STRATEGY",
    "BRANCHBOX_DEVCONTAINER_REUSE_POLICY",
    "BRANCHBOX_POLICY_ENFORCED_MODULES",
    "BRANCHBOX_EMIT_TELEMETRY",
    "BRANCHBOX_AGENT_SOCKET",
    "BRANCHBOX_AGENT_DIR",
    "BRANCHBOX_SBX_PATH",
];

/// An `assert_cmd::Command` for the `branchbox` binary, run in `$dir` with host validation
/// skipped, logging off and [`SCRUBBED_ENV`] removed. Extra `KEY => VALUE` pairs are set in the
/// child's environment.
#[macro_export]
macro_rules! branchbox_cmd {
    ($dir:expr $(, $key:expr => $value:expr )* $(,)?) => {{
        let mut cmd = ::assert_cmd::Command::new(env!("CARGO_BIN_EXE_branchbox"));
        cmd.current_dir($dir)
            .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
            .env("RUST_LOG", "off")
            // Byte-compatibility assertions compare stderr; CI's RUST_BACKTRACE=1 would append
            // anyhow backtraces to every `Error:` line.
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0");
        for name in $crate::support::SCRUBBED_ENV {
            cmd.env_remove(name);
        }
        $(
            cmd.env($key, $value);
        )*
        cmd
    }};
}

/// A throwaway repository at `<root>/main`, in BranchBox's parent layout with `root` named
/// `demo` (so derived names such as the compose project are `demo-<feature>`). Feature
/// worktrees land beside it, so everything a test creates lives under `root` and goes away with
/// the temp dir.
pub struct TestRepo {
    _temp: TempDir,
    root: PathBuf,
    repo: PathBuf,
}

impl TestRepo {
    /// The main worktree.
    pub fn path(&self) -> &Path {
        &self.repo
    }

    /// The directory holding the repository and its feature worktrees.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run git in the main worktree with the same fixed identity and dates as the initial
    /// commit.
    pub fn git(&self, args: &[&str]) {
        git(&self.repo, args);
    }
}

/// Fixed identity and dates make commit hashes, and so `last_commit`, identical on every run.
const GIT_DATE: &str = "2026-01-01T00:00:00Z";

/// Run git in `dir` with a fixed author, committer and dates, ignoring the user's git config.
pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", GIT_DATE)
        .env("GIT_COMMITTER_DATE", GIT_DATE)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap_or_else(|err| panic!("failed to run git {args:?}: {err}"));
    assert!(status.success(), "git {args:?} failed with {status}");
}

/// A repository on `main` with one commit (`README.md`) and an untracked `.env` that sets
/// `APP_URL`, so feature URLs are derived the same way every time.
pub fn init_test_repo() -> TestRepo {
    let temp = TempDir::new().expect("create temp dir");
    let root = temp.path().join("demo");
    let repo = root.join("main");
    fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("README.md"), "# Test Repo\n").expect("write README");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-q", "-m", "Initial commit"]);
    fs::write(repo.join(".env"), "APP_URL=dev.example.com\n").expect("write .env");
    TestRepo {
        _temp: temp,
        root,
        repo,
    }
}

/// Parse stdout as exactly one JSON document (surrounding whitespace allowed) and return it.
/// Fails with both streams when stdout is empty, is not JSON, or holds anything else.
pub fn assert_single_json(output: &Output) -> Value {
    let context = || {
        format!(
            "status: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };
    let mut documents = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Value>();
    let document = match documents.next() {
        Some(Ok(document)) => document,
        Some(Err(err)) => panic!("stdout is not JSON ({err})\n{}", context()),
        None => panic!("stdout carries no JSON document\n{}", context()),
    };
    match documents.next() {
        None => document,
        Some(Ok(_)) => panic!("stdout carries more than one JSON document\n{}", context()),
        Some(Err(err)) => panic!("stdout has trailing non-JSON text ({err})\n{}", context()),
    }
}

/// Replace what differs between runs, so a document can be compared with a fixture:
/// - each of `roots` (and its canonical form) inside strings → `<repo>`;
/// - RFC 3339 timestamps → `"<ts>"`;
/// - `duration_ms` / `elapsed_ms` → `0`;
/// - `pid` → `0`;
/// - `version` strings → `"<version>"`;
/// - `color` strings → `"#000000"`;
/// - `published_ports[].host` → `0`.
pub fn normalize_json(value: Value, roots: &[&Path]) -> Value {
    let mut replacements: Vec<String> = Vec::new();
    for root in roots {
        if let Ok(canonical) = fs::canonicalize(root) {
            replacements.push(canonical.display().to_string());
        }
        replacements.push(root.display().to_string());
    }
    // Longest first, so `/private/var/…` is replaced before the `/var/…` it contains.
    replacements.sort_by_key(|root| std::cmp::Reverse(root.len()));
    replacements.dedup();
    normalize_value(value, None, &replacements)
}

fn normalize_value(value: Value, key: Option<&str>, roots: &[String]) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(child_key, child)| {
                    let child = if child_key == "published_ports" {
                        zero_host_ports(child)
                    } else {
                        child
                    };
                    let normalized = normalize_value(child, Some(&child_key), roots);
                    (child_key, normalized)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| normalize_value(item, key, roots))
                .collect(),
        ),
        Value::Number(_) if matches!(key, Some("duration_ms" | "elapsed_ms" | "pid")) => {
            Value::from(0)
        }
        Value::String(_) if key == Some("version") => Value::from("<version>"),
        Value::String(_) if key == Some("color") => Value::from("#000000"),
        Value::String(text) if chrono::DateTime::parse_from_rfc3339(&text).is_ok() => {
            Value::from("<ts>")
        }
        Value::String(text) => Value::String(
            roots
                .iter()
                .fold(text, |text, root| text.replace(root.as_str(), "<repo>")),
        ),
        other => other,
    }
}

fn zero_host_ports(ports: Value) -> Value {
    match ports {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|mut port| {
                    if let Some(host) = port.get_mut("host") {
                        *host = Value::from(0);
                    }
                    port
                })
                .collect(),
        ),
        other => other,
    }
}

/// The fixture file for `area`/`name`.
pub fn fixture_path(area: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/contract")
        .join(area)
        .join(format!("{name}.json"))
}

/// Compare `actual` with `cli/tests/fixtures/contract/<area>/<name>.json`. With
/// `UPDATE_CONTRACT_FIXTURES=1` the fixture is (re)written instead; otherwise a missing or
/// different fixture fails the test with a line diff.
pub fn assert_fixture(area: &str, name: &str, actual: &Value) {
    let path = fixture_path(area, name);
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(actual).expect("render fixture")
    );
    if std::env::var_os("UPDATE_CONTRACT_FIXTURES").is_some_and(|value| value == "1") {
        fs::create_dir_all(path.parent().expect("fixture dir")).expect("create fixture dir");
        fs::write(&path, &rendered).expect("write fixture");
        return;
    }

    let expected_text = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing contract fixture {} ({err}); run the test with UPDATE_CONTRACT_FIXTURES=1 \
             to create it\nactual:\n{rendered}",
            path.display()
        )
    });
    let expected: Value = serde_json::from_str(&expected_text)
        .unwrap_or_else(|err| panic!("fixture {} is not JSON: {err}", path.display()));
    if &expected != actual {
        let expected_pretty = serde_json::to_string_pretty(&expected).unwrap();
        panic!(
            "contract fixture {} does not match (- fixture, + actual); rerun with \
             UPDATE_CONTRACT_FIXTURES=1 if the change is intended:\n{}",
            path.display(),
            line_diff(&expected_pretty, rendered.trim_end())
        );
    }
}

/// A minimal unified-style line diff (longest common subsequence); fixtures are small.
fn line_diff(expected: &str, actual: &str) -> String {
    let old: Vec<&str> = expected.lines().collect();
    let new: Vec<&str> = actual.lines().collect();
    let mut lcs = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut diff = String::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            diff.push_str(&format!("  {}\n", old[i]));
            i += 1;
            j += 1;
        } else if j < new.len() && (i == old.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            diff.push_str(&format!("+ {}\n", new[j]));
            j += 1;
        } else {
            diff.push_str(&format!("- {}\n", old[i]));
            i += 1;
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::process::ExitStatus;

    #[test]
    fn normalize_json_blanks_run_specific_values() {
        let root = Path::new("/tmp/run-1234");
        let value = json!({
            "worktree_path": "/tmp/run-1234/eta",
            "created_at": "2026-10-01T22:50:29.222458Z",
            "notes": ["Copied /tmp/run-1234/main/.env"],
            "module_outcomes": [{"duration_ms": 17, "status": "success"}],
            "setup": {"state": "in_progress", "pid": 4242},
            "version": "0.14.0",
            "color": "#4f9d69",
            "runtime": {"published_ports": [{"host": 49152, "runtime": 3000}]},
            "count": 3
        });
        assert_eq!(
            normalize_json(value, &[root]),
            json!({
                "worktree_path": "<repo>/eta",
                "created_at": "<ts>",
                "notes": ["Copied <repo>/main/.env"],
                "module_outcomes": [{"duration_ms": 0, "status": "success"}],
                "setup": {"state": "in_progress", "pid": 0},
                "version": "<version>",
                "color": "#000000",
                "runtime": {"published_ports": [{"host": 0, "runtime": 3000}]},
                "count": 3
            })
        );
    }

    #[test]
    fn assert_single_json_accepts_one_pretty_document() {
        let output = Output {
            status: ExitStatus::default(),
            stdout: b"{\n  \"a\": 1\n}\n".to_vec(),
            stderr: Vec::new(),
        };
        assert_eq!(assert_single_json(&output), json!({"a": 1}));
    }

    #[test]
    #[should_panic(expected = "more than one JSON document")]
    fn assert_single_json_rejects_a_second_document() {
        let output = Output {
            status: ExitStatus::default(),
            stdout: b"{}\n{}\n".to_vec(),
            stderr: Vec::new(),
        };
        assert_single_json(&output);
    }

    #[test]
    #[should_panic(expected = "stdout is not JSON")]
    fn assert_single_json_rejects_a_text_preamble() {
        let output = Output {
            status: ExitStatus::default(),
            stdout: "⚠️  Prompt truncated\n{}\n".as_bytes().to_vec(),
            stderr: Vec::new(),
        };
        assert_single_json(&output);
    }

    #[test]
    fn line_diff_marks_changed_lines() {
        assert_eq!(line_diff("a\nb\nc", "a\nx\nc"), "  a\n+ x\n- b\n  c\n");
    }
}
