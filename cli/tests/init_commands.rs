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
}

fn create_test_repo() -> TestRepo {
    let temp_dir = TempDir::new().expect("create temp dir");
    let repo_path = temp_dir.path().join("test-repo");
    fs::create_dir(&repo_path).expect("create repo dir");

    run_git(&repo_path, &["init", "-b", "main"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Test User"]);

    fs::write(repo_path.join("README.md"), "# Test Repo\n").expect("write README");
    run_git(&repo_path, &["add", "README.md"]);
    run_git(&repo_path, &["commit", "-m", "Initial commit"]);

    TestRepo {
        _temp_dir: temp_dir,
        repo_path,
    }
}

#[test]
fn init_basic_repository() {
    let test_repo = create_test_repo();
    let repo_path = test_repo.path();

    // Run init command
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized BranchBox"));

    // Verify registry was created
    let registry_path = repo_path.join(".branchbox/registry.json");
    assert!(
        registry_path.exists(),
        "expected .branchbox/registry.json to be created"
    );

    // Verify registry content
    let registry_content = fs::read_to_string(&registry_path).expect("read registry");
    assert!(
        registry_content.contains("\"version\""),
        "registry should have version field"
    );
    assert!(
        registry_content.contains("\"features\""),
        "registry should have features field"
    );

    // Verify .gitignore was updated
    let gitignore_path = repo_path.join(".gitignore");
    let gitignore_content = fs::read_to_string(&gitignore_path).expect("read .gitignore");
    assert!(
        gitignore_content.contains(".branchbox/registry.json"),
        ".gitignore should exclude registry.json"
    );

    // Verify devcontainer was created
    let devcontainer_path = repo_path.join(".devcontainer/devcontainer.json");
    assert!(
        devcontainer_path.exists(),
        "expected .devcontainer/devcontainer.json to be created"
    );
}

#[test]
fn init_skip_devcontainer() {
    let test_repo = create_test_repo();
    let repo_path = test_repo.path();

    // Run init command with --skip-devcontainer
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y", "--skip-devcontainer"])
        .assert()
        .success();

    // Verify registry was created
    assert!(repo_path.join(".branchbox/registry.json").exists());

    // Verify devcontainer was NOT created
    let devcontainer_path = repo_path.join(".devcontainer/devcontainer.json");
    assert!(
        !devcontainer_path.exists(),
        "devcontainer should not be created when --skip-devcontainer is used"
    );
}

#[test]
fn init_idempotent() {
    let test_repo = create_test_repo();
    let repo_path = test_repo.path();

    // First init
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y"])
        .assert()
        .success();

    // Second init should succeed and report already initialized
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already initialized"));
}

#[test]
fn init_validate_mode() {
    let test_repo = create_test_repo();
    let repo_path = test_repo.path();

    // Initialize first
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y"])
        .assert()
        .success();

    // Run in validate mode
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "--validate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Validation Mode"));
}

#[test]
fn init_dry_run() {
    let test_repo = create_test_repo();
    let repo_path = test_repo.path();

    // Run in dry-run mode
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DRY RUN"));

    // Verify nothing was actually created
    assert!(
        !repo_path.join(".branchbox").exists(),
        "dry-run should not create .branchbox directory"
    );
}

#[test]
fn init_not_git_repo() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let not_repo = temp_dir.path().join("not-a-repo");
    fs::create_dir(&not_repo).expect("create dir");

    // Should fail on non-git directory
    Command::new(cargo_bin!("branchbox"))
        .current_dir(&not_repo)
        .args(["init", "-y"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not a git repository"));
}

#[test]
fn init_with_rails_stack() {
    let test_repo = create_test_repo();
    let repo_path = test_repo.path();

    // Create Gemfile to make it look like Rails
    fs::write(repo_path.join("Gemfile"), "gem 'rails'\n").expect("write Gemfile");

    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y"])
        .assert()
        .success();

    // Verify devcontainer was created
    let devcontainer_path = repo_path.join(".devcontainer/devcontainer.json");
    assert!(devcontainer_path.exists());

    // Check that it detected Rails
    let devcontainer_content = fs::read_to_string(&devcontainer_path).expect("read devcontainer");
    assert!(
        devcontainer_content.contains("ruby") || devcontainer_content.contains("rails"),
        "devcontainer should reference Ruby/Rails"
    );
}

#[test]
fn init_verbose_mode() {
    let test_repo = create_test_repo();
    let repo_path = test_repo.path();

    // Run with verbose flag
    Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .args(["init", "-y", "-v"])
        .assert()
        .success();

    // Just verify it doesn't crash with verbose mode
    // The actual verbose output is validated manually
}
