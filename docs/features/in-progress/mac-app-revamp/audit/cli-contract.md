# Audit area: cli-contract

## Summary
I audited every CLI call the macOS app can make against branchbox 0.13.4: the help text, the Rust source and real captured output. I then fed the real payloads into the app's own Swift models and functions through a throwaway XCTest in a scratch copy. That copy is $SCRATCH/cli-contract/macos. The test files are CLIContractTests.swift, PipeDeadlockTests.swift and TunnelSummaryTests.swift. Captured output is in .../cli-contract/captures and the test logs are .../cli-contract/swift_test_*.log. The repo was not modified (`git status --porcelain` shows 0 changed files) and every process I started has exited.

**What the app calls.** All CLI calls go through one function, `CLICompat.run` (macos/Sources/BranchBoxApp/Agent/CLICompat.swift:103-128). It runs `/usr/bin/env <cli>` with no explicit environment and no stdin, and sets the working directory to the workspace. stdout and stderr go to separate pipes, so stderr is never mixed into the JSON. It waits for the process to exit before reading either pipe. On a non-zero exit it throws using the whole stderr as the message. The seven calls are:
- `feature list --json --repo WS [--all]`, decoded as JSON
- `feature start NAME --repo WS --json --no-summary [--title] [--minimal] [--branch-prefix] [--reuse] [--skip-module]* [--prompt]`, output thrown away
- `feature teardown NAME --repo WS [--force] [--complete-spec] [--delete-branch]`, output thrown away
- `--help`, scanned for the word "agent"
- `agent status --json`, decoded
- `devcontainer sync --path WS [--strategy] [--dry-run]`, output thrown away
- `detect --path P`, shown as raw text

All of these subcommands and flags still exist. None of the calls the app makes can hang on an interactive prompt: teardown and prune check whether stdout is a terminal, and the app's stdout is a pipe. Instead of hanging they now fail.

**Decoding.** Every real list payload decodes into `CLICompat.FeatureRecord` on this Mac (macOS 26), including fractional and nanosecond timestamps. Decoding is not the reason the app feels broken, at least on current macOS. The app targets macOS 13, though, and older Foundation may reject fractional seconds; I could not test that here.

What breaks is meaning, not parsing:
- **Tunnel.** The app reads flat `tunnel_status/provider/hostname` keys. The CLI now sends a nested `tunnel{}` object, so these are always nil and the Home tunnel card shows "Unknown provider / Unknown status".
- **Module status.** The CLI says "success|skipped|failed" but the app counts "ok", so every feature shows "0 ok".
- **New statuses.** degraded, failed_retained and orphaned are treated as removed, and one shows as "Failed_Retained".
- **Ignored fields.** The app drops 11 top-level keys, including runtime (provider, ids, host port mappings), default_agent, base_branch, last_commit, pr_number, color, env_path, compose_project_name, created_at and removed_at. It also ignores all start and teardown JSON.

**Why the CLI fallback feels broken: four app-side defects, all reproduced.**
1. In the default Automatic mode with no agent running (Homebrew ships no agent binary), `listFeatures` never falls back to the CLI. It was still waiting after 200 s. The cause is grpc-swift's default `waitsForConnectivity`, unlimited reconnect retries, and no deadline on the call.
2. `run()` waits for exit before draining the pipes. With about 104 KB of real `feature list --json` output, `CLICompat.featureList` hung for more than 30 s. At about 2 KB per feature, roughly 30 features would trigger it.
3. An app launched from Finder or the Dock gets launchd's PATH. `/usr/bin/env branchbox` then fails with "env: branchbox: No such file or directory".
4. Errors show the entire stderr. Since 0.12.1 that is mostly INFO log lines, with the real `Error:` line at the end.

**Teardown is the worst mismatch.**
- The CLI deletes the branch by default, so the app's "Delete branch" toggle being off does nothing. Verified: the feature branch was deleted with `deleteBranch:false`.
- If the branch has unmerged commits, the CLI first removes the worktree and marks the feature removed, then exits 1 because it can't delete the branch without force. The app shows "Teardown failed" and doesn't refresh the list.
- Teardown doesn't pass `--branch-prefix`, so features started with a custom prefix leave their branch behind, and the CLI still exits 0.
- A CLI-side issue makes the "Force removal" toggle misleading. Without `--force`, the CLI still deletes uncommitted work: when `git worktree remove` refuses, it falls back to `fs::remove_dir_all`. Verified: a modified README.md and an untracked notes.txt were deleted, exit 0.

Smaller issues:
- The CLI turns non-slug names into slugs ("Epsilon Feature" becomes "epsilon"), and `--title` is ignored when a name is given. The app never reads the start JSON, so it doesn't learn the real name.
- `devcontainer sync` has no `--json`. It exits 0 even when individual worktrees fail.
- The agent status probe swallows "agent not running" and reports everything as false.
- The CLI writes plain text to stdout in `--json` mode when a prompt is over 2000 characters, and in the dirty-worktree teardown path.


## Architecture notes
- Every CLI call goes through one synchronous helper, CLICompat.run (CLICompat.swift:103-128): `/usr/bin/env <cli> args`, cwd = workspace, stdout and stderr on separate pipes, no environment, no stdin. stdout is returned only on exit 0; on failure the whole stderr becomes AgentBridgeError.cliUnavailable. Only `feature list` and `agent status` output is parsed. start, teardown and sync output is thrown away.
- stderr is never merged into stdout before JSON parsing, which is correct after the 0.12.1 change that moved diagnostics to stderr. The problems are the pipe-drain order (deadlock) and showing raw stderr as the error.
- The CLI fallback can only be reached in practice through 'Force CLI'. Automatic mode waits on gRPC indefinitely because of grpc-swift defaults (waitsForConnectivity, unlimited retries, no deadline).
- `--no-summary` is a no-op with `--json` (help: 'text mode only'). `--title` is ignored when NAME is given (core feature.rs:1593-1604).
- The CLI's own agent IPC FeatureRecord (cli/src/agent.rs:229-271) uses flat tunnel_* fields. `feature list --json` serializes FeatureMetadata with a nested `tunnel` plus `runtime` and `default_agent`. The app's FeatureRecord matches neither shape exactly.
- Teardown recomputes the branch name from prefix + name (core feature.rs:1068-1069) rather than using the registry's branch_name. Prune does use derive_branch_prefix(feature).
- Calls the app makes are safe from interactive hangs: teardown, prune and the force-delete prompt all check Term::stdout().is_term(), and the app's stdout is a pipe, so they fail fast instead. core init's stdin.read_line (init.rs:747,802,2122) is not gated that way; any future GUI init must pass --yes and a null stdin.
- Scratch artifacts: captures in $SCRATCH/cli-contract/captures (synthetic_*.json are hand-built from core serde types); tests in .../cli-contract/macos/Tests/BranchBoxAppTests/{CLIContractTests,PipeDeadlockTests,TunnelSummaryTests}.swift; logs .../cli-contract/swift_test_contract.log, swift_test_grpc_200s.log, swift_test_pipe.log, swift_test_tunnel.log; help text .../cli-contract/help/.

## Findings

