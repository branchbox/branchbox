---
title: CLI End-to-End Manual Test
description: Hands-on checklist for validating `branchbox` from `init` through feature teardown.
---

The CLI smoke test below exercises the full BranchBox workflow on a disposable repository. Run it whenever you want to validate that `branchbox init`, devcontainer bootstrapping, feature lifecycle commands, and cleanup all behave as expected (e.g., before tagging a release).

## Prerequisites

- Docker Engine + `docker compose`
- Dev Container CLI (`devcontainer`)
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
     branchbox init --stack rust --reorganize -y
     ```
   - Expect a `main/` worktree to appear under the projects directory, `.devcontainer/` to be generated, `.env.sample` to be stamped, and `.branchbox/registry.json` to exist.

3. **Bring up the main devcontainer**
   - Ensure `main/.env` exists (copy from `.env.sample` if needed) so `docker compose` can load the env file list.
   - Run `docker compose -f main/.devcontainer/compose.yaml up -d --build` (supply `--project-directory main/.devcontainer` if you prefer explicit context).
   - Resolve the service named by `devcontainer.json`, then run `docker compose exec <service> git --version`. This should succeed, confirming the container has git and the repo bind mount.
   - Tear down with `docker compose ... down -v --remove-orphans`.

4. **Start a feature worktree**
   - From the container directory run `branchbox feature start cli-e2e-smoke`.
   - Expect:
     - New worktree directory `<container>/cli-e2e-smoke/` with a `.git` file pointing to the shared gitdir.
     - Git branch `feature/cli-e2e-smoke`.
     - `.devcontainer/` copied to the feature, `.env` duplicated with feature-specific `APP_URL`/`COMPOSE_PROJECT_NAME`.
     - Specs module creates/updates `docs/features/in-progress/cli-e2e-smoke.md`.
   - Build the feature with `devcontainer up --workspace-folder <feature>` and verify `git --version` with `devcontainer exec`. Capture `composeProjectName` from the CLI's success JSON; depending on the environment overlay, it may be the persisted BranchBox name or the CLI's `<feature>_devcontainer` default.

5. **Teardown and verify cleanup**
   - Run `branchbox feature teardown cli-e2e-smoke --delete-branch --complete-spec`.
   - Confirm the feature directory is gone, `git branch --list feature/cli-e2e-smoke` returns empty, the spec moved from `docs/features/in-progress/` to `docs/features/completed/`, and Docker label queries for the captured Compose project return no containers, networks, or volumes.

Document every discrepancy (missing `main/`, failed container launch, stale branches, etc.) before releasing.

## Automation Script

The repository ships `scripts/manual-cli-e2e.sh`, which runs the entire flow above:

- Builds `branchbox` if needed.
- Seeds a throwaway git repo under `$(mktemp)` and forces `branchbox init` to reorganize into a sibling temp directory.
- Brings the main stack up via `docker compose` and Feature A up through the devcontainer CLI, confirming git works inside both containers.
- Captures Feature A's actual Compose project from the devcontainer CLI success response and verifies teardown removes its containers, networks, and volumes.
- Starts three feature worktrees covering the normal, Cloudflared, and credential-loss fallback paths; validates registry/git state and tears each one down.
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

# Select a stack (rust, generic, rails, or node)
./scripts/manual-cli-e2e.sh --stack generic

# Spin up the HTTP drain stub and verify acks
./scripts/manual-agent-e2e.sh --cp-stub
```

`--mode verbose` enables shell tracing and passes verbose flags to BranchBox commands so you can watch every git/module operation. `--mode pretend` is a safe dry-run that logs each action without invoking BranchBox or Docker while still performing lightweight repo scaffolding under `/tmp`. `--stack` selects the generated template and the harness resolves the matching Compose service from `devcontainer.json`. Combine any mode with `KEEP_E2E_TMP=1` to preserve the temporary workspace for manual inspection. The script avoids Bash-4-only case-conversion syntax so the same commands work with the Bash version shipped by macOS.

`--cp-stub` starts a disposable Python HTTP server inside the devcontainer, points the agent’s `BRANCHBOX_CP_ENDPOINT` at it, and prints both the stub log and the `control_plane_status.last_ack_event_id` cursor once the CLI harness finishes. Use this whenever you want to see the durable-ack logic in action or reproduce control-plane failures locally.

