//! Hermetic cleanup receipts and ownership regressions: no real Docker daemon or images are used.
#![cfg(unix)]

#[macro_use]
mod support;

use serde_json::Value;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct FakeDocker {
    _temp: TempDir,
    binary: PathBuf,
    inventory: PathBuf,
    log: PathBuf,
    removed: PathBuf,
    networks: PathBuf,
    volumes: PathBuf,
}

impl FakeDocker {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let binary = temp.path().join("docker");
        let inventory = temp.path().join("inventory");
        let log = temp.path().join("calls");
        let removed = temp.path().join("removed");
        let networks = temp.path().join("networks");
        let volumes = temp.path().join("volumes");
        fs::write(&inventory, "").unwrap();
        fs::write(&networks, "").unwrap();
        fs::write(&volumes, "").unwrap();
        fs::write(&binary, r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_DOCKER_LOG"
case "$1" in
  ps)
    if [ "${FAKE_DOCKER_FAILURE:-}" = discovery ] || { [ "${FAKE_DOCKER_FAILURE:-}" = postprobe ] && [ -f "$FAKE_DOCKER_REMOVED" ]; }; then
      printf '%s\n' 'inventory unavailable' >&2; exit 9
    fi
    if [ "${FAKE_DOCKER_FAILURE:-}" = module ] && printf '%s' "$*" | grep -q 'com.docker.compose.project'; then
      printf '%s\n' 'compose inventory unavailable' >&2; exit 9
    fi
    folder=''; project=''
    for arg in "$@"; do
      case "$arg" in
        label=devcontainer.local_folder=*) folder=${arg#label=devcontainer.local_folder=} ;;
        label=com.docker.compose.project=*) project=${arg#label=com.docker.compose.project=} ;;
      esac
    done
    if [ -n "$folder" ]; then
      if printf '%s' "$*" | grep -q '\.Label'; then
        awk -F '\t' -v folder="$folder" '$2 == folder && $3 != "" {print $3}' "$FAKE_DOCKER_INVENTORY"
      else
        awk -F '\t' -v folder="$folder" '$2 == folder {print $1}' "$FAKE_DOCKER_INVENTORY"
      fi
    elif [ -n "$project" ]; then
      awk -F '\t' -v project="$project" '$3 == project {print $1}' "$FAKE_DOCKER_INVENTORY"
    fi
    ;;
  inspect)
    if [ "${FAKE_DOCKER_FAILURE:-}" = ownership ]; then printf '%s\n' 'ownership unavailable' >&2; exit 9; fi
    for id in "$@"; do :; done
    awk -F '\t' -v id="$id" '$1 == id {print $3}' "$FAKE_DOCKER_INVENTORY"
    ;;
  rm)
    shift
    [ "${1:-}" != -f ] || shift
    if [ "${FAKE_DOCKER_FAILURE:-}" = remove ]; then printf '%s\n' 'remove denied' >&2; exit 9; fi
    touch "$FAKE_DOCKER_REMOVED"
    if [ "${FAKE_DOCKER_FAILURE:-}" != residue ]; then
      awk -F '\t' -v id="$1" '$1 != id' "$FAKE_DOCKER_INVENTORY" > "$FAKE_DOCKER_INVENTORY.tmp"
      mv "$FAKE_DOCKER_INVENTORY.tmp" "$FAKE_DOCKER_INVENTORY"
    fi
    ;;
  network|volume)
    kind=$1; operation=$2; shift 2
    if [ "$kind" = network ]; then inventory=$FAKE_DOCKER_NETWORKS; else inventory=$FAKE_DOCKER_VOLUMES; fi
    if [ "$operation" = ls ]; then
      project=''
      for arg in "$@"; do case "$arg" in label=com.docker.compose.project=*) project=${arg#label=com.docker.compose.project=} ;; esac; done
      awk -F '\t' -v project="$project" '$2 == project {print $1}' "$inventory"
    elif [ "$operation" = rm ]; then
      if [ "${FAKE_DOCKER_FAILURE:-}" = resources ]; then printf '%s\n' 'resource busy' >&2; exit 9; fi
      awk -F '\t' -v id="$1" '$1 != id' "$inventory" > "$inventory.tmp"
      mv "$inventory.tmp" "$inventory"
    else exit 7; fi
    ;;
  compose|version) exit 0 ;;
  *) printf '%s\n' "unexpected Docker operation: $*" >&2; exit 7 ;;