### CLI-01 [critical/bug] Automatic transport never falls back to the CLI when the gRPC agent is unreachable (hangs indefinitely)
With the default Automatic transport and nothing listening on the gRPC port, AgentBridge.listFeatures awaits client.list() forever. grpc-swift 1.27 defaults to CallStartBehavior.waitsForConnectivity with unlimited reconnect retries (ConnectionBackoff retries: .unlimited), and the app sets no call deadline and no fastFailure. As a result the `catch` that calls fetchFeaturesViaCLI is never reached, the spinner never stops, and start and teardown in Automatic mode hang the same way. Homebrew ships only branchbox/bb (no agent binary), so 'no agent' is the normal state. The CLI fallback can only be reached by manually choosing 'Force CLI'.

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:299-301 `ClientConnection.insecure(group:).withConnectionBackoff(maximum: .seconds(5)).connect(...)`; :167 `try await client.list(request)` (no CallOptions); grpc-swift Sources/GRPC/ClientConnection.swift:438 `callStartBehavior: CallStartBehavior = .waitsForConnectivity`; ConnectionBackoff.swift:85 `retries: Retries = .unlimited`. Test test09 (GRPC_WAIT_SECONDS=200): `CONTRACT| automatic transport, gRPC port closed: STILL WAITING after 200s (no fallback) elapsed=200.1s`

**Suggested fix:** Use .withCallStartBehavior(.fastFailure) and/or a short CallOptions timeLimit (e.g. 2s deadline) for list/status/start/teardown, or probe agent reachability once and cache the transport. Consider also probing the unix socket (~/.branchbox/agent/branchbox-agent.sock) that the CLI uses.

**Verifier:** confirmed — The code matches the claim. AgentBridge.swift:299-301 builds a ClientConnection that only sets withConnectionBackoff(maximum:). That builder (GRPCChannelBuilder.swift:137-139) changes maximumBackoff only, so retries stay at the ConnectionBackoff default `.unlimited` (ConnectionBackoff.swift:85). callStartBehavior defaults to `.waitsForConnectivity` (ClientConnection.swift:438). In that mode ConnectionManager.getHTTP2MultiplexerPatient hands back the readyChannelMuxPromise during transientFailure, so the call just waits. CallOptions timeLimit defaults to `.none` (CallOptions.swift:86), and the app passes no CallOptions.

I reproduced it in a scratch copy with a throwaway XCTest, with nothing listening on 127.0.0.1:50599 (nc -z exit=1). The Force CLI control returned at once: `transport=cliFallback count=1 elapsed=0.04s`. Automatic mode did not: `automatic listFeatures, port 50599 closed: STILL WAITING after 60.0s (no fallback)` and `automatic teardownFeature(nonexistent) ... STILL WAITING after 15.0s`. So the catch that calls the CLI fallback is never reached. Nothing listens on the default port 50515 here (lsof is empty).

On the Homebrew claim: the Cellar bin holds branchbox, bb and branchbox-local-vm, and none of them is an agent. `branchbox agent` offers only `status`. 'No gRPC agent' is therefore the normal state, and critical severity fits because this is the default transport.
Corrected: Minor: Homebrew installs branchbox, bb and branchbox-local-vm, not just branchbox/bb. None of them is a gRPC agent.

### CLI-02 [high/bug] CLICompat.run deadlocks when CLI output exceeds the pipe buffer (waitUntilExit before draining pipes)
run() calls process.waitUntilExit() and only then reads stdout and stderr. Once the child writes more than the pipe buffer (about 64 KiB), the child blocks on write and the app blocks on exit, so they wait on each other forever. Real `feature list --json` is about 2.2 KB per feature, and the registry keeps removed entries (main repo `--all`: 16,514 bytes for 8 entries). Roughly 30 features is enough to freeze the fallback. stderr is never drained on success either.

**Evidence:** CLICompat.swift:121-124 (`process.waitUntilExit()` then `readDataToEndOfFile()`). Sandbox with 46 features: `branchbox feature list --json | wc -c` -> 103913. PipeDeadlockTests: `CONTRACT| CLICompat.featureList with ~104KB stdout: HUNG > 30s (pipe deadlock) elapsed=30.0s`

**Suggested fix:** Read both pipes concurrently (readabilityHandler or background readDataToEndOfFile) before or while waiting, or use async Process wrappers; never block a cooperative-pool thread.

**Verifier:** confirmed — CLICompat.swift:121-124 calls waitUntilExit() and only afterwards reads stdout, and reads stderr only on failure. I drove CLICompat.featureList through BRANCHBOX_CLI_PATH scripts. The limit is exactly 64 KiB: stdout of 16000, 60000 and 65536 B decoded in about 0.2s, while 70000 B and 110000 B were `STILL BLOCKED after 10.0s`. stderr is a problem even on success: exit 0 with 60000 B of stderr worked, but 70000 B and 200000 B blocked.

With the real CLI, a sandbox with 34 minimal features gave `feature list --json` = 77,521 bytes (about 2.28 KB per feature), and `real CLI featureList --all ... STILL BLOCKED after 30.0s`.

One scope nuance: the app passes --all only when BRANCHBOX_SHOW_REMOVED=1 (AgentBridge.swift:22). The default list therefore needs about 29 non-removed features (active, degraded, failed_retained or orphaned), or more than 64 KiB of stderr, to deadlock. Removed entries piling up in the registry only matter with --all.
Corrected: The deadlock triggers once stdout or stderr passes 64 KiB, which is about 29 features at about 2.2-2.3 KB each. Without BRANCHBOX_SHOW_REMOVED=1 the app does not pass --all, so removed registry entries do not count toward that by default.

### CLI-03 [high/bug] A Finder-launched app cannot find the Homebrew CLI: /usr/bin/env runs with launchd PATH and no environment is passed
CLI resolution falls back to the bare name 'branchbox', run through /usr/bin/env using the app's inherited environment. GUI apps get launchd's PATH (/usr/bin:/bin:/usr/sbin:/sbin), which doesn't include /opt/homebrew/bin. Every CLI call fails unless BRANCHBOX_CLI_PATH is set or a CLI is embedded in the bundle. Even when the CLI is found, the tools it spawns (docker, devcontainer, cloudflared, sbx) also need a real PATH.

**Evidence:** CLICompat.swift:106-108 (`/usr/bin/env`, `[cliBinary] + arguments`, no `process.environment`), :131-147 resolveCLIBinary falls back to "branchbox". test07 (PATH set to launchd default): `CONTRACT| launchd PATH featureList THREW -> 'CLI fallback failed: env: branchbox: No such file or directory'` and same for detect.

**Suggested fix:** Resolve an absolute path (check /opt/homebrew/bin, /usr/local/bin, ~/.cargo/bin, or the login-shell PATH via `zsh -lc 'command -v branchbox'`). Set process.environment with an augmented PATH, and show 'CLI not found' as a distinct, actionable state.

