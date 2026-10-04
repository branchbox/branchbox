# CLI 0.13.4 fixtures

Real output of `branchbox 0.13.4` (Homebrew build, macOS arm64), captured on 2026-10-01 for the
macOS app revamp contract audit (`docs/features/in-progress/mac-app-revamp/audit/cli-contract.md`).
The app's Swift models are tested against these files (`BranchBoxKitTests`), so they must stay
byte-for-byte what the CLI printed apart from the path scrub below.

## Path scrub

Absolute paths were rewritten after capture. Nothing else was edited.

| Captured prefix | Fixture prefix |
|---|---|
| the disposable sandbox root (`/private/tmp/<session scratch>/cli-contract/sandbox`) | `/tmp/bbx` |
| any other path under the session scratch directory in `/private/tmp` | `/tmp/bbx` |
| the capturing user's home directory (`/Users/<user>`) | `/Users/dev` |

`ModelDecodingTests.fixturesCarryNoLocalPaths` fails if a fixture still contains the real home
directory or the session scratch root.

## Environments

- `main_*`: the BranchBox checkout itself (`$MAIN`, the main worktree), read-only commands only.
- `sandbox_*`: a disposable repo `R=/tmp/bbx/demo` (`git init` plus one commit, no `.gitignore`,
  no `.devcontainer`), so every feature worktree lands at `/tmp/bbx/<name>`.
- `.json`/`.txt`/`.stdout` hold stdout and `.stderr` holds stderr of the same run. stderr keeps the
  tracing subscriber's ANSI colour codes exactly as printed to a terminal.

## Capture commands

The commands are reconstructed from the audit's verification log (`cli-contract.md`, "Verification").
`sandbox_start_theta_trace` is not in that log; its command is inferred from the DEBUG-level stderr.

| File(s) | Command | Exit |
|---|---|---|
| `root_help.txt` | `branchbox --help` | 0 |
| `main_feature_list.json` / `.stderr` | `cd $MAIN && branchbox feature list --json` | 0 |
| `main_feature_list_all.json` / `.stderr` | `cd $MAIN && branchbox feature list --all --json` | 0 |
| `main_feature_list_repo.json` / `.stderr` | `branchbox feature list --repo $MAIN --json` | 0 |
| `main_detect.txt` / `.stderr` | `cd $MAIN && branchbox detect` | 0 |
| `main_detect_path.txt` / `.stderr` | `cd $MAIN && branchbox detect --path .` | 0 |
| `main_agent_status.json` (empty) / `.stderr` | `cd $MAIN && branchbox agent status --json` (no agent daemon running) | 1 |
| `main_name_generate.txt` / `.stderr` | `branchbox name generate "OAuth Integration"` | 0 |
| `sandbox_detect.txt` / `.stderr` | `branchbox detect --path $R` | 0 |
| `sandbox_start_alpha.json` / `.stderr` | `cd $R && branchbox feature start alpha --repo $R --json --no-summary --minimal --skip-module tunnel </dev/null` | 0 |
| `sandbox_start_beta_full.json` / `.stderr` | `branchbox feature start beta --repo $R --json --no-summary --skip-module tunnel </dev/null` (full mode) | 0 |
| `sandbox_start_gamma_longprompt.json` / `.stderr` | `branchbox feature start gamma --repo $R --json --no-summary --minimal --skip-module tunnel --prompt <2100 × "x">`; stdout starts with a non-JSON preamble line (BUG-11) | 0 |
| `sandbox_start_theta_trace.json` / `.stderr` | `RUST_LOG=debug branchbox feature start theta --repo $R --json --no-summary --skip-module tunnel </dev/null` | 0 |
| `sandbox_start_zeta_prefix.json` | `branchbox feature start zeta --branch-prefix spike --repo $R --json --no-summary --minimal --skip-module tunnel </dev/null` | 0 |
| `sandbox_exec_alpha.json` / `.stderr` | `branchbox feature exec alpha --repo $R --json -- echo hi` | 0 |
| `sandbox_exec_alpha_fail.json` / `.stderr` | `branchbox feature exec alpha --repo $R --json -- sh -c 'echo out; echo err >&2; exit 3'`; the payload is printed, then `Error:` on stderr | 1 |
| `sandbox_exec_alpha_trailing.txt` | `branchbox feature exec alpha -- echo hi --json --repo $R`; flags after `--` reach the command | 0 |
| `sandbox_feature_list.json` / `.stderr` | `branchbox feature list --repo $R --json` | 0 |
| `sandbox_feature_list_all_after.json` | `branchbox feature list --all --repo $R --json` after the teardowns below | 0 |
| `sandbox_teardown_alpha.json` / `.stderr` | `branchbox feature teardown alpha --repo $R --json`; refused, and stdout is the human dirty-module banner, not JSON | 1 |
| `sandbox_teardown_alpha_force.json` / `.stderr` | `branchbox feature teardown alpha --repo $R --force --json` | 0 |
| `sandbox_teardown_beta_default.stdout` (empty) / `.stderr` | commit on `feature/beta`, then `branchbox feature teardown beta --repo $R </dev/null` (unmerged branch) | 1 |
| `sandbox_teardown_gamma_force.stdout` / `.stderr` | `branchbox feature teardown gamma --repo $R --force` | 0 |
| `sandbox_teardown_eta_dirty.stdout` / `.stderr` | `echo work > /tmp/bbx/eta/notes.txt`, edit `README.md`, then `branchbox feature teardown eta --repo $R --keep-branch` (user changes lost) | 0 |
| `sandbox_teardown_zeta_noprefix.stdout` / `.stderr` | `branchbox feature teardown zeta --repo $R` (custom prefix not passed back) | 0 |
| `sandbox_devcontainer_sync_noactive.stdout` / `.stderr` | `branchbox devcontainer sync --path $R --strategy copy` with no active features | 0 |
| `sandbox_devcontainer_sync.stdout` / `.stderr` | the same with one active feature and no main `.devcontainer` | 1 |
| `sandbox_prune_noyes.stdout` / `.stderr` | 46 active features, `branchbox prune --repo $R </dev/null` | 1 |
| `sandbox_prune_yes.stdout` / `.stderr` | `branchbox prune --yes --delete-branch --repo $R` | 0 |

