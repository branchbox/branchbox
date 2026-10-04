//! Teardown safety (DESIGN §10.2): `feature teardown` never deletes work it was not told to
//! discard, refuses before it changes anything, does not count BranchBox's own files as user
//! changes, refuses a non-interactive delete of an unmerged branch up front, and deletes the
//! branch the registry recorded.
//!
//! The first tests are the BUG-04 regression: 0.13.4 reported success after deleting a modified
//! `README.md` and an untracked `notes.txt` through a `remove_dir_all` fallback. Golden documents
//! live in `fixtures/contract/teardown/`; rerun with `UPDATE_CONTRACT_FIXTURES=1` to rewrite them
//! after an intended change.
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use support::{assert_fixture, assert_single_json, git, init_test_repo, normalize_json, TestRepo};

const AREA: &str = "teardown";

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Run `branchbox <args>` in the main worktree. stdin and stdout are pipes, so nothing is
/// interactive.
fn branchbox(repo: &TestRepo, args: &[&str]) -> Output {
    branchbox_cmd!(repo.path())
        .args(args)
        .output()
        .expect("run branchbox")
}

fn start(repo: &TestRepo, name: &str, extra: &[&str]) -> PathBuf {
    let mut args = vec!["feature", "start", name, "--json"];
    args.extend_from_slice(extra);
    let output = branchbox(repo, &args);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let summary = assert_single_json(&output);
    PathBuf::from(summary["worktree_path"].as_str().expect("worktree_path"))
}

fn start_minimal(repo: &TestRepo, name: &str) -> PathBuf {
    start(repo, name, &["--minimal"])
}

/// A success: exit 0 and one JSON document.
fn assert_ok_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout_of(output),
        stderr_of(output)
    );
    assert_single_json(output)
}

/// A failing `--json` command: exit 1, exactly one envelope with `code`, and the envelope's
/// message as the `Error:` line on stderr. Returns the envelope.
fn assert_envelope(output: &Output, code: &str) -> Value {
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(output));
    let envelope = assert_single_json(output);
    assert_eq!(envelope["schema_version"], 1, "{envelope:#}");
    assert_eq!(envelope["error"]["code"], code, "{envelope:#}");
    let message = envelope["error"]["message"].as_str().expect("message");
    assert!(
        stderr_of(output).contains(&format!("Error: {message}")),
        "{}",
        stderr_of(output)
    );
    envelope
}

/// The registry status of `name` (`feature list --all` keeps removed entries).
fn status_of(repo: &TestRepo, name: &str) -> String {
    let listed = assert_ok_json(&branchbox(repo, &["feature", "list", "--all", "--json"]));
    listed
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["work_feature"] == name)
        .unwrap_or_else(|| panic!("{name} not listed: {listed:#}"))["status"]
        .as_str()
        .unwrap()
        .to_string()
}

fn branch_exists(repo: &TestRepo, branch: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .current_dir(repo.path())
        .status()
        .expect("git show-ref")
        .success()
}

/// Commit one new file on the feature branch, so it has a commit `main` lacks.
fn commit_in(worktree: &Path, file: &str) {
    fs::write(worktree.join(file), "committed work\n").unwrap();
    git(worktree, &["add", file]);
    git(worktree, &["commit", "-q", "-m", &format!("Add {file}")]);
}

/// Make README.md modified and notes.txt untracked: the BUG-04 worktree.
fn dirty(worktree: &Path) {
    fs::write(worktree.join("README.md"), "# Test Repo\n\nlocal edit\n").unwrap();
    fs::write(worktree.join("notes.txt"), "my notes\n").unwrap();
}

/// Every file under `root`, with its contents (or its link target), for no-change checks.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.file_type().is_symlink() {
                let target = fs::read_link(&path).unwrap();
                files.insert(path, target.to_string_lossy().as_bytes().to_vec());
            } else if metadata.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    files
}

fn user_paths(plan: &Value) -> Vec<String> {
    plan["changes"]["user"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["path"].as_str().unwrap().to_string())
        .collect()
}

