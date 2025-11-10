---
title: CLI End-to-End Manual Test
description: Hands-on checklist for validating `branchbox` from `init` through feature teardown.
---

The CLI smoke test below exercises the full BranchBox workflow on a disposable repository. Run it whenever you want to validate that `branchbox init`, devcontainer bootstrapping, feature lifecycle commands, and cleanup all behave as expected (e.g., before tagging a release).

## Prerequisites

- Docker Engine + `docker compose`
- Rust toolchain (so `cargo build -p branchbox-cli` succeeds)
- `jq` (used by the automation script to read `devcontainer.json`)
- Host machine can run privileged containers (the generated devcontainer enables Docker-in-Docker)

Set `BRANCHBOX_SKIP_HOST_VALIDATION=1` while running these steps so the workflow skips host safety checks. The script described later does this automatically.

## Manual Flow

1. **Seed a disposable repo**
   - Create a fresh git repo under `/tmp/branchbox-cli-e2e/seed-app`.
   - Drop in a tiny Rust project (one `Cargo.toml`, one `src/main.rs`), `git add`, and `git commit`.
   - Export `BRANCHBOX_PROJECTS_DIR` to a second temp directory so reorganization stays isolated.

2. **Initialize BranchBox**
   - From inside the repo run:  
     ```bash
     BRANCHBOX_SKIP_HOST_VALIDATION=1 \
     BRANCHBOX_PROJECTS_DIR="$BRANCHBOX_PROJECTS_DIR" \
     branchbox init --stack rust --reorganize --use-parent-structure -y
     ```
   - Expect a `main/` worktree to appear under the projects directory, `.devcontainer/` to be generated, `.env.sample` to be stamped, and `.branchbox/registry.json` to exist.

3. **Bring up the main devcontainer**
   - Ensure `main/.env` exists (copy from `.env.sample` if needed) so `docker compose` can load the env file list.
   - Run `docker compose -f main/.devcontainer/compose.yaml up -d --build` (supply `--project-directory main/.devcontainer` if you prefer explicit context).
   - `docker compose exec rust-dev git --version` should succeed, confirming the container has git and the repo bind mount.
   - Tear down with `docker compose ... down -v --remove-orphans`.

4. **Start a feature worktree**
   - From the container directory run `branchbox feature start cli-e2e-smoke`.
   - Expect:
     - New worktree directory `<container>/cli-e2e-smoke/` with a `.git` file pointing to the shared gitdir.
     - Git branch `feature/cli-e2e-smoke`.
     - `.devcontainer/` copied to the feature, `.env` duplicated with feature-specific `APP_URL`/`COMPOSE_PROJECT_NAME`.
     - Specs module creates/updates `docs/features/in-progress/cli-e2e-smoke.md`.
   - Build the feature devcontainer via `docker compose -f <feature>/.devcontainer/compose.yaml up -d --build` and verify `git --version` inside the container.

5. **Teardown and verify cleanup**
   - Run `branchbox feature teardown cli-e2e-smoke --delete-branch --complete-spec`.
   - Confirm the feature directory is gone, `git branch --list feature/cli-e2e-smoke` returns empty, the devcontainer directory vanished with the worktree, and the spec moved from `docs/features/in-progress/` to `docs/features/completed/`.

Document every discrepancy (missing `main/`, failed container launch, stale branches, etc.) before releasing.

## Automation Script

The repository ships `scripts/manual-cli-e2e.sh`, which runs the entire flow above:

- Builds `branchbox` if needed.
- Seeds a throwaway git repo under `$(mktemp)` and forces `branchbox init` to reorganize into a sibling temp directory.
- Brings main + feature devcontainers up via `docker compose`, confirming git works inside both containers.
- Starts a feature, validates registry/git state, then tears it down with `--delete-branch --complete-spec`.
- Ensures `.devcontainer/.branchbox.env` exists in both the main worktree and its feature copy so per-worktree overrides stay intact.
- Injects JSONC comments into `devcontainer.json` to confirm BranchBox accepts commented configs before syncing.
- Exercises `branchbox devcontainer sync --dry-run` with `copy` and `symlink` strategies so downstream tooling can rely on the command.
- Seeds a backlog spec under `docs/features/backlog/` and verifies the specs module promotes it to `in-progress/` on start and `completed/` on teardown via `FEATURES_DIR`.
- Captures `branchbox feature list --json` (while active) and `--json --all` (after teardown) to ensure the richer registry metadata matches reality.
- Records every failed expectation and exits non-zero with a summary of bugs.