**Verifier:** confirmed — CLICompat.swift:106-108 runs /usr/bin/env with [cliBinary]+args and sets no process.environment. resolveCLIBinary (132-148) falls back to the bare name "branchbox". I set PATH to /usr/bin:/bin:/usr/sbin:/sbin inside the test process with no BRANCHBOX_CLI_PATH. Result: `launchd PATH featureList: THREW -> 'CLI fallback failed: env: branchbox: No such file or directory'`, the same for detect, and the control with the full PATH worked. A shell check agrees: `env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env branchbox` exits 127. `launchctl getenv PATH` is empty, so launchd's default applies. The child-tool point also holds: on this machine docker is in /usr/local/bin, devcontainer is under ~/.nvm/..., and sbx is in /opt/homebrew/bin, and none of those are on the launchd PATH. The CLI finds them through PATH (which::which and Command::new).

Scope note: scripts/package-macos-app.sh:39,73-74 embeds a freshly built CLI at Contents/Resources/bin/branchbox unless SKIP_CLI=1. An app packaged that way does find a CLI, though it may be a different version from the user's Homebrew one. The missing PATH for child tools still applies to that build.
Corrected: An app packaged with scripts/package-macos-app.sh embeds its own CLI, so the 'CLI not found' failure hits swift-build, Xcode and SKIP_CLI builds. The missing docker/devcontainer/sbx PATH hits every GUI launch.

### DRIFT-01 [high/drift] Teardown branch semantics changed: the 'Delete branch' toggle being off still deletes, and unmerged branches make the app report failure after the worktree was already removed
The app sends --delete-branch only when the toggle is on, and nothing otherwise. The CLI now defaults to deleting the branch (config feature.teardown.delete_branch_by_default=true) and needs --keep-branch to keep it, so toggle-off still deletes. When the branch is unmerged and stdout is not a terminal, the CLI removes the worktree and marks the registry entry removed, then fails with exit 1 at the end. The app shows 'Teardown failed' with the full stderr, skips loadFeatures() on that path, and the list stays stale. The app also cannot pass --keep-branch, --force-delete-branch or --json.

**Evidence:** cli/src/commands/feature.rs:828-835 (default_delete_branch), :860-869 (`Branch '{}' could not be deleted without force`); core/src/config.rs:176-178 `default_teardown_delete_branch() -> true`; CLICompat.swift:44-53; FeatureListViewModel.swift:307-316 (no reload on error). Sandbox: `teardown exit=1 ... Error: Branch 'feature/beta' could not be deleted without force` then `beta removed` in `feature list --all`. test08: `CLICompat.teardownFeature(delta, all toggles off) THREW -> 'CLI fallback failed: 2026-...INFO ... Error: Branch 'feature/delta' could not be deleted without force...'` with `worktree exists=false`; epsilon with deleteBranch:false: branch list after teardown no longer contains feature/epsilon.

**Suggested fix:** Make the app's toggle three-way or map off to --keep-branch. Add a 'force delete unmerged branch' option (--force-delete-branch). Use `teardown --json` and treat 'worktree_removed=true, branch_deleted=false' as a partial success. Always reload the list after teardown, even when it fails.

**Verifier:** confirmed — The code matches: cli feature.rs:828-836 turns 'neither --keep-branch nor --delete-branch' into config.feature.teardown.delete_branch_by_default, which defaults to true (core/config.rs:176-178). feature.rs:860-870 bails on a non-TTY after workflow.teardown has already run. CLICompat.swift:44-53 sends only --force, --complete-spec and --delete-branch. FeatureListViewModel.swift:307-316 does not reload on error.

I reproduced it through CLICompat.teardownFeature(deleteBranch:false) in a scratch repo. For beta (merged): `OK -> returned (success)`, and `git branch` no longer listed feature/beta. For delta (unmerged): it threw a 10-line message of INFO logs ending in `Error: Branch 'feature/delta' could not be deleted without force...`. The worktree dir was gone, the registry showed delta as 'removed', and the feature/delta branch remained.

Extra finding: the gRPC transport honours delete_branch=false literally (agent/src/ops.rs:91-108), so the toggle means different things on the two transports.

Second extra finding: in a repo without a committed .devcontainer, even a fresh --minimal feature leaves an untracked .devcontainer/.branchbox.env. The app's non-force teardown then fails with 'Devcontainer/compose changes detected; rerun this command with --force'. I saw this on the first sandbox feature.


### BUG-04 [high/bug] (CLI-side) Teardown without --force still deletes modified/untracked user files via a remove_dir_all fallback
When `git worktree remove` refuses because of modified or untracked files, core falls back to fs::remove_dir_all on the worktree regardless of force_remove and reports success. The app's 'Force removal' toggle (off by default) therefore gives no protection for uncommitted work. The module dirty check only covers devcontainer and compose files.

**Evidence:** core/src/workflows/feature.rs:1203-1214 (`match self.git.remove(&worktree_path, force_remove) ... Err => fs::remove_dir_all ... "Worktree directory removed manually after git removal failed"`). Sandbox eta: `git status --short` showed ` M README.md` and `?? notes.txt`; `feature teardown eta --keep-branch` (no --force) -> `exit=0`, `Failed to remove worktree: ... contains modified or untracked files, use --force to delete it`, `Worktree directory removed manually after git removal failed`, and `ls .../sandbox/eta: No such file or directory`.

**Suggested fix:** In core, only fall back to remove_dir_all when force_remove is set; otherwise return WorktreeDirty or a similar error so GUI and CLI callers can confirm. In the app, check for a dirty worktree before teardown and warn.

**Verifier:** confirmed — The code matches: core/src/workflows/feature.rs:1201-1222. When `self.git.remove(&worktree_path, force_remove)` fails, the code calls fs::remove_dir_all whatever force_remove is, and sets worktree_removed=true. detect_module_dirty_changes (1924-2000) only flags .devcontainer, compose and docker-compose paths.

I reproduced it in a sandbox. In epsilon, `git status --short` showed ` M README.md`, `?? notes.txt` and `?? docs/`. Then `branchbox feature teardown epsilon --keep-branch` with no --force gave `exit=0`, `Worktree removed: yes`, `Failed to remove worktree: ... contains modified or untracked files, use --force to delete it`, and `Worktree directory removed manually after git removal failed`. Afterwards `ls .../sb/epsilon` said No such file or directory. The app's Force toggle is off by default, so this path is reachable from the app and loses user data. High severity is justified.


### DRIFT-02 [medium/drift] Tunnel fields: the app reads flat tunnel_status/provider/hostname keys but the CLI emits a nested tunnel{} object
FeatureRecord decodes tunnelStatus/tunnelProvider/tunnelHostname, but `feature list --json` (a flattened FeatureMetadata) emits `tunnel: {provider, hostname?, service_url?, status, descriptor?, instructions?, notes?, last_updated, removed_at?}`. Those three app fields are always nil. Because an active feature exists, tunnelSummary still returns a card that reads 'Unknown provider' / 'Unknown status', and 'Copy hostname' never works. The vocabularies also differ: CLI JSON uses serde snake_case (pending/active/manual/disabled), while gRPC uses the Display form (degraded/online/manual/disabled).

