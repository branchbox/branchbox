//! Exact workspace ownership cleanup for host devcontainers, including image/Dockerfile containers.

use super::{RuntimeMetadata, RuntimeProviderKind, RuntimeResidue, RuntimeTeardownReport};
use crate::{devcontainer_runtime::Docker, Result};
use std::collections::BTreeSet;
use std::path::Path;

fn workspace_paths(worktree_path: &Path) -> BTreeSet<String> {
    let mut paths = BTreeSet::from([worktree_path.to_string_lossy().into_owned()]);
    if let Ok(canonical) = worktree_path.canonicalize() {
        paths.insert(canonical.to_string_lossy().into_owned());
    } else if let (Some(parent), Some(name)) = (worktree_path.parent(), worktree_path.file_name()) {
        // Forced teardown can encounter an already missing worktree. Its verified parent alias
        // still identifies the exact canonical sibling label without resolving any other sibling.
        if let Ok(canonical_parent) = parent.canonicalize() {
            paths.insert(canonical_parent.join(name).to_string_lossy().into_owned());
        }
    }
    paths
}

fn owned_containers(docker: &Docker, paths: &BTreeSet<String>) -> (BTreeSet<String>, Vec<String>) {
    let mut ids = BTreeSet::new();
    let mut errors = Vec::new();
    for path in paths {
        match docker.find_containers(&[("devcontainer.local_folder", path.as_str())]) {
            Ok(found) => ids.extend(found),
            Err(err) => errors.push(format!("Cannot discover devcontainers for {path}: {err}")),
        }
    }
    (ids, errors)
}