fn blocker_kinds(plan: &Value) -> Vec<String> {
    plan["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|blocker| blocker["kind"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn bug_04_teardown_without_force_keeps_modified_and_untracked_files() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    dirty(&worktree);

    let output = branchbox(&repo, &["feature", "teardown", "eta", "--keep-branch"]);

    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
    let stdout = stdout_of(&output);
    assert!(
        stdout.starts_with("⚠️  Detected uncommitted changes inside "),
        "{stdout}"
    );
    let stderr = stderr_of(&output);
    for named in [
        "README.md (modified)",
        "notes.txt (untracked)",
        "--discard-changes",
        "nothing was removed",
    ] {
        assert!(stderr.contains(named), "{named} missing from:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(worktree.join("README.md")).unwrap(),
        "# Test Repo\n\nlocal edit\n"
    );
    assert_eq!(
        fs::read_to_string(worktree.join("notes.txt")).unwrap(),
        "my notes\n"
    );
    assert!(worktree.join("docs/features/in-progress/eta.md").exists());
    assert_eq!(status_of(&repo, "eta"), "active");
    assert!(branch_exists(&repo, "feature/eta"));
}

#[test]
fn bug_04_json_refusal_is_one_envelope_carrying_the_plan() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    dirty(&worktree);

    let output = branchbox(
        &repo,
        &["feature", "teardown", "eta", "--keep-branch", "--json"],
    );

    let envelope = assert_envelope(&output, "teardown_refused");
    let details = &envelope["error"]["details"];
    assert_eq!(details["changed_anything"], false);
    assert_eq!(details["completed_steps"], json!([]));
    assert_eq!(
        details["plan"]["changes"]["user"],
        json!([
            {"path": "README.md", "kind": "modified", "area": "other"},
            {"path": "notes.txt", "kind": "untracked", "area": "other"}
        ])
    );
    assert_eq!(blocker_kinds(&details["plan"]), ["uncommitted_changes"]);
    assert!(worktree.join("notes.txt").exists());
    assert!(
        !stdout_of(&output).contains("Detected"),
        "the banner goes to stderr in JSON mode"
    );
    assert_fixture(
        AREA,
        "refusal_envelope",
        &normalize_json(envelope, &[repo.root()]),
    );
}

#[test]
fn discard_changes_removes_the_worktree_and_deletes_a_merged_branch_with_d() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    dirty(&worktree);

    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "eta", "--discard-changes", "--json"],
    ));

    assert_eq!(summary["worktree_removed"], true);
    assert_eq!(summary["branch_action"], "delete", "-d, not -D");
    assert_eq!(summary["branch_deleted"], true);
    assert_eq!(summary["branch_delete_error"], Value::Null);
    assert_eq!(summary["registry_updated"], true);
    assert_eq!(
        summary["discarded_changes"],
        json!([
            {"path": "README.md", "kind": "modified", "area": "other"},
            {"path": "notes.txt", "kind": "untracked", "area": "other"}
        ])
    );
    assert_eq!(
        summary["preserved"],
        json!([{"path": "docs/features/in-progress/eta.md",
                "destination": "docs/features/backlog/eta.md"}])
    );
    assert!(!worktree.exists());
    assert!(!branch_exists(&repo, "feature/eta"));
    assert!(repo.path().join("docs/features/backlog/eta.md").exists());
    assert_eq!(status_of(&repo, "eta"), "removed");
    assert_fixture(
        AREA,
        "summary_discard",
        &normalize_json(summary, &[repo.root()]),
    );
}