## Synthetic files

These were written by hand, not captured, and are named `synthetic_*` (see `SYNTHETIC_NOTE.txt`).

| File | Built from | Why |
|---|---|---|
| `synthetic_feature_list_new_statuses.json` | core `FeatureMetadata` / `RuntimeMetadata` serde types | `degraded`, `failed_retained` and `orphaned` records, sbx published ports, a tunnel hostname, 9- and 1-digit fractions |
| `synthetic_agent_status.json` | `cli/src/agent.rs` `AgentStatus` | gRPC-era agent status; kept for the record, not decoded by the app |
| `synthetic_devcontainer_up.json` | `core/src/devcontainer_runtime/runtime.rs` `UpResult` (camelCase), compose-based | `devcontainer up --json` |
| `synthetic_devcontainer_up_image.json` | `UpResult`, image-based (`remoteUser` null, `composeProjectName` skipped) | `devcontainer up --json` |
| `synthetic_devcontainer_down.json` | `DownResult` (`removed`) | `devcontainer down --json` |
| `synthetic_devcontainer_down_compose.json` | `DownResult` (`stopped`, `removedContainers` skipped) | `devcontainer down --json` |
| `synthetic_devcontainer_build.json` | `BuildResult` | `devcontainer build --json` |
| `synthetic_devcontainer_exec.json` | `ExecResult` (camelCase `exitCode`) | `devcontainer exec --json` with a failing command |
| `synthetic_devcontainer_up_docker_unavailable.json` | `cli/src/commands/devcontainer.rs` `cmd_up` (`serde_json::json!` compact line) | the error outcome printed on stdout, exit 1 |
| `synthetic_devcontainer_detect.json` | `cli/src/commands/devcontainer.rs` `DetectOutput` (snake_case) | `devcontainer detect --json` |
