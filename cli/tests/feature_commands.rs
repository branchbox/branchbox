use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
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
        .stdout(predicate::str::contains("Feature workspace ready"));

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
        .stdout(predicate::str::contains(work_feature));

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