#[test]
fn dry_run_json_prints_the_plan_and_changes_nothing() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    commit_in(&worktree, "feature.txt");
    dirty(&worktree);
    let before = snapshot(repo.root());

    let plan = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "eta", "--dry-run", "--json"],
    ));
    let text = branchbox(&repo, &["feature", "teardown", "eta", "--dry-run"]);

    assert_eq!(snapshot(repo.root()), before, "a dry run changes no file");
    assert_eq!(plan["schema_version"], 1);
    assert_eq!(
        blocker_kinds(&plan),
        ["uncommitted_changes", "unmerged_branch"]
    );
    assert_eq!(plan["branch"]["ahead"], 1);
    assert_eq!(plan["branch"]["action"], "delete");
    assert_eq!(plan["branch"]["source"], "registry");
    assert_eq!(user_paths(&plan), ["README.md", "notes.txt"]);
    assert_fixture(
        AREA,
        "plan_dirty_unmerged",
        &normalize_json(plan, &[repo.root()]),
    );

    assert!(text.status.success(), "{}", stderr_of(&text));
    let stdout = stdout_of(&text);
    assert!(stdout.contains("dry run; nothing was changed"), "{stdout}");
    assert!(stdout.contains("✗ Teardown would refuse:"), "{stdout}");
    assert!(stdout.contains("1 commit not in main"), "{stdout}");
}

/// E4: in a repository without a `.gitignore`, every file `feature start --minimal` writes is
/// untracked, and none of them is user work.
#[test]
fn a_fresh_minimal_feature_tears_down_without_flags() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "fresh");

    let plan = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "fresh", "--dry-run", "--json"],
    ));
    assert_eq!(plan["blockers"], json!([]));
    assert_eq!(plan["changes"]["user"], json!([]));
    assert_fixture(AREA, "plan_fresh", &normalize_json(plan, &[repo.root()]));

    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "fresh", "--json"],
    ));
    assert_eq!(summary["worktree_removed"], true);
    assert_eq!(summary["branch_deleted"], true);
    assert_eq!(summary["discarded_changes"], json!([]));
    assert!(!worktree.exists());
    let spec = repo.path().join("docs/features/backlog/fresh.md");
    assert!(spec.exists(), "the spec is kept in the main worktree");
    assert!(fs::read_to_string(spec)
        .unwrap()
        .contains("status: backlog"));
}

#[test]
fn tracked_vscode_settings_count_as_user_work_only_once_edited() {
    let repo = init_test_repo();
    fs::create_dir(repo.path().join(".vscode")).unwrap();
    fs::write(
        repo.path().join(".vscode/settings.json"),
        "{\n  \"editor.tabSize\": 2\n}\n",
    )
    .unwrap();
    repo.git(&["add", ".vscode/settings.json"]);
    repo.git(&["commit", "-q", "-m", "Share editor settings"]);

    start_minimal(&repo, "vs-ok");
    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "vs-ok", "--json"],
    ));
    assert_eq!(summary["worktree_removed"], true);

    let worktree = start_minimal(&repo, "vs-edit");
    let settings_path = worktree.join(".vscode/settings.json");
    let mut settings: Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert!(
        settings.get("peacock.color").is_some(),
        "start adds its keys"
    );
    settings["editor.formatOnSave"] = json!(true);
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    let output = branchbox(&repo, &["feature", "teardown", "vs-edit", "--json"]);
    let envelope = assert_envelope(&output, "teardown_refused");
    assert_eq!(
        envelope["error"]["details"]["plan"]["changes"]["user"],
        json!([{"path": ".vscode/settings.json", "kind": "modified", "area": "vscode"}])
    );
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains(".vscode/settings.json (modified)"));
    assert!(settings_path.exists());
}

