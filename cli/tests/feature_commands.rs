use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
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

fn init_test_repo() -> TempDir {
    let temp_dir = TempDir::new().expect("create temp dir");
    let repo_path = temp_dir.path();

    run_git(repo_path, &["init", "-b", "main"]);
    run_git(repo_path, &["config", "user.email", "test@example.com"]);
    run_git(repo_path, &["config", "user.name", "Test User"]);

    fs::write(repo_path.join("README.md"), "# Test Repo\n").expect("write README");
    run_git(repo_path, &["add", "README.md"]);
    run_git(repo_path, &["commit", "-m", "Initial commit"]);

    fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").expect("write .env");

    temp_dir
}

#[test]
fn feature_start_list_teardown_end_to_end() {
    let temp = init_test_repo();
    let repo_path = temp.path();
    let work_feature = "integration-test-feature";
    let worktree_path = repo_path
        .parent()
        .expect("repo has parent")
        .join(work_feature);

    // Start feature
    Command::cargo_bin("branchbox")
        .expect("binary exists")
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args(["feature", "start", "--name", work_feature])
        .assert()
        .success()
        .stdout(predicate::str::contains("Feature workspace ready"));

    assert!(
        worktree_path.exists(),
        "expected worktree directory to be created"
    );

    // List features
    Command::cargo_bin("branchbox")
        .expect("binary exists")
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args(["feature", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(work_feature));

    // Teardown feature
    Command::cargo_bin("branchbox")
        .expect("binary exists")
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args([
            "feature",
            "teardown",
            "--name",
            work_feature,
            "--delete-branch",
            "--force",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Feature teardown finished"));

    assert!(
        !worktree_path.exists(),
        "expected worktree directory to be removed after teardown"
    );

    // Listing after teardown still reports historical entry as removed
    Command::cargo_bin("branchbox")
        .expect("binary exists")
        .current_dir(repo_path)
        .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
        .args(["feature", "list", "--status", "removed"])
        .assert()
        .success()
        .stdout(predicate::str::contains(work_feature));
}
