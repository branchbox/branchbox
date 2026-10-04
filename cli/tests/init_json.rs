//! `branchbox init --json` and the 1Password flags (DESIGN §5.13, capability `init-json`).
//!
//! `--json` implies `--yes`: one summary document on stdout, the progress text on stderr, and no
//! prompts. `--op-github-ref`/`--op-signing-key-ref` record 1Password references in
//! `.devcontainer/.env`, checked with `op read` first (a fake `op` on `PATH` here) unless
//! `--no-verify-op-refs`; `--skip-1password` records the opt-out. Golden documents live in
//! `fixtures/contract/commands/`; rerun with `UPDATE_CONTRACT_FIXTURES=1` to rewrite them after
//! an intended change.
#![cfg(unix)]
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use support::{assert_fixture, assert_single_json, init_test_repo, normalize_json, TestRepo};

const AREA: &str = "commands";

/// References the fake `op` resolves; anything else fails like 1Password does.
const GITHUB_REF: &str = "op://dev/github/token";
const SIGNING_REF: &str = "op://dev/ssh/signing-key";

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A directory holding a fake `op` that resolves [`GITHUB_REF`] and [`SIGNING_REF`] and records
/// every reference it was asked for in `op.log`.
fn fake_op(repo: &TestRepo) -> PathBuf {
    let bin = repo.root().join("fakebin");
    fs::create_dir_all(&bin).unwrap();
    let script = format!(
        "#!/bin/sh\n\
         # Fake 1Password CLI: op read --no-newline -- REF\n\
         echo \"$4\" >> '{log}'\n\
         case \"$4\" in\n\
           {GITHUB_REF}|{SIGNING_REF}) printf 'secret-value' ;;\n\
           *) echo \"[ERROR] could not read secret '$4': isn't an item\" >&2; exit 1 ;;\n\
         esac\n",
        log = bin.join("op.log").display()
    );
    let path = bin.join("op");
    fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn path_with(bin: &Path) -> std::ffi::OsString {
    let mut paths = vec![bin.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap()
}

fn init(repo: &TestRepo, bin: &Path, args: &[&str]) -> Output {
    branchbox_cmd!(repo.path(), "PATH" => path_with(bin))
        .arg("init")
        .args(args)
        .output()
        .expect("run branchbox init")
}

fn devcontainer_env(repo: &TestRepo) -> String {
    fs::read_to_string(repo.path().join(".devcontainer/.env")).unwrap_or_default()
}

fn op_log(bin: &Path) -> String {
    fs::read_to_string(bin.join("op.log")).unwrap_or_default()
}

#[test]
fn init_json_prints_one_summary_document_and_the_log_on_stderr() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    let output = init(&repo, &bin, &["--yes", "--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let mut summary = normalize_json(assert_single_json(&output), &[repo.root()]);
    // `needs_reorganization` depends on where the temp dir lives (a path under /tmp/ counts as
    // temporary), so the fixture pins it.
    let state = &mut summary["repository_state"];
    assert_eq!(state["kind"], "regular_clone");
    assert!(state["needs_reorganization"].is_boolean(), "{state}");
    state["needs_reorganization"] = Value::Bool(false);

    assert_eq!(summary["schema_version"], 1);
    assert_eq!(summary["workspace_path"], "<repo>/main");
    assert_eq!(summary["stack"], "generic");
    assert_eq!(summary["adapter"], "generic");
    assert_eq!(summary["devcontainer_status"], json!({"kind": "created"}));
    assert_eq!(summary["registry_initialized"], true);
    assert_eq!(summary["onepassword"], json!({"status": "not_configured"}));
    assert_fixture(AREA, "init_created", &summary);

    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("✓ Created devcontainer configuration"),
        "{stderr}"
    );
    assert!(repo.path().join(".branchbox/registry.json").exists());
    assert!(
        op_log(&bin).is_empty(),
        "op is not consulted without --op-*"
    );

    // Run again: already initialized, still one document, nothing changed.
    let again = init(&repo, &bin, &["--json"]);
    assert!(again.status.success(), "{}", stderr_of(&again));
    let again = normalize_json(assert_single_json(&again), &[repo.root()]);
    assert_eq!(
        again["repository_state"],
        json!({"kind": "already_initialized"})
    );
    assert_eq!(again["workspace_path"], "<repo>/main");
    assert_eq!(again["registry_initialized"], false);
    assert_eq!(again["devcontainer_status"], json!({"kind": "none"}));
    assert_fixture(AREA, "init_already_initialized", &again);
}

#[test]
fn op_flags_write_verified_references_to_the_devcontainer_env() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    let output = init(
        &repo,
        &bin,
        &[
            "--json",
            "--op-github-ref",
            GITHUB_REF,
            "--op-signing-key-ref",
            SIGNING_REF,
        ],
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    let summary = assert_single_json(&output);
    assert_eq!(summary["onepassword"], json!({"status": "configured"}));
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("secret-value")
            && !stderr_of(&output).contains("secret-value"),
        "the resolved secret is never printed"
    );

    assert_eq!(
        devcontainer_env(&repo),
        format!("OP_GITHUB_REF={GITHUB_REF}\nOP_SIGNING_KEY_REF={SIGNING_REF}\n")
    );
    assert_eq!(op_log(&bin), format!("{GITHUB_REF}\n{SIGNING_REF}\n"));
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(repo.path().join(".devcontainer/.env"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn an_unreadable_reference_is_refused_naming_it_before_anything_changes() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    let missing = "op://dev/missing/token";
    let output = init(&repo, &bin, &["--json", "--op-github-ref", missing]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let envelope = assert_single_json(&output);
    assert_eq!(
        envelope["error"]["code"], "validation_failed",
        "{envelope:#}"
    );
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("--op-github-ref '{missing}'")),
        "{message}"
    );
    assert!(
        message.contains("isn't an item"),
        "names the op error: {message}"
    );
    assert!(message.contains("--no-verify-op-refs"), "{message}");
    assert_fixture(
        AREA,
        "envelope_init_op_ref_unreadable",
        &normalize_json(envelope.clone(), &[repo.root()]),
    );

    assert!(!repo.path().join(".branchbox").exists());
    assert!(!repo.path().join(".devcontainer").exists());

    // The same in text mode, on stderr.
    let text = init(&repo, &bin, &["--yes", "--op-github-ref", missing]);
    assert_eq!(text.status.code(), Some(1));
    assert!(stderr_of(&text).contains(&format!("Error: {message}")));
}

#[test]
fn malformed_references_are_refused_even_unverified() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    let output = init(
        &repo,
        &bin,
        &[
            "--json",
            "--op-github-ref",
            "github-token",
            "--no-verify-op-refs",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let envelope = assert_single_json(&output);
    assert_eq!(envelope["error"]["code"], "validation_failed");
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--op-github-ref 'github-token' is not a 1Password secret reference"));
    assert!(!repo.path().join(".branchbox").exists());
}

#[test]
fn no_verify_saves_references_without_calling_op() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    let unverified = "op://elsewhere/github/token";
    let output = init(
        &repo,
        &bin,
        &[
            "--json",
            "--op-github-ref",
            unverified,
            "--no-verify-op-refs",
        ],
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(
        devcontainer_env(&repo),
        format!("OP_GITHUB_REF={unverified}\n")
    );
    assert!(op_log(&bin).is_empty());
}

#[test]
fn skip_1password_records_the_opt_out() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    let output = init(&repo, &bin, &["--json", "--skip-1password"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let summary = assert_single_json(&output);
    assert_eq!(summary["onepassword"], json!({"status": "skipped"}));
    assert_eq!(devcontainer_env(&repo), "BRANCHBOX_OP_SETUP=skip\n");
    assert!(summary["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .all(|warning| !warning.as_str().unwrap().contains("1Password")));

    // An update can switch to references later.
    let output = init(
        &repo,
        &bin,
        &["--json", "--update", "--op-github-ref", GITHUB_REF],
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(
        assert_single_json(&output)["onepassword"],
        json!({"status": "configured"})
    );
    assert_eq!(
        devcontainer_env(&repo),
        format!("OP_GITHUB_REF={GITHUB_REF}\n")
    );
}

#[test]
fn conflicting_onepassword_flags_are_usage_errors() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    for args in [
        &["--json", "--skip-1password", "--op-github-ref", GITHUB_REF][..],
        &["--json", "--op-signing-key-ref", SIGNING_REF],
        &["--json", "--no-verify-op-refs"],
    ] {
        let output = init(&repo, &bin, args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    assert!(!repo.path().join(".branchbox").exists());
}

#[test]
fn init_json_validate_and_unknown_stack() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    // The repository given as an argument, from outside it.
    let output = branchbox_cmd!(repo.root(), "PATH" => path_with(&bin))
        .args(["init", "--json", "--validate"])
        .arg(repo.path())
        .output()
        .expect("run branchbox init");
    assert!(output.status.success(), "{}", stderr_of(&output));
    let summary = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(summary["workspace_path"], "<repo>/main");
    assert_eq!(summary["repository_state"]["kind"], "regular_clone");
    assert!(stderr_of(&output).contains("Validation Mode"));
    assert!(!repo.path().join(".branchbox").exists());

    let output = init(&repo, &bin, &["--json", "--stack", "cobol"]);
    assert_eq!(output.status.code(), Some(1));
    let envelope: Value = assert_single_json(&output);
    assert_eq!(envelope["error"]["code"], "validation_failed");
    assert_eq!(
        envelope["error"]["message"],
        "Unknown stack: cobol\nValid stacks: rails, nodejs, rust, generic"
    );
}

/// Without a terminal and without `--yes`, init must not wait for an answer even when standard
/// input stays open (the app's pipes): every prompt takes its non-interactive default.
#[test]
fn init_without_a_terminal_completes_with_stdin_left_open() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let repo = init_test_repo();
    let mut child = Command::new(env!("CARGO_BIN_EXE_branchbox"))
        .arg("init")
        .current_dir(repo.path())
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .env("RUST_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn branchbox init");
    // Hold stdin open (never written, never closed) until init exits.
    let stdin = child.stdin.take();

    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll init") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("init blocked on standard input");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(stdin);
    let output = child.wait_with_output().expect("collect init output");
    assert!(status.success(), "{}", stderr_of(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Initialized BranchBox"));
    assert!(repo.path().join(".branchbox/registry.json").exists());
}

/// A `.branchbox/` that holds only configuration (`config set`, `tunnel credentials set`, or a
/// committed `config.json` in a fresh clone) is not initialized: `detect` reports
/// `initialized: false`, and `init` sets the project up, keeping that configuration.
#[test]
fn configuration_written_before_init_does_not_count_as_initialized() {
    let repo = init_test_repo();
    let bin = fake_op(&repo);
    let set = branchbox_cmd!(repo.path())
        .args(["config", "set", "feature.branch_prefix", "spike"])
        .output()
        .expect("run branchbox config set");
    assert!(set.status.success(), "{}", stderr_of(&set));
    assert!(repo.path().join(".branchbox/config.json").exists());

    let detect = branchbox_cmd!(repo.path())
        .args(["detect", "--json"])
        .output()
        .expect("run branchbox detect");
    assert_eq!(assert_single_json(&detect)["initialized"], false);

    let output = init(&repo, &bin, &["--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let summary = assert_single_json(&output);
    assert_ne!(
        summary["repository_state"],
        json!({"kind": "already_initialized"})
    );
    assert_eq!(summary["registry_initialized"], true);
    assert!(repo.path().join(".branchbox/registry.json").exists());
    let config: Value = serde_json::from_str(
        &fs::read_to_string(repo.path().join(".branchbox/config.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(config["feature"]["branch_prefix"], "spike");
}
