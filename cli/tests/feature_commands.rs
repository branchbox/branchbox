#[macro_use]
extern crate assert_cmd;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

macro_rules! branchbox_cmd {
    ($repo:expr $(, $key:expr => $value:expr )* $(,)?) => {{
        let mut cmd = Command::new(cargo_bin!("branchbox"));
        cmd.current_dir($repo)
            .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
            .env("RUST_LOG", "off");
        $(
            cmd.env($key, $value);
        )*
        cmd
    }};
}

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

    fn ensure_devcontainer_dir(&self) -> PathBuf {
        let devcontainer = self.repo_path.join(".devcontainer");
        std::fs::create_dir_all(&devcontainer).expect("create devcontainer dir");
        devcontainer
    }

    fn with_valid_devcontainer(&self) {
        let devcontainer = self.ensure_devcontainer_dir();
        std::fs::write(
            devcontainer.join("devcontainer.json"),
            r#"{
  "name": "test",
  "image": "mcr.microsoft.com/vscode/devcontainers/base:ubuntu",
  "forwardPorts": [3000]
}
"#,
        )
        .expect("write devcontainer.json");
        std::fs::write(
            devcontainer.join("docker-compose.yml"),
            r#"version: "3"
services:
  dev:
    image: alpine:3.19
"#,
        )
        .expect("write docker-compose.yml");
    }

    fn with_invalid_devcontainer(&self) {
        let devcontainer = self.ensure_devcontainer_dir();
        std::fs::write(devcontainer.join("devcontainer.json"), "{ invalid json")
            .expect("write invalid devcontainer");
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

#[cfg(unix)]
fn create_fake_sbx() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = TempDir::new().expect("create fake sbx temp dir");
    let binary = temp.path().join("sbx");
    let state = temp.path().join("state");
    let log = temp.path().join("calls.log");
    fs::write(
        &binary,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_SBX_LOG"
case "$1" in
  ls)
    if [ -f "$FAKE_SBX_STATE" ]; then cat "$FAKE_SBX_STATE"; fi
    ;;
  create)
    previous=""
    for argument in "$@"; do
      if [ "$previous" = "--name" ]; then
        printf '%s\n' "$argument" > "$FAKE_SBX_STATE"
        break
      fi
      previous="$argument"
    done
    ;;
  ports)
    case "$*" in
      *"--json"*) printf '%s\n' '[]' ;;
    esac
    ;;
  exec)
    case "$*" in
      *"devcontainer up"*)
        if [ "${FAKE_SBX_REQUIRE_RUN_SERVICES:-}" = "1" ]; then
          override="$3/.devcontainer/.devcontainer.json"
          if [ ! -f "$override" ] || ! grep -q '"runServices"' "$override" || ! grep -q '"app"' "$override" || grep -q '"tailscale"' "$override"; then
            printf '%s\n' 'SBX runServices override was not prepared correctly' >&2
            exit 45
          fi
        fi
        if [ "${FAKE_SBX_REQUIRE_CLOUDFLARED_ENV:-}" = "1" ]; then
          env_file="$3/.devcontainer/.cloudflared.env"
          if [ ! -f "$env_file" ] || ! grep -q '^TUNNEL_TOKEN=branchbox-tunnel-order-sentinel$' "$env_file"; then
            printf '%s\n' 'cloudflared environment was not prepared before devcontainer up' >&2
            exit 43
          fi
        fi
        if [ "${FAKE_SBX_START_FAILURE:-}" = "1" ]; then
          cat >&2 <<'EOF'
services:
  app:
    environment:
      ARBITRARY_CREDENTIAL: branchbox-cli-sentinel-secret-83d1
[2026-08-20T00:00:00Z] Error: docker compose up failed because branchbox-cli-sentinel-secret-83d1 was rejected
EOF
          exit 42
        fi
        container_id="${FAKE_SBX_RECONCILE_CONTAINER_ID:-fake-container-id}"
        printf '%s\n' "{\"outcome\":\"success\",\"containerId\":\"${container_id}\"}"
        ;;
      *)
        if [ "${FAKE_SBX_PROBE_FAILURE:-}" = "1" ]; then
          case "$*" in
            *"devcontainer exec"*" true"*) exit 46 ;;
          esac
        fi
        if [ "${FAKE_SBX_REQUIRE_LOGIN_SHELL:-}" = "1" ]; then
          case "$*" in
            *"devcontainer exec"*"-lic"*"ruby --version"*)
              printf '%s\n' 'ruby 3.4.4 (mise)'
              exit 0
              ;;
            *"ruby --version"*)
              printf '%s\n' 'ruby is absent without the devcontainer login environment' >&2
              exit 44
              ;;
          esac
        fi
        if [ -n "${FAKE_SBX_COMMAND_EXIT:-}" ] && printf '%s' "$*" | grep -q 'devcontainer exec'; then
          printf '%s\n' 'simulated command failure' >&2
          exit "$FAKE_SBX_COMMAND_EXIT"
        fi
        printf '%s\n' "fake-sbx-command-ok"
        ;;
    esac
    ;;
  rm)
    rm -f "$FAKE_SBX_STATE"
    ;;
  *)
    printf '%s\n' "unexpected fake sbx command: $*" >&2
    exit 2
    ;;
esac
"#,
    )
    .expect("write fake sbx");
    let mut permissions = fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&binary, permissions).unwrap();
    (temp, binary, state, log)
}

#[cfg(unix)]
fn create_fake_local_vm() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = TempDir::new().expect("create fake local-vm temp dir");
    let binary = temp.path().join("branchbox-local-vm");
    let state = temp.path().join("state");
    let log = temp.path().join("calls.log");
    fs::write(
        &binary,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_LOCAL_VM_LOG"
case "$1" in
  validate) ;;
  prepare)
    printf '%s\n' 'branchbox-fake-local-vm' > "$FAKE_LOCAL_VM_STATE"
    printf '%s\n' '{"runtime_id":"branchbox-fake-local-vm","published_ports":[{"host":33123,"runtime":3000}],"monitor":"Firecracker v1.16.1","kernel_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rootfs_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}'
    ;;
  exists)
    [ -f "$FAKE_LOCAL_VM_STATE" ]
    ;;
  start|probe)
    [ -f "$FAKE_LOCAL_VM_STATE" ]
    ;;
  exec|exec-interactive)
    [ -f "$FAKE_LOCAL_VM_STATE" ]
    printf '%s\n' 'local-vm-command-ok'
    ;;
  destroy)
    rm -f "$FAKE_LOCAL_VM_STATE"
    ;;
  *)
    printf '%s\n' "unexpected fake local-vm command: $*" >&2
    exit 2
    ;;
esac
"#,
    )
    .expect("write fake local-vm driver");
    let mut permissions = fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&binary, permissions).unwrap();
    (temp, binary, state, log)
}

