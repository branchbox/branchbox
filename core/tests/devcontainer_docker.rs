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
