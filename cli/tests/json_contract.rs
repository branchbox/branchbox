//! The machine-mode contract (DESIGN §5.2-§5.4): in `--json` mode stdout carries exactly one
//! JSON document, the success payload or the error envelope, while stderr keeps the human text
//! and the `Error: …` report. Golden documents live in `fixtures/contract/core/`; rerun with
//! `UPDATE_CONTRACT_FIXTURES=1` to rewrite them after an intended change.
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Output;
use support::{assert_fixture, assert_single_json, init_test_repo, normalize_json, TestRepo};

const AREA: &str = "core";

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Start `name` in minimal mode and return its summary.
fn start_minimal(repo: &TestRepo, name: &str) -> Value {
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "start", name, "--minimal", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_single_json(&output)
}

fn list_json(repo: &TestRepo) -> Value {
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "list", "--json"])
        .output()
        .expect("run feature list");
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_single_json(&output)
}

/// Assert a failing `--json` command: exit 1, exactly one envelope on stdout with `code`, and
/// the envelope's message as the `Error:` line on stderr. Returns the envelope.
fn assert_envelope(output: &Output, code: &str) -> Value {
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(output));
    let envelope = assert_single_json(output);
    assert_eq!(envelope["schema_version"], 1, "{envelope:#}");
    assert_eq!(envelope["error"]["code"], code, "{envelope:#}");
    let message = envelope["error"]["message"].as_str().expect("message");
    assert!(
        stderr_of(output).contains(&format!("Error: {message}")),
        "stderr keeps the Error: report\n{}",
        stderr_of(output)
    );
    for cause in envelope["error"]["causes"].as_array().expect("causes") {
        assert!(stderr_of(output).contains(cause.as_str().unwrap()));
    }
    envelope
}

#[test]
fn start_with_an_oversized_prompt_prints_one_document_with_the_truncation_warning() {
    let repo = init_test_repo();
    let prompt = "p".repeat(2100);
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "start", "eta", "--minimal", "--json", "--prompt"])
        .arg(&prompt)
        .output()
        .expect("run feature start");

    assert!(output.status.success(), "{}", stderr_of(&output));
    let mut summary = assert_single_json(&output);
    let notice = "Prompt truncated to 2000 characters before storage.";
    assert!(
        summary["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning == notice),
        "{summary:#}"
    );
    assert!(
        stderr_of(&output).contains(&format!("⚠️  {notice}")),
        "the human notice moves to stderr in JSON mode"
    );
    assert_eq!(
        summary["prompt_seed"].as_str().unwrap().chars().count(),
        2000
    );

    summary["prompt_seed"] = json!("<2000 chars>");
    assert_fixture(
        AREA,
        "start_minimal_truncated_prompt",
        &normalize_json(summary, &[repo.root()]),
    );
}

#[test]
fn start_text_mode_keeps_the_truncation_notice_on_stdout() {
    let repo = init_test_repo();
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "start", "eta", "--minimal", "--prompt"])
        .arg("p".repeat(2001))
        .output()
        .expect("run feature start");

    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("⚠️  Prompt truncated to 2000 characters before storage.\n"),
        "{stdout}"
    );
    assert_eq!(
        stdout.matches("Prompt truncated").count(),
        1,
        "text mode shows the notice once, not again under Warnings: {stdout}"
    );
}

#[test]
fn list_outside_a_repository_prints_the_not_a_git_repository_envelope() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = branchbox_cmd!(temp.path())
        .args(["feature", "list", "--json", "--repo", "/nonexistent"])
        .output()
        .expect("run feature list");

    let envelope = assert_envelope(&output, "not_a_git_repository");
    assert_eq!(
        stderr_of(&output),
        "Error: Validation error: Not a git repository: /nonexistent\n",
        "stderr is byte-compatible with 0.13.4"
    );
    assert_fixture(AREA, "envelope_not_a_git_repository", &envelope);
}

#[test]
fn text_mode_failures_print_no_envelope() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = branchbox_cmd!(temp.path())
        .args(["feature", "list", "--repo", "/nonexistent"])
        .output()
        .expect("run feature list");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    assert_eq!(
        stderr_of(&output),
        "Error: Validation error: Not a git repository: /nonexistent\n"
    );
}

