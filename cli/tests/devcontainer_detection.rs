#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
extern crate assert_cmd;

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn detect(config: &str) -> serde_json::Value {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("sample");
    let devcontainer = workspace.join(".devcontainer");
    fs::create_dir_all(&devcontainer).unwrap();
    fs::write(devcontainer.join("devcontainer.json"), config).unwrap();
    // These files belong to an older scaffold, not the active image/Dockerfile configuration.
    fs::write(
        devcontainer.join("Dockerfile"),
        "FROM alpine\nUSER vscode\n",
    )
    .unwrap();
    fs::write(
        devcontainer.join("compose.yaml"),
        "services:\n  unused:\n    build: .\n    ports: [\"3000:3000\"]\n",
    )
    .unwrap();
    let output = Command::new(cargo_bin!("branchbox"))
        .args(["devcontainer", "detect", "--json", "--path"])
        .arg(&workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn image_detection_uses_configured_user_and_workspace_not_unused_scaffolds() {
    let result = detect(
        r#"{
        // JSONC is supported by runtime operations and detection alike.
        "image": "python:3.12-alpine",
        "remoteUser": "root",
        "containerUser": "other",
        "workspaceFolder": "/srv/${localWorkspaceFolderBasename}"
    }"#,
    );
    assert_eq!(result["container_type"], "image");
    assert_eq!(result["configured_user"], "root");
    assert_eq!(result["container_user"], "root");
    assert_eq!(result["workspace_folder"], "/srv/sample");
    assert!(result["service_name"].is_null());
    assert_eq!(result["service_url"], "");
    assert_eq!(result["port"], 0);
}

#[test]
fn dockerfile_detection_uses_container_user_and_suppresses_unused_compose() {
    let result = detect(r#"{"build":{"dockerfile":"Dockerfile"},"containerUser":"1234"}"#);
    assert_eq!(result["container_type"], "dockerfile");
    assert_eq!(result["configured_user"], "1234");
    assert_eq!(result["container_user"], "1234");
    assert_eq!(result["workspace_folder"], "/workspaces/sample");
    assert!(result["service_name"].is_null());
}

#[test]
fn image_detection_distinguishes_configured_user_from_legacy_estimate() {
    let result = detect(r#"{"image":"alpine"}"#);
    assert!(result["configured_user"].is_null());
    assert_eq!(result["container_type"], "image");
    assert_eq!(result["workspace_folder"], "/workspaces/sample");
}

#[test]
fn unrelated_nullable_environment_fields_do_not_break_detection() {
    let result = detect(r#"{"image":"alpine","remoteUser":"root","remoteEnv":{"REMOVE_ME":null}}"#);
    assert_eq!(result["container_type"], "image");
    assert_eq!(result["configured_user"], "root");
}

#[test]
fn root_configuration_is_detected_without_a_devcontainer_directory() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join(".devcontainer.json"),
        r#"{"image":"alpine","remoteUser":"root","workspaceFolder":"/srv/root-config"}"#,
    )
    .unwrap();
    let output = Command::new(cargo_bin!("branchbox"))
        .args(["devcontainer", "detect", "--json", "--path"])
        .arg(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["configured_user"], "root");
    assert_eq!(result["workspace_folder"], "/srv/root-config");
    assert!(result["service_name"].is_null());
}

#[test]
fn active_compose_detection_keeps_service_facts() {
    let result =
        detect(r#"{"dockerComposeFile":"compose.yaml","service":"unused","remoteUser":"root"}"#);
    assert_eq!(result["container_type"], "compose");
    assert_eq!(result["service_name"], "unused");
    assert_eq!(result["port"], 3000);
    assert_eq!(result["service_url"], "http://unused:3000");
    assert_eq!(result["configured_user"], "root");
}