/// Cleanup depends only on the stable workspace label: configuration may have changed, and a standalone
/// container has no Compose project label. Compose modules separately remove their networks and volumes.
pub(super) fn destroy(
    docker: &Docker,
    metadata: &RuntimeMetadata,
    worktree_path: &Path,
) -> Result<RuntimeTeardownReport> {
    let paths = workspace_paths(worktree_path);
    let mut compose_residue = Vec::new();
    match crate::modules::compose::retained_teardown_projects(worktree_path) {
        Ok(projects) if !projects.is_empty() => compose_residue.push(RuntimeResidue {
            kind: "compose-project".to_string(),
            identifiers: projects.into_iter().collect(),
        }),
        Ok(_) => {}
        Err(error) => compose_residue.push(RuntimeResidue {
            kind: "compose-ownership-error".to_string(),
            identifiers: vec![error.to_string()],
        }),
    }
    // Finish every ownership probe before removing anything; a partial discovery cannot authorize cleanup.
    let (owned, discovery_errors) = owned_containers(docker, &paths);
    if !discovery_errors.is_empty() {
        let mut residue = compose_residue;
        residue.push(RuntimeResidue {
            kind: "container-discovery-error".to_string(),
            identifiers: discovery_errors,
        });
        if !owned.is_empty() {
            residue.push(RuntimeResidue {
                kind: "container".to_string(),
                identifiers: owned.into_iter().collect(),
            });
        }
        return Ok(RuntimeTeardownReport {
            provider: RuntimeProviderKind::Container,
            runtime_id: metadata.runtime_id.clone(),
            verified: false,
            residue_free: false,
            residue,
        });
    }
    let mut compose_projects = BTreeSet::new();
    let mut inspection_errors = Vec::new();
    for id in &owned {
        match docker.container_compose_project(id) {
            Ok(Some(project)) => {
                compose_projects.insert(project);
            }
            Ok(None) => {}
            Err(error) => inspection_errors.push(error.to_string()),
        }
    }
    if !compose_projects.is_empty() {
        compose_residue.push(RuntimeResidue {
            kind: "compose-project".to_string(),
            identifiers: compose_projects.into_iter().collect(),
        });
    }
    if !inspection_errors.is_empty() {
        compose_residue.push(RuntimeResidue {
            kind: "container-ownership-error".to_string(),
            identifiers: inspection_errors,
        });
    }
    // A remaining Compose container is ownership evidence for its networks/volumes. Preserve
    // it for module cleanup/retry rather than deleting that evidence and claiming all resources gone.
    if !compose_residue.is_empty() {
        if !owned.is_empty() {
            compose_residue.push(RuntimeResidue {
                kind: "container".to_string(),
                identifiers: owned.into_iter().collect(),
            });
        }
        return Ok(RuntimeTeardownReport {
            provider: RuntimeProviderKind::Container,
            runtime_id: metadata.runtime_id.clone(),
            verified: false,
            residue_free: false,
            residue: compose_residue,
        });
    }
    let attempted = owned.clone();
    let mut errors = Vec::new();
    for id in owned {
        match docker.remove_container(&id, true) {
            Ok(output) if output.success => {}
            Ok(output) => errors.push(format!(
                "Cannot remove container {id}: {}",
                output.stderr.trim()
            )),
            Err(err) => errors.push(format!("Cannot remove container {id}: {err}")),
        }
    }
    let (remaining, verification_errors) = owned_containers(docker, &paths);
    let mut residue = Vec::new();
    if !remaining.is_empty() {
        residue.push(RuntimeResidue {
            kind: "container".to_string(),
            identifiers: remaining.into_iter().collect(),
        });
    }
    let verified = errors.is_empty() && verification_errors.is_empty();
    if !errors.is_empty() {
        residue.push(RuntimeResidue {
            kind: "container-removal-error".to_string(),
            identifiers: errors,
        });
    }
    if !verification_errors.is_empty() {
        residue.push(RuntimeResidue {
            kind: "container-verification-error".to_string(),
            identifiers: verification_errors,
        });
    }
    if !verified && !attempted.is_empty() {
        residue.push(RuntimeResidue {
            kind: "container-cleanup-attempted".to_string(),
            identifiers: attempted.into_iter().collect(),
        });
    }
    Ok(RuntimeTeardownReport {
        provider: RuntimeProviderKind::Container,
        runtime_id: metadata.runtime_id.clone(),
        verified,
        residue_free: verified && residue.is_empty(),
        residue,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use tempfile::TempDir;

    #[test]
    fn lexical_and_canonical_workspace_labels_are_deduplicated_and_neighbors_kept() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let lexical = temp.path().join("workspace-alias");
        symlink(&workspace, &lexical).unwrap();
        let inventory = temp.path().join("inventory");
        fs::write(
            &inventory,
            format!(
                "first\t{}\nsecond\t{}\nneighbor\t{}\n",
                lexical.display(),
                workspace.canonicalize().unwrap().display(),
                temp.path().join("neighbor").display()
            ),
        )
        .unwrap();
        let binary = temp.path().join("docker");
        fs::write(&binary, r#"#!/bin/sh
set -eu
fixture=$(dirname "$0")
printf '%s\n' "$*" >> "$fixture/calls"
case "$1" in
 ps)
   folder=''
   for arg in "$@"; do case "$arg" in label=devcontainer.local_folder=*) folder=${arg#label=devcontainer.local_folder=} ;; esac; done
   awk -F '\t' -v folder="$folder" '$2 == folder {print $1}' "$fixture/inventory"
   ;;
 rm)
   awk -F '\t' -v id="$3" '$1 != id' "$fixture/inventory" > "$fixture/next"
   mv "$fixture/next" "$fixture/inventory"
   ;;
 inspect) ;;
 *) exit 7 ;;
esac
"#).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let docker = Docker::with_paths(binary.to_string_lossy(), None);
        let report = destroy(&docker, &RuntimeMetadata::default(), &lexical).unwrap();
        assert!(report.verified && report.residue_free);
        assert_eq!(fs::read_to_string(&inventory).unwrap().lines().count(), 1);
        assert!(fs::read_to_string(&inventory)
            .unwrap()
            .starts_with("neighbor\t"));
        let calls = fs::read_to_string(temp.path().join("calls")).unwrap();
        assert_eq!(
            calls.lines().filter(|line| line.starts_with("rm ")).count(),
            2
        );
        assert_eq!(
            calls.lines().filter(|line| line.starts_with("ps ")).count(),
            4
        );
        assert!(!calls.contains("rm -f neighbor"));
    }
}
