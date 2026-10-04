//! The RS-3 JSON surface (DESIGN §5.1, §5.7-§5.8): `detect --json`, `devcontainer sync --json`
//! and the `tunnel open/remove --json` contract (C9), their refusals, and the text output they
//! keep. Golden documents live in `fixtures/contract/commands/`; rerun with
//! `UPDATE_CONTRACT_FIXTURES=1` to rewrite them after an intended change.
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Output;
use support::{assert_fixture, assert_single_json, init_test_repo, normalize_json, TestRepo};

const AREA: &str = "commands";

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
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
    envelope
}

fn start_minimal(repo: &TestRepo, name: &str) {
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "start", name, "--minimal", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "{}", stderr_of(&output));
}

/// Give the main worktree a `.devcontainer` to sync from.
fn add_devcontainer_source(repo: &TestRepo) {
    let source = repo.path().join(".devcontainer");
    fs::create_dir_all(source.join("scripts")).unwrap();
    fs::write(source.join("devcontainer.json"), "{\"name\": \"demo\"}\n").unwrap();
    fs::write(source.join("scripts/setup.sh"), "#!/bin/sh\n").unwrap();
    // Excluded from syncs: secrets and generated files stay per worktree.
    fs::write(source.join(".env"), "SECRET=1\n").unwrap();
}

fn sync(repo: &TestRepo, args: &[&str]) -> Output {
    branchbox_cmd!(repo.path())
        .args(["devcontainer", "sync"])
        .args(args)
        .output()
        .expect("run devcontainer sync")
}

