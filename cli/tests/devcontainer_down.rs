//! Down ownership and failure receipts using a hermetic Docker executable; no daemon or images are used.
#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    workspace: PathBuf,
    docker: PathBuf,
    inventory: PathBuf,
    volumes: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new(compose: bool) -> Self {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        let config_dir = workspace.join(".devcontainer");
        fs::create_dir_all(&config_dir).unwrap();
        let config = if compose {
            r#"{"dockerComposeFile":"compose.yaml","service":"app"}"#
        } else {
            r#"{"image":"example.invalid/image:latest","remoteUser":"root"}"#
        };
        fs::write(config_dir.join("devcontainer.json"), config).unwrap();
        fs::write(
            config_dir.join("compose.yaml"),
            "services:\n  app:\n    image: example.invalid/app:latest\n",
        )
        .unwrap();
        let docker = temp.path().join("docker");
        let inventory = temp.path().join("inventory");
        let volumes = temp.path().join("volumes");
        let log = temp.path().join("calls");
        for name in ["inventory", "volumes", "networks"] {
            fs::write(temp.path().join(name), "").unwrap();
        }
        fs::write(&docker, r#"#!/bin/sh
set -eu
fixture=$(dirname "$0")
printf '%s\n' "$*" >> "$fixture/calls"
failure=${FAKE_FAILURE:-}
case "$1" in
  ps)
    if [ "$failure" = discovery ] || { [ "$failure" = postprobe ] && [ -f "$fixture/mutated" ]; }; then
      printf '%s\n' 'inventory unavailable' >&2; exit 9
    fi
    folder=''; config=''; project=''
    for arg in "$@"; do case "$arg" in
      label=devcontainer.local_folder=*) folder=${arg#label=devcontainer.local_folder=} ;;
      label=devcontainer.config_file=*) config=${arg#label=devcontainer.config_file=} ;;
      label=com.docker.compose.project=*) project=${arg#label=com.docker.compose.project=} ;;
    esac; done
    if [ -n "$project" ]; then
      awk -F '\t' -v project="$project" '$4 == project {print $1}' "$fixture/inventory"
    else
      awk -F '\t' -v folder="$folder" -v config="$config" '$2 == folder && $3 == config {print $1}' "$fixture/inventory"
    fi
    ;;
  inspect)
    if [ "$failure" = ownership ]; then printf '%s\n' 'ownership unavailable' >&2; exit 9; fi
    for id in "$@"; do :; done
    awk -F '\t' -v id="$id" '$1 == id {print $4}' "$fixture/inventory"
    ;;
  stop)
    if [ "$failure" = stop ]; then printf '%s\n' 'stop denied' >&2; exit 9; fi
    ;;
  rm)
    if [ "$failure" = remove ]; then printf '%s\n' 'remove denied' >&2; exit 9; fi
    delete_volumes=0
    for arg in "$@"; do [ "$arg" != -v ] || delete_volumes=1; id=$arg; done
    touch "$fixture/mutated"
    if [ "$failure" != residue ]; then
      awk -F '\t' -v id="$id" '$1 != id' "$fixture/inventory" > "$fixture/next"
      mv "$fixture/next" "$fixture/inventory"
      if [ "$delete_volumes" = 1 ]; then
        awk -F '\t' -v id="$id" '!($3 == id && $4 == "anonymous")' "$fixture/volumes" > "$fixture/next"
        mv "$fixture/next" "$fixture/volumes"
      fi
    fi
    ;;
  compose)
    project=''; previous=''; delete_volumes=0
    for arg in "$@"; do
      [ "$previous" != -p ] || project=$arg
      [ "$arg" != -v ] || delete_volumes=1
      previous=$arg
    done
    touch "$fixture/mutated"
    if [ "$failure" != residue ]; then
      awk -F '\t' -v project="$project" '$4 != project' "$fixture/inventory" > "$fixture/next"
      mv "$fixture/next" "$fixture/inventory"
      if [ "$failure" = compose ]; then printf '%s\n' 'partial compose failure' >&2; exit 9; fi
      awk -F '\t' -v project="$project" '$2 != project' "$fixture/networks" > "$fixture/next"
      mv "$fixture/next" "$fixture/networks"
      if [ "$delete_volumes" = 1 ]; then
        awk -F '\t' -v project="$project" '$2 != project' "$fixture/volumes" > "$fixture/next"
        mv "$fixture/next" "$fixture/volumes"
      fi
    fi
    ;;
  network|volume)
    kind=$1; project=''
    if [ "$failure" = "${kind}_probe" ]; then printf '%s\n' 'resource inventory unavailable' >&2; exit 9; fi
    for arg in "$@"; do case "$arg" in label=com.docker.compose.project=*) project=${arg#label=com.docker.compose.project=} ;; esac; done
    awk -F '\t' -v project="$project" '$2 == project {print $1}' "$fixture/${kind}s"
    ;;
  *) printf '%s\n' 'unexpected Docker operation' >&2; exit 90 ;;