/// A full-mode feature syncs and rewrites `.devcontainer/devcontainer.json`; the sync baseline
/// marks that as generated. The manual harness then edits it, and text mode must keep the 0.13
/// banner on stdout before the scripted `--force` retry.
#[test]
fn full_mode_devcontainer_edits_keep_the_harness_phrase_on_stdout() {
    let repo = init_test_repo();
    fs::create_dir(repo.path().join(".devcontainer")).unwrap();
    fs::write(
        repo.path().join(".devcontainer/devcontainer.json"),
        "{\n  \"name\": \"test\",\n  \"image\": \"alpine:3.19\"\n}\n",
    )
    .unwrap();
    repo.git(&["add", ".devcontainer"]);
    repo.git(&["commit", "-q", "-m", "Add devcontainer"]);

    start(&repo, "dc-ok", &[]);
    let plan = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "dc-ok", "--dry-run", "--json"],
    ));
    assert!(
        plan["changes"]["generated"]
            .as_array()
            .unwrap()
            .contains(&json!({"path": ".devcontainer/devcontainer.json",
                              "rule": "devcontainer_baseline"})),
        "{plan:#}"
    );
    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "dc-ok", "--json"],
    ));
    assert_eq!(summary["worktree_removed"], true);
    assert!(!repo
        .path()
        .join(".branchbox/devcontainer-sync/dc-ok.json")
        .exists());

    let worktree = start(&repo, "dc-edit", &[]);
    let devcontainer = worktree.join(".devcontainer/devcontainer.json");
    let mut contents = fs::read_to_string(&devcontainer).unwrap();
    contents.push_str("  // dirty-teardown-marker\n");
    fs::write(&devcontainer, contents).unwrap();

    let refused = branchbox(&repo, &["feature", "teardown", "dc-edit", "--json"]);
    let envelope = assert_envelope(&refused, "teardown_refused");
    assert_eq!(
        envelope["error"]["details"]["plan"]["changes"]["user"],
        json!([{"path": ".devcontainer/devcontainer.json", "kind": "modified",
                "area": "devcontainer"}])
    );

    let text = branchbox(
        &repo,
        &[
            "feature",
            "teardown",
            "dc-edit",
            "--delete-branch",
            "--complete-spec",
        ],
    );
    assert_eq!(text.status.code(), Some(1));
    let stdout = stdout_of(&text);
    assert!(
        stdout.starts_with("⚠️  Detected devcontainer/compose changes inside "),
        "{stdout}"
    );
    assert!(stdout.contains("    • .devcontainer/devcontainer.json (modified)"));
    let stderr = stderr_of(&text);
    assert!(stderr.starts_with(
        "Error: Devcontainer/compose changes detected; rerun this command with --force to proceed."
    ));
    assert!(stderr.contains("Caused by:") && stderr.contains("--discard-changes"));
    assert!(devcontainer.exists());

    let forced = branchbox(
        &repo,
        &[
            "feature",
            "teardown",
            "dc-edit",
            "--delete-branch",
            "--complete-spec",
            "--force",
        ],
    );
    assert!(forced.status.success(), "{}", stderr_of(&forced));
    assert!(stdout_of(&forced).contains("Feature teardown finished"));
    assert!(!worktree.exists());
    assert!(repo
        .path()
        .join("docs/features/completed/dc-edit.md")
        .exists());
}

#[test]
fn an_unmerged_branch_refuses_before_removal_without_a_terminal() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "un");
    commit_in(&worktree, "work.txt");

    let output = branchbox(&repo, &["feature", "teardown", "un"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("Branch 'feature/un' has 1 commit not merged into main"),
        "{stderr}"
    );
    assert!(stderr.contains("--keep-branch") && stderr.contains("--force-delete-branch"));
    assert!(worktree.exists(), "refused before the worktree was removed");
    assert!(worktree.join("docs/features/in-progress/un.md").exists());
    assert_eq!(status_of(&repo, "un"), "active");
    assert!(branch_exists(&repo, "feature/un"));

    let envelope = assert_envelope(
        &branchbox(&repo, &["feature", "teardown", "un", "--json"]),
        "teardown_refused",
    );
    let plan = &envelope["error"]["details"]["plan"];
    assert_eq!(blocker_kinds(plan), ["unmerged_branch"]);
    assert_eq!(
        plan["blockers"][0]["override"],
        "--keep-branch | --force-delete-branch"
    );

    let kept = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "un", "--keep-branch", "--json"],
    ));
    assert_eq!(kept["branch_action"], "keep");
    assert_eq!(kept["branch_deleted"], false);
    assert!(!worktree.exists());
    assert!(
        branch_exists(&repo, "feature/un"),
        "the unmerged commit is kept"
    );
}

