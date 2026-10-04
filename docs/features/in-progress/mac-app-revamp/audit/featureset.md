# Audit area: featureset

## Summary
BranchBox v0.13.4 (Cargo.toml:10, `branchbox --version` = 0.13.4) exposes 9 top-level command groups and 31 leaf subcommands. A recursive `--help` dump is at scratchpad/featureset/help.txt. The CLI has grown a lot since v0.4. It now has pluggable runtimes (container/sbx/local-vm/in-guest, 0.11–0.13), `feature exec`/`exec-provider`/`dispatch-tool`, `prune` (0.9.3), five registry statuses (0.12.0), runtime identity and host-port mappings in the registry (0.11.0), teardown residue evidence via `--json` (0.13.0), devcontainer up/exec/down/build (0.7.0), tunnel open/remove (0.4.1), interactive 1Password/Cloudflare prompts in init (0.9.0), and failed-runtime retention/retry (0.12.0).

The macOS app (3,083 non-generated Swift lines) has barely changed since 2025-11-14. Its gRPC proto has not changed since 2025-11-10. The app covers about 6 of the roughly 50 user-facing capabilities fully: title, branch prefix, skip-modules, complete-spec toggle, devcontainer-outdated badge, and adapter metadata. Around 12 more are covered partially, and the rest not at all.

The app is broken at the behaviour level, not at compile time. Root causes I verified:
- With no agent running, the app's gRPC client (same settings as AgentBridge) waits more than 90s and never errors, so the default "Automatic" transport never falls back to the CLI. Meanwhile the UI shows "Agent connection: Online", because the transport defaults to .grpc.
- No `branchbox-agent` binary ships through Homebrew or the release workflow, and the packaging script does not embed one. So the gRPC path cannot work on a real install.
- The CLI fallback runs `/usr/bin/env branchbox`. With the PATH a Finder-launched app gets (launchd default), this fails with exit 127.
- `LocalNotifier` calls `UNUserNotificationCenter.current()` with no bundle check. This raises NSInternalInconsistencyException under the documented `swift run` dev path.

Contract drift is extensive:
- The agent's gRPC List keeps only Active features, so degraded, failed_retained and orphaned features are hidden.
- The proto has no runtime, ports, default_agent, base_branch or residue fields, and no RPCs for exec, prune, tunnel or devcontainer.
- The CLI-fallback decoder expects top-level tunnel_* fields. The CLI emits a nested `tunnel{}` object, so tunnel data is always lost in fallback mode.
- The app counts module status "ok", but the CLI emits success/skipped/failed. The module summary always reads "0 ok".
- feature_url has no scheme (e.g. "dev-prine.localhost"), so the app's links do not open.
- Teardown without a TTY on an unmerged branch removes the worktree, marks the feature removed, then exits 1 without printing JSON. The app would report "Teardown failed" for what is the common case.

On macOS, the usable runtimes are `container` (default, needs Docker) and `sbx` (needs the Docker Sandboxes CLI plus `sbx login`; installed here but not signed in). `local-vm` fails ("local-vm requires a Linux host"). `in-guest` is orchestration plumbing and needs `--runtime-manifest`.

`feature exec` is captured-only. On the container runtime it runs on the host in the worktree: probe output showed Darwin and "not a tty". `feature start` on the container runtime never starts the devcontainer (start_environment is a no-op). A good Mac app therefore needs its own "Start environment" button (via `devcontainer up`) and must open interactive terminals itself.

Evidence:
- Runtime probes used a disposable repo plus a headless gRPC probe, all under scratchpad/featureset (grpcprobe/, sandbox/, decode_test.swift, unc_test.swift, feature-list.json, td2.err).
- The repo was left untouched (`git status --porcelain` = 0 lines).
- No containers were created, and every process I started has exited.


## Findings

### MAC-01 [critical/bug] Automatic transport hangs forever when no agent is running; CLI fallback never triggers
AgentBridge.listFeatures falls back to the CLI only when the gRPC call throws. The ClientConnection is built with the defaults (waitsForConnectivity call start, unlimited reconnect backoff, no call time limit), so with nothing on 127.0.0.1:50515 the List RPC simply waits. loadFeatures never returns, isLoading stays true, and features stay empty. transportStatus also defaults to .grpc, so Home and the menu bar show 'Agent connection • Online' during the hang. This matches the user's 'broken' report: there is no agent on this Mac.

**Evidence:** Headless probe built in scratch from the app's exact client config and generated stubs (scratchpad/featureset/grpcprobe/Sources/GRPCProbe/main.swift): "RESULT: list still pending after 90s watchdog (no error, no fallback would trigger)"; `lsof` showed nothing listening on 50515. Code: macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:162-168 (list then status, no CallOptions), :190-196 (fallback only in catch), :294-306 (ensureClient: no callStartBehavior/timeLimit); ViewModels/FeatureListViewModel.swift:51 (`transportStatus = .grpc` default), :147 (isAgentConnected).

**Suggested fix:** Use `.withCallStartBehavior(.fastFailure)` and CallOptions(timeLimit: .timeout(.seconds(2))) for List/Status. Probe the socket or port before choosing gRPC. Default transportStatus to an 'unknown/connecting' state and only show Online after a successful RPC. Better still, make the CLI JSON contract the primary transport (see ARCH-01).

### DIST-01 [critical/distribution] No agent daemon is shipped anywhere, so the app's primary gRPC transport is unreachable on real installs
The Homebrew formula and the release archive install only branchbox, bb and branchbox-local-vm. package-macos-app.sh embeds only the CLI. There is no LaunchAgent plist and nothing in the app starts an agent. The only way to get one is building from source (`cargo run -p branchbox-agent`, scripts/start-agent-local.sh, which defaults to a devcontainer workspace path).

**Evidence:** `command -v branchbox-agent` -> not found. /opt/homebrew/Library/Taps/branchbox/homebrew-tap/Formula/branchbox.rb: `bin.install "branchbox", "bb", "branchbox-local-vm"`. .github/workflows/release.yml:213-216 copies only branchbox, bb and branchbox-local-vm. scripts/package-macos-app.sh:28-29 builds only `-p branchbox-cli`; :70-73 embeds only Resources/bin/branchbox. scripts/start-agent-local.sh:5 WORKSPACE=/workspaces/milestone2. `branchbox agent status --json` -> 'failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock'.

**Suggested fix:** Decide on one path. (a) Treat the CLI's --json output as the app contract and retire the gRPC dependency for local use. (b) Ship branchbox-agent in the formula and app bundle, install a per-user LaunchAgent from the app, and manage start/stop/health in Settings.

### MAC-02 [high/bug] CLI fallback cannot find branchbox (or docker/devcontainer) when the app is launched from Finder
CLICompat.run runs `/usr/bin/env branchbox` unless BRANCHBOX_CLI_PATH is set or a CLI is embedded. GUI apps inherit launchd's PATH (/usr/bin:/bin:/usr/sbin:/sbin), which excludes /opt/homebrew/bin. Even an embedded or absolute CLI then shells out to docker (/usr/local/bin) and devcontainer (an nvm path) through the same PATH.

**Evidence:** `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env branchbox --version` -> "env: branchbox: No such file or directory" exit=127. `launchctl getenv PATH` -> empty. Code: macos/Sources/BranchBoxApp/Agent/CLICompat.swift:106-107, :132-148 (falls back to the bare name 'branchbox'). Tool locations here: docker=/usr/local/bin/docker, devcontainer=~/.nvm/versions/node/v22.16.0/bin/devcontainer.

**Suggested fix:** Resolve the CLI to an absolute path: Settings override, then the bundle, then /opt/homebrew/bin, /usr/local/bin, ~/.cargo/bin. Capture the user's login-shell PATH once (`/bin/zsh -lic 'printf %s "$PATH"'`) and pass it as the child environment. Add a Doctor panel that shows the resolved tool paths.

### MAC-03 [high/bug] LocalNotifier crashes the app when run via `swift run` (the documented dev path)
After every successful start, teardown or devcontainer sync, LocalNotifier.notify calls UNUserNotificationCenter.current() unconditionally. AppDelegate guards permission requests on Bundle.main.bundleIdentifier, but the notifier does not. In an unbundled process this raises NSInternalInconsistencyException.

**Evidence:** scratchpad/featureset/unc_test.swift (an unbundled process calling only UNUserNotificationCenter.current()): "*** Terminating app due to uncaught exception 'NSInternalInconsistencyException', reason: 'bundleProxyForCurrentProcess is nil…'" exit=134. Code: macos/Sources/BranchBoxApp/Services/LocalNotifier.swift:6-12; call sites ViewModels/FeatureListViewModel.swift:249, :275, :311; guard exists only at App/BranchBoxMacApp.swift:13. Docs tell devs to `swift run BranchBoxApp` (macos/README.md, docs/docs/getting-started/manual-cli-e2e.md:109).

