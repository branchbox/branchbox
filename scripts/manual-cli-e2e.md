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
./scripts/manual-cli-e2e.sh --mode verbose --stack generic
```

Supported stacks today: `rust` (default), `generic`, `rails`, and `node`. Pass `--stack <stack>` or set `STACK=<stack>` to override; the harness resolves the corresponding Compose service from `devcontainer.json`. CI runs a matrix so template regressions surface quickly. Argument normalization remains compatible with the Bash version bundled with macOS.

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
2. **Feature A (manual tunnel)** – exercises the default path where Cloudflare credentials are absent. Boots the workspace through the devcontainer CLI, validates specs, registry insertion and sync, and ensures the tunnel module reports “skipped”.
3. **Feature B (Cloudflared)** – seeds fake Cloudflare credentials/config, enforces the tunnel module, boots the feature devcontainer, and asserts `.devcontainer/.cloudflared.env` contents and registry metadata. After `branchbox devcontainer sync` runs, the harness confirms both worktrees pick up the change (real sync plus dry-run log scanning).
4. **Feature B teardown** – removes the Cloudflared worktree, ensuring `.cloudflared.env` and registry fields disappear.
5. **Dirty teardown and Docker cleanup** – appends a comment to Feature A’s devcontainer file before teardown so the CLI warns about dirty files, then repeats with `--force`. The harness captures the devcontainer CLI's reported Compose project and verifies its labels leave no containers, networks, or volumes. The initial failure before the automatic retry is intentional.
6. **Credential-loss fallback (Feature C)** – deletes `.branchbox/secure/cloudflared.env` and flips config back to manual instructions, starts another feature, verifies the tunnel module downgrades to “skipped”, then tears it down.

## Debugging Tips

| Tip | Details |
|-----|---------|
| Preserve artifacts | `KEEP_E2E_TMP=1 ./scripts/manual-cli-e2e.sh` prevents cleanup so you can inspect workspace logs, configs, and worktrees under `/tmp/branchbox-cli-e2e-*`. |
| Custom binaries | Set `BRANCHBOX_BIN=/path/to/custom/branchbox` to reuse a prebuilt CLI. |
| Feature names | Override `FEATURE_NAME`, `SECONDARY_FEATURE_NAME`, or `FALLBACK_FEATURE_NAME` if you need deterministic names while debugging. |
| Logs | All key command logs land in `$TMP/logs/` (init/start/teardown, devcontainer sync, etc.). Tail them instead of rerunning when possible. |
| Docker cleanup | The script tracks both `docker compose` and devcontainer CLI workspaces and tears them down automatically. If you exit early, inspect resources with the `com.docker.compose.project` label before cleaning up stragglers. |

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