#[test]
fn force_delete_branch_deletes_an_unmerged_branch() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "un");
    commit_in(&worktree, "work.txt");

    let summary = assert_ok_json(&branchbox(
        &repo,
        &[
            "feature",
            "teardown",
            "un",
            "--force-delete-branch",
            "--json",
        ],
    ));
    assert_eq!(summary["branch_action"], "force_delete");
    assert_eq!(summary["branch_deleted"], true);
    assert!(
        summary["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|warning| !warning.as_str().unwrap().contains("Force-deleted")),
        "an explicit --force-delete-branch needs no warning"
    );
    assert!(!branch_exists(&repo, "feature/un"));
}

/// D-27: `--force` still implies `-D` when deleting, and says what that cost.
#[test]
fn force_still_force_deletes_and_names_the_commits() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "un");
    commit_in(&worktree, "one.txt");
    commit_in(&worktree, "two.txt");
    fs::write(worktree.join("scratch.txt"), "scratch\n").unwrap();

    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "un", "--force", "--json"],
    ));
    assert_eq!(summary["branch_action"], "force_delete");
    assert_eq!(summary["branch_deleted"], true);
    assert!(summary["warnings"].as_array().unwrap().contains(&json!(
        "Force-deleted unmerged branch feature/un (2 commits); use --discard-changes to discard \
         files without deleting unmerged commits"
    )));
    assert_eq!(summary["discarded_changes"][0]["path"], "scratch.txt");
}

#[test]
fn force_delete_unmerged_by_default_applies_without_a_terminal() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "un");
    commit_in(&worktree, "work.txt");
    fs::write(
        repo.path().join(".branchbox/config.json"),
        r#"{"feature": {"teardown": {"force_delete_unmerged_by_default": true}}}"#,
    )
    .unwrap();

    let plan = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "un", "--dry-run", "--json"],
    ));
    assert_eq!(plan["branch"]["action"], "force_delete");
    assert_eq!(plan["defaults"]["force_delete_unmerged_by_default"], true);
    assert_eq!(plan["blockers"], json!([]));

    let summary = assert_ok_json(&branchbox(&repo, &["feature", "teardown", "un", "--json"]));
    assert_eq!(summary["branch_action"], "force_delete");
    assert!(!branch_exists(&repo, "feature/un"));
}

/// DRIFT-07: the branch comes from the registry, so a custom prefix needs no repeating.
#[test]
fn a_custom_prefix_branch_is_deleted_without_repeating_the_prefix() {
    let repo = init_test_repo();
    start(&repo, "sp", &["--minimal", "--branch-prefix", "spike"]);
    assert!(branch_exists(&repo, "spike/sp"));

    let summary = assert_ok_json(&branchbox(&repo, &["feature", "teardown", "sp", "--json"]));
    assert_eq!(summary["branch_name"], "spike/sp");
    assert_eq!(summary["branch_deleted"], true);
    assert!(!branch_exists(&repo, "spike/sp"));
}

#[test]
fn a_locked_worktree_is_refused_then_removed_with_force() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "lk");
    repo.git(&["worktree", "lock", "--reason", "on a usb disk", "../lk"]);

    let envelope = assert_envelope(
        &branchbox(&repo, &["feature", "teardown", "lk", "--json"]),
        "teardown_refused",
    );
    let plan = &envelope["error"]["details"]["plan"];
    assert_eq!(blocker_kinds(plan), ["worktree_locked"]);
    assert_eq!(plan["blockers"][0]["reason"], "on a usb disk");
    assert_eq!(plan["worktree"]["locked"], true);
    assert!(worktree.exists());

    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "lk", "--force", "--json"],
    ));
    assert_eq!(summary["worktree_removed"], true);
    assert_eq!(summary["registry_updated"], true);
    assert!(!worktree.exists());
}

