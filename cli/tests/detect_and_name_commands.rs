use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn detect_command_shows_stack_info() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();

    // Create a Rails project marker
    fs::write(project_path.join("Gemfile"), "gem 'rails'\n").unwrap();
    fs::write(project_path.join("config.ru"), "# Rails app\n").unwrap();

    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("detect")
        .arg("--path")
        .arg(project_path.to_str().unwrap());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("BranchBox Configuration"))
        .stdout(predicate::str::contains("Stack:"))
        .stdout(predicate::str::contains("Adapter:"))
        .stdout(predicate::str::contains("Enabled modules:"));
}

#[test]
fn detect_command_shows_rails_adapter() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();

    fs::write(project_path.join("Gemfile"), "gem 'rails'\n").unwrap();

    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("detect")
        .arg("--path")
        .arg(project_path.to_str().unwrap());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Adapter: Rails"));
}

#[test]
fn detect_command_shows_nodejs_adapter() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();

    fs::write(project_path.join("package.json"), "{}").unwrap();

    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("detect")
        .arg("--path")
        .arg(project_path.to_str().unwrap());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Adapter: Node.js"));
}

#[test]
fn detect_command_shows_generic_adapter() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();

    // No stack-specific files

    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("detect")
        .arg("--path")
        .arg(project_path.to_str().unwrap());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Adapter: Generic"));
}

#[test]
fn detect_command_shows_compose_module() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join(".devcontainer")).unwrap();
    fs::write(
        project_path.join(".devcontainer/compose.yaml"),
        "version: '3'\n",
    )
    .unwrap();

    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("detect")
        .arg("--path")
        .arg(project_path.to_str().unwrap());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("compose"));
}

#[test]
fn detect_command_shows_database_module() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("config")).unwrap();
    fs::write(project_path.join("config/database.yml"), "# Rails DB\n").unwrap();

    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("detect")
        .arg("--path")
        .arg(project_path.to_str().unwrap());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("database"));
}

#[test]
fn detect_command_defaults_to_current_directory() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("detect");

    // Should not fail, will detect from current directory
    cmd.assert().success();
}

#[test]
fn name_generate_converts_title_to_name() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("generate").arg("OAuth Integration");

    cmd.assert().success().stdout("oauth\n");
}

#[test]
fn name_generate_handles_special_characters() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("generate").arg("Fix Bug #123");

    cmd.assert().success().stdout("fix-bug-123\n");
}

#[test]
fn name_generate_handles_multiple_words() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("generate").arg("Add New API Endpoint");

    cmd.assert().success().stdout("add-new-api\n");
}

#[test]
fn name_validate_accepts_valid_name() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("validate").arg("oauth-integration");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("✓"))
        .stdout(predicate::str::contains("Valid feature name"));
}

#[test]
fn name_validate_rejects_invalid_name_uppercase() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("validate").arg("OAuth-Integration");

    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("✗"))
        .stderr(predicate::str::contains("lowercase"));
}

#[test]
fn name_validate_rejects_invalid_name_spaces() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("validate").arg("oauth integration");

    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("✗"));
}

#[test]
fn name_validate_rejects_invalid_name_special_chars() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("validate").arg("oauth_integration");

    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("✗"));
}

#[test]
fn name_validate_rejects_empty_name() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("validate").arg("");

    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("✗"));
}

#[test]
fn name_validate_accepts_single_word() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("validate").arg("oauth");

    cmd.assert().success().stdout(predicate::str::contains("✓"));
}

#[test]
fn name_validate_accepts_numbers() {
    let mut cmd = Command::new(cargo_bin!("branchbox"));
    cmd.arg("name").arg("validate").arg("fix-bug-123");

    cmd.assert().success().stdout(predicate::str::contains("✓"));
}
