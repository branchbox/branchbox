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
- Records every failed expectation and exits non-zero with a summary of bugs.

Usage:

```bash
# Regular run (default)
./scripts/manual-cli-e2e.sh

# Verbose tracing + extra BranchBox logs
./scripts/manual-cli-e2e.sh --mode verbose

# Pretend/dry-run (log steps, skip BranchBox + Docker)
./scripts/manual-cli-e2e.sh --mode pretend
```

`--mode verbose` enables shell tracing and passes verbose flags to BranchBox commands so you can watch every git/module operation. `--mode pretend` is a safe dry-run that logs each action without invoking BranchBox or Docker while still performing lightweight repo scaffolding under `/tmp`. Combine any mode with `KEEP_E2E_TMP=1` to preserve the temporary workspace for manual inspection.

Run the script locally before publishing releases (or wire it into CI once Docker is available). When it fails, use the manual checklist above to dig into the exact stage and file detailed bug reports.