#[test]
fn teardown_of_an_unknown_feature_prints_the_worktree_not_found_envelope() {
    let repo = init_test_repo();
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "teardown", "nope", "--json"])
        .output()
        .expect("run feature teardown");

    let envelope = normalize_json(
        assert_envelope(&output, "worktree_not_found"),
        &[repo.root()],
    );
    assert_eq!(
        envelope["error"]["details"],
        json!({"name": "nope", "path": "<repo>/nope"})
    );
    assert_fixture(AREA, "envelope_worktree_not_found", &envelope);
}

#[test]
fn tunnel_remove_of_an_unregistered_feature_prints_the_feature_not_found_envelope() {
    let repo = init_test_repo();
    let output = branchbox_cmd!(repo.path())
        .args(["tunnel", "remove", "nope", "--json"])
        .output()
        .expect("run tunnel remove");

    let envelope = normalize_json(
        assert_envelope(&output, "feature_not_found"),
        &[repo.root()],
    );
    assert_eq!(
        envelope["error"]["details"],
        json!({"name": "nope", "registry": "<repo>/main/.branchbox/registry.json"})
    );
    assert_fixture(AREA, "envelope_feature_not_found", &envelope);
}

#[cfg(unix)]
#[test]
fn agent_status_without_a_socket_prints_the_agent_unreachable_envelope() {
    let temp = tempfile::TempDir::new().unwrap();
    let socket = temp.path().join("missing.sock");
    let output = branchbox_cmd!(temp.path(), "BRANCHBOX_AGENT_SOCKET" => &socket)
        .args(["agent", "status", "--json"])
        .output()
        .expect("run agent status");

    let envelope = assert_envelope(&output, "agent_unreachable");
    assert_eq!(
        stderr_of(&output),
        format!(
            "Error: failed to connect to BranchBox agent at {}\n\nCaused by:\n    No such file \
             or directory (os error 2)\n",
            socket.display()
        ),
        "stderr is byte-compatible with 0.13.4"
    );
    assert_fixture(
        AREA,
        "envelope_agent_unreachable",
        &normalize_json(envelope, &[temp.path()]),
    );
}

/// Argument refusals are usage errors, not crashes: in `--json` mode they carry
/// `validation_failed` (not `internal`), and the text-mode report is unchanged.
#[test]
fn argument_refusals_print_the_validation_failed_envelope() {
    let repo = init_test_repo();
    for args in [
        &["feature", "list", "--status", "bogus", "--json"][..],
        &["feature", "start", "eta", "--default-prompt", "--json"],
    ] {
        let output = branchbox_cmd!(repo.path())
            .args(args)
            .output()
            .expect("run branchbox");
        let envelope = assert_envelope(&output, "validation_failed");
        assert_eq!(envelope["error"]["details"], Value::Null, "{args:?}");
        assert_eq!(envelope["error"]["causes"], json!([]), "{args:?}");
    }

    let output = branchbox_cmd!(repo.path())
        .args(["feature", "start", "eta", "--default-prompt"])
        .output()
        .expect("run feature start");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        stderr_of(&output),
        "Error: --default-prompt can only be used with --minimal or --fast\n"
    );
    assert!(!repo.root().join("eta").exists(), "nothing was started");
}

#[test]
fn exec_failure_prints_only_the_in_band_payload() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "exec", "eta", "--json", "--"])
        .args(["sh", "-c", "echo out; echo err >&2; exit 3"])
        .output()
        .expect("run feature exec");

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let payload = assert_single_json(&output);
    assert_eq!(
        payload,
        json!({"exit_code": 3, "stdout": "out\n", "stderr": "err\n"}),
        "no envelope is added to an in-band failure"
    );
    assert!(stderr_of(&output).contains("Error: Runtime command exited with status 3"));
    assert_fixture(AREA, "exec_inband_failure", &payload);
}