**Suggested fix:** Return early from notify when Bundle.main.bundleIdentifier == nil, or route it through a protocol that falls back to an in-app toast.

### DRIFT-01 [high/drift] gRPC proto and agent are frozen at the v0.4 feature set
The Feature message has no runtime (provider, runtime_id, published_ports, container_id, workspace_folder, container_user), base_branch, last_commit, default_agent, or tunnel notes/instructions/service_url. StartRequest has no base, runtime, devcontainer_reuse, keep_runtime_on_failure/reuse_runtime, no_worktree or default_prompt. TeardownRequest has no keep_branch or force_delete_branch. TeardownSummary has no runtime_teardown residue. The service exposes only List/Start/Teardown/Status: nothing for prune, exec, tunnel open/remove, devcontainer sync/up/down, detect or init. The agent's unix-socket IPC already carries `runtime`, so the two agent transports have drifted from each other as well.

**Evidence:** agent/proto/agent.proto:5-10, :21-32, :38-46, :52-75, :120-128 (last changed 2025-11-10, `git log -1 -- agent/proto/agent.proto`). agent/src/ops.rs:72-89 hard-codes runtime: None, keep_runtime_on_failure: false, devcontainer_reuse default, workspace_mode default. agent/src/ipc.rs FeatureRecord has `runtime: RuntimeMetadata`. Generated Swift stubs last changed 2025-11-11.

**Suggested fix:** If gRPC stays, add proto v2 messages generated from core types: RuntimeMetadata, published ports, default_agent, all start/teardown flags, and Exec/Prune/Tunnel/Devcontainer RPCs plus a streaming progress RPC. Then regenerate with scripts/generate-swift-protos.sh. Otherwise move the app to the CLI JSON contract.

### DRIFT-02 [high/bug] Agent List hides degraded, failed_retained and orphaned features
ops::list_features keeps only FeatureStatus::Active unless include_removed is set. The CLI default instead shows everything except removed. Over gRPC, the features that most need attention (environment down, retained failed sandbox, missing runtime) vanish from the app.

**Evidence:** agent/src/ops.rs:17-19 `entries.retain(|feature| feature.status == FeatureStatus::Active)` vs cli/src/commands/feature.rs:549-554 (default retains != Removed). Status reconciliation: core/src/workflows/feature.rs:1278-1302.

**Suggested fix:** Mirror the CLI semantics (exclude only Removed) and add a status filter to ListRequest.

### UX-01 [high/ux] The UI is built around one 'Active feature', but BranchBox exists to run features in parallel
Home and the menu bar show only the first feature whose status is 'active'. Tunnel status comes from that single feature too. Every other running feature is reachable only from the Features list, and the menu bar has no per-feature actions for them.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:143-145 (`features.first { $0.status.lowercased() == "active" }`), :201-215 (tunnelSummary from that one feature); Views/HomeView.swift:31-35; Menu/StatusMenuView.swift:16-18. README tagline: 'Parallel development for humans and AI agents'.

**Suggested fix:** Make the menu bar and Home show every live feature as rows: color, status, runtime, port links, and Open (editor/terminal/browser) / Launch agent / Teardown actions.

### MISSING-01 [high/missing_feature] No runtime selection or runtime/port visibility
The app cannot choose container or sbx at start, does not show which runtime a feature uses, and cannot show the resolved host ports, container id or workspace folder. These are core v0.11+ capabilities. sbx is installed on this Mac (it needs `sbx login`); local-vm cannot run on macOS and should be shown disabled.

**Evidence:** No 'runtime'/'published' references in macos/Sources (grep). The CLI emits `"runtime": {"provider": "container"}` (feature list --json). sbx probe: 'Sign in with: sbx login'. local-vm probe: 'Error: Validation error: local-vm preflight failed: branchbox-local-vm: local-vm requires a Linux host'.

**Suggested fix:** Add a runtime picker to the Start sheet (prerequisite-aware), a runtime badge per row, and a Ports section with http://localhost:<host> links. For the container runtime, fall back to `devcontainer detect --json` for the port.

### MISSING-02 [high/missing_feature] No 'Open in editor', 'Launch coding agent', 'Run command' or environment start/stop
This is the core daily loop for agent-driven work, and none of it is in the app. There is no Open in VS Code/Cursor, no agent launch (BRANCHBOX_DEFAULT_AGENT_CMD/default_agent readiness), no feature exec, and no devcontainer up/down. On the container runtime, feature start never starts the devcontainer, so users must still drop to a terminal or VS Code. 'Open in Terminal' always opens a host Terminal, which is wrong for sbx. Note that `feature exec` is captured-only (no TTY), so interactive sessions need the app to open a terminal itself.

**Evidence:** core/src/runtime/mod.rs:330-336 (container start_environment is a no-op) and :338-357 (exec runs on the host in the worktree). Verified: `feature exec demo-one --json -- sh -c 'pwd; tty; uname -s'` -> stdout "…/demo-one\nnot a tty\nDarwin". App: FeatureListViewModel.swift:427-432 (`open -a Terminal <path>` only). code and cursor are installed at /usr/local/bin.

**Suggested fix:** Add per-feature actions: Open in VS Code/Cursor (folder or dev-container URI), Open Terminal (runtime-aware), Launch Agent (Claude/Codex) in Terminal, Start/Stop/Rebuild environment (devcontainer up/down/build --json), and Run Command (feature exec --json) with an output pane. Separately, add an interactive `feature exec --tty` to the CLI.

### MISSING-03 [high/missing_feature] No health remediation for degraded, failed_retained or orphaned features; no teardown residue report
The CLI tracks five statuses and retained SBX failures (--keep-runtime-on-failure / --reuse-runtime), and teardown --json returns verified/residue evidence. The app shows a gray pill for anything other than 'active', has no Retry/Discard, and ignores teardown output.

**Evidence:** core/src/workflows/feature.rs:5270-5276 (statuses); cli/src/commands/feature.rs:95-101 (keep/reuse runtime); core/src/runtime/mod.rs:166-174 (RuntimeTeardownReport). App: Views/FeaturesView.swift:81 and Views/FeatureDetailView.swift:133 color only 'active'; AgentBridge.swift:258 discards the TeardownResponse.

**Suggested fix:** Add status-specific badges and actions (Start environment, Retry with --reuse-runtime, Discard, Clean up orphan) and a post-teardown residue sheet.

### DRIFT-03 [medium/bug] CLI-fallback decoder no longer matches `feature list --json` (tunnel and new fields lost)
CLICompat.FeatureRecord expects top-level tunnel_status, tunnel_provider and tunnel_hostname, but the CLI emits a nested `tunnel{provider,status,hostname,notes,instructions,…}`. In CLI mode the app therefore never shows tunnels. runtime, color, base_branch, last_commit, compose_project_name, env_path, default_agent and prompt-related state are not decoded at all. The unit test fixture encodes the old shape (top-level tunnel_status, 'Active', no fractional seconds), so the drift goes unnoticed.

**Evidence:** Decode run with the app's decoder settings against real output (scratchpad/featureset/decode_test.swift): "DECODED prine active Optional(2026-03-17 03:37:57 +0000) tunnel: nil modules: [\"success\", …] url: dev-prine.localhost". Real JSON: `"tunnel": {"provider": "cloudflared", "status": "disabled", …}`. Code: macos/Sources/BranchBoxApp/Agent/CLICompat.swift:150-167; Tests/BranchBoxAppTests/BranchBoxAppTests.swift testCLIRecordDecodes fixture.

**Suggested fix:** Model the full FeatureMetadata + RuntimeMetadata + default_agent (nested tunnel). Make the tests decode a golden file captured from the real CLI, refreshed on every release.

### MAC-04 [medium/bug] Module status vocabulary mismatch ('ok' vs 'success') breaks module health display
Core serializes module status as success/skipped/failed. The app counts and colors the key 'ok', so the summary always reads '0 ok', failures show as '0 ok / N fail', and every module chip is orange even on success.

**Evidence:** core/src/workflows/feature.rs:58-66 (Display: success/skipped/failed); macos/Sources/BranchBoxApp/Agent/FeatureModels.swift:45 (`collapsed["ok"]`); Views/FeatureDetailView.swift:87 (`outcome.status.lowercased() == "ok"`). Real data: modules ["success","success","success","skipped"].

**Suggested fix:** Switch on success/skipped/failed (an enum with an unknown case) and render the CLI's 'N ok / N skip / N fail' summary.