#[cfg(unix)]
fn create_fake_docker() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = TempDir::new().expect("create fake docker temp dir");
    let binary = temp.path().join("docker");
    let state = temp.path().join("devcontainer-project");
    let log = temp.path().join("calls.log");
    fs::write(
        &binary,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_DOCKER_LOG"

case "${1:-}" in
  version)
    exit 0
    ;;
  compose)
    shift
    case " $* " in
      *" config "*) exit 0 ;;
      *" ps "*)
        case " $* " in
          *" -p $(cat "$FAKE_DOCKER_STATE" 2>/dev/null || true) "*)
            if [ -f "$FAKE_DOCKER_STATE" ]; then printf '%s\n' fake-container-id; fi
            ;;
        esac
        ;;
      *" down "*)
        if [ -f "$FAKE_DOCKER_STATE" ]; then
          project=$(cat "$FAKE_DOCKER_STATE")
          case " $* " in
            *" -p $project "*|*" --project-name $project "*) rm -f "$FAKE_DOCKER_STATE" ;;
          esac
        fi
        ;;
    esac
    ;;
  ps)
    if [ -f "$FAKE_DOCKER_STATE" ]; then
      project=$(cat "$FAKE_DOCKER_STATE")
      case " $* " in
        *"label=devcontainer.local_folder="*) printf '%s\n' "$project" ;;
        *"label=com.docker.compose.project=$project"*) printf '%s\n' fake-container-id ;;
      esac
    fi
    ;;
  network|volume|rm)
    ;;
esac
"#,
    )
    .expect("write fake docker");
    let mut permissions = fs::metadata(&binary).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&binary, permissions).unwrap();
    (temp, binary, state, log)
}

#[cfg(unix)]
struct FakeInGuestRuntime {
    _temp: TempDir,
    devcontainer: PathBuf,
    docker: PathBuf,
    resources: PathBuf,
    log: PathBuf,
}

#[cfg(unix)]
fn create_fake_in_guest_runtime() -> FakeInGuestRuntime {
    let temp = TempDir::new().expect("create fake in-guest runtime");
    let devcontainer = temp.path().join("devcontainer");
    let docker = temp.path().join("docker");
    let resources = temp.path().join("resources");
    let log = temp.path().join("docker.log");
    fs::create_dir_all(&resources).expect("create fake Docker resources");
    fs::write(
        &devcontainer,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version) printf '%s\n' '0.80.0' ;;
  read-configuration)
    printf '%s\n' '{"configuration":{"privileged":false}}'
    ;;
  up)
    override="$FAKE_IN_GUEST_WORKSPACE/.devcontainer/.branchbox-sbx-compose.yaml"
    if ! grep -q 'format: raw' "$override" || ! grep -Fq "$FAKE_IN_GUEST_PROJECT_ENVIRONMENT" "$override" || grep -q 'password with spaces' "$override"; then
      printf '%s\n' 'project-environment env-file facade was not isolated correctly' >&2
      exit 41
    fi
    touch "$FAKE_IN_GUEST_RESOURCES/partial-main"
    touch "$FAKE_IN_GUEST_RESOURCES/partial-db"
    touch "$FAKE_IN_GUEST_RESOURCES/partial-network"
    touch "$FAKE_IN_GUEST_RESOURCES/partial-volume"
    printf '%s\n' 'postCreateCommand failed after Compose dependencies started' >&2
    exit 42
    ;;
  *) exit 0 ;;
esac
"#,
    )
    .expect("write fake devcontainer");
    fs::write(
        &docker,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_IN_GUEST_DOCKER_LOG"
resources="$FAKE_IN_GUEST_RESOURCES"
workspace="$FAKE_IN_GUEST_WORKSPACE"
project=agentify_runtime_generated

case "${1:-}" in
  info) exit 0 ;;
  compose)
    if [ "${2:-}" = "version" ]; then printf '%s\n' '2.30.0'; fi
    ;;
  ps)
    case "$*" in
      *"label=devcontainer.local_folder="*)
        if [ -f "$resources/partial-main" ]; then
          case "$*" in
            *"com.docker.compose.project"*) printf 'partial-main\t%s\n' "$project" ;;
            *) printf '%s\n' 'partial-main' ;;
          esac
        fi
        ;;
      *"label=com.docker.compose.project=$project"*)
        [ ! -f "$resources/partial-main" ] || printf '%s\n' 'partial-main'
        [ ! -f "$resources/partial-db" ] || printf '%s\n' 'partial-db'
        ;;
      *"label=com.docker.compose.project"*)
        [ ! -f "$resources/partial-main" ] || printf 'partial-main\t%s\t%s/.devcontainer\t%s/.devcontainer/compose.yaml\n' "$project" "$workspace" "$workspace"
        [ ! -f "$resources/partial-db" ] || printf 'partial-db\t%s\t%s/.devcontainer\t%s/.devcontainer/compose.yaml\n' "$project" "$workspace" "$workspace"
        ;;
    esac
    ;;
  network)
    case "${2:-}" in
      ls) [ ! -f "$resources/partial-network" ] || printf '%s\n' 'partial-network' ;;
      rm) rm -f "$resources/${3:-}" ;;
    esac
    ;;
  volume)
    case "${2:-}" in
      ls) [ ! -f "$resources/partial-volume" ] || printf '%s\n' 'partial-volume' ;;
      rm) rm -f "$resources/${3:-}" ;;
    esac
    ;;
  rm)
    for identifier in "$@"; do
      rm -f "$resources/$identifier"
    done
    ;;
  inspect) exit 1 ;;
esac
"#,
    )
    .expect("write fake Docker");
    for binary in [&devcontainer, &docker] {
        let mut permissions = fs::metadata(binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(binary, permissions).unwrap();
    }
    FakeInGuestRuntime {
        _temp: temp,
        devcontainer,
        docker,
        resources,
        log,
    }
}

#[cfg(unix)]
fn commit_in_guest_devcontainer(test_repo: &TestRepo) -> String {
    let devcontainer = test_repo.ensure_devcontainer_dir();
    fs::write(
        devcontainer.join("devcontainer.json"),
        r#"{
  "name": "Agentify",
  "dockerComposeFile": "compose.yaml",
  "service": "app",
  "workspaceFolder": "/workspaces/main",
  "forwardPorts": [3000],
  "postCreateCommand": "false"
}
"#,
    )
    .unwrap();
    fs::write(
        devcontainer.join("compose.yaml"),
        r#"name: agentify
services:
  app:
    image: alpine:3.19
    command: sleep infinity
    depends_on:
      - postgres
  postgres:
    image: postgres:16
"#,
    )
    .unwrap();
    run_git(test_repo.path(), &["add", ".devcontainer"]);
    run_git(
        test_repo.path(),
        &["commit", "-m", "Add in-guest devcontainer fixture"],
    );
    let output = StdCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(test_repo.path())
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[cfg(unix)]
fn write_in_guest_manifest(
    root: &Path,
    test_repo: &TestRepo,
    work_feature: &str,
    revision: &str,
) -> PathBuf {
    let manifest_path = root.join("branchbox-in-guest.json");
    let materializations = root.join("materializations");
    fs::create_dir_all(&materializations).unwrap();
    let project_environment = materializations.join("project-environment.env");
    fs::write(
        &project_environment,
        b"ACCOUNT_NAME=Matchup\nADMIN_EMAIL=admin@example.com\nADMIN_PASSWORD=$2b$12# password with spaces\n",
    )
    .unwrap();
    let mut environment_permissions = fs::metadata(&project_environment).unwrap().permissions();
    environment_permissions.set_mode(0o600);
    fs::set_permissions(&project_environment, environment_permissions).unwrap();
    let manifest = serde_json::json!({
        "version": "1",
        "run_id": format!("run-{work_feature}"),
        "lease_id": "assignment-lease",
        "outer_runtime_id": "outer-vm",
        "workspace": test_repo.worktree_parent(),
        "repository": {
            "path": test_repo.path(),
            "revision": revision
        },
        "task_branch": format!("feature/{work_feature}"),
        "tunnel_placement": "outer",
        "published_ports": [{"host": 3000, "runtime": 3000}],
        "leases": [
            {
                "lease_id": "project-environment",
                "scope": "project-environment",
                "consumer": "app",
                "materializations": [{
                    "source_path": project_environment,
                    "target_path": "/run/branchbox/leases/project-env",
                    "sha256": "e54f59da53efa290bc6bf0f61b90ee9d56f947934b346600ad45311efa11d7b6"
                }]
            },
            {
                "lease_id": "outer-tunnel",
                "scope": "platform-tunnel",
                "consumer": "outer-connector",
                "materializations": []
            }
        ]
    });
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let mut permissions = fs::metadata(&manifest_path).unwrap().permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&manifest_path, permissions).unwrap();
    manifest_path
}