**Evidence:** CLICompat.swift:158-160; core/src/workflows/feature.rs:5351-5372 (FeatureTunnelState), 5322-5340 (Display: Pending=>"degraded", Active=>"online"). test03: `tunnelStatus=nil tunnelProvider=nil tunnelHostname=nil rawTunnel.status=Optional(disabled) rawTunnel.provider=Optional(cloudflared)`; TunnelSummaryTests: `tunnelSummary from real list: provider='Unknown provider' status='Unknown status' hostname=nil feature=prine`

**Suggested fix:** Add a nested `tunnel: TunnelRecord?` (provider, hostname, serviceUrl, status, notes, instructions, lastUpdated) to FeatureRecord. Normalize status words across CLI and gRPC, and only show the tunnel card when a tunnel object exists and its status is not disabled.

**Verifier:** confirmed — Real `feature list --json` from main (read-only) has a nested `tunnel: {provider: 'cloudflared', status: 'disabled', notes, last_updated}` and no flat tunnel_* keys. I decoded it with the app's own decoder: prine `tunnelStatus=nil tunnelProvider=nil tunnelHostname=nil`. Feeding those records into FeatureListViewModel.features gave `tunnelSummary provider='Unknown provider' status='Unknown status' hostname=nil feature=prine`. That card is shown by HomeView:146 and StatusMenuView:78, and copyTunnelHostname always alerts 'No tunnel hostname'.

The vocabulary point is right too. FeatureTunnelStatus is serde snake_case (pending/active/manual/disabled), and its Display maps Pending to 'degraded' and Active to 'online' (core feature.rs around 5320-5340). The gRPC mapping uses `state.status.to_string()` and falls back to 'none' when there is no tunnel (agent/src/grpc.rs:193-206). The gRPC path does fill the flat proto fields; the nil fields happen only on the CLI path.


### DRIFT-03 [medium/drift] Module status vocabulary mismatch: the CLI emits success/skipped/failed but the app counts 'ok'
moduleSummary counts only 'ok' and 'failed', so every real feature shows '0 ok'. Module pills are green only when status == 'ok', so successful modules render orange. gRPC uses the same to_string() wording ('success'), so this is wrong on both transports.

**Evidence:** FeatureModels.swift:45 `let ok = collapsed["ok"] ?? 0`; FeatureDetailView.swift:87; core/src/workflows/feature.rs:52-69 (ModuleStatus success/skipped/failed); agent/src/grpc.rs:281,291 `status: outcome.status.to_string()`. test03: `moduleSummary='0 ok' moduleStatuses=["devcontainer=success", "compose=success", "specs=success", "tunnel=skipped"]`

**Suggested fix:** Model ModuleStatus as an enum (success/skipped/failed plus unknown) and summarize as 'N ok / N skipped / N failed'.

**Verifier:** confirmed — FeatureModels.swift:45 counts only collapsed["ok"], and FeatureDetailView.swift:87 colours a pill green only when status == "ok". Core ModuleStatus is serde lowercase with Display success/skipped/failed (core feature.rs:50-69). gRPC uses outcome.status.to_string() (agent/src/grpc.rs:281,291), so both transports send 'success'. Decoding the real list gave `moduleSummary='0 ok' modules=["devcontainer=success", "compose=success", "specs=success", "tunnel=skipped"]` for prine and remotion.


### DRIFT-04 [medium/drift] New feature statuses (degraded, failed_retained, orphaned) are treated as 'removed' by the UI
Status is an untyped String. Everything other than 'active' falls under the Removed filter, gets a gray pill, and is skipped by activeFeature. Labels come from .capitalized ('Failed_Retained'). `feature list` without --all now returns retained and orphaned entries too ('retained and orphaned features are shown by default'), so they show up as 'removed' rows in the Active view's data.

**Evidence:** FeaturesView.swift:66-67, FeatureListViewModel.swift:144, FeatureModels.swift:30-32; core/src/workflows/feature.rs:5270-5277 (FeatureStatus), 1278-1302 (orphaned/degraded assignment); help: `--status <STATUS>  Filter by status (active, degraded, failed_retained, orphaned, removed)`. test04: `status=failed_retained label='Failed_Retained' activeFilter=false removedFilter=true`; synthetic sbx-demo `status=degraded ... isActiveForUI=false`

**Suggested fix:** Add a FeatureStatus enum with an unknown case, with per-status color, label and filter (e.g. Active / Needs attention / Removed), and add actions for retained runtimes (--reuse-runtime) and prune.

**Verifier:** confirmed — FeatureStatus covers Active, Degraded, FailedRetained, Orphaned and Removed, serialized snake_case (core feature.rs:5268-5276). list_features turns statuses into Orphaned or Degraded at runtime (1278-1302). The CLI help says `--all  Include removed features (retained and orphaned features are shown by default)`.

The app checks only `== "active"`: FeaturesView.swift:66-67 (Active and Removed filters), :81 (pill colour), FeatureListViewModel.swift:144 (activeFeature), and FeatureModels.swift:30-32 (.capitalized label). My test printed `failed_retained label='Failed_Retained' activeFilter=false removedFilter=true`, and the same false/true for degraded and orphaned. I did not get a real degraded or orphaned entry: the default container runtime's exists/ready checks never trigger it. The result therefore rests on the code plus string evaluation, which is enough to show the mapping.


### DRIFT-05 [medium/drift] Fractional-second timestamps rely on JSONDecoder .iso8601 accepting fractions (works on macOS 26; likely fails on the declared macOS 13/14 minimum)
All real timestamps include fractional seconds, some with nanosecond precision. On this machine (Darwin 25.5 / macOS 26, Swift 6.2 Foundation) .iso8601 decodes them. The package declares .macOS(.v13), and the older Foundation .iso8601 strategy (ISO8601DateFormatter with .withInternetDateTime only) is widely reported to reject fractional seconds. That would make the whole [FeatureRecord] decode throw and the CLI list fail. I could not verify this on macOS 13 or 14.

**Evidence:** CLICompat.swift:15 `decoder.dateDecodingStrategy = .iso8601`; Package.swift `.macOS(.v13)`; captures: `"updated_at": "2026-03-17T03:37:57.979509Z"`, `created 2025-11-10T04:29:00.768535795Z`; test01: `app-decoder main_feature_list_all.json: DECODED 8 records`; test02: `control fractional updated_at decodes: true` (on macOS 26 only).

**Suggested fix:** Use a custom date strategy that tries .withFractionalSeconds and then plain internet date-time (the app already builds such a formatter in FeatureModels.swift:142-146 for gRPC), or raise the deployment target.