### MAC-05 [medium/bug] Feature URL links have no scheme and do not open
The registry stores feature_url without a scheme (e.g. dev-prine.localhost). The app passes it straight to URL(string:)/Link, which produces a scheme-less relative URL. The CLI renders https://<url>.

**Evidence:** feature list --json: `"feature_url": "dev-prine.localhost"`. cli/src/commands/feature.rs:712-718 prefixes https://. App: Views/HomeView.swift:177, Views/FeatureDetailView.swift:37 and :114, Views/FeaturesView.swift:44, Menu/StatusMenuView.swift:117, Views/CommandPaletteView.swift:80.

**Suggested fix:** Normalize in one place, matching the CLI's choice of scheme (https:// unless one is already present), and show the tunnel hostname separately.

### MAC-06 [medium/bug] Teardown behaves differently per transport and reports failure on partial success
The app's default teardown has deleteBranch=false. Over CLI fallback it sends neither --keep-branch nor --delete-branch, so the config default (delete_branch_by_default=true) applies and the branch is deleted. Over gRPC, delete_branch=false keeps it. Without a TTY, the CLI also removes the worktree and marks the feature removed, then exits 1 when an unmerged branch cannot be deleted, without printing the --json summary. The app then shows 'Teardown failed' for what is the common case (an unmerged feature branch).

**Evidence:** Sandbox run (scratchpad/featureset/sandbox): `branchbox feature teardown demo-one --repo … --json` (non-TTY, unmerged) -> exit=1, stdout empty, "Error: Branch 'feature/demo-one' could not be deleted without force; rerun with `--force-delete-branch` (or `--force`)." Afterwards the worktree dir was gone, the branch still existed, and the registry status was "removed". Code: cli/src/commands/feature.rs:830-836, :860-872; core/src/config.rs:176-178; app ViewModels/FeatureListViewModel.swift:442-446, Agent/CLICompat.swift:42-55 (stale comment 'CLI does not support --json for teardown'), agent/src/ops.rs:91-108 (force_delete_branch always false).

**Suggested fix:** Always pass an explicit --keep-branch or --delete-branch (pre-filled from config) and --json. Add a 'Force-delete unmerged branch' option. On the CLI side, print the JSON summary before bailing, or report branch_deleted=false as a warning instead of a non-zero exit.

### MAC-07 [medium/ux] Long operations have no progress and their results are thrown away
feature start can take minutes (devcontainer/SBX builds), yet the app shows only a disabled state. The returned StartSummary (worktree path, warnings, skipped modules, default_agent readiness, tunnel) is discarded, and the success notification uses the typed name rather than the resolved work_feature. On failure, the alert shows the CLI's raw stderr, which includes every ANSI-colored INFO tracing line.

**Evidence:** AgentBridge.swift:222 `_ = try await client.start(request)`; CLICompat.swift:39 `_ = try run(...)`, :123-127 (stderr becomes the alert text). FeatureListViewModel.swift:249 uses trimmedName. scratchpad/featureset/td2.err: 10 lines, 9 of them like `^[[2m2026-10-01T22:53:20.897764Z^[[0m ^[[32m INFO^[[0m …` before the 'Error:' line.

**Suggested fix:** Stream stderr line by line into a progress log (strip ANSI, set NO_COLOR or RUST_LOG=warn for the child). Present the parsed --json summary as a result sheet. In alerts, show only the final 'Error:' line, with the log behind a disclosure.

### MISSING-04 [medium/missing_feature] No Prune All, no tunnel open/remove, no base-branch picker
These are frequent cleanup and sharing tasks with no GUI path. prune requires --yes without a TTY and has no --json, so the app needs a dry-run list plus a confirm, or a loop of `feature teardown --json`. Tunnel JSON includes manual setup instructions that the GUI could render. --base is not exposed at all.

**Evidence:** cli/src/commands/feature.rs:883-990 (prune: text only; bails 'Refusing to prune in non-interactive mode without --yes', verified). cli/src/commands/tunnel.rs run_open/run_remove --json. No 'prune', 'tunnel open' or 'base' usage in macos/Sources.

**Suggested fix:** Add a Prune sheet (dry-run list with checkboxes), a per-feature Share toggle (tunnel open/remove --json, with an instructions panel when status=manual), and a branch picker in the Start sheet. Add --json to prune in the CLI.

### MISSING-05 [medium/missing_feature] No project onboarding or settings editor for .branchbox/config.json
A new user must run `branchbox init` in a terminal, because its tunnel and 1Password prompts need a TTY. The app's Settings hold only the workspace path and a sync strategy. There is no UI for runtime.provider, sbx.run_services, feature.branch_prefix, teardown defaults, Cloudflare settings or the editor block. The app also keeps its own UserDefaults teardown toggles that disagree with the project config.

**Evidence:** core/src/workflows/init.rs:1034, :1520 (prompts only when stdout is a TTY); core/src/config.rs:16-31 (schema); Views/SettingsView.swift:10-40 (workspace + strategy only); FeatureListViewModel.swift:390-402 (UserDefaults teardown defaults).

**Suggested fix:** Add an 'Add Project' wizard (init -y with env/config writes) and a project Settings pane that edits config.json atomically. Separately, add a `branchbox config get/set --json` command so GUI and CLI share validation.

### ARCH-01 [medium/architecture] Three divergent client contracts (gRPC proto, IPC JSON, CLI --json) with inconsistent casing
Each surface exposes a different field set: gRPC has no runtime, IPC has runtime, and the CLI has everything plus default_agent. Casing differs too: feature/tunnel JSON is snake_case, devcontainer up/exec/down/build JSON is camelCase. The mac app supports two of these surfaces and drifts on both. Keeping three in sync has clearly failed since v0.4.

**Evidence:** agent/proto/agent.proto; agent/src/ipc.rs FeatureRecord (runtime: RuntimeMetadata); cli/src/commands/feature.rs:596-613 (list JSON flattens FeatureMetadata + default_agent); core/src/devcontainer_runtime/runtime.rs:28-63 (`#[serde(rename_all = "camelCase")]`).

**Suggested fix:** Pick one versioned contract for the app. The CLI's --json is the most complete and already covers every command. Add a top-level schema version, ship JSON Schema files, and generate Swift Codable models from them. Add --json to the remaining text-only commands: detect, devcontainer sync, prune, name, init --validate.

### DIST-02 [medium/distribution] The Mac app is not built, versioned, signed or distributed by any pipeline
CI only runs swift build and swift test. The release workflow ships no .app. The packaging script hard-codes CFBundleShortVersionString 0.1.0, produces an unsigned bundle, and embeds a CLI built from the same checkout (no version pinning or compatibility check). milestone3.md planned a macOS artifact workflow and notarization, but neither exists.

**Evidence:** .github/workflows/ci.yml:264-282 (macos_swift: build/test only); scripts/package-macos-app.sh:58-59 (version 0.1.0); docs/features/backlog/milestone3.md 'Deliverables' (.github/workflows/macos-app.yml not present: `ls .github/workflows`).

**Suggested fix:** Version the app with the CLI, build it in the release workflow, sign and notarize it, and publish it as a Homebrew cask. At launch, check `branchbox --version` against a minimum and show an upgrade prompt.

### MAC-08 [medium/performance] Process pipes are drained only after exit, and detect blocks the main thread
CLICompat.run calls waitUntilExit() before reading stdout or stderr. A child that writes more than the pipe buffer (~64KB), for example a large list/start JSON or chatty stderr from SBX or devcontainer work, would block forever. runDetect calls the synchronous CLI from a MainActor Task, freezing the UI while it runs. I have not reproduced the deadlock; it is inferred from the code pattern (PLAUSIBLE).

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:116-129 (run, then waitUntilExit, then readDataToEndOfFile); ViewModels/FeatureListViewModel.swift:352-365 (runDetect inside a MainActor Task calling CLICompat.detectProject synchronously).

**Suggested fix:** Use async readers (readabilityHandler / AsyncBytes) on both pipes while the process runs, run everything off the main actor, and add cancellation.

### UX-02 [medium/ux] Per-feature 'Sync devcontainer' actually syncs every worktree; app and CLI generate different names
`devcontainer sync` has no feature target, so the row, context-menu and detail buttons silently touch every worktree. The result is always 'Sync completed', even for a dry run, because output is discarded. Name handling also differs: the Start sheet's normalizer keeps every word, while core drops filler words and caps the word count, and the quick-start fields skip normalization entirely. The user never sees the final slug before starting.