#[test]
fn feature_start_list_teardown_end_to_end() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "integration-test-feature";
    let worktree_path = test_repo.worktree_parent().join(work_feature);

    // Start feature
    branchbox_cmd!(repo_path)
        .args(["feature", "start", work_feature])
        .assert()
        .success()
        .stdout(predicate::str::contains("Feature workspace ready (full)"));

    assert!(
        worktree_path.exists(),
        "expected worktree directory to be created"
    );

    // Spec stub should be created inside the feature worktree under docs/features/in-progress
    let spec_path = worktree_path
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
    branchbox_cmd!(repo_path)
        .args(["feature", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(work_feature)
                .and(predicate::str::contains("Mode"))
                .and(predicate::str::contains("Modules")),
        );

    // Teardown feature
    branchbox_cmd!(repo_path)
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
    branchbox_cmd!(repo_path)
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
fn in_guest_start_requires_absolute_runtime_manifest_before_worktree_creation() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let worktree = test_repo
        .worktree_parent()
        .join("in-guest-missing-manifest");

    branchbox_cmd!(repo_path)
        .args([
            "feature",
            "start",
            "in-guest-missing-manifest",
            "--runtime",
            "in-guest",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "requires --runtime-manifest with a trusted guest path",
        ));

    assert!(!worktree.exists());
}

#[test]
fn runtime_manifest_is_rejected_for_non_in_guest_runtime() {
    let test_repo = init_test_repo();
    branchbox_cmd!(test_repo.path())
        .args([
            "feature",
            "start",
            "wrong-runtime-manifest",
            "--runtime",
            "container",
            "--runtime-manifest",
            "/run/agentify-runtime/branchbox-in-guest.json",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--runtime-manifest is accepted only with --runtime in-guest",
        ));
}

#[test]
fn teardown_json_includes_verified_runtime_residue_contract() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "teardown-json-runtime";
    branchbox_cmd!(repo_path)
        .args(["feature", "start", work_feature])
        .assert()
        .success();

    let output = branchbox_cmd!(repo_path)
        .args([
            "feature",
            "teardown",
            work_feature,
            "--delete-branch",
            "--force",
            "--json",
        ])
        .output()
        .expect("feature teardown --json");
    assert!(
        output.status.success(),
        "teardown failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("teardown JSON");
    assert_eq!(payload["runtime_teardown"]["provider"], "container");
    assert_eq!(payload["runtime_teardown"]["verified"], true);
    assert_eq!(payload["runtime_teardown"]["residue_free"], true);
    assert_eq!(
        payload["runtime_teardown"]["residue"],
        serde_json::json!([])
    );
}

#[test]
fn exec_provider_cli_accepts_fixed_provider_contract_before_feature_lookup() {
    let test_repo = init_test_repo();
    branchbox_cmd!(test_repo.path())
        .args([
            "feature",
            "exec-provider",
            "missing-feature",
            "--provider",
            "codex",
            "--inherit-env",
            "OPENAI_API_KEY",
            "--",
            "--version",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing-feature"));
}

#[cfg(unix)]
#[test]
fn in_guest_partial_start_failure_removes_compose_residue_worktree_and_branch() {
    let test_repo = init_test_repo();
    let revision = commit_in_guest_devcontainer(&test_repo);
    let fake = create_fake_in_guest_runtime();
    let assignment = TempDir::new().expect("create assignment directory");
    let work_feature = "in-guest-partial-start";
    let manifest = write_in_guest_manifest(assignment.path(), &test_repo, work_feature, &revision);
    let worktree = test_repo.worktree_parent().join(work_feature);

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_DEVCONTAINER_PATH" => &fake.devcontainer,
        "BRANCHBOX_DOCKER_PATH" => &fake.docker,
        "FAKE_IN_GUEST_RESOURCES" => &fake.resources,
        "FAKE_IN_GUEST_DOCKER_LOG" => &fake.log,
        "FAKE_IN_GUEST_WORKSPACE" => &worktree,
        "FAKE_IN_GUEST_PROJECT_ENVIRONMENT" => assignment.path().join("materializations/project-environment.env"),
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "in-guest",
        "--runtime-manifest",
        manifest.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "postCreateCommand failed after Compose dependencies started",
    ));

    assert!(!worktree.exists(), "failed in-guest worktree leaked");
    let branch = StdCommand::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/feature/{work_feature}"),
        ])
        .current_dir(test_repo.path())
        .status()
        .unwrap();
    assert!(!branch.success(), "failed in-guest task branch leaked");
    assert_eq!(
        fs::read_dir(&fake.resources).unwrap().count(),
        0,
        "partial Compose resources leaked; Docker calls:\n{}",
        fs::read_to_string(&fake.log).unwrap_or_default()
    );
    assert!(
        !assignment
            .path()
            .join("materializations/project-environment.env")
            .exists(),
        "failed-start cleanup leaked the project-environment materialization"
    );
    let provider_states = test_repo.path().join(".branchbox/runtime/in-guest");
    assert!(
        !provider_states.exists() || fs::read_dir(provider_states).unwrap().next().is_none(),
        "successful failed-start cleanup should remove provider state"
    );
}

