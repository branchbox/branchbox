---
title: 1Password Devcontainer E2E
description: Validate BranchBox's host-side 1Password secret fetch and container git/signing setup.
---
# Manual 1Password E2E Harness

`scripts/manual-1password-e2e.sh` validates the Docker Desktop for Mac workflow from issue #45 end-to-end:

1. `branchbox init` generates devcontainer + 1Password assets.
2. Devcontainer `initializeCommand` fetches PAT/signing key from 1Password.
3. Devcontainer `postStartCommand` configures git credential helper, HTTPS remote, and SSH signing.
4. A feature worktree is started and synced, then validated with the same checks.

## Prerequisites

- Docker Desktop running
- `devcontainer` CLI installed
- 1Password desktop app + `op` CLI available and authenticated
- A reachable GitHub SSH remote for `ORIGIN_SSH_URL`
- BranchBox binary build prerequisites (`cargo`, Rust toolchain), or run with `--skip-build` if `BRANCHBOX_BIN` already exists
- The harness runs on the **host** (not inside the devcontainer), so `BRANCHBOX_BIN` must match host OS/arch (a Linux devcontainer build will not execute on macOS host)

## Required inputs

```bash
export ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git'
export OP_GITHUB_REF='op://<vault>/<item>/token'
export OP_SIGNING_KEY_REF='op://<vault>/<item>/private key'
```

## Run

```bash
# Full run (default stack: generic)
./scripts/manual-1password-e2e.sh

# Include failure-path smoke check (invalid OP refs on restart)
./scripts/manual-1password-e2e.sh --check-failure-path

# Keep temp workspace for inspection
KEEP_E2E_TMP=1 ./scripts/manual-1password-e2e.sh

# Dry-run command plan only
./scripts/manual-1password-e2e.sh --mode pretend
```

If your host binary is missing:

```bash
cargo build -p branchbox-cli
```

## What it verifies

- `.devcontainer/scripts/init-host.sh` and `.devcontainer/scripts/setup-git.sh` exist.
- Main and feature containers receive non-empty token material from mounted files.
- `origin` remote is converted from SSH to HTTPS inside the container.
- If the mounted signing key is valid SSH private key material, git global config enables SSH signing with `branchbox-signing-key` and signature verification succeeds.
- If the mounted signing key is invalid/non-SSH material, workflow still succeeds with signing skipped (warning path), while git/gh token auth remains validated.
- `git commit` succeeds in both workspaces.