#[test]
fn runtime_capabilities_is_one_json_document_and_matches_the_version_contract() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = branchbox_cmd!(temp.path())
        .arg("runtime-capabilities")
        .output()
        .expect("run runtime-capabilities outside a repository");
    assert!(output.status.success(), "{}", stderr_of(&output));
    let payload = assert_single_json(&output);
    assert_eq!(
        payload,
        json!({
            "schema_version": 1,
            "managed_workspace_contract_v1": true,
            "preloaded_compose_sanitization_v1": true,
        })
    );
    assert_fixture(AREA, "runtime_capabilities", &payload);

    let version = branchbox_cmd!(temp.path())
        .args(["version", "--json"])
        .output()
        .unwrap();
    assert!(version.status.success(), "{}", stderr_of(&version));
    let version = assert_single_json(&version);
    let capabilities = version["capabilities"].as_array().unwrap();
    for (key, capability) in [
        (
            "managed_workspace_contract_v1",
            "managed-workspace-contract",
        ),
        (
            "preloaded_compose_sanitization_v1",
            "preloaded-compose-sanitization",
        ),
    ] {
        assert_eq!(payload[key], capabilities.contains(&json!(capability)));
    }
}

#[test]
fn version_json_reports_the_contract_and_capabilities() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = branchbox_cmd!(temp.path())
        .args(["version", "--json"])
        .output()
        .expect("run version");

    assert!(output.status.success(), "{}", stderr_of(&output));
    let payload = assert_single_json(&output);
    let mut keys: Vec<&str> = payload
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["capabilities", "contract_version", "version"]);
    assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(payload["contract_version"], 1);
    let capabilities: Vec<&str> = payload["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|capability| capability.as_str().unwrap())
        .collect();
    assert_eq!(capabilities.first(), Some(&"json-error-envelope"));
    for expected in [
        "json-error-envelope",
        "registry-lock",
        "write-ahead-start",
        "managed-workspace-contract",
        "preloaded-compose-sanitization",
    ] {
        assert!(capabilities.contains(&expected), "{capabilities:?}");
    }

    let text = branchbox_cmd!(temp.path()).arg("version").output().unwrap();
    let flag = branchbox_cmd!(temp.path())
        .arg("--version")
        .output()
        .unwrap();
    assert!(text.status.success() && flag.status.success());
    assert_eq!(
        String::from_utf8_lossy(&text.stdout),
        format!("branchbox {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(text.stdout, flag.stdout, "`version` matches `--version`");
}

#[test]
fn list_reports_a_deleted_worktree_as_orphaned() {
    let repo = init_test_repo();
    let started = start_minimal(&repo, "eta");
    let worktree = Path::new(started["worktree_path"].as_str().unwrap());
    fs::remove_dir_all(worktree).expect("delete the worktree directory");

    let listed = list_json(&repo);
    let entries = listed.as_array().unwrap();
    assert_eq!(entries.len(), 1, "{listed:#}");
    assert_eq!(entries[0]["status"], "orphaned");
    assert_fixture(
        AREA,
        "list_orphaned",
        &normalize_json(listed, &[repo.root()]),
    );

    // The registry itself is unchanged; orphaned is computed at list time.
    let registry: Value =
        serde_json::from_slice(&fs::read(repo.path().join(".branchbox/registry.json")).unwrap())
            .unwrap();
    assert_eq!(registry["features"][0]["status"], "active");
}

/// A fake `sbx` that SIGKILLs its parent, `branchbox feature start`, when asked to create the
/// sandbox: the worktree and the write-ahead entry exist by then, the runtime does not.
#[cfg(unix)]
fn create_killing_sbx(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let binary = dir.join("sbx");
    fs::write(
        &binary,
        "#!/bin/sh\ncase \"$1\" in\n  create) kill -9 \"$PPID\" ;;\nesac\nexit 0\n",
    )
    .expect("write fake sbx");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    binary
}

#[cfg(unix)]
#[test]
fn list_reports_a_killed_start_as_interrupted() {
    use std::os::unix::process::ExitStatusExt;

    let repo = init_test_repo();
    fs::create_dir(repo.path().join(".devcontainer")).unwrap();
    fs::write(
        repo.path().join(".devcontainer/devcontainer.json"),
        "{\n  \"name\": \"test\",\n  \"image\": \"alpine:3.19\"\n}\n",
    )
    .unwrap();
    repo.git(&["add", ".devcontainer"]);
    repo.git(&["commit", "-q", "-m", "Add devcontainer"]);
    let tools = tempfile::TempDir::new().unwrap();
    let sbx = create_killing_sbx(tools.path());

    // `output()` waits for (and so reaps) the killed process before the list below; an unreaped
    // zombie would still count as a live pid.
    let output = branchbox_cmd!(repo.path(), "BRANCHBOX_SBX_PATH" => &sbx)
        .args([
            "feature",
            "start",
            "eta",
            "--minimal",
            "--runtime",
            "sbx",
            "--json",
        ])
        .output()
        .expect("run feature start");
    assert_eq!(output.status.signal(), Some(9), "{}", stderr_of(&output));
    assert!(output.stdout.is_empty(), "a killed start prints nothing");
    assert!(repo.root().join("eta").is_dir(), "the worktree was created");

    let listed = list_json(&repo);
    let entries = listed.as_array().unwrap();
    assert_eq!(entries.len(), 1, "{listed:#}");
    assert_eq!(entries[0]["status"], "active");
    assert_eq!(entries[0]["setup"]["state"], "interrupted");
    assert_eq!(entries[0]["runtime"]["provider"], "sbx");
    assert_fixture(
        AREA,
        "list_interrupted",
        &normalize_json(listed, &[repo.root()]),
    );

    // The stored record still says in_progress; interrupted is computed at list time.
    let registry: Value =
        serde_json::from_slice(&fs::read(repo.path().join(".branchbox/registry.json")).unwrap())
            .unwrap();
    assert_eq!(registry["features"][0]["setup"]["state"], "in_progress");
}

/// Text mode is unchanged: the phrases `scripts/manual-cli-e2e.sh` greps for still reach
/// stdout, and in `--json` mode the same refusal becomes one envelope with the banner kept off
/// stdout.
#[test]
fn text_mode_keeps_the_manual_harness_phrases_on_stdout() {
    let repo = init_test_repo();
    let start = branchbox_cmd!(repo.path())
        .args(["feature", "start", "eta", "--minimal"])
        .output()
        .expect("run feature start");
    assert!(start.status.success(), "{}", stderr_of(&start));
    let stdout = String::from_utf8_lossy(&start.stdout);
    assert!(stdout.contains("Tunnel          | ⏭ disabled"), "{stdout}");

    let sync = branchbox_cmd!(repo.path())
        .args(["devcontainer", "sync", "--dry-run", "--strategy", "copy"])
        .output()
        .expect("run devcontainer sync");
    assert!(sync.status.success(), "{}", stderr_of(&sync));
    let stdout = String::from_utf8_lossy(&sync.stdout);
    assert!(stdout.contains("  eta ... would sync"), "{stdout}");

    // A module file changed inside the worktree: teardown refuses without --force.
    let worktree = repo.root().join("eta");
    fs::create_dir_all(worktree.join(".devcontainer")).unwrap();
    fs::write(worktree.join(".devcontainer/local.json"), "{}\n").unwrap();
    let teardown = branchbox_cmd!(repo.path())
        .args(["feature", "teardown", "eta"])
        .output()
        .expect("run feature teardown");
    assert_eq!(teardown.status.code(), Some(1), "{}", stderr_of(&teardown));
    let stdout = String::from_utf8_lossy(&teardown.stdout);
    assert!(
        stdout.starts_with("⚠️  Detected devcontainer/compose changes inside "),
        "{stdout}"
    );
    assert!(stderr_of(&teardown).contains(
        "Error: Devcontainer/compose changes detected; rerun this command with --force to proceed."
    ));

    let refused = branchbox_cmd!(repo.path())
        .args(["feature", "teardown", "eta", "--json"])
        .output()
        .expect("run feature teardown --json");
    assert_envelope(&refused, "teardown_refused");
    assert!(
        worktree.is_dir(),
        "a refused teardown leaves the worktree in place"
    );
}
