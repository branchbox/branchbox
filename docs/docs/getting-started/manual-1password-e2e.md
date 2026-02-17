---
title: 1Password Devcontainer E2E
description: Validate BranchBox's host-side 1Password secret fetch and container git/signing setup.
---
# Manual 1Password E2E Harness

`scripts/manual-1password-e2e.sh` validates devcontainer credential/bootstrap flows end-to-end:

1. `branchbox init` generates devcontainer + credential bootstrap assets.
2. Devcontainer `initializeCommand` refreshes host credential files (1Password or fixtures).
3. Devcontainer `postStartCommand` configures git credential helper, remote form, and optional SSH signing.
4. A feature worktree is started and synced, then validated with the same expectations.

## Prerequisites

- Docker Desktop running
- `devcontainer` CLI installed
- Docker host can run `devcontainer up`
- A reachable SSH remote URL for `ORIGIN_SSH_URL` (GitHub required only when expecting SSH→HTTPS rewrite)
- BranchBox binary build prerequisites (`cargo`, Rust toolchain), or run with `--skip-build` if `BRANCHBOX_BIN` already exists
- The harness runs on the **host** (not inside the devcontainer), so `BRANCHBOX_BIN` must match host OS/arch (a Linux devcontainer build will not execute on macOS host)

## Required inputs

```bash
export ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git'   # or ssh://...
export OP_GITHUB_REF='op://<vault>/<item>/token'          # required unless --skip-op-refresh
export OP_SIGNING_KEY_REF='op://<vault>/<item>/private key' # required unless --skip-op-refresh
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

## Scenario matrix examples

### 1) Full 1Password + GitHub + signing flow (issue #45 baseline)

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
OP_GITHUB_REF='op://<vault>/<item>/token' \
OP_SIGNING_KEY_REF='op://<vault>/<item>/private key' \
./scripts/manual-1password-e2e.sh --expect-remote https --expect-signing auto
```

### 2) No 1Password, GitHub token fixture only, no signing

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
./scripts/manual-1password-e2e.sh \
  --skip-op-refresh \
  --seed-token \
  --expect-remote https \
  --expect-signing disabled
```

### 3) No 1Password, no GitHub token, keep SSH remote, no signing

```bash
ORIGIN_SSH_URL='git@gitlab.com:<org>/<repo>.git' \
./scripts/manual-1password-e2e.sh \
  --skip-op-refresh \
  --allow-missing-token \
  --expect-remote ssh \
  --expect-signing disabled
```

### 4) No 1Password, fixture token + fixture signing key

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
./scripts/manual-1password-e2e.sh \
  --skip-op-refresh \
  --seed-token \
  --seed-signing-key valid \
  --expect-remote https \
  --expect-signing required
```

If your host binary is missing:

```bash
cargo build -p branchbox-cli
```

On macOS, the harness now auto-detects `SDKROOT` (via `xcrun`) before host `cargo build` to avoid missing system-header errors.

## What it verifies

- `.devcontainer/scripts/init-host.sh` and `.devcontainer/scripts/setup-git.sh` exist.
- Optional token material handling (`--allow-missing-token` vs enforced).
- Remote behavior (`--expect-remote https|ssh`) based on token/remote policy.
- Signing behavior (`--expect-signing auto|required|disabled`) for valid/invalid/absent key material.
- 1Password-hosted and fixture-based credential seeding paths.
- `git commit` succeeds in both workspaces.
