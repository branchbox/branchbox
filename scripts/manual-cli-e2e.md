# Manual CLI E2E Harness

`scripts/manual-cli-e2e.sh` is the high-signal regression harness for BranchBox’s CLI. It bootstraps a disposable project, runs through the entire workflow (init → multi-feature sync → tunnel paths → dirty teardown), and can operate in three modes:

| Mode     | Description                                                                 |
|----------|-----------------------------------------------------------------------------|
| regular  | Full Docker/devcontainer execution. Builds the CLI, spawns containers, and performs real worktree operations. |
| verbose  | Same as regular but with `set -x` plus component logs to make debugging easier. |
| pretend  | Dry-run mode. Logs every step without touching Docker/git so you can sanity check control flow. |

```bash
# Always run all three before marking a PR ready
./scripts/manual-cli-e2e.sh
./scripts/manual-cli-e2e.sh --mode verbose
./scripts/manual-cli-e2e.sh --mode pretend

# Target a specific stack (default: rust)
STACK=generic ./scripts/manual-cli-e2e.sh --mode verbose --stack generic
```

Supported stacks today: `rust` (default), `generic`, `rails`, and `node`. Pass `--stack <stack>` or set `STACK=<stack>` to override; CI runs a matrix so template regressions surface quickly.

## Release-blocking matrix

Before cutting a tag, run the harness across every mode and stack:

```bash
./scripts/manual-cli-e2e.sh
./scripts/manual-cli-e2e.sh --mode verbose
./scripts/manual-cli-e2e.sh --mode pretend
STACK=generic ./scripts/manual-cli-e2e.sh
STACK=rails ./scripts/manual-cli-e2e.sh
STACK=node ./scripts/manual-cli-e2e.sh
```

Document pass/fail status in the release PR. If you change or add an adapter, extend the matrix with `STACK=<stack>` for that target until CI covers it.

## Coverage Matrix

1. **Init & bootstrap** – seeds a sample Rust repo, runs `branchbox init`, records generated artifacts in git, and boots the root devcontainer.
2. **Feature A (manual tunnel)** – exercises the default path where Cloudflare credentials are absent. Validates specs, registry insertion, devcontainer sync, and ensures the tunnel module reports “skipped”.
3. **Feature B (Cloudflared)** – seeds fake Cloudflare credentials/config, enforces the tunnel module, boots the feature devcontainer, and asserts `.devcontainer/.cloudflared.env` contents and registry metadata. After `branchbox devcontainer sync` runs, the harness confirms both worktrees pick up the change (real sync plus dry-run log scanning).
4. **Feature B teardown** – removes the Cloudflared worktree, ensuring `.cloudflared.env` and registry fields disappear.
5. **Dirty teardown guard** – appends a comment to Feature A’s devcontainer file before teardown so the CLI warns about dirty files, then repeats with `--force`. This is intentional; expect the first teardown to fail before the automatic retry.
6. **Credential-loss fallback (Feature C)** – deletes `.branchbox/secure/cloudflared.env` and flips config back to manual instructions, starts another feature, verifies the tunnel module downgrades to “skipped”, then tears it down.

## Debugging Tips

| Tip | Details |
|-----|---------|
| Preserve artifacts | `KEEP_E2E_TMP=1 ./scripts/manual-cli-e2e.sh` prevents cleanup so you can inspect workspace logs, configs, and worktrees under `/tmp/branchbox-cli-e2e-*`. |
| Custom binaries | Set `BRANCHBOX_BIN=/path/to/custom/branchbox` to reuse a prebuilt CLI. |
| Feature names | Override `FEATURE_NAME`, `SECONDARY_FEATURE_NAME`, or `FALLBACK_FEATURE_NAME` if you need deterministic names while debugging. |
| Logs | All key command logs land in `$TMP/logs/` (init/start/teardown, devcontainer sync, etc.). Tail them instead of rerunning when possible. |
| Docker cleanup | The script tracks every `docker compose up` and tears them down automatically. If you exit early, run `docker ps --format '{{.Names}}' | grep cli-e2e` to clean up stragglers. |

## Common Failures

- **“branchbox devcontainer sync … failed”** – check Docker availability and ensure the repo builds (`cargo build -p branchbox-cli` runs first).
- **Dirty teardown prompt keeps failing** – remove your manual edits, rerun the harness, or inspect `feature-teardown.log` under `$TMP/logs/`.
- **Tunnel assertions** – if `.cloudflared.env` is missing for the Cloudflared feature, inspect `.branchbox/config.json` in the temporary workspace to confirm the seeded credentials landed.

Keeping this harness green is a release-blocking requirement. If you modify devcontainer templates, tunnel logic, or registry fields, update the script and rerun all three modes before pushing.

## Related harnesses

- `scripts/manual-1password-e2e.sh` focuses specifically on the 1Password PAT + SSH signing flow described in issue #45 (host `op read` + container git setup).
- If your changes touch `.devcontainer` auth/signing bootstrap, also run:

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
OP_GITHUB_REF='op://<vault>/<item>/token' \
OP_SIGNING_KEY_REF='op://<vault>/<item>/private key' \
./scripts/manual-1password-e2e.sh --check-failure-path
```