**Verifier:** partially_confirmed — The facts check out: CLICompat.swift:15 uses .iso8601; Package.swift declares .macOS(.v13) and the packaging Info.plist has LSMinimumSystemVersion 13.0; real timestamps are fractional, e.g. `2026-03-17T03:37:57.979509Z`. On this machine (macOS 26.5.1), JSONDecoder .iso8601 and Date.ISO8601FormatStyle() both parse the 6- and 9-digit fractions. The legacy `ISO8601DateFormatter` with `.withInternetDateTime` returned false for both fractional strings and true only for `...57Z`. The pre-Sonoma Swift Foundation overlay's .iso8601 strategy used exactly that formatter (formatOptions = .withInternetDateTime), so the decode almost certainly throws on macOS 13 and breaks the whole [FeatureRecord] list. I could not run macOS 13, 14 or 15, and I can't say in which release Date.ISO8601FormatStyle became lenient, so the 14/15 behaviour is unverified. Note for the fix: the fractional formatter in FeatureModels.swift:142-146 rejects `2026-03-17T03:37:57Z` (it returned nil), so the fallback chain the finding suggests is needed. The gRPC parse path has the opposite problem with whole-second timestamps.
Corrected: The macOS 13 failure is well supported by the code trace: the old overlay's .iso8601 used ISO8601DateFormatter(.withInternetDateTime), and that formatter rejects fractional seconds here too. macOS 14/15 are unknown. It works on macOS 26.

### BUG-05 [medium/bug] Error surfacing: the whole stderr (tracing INFO logs) becomes the alert; launch and cwd failures are reported as 'CLI not runnable'
Since 0.12.1, diagnostics go to stderr. On non-zero exit the app shows all of stderr, so users see a block of INFO lines with the actual `Error:` line last. The shell captures include ANSI escape codes; it's unknown whether they appear under a GUI. A missing working directory makes Process.run() throw, which is reported as 'CLI not runnable: The file “X” doesn’t exist.' The default workspace is FileManager.currentDirectoryPath, which is '/' for a Finder launch, and the CLI answers 'Error: Validation error: Not a git repository: /'.

**Evidence:** CLICompat.swift:115-127; AgentBridge.swift:19-21. test08 alert text begins `CLI fallback failed: 2026-10-01T23:01:16.517136Z  INFO worktree_core::modules::specs: Created backlog directory ...` and ends `Error: Branch 'feature/delta' could not be deleted without force...`; captures/sandbox_start_alpha.stderr contains `[2m...[32m INFO[0m`; cwd_probe: `run() threw: The file “milestone2” doesn’t exist.`; `branchbox feature list --json --repo /` -> `Error: Validation error: Not a git repository: /`

**Suggested fix:** Show only the trailing `Error:` / `Caused by:` block (or set RUST_LOG=warn and NO_COLOR=1 in the child env) and keep the full log behind a 'details' disclosure. Validate the workspace (exists, git repo) before spawning, with dedicated messages.

**Verifier:** confirmed — CLICompat.swift:115-127 puts all of trimmed stderr into the error, and AgentBridgeError adds 'CLI fallback failed: '. In my test the unmerged-branch teardown alert had 10 lines: 9 INFO lines, then `Error: Branch 'feature/delta' could not be deleted without force...`. A missing cwd gave `CLI fallback failed: CLI not runnable: The file “does-not-exist-dir” doesn’t exist.`. Workspace '/' gave `CLI fallback failed: Error: Validation error: Not a git repository: /`. AgentBridge.swift:19-21 falls back to currentDirectoryPath, and FeatureListViewModel.swift:84 uses it when no default is stored.

I also settled the ANSI question the finding left open. tracing-subscriber 0.3.23 (cli/src/main.rs:77-83) emits ANSI codes whenever NO_COLOR is unset, even when stderr is a pipe: `branchbox feature start ... 2>&1 >/dev/null | cat -v` showed `^[[2m...^[[32m INFO^[[0m`. `swift test` sets NO_COLOR=1, which is why my test alerts had no escape codes (hasESC=false). A Finder-launched app normally has no NO_COLOR, so its alerts would very likely include raw escape sequences.
Corrected: ANSI codes do reach piped stderr unless NO_COLOR is set. tracing-subscriber does not check for a TTY. A GUI launch without NO_COLOR will therefore very likely show escape codes in alerts.

### DRIFT-06 [medium/drift] feature start: the CLI rewrites non-slug names, ignores --title when a name is given, and the app throws away the JSON summary
The app passes the raw user name and optional title. Core uses the name if it is a valid slug, otherwise slugs it with filler-word removal ('Epsilon Feature' -> 'epsilon', 'My Cool Feature' -> 'my-cool'). The title is used only when no name is given, so the app's Title field has no effect. The app discards the start JSON (work_feature, worktree_path, warnings, skipped_modules, runtime, tunnel, default_agent, generated_at), so it notifies '<raw name> is ready' and never learns the real slug or the warnings. Tearing down with the raw name fails.

**Evidence:** core/src/workflows/feature.rs:1592-1614 (resolve_work_feature); CLICompat.swift:19-39 (`_ = try run(...)`); FeatureListViewModel.swift:249. test08: `registry names after 'Epsilon Feature' start: ["epsilon", "syncme"]`, `teardown using the app's raw name 'Epsilon Feature' THREW -> 'CLI fallback failed: Error: Invalid feature name: Epsilon Feature'`; `branchbox name generate "OAuth Integration"` -> `oauth`.

**Suggested fix:** Decode the start JSON (StartSummary) and use its work_feature, warnings and skipped_modules. Preview the slug with `name generate`/`name validate` (suggestedFeatureName currently returns "" at FeatureListViewModel.swift:158). Send either a name or a title, not both.

**Verifier:** partially_confirmed — What holds: resolve_work_feature (core feature.rs:1592-1619) keeps a valid slug, otherwise slugs the name, and uses the title only when no name is given. The specs title comes from work_feature (specs.rs:227), not from the request title. The app always requires a name (the StartFeatureSheet Start button is disabled when the name is empty, and startFeature guards on it), so the Title field never has any effect. CLICompat.swift:39 and AgentBridge.swift:222 throw away the start summary.

Through CLICompat.startFeature I got 'Epsilon Feature' -> registry `('epsilon','feature/epsilon','active')`. `name generate` gives oauth, my-cool and epsilon.

What is overstated: (1) The Options sheet normalizes the name before starting (StartFeatureSheet.swift:65-85, lowercase plus dashes, so 'Epsilon Feature' becomes 'epsilon-feature', which is valid and kept). Only quick start (HomeView.swift:224-226, StatusMenuView.swift:147-148) sends raw names, so the wrong 'is ready' notification only appears there. (2) Tearing down with the raw name fails at the CLI (`Error: Invalid feature name: Epsilon Feature`), but the app can't trigger that: teardown always passes feature.workFeature from the reloaded registry list (FeatureListViewModel.swift:294-308).
Corrected: Rewritten slugs, the ignored title and the discarded start JSON are real. Raw names reach the CLI only from quick start, because the Options sheet normalizes them. The app never tears down with the raw name; it uses the registry's work_feature.

### DRIFT-07 [medium/drift] Teardown omits --branch-prefix; core recomputes the branch from the prefix (not the registry), so custom-prefix branches are silently left behind
The app accepts a branch prefix at start but does not pass it to teardown. Core builds the branch name as `<config prefix>/<name>` instead of reading the registry's branch_name. It then tries to delete feature/<name>, fails quietly (as a warning), exits 0, and the app reports success.

**Evidence:** CLICompat.swift:44-53 (no --branch-prefix); core/src/workflows/feature.rs:1068-1069. Sandbox: start zeta `--branch-prefix spike` -> `spike/zeta`; app-style teardown -> `exit=0`, `Branch kept: feature/zeta`, `Failed to delete branch 'feature/zeta' ... not found`; `git branch` still lists `spike/zeta`.