**Evidence:** `branchbox devcontainer sync --help`: 'Sync devcontainer configuration to all feature worktrees'. App: Views/FeaturesView.swift:53, Views/FeatureDetailView.swift:119, ViewModels/FeatureListViewModel.swift:265-282. `branchbox name generate "OAuth Integration"` -> 'oauth' vs Views/StartFeatureSheet.swift:74-85 -> 'oauth-integration'; core/src/naming.rs:79-98.

**Suggested fix:** Move sync to a project-level 'Update all workspaces' action with a dry-run preview, or add `devcontainer sync --feature <name> --json` to the CLI. Show a live slug preview via `name generate` and display summary.work_feature after start.

### TEST-01 [medium/test_gap] App tests do not exercise the real CLI or agent contract or the fallback timing
The only decode test uses a hand-written fixture in the old shape, so the tunnel nesting, module-status vocabulary and URL-scheme regressions all pass CI. Nothing tests the gRPC-unreachable path (MAC-01), the launchd PATH, or teardown partial-failure handling. Fractional-second timestamps (e.g. 2026-03-17T03:37:57.979509Z) decoded fine with `.iso8601` on macOS 26.5.1 here. I could not test whether the same holds on the macOS 13/14 deployment targets; the older Foundation ISO8601 strategy is known to reject fractional seconds.

**Evidence:** macos/Tests/BranchBoxAppTests/BranchBoxAppTests.swift testCLIRecordDecodes ('updated_at': '2024-02-01T12:34:56Z', top-level 'tunnel_status'); CI macos_swift job .github/workflows/ci.yml:264-282; decode_test output on macOS 26.5.1 (sw_vers).

**Suggested fix:** Add golden-file decode tests from captured `branchbox … --json` output per release. Add a fake-CLI integration test for start/teardown/list, a test that gRPC falls back to the CLI within ~2s against a closed port, and a decoder with an explicit fractional-seconds ISO8601 formatter.

### DOC-01 [medium/doc_gap] Architecture and agent docs describe agent install commands and config paths that do not exist
architecture.md documents `brew install branchbox-agent`, `branchbox-agent init`, `sudo branchbox-agent install` and ~/.branchbox/agent/config.toml. There is no agent formula, the agent binary has no subcommands, and it reads agent.toml. The same doc lists Mac App Store/DMG distribution and 'Monitor Docker containers', neither of which exists. AGENTS.md calls `branchbox devcontainer sync --json` the canonical probe, but that flag is rejected.

**Evidence:** docs/docs/internals/architecture.md:137-155, :257-277; agent/src/main.rs:32-44 (no CLI args); agent/src/config.rs:178 (`agent.toml`); tap Formula dir contains only branchbox.rb; AGENTS.md:135, :181; `branchbox devcontainer sync --json` -> "error: unexpected argument '--json' found".

**Suggested fix:** Rewrite the agent install and mac app sections to match reality (or implement them), and fix the sync --json references (or add the flag).

### DOC-02 [low/doc_gap] Mac app loop docs use the wrong defaults key/domain; CHANGELOG and CLI reference miss recent surface
manual-cli-e2e.md says `defaults write dev.branchbox.app workspace …`, but the app reads key `branchbox.workspace`, and under `swift run` there is no bundle id so that domain is not used. It also promises CLI-tagged rows, which only the unused FeatureListView renders. CHANGELOG has no 0.13.4 section and never mentions `--no-worktree` (present since v0.13.0). docs/docs/reference/cli.md omits devcontainer up/exec/down/build/configure/detect/add-tunnel/inject-agents and --no-worktree.

**Evidence:** docs/docs/getting-started/manual-cli-e2e.md:106, :109; macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:20 and ViewModels/FeatureListViewModel.swift:73 (`branchbox.workspace`); Views/FeatureListView.swift:224 is the only `.cliFallback` row tag and FeatureListView() is never instantiated; `git show v0.13.0:cli/src/commands/feature.rs | grep -c no_worktree` = 3; `grep -c -- '--no-worktree' docs/docs/reference/cli.md` = 0.

**Suggested fix:** Regenerate cli.md from the recursive help dump, add a 0.13.4 CHANGELOG entry, fix the defaults instructions, and delete the dead FeatureListView.

### MISSING-06 [low/missing_feature] No PR linkage, workspace color or editor-config consumption
The registry has pr_number (never populated), color (used for Peacock) and last_commit/base_branch. The editor block (default_agent etc.) is schema-only with no reader anywhere. A Mac app could be the first consumer: a PR badge via gh, a row tint from color, and default-agent and terminal preferences.

**Evidence:** core/src/workflows/feature.rs:909 (`pr_number: None`); no references to config.editor outside core/src/config.rs:106-124 and its tests; docs/features/in-progress/devcontainer-editor-experience.md 'Next Steps' (unchecked); gh present at /opt/homebrew/bin/gh.

**Suggested fix:** Tint rows with color, show a PR badge from `gh pr view <branch> --json number,state,url,statusCheckRollup`, and have the app read and write editor.default_agent for its Launch Agent button.

### SEC-01 [low/security] Agent gRPC is unauthenticated plaintext TCP; long workflows block async workers
The FeatureService listens on 127.0.0.1:50515 (start-agent-local.sh uses 0.0.0.0) with no auth. Any local process, or any host if bound to 0.0.0.0, can start or tear down worktrees. architecture.md claims auth 'piggybacks on OS permissions' of the unix socket, which gRPC bypasses. The handlers also run the blocking FeatureWorkflow start/teardown directly inside async tonic handlers.

**Evidence:** agent/src/config.rs:12 DEFAULT_GRPC_ADDR "127.0.0.1:50515"; scripts/start-agent-local.sh:9 GRPC_ADDR 0.0.0.0:50515; agent/src/grpc.rs:40-48 (no interceptor), :98 and :124 (sync ops in async fn); docs/docs/internals/architecture.md:162-164.

**Suggested fix:** Serve gRPC over the owner-only unix socket (grpc-swift supports UDS) or add a per-user token. Wrap workflow calls in spawn_blocking.

## Capability catalog

- **project setup / Initialize project (init/bootstrap)** — relevance=high support=none since=0.1.0 (parent structure default 0.5.0; 1Password scaffolding 0.8.0; interactive OP_GITHUB_REF/OP_SIGNING_KEY_REF prompts validated via `op read` 0.9.0; --no-coding-agents 0.5.0)
  - CLI: `branchbox init [SOURCE] [-p PATH] [-s rails|nodejs|rust|generic] [--skip-devcontainer] [--skip-env] [--reorganize] [--no-parent-structure] [-y] [-v] [--no-coding-agents] (alias: bootstrap)`
  - JSON: no; interactive: Interactive when stdout is a TTY: Confirm/Input/Password prompts for Cloudflare tunnel (enable, prefix, dns zone, account id, API token, provision now) and 1Password refs (core/src/workflows/init.rs:1034-1189, 1520-1609). -y or non-TTY uses defaults and only warns. Can clone a URL (long-running).; prereqs: git; docker for devcontainer scaffolding; optional 1Password CLI (op) and Cloudflare API token
  - GUI: 'Add Project…' onboarding wizard: choose a folder or clone a URL, stack picker pre-filled from detect, toggles for devcontainer/env/coding-agent mounts/parent layout, dry-run preview, native forms for 1Password refs (live `op read` check) and Cloudflare (Keychain-stored token), then run `init -y` with env vars. The CLI has no flags for these values, so the GUI must pass env (OP_GITHUB_REF/OP_SIGNING_KEY_REF) or write .branchbox/config.json itself.
  - Notes: The interactive prompts cannot be driven from a GUI-spawned process (no TTY). The app's own 'workspace picker' only points at an existing repo.

- **project setup / Repair/update existing setup** — relevance=medium support=none since=0.4.1 (gitignore repair); 0.9.0 (re-prompt 1Password)
  - CLI: `branchbox init --update`
  - JSON: no; interactive: Interactive on TTY (same prompts as init); prereqs: initialized repo
  - GUI: Project menu > 'Repair Setup…' showing what will change (pair with --dry-run)
  - Notes: 

- **project setup / Validate / dry-run setup** — relevance=medium support=none since=0.1.0
  - CLI: `branchbox init --validate | --dry-run`
  - JSON: no; interactive: no; prereqs: none
  - GUI: Project health check card with pass/fail rows; preview pane before init
  - Notes: Needs --json to be machine-readable.

- **project setup / Detect stack/adapter/modules** — relevance=medium support=partial since=0.1.0
  - CLI: `branchbox detect [-p PATH]`
  - JSON: no (text with emoji; cli/src/main.rs:101-129); interactive: no; prereqs: none
  - GUI: Project header chips (Stack: Rust · Adapter: Generic · Modules: devcontainer, compose, tunnel, specs) instead of a raw text sheet
  - Notes: The app shows the raw stdout in DetectOutputView, and runs it synchronously on the MainActor (FeatureListViewModel.swift:352-365). A `detect --json` would let the GUI render chips.