#[cfg(unix)]
#[test]
fn in_guest_no_registry_teardown_recovers_state_without_project_modules() {
    let test_repo = init_test_repo();
    commit_in_guest_devcontainer(&test_repo);
    let fake = create_fake_in_guest_runtime();
    let work_feature = "in-guest-orphaned-start";
    let branch_name = format!("feature/{work_feature}");
    let worktree = test_repo.worktree_parent().join(work_feature);
    let status = StdCommand::new("git")
        .args(["worktree", "add", "-b", &branch_name])
        .arg(&worktree)
        .current_dir(test_repo.path())
        .status()
        .unwrap();
    assert!(status.success());
    for resource in [
        "partial-main",
        "partial-db",
        "partial-network",
        "partial-volume",
    ] {
        fs::write(fake.resources.join(resource), b"").unwrap();
    }
    let state_dir = test_repo.path().join(".branchbox/runtime/in-guest");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("orphaned-run.json");
    let state = serde_json::json!({
        "version": "1",
        "manifest_path": "/run/agentify-runtime/branchbox-in-guest.json",
        "worktree_path": worktree.clone(),
        "workspace_paths": [worktree.clone()],
        "config_path": worktree.join(".devcontainer/.devcontainer.json"),
        "run_id": "orphaned-run",
        "outer_runtime_id": "outer-vm",
        "materializations": [],
        "proxy_names": [],
        "compose_projects": ["agentify"],
        "container_id": null
    });
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    let mut permissions = fs::metadata(&state_path).unwrap().permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&state_path, permissions).unwrap();

    let output = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_DEVCONTAINER_PATH" => &fake.devcontainer,
        "BRANCHBOX_DOCKER_PATH" => &fake.docker,
        "FAKE_IN_GUEST_RESOURCES" => &fake.resources,
        "FAKE_IN_GUEST_DOCKER_LOG" => &fake.log,
        "FAKE_IN_GUEST_WORKSPACE" => &worktree,
    )
    .args([
        "feature",
        "teardown",
        work_feature,
        "--delete-branch",
        "--force",
        "--force-delete-branch",
        "--json",
    ])
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "orphan teardown failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["runtime_teardown"]["provider"], "in-guest");
    assert_eq!(payload["runtime_teardown"]["residue_free"], true);
    assert_eq!(payload["module_reports"], serde_json::json!([]));
    assert_eq!(payload["adapter_cleanup_warnings"], serde_json::json!([]));
    assert!(!worktree.exists());
    assert!(!state_path.exists());
    assert_eq!(fs::read_dir(&fake.resources).unwrap().count(), 0);
    let calls = fs::read_to_string(&fake.log).unwrap_or_default();
    assert!(
        !calls.lines().any(|line| line.starts_with("compose ")),
        "no-registry in-guest teardown ran host-side Compose/modules:\n{calls}"
    );
}

#[cfg(unix)]
#[test]
fn feature_teardown_removes_devcontainer_cli_compose_project() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let repo_path = test_repo.path();
    let work_feature = "devcontainer-cleanup";
    let worktree_path = test_repo.worktree_parent().join(work_feature);
    let (fake_docker, _binary, state, log) = create_fake_docker();
    let path = format!(
        "{}:{}",
        fake_docker.path().display(),
        std::env::var("PATH").expect("PATH is set")
    );

    branchbox_cmd!(
        repo_path,
        "PATH" => &path,
        "FAKE_DOCKER_STATE" => &state,
        "FAKE_DOCKER_LOG" => &log,
    )
    .args(["feature", "start", work_feature, "--skip-module", "tunnel"])
    .assert()
    .success();

    fs::write(&state, format!("{work_feature}_devcontainer"))
        .expect("simulate devcontainer CLI compose project");

    branchbox_cmd!(
        repo_path,
        "PATH" => &path,
        "FAKE_DOCKER_STATE" => &state,
        "FAKE_DOCKER_LOG" => &log,
    )
    .args([
        "feature",
        "teardown",
        work_feature,
        "--force",
        "--force-delete-branch",
    ])
    .assert()
    .success();

    assert!(
        !worktree_path.exists(),
        "feature worktree should be removed"
    );
    assert!(
        !state.exists(),
        "teardown leaked the devcontainer CLI compose project; docker calls:\n{}",
        fs::read_to_string(log).unwrap_or_default()
    );
}

#[test]
fn feature_start_rejects_container_without_override() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args(["feature", "start", "container-rejected"])
        .output()
        .expect("run feature start in container");

    assert!(
        !output.status.success(),
        "feature start should fail host validation without override"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must be run on the host machine"),
        "expected host validation error, got: {stderr}"
    );
}

#[test]
fn feature_start_allows_container_with_override_flag() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args([
            "feature",
            "start",
            "container-allowed",
            "--allow-container",
            "--json",
        ])
        .output()
        .expect("run feature start --allow-container");

    assert!(
        output.status.success(),
        "feature start should succeed with --allow-container: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: Value =
        serde_json::from_slice(&output.stdout).expect("parse feature start JSON summary");
    assert_eq!(summary["work_feature"], "container-allowed");
}

