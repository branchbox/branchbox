#[macro_use]
extern crate assert_cmd;

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread;
    use tempfile::TempDir;

    struct FakeComposeRuntime {
        _temp: TempDir,
        workspace: PathBuf,
        docker: PathBuf,
        log: PathBuf,
    }

    impl FakeComposeRuntime {
        fn new() -> Self {
            let temp = TempDir::new().expect("create fake Compose runtime");
            let workspace = temp.path().join("workspace");
            let devcontainer = workspace.join(".devcontainer");
            fs::create_dir_all(&devcontainer).expect("create devcontainer directory");
            fs::write(
                devcontainer.join("devcontainer.json"),
                r#"{
  "name": "Environment propagation",
  "dockerComposeFile": "compose.yaml",
  "service": "app",
  "workspaceFolder": "/workspaces/environment-propagation",
  "containerEnv": {"DB_HOST": "postgres"},
  "remoteEnv": {"CONFIG_REMOTE": "configured"}
}
"#,
            )
            .expect("write devcontainer config");
            fs::write(
                devcontainer.join("compose.yaml"),
                "services:\n  app:\n    image: example.invalid/app:latest\n",
            )
            .expect("write Compose config");

            let docker = temp.path().join("docker");
            fs::write(
                &docker,
                r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >>"$FAKE_DOCKER_LOG"
case "${1:-}" in
  version)
    exit 0
    ;;
  ps)
    if test -e "$FAKE_CONTAINER_STARTED"; then
      printf '%s\n' 'fake-container'
    fi
    ;;
  inspect)
    container_env_names_digest=''
    if test -e "$FAKE_CONTAINER_ENV_NAMES_DIGEST"; then
      container_env_names_digest=$(cat "$FAKE_CONTAINER_ENV_NAMES_DIGEST")
    fi
    if test -e "$FAKE_CONTAINER_ENV"; then
      db_host=$(cat "$FAKE_CONTAINER_ENV")
      printf '%s\n' "{\"Id\":\"fake-container\",\"Name\":\"/fake-container\",\"State\":{\"Status\":\"running\"},\"Config\":{\"Labels\":{\"devcontainer.branchbox.container_env_names_sha256\":\"$container_env_names_digest\"},\"Env\":[\"DB_HOST=$db_host\"]}}"
    else
      printf '%s\n' "{\"Id\":\"fake-container\",\"Name\":\"/fake-container\",\"State\":{\"Status\":\"running\"},\"Config\":{\"Labels\":{\"devcontainer.branchbox.container_env_names_sha256\":\"$container_env_names_digest\"},\"Env\":[]}}"
    fi
    ;;
  compose)
    shift
    all=" $* "
    case "$all" in
      *" up "*)
        previous=''
        for argument in "$@"; do
          if test "$previous" = '-f' && test -f "$argument"; then
            if grep -q 'DB_HOST' "$argument" && grep -q 'postgres' "$argument"; then
              printf '%s' 'postgres' >"$FAKE_CONTAINER_ENV"
            fi
            if grep -q 'devcontainer.branchbox.container_env_names_sha256' "$argument"; then
              grep 'devcontainer.branchbox.container_env_names_sha256' "$argument" | grep -Eo '[0-9a-f]{64}' | head -n 1 >"$FAKE_CONTAINER_ENV_NAMES_DIGEST"
            fi
          fi
          previous="$argument"
        done
        : >"$FAKE_CONTAINER_STARTED"
        ;;
      *" ps -q "*)
        if test -e "$FAKE_CONTAINER_STARTED"; then
          printf '%s\n' 'fake-container'
        fi
        ;;
      *" exec "*)
        db_host=''
        config_remote=''
        marker=''
        capture_env=0
        for argument in "$@"; do
          if test "$capture_env" = '1'; then
            case "$argument" in
              DB_HOST=*) db_host=${argument#DB_HOST=} ;;
              CONFIG_REMOTE=*) config_remote=${argument#CONFIG_REMOTE=} ;;
              EXEC_MARKER=*) marker=${argument#EXEC_MARKER=} ;;
            esac
            capture_env=0
            continue
          fi
          case "$argument" in
            -e|--env) capture_env=1 ;;
          esac
        done
        case "$all" in
          *" printenv DB_HOST "*)
            if test -n "$db_host"; then
              printf '%s\n' "$db_host"
            elif test -e "$FAKE_CONTAINER_ENV"; then
              cat "$FAKE_CONTAINER_ENV"
              printf '\n'
            fi
            ;;
          *" cargo test "*)
            sleep 0.2
            printf 'cargo:%s\n' "$marker"
            ;;
          *" python3 -m pytest "*)
            sleep 0.1
            printf 'python:%s\n' "$marker"
            ;;
          *" env "*)
            test -z "$db_host" || printf 'DB_HOST=%s\n' "$db_host"
            test -z "$config_remote" || printf 'CONFIG_REMOTE=%s\n' "$config_remote"
            test -z "$marker" || printf 'EXEC_MARKER=%s\n' "$marker"
            ;;
          *)
            printf '%s\n' 'unexpected fake Compose exec command' >&2
            exit 91
            ;;
        esac
        ;;
      *)
        printf '%s\n' "unexpected fake Compose command: $all" >&2
        exit 92
        ;;
    esac
    ;;
  *)
    printf '%s\n' "unexpected fake Docker command: $*" >&2
    exit 93
    ;;