- **project setup / Devcontainer service/port detection** — relevance=medium support=none since=0.7.0
  - CLI: `branchbox devcontainer detect [-p PATH] [-s STACK] [--json]`
  - JSON: yes: {service_name, port, service_url, container_user, home_path} (verified: rust-dev / 50515 / vscode); interactive: no; prereqs: a .devcontainer/ in the repo
  - GUI: Show the primary service and port in the project/feature detail with a clickable http://localhost:<port> link
  - Notes: This is the only port source for the container runtime, whose runtime.published_ports is empty.

- **project setup / Feature name generation/validation** — relevance=medium support=none since=0.1.0
  - CLI: `branchbox name generate <TITLE> | branchbox name validate <NAME>`
  - JSON: no (plain stdout; validate exits 1 on invalid); interactive: no; prereqs: none
  - GUI: Live slug preview under the title field in the Start sheet ('Will create feature/oauth')
  - Notes: Core drops filler words and caps at a few words (`name generate "OAuth Integration"` -> 'oauth'). The app's own normalizer (StartFeatureSheet.swift:74-85) would produce 'oauth-integration', so the two disagree.

- **feature lifecycle / Start feature** — relevance=high support=partial since=0.1.0 (--json summary 0.2.2; `new` alias 0.2.2)
  - CLI: `branchbox feature start|new [NAME] [--title T] [--base B] [--branch-prefix P] [--repo R] [--json] [--no-summary] [--telemetry]`
  - JSON: yes: work_feature, branch_name, worktree_path, mode, prompt_seed, feature_url, compose_project_name, runtime, env_path, color, module_outcomes, skipped_modules, warnings, adapter, tunnel, prompt_bridge_enabled, generated_at, default_agent; interactive: Long-running (minutes with devcontainer/SBX). No progress stream. In text mode on a TTY it may auto-launch the default agent interactively.; prereqs: git repo; docker for compose/devcontainer modules
  - GUI: Start sheet with name/title (live slug), base-branch picker, runtime picker, mode, a progress log streamed from stderr, then a result sheet (checklist rows, warnings, links, 'Open in…' buttons)
  - Notes: The app sends name/title/minimal/prompt/prefix/reuse/skip-modules only, and discards the summary (AgentBridge.swift:222; CLICompat.swift:39).

- **feature lifecycle / Base branch selection** — relevance=high support=none since=0.1.0
  - CLI: `branchbox feature start --base <BRANCH>`
  - JSON: n/a (recorded as base_branch in registry); interactive: no; prereqs: git
  - GUI: Searchable branch picker (local/remote branches) defaulting to current HEAD
  - Notes: The proto StartRequest has no base field.

- **feature lifecycle / Reuse existing worktree + devcontainer conflict policy** — relevance=medium support=partial since=--reuse 0.1.0; --devcontainer-reuse 0.12.0
  - CLI: `branchbox feature start --reuse [--devcontainer-reuse fail|preserve|overwrite|inspect]`
  - JSON: n/a; interactive: no prompt; defaults to fail on divergence; prereqs: existing worktree
  - GUI: When reuse hits divergence, show the inspect diff and offer Keep mine (preserve) / Overwrite / Cancel
  - Notes: The app has a reuse toggle but no policy, so it always gets 'fail'.

- **feature lifecycle / In-place checkout (no worktree)** — relevance=low support=none since=0.13.0 (not mentioned in CHANGELOG)
  - CLI: `branchbox feature start --no-worktree`
  - JSON: n/a; interactive: no; prereqs: per-run clone
  - GUI: Advanced option only; mainly for orchestrators
  - Notes: Conflicts with --reuse.

- **feature lifecycle / Minimal mode + default prompt** — relevance=medium support=partial since=0.3.0
  - CLI: `branchbox feature start --minimal|--fast [--default-prompt]`
  - JSON: mode field in summary/registry (start_mode); interactive: no; prereqs: none
  - GUI: 'Quick (minimal)' segmented option; badge on the row; 'Upgrade to full' action (devcontainer sync / up)
  - Notes: The app supports --minimal but not --default-prompt.

- **feature lifecycle / Prompt seed** — relevance=medium support=partial since=0.3.0 (field 0.2.2)
  - CLI: `branchbox feature start --prompt <TEXT> (truncated to 2000 chars)`
  - JSON: prompt_seed in summary + registry; interactive: no; prereqs: none
  - GUI: Multi-line prompt editor; show the stored prompt in feature detail with 'Copy' / 'Send to agent'
  - Notes: The app sends it and keeps local history, but never displays the stored prompt.

- **feature lifecycle / Skip modules** — relevance=medium support=full since=0.1.0
  - CLI: `branchbox feature start --skip-module compose|database|tunnel|specs (repeatable)`
  - JSON: skipped_modules[{module,reason}]; interactive: no; prereqs: none
  - GUI: Checkbox list under 'More options'
  - Notes: 

- **feature lifecycle / List features** — relevance=high support=partial since=0.1.0 (--json 0.2.0; new statuses 0.12.0)
  - CLI: `branchbox feature list|features list [--repo R] [--status active|degraded|failed_retained|orphaned|removed] [--all] [--json]`
  - JSON: yes: array of the full registry record plus default_agent; interactive: no (reconciles runtime existence per entry); prereqs: none (provider binaries for health checks)
  - GUI: Sidebar list of every feature with color swatch, status badge, runtime badge, port links, a filter by status, and auto-refresh (FSEvents on .branchbox/registry.json)
  - Notes: The gRPC path hides non-active statuses (agent/src/ops.rs:17-19). The CLI path drops tunnel/runtime/color/etc.

- **feature lifecycle / Teardown feature** — relevance=high support=partial since=0.1.0 (--force-delete-branch 0.4.1; --json residue evidence 0.13.0)
  - CLI: `branchbox feature teardown <NAME> [--branch-prefix P] [--repo R] [--keep-branch|--delete-branch] [--force] [--force-delete-branch] [--complete-spec] [--telemetry] [--allow-container] [--json]`
  - JSON: yes (0.13.0): work_feature, branch_name, worktree_removed, branch_deleted, adapter_cleanup_warnings, module_reports, runtime_teardown, warnings. Not printed when the post-teardown branch check bails.; interactive: TTY prompts for a dirty-module --force and an unmerged branch -D. Without a TTY it errors (verified: exit 1 after the worktree was removed).; prereqs: docker for compose cleanup
  - GUI: Destructive confirm sheet: shows the uncommitted/unmerged state up front, then Keep/Delete/Force-delete branch, Complete spec, then a result panel with residue warnings
  - Notes: The app has force/complete-spec/delete-branch toggles. Its default deleteBranch=false conflicts with the CLI config default delete_branch_by_default=true.

- **feature lifecycle / Prune all features** — relevance=high support=none since=0.9.3
  - CLI: `branchbox prune | branchbox feature prune [--repo R] [--dry-run] [-y] [--keep-branch|--delete-branch] [--complete-spec] [--telemetry] [--allow-container]`
  - JSON: no; interactive: Confirm prompt on TTY. Without a TTY it refuses unless -y (verified). Forces removal and force-deletes branches when deleting.; prereqs: none
  - GUI: 'Prune All…' sheet listing the dry-run results with per-row checkboxes and a branch policy, then a destructive confirm and per-feature progress
  - Notes: A GUI could loop teardown --json per feature until prune gets --json.

- **feature lifecycle / Specs lifecycle (backlog -> in-progress -> completed)** — relevance=medium support=partial since=0.1.0 (backlog discovery 0.3.0)
  - CLI: `automatic on feature start (specs module); branchbox feature teardown --complete-spec`
  - JSON: module_outcomes entry 'specs'; interactive: no; prereqs: docs/features/ dir (FEATURES_DIR override)
  - GUI: 'Open spec' button in feature detail; start from a backlog spec picker
  - Notes: The app only has the complete-spec toggle.

- **feature lifecycle / Run from inside a container** — relevance=none support=none since=0.10.0
  - CLI: `--allow-container / --no-host-check on feature start (0.10.0), teardown and prune (0.10.1)`
  - JSON: n/a; interactive: no; prereqs: docker socket in container
  - GUI: Not needed in a Mac app
  - Notes: Orchestration use.