Usage:

```bash
# Regular run (default)
./scripts/manual-cli-e2e.sh

# Verbose tracing + extra BranchBox logs
./scripts/manual-cli-e2e.sh --mode verbose

# Pretend/dry-run (log steps, skip BranchBox + Docker)
./scripts/manual-cli-e2e.sh --mode pretend

# Spin up the HTTP drain stub and verify acks
./scripts/manual-agent-e2e.sh --cp-stub
```

`--mode verbose` enables shell tracing and passes verbose flags to BranchBox commands so you can watch every git/module operation. `--mode pretend` is a safe dry-run that logs each action without invoking BranchBox or Docker while still performing lightweight repo scaffolding under `/tmp`. Combine any mode with `KEEP_E2E_TMP=1` to preserve the temporary workspace for manual inspection.

`--cp-stub` starts a disposable Python HTTP server inside the devcontainer, points the agent’s `BRANCHBOX_CP_ENDPOINT` at it, and prints both the stub log and the `control_plane_status.last_ack_event_id` cursor once the CLI harness finishes. Use this whenever you want to see the durable-ack logic in action or reproduce control-plane failures locally.

Run the script locally before publishing releases (or wire it into CI once Docker is available). When it fails, use the manual checklist above to dig into the exact stage and file detailed bug reports.

## Mac App ↔ Agent Loop

Milestone 2 adds a minimal SwiftUI client under `macos/` so we can validate end-to-end agent orchestration on macOS. Run this loop in addition to the CLI harness whenever you touch the agent, control-plane drain, or desktop app code:

1. **Start the agent locally**
   - From the repo root run `cargo run -p branchbox-agent` (or `scripts/manual-agent-e2e.sh` to reuse the smoke harness). Set `BRANCHBOX_AGENT_DIR` so the daemon stores its SQLite queue outside your real workspace if you want a clean slate.
   - Optional: point the HTTP drain at a staging endpoint with `BRANCHBOX_CP_ENDPOINT=https://example.test/hooks/devices BRANCHBOX_CP_TOKEN=fake-token` so you can confirm batches land outside stdout.
2. **Configure workspace + gRPC address for the mac app**
   - On macOS set the expected workspace path via `defaults write dev.branchbox.app workspace "$(pwd)"`.
   - Override the transport as needed with `export BRANCHBOX_AGENT_GRPC_ADDR=127.0.0.1:50515` or by editing `~/Library/Preferences/dev.branchbox.app.plist`.
3. **Run the SwiftUI preview**
   - From a mac host run `cd macos && swift run BranchBoxApp`. The window should list all features detected by `FeatureService/List`. Rows tagged “CLI” indicate the fallback path kicked in because the gRPC transport was unavailable.
4. **Start + teardown from the UI**
   - Use the “Start feature” form to launch a new worktree (toggle minimal mode + prompt seed as needed). Confirm the action flows through gRPC (watch the agent logs) and that the entry appears with the right status.
   - Select “Teardown” on the new feature. Verify the worktree disappears, the specs module runs, and the UI updates.
5. **Confirm control-plane delivery**
   - Tail the agent logs to ensure the HTTP drain batches the start + teardown events (look for `control plane` lines and host metadata). When pointing at a stub endpoint you should see HTTP 200s; otherwise the agent logs that it fell back to local logging.

Document any divergence (UI not updating, CLI fallback misfiring, HTTP drain errors) in the Milestone 2 tracking issue before marking a PR ready for review.

## Release-blocking matrix

Every release candidate must pass the harness in all modes and stacks listed below. This matrix mirrors the requirements in `AGENTS.md` and `RELEASING.md`—document the results in your release notes so reviewers know the workflow was exercised end-to-end.

```bash
./scripts/manual-cli-e2e.sh
./scripts/manual-cli-e2e.sh --mode verbose
./scripts/manual-cli-e2e.sh --mode pretend
STACK=generic ./scripts/manual-cli-e2e.sh
STACK=rails ./scripts/manual-cli-e2e.sh
STACK=node ./scripts/manual-cli-e2e.sh
```

If you touch a different adapter or stack, repeat with `STACK=<stack>` for that target as well. Use `KEEP_E2E_TMP=1` when you need to preserve the temporary workspace for debugging and summarize any deviations in the release PR before attempting `cargo release`.