esac
"#,
            )
            .expect("write fake Docker executable");
            let mut permissions = fs::metadata(&docker).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&docker, permissions).expect("make fake Docker executable");

            Self {
                workspace,
                docker,
                log: temp.path().join("docker.log"),
                _temp: temp,
            }
        }

        fn command(&self) -> Command {
            let mut command = Command::new(cargo_bin!("branchbox"));
            command
                .env("BRANCHBOX_SKIP_HOST_VALIDATION", "1")
                .env("RUST_LOG", "trace")
                .env("FAKE_DOCKER_LOG", &self.log)
                .env(
                    "FAKE_CONTAINER_STARTED",
                    self.workspace.join("container-started"),
                )
                .env(
                    "FAKE_CONTAINER_ENV",
                    self.workspace.join("container-environment"),
                )
                .env(
                    "FAKE_CONTAINER_ENV_NAMES_DIGEST",
                    self.workspace.join("container-environment-names-digest"),
                );
            command
        }

        fn up(&self) -> std::process::Output {
            self.command()
                .args([
                    "devcontainer",
                    "up",
                    self.workspace.to_str().unwrap(),
                    "--docker-path",
                    self.docker.to_str().unwrap(),
                    "--skip-post-create",
                    "--json",
                ])
                .output()
                .expect("run devcontainer up")
        }

        fn exec_command(&self, remote_env: &[&str], command: &[&str]) -> Command {
            let mut invocation = self.command();
            invocation.args([
                "devcontainer",
                "exec",
                "--workspace-folder",
                self.workspace.to_str().unwrap(),
                "--docker-path",
                self.docker.to_str().unwrap(),
            ]);
            for value in remote_env {
                invocation.args(["--remote-env", value]);
            }
            invocation.arg("--").args(command);
            invocation
        }

        fn exec(&self, remote_env: &[&str], command: &[&str]) -> std::process::Output {
            self.exec_command(remote_env, command)
                .output()
                .expect("run devcontainer exec")
        }

        fn log(&self) -> String {
            fs::read_to_string(&self.log).unwrap_or_default()
        }
    }

    fn assert_success(output: &std::process::Output, operation: &str) {
        assert!(
            output.status.success(),
            "{operation} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn compose_container_env_is_applied_at_up_and_visible_to_exec() {
        let runtime = FakeComposeRuntime::new();
        let up = runtime.up();
        assert_success(&up, "devcontainer up");

        let exec = runtime.exec(&[], &["printenv", "DB_HOST"]);
        assert_success(&exec, "devcontainer exec");
        assert_eq!(String::from_utf8_lossy(&exec.stdout), "postgres\n");
        assert_success(&runtime.up(), "unchanged existing devcontainer up");

        let changed_runtime = FakeComposeRuntime::new();
        let changed_config_path = changed_runtime
            .workspace
            .join(".devcontainer/devcontainer.json");
        let changed_config = fs::read_to_string(&changed_config_path)
            .expect("read comparison devcontainer config")
            .replace("\"DB_HOST\": \"postgres\"", "\"DB_HOST\": \"changed\"");
        fs::write(changed_config_path, changed_config)
            .expect("change comparison containerEnv value");
        assert_success(
            &changed_runtime.up(),
            "comparison devcontainer up with same environment names",
        );
        let original_binding =
            fs::read_to_string(runtime.workspace.join("container-environment-names-digest"))
                .expect("read original public environment binding");
        let changed_binding = fs::read_to_string(
            changed_runtime
                .workspace
                .join("container-environment-names-digest"),
        )
        .expect("read changed public environment binding");
        assert_eq!(
            original_binding, changed_binding,
            "public container label must bind names only, never values"
        );

        let config_path = runtime.workspace.join(".devcontainer/devcontainer.json");
        let original_config = fs::read_to_string(&config_path).expect("read devcontainer config");
        let changed =
            original_config.replace("\"DB_HOST\": \"postgres\"", "\"DB_HOST\": \"changed\"");
        fs::write(&config_path, changed).expect("change static containerEnv");
        let stale = runtime.up();
        assert!(
            !stale.status.success(),
            "existing container silently accepted changed containerEnv"
        );
        let stderr = String::from_utf8_lossy(&stale.stderr);
        assert!(stderr.contains("--remove-existing-container"));
        assert!(!stderr.contains("changed"), "containerEnv value leaked");

        let added_name = original_config.replace(
            "\"DB_HOST\": \"postgres\"",
            "\"DB_HOST\": \"postgres\", \"CACHE_HOST\": \"redis\"",
        );
        fs::write(&config_path, added_name).expect("add static containerEnv name");
        let added_name_stale = runtime.up();
        assert!(
            !added_name_stale.status.success(),
            "existing container silently accepted an added containerEnv name"
        );
        assert!(String::from_utf8_lossy(&added_name_stale.stderr)
            .contains("--remove-existing-container"));

        let removed_name = original_config.replace(
            "\"containerEnv\": {\"DB_HOST\": \"postgres\"}",
            "\"containerEnv\": {}",
        );
        fs::write(&config_path, removed_name).expect("remove static containerEnv name");
        let removed_name_stale = runtime.up();
        assert!(
            !removed_name_stale.status.success(),
            "existing container silently accepted a removed containerEnv name"
        );
        assert!(String::from_utf8_lossy(&removed_name_stale.stderr)
            .contains("--remove-existing-container"));
        assert!(
            !runtime
                .log()
                .contains("--filter label=devcontainer.branchbox.container_env_names_sha256="),
            "mutable environment binding participated in container discovery"
        );
    }

    #[test]
    fn compose_exec_merges_config_and_explicit_remote_env_without_host_inheritance_or_logs() {
        let runtime = FakeComposeRuntime::new();
        assert_success(&runtime.up(), "devcontainer up");

        let value = "per-command-sensitive-value";
        let mut command = runtime.command();
        command
            .env("HOST_ONLY_SHOULD_NOT_LEAK", "ambient-host-value")
            .args([
                "devcontainer",
                "exec",
                "--workspace-folder",
                runtime.workspace.to_str().unwrap(),
                "--docker-path",
                runtime.docker.to_str().unwrap(),
                "--remote-env",
                &format!("DB_HOST={value}"),
                "--",
                "env",
            ]);
        let exec = command.output().expect("run explicit remote-env exec");
        assert_success(&exec, "devcontainer exec --remote-env");
        let stdout = String::from_utf8_lossy(&exec.stdout);
        assert!(stdout.contains(&format!("DB_HOST={value}\n")));
        assert!(stdout.contains("CONFIG_REMOTE=configured\n"));
        assert!(!runtime.log().contains("HOST_ONLY_SHOULD_NOT_LEAK="));
        assert!(
            !String::from_utf8_lossy(&exec.stderr).contains(value),
            "remote environment value leaked to BranchBox diagnostics"
        );
    }

    #[test]
    fn concurrent_compose_execs_keep_commands_and_remote_env_isolated() {
        let runtime = FakeComposeRuntime::new();
        assert_success(&runtime.up(), "devcontainer up");
        let mut cargo =
            runtime.exec_command(&["EXEC_MARKER=cargo-one"], &["cargo", "test", "alpha"]);
        let mut python = runtime.exec_command(
            &["EXEC_MARKER=python-two"],
            &["python3", "-m", "pytest", "beta"],
        );

        let cargo_thread =
            thread::spawn(move || cargo.output().expect("run concurrent cargo exec"));
        let python_thread =
            thread::spawn(move || python.output().expect("run concurrent Python exec"));
        let cargo = cargo_thread.join().expect("join cargo exec");
        let python = python_thread.join().expect("join Python exec");
        assert_success(&cargo, "concurrent cargo exec");
        assert_success(&python, "concurrent Python exec");
        assert_eq!(String::from_utf8_lossy(&cargo.stdout), "cargo:cargo-one\n");
        assert_eq!(
            String::from_utf8_lossy(&python.stdout),
            "python:python-two\n"
        );
    }
}