fn registry_entry(repo: &TestRepo, name: &str) -> Value {
    let registry: Value =
        serde_json::from_slice(&fs::read(repo.path().join(".branchbox/registry.json")).unwrap())
            .unwrap();
    registry["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["work_feature"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("{name} not in {registry:#}"))
}

// --- detect ---------------------------------------------------------------------------------

#[test]
fn detect_json_describes_an_initialized_repository() {
    let repo = init_test_repo();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    fs::create_dir_all(repo.path().join(".devcontainer")).unwrap();
    fs::write(repo.path().join(".devcontainer/Dockerfile"), "FROM rust\n").unwrap();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    fs::write(
        repo.path().join(".branchbox/registry.json"),
        "{\"version\": \"1\", \"features\": []}\n",
    )
    .unwrap();

    let output = branchbox_cmd!(repo.root())
        .args(["detect", "--json", "-p"])
        .arg(repo.path())
        .output()
        .expect("run detect");
    assert!(output.status.success(), "{}", stderr_of(&output));
    let detection = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(
        detection,
        json!({
            "schema_version": 1,
            "project": "<repo>/main",
            "git_repository": true,
            "initialized": true,
            "stack": "rust",
            "adapter": "generic",
            "modules": ["devcontainer", "compose", "tunnel", "specs"],
            "has_devcontainer": true,
            "has_env": true,
            "warnings": []
        })
    );
    assert_fixture(AREA, "detect_initialized", &detection);
}

#[test]
fn detect_json_outside_a_repository() {
    let temp = tempfile::TempDir::new().unwrap();
    fs::write(temp.path().join("package.json"), "{}").unwrap();
    let output = branchbox_cmd!(temp.path())
        .args(["detect", "--json"])
        .output()
        .expect("run detect");
    assert!(output.status.success(), "{}", stderr_of(&output));
    let detection = normalize_json(assert_single_json(&output), &[temp.path()]);
    assert_eq!(detection["project"], "<repo>");
    assert_eq!(detection["git_repository"], false);
    assert_eq!(detection["initialized"], false);
    assert_eq!(detection["stack"], "nodejs");
    assert_eq!(detection["adapter"], "nodejs");
    assert_eq!(detection["has_env"], false);
    assert_fixture(AREA, "detect_plain_folder", &detection);
}

#[test]
fn detect_json_refuses_a_missing_folder() {
    let temp = tempfile::TempDir::new().unwrap();
    let missing = temp.path().join("nope");
    let output = branchbox_cmd!(temp.path())
        .args(["detect", "--json", "--path"])
        .arg(&missing)
        .output()
        .expect("run detect");
    let envelope = assert_envelope(&output, "validation_failed");
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains(&missing.display().to_string()));
}

#[test]
fn detect_text_output_is_unchanged() {
    let repo = init_test_repo();
    fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    let output = branchbox_cmd!(repo.path())
        .arg("detect")
        .output()
        .expect("run detect");
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(
        stdout_of(&output),
        "📦 BranchBox Configuration\n\nProject: .\nStack: Rust\nAdapter: Generic\n\n\
         Enabled modules: 2\n  ✓ tunnel\n  ✓ specs\n"
    );
}

// --- devcontainer sync ----------------------------------------------------------------------

#[test]
fn sync_with_no_active_features_reports_no_results() {
    let repo = init_test_repo();
    let output = sync(&repo, &["--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let report = assert_single_json(&output);
    assert_eq!(
        report,
        json!({
            "schema_version": 1,
            "dry_run": false,
            "strategy": "copy",
            "results": [],
            "synced": 0,
            "failed": 0,
            "skipped": 0
        })
    );
    assert_fixture(AREA, "sync_none_active", &report);

    let text = sync(&repo, &[]);
    assert!(text.status.success());
    assert_eq!(stdout_of(&text), "No active feature worktrees found\n");
}

#[test]
fn sync_copies_the_source_records_the_baseline_and_updates_the_registry() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");
    add_devcontainer_source(&repo);
    let baseline = repo.path().join(".branchbox/devcontainer-sync/eta.json");

    let output = sync(&repo, &["--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let report = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(
        report,
        json!({
            "schema_version": 1,
            "dry_run": false,
            "strategy": "copy",
            "results": [{
                "work_feature": "eta",
                "worktree_path": "<repo>/eta",
                "status": "synced",
                "files": ["devcontainer.json", "scripts/setup.sh"],
                "skip_reason": null,
                "error": null,
                "registry_updated": true
            }],
            "synced": 1,
            "failed": 0,
            "skipped": 0
        })
    );
    assert_fixture(AREA, "sync_synced", &report);

    let worktree = repo.root().join("eta/.devcontainer");
    assert_eq!(
        fs::read_to_string(worktree.join("devcontainer.json")).unwrap(),
        "{\"name\": \"demo\"}\n"
    );
    assert_ne!(
        fs::read_to_string(worktree.join(".env")).ok().as_deref(),
        Some("SECRET=1\n"),
        "excluded files are not synced"
    );

    let recorded: Value = serde_json::from_slice(&fs::read(&baseline).unwrap()).unwrap();
    let keys: Vec<&str> = recorded
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["devcontainer.json", "scripts/setup.sh"]);

    let entry = registry_entry(&repo, "eta");
    assert_eq!(entry["sync_strategy"], "copy");
    assert_eq!(entry["devcontainer_outdated"], false);
    assert!(entry["last_sync_at"].is_string(), "{entry:#}");

    // Text mode prints the 0.13 lines.
    let text = sync(&repo, &[]);
    assert!(text.status.success(), "{}", stderr_of(&text));
    let stdout = stdout_of(&text);
    assert!(
        stdout.contains("  eta ... ✓ synced 2 files (copy)\n"),
        "{stdout}"
    );
    assert!(stdout.contains("✓ Successfully synced 1 feature worktree(s)"));
}

#[test]
fn sync_dry_run_reports_would_sync_and_changes_nothing() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");
    add_devcontainer_source(&repo);

    let output = sync(&repo, &["--json", "--dry-run"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let report = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["results"][0]["status"], "would_sync");
    assert_eq!(report["results"][0]["files"], json!([]));
    assert_eq!(report["results"][0]["registry_updated"], false);
    assert_eq!(report["synced"], 1);
    assert_fixture(AREA, "sync_dry_run", &report);

    assert!(!repo
        .root()
        .join("eta/.devcontainer/devcontainer.json")
        .exists());
    assert!(!repo
        .path()
        .join(".branchbox/devcontainer-sync/eta.json")
        .exists());
    assert!(registry_entry(&repo, "eta").get("last_sync_at").is_none());
}

#[test]
fn sync_feature_selects_named_features_only() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");
    start_minimal(&repo, "zeta");
    add_devcontainer_source(&repo);

    let output = sync(&repo, &["--json", "--feature", "zeta", "--feature", "zeta"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let report = assert_single_json(&output);
    let synced: Vec<&str> = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["work_feature"].as_str().unwrap())
        .collect();
    assert_eq!(synced, ["zeta"]);
    assert!(repo
        .root()
        .join("zeta/.devcontainer/devcontainer.json")
        .exists());
    assert!(!repo
        .root()
        .join("eta/.devcontainer/devcontainer.json")
        .exists());

    // Unknown names are refused before anything is synced.
    let output = sync(&repo, &["--json", "--feature", "eta", "--feature", "nope"]);
    let envelope = normalize_json(
        assert_envelope(&output, "feature_not_found"),
        &[repo.root()],
    );
    assert_eq!(
        envelope["error"]["details"],
        json!({"name": "nope", "registry": "<repo>/main/.branchbox/registry.json"})
    );
    assert_fixture(AREA, "envelope_sync_feature_not_found", &envelope);
    assert!(!repo
        .root()
        .join("eta/.devcontainer/devcontainer.json")
        .exists());
}

#[test]
fn sync_feature_refuses_a_removed_feature() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");
    let teardown = branchbox_cmd!(repo.path())
        .args(["feature", "teardown", "eta", "--force", "--json"])
        .output()
        .expect("run feature teardown");
    assert!(teardown.status.success(), "{}", stderr_of(&teardown));
    add_devcontainer_source(&repo);

    let output = sync(&repo, &["--json", "--feature", "eta"]);
    let envelope = assert_envelope(&output, "feature_not_found");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Feature 'eta' was removed"),
        "{envelope:#}"
    );
    assert_eq!(envelope["error"]["details"]["name"], "eta");
}