#[test]
fn feature_start_allows_container_with_no_host_check_alias() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args([
            "feature",
            "start",
            "container-allowed-alias",
            "--no-host-check",
            "--json",
        ])
        .output()
        .expect("run feature start --no-host-check");

    assert!(
        output.status.success(),
        "feature start should succeed with --no-host-check: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn feature_teardown_rejects_container_without_override() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    // Create a feature (bypassing host check for setup).
    let output = branchbox_cmd!(repo_path)
        .args(["feature", "start", "container-td-test", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "feature start should succeed");

    // Teardown WITHOUT override — should fail.
    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args(["feature", "teardown", "container-td-test"])
        .output()
        .expect("run feature teardown in container");

    assert!(
        !output.status.success(),
        "feature teardown should fail host validation without override"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must be run on the host machine"),
        "expected host validation error, got: {stderr}"
    );
}

#[test]
fn feature_teardown_allows_container_with_override_flag() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    // Create a feature (bypassing host check for setup).
    let output = branchbox_cmd!(repo_path)
        .args(["feature", "start", "container-td-allowed", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "feature start should succeed");

    // Teardown WITH --allow-container — should succeed.
    // Also pass --force to bypass the dirty-worktree guard (devcontainer files synced during start).
    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args([
            "feature",
            "teardown",
            "container-td-allowed",
            "--allow-container",
            "--force",
        ])
        .output()
        .expect("run feature teardown --allow-container");

    assert!(
        output.status.success(),
        "feature teardown should succeed with --allow-container: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn feature_teardown_allows_container_with_no_host_check_alias() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    // Create a feature (bypassing host check for setup).
    let output = branchbox_cmd!(repo_path)
        .args(["feature", "start", "container-td-alias", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "feature start should succeed");

    // Teardown WITH --no-host-check alias — should succeed.
    // Also pass --force to bypass the dirty-worktree guard (devcontainer files synced during start).
    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args([
            "feature",
            "teardown",
            "container-td-alias",
            "--no-host-check",
            "--force",
        ])
        .output()
        .expect("run feature teardown --no-host-check");

    assert!(
        output.status.success(),
        "feature teardown should succeed with --no-host-check: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn feature_prune_rejects_container_without_override() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    // Create a feature (bypassing host check for setup).
    let output = branchbox_cmd!(repo_path)
        .args(["feature", "start", "container-prune-test", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "feature start should succeed");

    // Prune WITHOUT override — should fail.
    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args(["feature", "prune", "--yes"])
        .output()
        .expect("run feature prune in container");

    assert!(
        !output.status.success(),
        "feature prune should fail host validation without override"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must be run on the host machine"),
        "expected host validation error, got: {stderr}"
    );
}

#[test]
fn feature_prune_allows_container_with_override_flag() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    // Create a feature (bypassing host check for setup).
    let output = branchbox_cmd!(repo_path)
        .args(["feature", "start", "container-prune-allowed", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "feature start should succeed");

    // Prune WITH --allow-container — should succeed.
    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args(["feature", "prune", "--yes", "--allow-container"])
        .output()
        .expect("run feature prune --allow-container");

    assert!(
        output.status.success(),
        "feature prune should succeed with --allow-container: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn feature_prune_allows_container_with_no_host_check_alias() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    // Create a feature (bypassing host check for setup).
    let output = branchbox_cmd!(repo_path)
        .args(["feature", "start", "container-prune-alias", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "feature start should succeed");

    // Prune WITH --no-host-check alias — should succeed.
    let output = Command::new(cargo_bin!("branchbox"))
        .current_dir(repo_path)
        .env("DOCKER_CONTAINER", "1")
        .env("RUST_LOG", "off")
        .args(["feature", "prune", "--yes", "--no-host-check"])
        .output()
        .expect("run feature prune --no-host-check");

    assert!(
        output.status.success(),
        "feature prune should succeed with --no-host-check: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn feature_start_minimal_mode_json_summary() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "minimal-mode-feature";
    let worktree_path = test_repo.worktree_parent().join(work_feature);

    let output = branchbox_cmd!(repo_path)
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
    assert_eq!(
        summary["runtime"]["provider"],
        Value::String("container".into())
    );

    let skipped_modules = summary["skipped_modules"]
        .as_array()
        .expect("skipped modules array");
    assert!(skipped_modules
        .iter()
        .any(|entry| entry["module"] == Value::String("devcontainer".into())));

    assert!(summary["prompt_bridge_enabled"].is_boolean());
    assert_eq!(
        summary["default_agent"]["status"],
        Value::String("disabled".into())
    );

    assert!(worktree_path.exists());

    let list_output = branchbox_cmd!(repo_path)
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
    assert_eq!(
        first["runtime"]["provider"],
        Value::String("container".into())
    );
    let module_outcomes = first["module_outcomes"]
        .as_array()
        .expect("module_outcomes should be an array");
    assert!(
        !module_outcomes.is_empty(),
        "module_outcomes should include entries from feature start"
    );
    assert_eq!(
        first["default_agent"]["status"],
        Value::String("disabled".into())
    );

    branchbox_cmd!(repo_path)
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

#[test]
fn feature_start_json_keeps_diagnostics_on_stderr() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "json-diagnostics";

    fs::remove_file(repo_path.join(".env")).expect("remove .env to trigger warning");

    let output = branchbox_cmd!(repo_path, "RUST_LOG" => "warn")
        .args([
            "feature",
            "start",
            work_feature,
            "--skip-module",
            "tunnel",
            "--json",
        ])
        .output()
        .expect("run warning-producing feature start --json");

    assert!(
        output.status.success(),
        "expected feature start to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: Value = serde_json::from_slice(&output.stdout)
        .expect("diagnostics must not corrupt feature start JSON stdout");
    assert_eq!(summary["work_feature"], Value::String(work_feature.into()));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("No .env found"),
        "expected missing .env diagnostic on stderr"
    );

    branchbox_cmd!(repo_path)
        .args([
            "feature",
            "teardown",
            work_feature,
            "--delete-branch",
            "--force",
        ])
        .assert()
        .success();
}

#[cfg(unix)]
#[test]
fn local_vm_runtime_full_cli_lifecycle_reports_immutable_artifacts() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let repo_path = test_repo.path();
    let work_feature = "local-vm-e2e";
    let worktree_path = test_repo.worktree_parent().join(work_feature);
    let (_fake_temp, fake_driver, fake_state, fake_log) = create_fake_local_vm();

    let start = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_LOCAL_VM_DRIVER_PATH" => &fake_driver,
        "FAKE_LOCAL_VM_STATE" => &fake_state,
        "FAKE_LOCAL_VM_LOG" => &fake_log,
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "local-vm",
        "--json",
    ])
    .output()
    .expect("start local-vm feature");
    assert!(
        start.status.success(),
        "local-vm start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    let summary: Value = serde_json::from_slice(&start.stdout).expect("parse start metadata");
    assert_eq!(summary["runtime"]["provider"], "local-vm");
    assert_eq!(summary["runtime"]["runtime_id"], "branchbox-fake-local-vm");
    assert_eq!(
        summary["runtime"]["version"]["monitor"],
        "Firecracker v1.16.1"
    );
    assert_eq!(summary["runtime"]["published_ports"][0]["host"], 33123);

    let exec = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_LOCAL_VM_DRIVER_PATH" => &fake_driver,
        "FAKE_LOCAL_VM_STATE" => &fake_state,
        "FAKE_LOCAL_VM_LOG" => &fake_log,
    )
    .args([
        "feature",
        "exec",
        work_feature,
        "--json",
        "--",
        "git",
        "status",
    ])
    .output()
    .expect("exec through local-vm");
    assert!(exec.status.success());
    let result: Value = serde_json::from_slice(&exec.stdout).expect("parse exec metadata");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["stdout"], "local-vm-command-ok\n");

    branchbox_cmd!(
        repo_path,
        "BRANCHBOX_LOCAL_VM_DRIVER_PATH" => &fake_driver,
        "FAKE_LOCAL_VM_STATE" => &fake_state,
        "FAKE_LOCAL_VM_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();

    assert!(!worktree_path.exists());
    assert!(!fake_state.exists());
    let calls = fs::read_to_string(fake_log).expect("read fake local-vm calls");
    assert!(calls.contains("validate"));
    assert!(calls.contains("prepare"));
    assert!(calls.contains("start branchbox-fake-local-vm"));
    assert!(calls.contains("exec branchbox-fake-local-vm"));
    assert!(calls.contains("destroy branchbox-fake-local-vm"));
}

#[cfg(unix)]
#[test]
fn sbx_runtime_full_cli_lifecycle() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let repo_path = test_repo.path();
    let work_feature = "sbx-e2e";
    let worktree_path = test_repo.worktree_parent().join(work_feature);
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    let output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--json",
    ])
    .output()
    .expect("start feature through fake sbx");
    assert!(
        output.status.success(),
        "SBX feature start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).expect("parse SBX start JSON");
    assert_eq!(summary["runtime"]["provider"], "sbx");
    assert!(summary["runtime"]["runtime_id"]
        .as_str()
        .unwrap()
        .starts_with("branchbox-"));
    let published_host = summary["runtime"]["published_ports"][0]["host"]
        .as_u64()
        .expect("published host port");
    assert!((1..=u16::MAX as u64).contains(&published_host));
    assert_eq!(summary["runtime"]["published_ports"][0]["runtime"], 3000);
    assert!(worktree_path.exists());

    let exec_output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args([
        "feature",
        "exec",
        work_feature,
        "--json",
        "--",
        "codex",
        "--version",
    ])
    .output()
    .expect("execute coding-agent probe through fake sbx");
    assert!(exec_output.status.success());
    let exec_result: Value =
        serde_json::from_slice(&exec_output.stdout).expect("parse runtime exec JSON");
    assert_eq!(exec_result["exit_code"], 0);
    assert!(exec_result["stdout"]
        .as_str()
        .unwrap()
        .contains("fake-sbx-command-ok"));

    branchbox_cmd!(
        repo_path,
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();
    assert!(!worktree_path.exists());
    assert!(!fake_state.exists());

    let calls = fs::read_to_string(fake_log).expect("read fake sbx calls");
    assert!(calls.contains("create shell"));
    assert!(calls.contains("ports branchbox-"));
    assert!(calls.contains(&format!("--publish {published_host}:3000")));
    assert!(calls.contains("exec --workdir"));
    assert!(calls.contains("bash -lc"));
    assert!(calls.contains("env -u NPM_CONFIG_PREFIX npx"));
    assert!(calls.contains("@devcontainers/cli exec --workspace-folder ."));
    assert!(calls.contains("branchbox-port-proxy fake-container-id 3000"));
    assert!(calls.contains("codex --version"));
    assert!(calls.contains("rm --force branchbox-"));
}

#[cfg(unix)]
#[test]
fn sbx_exec_reconciles_resumed_devcontainer_and_refreshes_port_proxy() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let work_feature = "sbx-resume";
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "start", work_feature, "--runtime", "sbx"])
    .assert()
    .success();

    let degraded = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_PROBE_FAILURE" => "1",
    )
    .args(["feature", "list", "--json"])
    .output()
    .expect("inspect stopped devcontainer health");
    let entries: Value = serde_json::from_slice(&degraded.stdout).expect("parse degraded list");
    assert_eq!(entries[0]["status"], "degraded");

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_RECONCILE_CONTAINER_ID" => "resumed-container-id",
    )
    .args(["feature", "exec", work_feature, "--", "codex", "--version"])
    .assert()
    .success();

    let calls = fs::read_to_string(&fake_log).expect("read fake SBX calls");
    let reconciled_up = calls
        .rfind("devcontainer up")
        .expect("reconcile devcontainer up");
    let command = calls.rfind("codex --version").expect("runtime command");
    assert!(
        reconciled_up < command,
        "command ran before reconciliation: {calls}"
    );
    assert!(calls.contains("branchbox-port-proxy resumed-container-id 3000"));

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();
}