- **runtime & isolation / Runtime provider selection** — relevance=high support=none since=0.11.0 (local-vm functional 0.12.0; in-guest 0.13.0)
  - CLI: `branchbox feature start --runtime container|sbx|local-vm|in-guest; default from .branchbox/config.json runtime.provider`
  - JSON: runtime object in start/list JSON; interactive: no; prereqs: container: Docker; sbx: Docker Sandboxes CLI + `sbx login`; local-vm: Linux x86_64 + /dev/kvm; in-guest: supervisor manifest
  - GUI: Runtime picker in the Start sheet showing only Mac-capable options (Container, Docker Sandbox when sbx is installed and signed in). local-vm is shown disabled ('Linux/KVM only'); in-guest is hidden. Runtime badge on each row.
  - Notes: Verified on this Mac: local-vm fails with 'local-vm requires a Linux host'; sbx fails with 'Sign in with: sbx login'.

- **runtime & isolation / Docker SBX microVM runtime** — relevance=high support=none since=0.11.0 (experimental)
  - CLI: `branchbox feature start --runtime sbx`
  - JSON: runtime{provider:sbx, runtime_id, published_ports, container_id, workspace_folder, container_user, config_path}; interactive: Long-running sandbox and devcontainer build; prereqs: sbx CLI (BRANCHBOX_SBX_PATH override) + Docker sign-in
  - GUI: Prerequisite check with a 'Sign in to Docker Sandboxes' button (opens Terminal running `sbx login`); sandbox status row
  - Notes: macOS-capable.

- **runtime & isolation / Failed-runtime retention and retry** — relevance=medium support=none since=0.12.0
  - CLI: `branchbox feature start --keep-runtime-on-failure | --reuse-runtime`
  - JSON: status failed_retained in list; interactive: no; prereqs: sbx runtime
  - GUI: On a failed start: a 'Keep sandbox for debugging' checkbox. failed_retained rows get a 'Retry' button (calls --reuse-runtime) and a 'Discard' button (teardown).
  - Notes: 

- **runtime & isolation / SBX run_services policy** — relevance=low support=none since=0.12.0
  - CLI: `.branchbox/config.json runtime.sbx.run_services: [service,…]`
  - JSON: n/a (config); interactive: no; prereqs: sbx
  - GUI: Settings multi-select of Compose services, prompted when the /dev/net/tun preflight fails
  - Notes: core/src/workflows/feature.rs:4621-4623 error text tells the user to set it.

- **runtime & isolation / local-vm Firecracker runtime** — relevance=none support=none since=0.12.0
  - CLI: `branchbox feature start --runtime local-vm (driver: branchbox-local-vm)`
  - JSON: runtime.version{monitor,kernel_sha256,rootfs_sha256}; interactive: long-running; prereqs: Linux x86_64 + KVM (driver validate: uname Linux, x86_64, /dev/kvm)
  - GUI: Show as disabled in the runtime picker with a 'Linux/KVM only' explanation
  - Notes: The driver is installed by Homebrew on macOS but always fails validate.

- **runtime & isolation / Agentify in-guest runtime** — relevance=none support=none since=0.13.0
  - CLI: `branchbox feature start --runtime in-guest --runtime-manifest <ABS_PATH>`
  - JSON: runtime.in_guest; interactive: long-running; prereqs: already-owned Firecracker guest + supervisor-authored manifest
  - GUI: Hide
  - Notes: Orchestration plumbing.

- **runtime & isolation / Runtime identity and resolved host-port mappings** — relevance=high support=none since=0.11.0
  - CLI: `feature list --json / feature start --json -> runtime{provider, runtime_id, published_ports[{host,runtime}], container_id, workspace_folder, container_user, config_path}`
  - JSON: yes (CLI JSON and unix-socket IPC; not in gRPC proto); interactive: no; prereqs: none
  - GUI: 'Ports' section with clickable http://localhost:<host> links (runtime port shown as a subtitle); copy container id; 'Open in Docker Desktop'
  - Notes: Empty published_ports for the container runtime; use devcontainer detect there.

- **runtime & isolation / Teardown residue evidence** — relevance=medium support=none since=0.13.0
  - CLI: `branchbox feature teardown <NAME> --json -> runtime_teardown{provider, runtime_id, verified, residue_free, residue[{kind,identifiers}]}`
  - JSON: yes; interactive: no; prereqs: none
  - GUI: Post-teardown report: a green 'Cleanup verified' check, or a warning listing leftover containers/volumes with a 'Retry cleanup' button
  - Notes: 

- **exec & coding agents / Run a command in the feature runtime** — relevance=high support=none since=0.11.0
  - CLI: `branchbox feature exec <NAME> [--repo R] [--json] -- <CMD>...`
  - JSON: yes: {exit_code, stdout, stderr}; interactive: Captured only (no TTY; verified 'not a tty'). Container runtime runs on the host in the worktree dir (verified uname=Darwin). sbx runs inside the devcontainer via a login shell.; prereqs: active feature; runtime provider binary
  - GUI: 'Run Command…' sheet with an output pane and saved quick commands (tests, git status)
  - Notes: Interactive exec (exec_runtime_interactive, core/src/workflows/feature.rs:1327) exists in the library but has no CLI surface. A GUI 'Open Terminal in Runtime' would need one.

- **exec & coding agents / Open terminal at worktree** — relevance=high support=partial since=n/a
  - CLI: `(no CLI) worktree_path from feature list --json`
  - JSON: n/a; interactive: interactive; prereqs: Terminal/iTerm
  - GUI: 'Open in Terminal' (user-chosen terminal app) at the worktree for the container runtime. For sbx, open a terminal running an interactive exec into the sandbox devcontainer.
  - Notes: The app does `open -a Terminal <path>` (FeatureListViewModel.swift:427-432). That is fine on the host, but wrong for sbx.

- **exec & coding agents / Open in VS Code / Cursor (editor integration)** — relevance=high support=none since=0.1.0 (Peacock color)
  - CLI: `(no CLI) feature start writes .vscode/settings.json (peacock.color/remoteColor) and tasks.json (open feature URL)`
  - JSON: color field; interactive: no; prereqs: code / cursor CLIs (both present on this Mac)
  - GUI: 'Open in VS Code' / 'Open in Cursor' buttons (plain folder, or the dev-container URI so it reopens in container); row tinted with the feature color
  - Notes: 

- **exec & coding agents / Default coding agent hand-off** — relevance=high support=none since=0.3.0 (runtime-routed 0.11.0)
  - CLI: `env BRANCHBOX_DEFAULT_AGENT_CMD / BRANCHBOX_DEFAULT_AGENT_NAME; feature start (text mode) auto-launches via runtime exec_interactive; JSON default_agent{status ready|waiting|blocked|disabled, label, command, detail, followup}`
  - JSON: yes (start and list); interactive: Needs a TTY (interactive agent session); skipped in --json mode; prereqs: agent CLI (claude/codex) on PATH or in the devcontainer
  - GUI: 'Launch Agent' split button (Claude Code / Codex / custom) that opens a terminal running the agent in the feature; readiness badge from default_agent.status; Settings for the default agent
  - Notes: Configured only through env vars today. The editor.default_agent config field is not read.

- **exec & coding agents / Managed provider exec (exec-provider)** — relevance=none support=none since=0.13.0
  - CLI: `branchbox feature exec-provider <NAME> --provider <EXE> [--inherit-env NAME]... [-- ARGS]`
  - JSON: no; interactive: interactive (inherits stdio); prereqs: in-guest managed assignment
  - GUI: Hide (orchestration plumbing)
  - Notes: Other runtimes reject it (runtime/mod.rs:288-299).

- **exec & coding agents / Trusted tool dispatch (dispatch-tool)** — relevance=none support=none since=0.13.0
  - CLI: `branchbox feature dispatch-tool <NAME> --lease <ID> --request-id <ID> [--json] [--wait-seconds 0-300]`
  - JSON: yes ({status: dispatched|not-pending, retryable, result}); exit 75 when not pending; interactive: may wait up to 300s; prereqs: in-guest tool-request lease
  - GUI: Hide
  - Notes: 

- **exec & coding agents / Coding-agent credential mounts** — relevance=low support=none since=0.5.0 (.claude.json); 0.7.0 (.ai-agents/ layout, inject-agents)
  - CLI: `branchbox init (default on; --no-coding-agents to disable); branchbox devcontainer inject-agents [-p PATH] [--json]`
  - JSON: inject-agents --json yes; interactive: no; prereqs: compose-based devcontainer
  - GUI: Project settings row: 'Share Claude/Codex/gh credentials across features' toggle
  - Notes: 

