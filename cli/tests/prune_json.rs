//! `prune --json` and `--feature` (DESIGN §5.9, capability `prune-json`). Prune keeps its
//! documented forced semantics: it discards uncommitted changes and force-deletes branches when
//! deleting them. The dry run says, per candidate, what that will destroy. Golden documents live
//! in `fixtures/contract/teardown/`; rerun with `UPDATE_CONTRACT_FIXTURES=1` to rewrite them.
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

#[path = "support/empty_docker.rs"]
mod empty_docker;

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use support::{assert_fixture, assert_single_json, git, init_test_repo, normalize_json, TestRepo};

const AREA: &str = "teardown";

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn branchbox(repo: &TestRepo, args: &[&str]) -> Output {
    branchbox_cmd!(repo.path())
        .args(args)
        .output()
        .expect("run branchbox")
}

fn start_minimal(repo: &TestRepo, name: &str) -> PathBuf {
    let output = branchbox(repo, &["feature", "start", name, "--minimal", "--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let summary = assert_single_json(&output);
    PathBuf::from(summary["worktree_path"].as_str().unwrap())
}

/// Two features: `risky` has an unmerged commit and an untracked file, `clean` has neither.
/// `clean` starts last, so it is listed first (most recently updated).
fn two_features(repo: &TestRepo) -> (PathBuf, PathBuf) {
    let risky = start_minimal(repo, "risky");
    fs::write(risky.join("work.txt"), "work\n").unwrap();
    git(&risky, &["add", "work.txt"]);
    git(&risky, &["commit", "-q", "-m", "Work in progress"]);
    fs::write(risky.join("scratch.txt"), "scratch\n").unwrap();
    let clean = start_minimal(repo, "clean");
    (risky, clean)
}

fn assert_ok_json(output: &Output) -> Value {
    assert!(output.status.success(), "{}", stderr_of(output));
    assert_single_json(output)
}

fn assert_envelope(output: &Output, code: &str) -> Value {
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(output));
    let envelope = assert_single_json(output);
    assert_eq!(envelope["error"]["code"], code, "{envelope:#}");
    envelope
}

fn active_features(repo: &TestRepo) -> Vec<String> {
    let listed = assert_ok_json(&branchbox(repo, &["feature", "list", "--json"]));
    let mut names: Vec<String> = listed
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["status"] == "active")
        .map(|entry| entry["work_feature"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn dry_run_json_lists_every_candidate_with_its_plan_and_what_is_at_risk() {
    let repo = init_test_repo();
    let (risky, clean) = two_features(&repo);

    let report = assert_ok_json(&branchbox(&repo, &["prune", "--dry-run", "--json"]));

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["dry_run"], true);
    assert_eq!(
        report["policy"],
        json!({"delete_branch": true, "force_delete_branch": true, "discard_changes": true,
               "complete_spec": false})
    );
    let candidates = report["candidates"].as_array().unwrap();
    let names: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate["work_feature"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["clean", "risky"]);
    for candidate in candidates {
        assert_eq!(candidate["plan"]["schema_version"], 1, "{candidate:#}");
        assert_eq!(candidate["plan"]["blockers"], json!([]), "prune is forced");
        assert_eq!(candidate["plan"]["branch"]["action"], "force_delete");
    }
    assert_eq!(
        report["at_risk"],
        json!({
            "uncommitted_changes": [{"work_feature": "risky", "count": 1, "truncated": false,
                                     "paths": ["scratch.txt"]}],
            "unmerged_commits": [{"work_feature": "risky", "branch": "feature/risky", "ahead": 1}]
        })
    );
    assert!(risky.join("scratch.txt").exists() && clean.exists());
    assert_eq!(active_features(&repo), ["clean", "risky"]);
    assert_fixture(
        AREA,
        "prune_dry_run",
        &normalize_json(report, &[repo.root()]),
    );
}

#[test]
fn text_dry_run_says_what_each_feature_loses() {
    let repo = init_test_repo();
    two_features(&repo);

    let output = branchbox(&repo, &["feature", "prune", "--dry-run"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("  - clean\n"), "{stdout}");
    assert!(
        stdout.contains(
            "  - risky (1 uncommitted change discarded; branch feature/risky force-deleted with \
             1 unmerged commit)"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("Dry run only; no teardown executed."));
}

#[test]
fn yes_json_prunes_every_candidate_and_reports_each_result() {
    let repo = init_test_repo();
    let (risky, clean) = two_features(&repo);

    let docker = empty_docker::EmptyDocker::new();
    let output = docker
        .command(branchbox_cmd!(repo.path()))
        .args(["prune", "--yes", "--json"])
        .output()
        .expect("prune with empty engine");
    let report = assert_ok_json(&output);

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["dry_run"], false);
    assert_eq!(report["pruned"], 2);
    assert_eq!(report["failed"], 0);
    for result in report["results"].as_array().unwrap() {
        assert_eq!(result["outcome"], "removed", "{result:#}");
        assert_eq!(result["error"], Value::Null);
        assert_eq!(result["summary"]["worktree_removed"], true);
        assert_eq!(result["summary"]["branch_action"], "force_delete");
    }
    assert!(!risky.exists() && !clean.exists());
    assert!(active_features(&repo).is_empty());
    docker.assert_probed(&[&risky, &clean]);
    assert_fixture(
        AREA,
        "prune_execute",
        &normalize_json(report, &[repo.root()]),
    );
}

#[test]
fn feature_selection_prunes_only_the_named_features() {
    let repo = init_test_repo();
    let (risky, clean) = two_features(&repo);

    let report = assert_ok_json(&branchbox(
        &repo,
        &["prune", "--yes", "--json", "--feature", "clean"],
    ));
    let names: Vec<&str> = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["work_feature"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["clean"]);
    assert!(!clean.exists());
    assert!(
        risky.join("scratch.txt").exists(),
        "unselected work is untouched"
    );
    assert_eq!(active_features(&repo), ["risky"]);

    let dry = assert_ok_json(&branchbox(
        &repo,
        &[
            "feature",
            "prune",
            "--dry-run",
            "--json",
            "--feature",
            "risky",
            "--keep-branch",
        ],
    ));
    assert_eq!(dry["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(dry["policy"]["delete_branch"], false);
    assert_eq!(dry["candidates"][0]["plan"]["branch"]["action"], "keep");
    assert_eq!(dry["at_risk"]["unmerged_commits"], json!([]));
}

#[test]
fn an_unknown_feature_selection_refuses_before_pruning_anything() {
    let repo = init_test_repo();
    let (risky, clean) = two_features(&repo);

    let envelope = assert_envelope(
        &branchbox(
            &repo,
            &[
                "prune",
                "--yes",
                "--json",
                "--feature",
                "clean",
                "--feature",
                "nope",
            ],
        ),
        "feature_not_found",
    );
    assert_eq!(envelope["error"]["details"]["name"], "nope");
    assert!(risky.exists() && clean.exists());
}

#[test]
fn machine_mode_without_yes_requires_confirmation() {
    let repo = init_test_repo();
    let (risky, clean) = two_features(&repo);

    let envelope = assert_envelope(
        &branchbox(&repo, &["prune", "--json"]),
        "confirmation_required",
    );
    assert_eq!(envelope["error"]["details"], json!({"count": 2}));
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--yes"));
    assert!(risky.exists() && clean.exists());

    // Text mode without a terminal refuses with the same cause.
    let text = branchbox(&repo, &["feature", "prune"]);
    assert_eq!(text.status.code(), Some(1));
    assert!(stderr_of(&text).contains(
        "Error: Refusing to prune in non-interactive mode without --yes. Rerun with --yes to confirm."
    ));
}

#[test]
fn nothing_to_prune_is_an_empty_report() {
    let repo = init_test_repo();
    let dry = assert_ok_json(&branchbox(&repo, &["prune", "--dry-run", "--json"]));
    assert_eq!(dry["candidates"], json!([]));
    let report = assert_ok_json(&branchbox(&repo, &["prune", "--json"]));
    assert_eq!(
        report,
        json!({"schema_version": 1, "dry_run": false, "results": [], "pruned": 0, "failed": 0})
    );
    let text = branchbox(&repo, &["prune"]);
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("No active or retained features"));
}

/// Add a registry entry whose name teardown rejects, so its row fails.
fn add_unprunable_entry(repo: &Path) {
    let path = repo.join(".branchbox/registry.json");
    let mut registry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let features = registry["features"].as_array_mut().unwrap();
    let mut broken = features[0].clone();
    broken["work_feature"] = json!("Not Valid");
    features.push(broken);
    fs::write(&path, serde_json::to_vec_pretty(&registry).unwrap()).unwrap();
}

#[test]
fn a_failed_row_prints_the_whole_report_and_exits_1() {
    let repo = init_test_repo();
    start_minimal(&repo, "fine");
    add_unprunable_entry(repo.path());

    let output = branchbox(&repo, &["prune", "--yes", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let report = assert_single_json(&output);
    assert_eq!(report["pruned"], 1);
    assert_eq!(report["failed"], 1);
    let failed = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["outcome"] == "failed")
        .expect("a failed row");
    assert_eq!(failed["work_feature"], "Not Valid");
    assert_eq!(failed["summary"], Value::Null);
    assert_eq!(failed["error"]["code"], "invalid_feature_name");
    assert!(
        report.get("error").is_none(),
        "an in-band failure adds no envelope"
    );
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("Error: Prune completed with failures."),
        "{stderr}"
    );

    let dry = assert_ok_json(&branchbox(&repo, &["prune", "--dry-run", "--json"]));
    let candidate = &dry["candidates"][0];
    assert_eq!(candidate["work_feature"], "Not Valid");
    assert_eq!(candidate["plan"], Value::Null);
    assert!(candidate["plan_error"]
        .as_str()
        .unwrap()
        .contains("Invalid feature name"));
}