#[test]
fn sync_without_a_main_devcontainer_is_refused() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");

    let output = sync(&repo, &["--json"]);
    let envelope = normalize_json(
        assert_envelope(&output, "devcontainer_source_missing"),
        &[repo.root()],
    );
    assert_eq!(
        envelope["error"]["details"],
        json!({"path": "<repo>/main/.devcontainer"})
    );
    assert_fixture(AREA, "envelope_devcontainer_source_missing", &envelope);

    let text = sync(&repo, &[]);
    assert_eq!(text.status.code(), Some(1));
    assert!(text.stdout.is_empty(), "{}", stdout_of(&text));
    assert!(stderr_of(&text).contains("Error: Devcontainer source not found:"));

    // A dry run still predicts per worktree, as 0.13 did.
    let dry_run = sync(&repo, &["--dry-run"]);
    assert!(dry_run.status.success(), "{}", stderr_of(&dry_run));
    assert!(stdout_of(&dry_run).contains("  eta ... would sync"));
}

#[test]
fn sync_reports_a_missing_worktree_as_skipped() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");
    add_devcontainer_source(&repo);
    // A worktree that vanished is listed as orphaned, so only --feature still selects it.
    fs::remove_dir_all(repo.root().join("eta")).unwrap();

    let output = sync(&repo, &["--json", "--feature", "eta"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let report = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(report["results"][0]["status"], "skipped");
    assert_eq!(
        report["results"][0]["skip_reason"],
        "worktree not found at <repo>/eta"
    );
    assert_eq!(report["skipped"], 1);
}

/// Makes `dir` read-only for the duration of a test and restores it afterwards, so the temp
/// directory can still be removed.
#[cfg(unix)]
struct ReadOnly<'a>(&'a Path);

#[cfg(unix)]
impl<'a> ReadOnly<'a> {
    fn new(dir: &'a Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
        Self(dir)
    }

    /// Whether the permission actually binds (it does not for root).
    fn enforced(&self) -> bool {
        let probe = self.0.join(".write-probe");
        let writable = fs::write(&probe, "").is_ok();
        let _ = fs::remove_file(&probe);
        !writable
    }
}

#[cfg(unix)]
impl Drop for ReadOnly<'_> {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(self.0, fs::Permissions::from_mode(0o755));
    }
}

#[cfg(unix)]
#[test]
fn a_failed_worktree_is_reported_and_exits_1_in_both_modes() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");
    add_devcontainer_source(&repo);
    let target = repo.root().join("eta/.devcontainer");
    fs::create_dir_all(&target).unwrap();
    let read_only = ReadOnly::new(&target);
    if !read_only.enforced() {
        eprintln!("skipping: permissions are not enforced for this user");
        return;
    }

    let output = sync(&repo, &["--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let report = normalize_json(assert_single_json(&output), &[repo.root()]);
    let result = &report["results"][0];
    assert_eq!(result["status"], "failed");
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("Permission denied"),
        "{report:#}"
    );
    assert_eq!(result["registry_updated"], true);
    assert_eq!(report["failed"], 1);
    assert!(
        report.get("error").is_none(),
        "an in-band payload, not an envelope"
    );
    assert!(
        stderr_of(&output)
            .contains("Error: Devcontainer sync failed for 1 of 1 feature worktree(s): eta"),
        "{}",
        stderr_of(&output)
    );
    assert_eq!(registry_entry(&repo, "eta")["devcontainer_outdated"], true);

    let text = sync(&repo, &[]);
    assert_eq!(text.status.code(), Some(1), "{}", stderr_of(&text));
    let stdout = stdout_of(&text);
    assert!(stdout.contains("  eta ... ✗ failed: "), "{stdout}");
    assert!(stdout.contains("⚠️  1 error(s) occurred:"), "{stdout}");
}