- **devcontainer / Sync devcontainer config to all worktrees** — relevance=medium support=partial since=0.2.0
  - CLI: `branchbox devcontainer sync [-p PATH] [-s copy|symlink] [-n|--dry-run]`
  - JSON: no (`--json` rejected by clap; AGENTS.md:135,181 say it exists); interactive: no; prereqs: registered worktrees
  - GUI: Project-level 'Update all workspaces…' with a dry-run diff preview, then apply. Per-feature 'Outdated' badge links to it.
  - Notes: The app's per-feature 'Sync devcontainer' button actually syncs every worktree, and it throws the output away.

- **devcontainer / Devcontainer up/down/build** — relevance=high support=none since=0.7.0
  - CLI: `branchbox devcontainer up [WS] [--remove-existing-container] [--build-no-cache] [--skip-post-create] [--remote-env K=V] [--json]; down [WS] [-v] [--remove-orphans] [--json]; build [WS] [--no-cache] [--image-name] [--json]`
  - JSON: yes, camelCase: up{outcome, containerId, remoteUser, remoteWorkspaceFolder, composeProjectName}; down{outcome, removedContainers}; build{outcome, imageName}; interactive: long-running (image build); prereqs: Docker (native implementation; does not need the @devcontainers/cli)
  - GUI: Per-feature environment controls: Start / Stop / Rebuild (no cache) buttons with a spinner and status
  - Notes: For the container runtime, feature start never starts the devcontainer (ContainerRuntimeProvider::start_environment is a no-op, core/src/runtime/mod.rs:330-336), so the GUI needs these.

- **devcontainer / Exec in devcontainer** — relevance=medium support=none since=0.7.0
  - CLI: `branchbox devcontainer exec [-w WS] [-u USER] [--workdir DIR] [--remote-env K=V] [--json] -- <CMD>...`
  - JSON: yes {outcome, exitCode, stdout, stderr}; interactive: Captured (no TTY); prereqs: running devcontainer
  - GUI: 'Run in container' option in the Run Command sheet; an interactive shell needs `docker exec -it <containerId>` in a terminal
  - Notes: 

- **devcontainer / Read devcontainer configuration** — relevance=low support=none since=0.7.0
  - CLI: `branchbox devcontainer read-configuration [WS] [--json]`
  - JSON: always JSON {workspaceFolder, configPath, configuration, containerType}; interactive: no; prereqs: .devcontainer
  - GUI: 'Devcontainer' inspector tab (image/compose, features, forwarded ports)
  - Notes: 

- **devcontainer / Configure / add tunnel sidecar** — relevance=low support=none since=0.7.0
  - CLI: `branchbox devcontainer configure [-p] [--json]; branchbox devcontainer add-tunnel [-p] [-s SERVICE] [--json]`
  - JSON: yes; interactive: no; prereqs: compose devcontainer
  - GUI: 'Fix worktree compatibility' and 'Add tunnel sidecar' actions in project setup
  - Notes: These mutate project files.

- **tunnels / Tunnel provisioning at start** — relevance=medium support=partial since=0.2.0
  - CLI: `tunnel module during feature start (skip with --skip-module tunnel); config tunnel.*`
  - JSON: tunnel{provider, hostname, service_url, status pending|active|manual|disabled, descriptor, instructions[], notes, last_updated}; interactive: no; prereqs: Cloudflare account id + API token + DNS zone; cloudflared runs as a compose sidecar (not a host binary)
  - GUI: Per-feature 'Share' card: public hostname link + Copy, status light, manual instructions list when status=manual
  - Notes: gRPC delivers provider/status/hostname only. The CLI fallback loses tunnel data entirely (decode test: tunnel nil). The display label for pending is 'degraded'.

- **tunnels / Open/re-provision tunnel for an existing feature** — relevance=medium support=none since=0.4.1
  - CLI: `branchbox tunnel open <NAME> [--repo R] [--json]`
  - JSON: yes {work_feature, state, warnings}; interactive: no (network calls); prereqs: Cloudflare credentials
  - GUI: 'Share via Tunnel' toggle on the feature
  - Notes: 

- **tunnels / Remove tunnel** — relevance=medium support=none since=0.4.1
  - CLI: `branchbox tunnel remove <NAME> [--repo R] [--force] [--json]`
  - JSON: yes; interactive: no; prereqs: Cloudflare credentials
  - GUI: Toggle off 'Share'; offer force when provider teardown fails
  - Notes: 

- **config/settings / Runtime defaults** — relevance=high support=none since=0.11.0 / 0.12.0
  - CLI: `.branchbox/config.json runtime.provider (container|sbx|local-vm|in-guest), runtime.sbx.run_services`
  - JSON: file is JSON; interactive: no; prereqs: none
  - GUI: Project Settings > Isolation: default runtime picker (Mac-capable only)
  - Notes: No `branchbox config` command exists. The GUI must edit the file atomically or the CLI needs a config subcommand.

- **config/settings / Feature branch prefix and teardown defaults** — relevance=medium support=none since=0.4.1
  - CLI: `.branchbox/config.json feature.branch_prefix (default 'feature'), feature.teardown.{delete_branch_by_default=true, force_delete_unmerged_by_default=false, prompt_force_delete_unmerged=true}`
  - JSON: file; interactive: no; prereqs: none
  - GUI: Project Settings > Features: prefix field and teardown default toggles; the teardown sheet should pre-fill from these
  - Notes: The app keeps its own UserDefaults teardown toggles (FeatureListViewModel.swift:390-402), which conflict with these.

- **config/settings / Tunnel configuration** — relevance=medium support=none since=0.2.0
  - CLI: `.branchbox/config.json tunnel.{enabled, default_provider, providers.cloudflared.{account_id, api_token_path, tunnel_name_prefix, dns_zone, service_url, manual_instructions}}; secrets in .branchbox/secure/cloudflared.env`
  - JSON: file; interactive: Configured via interactive init prompts; prereqs: Cloudflare account
  - GUI: Settings > Sharing: Cloudflare account form with the token stored in Keychain and written to the secure env file
  - Notes: 

- **config/settings / Editor preferences** — relevance=medium support=none since=0.3.0 (schema only)
  - CLI: `.branchbox/config.json editor.{default_agent, auto_launch_agent_terminal, preferred_sidebar_view, hide_secondary_sidebar}`
  - JSON: file; interactive: no; prereqs: none
  - GUI: Settings > Coding Agent: default agent, auto-open terminal. The app could be the first consumer.
  - Notes: No code reads these fields (only core/src/config.rs:106-124 and its tests). The planned `branchbox config editor` is unimplemented (docs/features/in-progress/devcontainer-editor-experience.md).

- **config/settings / Environment knobs** — relevance=medium support=none since=various (0.3.0-0.12.0)
  - CLI: `BRANCHBOX_DEFAULT_AGENT_CMD/NAME, BRANCHBOX_ENABLE_PROMPT_BRIDGE, BRANCHBOX_DEVCONTAINER_STRATEGY, BRANCHBOX_SBX_PATH, BRANCHBOX_LOCAL_VM_DRIVER_PATH, BRANCHBOX_SKIP_HOST_VALIDATION, OP_GITHUB_REF/OP_SIGNING_KEY_REF, DEVCONTAINER_IMAGE/DEVCONTAINER_PULL_POLICY, RUST_LOG`
  - JSON: n/a; interactive: no; prereqs: none
  - GUI: Settings > Advanced: an env editor applied to the CLI processes the app spawns
  - Notes: A GUI-spawned CLI inherits the launchd environment, not the user's shell profile.

- **agent/control plane / Agent status** — relevance=low support=partial since=0.4.0
  - CLI: `branchbox agent status [--json]`
  - JSON: yes {control_plane_configured, control_plane_connected, last_delivery_at, last_failure_at, last_error, last_ack_event_id, last_sent_batch_id, last_sent_event_id, last_sent_at}; interactive: no; prereqs: running branchbox-agent (unix socket ~/.branchbox/agent/branchbox-agent.sock); verified failing here: No such file or directory
  - GUI: Diagnostics pane only; most users should not see control-plane jargon
  - Notes: 

- **agent/control plane / Agent daemon (gRPC FeatureService + IPC + control-plane drain)** — relevance=medium support=partial since=0.4.0
  - CLI: `branchbox-agent (no subcommands; config ~/.branchbox/agent/agent.toml or BRANCHBOX_AGENT_CONFIG; gRPC 127.0.0.1:50515; env BRANCHBOX_CP_ENDPOINT/TOKEN/VERIFY_TLS)`
  - JSON: IPC JSON; gRPC proto frozen at the 0.4 field set; interactive: daemon; prereqs: build from source (`cargo run -p branchbox-agent`); not shipped by Homebrew/release
  - GUI: Either drop it for a CLI-JSON transport, or have the app manage it (bundled binary + LaunchAgent, start/stop in Settings)
  - Notes: RPCs: List/Start/Teardown/Status only.

