#[macro_use]
extern crate assert_cmd;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::TempDir;

fn run_git(repo: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap_or_else(|err| panic!("failed to execute git {:?}: {}", args, err));
    assert!(
        status.success(),
        "git {:?} failed with status {}",
        args,
        status
    );
}

struct TestRepo {
    _temp_dir: TempDir,
    repo_path: PathBuf,
}

impl TestRepo {
    fn path(&self) -> &Path {
        &self.repo_path
    }

    fn worktree_parent(&self) -> &Path {
        self.repo_path
            .parent()
            .expect("temporary repo has parent directory")
    }
}

fn init_test_repo() -> TestRepo {
    let temp_dir = TempDir::new().expect("create temp dir");
    let repo_path = temp_dir.path().join("repo");
    fs::create_dir(&repo_path).expect("create repo dir");

    run_git(&repo_path, &["init", "-b", "main"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Test User"]);

    fs::write(repo_path.join("README.md"), "# Test Repo\n").expect("write README");
    run_git(&repo_path, &["add", "README.md"]);
    run_git(&repo_path, &["commit", "-m", "Initial commit"]);

    fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").expect("write .env");

    TestRepo {
        _temp_dir: temp_dir,
        repo_path,
    }
}

#[test]
fn feature_start_list_teardown_end_to_end() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "integration-test-feature";
    let worktree_path = test_repo.worktree_parent().join(work_feature);

    // Start feature
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args(["feature", "start", work_feature])
        .assert()
        .success()
        .stdout(predicate::str::contains("Feature workspace ready (full)"));

    assert!(
        worktree_path.exists(),
        "expected worktree directory to be created"
    );

    // Spec stub should be created under docs/features/in-progress
    let spec_path = repo_path
        .join("docs")
        .join("features")
        .join("in-progress")
        .join(format!("{work_feature}.md"));
    assert!(
        spec_path.exists(),
        "expected spec stub {:?} to be created",
        spec_path
    );
    let spec_content = fs::read_to_string(&spec_path).expect("read spec stub");
    assert!(
        spec_content.contains("status: in-progress"),
        "spec stub should include status field"
    );
    assert!(
        spec_content.contains("## Overview"),
        "spec stub should include scaffolded sections"
    );

    // List features
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args(["feature", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(work_feature)
                .and(predicate::str::contains("Mode"))
                .and(predicate::str::contains("Modules")),
        );

    // Teardown feature
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args([
            "feature",
            "teardown",
            work_feature,
            "--delete-branch",
            "--force",
            "--complete-spec",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Feature teardown finished"));

    assert!(
        !worktree_path.exists(),
        "expected worktree directory to be removed after teardown"
    );

    // Listing after teardown still reports historical entry as removed
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args(["feature", "list", "--status", "removed"])
        .assert()
        .success()
        .stdout(predicate::str::contains(work_feature));

    let completed_spec = repo_path
        .join("docs")
        .join("features")
        .join("completed")
        .join(format!("{work_feature}.md"));
    assert!(
        completed_spec.exists(),
        "expected spec to move to completed"
    );
}

#[test]
fn feature_start_minimal_mode_json_summary() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "minimal-mode-feature";
    let worktree_path = test_repo.worktree_parent().join(work_feature);

    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .env("BRANCHBOX_ENABLE_FAST_MODE", "1")
        .env("RUST_LOG", "off")
        .args([
            "feature",
            "start",
            work_feature,
            "--minimal",
            "--json",
            "--prompt",
            "Quick seed",
        ])
        .output()
        .expect("run feature start --minimal");

    assert!(
        output.status.success(),
        "expected feature start to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: Value =
        serde_json::from_slice(&output.stdout).expect("parse feature start JSON summary");

    assert_eq!(summary["mode"], Value::String("minimal".into()));
    assert_eq!(summary["work_feature"], Value::String(work_feature.into()));
    assert_eq!(summary["prompt_seed"], Value::String("Quick seed".into()));

    let skipped_modules = summary["skipped_modules"]
        .as_array()
        .expect("skipped modules array");
    assert!(skipped_modules
        .iter()
        .any(|entry| entry["module"] == Value::String("devcontainer".into())));

    assert!(summary["prompt_bridge_enabled"].is_boolean());

    assert!(worktree_path.exists());

    let list_output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args(["feature", "list", "--json"])
        .output()
        .expect("feature list --json");

    assert!(
        list_output.status.success(),
        "expected feature list --json to succeed: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );

    let list: Value =
        serde_json::from_slice(&list_output.stdout).expect("parse feature list JSON output");
    let features = list
        .as_array()
        .expect("JSON feature list should be an array");
    assert!(
        !features.is_empty(),
        "expected at least one feature in JSON list"
    );

    let first = &features[0];
    assert_eq!(first["start_mode"], Value::String("minimal".into()));
    assert_eq!(first["prompt_seed"], Value::String("Quick seed".into()));
    let module_outcomes = first["module_outcomes"]
        .as_array()
        .expect("module_outcomes should be an array");
    assert!(
        !module_outcomes.is_empty(),
        "module_outcomes should include entries from feature start"
    );

    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .env("RUST_LOG", "off")
        .args([
            "feature",
            "teardown",
            work_feature,
            "--delete-branch",
            "--force",
        ])
        .assert()
        .success();

    assert!(
        !worktree_path.exists(),
        "expected worktree directory removed during teardown"
    );
}