**Suggested fix:** Pass --branch-prefix derived from FeatureRecord.branchName. Better, fix core to use the recorded branch_name.

**Verifier:** confirmed — Core teardown computes branch_name = build_branch_name(branch_prefix or the config prefix, work_feature) (core feature.rs:1067-1069) and never reads the recorded branch_name. CLI run_teardown fills in the config prefix too (cli feature.rs:840). CLICompat.teardownFeature (44-53) passes no --branch-prefix, and the gRPC path leaves request.branchPrefix empty. I started zeta through CLICompat with branchPrefix 'spike' and got `+ spike/zeta`. App-style teardown `branchbox feature teardown zeta --repo .` returned `exit=0`, `Branch deleted: no`, `Branch kept: feature/zeta`, and `Failed to delete branch 'feature/zeta': ... branch 'feature/zeta' not found`. `git branch` still lists `spike/zeta`. The non-TTY bail does not fire because feature/zeta doesn't exist, so the app reports success.


### GAP-08 [medium/missing_feature] Current CLI JSON and flags the GUI ignores but would want
Top-level list keys present but ignored: base_branch, color, compose_project_name, created_at, default_agent{status,label,command,detail,followup}, env_path, last_commit, last_summary_rendered_at, pr_number, removed_at, runtime{provider (container|sbx|in-guest|local-vm), runtime_id, published_ports[{host,runtime}], container_id, workspace_folder, container_user, config_path, in_guest, version}, tunnel{...}. module_outcomes[].duration_ms/notes/forced/recorded_at are also ignored. Start JSON is fully ignored (warnings, skipped_modules, mode, runtime, default_agent, prompt_bridge_enabled). Teardown --json (worktree_removed, branch_deleted, runtime_teardown{verified,residue_free,residue}, adapter_cleanup_warnings, module_reports, warnings) and exec --json ({exit_code, stdout, stderr}) are unused. Start flags the app doesn't expose: --base, --runtime, --runtime-manifest, --no-worktree, --default-prompt, --devcontainer-reuse, --keep-runtime-on-failure, --reuse-runtime, --allow-container, --telemetry. Commands the app doesn't use: feature exec / exec-provider / dispatch-tool, feature prune and prune, tunnel open/remove, devcontainer up/exec/down/build/read-configuration, name generate/validate, and list --status.