Need a quick health check without rerunning the harness? Use `branchbox agent status --json`—it reports whether the drain is configured/connected and when the last delivery or failure occurred so you can diagnose token/endpoint issues.

Run the script locally before publishing releases (or wire it into CI once Docker is available). When it fails, use the manual checklist above to dig into the exact stage and file detailed bug reports.

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

## 1Password regression add-on

If your change touches devcontainer authentication/signing bootstrap (issue #45 path), run the dedicated harness after the CLI matrix:

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
OP_GITHUB_REF='op://<vault>/<item>/token' \
OP_SIGNING_KEY_REF='op://<vault>/<item>/private key' \
./scripts/manual-1password-e2e.sh --check-failure-path
```

See `scripts/manual-1password-e2e.md` for prerequisites, troubleshooting, and the expected warning-path behavior when invalid OP refs are provided.

## Mac App ↔ CLI Loop

The Mac app (`macos/`) is a front end for the `branchbox` CLI: every action runs `branchbox … --json`. Run this loop on a Mac whenever you touch the macOS app, the CLI's JSON output, teardown, prune, `config`, `init` or the feature registry. Use a disposable repository, never a real project. Record the results (CLI version, legacy or contract mode, pass/fail per step) in `macos/TESTING.md` and in the PR.

Run it twice when the change affects both modes: once with the branch-built CLI (contract mode) and once with the released 0.13.4 CLI (legacy mode). In legacy mode, step 12 is skipped.

0. **Build.** Run `cargo build -p branchbox-cli`, then `scripts/package-macos-app.sh --native --zip`. If you use a CI artifact instead, remove the quarantine flag first: `xattr -dr com.apple.quarantine BranchBox.app`.
1. **Launch from Finder**, so the app starts with launchd's minimal `PATH`. Onboarding should find `/opt/homebrew/bin/branchbox`; to test the branch CLI, choose `target/debug/branchbox` in Settings › Tools › Locate…. Diagnostics shows the chosen CLI, its capabilities and the doctor checks.
2. **Add a project.** Create a disposable repository (`git init`, one commit), add it, and choose Set Up BranchBox with the layout kept (the app runs `init -y`). The project appears, and the repository did not move.
3. **Start a minimal feature.** The inspector streams the log, and the result shows the resolved name. In Terminal, `branchbox feature list --json --repo <repo>` lists it.
4. **Start another feature from Terminal.** The app shows it within about a second, without a manual refresh.
5. **Run Command** `echo hi`, then `sh -c 'exit 3'`. The second shows exit code 3 in the output, with no error alert.
6. **Teardown refusal and discard.** Run `touch notes.txt` in a feature's worktree, then Tear Down. The refusal card names `notes.txt` and nothing was removed. Choose Discard and confirm: the feature is removed, and `git branch --list` shows the branch kept or deleted as the chosen policy says.
7. **Unmerged branch.** Commit in a feature's worktree, then Tear Down with Delete if merged. Teardown is blocked on the unmerged branch. Choose Force-delete and confirm: the branch is deleted.
8. **Prune.** With three features, one of them with an untracked file, open Prune. The dirty feature is unchecked. Run it and check the per-feature results.
9. **Cancel a start.** Add a `post-checkout` hook that sleeps (`printf '#!/bin/sh\nsleep 30\n' > .git/hooks/post-checkout && chmod +x .git/hooks/post-checkout`), start a feature and cancel it. The confirmation explains what cancelling leaves behind. The feature then shows as Interrupted (or an unregistered worktree), with Resume Setup or Remove.
10. **Window lifecycle.** Close the main window: the app stays in the menu bar, and the menu bar's Open BranchBox reopens it. Close it again and use the menu bar's Tear Down… on a feature: the window opens with the teardown sheet.
11. **Quit during an operation.** Start a feature and quit. The app asks first. Choose Cancel and Quit; afterwards `pgrep branchbox` prints nothing.
12. **Project settings** (contract CLI only). Change the branch prefix in the project's settings. `branchbox config get feature.branch_prefix --json --repo <repo>` shows the new value.
13. **Unbundled dev loop.** `cd macos && swift run BranchBox` starts a feature without crashing. Notifications are switched off there.

Clean up the disposable repository and its sibling worktrees afterwards. File any divergence (the app and `branchbox feature list` disagreeing, a refusal without a recovery, a leftover `branchbox` process) before marking the PR ready.