#[cfg(unix)]
#[test]
fn sbx_exec_restores_login_toolchain_environment_and_reports_exit_status() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let work_feature = "sbx-login-env";
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "start", work_feature, "--runtime", "sbx"])
    .assert()
    .success();

    let success = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_REQUIRE_LOGIN_SHELL" => "1",
    )
    .args([
        "feature",
        "exec",
        work_feature,
        "--json",
        "--",
        "ruby",
        "--version",
    ])
    .output()
    .expect("execute version-manager tool");
    assert!(success.status.success());
    let result: Value = serde_json::from_slice(&success.stdout).expect("parse exec JSON");
    assert_eq!(result["exit_code"], 0);
    assert!(result["stdout"].as_str().unwrap().contains("ruby 3.4.4"));

    let failure = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_COMMAND_EXIT" => "23",
    )
    .args(["feature", "exec", work_feature, "--json", "--", "false"])
    .output()
    .expect("execute failing runtime command");
    assert!(!failure.status.success());
    let result: Value = serde_json::from_slice(&failure.stdout).expect("parse failing exec JSON");
    assert_eq!(result["exit_code"], 23);

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();
}

#[cfg(unix)]
#[test]
fn failed_start_reuse_requires_explicit_devcontainer_conflict_policy() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    run_git(test_repo.path(), &["add", ".devcontainer"]);
    run_git(
        test_repo.path(),
        &["commit", "-m", "Add devcontainer fixture"],
    );
    let work_feature = "sbx-reuse-customization";
    let worktree_path = test_repo.worktree_parent().join(work_feature);
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_START_FAILURE" => "1",
    )
    .args(["feature", "start", work_feature, "--runtime", "sbx"])
    .assert()
    .failure();

    let compose_path = worktree_path.join(".devcontainer/docker-compose.yml");
    let customized = "services:\n  dev:\n    image: alpine:3.20\n    environment:\n      FEATURE_LOCAL: preserved\n";
    fs::write(&compose_path, customized).expect("customize feature Compose file");

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--reuse",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "Refusing to overwrite feature-local devcontainer changes",
    ));
    assert_eq!(fs::read_to_string(&compose_path).unwrap(), customized);

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--reuse",
        "--devcontainer-reuse",
        "inspect",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("git diff --no-index"));
    assert_eq!(fs::read_to_string(&compose_path).unwrap(), customized);

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--reuse",
        "--devcontainer-reuse",
        "preserve",
    ])
    .assert()
    .success();
    assert_eq!(fs::read_to_string(&compose_path).unwrap(), customized);

    let registry: Value = serde_json::from_slice(
        &fs::read(test_repo.path().join(".branchbox/registry.json"))
            .expect("read feature registry"),
    )
    .expect("parse registry");
    assert_eq!(
        registry["features"][0]["sync_strategy"],
        "copy:reuse-preserve"
    );

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();
}

#[cfg(unix)]
#[test]
fn sbx_failed_runtime_can_be_retained_reused_and_torn_down() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    run_git(test_repo.path(), &["add", ".devcontainer"]);
    run_git(
        test_repo.path(),
        &["commit", "-m", "Add devcontainer fixture"],
    );
    let work_feature = "sbx-retained-retry";
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    let failed = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_START_FAILURE" => "1",
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--keep-runtime-on-failure",
    ])
    .output()
    .expect("run retained SBX failure");
    assert!(!failed.status.success());
    let failure_text = String::from_utf8_lossy(&failed.stderr);
    assert!(failure_text.contains("Retained SBX runtime"));
    assert!(failure_text.contains("--reuse-runtime"));
    assert!(fake_state.exists());

    let list = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "list", "--json"])
    .output()
    .expect("list retained feature");
    let entries: Value = serde_json::from_slice(&list.stdout).expect("parse retained list");
    assert_eq!(entries[0]["status"], "failed_retained");

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--reuse-runtime",
    ])
    .assert()
    .success();

    let calls = fs::read_to_string(&fake_log).expect("read fake SBX calls");
    assert_eq!(
        calls.matches("create shell").count(),
        1,
        "runtime was recreated: {calls}"
    );

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();
    assert!(!fake_state.exists());
}

#[cfg(unix)]
#[test]
fn retained_sbx_runtime_is_reported_orphaned_when_boundary_disappears() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();
    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_START_FAILURE" => "1",
    )
    .args([
        "feature",
        "start",
        "sbx-orphan",
        "--runtime",
        "sbx",
        "--keep-runtime-on-failure",
    ])
    .assert()
    .failure();
    fs::remove_file(&fake_state).expect("simulate externally removed sandbox");

    let list = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "list", "--json"])
    .output()
    .expect("list orphaned feature");
    let entries: Value = serde_json::from_slice(&list.stdout).expect("parse orphan list");
    assert_eq!(entries[0]["status"], "orphaned");
}

