---
title: 1Password Devcontainer E2E
description: Validate BranchBox's host-side 1Password secret fetch and container git/signing setup.
---

`scripts/manual-1password-e2e.sh` validates the Docker Desktop for Mac flow from issue #45 end-to-end:

1. `branchbox init` generates devcontainer + 1Password assets.
2. Devcontainer `initializeCommand` fetches PAT/signing key from 1Password on the host.
3. Devcontainer `postStartCommand` configures git credential helper, HTTPS remote, and SSH signing in the container.
4. A feature worktree is started/synced and validated with the same checks.

## Prerequisites

- Docker Desktop running
- `devcontainer` CLI installed
- 1Password desktop app + `op` CLI installed and authenticated
- Reachable GitHub SSH remote (`git@github.com:...`) for `ORIGIN_SSH_URL`
- BranchBox CLI binary built for your **host OS/arch** (Linux devcontainer binaries will not execute on macOS host)

## Required environment variables

```bash
export ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git'
export OP_GITHUB_REF='op://<vault>/<item>/token'
export OP_SIGNING_KEY_REF='op://<vault>/<item>/private key'
```

## Run the harness

```bash
# Full run (default stack: generic)
./scripts/manual-1password-e2e.sh

# Include invalid-reference warning-path check
./scripts/manual-1password-e2e.sh --check-failure-path

# Preserve temp workspace for inspection
KEEP_E2E_TMP=1 ./scripts/manual-1password-e2e.sh

# Dry-run (prints steps only)
./scripts/manual-1password-e2e.sh --mode pretend
```

Need to rebuild the host binary?

```bash
cargo build -p branchbox-cli
```

## What this verifies

- `.devcontainer/scripts/init-host.sh` and `.devcontainer/scripts/setup-git.sh` exist.
- Main and feature containers receive non-empty token material from mounted files.
- `origin` remote is converted from SSH to HTTPS in the container.
- If signing key content is valid SSH private-key material, git SSH signing is configured and signature verification succeeds.
- If signing key content is invalid/non-SSH material, setup continues with a warning while PAT-based auth still works.
- `git commit` succeeds in both main and feature workspaces.