#[test]
fn a_corrupted_git_file_reports_the_git_error_as_status_unavailable() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "broken");
    fs::write(
        worktree.join(".git"),
        "gitdir: /nonexistent/worktrees/broken\n",
    )
    .unwrap();

    let envelope = assert_envelope(
        &branchbox(&repo, &["feature", "teardown", "broken", "--json"]),
        "teardown_refused",
    );
    let plan = &envelope["error"]["details"]["plan"];
    assert_eq!(blocker_kinds(plan), ["status_unavailable"]);
    assert_eq!(plan["changes"]["status_available"], false);
    let cause = plan["blockers"][0]["cause"].as_str().unwrap();
    assert!(
        cause.contains("fatal: not a git repository: /nonexistent/worktrees/broken"),
        "{cause}"
    );
    assert_eq!(plan["blockers"][0]["override"], "--force");
    assert!(worktree.exists());

    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "broken", "--force", "--json"],
    ));
    assert_eq!(summary["worktree_removed"], true);
    assert!(!worktree.exists());
}

#[test]
fn a_failed_branch_delete_after_removal_is_a_partial_success() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    // The branch is also checked out elsewhere, so `git branch -d` refuses it.
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "--force",
        "../elsewhere",
        "feature/eta",
    ]);

    let summary = assert_ok_json(&branchbox(&repo, &["feature", "teardown", "eta", "--json"]));
    assert_eq!(summary["worktree_removed"], true);
    assert_eq!(summary["branch_action"], "delete");
    assert_eq!(summary["branch_deleted"], false);
    let error = summary["branch_delete_error"].as_str().unwrap();
    assert!(error.contains("used by worktree"), "{error}");
    assert!(!worktree.exists());
    assert_eq!(status_of(&repo, "eta"), "removed");
}

#[test]
fn a_missing_worktree_is_planned_but_needs_force() {
    let repo = init_test_repo();
    let plan = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "nope", "--dry-run", "--json"],
    ));
    assert_eq!(plan["registered"], false);
    assert_eq!(plan["worktree"]["exists"], false);
    assert_eq!(plan["branch"]["source"], "config_prefix");

    let worktree = start_minimal(&repo, "gone");
    fs::remove_dir_all(&worktree).unwrap();
    assert_envelope(
        &branchbox(&repo, &["feature", "teardown", "gone", "--json"]),
        "worktree_not_found",
    );
    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "gone", "--force", "--json"],
    ));
    assert_eq!(summary["registry_updated"], true);
    assert_eq!(status_of(&repo, "gone"), "removed");
}

/// A directory at a feature's worktree path that is not a linked worktree of the repository
/// (the main worktree itself, an unrelated repository, a plain folder) is never torn down, with
/// or without `--force`, and nothing in it changes.
#[test]
fn teardown_never_touches_a_directory_that_is_not_a_linked_worktree() {
    let repo = init_test_repo();
    let main_tmp = repo.path().join("tmp");
    fs::create_dir_all(&main_tmp).unwrap();
    fs::write(main_tmp.join("important.txt"), "keep me\n").unwrap();
    let other = repo.root().join("other");
    fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    fs::write(other.join("work.txt"), "unrelated work\n").unwrap();
    let plain = repo.root().join("plain");
    fs::create_dir_all(plain.join("tmp")).unwrap();
    fs::write(plain.join("tmp/notes.txt"), "a plain folder\n").unwrap();

    let before = [snapshot(&main_tmp), snapshot(&other), snapshot(&plain)];
    for name in ["main", "other", "plain"] {
        for extra in [&[][..], &["--force"], &["--discard-changes"]] {
            let mut args = vec!["feature", "teardown", name, "--keep-branch", "--json"];
            args.extend_from_slice(extra);
            let envelope = assert_envelope(&branchbox(&repo, &args), "teardown_refused");
            let details = &envelope["error"]["details"];
            assert_eq!(details["changed_anything"], false, "{name} {extra:?}");
            let plan = &details["plan"];
            assert_eq!(blocker_kinds(plan)[0], "not_a_worktree", "{name} {extra:?}");
            assert!(plan["blockers"][0].get("override").is_none(), "{plan:#}");
            let message = envelope["error"]["message"].as_str().unwrap();
            assert!(message.contains("never removes it"), "{message}");
        }
    }
    let plan = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "main", "--dry-run", "--json"],
    ));
    assert_eq!(blocker_kinds(&plan), ["not_a_worktree"]);
    assert!(plan["blockers"][0]["cause"]
        .as_str()
        .unwrap()
        .contains("main worktree"));
    assert_eq!(
        [snapshot(&main_tmp), snapshot(&other), snapshot(&plain)],
        before
    );
    assert!(repo.path().join(".git").is_dir());
    assert!(other.join(".git").is_dir());
}

