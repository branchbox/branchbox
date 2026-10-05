//! A scoped ownership probe fixture for success receipts that require an available empty engine.
//! It rejects mutations and unrelated Docker commands; unavailable-engine behavior has its own tests.

use assert_cmd::Command;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub struct EmptyDocker {
    _temp: TempDir,
    binary: PathBuf,
    probes: PathBuf,
}

impl EmptyDocker {
    pub fn new() -> Self {
        let temp = TempDir::new().expect("empty Docker fixture directory");
        let binary = temp.path().join(if cfg!(windows) {
            "docker.cmd"
        } else {
            "docker"
        });
        let probes = temp.path().join("ownership-probes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(
                &binary,
                r#"#!/bin/sh
set -eu
if [ "$#" -eq 5 ] && [ "$1" = ps ] && [ "$2" = -q ] && [ "$3" = -a ] && [ "$4" = --filter ]; then
  case "$5" in
    label=devcontainer.local_folder=*) printf '%s\n' "$5" >> "$EMPTY_DOCKER_PROBES"; exit 0 ;;
  esac
fi
printf '%s\n' rejected >> "$EMPTY_DOCKER_PROBES"
printf '%s\n' 'empty Docker fixture rejects mutations and unrelated commands' >&2
exit 90
"#,
            )
            .expect("write empty Docker executable");
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        }
        #[cfg(windows)]
        fs::write(&binary, "@echo off\r\nif not \"%~1\"==\"ps\" goto reject\r\nif not \"%~2\"==\"-q\" goto reject\r\nif not \"%~3\"==\"-a\" goto reject\r\nif not \"%~4\"==\"--filter\" goto reject\r\nif not \"%~6\"==\"\" goto reject\r\nset \"filter=%~5\"\r\nif not \"%filter:~0,32%\"==\"label=devcontainer.local_folder=\" goto reject\r\necho %filter%>>\"%EMPTY_DOCKER_PROBES%\"\r\nexit /b 0\r\n:reject\r\necho rejected>>\"%EMPTY_DOCKER_PROBES%\"\r\nexit /b 90\r\n").expect("write empty Docker executable");
        Self {
            _temp: temp,
            binary,
            probes,
        }
    }

    /// Apply to a single explicit command, leaving all other test commands and host state alone.
    pub fn command(&self, mut command: Command) -> Command {
        let mut paths = vec![self.binary.parent().unwrap().to_path_buf()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        command
            .env("DOCKER_PATH", &self.binary)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("EMPTY_DOCKER_PROBES", &self.probes);
        command
    }

    pub fn assert_probed(&self, workspaces: &[&Path]) {
        let observed: Vec<_> = fs::read_to_string(&self.probes)
            .expect("ownership probes occurred")
            .lines()
            .map(ToOwned::to_owned)
            .collect();
        let labels = |workspace: &Path| {
            let mut paths = BTreeSet::from([workspace.to_path_buf()]);
            // The worktree is gone after success; its surviving parent still resolves its exact label.
            if let (Some(parent), Some(name)) = (workspace.parent(), workspace.file_name()) {
                paths.insert(parent.canonicalize().unwrap().join(name));
            }
            paths
                .into_iter()
                .map(|path| format!("label=devcontainer.local_folder={}", path.display()))
                .collect::<BTreeSet<_>>()
        };
        let allowed: BTreeSet<_> = workspaces.iter().flat_map(|path| labels(path)).collect();
        assert!(
            observed.iter().all(|label| allowed.contains(label)),
            "unexpected ownership scope: {observed:?}"
        );
        for workspace in workspaces {
            let expected = labels(workspace);
            assert!(
                observed.iter().any(|label| expected.contains(label)),
                "ownership discovery must probe {}: {observed:?}",
                workspace.display()
            );
        }
        for label in observed.iter().collect::<BTreeSet<_>>() {
            assert!(observed.iter().filter(|observed| *observed == label).count() >= 2,
                "discovery and post-cleanup verification must repeat each observed label: {label}; {observed:?}");
        }
    }
}

impl Default for EmptyDocker {
    fn default() -> Self {
        Self::new()
    }
}