#[cfg(unix)]
#[test]
fn sbx_run_services_excludes_incompatible_sidecar_and_keeps_required_database() {
    let test_repo = init_test_repo();
    let devcontainer = test_repo.ensure_devcontainer_dir();
    fs::write(
        devcontainer.join("devcontainer.json"),
        r#"{
  "name": "compose-stack",
  "dockerComposeFile": "compose.yaml",
  "service": "app",
  "workspaceFolder": "/workspaces/repo"
}
"#,
    )
    .unwrap();
    fs::write(
        devcontainer.join("compose.yaml"),
        r#"services:
  app:
    image: alpine:3.19
    depends_on:
      - postgres
    volumes:
      - ../../main/.git:/workspaces/main/.git
  postgres:
    image: postgres:17
  tailscale:
    image: tailscale/tailscale:latest
    devices:
      - /dev/net/tun:/dev/net/tun
    cap_add:
      - net_admin
      - net_raw
"#,
    )
    .unwrap();
    run_git(test_repo.path(), &["add", ".devcontainer"]);
    run_git(
        test_repo.path(),
        &["commit", "-m", "Add multi-service fixture"],
    );
    let work_feature = "sbx-optional-sidecar";
    let worktree_path = test_repo.worktree_parent().join(work_feature);
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    let preflight = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "start", work_feature, "--runtime", "sbx"])
    .output()
    .expect("run unsupported-device preflight");
    assert!(!preflight.status.success());
    assert!(String::from_utf8_lossy(&preflight.stderr).contains("/dev/net/tun"));
    assert!(!fake_state.exists());

    fs::create_dir_all(test_repo.path().join(".branchbox")).unwrap();
    fs::write(
        test_repo.path().join(".branchbox/config.json"),
        r#"{"runtime":{"sbx":{"run_services":["app"]}}}"#,
    )
    .unwrap();
    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_REQUIRE_RUN_SERVICES" => "1",
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--reuse",
    ])
    .assert()
    .success();

    let override_config: Value = serde_json::from_slice(
        &fs::read(worktree_path.join(".devcontainer/.devcontainer.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(override_config["runServices"], serde_json::json!(["app"]));
    assert_eq!(
        override_config["dockerComposeFile"],
        serde_json::json!(["compose.yaml", ".branchbox-sbx-compose.yaml"])
    );
    let sbx_compose =
        fs::read_to_string(worktree_path.join(".devcontainer/.branchbox-sbx-compose.yaml"))
            .unwrap();
    assert!(sbx_compose.contains("restart: unless-stopped"));
    assert!(sbx_compose.contains(&format!(
        "source: {}",
        fs::canonicalize(test_repo.path().join(".git"))
            .unwrap()
            .display()
    )));
    assert!(sbx_compose.contains("target: /workspaces/main/.git"));
    let compose = fs::read_to_string(worktree_path.join(".devcontainer/compose.yaml")).unwrap();
    assert!(compose.contains("postgres"));
    assert!(compose.contains("depends_on"));

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();
}

#[cfg(unix)]
#[test]
fn sbx_materializes_required_cloudflared_env_before_runtime_creation() {
    const TOKEN: &str = "branchbox-tunnel-order-sentinel";
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    fs::write(
        test_repo.path().join(".devcontainer/docker-compose.yml"),
        r#"services:
  dev:
    image: alpine:3.19
    depends_on:
      - cloudflared
  cloudflared:
    image: cloudflare/cloudflared:latest
    env_file:
      - .cloudflared.env
"#,
    )
    .expect("write Compose fixture");
    run_git(test_repo.path(), &["add", ".devcontainer"]);
    run_git(
        test_repo.path(),
        &["commit", "-m", "Add required cloudflared fixture"],
    );

    let work_feature = "sbx-tunnel-order";
    let worktree_path = test_repo.worktree_parent().join(work_feature);
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();
    let output = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_REQUIRE_CLOUDFLARED_ENV" => "1",
        "CLOUDFLARE_TUNNEL_TOKEN" => TOKEN,
    )
    .args([
        "feature",
        "start",
        work_feature,
        "--runtime",
        "sbx",
        "--json",
    ])
    .output()
    .expect("start SBX feature with required cloudflared env");

    assert!(
        output.status.success(),
        "SBX feature start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains(TOKEN), "tunnel token leaked: {combined}");
    assert_eq!(
        fs::read_to_string(worktree_path.join(".devcontainer/.cloudflared.env"))
            .expect("read prepared tunnel env"),
        format!("TUNNEL_TOKEN={TOKEN}\nDEV_HOSTNAME=dev-sbx-tunnel-order.example.com\n")
    );

    branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args(["feature", "teardown", work_feature, "--force"])
    .assert()
    .success();
}

#[cfg(unix)]
#[test]
fn sbx_missing_required_cloudflare_credentials_fails_before_runtime_creation() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    fs::write(
        test_repo
            .path()
            .join(".devcontainer/docker-compose.yml"),
        "services:\n  dev:\n    image: alpine:3.19\n  cloudflared:\n    image: cloudflare/cloudflared:latest\n    env_file:\n      - .cloudflared.env\n",
    )
    .expect("write Compose fixture");
    run_git(test_repo.path(), &["add", ".devcontainer"]);
    run_git(
        test_repo.path(),
        &["commit", "-m", "Add required cloudflared fixture"],
    );
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    let output = branchbox_cmd!(
        test_repo.path(),
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
    )
    .args([
        "feature",
        "start",
        "sbx-tunnel-no-credentials",
        "--runtime",
        "sbx",
        "--json",
    ])
    .output()
    .expect("run SBX tunnel credential preflight");

    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("Cloudflare credentials are not configured"));
    assert!(combined.contains("no sandbox was created"));
    assert!(!fake_state.exists());
    let calls = fs::read_to_string(fake_log).expect("read fake SBX calls");
    assert!(
        !calls.contains("create shell"),
        "runtime was created: {calls}"
    );
}

#[cfg(unix)]
#[test]
fn sbx_start_failure_never_exposes_compose_secrets_through_cli_json_mode() {
    const SENTINEL: &str = "branchbox-cli-sentinel-secret-83d1";
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let repo_path = test_repo.path();
    let (_fake_temp, fake_sbx, fake_state, fake_log) = create_fake_sbx();

    let output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_SBX_PATH" => &fake_sbx,
        "FAKE_SBX_STATE" => &fake_state,
        "FAKE_SBX_LOG" => &fake_log,
        "FAKE_SBX_START_FAILURE" => "1",
    )
    .args([
        "feature",
        "start",
        "sbx-redaction",
        "--runtime",
        "sbx",
        "--json",
    ])
    .output()
    .expect("run failing SBX feature start");

    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains(SENTINEL), "secret leaked: {combined}");
    assert!(!combined.contains("ARBITRARY_CREDENTIAL:"));
    assert!(combined.contains("exit status 42"));
    assert!(combined.contains("docker compose up failed"));
}