- **diagnostics/health / Feature health statuses** — relevance=high support=none since=0.12.0
  - CLI: `feature list (reconciles provider.exists/environment_ready -> degraded/orphaned; failed_retained from SBX retention)`
  - JSON: status field; interactive: no; prereqs: provider binaries
  - GUI: Colored status badges plus inline remediation: Degraded -> 'Start environment', Orphaned -> 'Clean up', Failed (retained) -> 'Retry' / 'Discard'
  - Notes: The agent filters these out and the app colors everything other than 'active' gray.

- **diagnostics/health / Module outcomes** — relevance=medium support=partial since=0.2.2
  - CLI: `module_outcomes[{module, status success|skipped|failed, duration_ms, notes, forced, recorded_at}] in start/list JSON`
  - JSON: yes; interactive: no; prereqs: none
  - GUI: Checklist view mirroring the CLI start table (Worktree, Branch, Runtime, Adapter, URL, Compose, .env, Prompt, Tunnel, Modules, Agent)
  - Notes: The app expects 'ok', so the summary always shows '0 ok' and every chip is orange.

- **diagnostics/health / Devcontainer drift tracking** — relevance=medium support=full since=0.2.0
  - CLI: `devcontainer_outdated, last_sync_at, sync_strategy in list JSON`
  - JSON: yes; interactive: no; prereqs: none
  - GUI: 'Outdated' badge with an 'Update' action
  - Notes: Minimal-mode features show 'outdated' in the CLI list (verified).

- **diagnostics/health / Adapter metadata** — relevance=medium support=full since=0.1.0
  - CLI: `adapter{name, service_url, warnings[]} in start/list JSON`
  - JSON: yes; interactive: no; prereqs: none
  - GUI: Detail rows plus a warnings callout
  - Notes: 

- **diagnostics/health / Verbose telemetry / logging** — relevance=medium support=none since=0.2.0 / 0.12.1
  - CLI: `--telemetry on start/teardown/prune; RUST_LOG=<level> (diagnostics on stderr since 0.12.1)`
  - JSON: stdout JSON stays clean (0.12.1); interactive: no; prereqs: none
  - GUI: 'Show log' disclosure on each operation streaming stderr (ANSI stripped); 'Verbose' toggle in Settings
  - Notes: stderr carries ANSI color codes even when piped (verified).

- **diagnostics/health / CLI version / prerequisite doctor** — relevance=high support=none since=0.1.0
  - CLI: `branchbox --version; (no doctor command) provider validate runs before mutations`
  - JSON: no; interactive: no; prereqs: none
  - GUI: Onboarding/Settings 'Doctor': CLI path and version (warn if below the minimum the app supports), Docker running, sbx signed in, op/gh/code/cursor present
  - Notes: The app never checks the CLI version.

- **diagnostics/health / Pull request linkage** — relevance=medium support=none since=0.1.0 (field only)
  - CLI: `registry pr_number (never populated by any code path)`
  - JSON: field present when set; interactive: no; prereqs: gh CLI
  - GUI: PR badge (number/state/checks) via `gh pr view <branch> --json`, plus 'Create PR' / 'Open PR' buttons
  - Notes: pr_number is hard-coded to None (core/src/workflows/feature.rs:909).

## GUI-worthy fields
- feature.work_feature, branch_name, worktree_path, base_branch — core/src/workflows/feature.rs:5441-5446 (FeatureMetadata)
- feature.status (active|degraded|failed_retained|orphaned|removed) — core/src/workflows/feature.rs:5270-5276; reconciled live in list_features :1278-1302
- feature.feature_url (stored without a scheme, CLI renders https://) — core/src/workflows/feature.rs:5445; cli/src/commands/feature.rs:712-718
- feature.compose_project_name, env_path — core/src/workflows/feature.rs:5446-5447
- feature.created_at / updated_at / removed_at (RFC3339 with fractional seconds) — core/src/workflows/feature.rs:5449-5451
- feature.tunnel{provider, hostname, service_url, status pending|active|manual|disabled (pending displayed as 'degraded'), descriptor{tunnel_name,tunnel_id}, instructions[], notes, last_updated, removed_at} — core/src/workflows/feature.rs:5322-5367
- feature.color (12-color palette, used for Peacock .vscode/settings.json) — core/src/workflows/feature.rs:5453-5459, :3162-3197
- feature.pr_number (field exists, never populated) — core/src/workflows/feature.rs:5460-5465, :909
- feature.last_commit — core/src/workflows/feature.rs:5466-5471
- feature.devcontainer_outdated, last_sync_at, sync_strategy — core/src/workflows/feature.rs:5472-5481
- feature.start_mode (full|minimal), prompt_seed — core/src/workflows/feature.rs:5482-5487
- feature.module_outcomes[{module, status success|skipped|failed, duration_ms, notes[], forced, recorded_at}] — core/src/workflows/feature.rs:5426-5439
- feature.adapter{name, service_url, warnings[]} — core/src/workflows/feature.rs:5493-5495
- feature.runtime{provider, runtime_id, published_ports[{host,runtime}], container_id, workspace_folder, container_user, config_path, version{monitor,kernel_sha256,rootfs_sha256}} — core/src/runtime/mod.rs:77-102, :129-133
- list/start default_agent{status ready|waiting|blocked|disabled, label, command, detail, followup} — cli/src/commands/feature.rs:1773-1806, :596-613
- start summary extras: warnings[], skipped_modules[{module,reason}], prompt_bridge_enabled, generated_at, mode — cli/src/commands/feature.rs:1216-1235
- teardown summary: worktree_removed, branch_deleted, adapter_cleanup_warnings[], module_reports[{name,teardown_ok,errors[]}], runtime_teardown{provider,runtime_id,verified,residue_free,residue[{kind,identifiers[]}]}, warnings[] — core/src/workflows/feature.rs:298-316; core/src/runtime/mod.rs:157-174
- feature exec result {exit_code, stdout, stderr} — core/src/runtime/mod.rs:141-146
- dispatch-tool result {run_id, lease_id, consumer, request_id, response} — core/src/runtime/mod.rs:149-156 (no GUI value)
- devcontainer detect {service_name, port, service_url, container_user, home_path} — cli/src/commands/devcontainer.rs (detect --json; verified output)
- devcontainer up {outcome, containerId, remoteUser, remoteWorkspaceFolder, composeProjectName} / down {outcome, removedContainers} / build {outcome, imageName} / exec {outcome, exitCode, stdout, stderr} (camelCase) — core/src/devcontainer_runtime/runtime.rs:28-63
- tunnel open/remove JSON {work_feature, state, warnings} — cli/src/commands/tunnel.rs run_open
- config.runtime.provider, runtime.sbx.run_services — core/src/config.rs:46-63
- config.feature.branch_prefix, feature.teardown.{delete_branch_by_default, force_delete_unmerged_by_default, prompt_force_delete_unmerged} — core/src/config.rs:126-182
- config.tunnel.{enabled, default_provider, providers.cloudflared.{account_id, api_token_path, tunnel_name_prefix, dns_zone, service_url, manual_instructions}} — core/src/config.rs:184-284
- config.editor.{default_agent, auto_launch_agent_terminal, preferred_sidebar_view, hide_secondary_sidebar} (schema only) — core/src/config.rs:106-124
- agent status {control_plane_configured, control_plane_connected, last_delivery_at, last_failure_at, last_error, last_ack_event_id, last_sent_batch_id, last_sent_event_id, last_sent_at} — agent/src/ipc.rs:192-216, AgentStatusPayload :499
- agent config {workspace_root, state_dir, socket_path, grpc_enabled, grpc_addr, event_flush_interval_secs, control_plane.endpoint/verify_tls; env BRANCHBOX_CP_ENDPOINT/TOKEN/VERIFY_TLS, BRANCHBOX_AGENT_GRPC_ADDR} — agent/src/config.rs:17-30, :152-170, :198-245
- env knobs BRANCHBOX_DEFAULT_AGENT_CMD/NAME, BRANCHBOX_ENABLE_PROMPT_BRIDGE — cli/src/commands/feature.rs:1750-1771, :1169; BRANCHBOX_SBX_PATH — core/src/runtime/mod.rs (SbxRuntimeProvider::new); BRANCHBOX_LOCAL_VM_DRIVER_PATH — core/src/runtime/local_vm.rs:34-51; OP_GITHUB_REF/OP_SIGNING_KEY_REF — core/src/workflows/init.rs:1520-1609