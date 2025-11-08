use std::path::PathBuf;
use std::process::Command;

#[test]
fn devcontainer_compose_parses_with_docker() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("BRANCHBOX_DOCKER_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping docker-compose smoke test (set BRANCHBOX_DOCKER_TESTS=1 to run)");
        return Ok(());
    }

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core crate has repo parent")
        .to_path_buf();
    let compose_dir = repo_root.join(".devcontainer");

    let status = Command::new("docker")
        .args(["compose", "--env-file", ".branchbox.env", "config"])
        .current_dir(&compose_dir)
        .status();

    match status {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            panic!("docker binary not available while BRANCHBOX_DOCKER_TESTS=1: {e}");
        }
        Err(e) => return Err(Box::new(e)),
        Ok(status) => {
            assert!(status.success(), "docker compose config failed");
        }
    }

    Ok(())
}

#[test]
fn devcontainer_workspace_mount_exposes_repo() -> Result<(), Box<dyn std::error::Error>> {
    // Ensures the bind mount exposes the host repo in /workspaces/<repo_name>.
    if std::env::var("BRANCHBOX_DOCKER_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping workspace mount test (set BRANCHBOX_DOCKER_TESTS=1 to run)");
        return Ok(());
    }

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core crate has repo parent")
        .to_path_buf();
    let repo_name = repo_root
        .file_name()
        .expect("repo root has final component")
        .to_string_lossy()
        .into_owned();
    let compose_dir = repo_root.join(".devcontainer");
    let container_repo = format!("/workspaces/{repo_name}");

    let workspace_probe = r#"
set -euo pipefail
ls "$HOST_REPO" >/dev/null
test -f "$HOST_REPO/Cargo.toml"
test -d "$HOST_REPO/.git"
actual=$(git -C "$HOST_REPO" rev-parse --show-toplevel)
if [ "$actual" != "$HOST_REPO" ]; then
  echo "git toplevel mismatch: expected $HOST_REPO got $actual" >&2
  exit 1
fi
"#;

    let status = Command::new("docker")
        .args([
            "compose",
            "--env-file",
            ".branchbox.env",
            "run",
            "--rm",
            "-e",
            &format!("HOST_REPO={container_repo}"),
            "rust-dev",
            "bash",
            "-lc",
            workspace_probe,
        ])
        .current_dir(&compose_dir)
        .status();

    match status {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            panic!("docker binary not available while BRANCHBOX_DOCKER_TESTS=1: {e}");
        }
        Err(e) => return Err(Box::new(e)),
        Ok(status) => {
            assert!(status.success(), "workspace mount validation failed");
        }
    }

    Ok(())
}