#[test]
fn keep_runtime_on_failure_rejects_non_sbx_runtime_before_creating_a_worktree() {
    let test_repo = init_test_repo();
    let work_feature = "container-retain-invalid";

    let output = branchbox_cmd!(test_repo.path())
        .args([
            "feature",
            "start",
            work_feature,
            "--runtime",
            "container",
            "--keep-runtime-on-failure",
        ])
        .output()
        .expect("reject keep-runtime-on-failure for container runtime");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only supported with the SBX runtime"));
    assert!(!test_repo.worktree_parent().join(work_feature).exists());
}

#[test]
fn feature_start_minimal_mode_default_prompt_seed() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "minimal-prompt-seed";

    let output = branchbox_cmd!(repo_path)
        .args([
            "feature",
            "start",
            work_feature,
            "--minimal",
            "--json",
            "--default-prompt",
        ])
        .output()
        .expect("run feature start --minimal --default-prompt");

    assert!(
        output.status.success(),
        "expected feature start to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: Value =
        serde_json::from_slice(&output.stdout).expect("parse feature start JSON summary");
    let prompt_seed = summary["prompt_seed"].as_str().expect("prompt_seed string");

    assert!(
        prompt_seed.contains("minimal mode"),
        "default prompt should mention minimal mode"
    );

    branchbox_cmd!(repo_path)
        .args([
            "feature",
            "teardown",
            work_feature,
            "--delete-branch",
            "--force",
        ])
        .assert()
        .success();
}

#[test]
fn feature_start_rejects_default_prompt_without_minimal() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    branchbox_cmd!(repo_path)
        .args([
            "feature",
            "start",
            "invalid-default-prompt",
            "--default-prompt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--default-prompt can only be used with --minimal",
        ));
}

#[test]
fn feature_start_launches_default_agent_after_devcontainer() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let repo_path = test_repo.path();
    let work_feature = "agent-ready";
    let worktree_path = test_repo.worktree_parent().join(work_feature);

    let output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_DEFAULT_AGENT_CMD" => "true",
        "BRANCHBOX_DEFAULT_AGENT_NAME" => "test agent"
    )
    .args(["feature", "start", work_feature])
    .output()
    .expect("run feature start with default agent");

    assert!(
        output.status.success(),
        "expected feature start to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Will launch test agent"));
    assert!(stdout.contains("🤖 Launching test agent"));
    assert!(stdout.contains("✅ Agent session completed successfully."));
    assert!(
        worktree_path.exists(),
        "expected worktree directory to exist"
    );

    let list_output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_DEFAULT_AGENT_CMD" => "true",
        "BRANCHBOX_DEFAULT_AGENT_NAME" => "test agent"
    )
    .args(["feature", "list", "--json"])
    .output()
    .expect("feature list --json with default agent cmd");

    assert!(list_output.status.success(), "feature list should succeed");
    let list: Value = serde_json::from_slice(&list_output.stdout).expect("parse feature list json");
    let features = list.as_array().expect("feature list array");
    assert!(!features.is_empty(), "list should include started feature");
    assert_eq!(
        features[0]["default_agent"]["status"],
        Value::String("ready".into())
    );

    branchbox_cmd!(repo_path)
        .args([
            "feature",
            "teardown",
            work_feature,
            "--delete-branch",
            "--force",
        ])
        .assert()
        .success();
}

#[test]
fn feature_start_blocks_default_agent_when_devcontainer_fails() {
    let test_repo = init_test_repo();
    test_repo.with_invalid_devcontainer();
    let repo_path = test_repo.path();

    let output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_DEFAULT_AGENT_CMD" => "true",
        "BRANCHBOX_DEFAULT_AGENT_NAME" => "test agent"
    )
    .args(["feature", "start", "agent-blocked"])
    .output()
    .expect("run feature start with invalid devcontainer");

    assert!(
        output.status.success(),
        "expected feature start to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Devcontainer failed; fix provisioning before auto-launching"));
    assert!(stdout
        .contains("Skipping default coding agent launch because the devcontainer module failed."));
}

#[test]
fn feature_start_json_mode_skips_default_agent_launch() {
    let test_repo = init_test_repo();
    test_repo.with_valid_devcontainer();
    let repo_path = test_repo.path();

    let output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_DEFAULT_AGENT_CMD" => "false"
    )
    .args(["feature", "start", "agent-json", "--json"])
    .output()
    .expect("run feature start --json with default agent cmd");

    assert!(output.status.success(), "feature start should succeed");

    let summary: Value = serde_json::from_slice(&output.stdout).expect("parse start summary json");
    assert_eq!(summary["work_feature"], Value::String("agent-json".into()));
    assert_eq!(
        summary["default_agent"]["status"],
        Value::String("ready".into())
    );
}

#[test]
fn feature_start_minimal_defers_default_agent_launch() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();

    let output = branchbox_cmd!(
        repo_path,
        "BRANCHBOX_DEFAULT_AGENT_CMD" => "true"
    )
    .args([
        "feature",
        "start",
        "agent-waits",
        "--minimal",
        "--default-prompt",
    ])
    .output()
    .expect("run feature start minimal with default agent");

    assert!(
        output.status.success(),
        "expected feature start to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout
        .contains("Devcontainer skipped (minimal mode); run `branchbox devcontainer sync` first"));
    assert!(stdout.contains(
        "Default coding agent launch skipped (devcontainer not provisioned yet). Run `branchbox devcontainer sync` first."
    ));
}

#[test]
fn features_alias_accepts_plural_subcommand() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let work_feature = "features-alias";
    let worktree_path = test_repo.worktree_parent().join(work_feature);

    branchbox_cmd!(repo_path)
        .args(["feature", "start", work_feature])
        .assert()
        .success();

    branchbox_cmd!(repo_path)
        .args(["features", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(work_feature));

    branchbox_cmd!(repo_path)
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
        "expected worktree to be removed during teardown"
    );
}

#[test]
fn prune_command_tears_down_all_active_features() {
    let test_repo = init_test_repo();
    let repo_path = test_repo.path();
    let feature_one = "prune-one";
    let feature_two = "prune-two";
    let worktree_one = test_repo.worktree_parent().join(feature_one);
    let worktree_two = test_repo.worktree_parent().join(feature_two);

    branchbox_cmd!(repo_path)
        .args(["feature", "start", feature_one])
        .assert()
        .success();

    branchbox_cmd!(repo_path)
        .args(["feature", "start", feature_two])
        .assert()
        .success();

    branchbox_cmd!(repo_path)
        .args(["prune", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pruned 2 feature(s)."));

    assert!(
        !worktree_one.exists() && !worktree_two.exists(),
        "expected both worktrees to be removed by prune"
    );

    let output = branchbox_cmd!(repo_path)
        .args(["feature", "list", "--status", "active", "--json"])
        .output()
        .expect("feature list active --json");
    assert!(
        output.status.success(),
        "expected feature list active --json to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value =
        serde_json::from_slice(&output.stdout).expect("parse active feature list JSON");
    let active = payload
        .as_array()
        .expect("active list should serialize as array");
    assert!(active.is_empty(), "expected no active features after prune");
}