**Evidence:** test03: `main_feature_list.json: keys present but IGNORED by app = ["base_branch", "color", "compose_project_name", "created_at", "default_agent", "env_path", "last_commit", "last_summary_rendered_at", "removed_at", "runtime", "tunnel"]`; synthetic adds `pr_number`. Help captures in .../cli-contract/help/*.txt; core/src/runtime/mod.rs:77-104, 130-133; cli/src/commands/feature.rs:1216-1235 (start JSON payload).

**Suggested fix:** Change FeatureRecord to match core FeatureMetadata (ideally generate the schema or share fixtures from core). Show the runtime provider with host port links (http://localhost:<host>), the last commit, the base branch and the default-agent plan, and add retained-runtime and prune actions.

### DRIFT-09 [low/drift] devcontainer sync: no --json, exit 0 on per-worktree failures, repo-wide only, active status only
The app always reports 'Sync completed' on exit 0. The CLI prints per-worktree failures to stdout (which the app discards) and still returns Ok. Per-feature 'Sync devcontainer' buttons actually sync every Active worktree, using that feature's strategy. Degraded features are skipped. When the main repo has no .devcontainer it exits 1 with a multi-line stderr.

**Evidence:** cli/src/commands/devcontainer.rs:540-651 (562 `No active feature worktrees found`, 585 `Failed to initialize devcontainer module`, 627-650 errors printed then `Ok(())`); FeatureDetailView.swift:119, FeaturesView.swift:53, HomeView.swift:180; FeatureListViewModel.swift:271-272. Sandbox: `sync exit=1 ... Error: Failed to initialize devcontainer module ... Devcontainer directory not found`.

**Suggested fix:** Add `devcontainer sync --json` (per-worktree results) and a --feature filter in the CLI, and parse it in the app. Until then, scan stdout for '✗ failed'.

**Verifier:** confirmed — In cli/src/commands/devcontainer.rs:540-651, sync filters to FeatureStatus::Active (556-559). It prints 'No active feature worktrees found' and returns Ok (561-563). It returns an error only when module init fails (582-585). Per-worktree errors are printed to stdout and still return Ok(()) (627-650). `devcontainer sync --help` has no --json or --feature.

In the sandbox, with an active feature and no .devcontainer, the run gave `sync exit=1` and the stderr was `Error: Failed to initialize devcontainer module / Caused by: Validation error: Devcontainer directory not found`. I then forced a per-worktree failure by making eta/.devcontainer read-only. That gave `sync exit=0` with stdout `eta ... ✗ failed: IO error: Permission denied`, `✓ Successfully synced 0 feature worktree(s)`, and `1 error(s) occurred`. The app would show 'Sync completed' (FeatureListViewModel.swift:271-272). The per-feature buttons (FeatureDetailView.swift:118, FeaturesView.swift:53, HomeView.swift:180) pass only --path <workspace> plus that feature's strategy, so they sync every active worktree.


### DRIFT-10 [low/drift] Agent status fallback: a pointless --help probe, and a missing daemon is silently reported as 'CP pending'
Every CLI refresh spawns `branchbox --help`. `help.contains("agent")` is always true. It then runs `agent status --json`, which needs the agent unix socket. With no daemon the CLI exits 1 ('failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock'), the error is swallowed, and all-false defaults are returned, so the UI cannot tell 'agent offline' from 'control plane disconnected'. The AgentStatusRecord shape matches the serde AgentStatus struct; a synthetic payload decoded fine.

**Evidence:** CLICompat.swift:56-83 (61 `help.contains("agent status") || help.contains("agent")`); cli/src/commands/agent.rs:25-31; cli/src/agent.rs:145-163 (socket path), 294-312 (AgentStatus). captures/main_agent_status.stderr: `Error: failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock ... No such file or directory`; test05: `agentStatusOrDefault (no daemon) -> configured=false connected=false lastError=nil`.

**Suggested fix:** Drop the --help probe. Return a tri-state (agent unreachable / CP disconnected / CP connected) and show the CLI's error text.

**Verifier:** confirmed — CLICompat.swift:57-85 runs `--help` on every CLI refresh (called from fetchFeaturesViaCLI). `help.contains("agent")` is always true because root help lists `agent  Agent and control-plane helpers`. Read-only `branchbox agent status --json` exits 1 with `Error: failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock ... No such file or directory`. My test of agentStatusOrDefault returned `configured=false connected=false lastError=nil`, so the error is swallowed. The UI's 'CP pending' label (FeatureListViewModel.swift:178-180) looks the same whether the agent is offline or the control plane is disconnected. The transport badge does at least show 'CLI fallback'. One citation is wrong: the serde AgentStatus struct is at cli/src/agent.rs:322-339, not 294-312 (294-312 is StartFeatureSummary/TeardownFeatureSummary). Its fields match AgentStatusRecord.
Corrected: The AgentStatus struct is at cli/src/agent.rs:322-339, not 294-312.

### BUG-11 [low/bug] (CLI-side) stdout is polluted in --json mode (prompt truncation warning; dirty-worktree teardown text)
`feature start --json` with a prompt over 2000 characters prints a warning to stdout before the JSON, which makes the output invalid. `feature teardown --json` prints the dirty-worktree banner to stdout. The app doesn't parse these today, but any GUI that adopts start or teardown JSON would break.

**Evidence:** cli/src/commands/feature.rs:453 `println!("⚠️  Prompt truncated to {PROMPT_MAX_CHARS} characters before storage.")`; :1011-1018 handle_dirty_worktree println!. captures/sandbox_start_gamma_longprompt.json first line `⚠️  Prompt truncated to 2000 characters before storage.` -> python json.load `JSONDecodeError: Expecting value: line 1 column 1`; captures/sandbox_teardown_alpha.json contains `⚠️  Detected devcontainer/compose changes inside ...`.

**Suggested fix:** Route these to stderr (eprintln!) or into the JSON `warnings` array whenever --json is set.

**Verifier:** partially_confirmed — The start case holds. cli feature.rs:453 uses println! for the truncation warning. In the sandbox, `feature start theta --json --prompt <2100 chars>` exited 0, and stdout began `⚠️  Prompt truncated to 2000 characters before storage.` followed by `{`. python json.load failed with `JSONDecodeError: Expecting value: line 1 column 1`.

The teardown case is weaker than stated. handle_dirty_worktree (1005-1023) does println! the banner to stdout, but on a non-TTY it bails right after. `feature teardown theta --json` exited 1 with only the banner on stdout and `Error: Devcontainer/compose changes detected...` on stderr. No JSON payload is produced on that path, so a GUI that checks the exit code first won't fail to parse a success result. It is human text on stdout during an error, not corrupted success JSON. The app ignores start stdout and doesn't pass --json to teardown, so nothing breaks today.
Corrected: The start case corrupts the JSON on a successful exit 0. The teardown banner only appears on the exit-1 dirty-module error path, where no JSON is printed at all.

### ARCH-12 [low/architecture] Non-zero exit discards stdout, which loses JSON from commands that report failure in-band (feature exec)
`feature exec --json` prints {exit_code, stdout, stderr} and then exits 1 when the inner command fails. run() throws on non-zero exit without returning stdout, so an exec integration would lose the payload. stdin is never set (inherited). Calls the app makes today are gated on whether stdout is a terminal, but core init reads stdin without that check, so a future init call must pass --yes and use a null stdin.

**Evidence:** cli/src/commands/feature.rs:392-407 (prints JSON then `bail!("Runtime command exited with status {}")`); CLICompat.swift:123-127; core/src/workflows/init.rs:747,802,2122 `std::io::stdin().read_line`. captures/sandbox_exec_alpha_fail.json `{"exit_code": 3, "stdout": "out\n", "stderr": "err\n"}` with `exec fail exit=1`.

**Suggested fix:** Return (exitCode, stdout, stderr) from run() and let each caller decide. Set process.standardInput = FileHandle.nullDevice.

### UX-13 [low/ux] (Observed in an uninitialized repo) a fresh feature can't be torn down without --force because BranchBox's own .devcontainer/.branchbox.env counts as dirty
In a repo without a .gitignore entry, minimal-mode start writes .devcontainer/.branchbox.env. Teardown's module dirty check then refuses without a terminal: 'Devcontainer/compose changes detected; rerun this command with --force'. Repos set up by `branchbox init` probably ignore that file (the core tests use such a .gitignore), so this mainly affects arbitrary folders picked in the app.

**Evidence:** captures/sandbox_teardown_alpha.stderr `Error: Devcontainer/compose changes detected; rerun this command with --force to proceed.`; `find .devcontainer` -> `.devcontainer/.branchbox.env`; cli/src/commands/feature.rs:1020-1023; core/src/workflows/feature.rs:6365 (test .gitignore).

**Suggested fix:** Exclude BranchBox-managed files from the dirty check in core. In the app, offer 'retry with force' when this specific error appears.

### TEST-14 [low/test_gap] The app's only CLI decode test uses the pre-0.5 contract (flat tunnel_status, whole-second timestamps)
The existing fixture hides the tunnel, module-status, status-enum and fractional-date drift. There are no fixtures captured from the real CLI and no test of CLICompat.run behavior (large output, non-zero exit, PATH).

**Evidence:** macos/Tests/BranchBoxAppTests/BranchBoxAppTests.swift:5-25 (`"tunnel_status": "none"`, `"updated_at": "2024-02-01T12:34:56Z"`).

**Suggested fix:** Check in golden payloads produced by the CLI (or by core serde tests) and decode them in Swift CI. The scratch CLIContractTests.swift can serve as a starting point.

### UX-15 [low/ux] runDetect blocks the main thread; detect output is plain text with no --json
runDetect calls CLICompat.detectProject synchronously inside a Task that inherits @MainActor, so the UI freezes while the subprocess runs (about 0.1 s here; longer on slow repos). `detect` has no --json and prints a Rust Debug-formatted stack (`Stack: Rust`) plus emoji, which the app shows raw.

**Evidence:** FeatureListViewModel.swift:352-365; cli/src/main.rs:101-129; captures/main_detect.txt `📦 BranchBox Configuration ... Stack: Rust ... Enabled modules: 4`.

**Suggested fix:** Run it in Task.detached. Add `detect --json` (stack, adapter, modules, warnings) to the CLI.

## Experiments

- [pass] **Enumerate app CLI invocations** — `grep -rn -E 'Process\(|arguments|CLICompat\.|--json|executableURL' macos/Sources macos/Tests (excluding Generated)`
  - 7 argv shapes, all built in CLICompat.swift (lines 5, 20, 44, 60, 62, 88, 99), all run via run() at :103-128; the only other Process is /usr/bin/open -a Terminal (FeatureListViewModel.swift:428-431)

- [pass] **CLI help for every invoked subcommand** — `branchbox {feature list|start|teardown|exec|prune, agent status, devcontainer sync, detect, name generate, prune, tunnel} --help > cli-contract/help/*.txt`
  - All app flags still exist. New: list --status; start --base/--no-worktree/--devcontainer-reuse/--keep-runtime-on-failure/--reuse-runtime/--default-prompt/--allow-container/--runtime/--runtime-manifest; teardown --keep-branch/--force-delete-branch/--json ('Keep the git branch ... (default is to delete it)')

- [pass] **Read-only captures in main checkout** — `cd main && branchbox feature list --json; feature list --all --json; detect; detect --path .; agent status --json; name generate "OAuth Integration"`
  - list: 2 records (prine, remotion active), stderr empty; --all: 8 records; detect: 'Stack: Rust ... Enabled modules: 4'; agent status exit=1 'failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock'; name generate -> 'oauth'

- [pass] **feature start in disposable repo (exact app argv)** — `cd sandbox/demo && /usr/bin/env branchbox feature start alpha --repo $R --json --no-summary --minimal --skip-module tunnel </dev/null`
  - exit=0; clean JSON on stdout (keys: adapter, branch_name, color, compose_project_name, default_agent, env_path, feature_url, generated_at, mode, module_outcomes, prompt_bridge_enabled, prompt_seed, runtime{provider:container}, skipped_modules, tunnel, warnings, work_feature, worktree_path); tracing INFO logs with ANSI codes on stderr

- [fail] **feature start long prompt in JSON mode** — `feature start gamma ... --json --prompt <2100 x chars>`
  - stdout line 1: '⚠️  Prompt truncated to 2000 characters before storage.' -> json.load: 'Expecting value: line 1 column 1'

- [partial] **feature exec --json** — `feature exec alpha --repo $R --json -- echo hi ; ... -- sh -c 'echo out; echo err >&2; exit 3'`
  - {"exit_code":0,"stdout":"hi\n","stderr":""} exit 0; failing inner command: JSON {exit_code:3,...} on stdout, then 'Error: Runtime command exited with status 3' and exit=1. A trailing '--json' after the command is passed to the command ('hi --json --repo ...')

- [fail] **Teardown with app defaults on an unmerged branch** — `commit on feature/beta; feature teardown beta --repo $R </dev/null`
  - exit=1 'Error: Branch 'feature/beta' could not be deleted without force'; worktree dir gone; `feature list --all` -> 'beta removed'; branch kept

- [fail] **Teardown --json on a fresh minimal feature (no .gitignore)** — `feature teardown alpha --repo $R --json`
  - exit=1; stdout '⚠️  Detected devcontainer/compose changes ... • .devcontainer/' (not JSON); stderr 'rerun this command with --force'

- [pass] **Teardown --force --json** — `feature teardown alpha --repo $R --force --json`
  - exit=0 {work_feature, branch_name, worktree_removed:true, branch_deleted:true, adapter_cleanup_warnings, module_reports, runtime_teardown{provider:container, verified:true, residue_free:true}, warnings}

- [fail] **Teardown of a custom-prefix feature (app omits --branch-prefix)** — `feature start zeta --branch-prefix spike ...; feature teardown zeta --repo $R`
  - exit=0; 'Failed to delete branch 'feature/zeta' ... not found'; git branch still lists spike/zeta

- [fail] **Non-forced teardown with user changes** — `echo work > eta/notes.txt; edit README.md; feature teardown eta --repo $R --keep-branch`
  - exit=0; 'Failed to remove worktree: ... contains modified or untracked files' + 'Worktree directory removed manually after git removal failed'; eta directory gone (user changes lost)

- [partial] **devcontainer sync semantics** — `devcontainer sync --path $R --strategy copy (no active; then 1 active, no .devcontainer)`
  - no active: exit 0 'No active feature worktrees found'; with active: exit 1 'Failed to initialize devcontainer module ... Devcontainer directory not found'

- [pass] **prune without a terminal** — `branchbox prune --repo $R </dev/null; prune --yes --delete-branch`
  - no --yes: exit 1 'Refusing to prune in non-interactive mode without --yes'; --yes: '✓ Pruned 46 feature(s).'

- [pass] **Swift decode of captured payloads with the app's decoder** — `cd scratch/macos && swift test --filter CLIContractTests (test01/test02)`
  - All 5 payloads DECODED (2, 8, 3, 3, 3 records); fractional-second dates decode on macOS 26

- [fail] **Field drift through FeatureViewData(cli:)** — `swift test --filter CLIContractTests (test03/test04)`
  - ignored keys = [base_branch, color, compose_project_name, created_at, default_agent, env_path, last_commit, last_summary_rendered_at, removed_at, runtime, tunnel] (+pr_number); always nil = [tunnel_hostname, tunnel_provider, tunnel_status]; moduleSummary='0 ok' for all-success; failed_retained -> label 'Failed_Retained', removedFilter=true

- [pass] **Real CLICompat.featureList end to end** — `test06 (spawns /usr/bin/env branchbox ...)`
  - sandbox: 1 / 4 records; main: 2 records

- [fail] **launchd PATH** — `test07: setenv PATH=/usr/bin:/bin:/usr/sbin:/sbin; CLICompat.featureList / detectProject`
  - 'CLI fallback failed: env: branchbox: No such file or directory' (both)

- [fail] **Start and teardown through app code** — `test08: CLICompat.startFeature + teardownFeature (all toggles off)`
  - delta (unmerged): THREW with the whole INFO log + 'Error: Branch 'feature/delta' could not be deleted without force', worktree removed; 'Epsilon Feature' -> registry 'epsilon'; teardown('Epsilon Feature') -> 'Invalid feature name'; teardown(epsilon, deleteBranch:false) ok and branch deleted anyway

- [fail] **Automatic transport with gRPC unreachable** — `GRPC_WAIT_SECONDS=200 swift test --filter CLIContractTests/test09`
  - 'STILL WAITING after 200s (no fallback) elapsed=200.1s'

- [fail] **Pipe deadlock with large stdout** — `create 45 sandbox features (list = 103913 bytes); swift test --filter PipeDeadlockTests`
  - 'CLICompat.featureList with ~104KB stdout: HUNG > 30s (pipe deadlock)'

- [fail] **Tunnel summary from real payload** — `swift test --filter TunnelSummaryTests`
  - provider='Unknown provider' status='Unknown status' hostname=nil feature=prine

- [fail] **Process with a nonexistent working directory** — `swiftc cwd_probe.swift && ./cwd_probe (cwd=/workspaces/milestone2)`
  - run() threw: The file “milestone2” doesn’t exist.

- [partial] **Agent status decode (synthetic) and fallback** — `test05 with captures/synthetic_agent_status.json; CLICompat.agentStatusOrDefault`
  - synthetic DECODED (configured=true, ack=12); no daemon -> silent defaults configured=false connected=false lastError=nil

## Open questions
- Does the `.iso8601` JSONDecoder strategy reject fractional-second timestamps on macOS 13/14 (the app's declared minimum)? It decodes them on macOS 26, but I could not test older Foundation here.
- Do tracing ANSI escape codes appear in stderr when the CLI is spawned from a GUI app? They appeared in shell captures but not under `swift test`. tracing-subscriber's color detection depends on the environment.
- Is an agent binary meant to ship alongside the Homebrew CLI? Only branchbox, bb and branchbox-local-vm are in /opt/homebrew/bin. If not, the gRPC-first design means the default app experience always hangs (CLI-01).
- Should the 'Delete branch' default follow the project config (delete_branch_by_default=true) or should the GUI default to --keep-branch? Product intent is unclear.
- Degraded and orphaned statuses can only be produced by sbx/local-vm/in-guest runtimes. My examples for those are synthetic (no Docker Sandboxes or VM here), so the real JSON shape of runtime.published_ports, in_guest and version is inferred from the serde types.
- The non-forced teardown deletion of uncommitted files (BUG-04) is in core, not the app. Does it also affect the agent's gRPC and IPC teardown? Likely, since they share FeatureWorkflow::teardown, but I didn't test it.