esac
"#).unwrap();
        fs::set_permissions(&docker, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            _temp: temp,
            workspace,
            docker,
            inventory,
            volumes,
            log,
        }
    }

    fn owned_row(&self, id: &str, project: &str) -> String {
        let folder = self.workspace.canonicalize().unwrap();
        format!(
            "{id}\t{}\t{}\t{project}\n",
            folder.display(),
            folder.join(".devcontainer/devcontainer.json").display()
        )
    }

    fn compose_inventory(&self) {
        fs::write(&self.inventory, self.owned_row("owned", "vscode-actual") + "dependency\t\t\tvscode-actual\nneighbor\t/foreign\t/foreign/config\tneighbor-project\n").unwrap();
        fs::write(
            self._temp.path().join("networks"),
            "owned-network\tvscode-actual\nneighbor-network\tneighbor-project\n",
        )
        .unwrap();
        fs::write(&self.volumes, "owned-volume\tvscode-actual\t\tnamed\nshared-volume\t\t\tnamed\nneighbor-volume\tneighbor-project\t\tnamed\n").unwrap();
    }

    fn down(&self, workspace: &Path, volumes: bool, failure: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_branchbox"));
        command
            .args(["devcontainer", "down"])
            .arg(workspace)
            .arg("--docker-path")
            .arg(&self.docker)
            .arg("--json")
            .env("RUST_LOG", "off")
            .env("RUST_BACKTRACE", "0")
            .env("FAKE_FAILURE", failure);
        if volumes {
            command.arg("--volumes");
        }
        command.output().unwrap()
    }

    fn history(&self) -> String {
        fs::read_to_string(self.workspace.join(".devcontainer/.branchbox.env")).unwrap_or_default()
    }

    fn calls(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn failure(output: &Output, message: &str) {
    assert!(
        !output.status.success(),
        "unexpected successful receipt: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(text.contains(message), "{text}");
    assert!(!text.contains("\"outcome\":\"removed\"") && !text.contains("\"outcome\":\"stopped\""));
}

#[test]
fn standalone_down_removes_every_exact_match_and_only_explicit_anonymous_volumes() {
    for delete_volumes in [false, true] {
        let fixture = Fixture::new(false);
        fs::write(
            &fixture.inventory,
            fixture.owned_row("first", "")
                + &fixture.owned_row("second", "")
                + "neighbor\t/foreign\t/foreign/config\t\n"
                + &format!(
                    "other-config\t{}\t/other/config\t\n",
                    fixture.workspace.canonicalize().unwrap().display()
                ),
        )
        .unwrap();
        fs::write(&fixture.volumes, "anonymous-first\t\tfirst\tanonymous\nanonymous-second\t\tsecond\tanonymous\nshared-named\t\tfirst\tnamed\n").unwrap();
        let result = success(&fixture.down(&fixture.workspace, delete_volumes, ""));
        assert_eq!(result["outcome"], "removed");
        assert_eq!(
            result["removedContainers"],
            serde_json::json!(["first", "second"])
        );
        let remaining = fs::read_to_string(&fixture.inventory).unwrap();
        assert!(!remaining.contains("first") && !remaining.contains("second"));
        assert!(remaining.contains("neighbor") && remaining.contains("other-config"));
        let volumes = fs::read_to_string(&fixture.volumes).unwrap();
        assert!(volumes.contains("shared-named"));
        assert_eq!(volumes.contains("anonymous-first"), !delete_volumes);
        assert_eq!(fixture.calls().contains("rm -f -v"), delete_volumes);
        assert!(!fixture.calls().contains("volume rm"));
    }
}

#[test]
fn standalone_down_failure_never_emits_a_success_receipt() {
    for (mode, message) in [
        ("discovery", "Cannot discover"),
        ("ownership", "Cannot inspect"),
        ("stop", "Cannot stop"),
        ("remove", "Cannot remove"),
        ("residue", "Owned devcontainers remain"),
        ("postprobe", "Cannot verify devcontainer cleanup"),
    ] {
        let fixture = Fixture::new(false);
        fs::write(&fixture.inventory, fixture.owned_row("owned", "")).unwrap();
        failure(&fixture.down(&fixture.workspace, false, mode), message);
        if matches!(mode, "discovery" | "ownership" | "stop") {
            assert!(!fixture.calls().contains("rm "));
        }
    }
}

#[test]
fn lexical_and_canonical_workspace_labels_are_both_owned_without_duplicates() {
    for nested in [false, true] {
        let fixture = Fixture::new(false);
        let alias = fixture._temp.path().join("alias");
        symlink(&fixture.workspace, &alias).unwrap();
        fs::create_dir(fixture.workspace.join("sub")).unwrap();
        fs::write(
            &fixture.inventory,
            fixture.owned_row("canonical", "")
                + &format!(
                    "lexical\t{}\t{}\t\n",
                    alias.display(),
                    alias.join(".devcontainer/devcontainer.json").display()
                ),
        )
        .unwrap();
        let selector = if nested { alias.join("sub/..") } else { alias };
        let result = success(&fixture.down(&selector, false, ""));
        assert_eq!(
            result["removedContainers"],
            serde_json::json!(["canonical", "lexical"])
        );
        assert_eq!(
            fixture
                .calls()
                .lines()
                .filter(|line| line.starts_with("rm "))
                .count(),
            2
        );
    }
}

#[test]
fn compose_workspace_basename_does_not_authorize_foreign_project_cleanup() {
    let fixture = Fixture::new(true);
    let inventory = "foreign\t/foreign\t/foreign/config\tworkspace\n";
    fs::write(&fixture.inventory, inventory).unwrap();
    fs::write(&fixture.volumes, "foreign-volume\tworkspace\t\tnamed\n").unwrap();
    let result = success(&fixture.down(&fixture.workspace, true, ""));
    assert_eq!(result["outcome"], "not_found");
    assert_eq!(fs::read_to_string(&fixture.inventory).unwrap(), inventory);
    assert!(fs::read_to_string(&fixture.volumes)
        .unwrap()
        .contains("foreign-volume"));
    assert!(!fixture.calls().contains("compose ") && !fixture.calls().contains("rm "));
    assert!(!fixture
        .history()
        .contains("BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS"));
}

fn root_configuration(fixture: &Fixture) {
    let directory = fixture.workspace.join(".devcontainer");
    fs::rename(
        directory.join("devcontainer.json"),
        fixture.workspace.join(".devcontainer.json"),
    )
    .unwrap();
    fs::rename(
        directory.join("compose.yaml"),
        fixture.workspace.join("compose.yaml"),
    )
    .unwrap();
    fs::remove_dir(directory).unwrap();
    let folder = fixture.workspace.canonicalize().unwrap();
    fs::write(
        &fixture.inventory,
        format!(
            "owned\t{}\t{}\tvscode-root\n",
            folder.display(),
            folder.join(".devcontainer.json").display()
        ),
    )
    .unwrap();
}

#[test]
fn root_only_configuration_can_create_managed_history_and_keep_it_until_volume_deletion() {
    let fixture = Fixture::new(true);
    root_configuration(&fixture);
    assert!(!fixture.workspace.join(".devcontainer").exists());
    success(&fixture.down(&fixture.workspace, false, ""));
    assert!(fixture
        .history()
        .contains("BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS=vscode-root"));
    success(&fixture.down(&fixture.workspace, true, ""));
    assert!(!fixture
        .history()
        .contains("BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS"));
}

#[test]
fn managed_history_directory_symlink_cannot_redirect_stop_writes() {
    let fixture = Fixture::new(true);
    root_configuration(&fixture);
    let outside = fixture._temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let private = outside.join(".branchbox.env");
    fs::write(&private, "preserve-private-value\n").unwrap();
    symlink(&outside, fixture.workspace.join(".devcontainer")).unwrap();
    failure(
        &fixture.down(&fixture.workspace, false, ""),
        "symlinked devcontainer",
    );
    assert_eq!(
        fs::read_to_string(&private).unwrap(),
        "preserve-private-value\n"
    );
    assert!(!fixture.calls().contains("compose ") && !fixture.calls().contains("rm "));
}

#[test]
fn compose_stop_uses_actual_project_and_keeps_volume_identity_for_explicit_later_cleanup() {
    let fixture = Fixture::new(true);
    fixture.compose_inventory();
    assert_eq!(
        success(&fixture.down(&fixture.workspace, false, ""))["outcome"],
        "stopped"
    );
    assert!(fixture.calls().contains("-p vscode-actual down"));
    assert!(!fixture.calls().contains("-p workspace down"));
    assert!(fixture
        .history()
        .contains("BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS=vscode-actual"));
    assert!(fs::read_to_string(&fixture.volumes)
        .unwrap()
        .contains("owned-volume"));
    assert!(!fixture
        .calls()
        .lines()
        .any(|line| line.starts_with("volume ")));
    success(&fixture.down(&fixture.workspace, true, ""));
    assert!(!fixture
        .history()
        .contains("BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS"));
    let volumes = fs::read_to_string(&fixture.volumes).unwrap();
    assert!(!volumes.contains("owned-volume"));
    assert!(volumes.contains("shared-volume") && volumes.contains("neighbor-volume"));
    assert!(fs::read_to_string(&fixture.inventory)
        .unwrap()
        .contains("neighbor"));
}

#[test]
fn partial_compose_down_retains_actual_project_for_retry_after_containers_disappear() {
    let fixture = Fixture::new(true);
    fixture.compose_inventory();
    failure(
        &fixture.down(&fixture.workspace, true, "compose"),
        "partial compose failure",
    );
    assert!(!fs::read_to_string(&fixture.inventory)
        .unwrap()
        .contains("owned"));
    assert!(fixture.history().contains("vscode-actual"));
    success(&fixture.down(&fixture.workspace, true, ""));
    assert!(!fixture
        .history()
        .contains("BRANCHBOX_TEARDOWN_COMPOSE_PROJECTS"));
    assert!(!fs::read_to_string(&fixture.volumes)
        .unwrap()
        .contains("owned-volume"));
}

#[test]
fn compose_residue_and_unverifiable_resources_fail_and_preserve_history() {
    for (mode, message) in [
        ("residue", "still has"),
        ("network_probe", "Cannot verify network"),
        ("volume_probe", "Cannot verify volume"),
        ("postprobe", "Cannot verify devcontainer"),
    ] {
        let fixture = Fixture::new(true);
        fixture.compose_inventory();
        failure(&fixture.down(&fixture.workspace, true, mode), message);
        assert!(fixture.history().contains("vscode-actual"));
    }
}

#[test]
fn copied_compose_cleanup_history_cannot_authorize_another_workspace() {
    let source = Fixture::new(true);
    source.compose_inventory();
    success(&source.down(&source.workspace, false, ""));
    let other = Fixture::new(true);
    fs::write(
        other.workspace.join(".devcontainer/.branchbox.env"),
        source.history(),
    )
    .unwrap();
    failure(
        &other.down(&other.workspace, true, ""),
        "belongs to another workspace",
    );
    assert!(!other.calls().contains("compose ") && !other.calls().contains("rm "));
}