esac
"#).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _temp: temp,
            binary,
            inventory,
            log,
            removed,
            networks,
            volumes,
        }
    }

    fn command(&self, repo: &Path, failure: &str) -> assert_cmd::Command {
        let path = format!(
            "{}:{}",
            self.binary.parent().unwrap().display(),
            std::env::var("PATH").unwrap()
        );
        branchbox_cmd!(repo,
            "PATH" => path,
            "DOCKER_PATH" => &self.binary,
            "FAKE_DOCKER_INVENTORY" => &self.inventory,
            "FAKE_DOCKER_LOG" => &self.log,
            "FAKE_DOCKER_REMOVED" => &self.removed,
            "FAKE_DOCKER_FAILURE" => failure,
            "FAKE_DOCKER_NETWORKS" => &self.networks,
            "FAKE_DOCKER_VOLUMES" => &self.volumes,
        )
    }

    fn calls(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

fn setup_config(repo: &support::TestRepo, kind: &str, unused_compose: bool) {
    let dir = repo.path().join(".devcontainer");
    fs::create_dir_all(&dir).unwrap();
    let config = match kind {
        "image" => r#"{"image":"python:3.12-alpine"}"#,
        "dockerfile" => r#"{"build":{"dockerfile":"Dockerfile"}}"#,
        _ => panic!("unsupported fixture kind"),
    };
    fs::write(dir.join("devcontainer.json"), config).unwrap();
    if kind == "dockerfile" {
        fs::write(dir.join("Dockerfile"), "FROM python:3.12-alpine\n").unwrap();
    }
    if unused_compose {
        fs::write(
            dir.join("compose.yaml"),
            "services:\n  unused:\n    image: python:3.12-alpine\n",
        )
        .unwrap();
    }
    repo.git(&["add", ".devcontainer"]);
    repo.git(&["commit", "-q", "-m", "devcontainer fixture"]);
}

fn start(repo: &support::TestRepo, docker: &FakeDocker) -> PathBuf {
    docker
        .command(repo.path(), "")
        .args(["feature", "start", "owned", "--minimal", "--json"])
        .assert()
        .success();
    let path = repo.root().join("owned");
    // The start writes generated env files but never starts a container in this fixture.
    fs::write(&docker.log, "").unwrap();
    path
}

fn teardown(
    repo: &support::TestRepo,
    docker: &FakeDocker,
    failure: &str,
    force: bool,
) -> std::process::Output {
    let mut cmd = docker.command(repo.path(), failure);
    cmd.args([
        "feature",
        "teardown",
        "owned",
        "--keep-branch",
        "--json",
        "--repo",
    ]);
    cmd.arg(repo.path());
    if force {
        cmd.arg("--force");
    }
    cmd.output().unwrap()
}

fn registry_status(repo: &support::TestRepo) -> String {
    let registry: Value =
        serde_json::from_slice(&fs::read(repo.path().join(".branchbox/registry.json")).unwrap())
            .unwrap();
    registry["features"][0]["status"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn image_and_dockerfile_teardown_remove_all_exact_workspace_matches() {
    for (kind, unused_compose) in [("image", false), ("image", true), ("dockerfile", false)] {
        let repo = support::init_test_repo();
        setup_config(&repo, kind, unused_compose);
        let docker = FakeDocker::new();
        let worktree = start(&repo, &docker);
        let canonical = worktree.canonicalize().unwrap();
        fs::write(
            &docker.inventory,
            format!(
                "owned-a\t{}\nowned-b\t{}\nneighbor\t{}\n",
                worktree.display(),
                canonical.display(),
                repo.root().join("neighbor").display()
            ),
        )
        .unwrap();
        let output = teardown(&repo, &docker, "", false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let payload = support::assert_single_json(&output);
        assert_eq!(payload["runtime_teardown"]["verified"], true);
        assert_eq!(payload["runtime_teardown"]["residue_free"], true);
        assert!(!worktree.exists());
        assert_eq!(registry_status(&repo), "removed");
        let inventory = fs::read_to_string(&docker.inventory).unwrap();
        assert!(
            inventory.starts_with("neighbor\t"),
            "inventory={inventory:?}; calls={}",
            docker.calls()
        );
        assert_eq!(inventory.lines().count(), 1);
        assert!(!docker.calls().lines().any(|line| line == "rm -f neighbor"));
        assert!(
            docker
                .calls()
                .lines()
                .filter(|line| line.starts_with("ps "))
                .count()
                >= 2
        );
    }
}

#[test]
fn failed_discovery_removal_and_postprobe_keep_worktree_and_registry() {
    for failure in [
        "discovery",
        "ownership",
        "remove",
        "postprobe",
        "residue",
        "module",
    ] {
        let repo = support::init_test_repo();
        setup_config(&repo, "image", failure == "module");
        let docker = FakeDocker::new();
        let worktree = start(&repo, &docker);
        fs::write(
            &docker.inventory,
            format!("owned-a\t{}\n", worktree.canonicalize().unwrap().display()),
        )
        .unwrap();
        let before: Value = serde_json::from_slice(
            &fs::read(repo.path().join(".branchbox/registry.json")).unwrap(),
        )
        .unwrap();
        let output = teardown(&repo, &docker, failure, false);
        assert!(
            !output.status.success(),
            "cleanup failure {failure} must stop teardown"
        );
        let payload = support::assert_single_json(&output);
        assert_eq!(payload["error"]["code"], "teardown_refused");
        assert!(String::from_utf8_lossy(&output.stderr).contains("runtime cleanup"));
        assert!(worktree.join(".git").exists());
        assert_eq!(registry_status(&repo), "active");
        let after: Value = serde_json::from_slice(
            &fs::read(repo.path().join(".branchbox/registry.json")).unwrap(),
        )
        .unwrap();
        // Tunnel teardown can update its recorded state before runtime cleanup stops. The owning feature
        // must still retain its path, runtime identity and active status for a safe retry.
        for key in ["worktree_path", "branch_name", "runtime", "removed_at"] {
            assert_eq!(before["features"][0][key], after["features"][0][key]);
        }
        if failure == "discovery" {
            assert!(!docker.calls().lines().any(|line| line.starts_with("rm ")));
        }
    }
}

#[test]
fn forced_cleanup_failures_never_report_verified_clean() {
    for failure in [
        "discovery",
        "ownership",
        "remove",
        "postprobe",
        "residue",
        "module",
    ] {
        let repo = support::init_test_repo();
        setup_config(&repo, "image", failure == "module");
        let docker = FakeDocker::new();
        let worktree = start(&repo, &docker);
        fs::write(
            &docker.inventory,
            format!("owned-a\t{}\n", worktree.canonicalize().unwrap().display()),
        )
        .unwrap();
        let output = teardown(&repo, &docker, failure, true);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let payload = support::assert_single_json(&output);
        assert_eq!(payload["runtime_teardown"]["residue_free"], false);
        if failure != "residue" {
            assert_eq!(payload["runtime_teardown"]["verified"], false);
        }
        assert!(!payload["runtime_teardown"]["residue"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(!worktree.exists());
        assert_eq!(registry_status(&repo), "removed");
    }
}

#[test]
fn initial_user_work_refusal_precedes_every_docker_operation() {
    let repo = support::init_test_repo();
    setup_config(&repo, "image", false);
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    fs::write(worktree.join("user-work.txt"), "keep me").unwrap();
    let output = teardown(&repo, &docker, "", false);
    assert!(!output.status.success());
    assert_eq!(
        support::assert_single_json(&output)["error"]["code"],
        "teardown_refused"
    );
    assert!(docker.calls().is_empty());
    assert_eq!(
        fs::read_to_string(worktree.join("user-work.txt")).unwrap(),
        "keep me"
    );
    assert_eq!(registry_status(&repo), "active");
}

#[test]
fn bare_env_only_minimal_worktree_still_tears_down_without_docker() {
    let repo = support::init_test_repo();
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    assert!(worktree.join(".devcontainer/.branchbox.env").exists());
    assert!(!worktree.join(".devcontainer/devcontainer.json").exists());
    let output = teardown(&repo, &docker, "discovery", false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = support::assert_single_json(&output);
    assert_eq!(payload["runtime_teardown"]["verified"], false);
    assert_eq!(payload["runtime_teardown"]["residue_free"], false);
    assert!(!worktree.exists());
    assert_eq!(registry_status(&repo), "removed");
}

#[test]
fn observed_owned_resources_block_a_bare_worktree_on_cleanup_failure() {
    for failure in ["remove", "postprobe", "residue"] {
        let repo = support::init_test_repo();
        let docker = FakeDocker::new();
        let worktree = start(&repo, &docker);
        fs::write(
            &docker.inventory,
            format!("owned-a\t{}\n", worktree.canonicalize().unwrap().display()),
        )
        .unwrap();
        let output = teardown(&repo, &docker, failure, false);
        assert!(
            !output.status.success(),
            "observed runtime failure {failure} must keep even a bare worktree"
        );
        assert_eq!(
            support::assert_single_json(&output)["error"]["code"],
            "teardown_refused"
        );
        assert!(worktree.join(".git").exists());
        assert_eq!(registry_status(&repo), "active");
    }
}

#[test]
fn failed_compose_module_keeps_worktree_even_without_a_devcontainer_config() {
    let repo = support::init_test_repo();
    fs::create_dir(repo.path().join(".devcontainer")).unwrap();
    fs::write(
        repo.path().join(".devcontainer/compose.yaml"),
        "services:\n  app:\n    image: python:3.12-alpine\n",
    )
    .unwrap();
    repo.git(&["add", ".devcontainer"]);
    repo.git(&["commit", "-q", "-m", "compose-only fixture"]);
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    let output = teardown(&repo, &docker, "module", false);
    assert!(!output.status.success());
    assert_eq!(
        support::assert_single_json(&output)["error"]["code"],
        "teardown_refused"
    );
    assert!(worktree.join(".git").exists());
    assert_eq!(registry_status(&repo), "active");
}

#[test]
fn a_repository_only_symlink_does_not_authorize_other_sibling_containers() {
    let repo = support::init_test_repo();
    setup_config(&repo, "image", false);
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    let other = TempDir::new().unwrap();
    let repo_alias = other.path().join("main");
    symlink(repo.path(), &repo_alias).unwrap();
    let foreign_workspace = other.path().join("owned");
    fs::create_dir(&foreign_workspace).unwrap();
    fs::write(
        &docker.inventory,
        format!(
            "actual\t{}\nforeign\t{}\n",
            worktree.canonicalize().unwrap().display(),
            foreign_workspace.display()
        ),
    )
    .unwrap();
    let output = docker
        .command(&repo_alias, "")
        .args([
            "feature",
            "teardown",
            "owned",
            "--keep-branch",
            "--json",
            "--repo",
        ])
        .arg(&repo_alias)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        support::assert_single_json(&output)["runtime_teardown"]["residue_free"]
            .as_bool()
            .unwrap()
    );
    assert!(fs::read_to_string(&docker.inventory)
        .unwrap()
        .starts_with("foreign\t"));
    assert!(!docker.calls().contains("rm -f foreign"));
    assert!(foreign_workspace.exists());
}

fn compose_inventory(docker: &FakeDocker, workspace: &Path, neighbor: &Path) {
    fs::write(
        &docker.inventory,
        format!(
            "actual\t{}\tvsc-actual-worktree\nforeign\t{}\tvsc-neighbor\n",
            workspace.display(),
            neighbor.display(),
        ),
    )
    .unwrap();
    fs::write(
        &docker.networks,
        "actual-network\tvsc-actual-worktree\nforeign-network\tvsc-neighbor\n",
    )
    .unwrap();
    fs::write(
        &docker.volumes,
        "actual-volume\tvsc-actual-worktree\nforeign-volume\tvsc-neighbor\n",
    )
    .unwrap();
}

fn assert_only_neighbor_remains(docker: &FakeDocker) {
    assert!(fs::read_to_string(&docker.inventory)
        .unwrap()
        .starts_with("foreign\t"));
    assert_eq!(
        fs::read_to_string(&docker.inventory)
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(&docker.networks).unwrap(),
        "foreign-network\tvsc-neighbor\n"
    );
    assert_eq!(
        fs::read_to_string(&docker.volumes).unwrap(),
        "foreign-volume\tvsc-neighbor\n"
    );
    assert!(!docker.calls().contains("rm -f foreign"));
    assert!(!docker.calls().contains("network rm foreign-network"));
    assert!(!docker.calls().contains("volume rm foreign-volume"));
}

#[test]
fn compose_uses_validated_parent_alias_for_main_nested_and_linked_worktree_selection() {
    for selection in ["main", "nested-main", "linked-subfolder"] {
        let repo = support::init_test_repo();
        setup_config(&repo, "image", true);
        let docker = FakeDocker::new();
        let worktree = start(&repo, &docker);
        // The Compose module must discover the actual CLI project before the host hook can
        // remove its container. A separate main-only alias is covered by the negative test.
        let alias_temp = TempDir::new().unwrap();
        let alias_parent = alias_temp.path().join("parent-alias");
        symlink(repo.root(), &alias_parent).unwrap();
        let alias_worktree = alias_parent.join("owned");
        let repo_selector = match selection {
            "main" => alias_parent.join("main"),
            "nested-main" => {
                fs::create_dir(repo.path().join("sub")).unwrap();
                alias_parent.join("main/sub/..")
            }
            "linked-subfolder" => {
                docker
                    .command(repo.path(), "")
                    .args(["feature", "start", "selector", "--minimal", "--json"])
                    .assert()
                    .success();
                fs::create_dir(repo.root().join("selector/sub")).unwrap();
                alias_parent.join("selector/sub")
            }
            _ => unreachable!(),
        };
        compose_inventory(&docker, &alias_worktree, &alias_parent.join("neighbor"));
        fs::write(&docker.log, "").unwrap();
        let output = docker
            .command(&repo_selector, "")
            .args([
                "feature",
                "teardown",
                "owned",
                "--keep-branch",
                "--json",
                "--repo",
            ])
            .arg(&repo_selector)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "selection={selection}; stderr={}; calls={}",
            String::from_utf8_lossy(&output.stderr),
            docker.calls()
        );
        let payload = support::assert_single_json(&output);
        assert_eq!(payload["runtime_teardown"]["verified"], true);
        assert_eq!(payload["runtime_teardown"]["residue_free"], true);
        assert!(!worktree.exists());
        assert_only_neighbor_remains(&docker);
        assert!(docker
            .calls()
            .contains("--project-name vsc-actual-worktree"));
        assert!(docker.calls().contains("network rm actual-network"));
        assert!(docker.calls().contains("volume rm actual-volume"));
    }
}

#[test]
fn compose_identity_survives_container_removal_then_cleans_resources_on_retry() {
    let repo = support::init_test_repo();
    setup_config(&repo, "image", true);
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    compose_inventory(&docker, &worktree, &repo.root().join("neighbor"));
    let env_path = worktree.join(".devcontainer/.branchbox.env");
    let before = fs::read_to_string(&env_path).unwrap();
    let mode = fs::metadata(&env_path).unwrap().permissions().mode() & 0o777;
    let first = teardown(&repo, &docker, "resources", false);
    assert!(!first.status.success());
    assert_eq!(
        support::assert_single_json(&first)["error"]["code"],
        "teardown_refused"
    );
    assert!(worktree.exists());
    assert_eq!(registry_status(&repo), "active");
    assert!(!fs::read_to_string(&docker.inventory)
        .unwrap()
        .contains("actual\t"));
    assert!(fs::read_to_string(&docker.networks)
        .unwrap()
        .contains("actual-network"));
    assert!(fs::read_to_string(&docker.volumes)
        .unwrap()
        .contains("actual-volume"));
    let retained = fs::read_to_string(&env_path).unwrap();
    assert!(
        retained.starts_with(&before),
        "unrelated env bytes must survive"
    );
    assert!(retained.lines().any(
        |line| line.starts_with("BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS=")
            && line.contains("vsc-actual-worktree")
    ));
    assert_eq!(
        fs::metadata(&env_path).unwrap().permissions().mode() & 0o777,
        mode
    );
    fs::write(&docker.log, "").unwrap();
    let retry = teardown(&repo, &docker, "", false);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let payload = support::assert_single_json(&retry);
    assert_eq!(payload["runtime_teardown"]["verified"], true);
    assert_eq!(payload["runtime_teardown"]["residue_free"], true);
    assert!(docker
        .calls()
        .contains("--project-name vsc-actual-worktree"));
    assert_only_neighbor_remains(&docker);
    assert!(!worktree.exists());
}

#[test]
fn root_jsonc_compose_config_cleans_observed_groups_without_guessing_a_project() {
    let repo = support::init_test_repo();
    fs::write(repo.path().join(".devcontainer.json"), "{\n// active root configuration\n\"dockerComposeFile\":\"root-compose.yaml\",\"service\":\"app\",\"remoteEnv\":null,\n}\n").unwrap();
    fs::write(
        repo.path().join("root-compose.yaml"),
        "services:\n  app:\n    image: python:3.12-alpine\n",
    )
    .unwrap();
    repo.git(&["add", ".devcontainer.json", "root-compose.yaml"]);
    repo.git(&["commit", "-q", "-m", "root compose fixture"]);
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    compose_inventory(&docker, &worktree, &repo.root().join("neighbor"));
    let output = teardown(&repo, &docker, "", false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = support::assert_single_json(&output);
    assert_eq!(payload["runtime_teardown"]["verified"], true);
    assert_eq!(payload["runtime_teardown"]["residue_free"], true);
    assert!(!worktree.exists());
    assert_only_neighbor_remains(&docker);
    assert!(!docker
        .calls()
        .lines()
        .any(|line| line.starts_with("compose ")));
    assert!(docker
        .calls()
        .contains("label=com.docker.compose.project=vsc-actual-worktree"));
}

#[test]
fn retained_projects_block_false_clean_when_the_compose_module_is_no_longer_detected() {
    let repo = support::init_test_repo();
    setup_config(&repo, "image", true);
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    compose_inventory(&docker, &worktree, &repo.root().join("neighbor"));
    assert!(!teardown(&repo, &docker, "resources", false)
        .status
        .success());
    fs::remove_file(repo.path().join(".devcontainer/compose.yaml")).unwrap();
    fs::write(&docker.log, "").unwrap();
    let retry = teardown(&repo, &docker, "", false);
    assert!(!retry.status.success());
    assert_eq!(
        support::assert_single_json(&retry)["error"]["code"],
        "teardown_refused"
    );
    assert!(String::from_utf8_lossy(&retry.stderr).contains("vsc-actual-worktree"));
    assert!(worktree.exists());
    assert_eq!(registry_status(&repo), "active");
    assert!(!docker.calls().contains("network rm"));
    let forced = teardown(&repo, &docker, "", true);
    assert!(
        forced.status.success(),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    let payload = support::assert_single_json(&forced);
    assert_eq!(payload["runtime_teardown"]["verified"], false);
    assert_eq!(payload["runtime_teardown"]["residue_free"], false);
    assert!(fs::read_to_string(&docker.networks)
        .unwrap()
        .contains("actual-network"));
    assert!(fs::read_to_string(&docker.volumes)
        .unwrap()
        .contains("actual-volume"));
}

#[test]
fn missing_worktree_force_preserves_unhandled_compose_evidence_and_reports_unverified() {
    let repo = support::init_test_repo();
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    compose_inventory(
        &docker,
        &worktree.canonicalize().unwrap(),
        &repo.root().join("neighbor"),
    );
    fs::remove_dir_all(&worktree).unwrap();
    let output = teardown(&repo, &docker, "", true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = support::assert_single_json(&output);
    assert_eq!(payload["runtime_teardown"]["verified"], false);
    assert_eq!(payload["runtime_teardown"]["residue_free"], false);
    assert!(payload["runtime_teardown"]["residue"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "compose-project"));
    assert!(fs::read_to_string(&docker.inventory)
        .unwrap()
        .contains("actual\t"));
    assert!(!docker.calls().contains("rm -f actual"));
    assert!(fs::read_to_string(&docker.networks)
        .unwrap()
        .contains("actual-network"));
    assert!(fs::read_to_string(&docker.volumes)
        .unwrap()
        .contains("actual-volume"));
}

#[test]
fn cleanup_identity_copied_from_another_workspace_never_authorizes_project_cleanup() {
    let repo = support::init_test_repo();
    setup_config(&repo, "image", true);
    let docker = FakeDocker::new();
    let worktree = start(&repo, &docker);
    compose_inventory(&docker, &worktree, &repo.root().join("neighbor"));
    assert!(!teardown(&repo, &docker, "resources", false)
        .status
        .success());
    let history = fs::read(worktree.join(".devcontainer/.branchbox.env")).unwrap();
    docker
        .command(repo.path(), "")
        .args(["feature", "start", "other", "--minimal", "--json"])
        .assert()
        .success();
    let other = repo.root().join("other");
    fs::write(other.join(".devcontainer/.branchbox.env"), history).unwrap();
    fs::write(&docker.log, "").unwrap();
    let output = docker
        .command(repo.path(), "")
        .args([
            "feature",
            "teardown",
            "other",
            "--keep-branch",
            "--json",
            "--repo",
        ])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("another workspace"));
    assert!(other.exists());
    assert!(!docker.calls().contains("network rm"));
    assert!(!docker.calls().contains("volume rm"));
    assert!(!docker.calls().contains("compose --env-file"));
    assert!(fs::read_to_string(&docker.networks)
        .unwrap()
        .contains("actual-network"));
    assert!(fs::read_to_string(&docker.volumes)
        .unwrap()
        .contains("actual-volume"));
}