#[test]
fn version_lists_every_rs3_capability() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = branchbox_cmd!(temp.path())
        .args(["version", "--json"])
        .output()
        .expect("run version");
    let payload = assert_single_json(&output);
    let capabilities = payload["capabilities"].as_array().unwrap();
    for expected in [
        "detect-json",
        "devcontainer-sync-json",
        "config",
        "tunnel-credentials",
        "doctor",
        "init-json",
    ] {
        assert_eq!(
            capabilities
                .iter()
                .filter(|capability| *capability == expected)
                .count(),
            1,
            "{expected} not listed exactly once in {payload:#}"
        );
    }
}

// --- tunnel (C9) ----------------------------------------------------------------------------

fn tunnel(repo: &TestRepo, args: &[&str]) -> Output {
    branchbox_cmd!(repo.path())
        .arg("tunnel")
        .args(args)
        .output()
        .expect("run branchbox tunnel")
}

/// Without Cloudflare credentials, `tunnel open` records manual setup instructions (no network
/// call), and `tunnel remove` turns the tunnel off; both payloads keep their 0.13 shape.
#[test]
fn tunnel_open_and_remove_json_payloads() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");

    let opened = tunnel(&repo, &["open", "eta", "--json"]);
    assert!(opened.status.success(), "{}", stderr_of(&opened));
    let opened = normalize_json(assert_single_json(&opened), &[repo.root()]);
    assert_eq!(opened["work_feature"], "eta");
    assert_eq!(opened["state"]["status"], "manual");
    assert_eq!(opened["state"]["hostname"], "dev-eta.example.com");
    assert_fixture(AREA, "tunnel_open_manual", &opened);
    assert_eq!(registry_entry(&repo, "eta")["tunnel"]["status"], "manual");

    let removed = tunnel(&repo, &["remove", "eta", "--json"]);
    assert!(removed.status.success(), "{}", stderr_of(&removed));
    let removed = normalize_json(assert_single_json(&removed), &[repo.root()]);
    assert_eq!(removed["previous_state"], opened["state"]);
    assert_eq!(removed["updated_state"]["status"], "disabled");
    assert_fixture(AREA, "tunnel_remove", &removed);
    assert_eq!(registry_entry(&repo, "eta")["tunnel"]["status"], "disabled");

    // Text mode keeps its 0.13 report.
    let text = tunnel(&repo, &["open", "eta"]);
    assert!(text.status.success(), "{}", stderr_of(&text));
    let stdout = stdout_of(&text);
    assert!(
        stdout.starts_with("🌐 Tunnel\n  Feature: eta\n  Status: manual\n"),
        "{stdout}"
    );
}

#[test]
fn tunnel_refusals_print_envelopes() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");

    let envelope = assert_envelope(
        &tunnel(&repo, &["open", "nope", "--json"]),
        "feature_not_found",
    );
    assert_eq!(
        normalize_json(envelope["error"]["details"].clone(), &[repo.root()]),
        json!({"name": "nope", "registry": "<repo>/main/.branchbox/registry.json"})
    );
    assert_fixture(
        AREA,
        "envelope_tunnel_open_feature_not_found",
        &normalize_json(envelope, &[repo.root()]),
    );

    // An entry from an older BranchBox may carry no tunnel metadata at all.
    let registry_path = repo.path().join(".branchbox/registry.json");
    let mut registry: Value = serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
    for entry in registry["features"].as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("tunnel");
    }
    fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).unwrap(),
    )
    .unwrap();
    let envelope = assert_envelope(
        &tunnel(&repo, &["remove", "eta", "--json"]),
        "validation_failed",
    );
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("has no tunnel metadata recorded"));
    assert_fixture(
        AREA,
        "envelope_tunnel_remove_no_metadata",
        &normalize_json(envelope, &[repo.root()]),
    );

    let disabled = branchbox_cmd!(repo.path())
        .args(["config", "set", "tunnel.enabled", "false"])
        .output()
        .expect("run config set");
    assert!(disabled.status.success(), "{}", stderr_of(&disabled));
    let envelope = assert_envelope(
        &tunnel(&repo, &["open", "eta", "--json"]),
        "validation_failed",
    );
    assert_fixture(
        AREA,
        "envelope_tunnel_disabled",
        &normalize_json(envelope, &[repo.root()]),
    );
    assert!(registry_entry(&repo, "eta")
        .get("tunnel")
        .is_none_or(Value::is_null));
}