/// The plan promises to keep the feature spec. When moving it to the main worktree fails,
/// teardown stops before removing the worktree, even with `--discard-changes`; only `--force`
/// removes it anyway.
#[cfg(unix)]
#[test]
fn a_spec_that_cannot_be_moved_to_main_stops_teardown_even_when_discarding() {
    use std::os::unix::fs::PermissionsExt;

    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    let spec = worktree.join("docs/features/in-progress/eta.md");
    fs::write(&spec, "# eta\n\nmy spec edits\n").unwrap();
    let backlog = repo.path().join("docs/features/backlog");
    fs::create_dir_all(&backlog).unwrap();
    fs::set_permissions(&backlog, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(backlog.join(".probe"), "").is_ok() {
        // Running as root: permissions do not stop the move.
        fs::set_permissions(&backlog, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    for extra in [&[][..], &["--discard-changes"]] {
        let mut args = vec!["feature", "teardown", "eta", "--keep-branch", "--json"];
        args.extend_from_slice(extra);
        let envelope = assert_envelope(&branchbox(&repo, &args), "teardown_refused");
        let details = &envelope["error"]["details"];
        assert_eq!(details["changed_anything"], true, "{extra:?}");
        assert_eq!(blocker_kinds(&details["plan"]), ["spec_not_preserved"]);
        assert_eq!(
            details["plan"]["blockers"][0]["path"],
            "docs/features/in-progress/eta.md"
        );
        assert_eq!(details["plan"]["blockers"][0]["override"], "--force");
        assert_eq!(
            fs::read_to_string(&spec).unwrap(),
            "# eta\n\nmy spec edits\n"
        );
        assert_eq!(status_of(&repo, "eta"), "active");
    }

    fs::set_permissions(&backlog, fs::Permissions::from_mode(0o755)).unwrap();
    let summary = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "eta", "--keep-branch", "--json"],
    ));
    assert_eq!(summary["worktree_removed"], true);
    assert!(!worktree.exists());
    assert!(fs::read_to_string(backlog.join("eta.md"))
        .unwrap()
        .contains("my spec edits"));
}

/// Teardown moves one spec (the first of in-progress, backlog and completed). A second copy
/// would be lost with the worktree, so it is a user change, not "preserved".
#[test]
fn a_second_copy_of_the_spec_is_a_user_change() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    let completed = worktree.join("docs/features/completed");
    fs::create_dir_all(&completed).unwrap();
    fs::write(completed.join("eta.md"), "# my completed notes\n").unwrap();

    let plan = assert_ok_json(&branchbox(
        &repo,
        &["feature", "teardown", "eta", "--dry-run", "--json"],
    ));
    let preserved: Vec<&str> = plan["changes"]["preserved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect();
    assert_eq!(preserved, ["docs/features/in-progress/eta.md"]);
    assert_eq!(user_paths(&plan), ["docs/features/completed/eta.md"]);
    assert_eq!(blocker_kinds(&plan), ["uncommitted_changes"]);
}

#[test]
fn text_dry_run_describes_each_part_of_the_plan() {
    let repo = init_test_repo();
    start_minimal(&repo, "eta");

    let clean = branchbox(&repo, &["feature", "teardown", "eta", "--dry-run"]);
    assert!(clean.status.success(), "{}", stderr_of(&clean));
    let stdout = stdout_of(&clean);
    for expected in [
        "Uncommitted changes: none",
        "BranchBox-generated files (discarded): 5",
        "Kept: docs/features/in-progress/eta.md → docs/features/backlog/eta.md",
        "Branch: feature/eta (from the registry; merged into main) → delete (git branch -d)",
        "✓ Teardown would proceed.",
    ] {
        assert!(
            stdout.contains(expected),
            "{expected} missing from:\n{stdout}"
        );
    }

    repo.git(&["worktree", "lock", "--reason", "busy", "../eta"]);
    let locked = branchbox(
        &repo,
        &[
            "feature",
            "teardown",
            "eta",
            "--dry-run",
            "--branch-prefix",
            "other",
            "--force-delete-branch",
        ],
    );
    let stdout = stdout_of(&locked);
    for expected in [
        "(exists), locked: busy",
        "Branch: other/eta (from --branch-prefix; does not exist) → force-delete (git branch -D)",
        "  Warnings:\n    - Branch 'other/eta' does not exist",
        "✗ Teardown would refuse:",
    ] {
        assert!(
            stdout.contains(expected),
            "{expected} missing from:\n{stdout}"
        );
    }

    let worktree = repo.root().join("eta");
    fs::write(worktree.join("notes.txt"), "notes\n").unwrap();
    let discarded = branchbox(
        &repo,
        &[
            "feature",
            "teardown",
            "eta",
            "--dry-run",
            "--force",
            "--keep-branch",
        ],
    );
    let stdout = stdout_of(&discarded);
    assert!(
        stdout.contains("Uncommitted changes (discarded): 1\n    • notes.txt (untracked)"),
        "{stdout}"
    );
    assert!(stdout.contains("→ keep"), "{stdout}");

    let missing = branchbox(&repo, &["feature", "teardown", "nope", "--dry-run"]);
    let stdout = stdout_of(&missing);
    assert!(stdout.contains("(missing)"), "{stdout}");
    assert!(stdout.contains("from the configured prefix"), "{stdout}");

    fs::write(worktree.join(".git"), "gitdir: /nonexistent\n").unwrap();
    let unreadable = branchbox(&repo, &["feature", "teardown", "eta", "--dry-run"]);
    assert!(stdout_of(&unreadable).contains("Uncommitted changes: unknown (git status failed)"));
}

#[test]
fn text_mode_lists_long_change_sets_and_reports_discards() {
    let repo = init_test_repo();
    let worktree = start_minimal(&repo, "eta");
    for index in 0..12 {
        fs::write(worktree.join(format!("note-{index:02}.txt")), "x\n").unwrap();
    }
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "--force",
        "../elsewhere",
        "feature/eta",
    ]);

    let refused = branchbox(&repo, &["feature", "teardown", "eta"]);
    assert_eq!(refused.status.code(), Some(1));
    let stdout = stdout_of(&refused);
    assert!(
        stdout.contains("    • note-09.txt (untracked)\n    • … and more\n"),
        "{stdout}"
    );
    assert!(!stdout.contains("note-10.txt"), "{stdout}");
    assert!(stderr_of(&refused).contains("and 2 more"));

    let output = branchbox(&repo, &["feature", "teardown", "eta", "--discard-changes"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = stdout_of(&output);
    for expected in [
        "Feature teardown finished",
        "  Branch delete failed: ",
        "  Discarded changes: 12 (note-00.txt, ",
        ", note-09.txt, …)",
        "  Kept: docs/features/in-progress/eta.md → docs/features/backlog/eta.md in the main worktree",
    ] {
        assert!(stdout.contains(expected), "{expected} missing from:\n{stdout}");
    }
    assert!(!worktree.exists());
}

#[test]
fn version_lists_the_teardown_capabilities() {
    let repo = init_test_repo();
    let payload = assert_ok_json(&branchbox(&repo, &["version", "--json"]));
    let capabilities = payload["capabilities"].as_array().unwrap();
    for expected in [
        "teardown-plan",
        "teardown-discard-changes",
        "teardown-unmerged-preflight",
        "prune-json",
    ] {
        assert!(
            capabilities.contains(&json!(expected)),
            "{expected} missing from {capabilities:?}"
        );
    }
}
