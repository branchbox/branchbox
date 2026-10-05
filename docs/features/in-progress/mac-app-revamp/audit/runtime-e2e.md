# Audit area: runtime-e2e

## Summary
I ran the mac app's real data layer (AgentBridge, CLICompat, FeatureListViewModel) end to end, without opening any GUI, against two backends: the installed CLI (0.13.4) and an agent daemon built from source. The tests live in a scratch copy at $SCRATCH/runtime-e2e/macos/Tests/BranchBoxAppTests/E2ETests.swift. They are driven by env vars and run through runtime-e2e/run-xctest.sh, which wraps xctest in a hard timeout. Logs are runtime-e2e/e2e-*.log. The repo was never written to: `git status --porcelain` is empty, and all Swift and cargo output went to scratch (cargo was run with `--locked`).

Compile level: `swift build`, `swift test` (4 tests) and a release build all pass, with no warnings in the app's own code. The parts of the backends that work: forcing a single transport, list → start → list → teardown(--force) → list succeeds over both CLI and gRPC. Start takes 0.15–0.33s on a plain repo with no docker.

What breaks at runtime:
1. **Default ("Automatic") transport hangs forever when no agent is listening.** It never falls back to the CLI. `listFeatures` was still pending after 45s and `startFeature` after 30s. The same happens if the agent stops mid-session (still pending after 40s). The cause is a gRPC ClientConnection with no call deadline and unlimited reconnect. Nothing ships or launches an agent on macOS: the Homebrew formula only installs `branchbox`, and the packaging script embeds only the CLI. So a real user's app would hang on load, and on this machine the stored defaults (Automatic) would do exactly that.
2. **Any successful start/teardown/sync crashes the app when it isn't running from a .app bundle.** The README's `swift run` dev flow is such a case. LocalNotifier calls UNUserNotificationCenter without a guard. Reproduced: NSInternalInconsistencyException "bundleProxyForCurrentProcess is nil".
3. **Teardown with the app's default options always fails on a freshly started feature.** Over the CLI the error is "Devcontainer/compose changes detected; rerun this command with --force". Over gRPC the alert just says "The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)", which hides the real message.
4. **"Delete branch" behaves differently per transport.** Off over the CLI: the branch is deleted anyway, because the CLI default changed and the app never passes --keep-branch. Off over gRPC: the branch is kept.
5. **The app reports start as successful when modules failed.** Both transports throw away the start summary.
6. **Started from Finder, the CLI's own tool lookups fail.** Docker and other tools are found only through PATH, and a Finder-style environment (PATH=/usr/bin:/bin:/usr/sbin:/sbin, simulated with `env -i`) doesn't include them. On a compose repo the compose module failed with "No such file or directory (os error 2)", yet the CLI exited 0 and the app showed success (point 5). Without the embedded binary, finding the CLI itself fails too ("env: branchbox: No such file or directory").
7. **More than 64KB of CLI stdout deadlocks CLICompat.run.** It waits for exit before reading the pipe. A 96KB listing timed out; 29KB was fine. The real CLI emits about 2KB per feature, so roughly 32 features would hang.
8. **JSON shape and naming drift.** The CLI nests `tunnel{}`, so tunnel fields come back nil and Home shows "Unknown provider/Unknown status". Module statuses are now "success", so the summary always reads "0 ok". The agent's list returns only Active features, while the CLI also shows degraded, retained and orphaned ones. The "Removed" filter can never show removed features.
9. **No visible progress anywhere.** The only ProgressView is in FeatureListView, which nothing instantiates any more.
10. **A Finder launch would point at a missing workspace.** The bundled domain dev.branchbox.app stores a workspace that doesn't exist (.../branchbox/milestone2), so CLI calls fail with "The file “milestone2” doesn’t exist."

Packaging: the bundle is arm64 only, with an ad-hoc linker signature and Info.plist not bound. `codesign --verify` and `spctl` both fail ("code has no resources but signature indicates they must be present"). There is no icon, no LSUIElement, and CFBundleShortVersionString is hard-coded to 0.1.0 while the CLI is 0.13.4. The embedded CLI is at Contents/Resources/bin/branchbox, as CLICompat expects. I built that embedded CLI from source in release mode (0.13.4, about 3 minutes); the Homebrew binary was not used for it.

Not verified:
- Decoding dates that have fractional seconds with the `.iso8601` strategy works on macOS 26. It probably fails on macOS 13–14, the app's minimum (marked PLAUSIBLE).
- Error alerts include the CLI's raw stderr, with ANSI codes and INFO lines (code-level only).
- Docker-dependent start paths were not run in a normal shell, by rule.


## Architecture notes
- Transport design: AgentBridge tries gRPC first in Automatic mode and catches any error to re-run the same operation through the CLI (AgentBridge.swift:154-272). With grpc-swift 1.x defaults (waitsForConnectivity, unlimited reconnect, no deadline) the catch is never reached while the agent is down. The design depends on an agent that nothing ships for macOS hosts (scripts/start-agent-local.sh targets a devcontainer at /workspaces/milestone2; the Homebrew formula installs only branchbox).
- There are two parallel contracts: agent.proto (frozen 2025-11-10, flat tunnel fields, no runtime, default_agent or ports) and CLI --json (nested tunnel, runtime, default_agent, new statuses). The app hand-maps both into FeatureViewData, and each mapping has drifted differently. The agent and CLI also differ in behaviour: list filtering (ops.rs:17-19 Active-only) and branch-deletion default (CLI honors config.delete_branch_by_default, agent does not).
- CLICompat runs `/usr/bin/env <cli> …` with the inherited environment, synchronous waitUntilExit, no timeout and no cancellation, discarding stdout on mutating commands. All structured results (StartSummary, teardown --json residue) are thrown away.
- FeatureListViewModel is @MainActor. Mutations spawn unstructured Tasks and toggle a single isWorking flag. AgentBridge is a non-Sendable class whose async methods mutate cached client and connection state off the main actor (not runtime-verified as a race).
- UI wiring: BranchBoxMacApp → MainAppView (Home, Features, Agent, Settings) plus a MenuBarExtra and Settings scene. FeatureListView (the only ProgressView) is orphaned. Notifications go through LocalNotifier with no bundle guard.
- Packaging is a shell script that copies the SwiftPM release binary plus the CLI into a hand-written bundle, with no signing step, no icon, a single arch and a hard-coded version.
- The scratch harness is reusable: runtime-e2e/macos/Tests/BranchBoxAppTests/E2ETests.swift, with env vars E2E_REPO, E2E_MODE (cli|grpc|auto), E2E_GRPC_PORT, E2E_FEATURE, E2E_READONLY_REPO, E2E_LARGE_OUTPUT, E2E_KILL_PID, E2E_NOTIFIER and E2E_SWIZZLE_NOTIFIER. The last one swizzles +[UNUserNotificationCenter currentNotificationCenter] in test only, so the VM path can run headless without modifying app code. run-xctest.sh wraps xctest with a perl alarm timeout.

## Findings

### RT-01 [critical/bug] Automatic transport (the default) hangs forever with no agent running; no CLI fallback
When no agent listens on 127.0.0.1:50515, gRPC calls in Automatic mode never fail, so the CLI fallback in the catch block never runs. Load stays pending forever and start never returns; isWorking stays true, so every button stays disabled. Nothing installs or launches an agent on macOS: the Homebrew formula only does `bin.install "branchbox"` and the packaging script embeds only the CLI. So the out-of-box experience is a permanently empty, stuck app. The user's dev.branchbox.app defaults have no transportPreference key, so they get Automatic.

**Evidence:** AgentBridge.swift:299-305: ClientConnection.insecure(...).withConnectionBackoff(maximum: .seconds(5)).connect(...). It sets no callStartBehavior, no retry limit, and no CallOptions timeLimit. AgentBridge.swift:162-196: fallback happens only in catch. FeatureListViewModel.swift:89-91 defaults to .automatic. Run: `E2E_GRPC_PORT=59999 run-xctest.sh 45 ...testAutomaticTransportFallbackTiming` gave exit=142 (killed by the alarm); the only output was 'E2E| STEP auto list BEGIN'. The start probe gave exit=142 after 30s with only 'STEP auto start#1(auto-noagent) BEGIN' and no feature or branch created. homebrew-tap/main/Formula/branchbox.rb:30 has `bin.install "branchbox"`.

**Suggested fix:** Use callStartBehavior .fastFailure plus a short CallOptions timeLimit (e.g. 2s for list/status), and limit connection retries. Or probe agent health once (Status RPC with a deadline) before choosing gRPC, and default to the CLI when no agent is configured. Reset the cached client on failure.

### RT-02 [high/bug] Agent stopping mid-session wedges the app (cached gRPC client hangs)
After one successful gRPC list, I sent SIGTERM to the agent. The next Automatic-mode list never returned within 40s and never fell back to the CLI. The bridge caches the client and only resets it when the workspace or transport preference changes.

**Evidence:** e2e-agent-gone.log: 'STEP auto list (agent up) OK (0.06s)', then 'sent SIGTERM to agent pid=29949 rc=0', then 'list-after-agent-stop waiter result=2 (1=completed, 2=timedOut) elapsed=40.03s'. AgentBridge.swift:294-314: client caching, with resetConnection called only from updateWorkspacePath and setTransportOverride.

**Suggested fix:** Same deadline and fast-failure fix as RT-01. Also call resetConnection() on UNAVAILABLE or deadline errors, and watch connectivity state.

### RT-03 [high/bug] LocalNotifier crashes the process when not running from a .app bundle (swift run dev flow)
LocalNotifier.notify calls UNUserNotificationCenter.current() unconditionally after every successful start, teardown and devcontainer sync. Outside a .app bundle this raises NSInternalInconsistencyException and aborts. That includes the README's `swift run BranchBoxApp` flow and xctest. AppDelegate guards only the authorization request, and its check (bundleIdentifier != nil) isn't reliable: xctest has a bundle id and still crashes.

**Evidence:** LocalNotifier.swift:11. Called at FeatureListViewModel.swift:249, 275 and 311. BranchBoxMacApp.swift:13. xctest probe: "Terminating app due to uncaught exception 'NSInternalInconsistencyException', reason: 'bundleProxyForCurrentProcess is nil: mainBundle.bundleURL file:///Applications/Xcode.app/Contents/Developer/usr/bin/'". An unbundled swiftc probe (same as swift run) printed bundleIdentifier=nil, then the same exception, exit=134.

**Suggested fix:** Guard with Bundle.main.bundleURL.pathExtension == "app" (or a cached flag) before touching UNUserNotificationCenter. Request authorization lazily.

### RT-04 [high/bug] Finder launch: the CLI's own tool lookups (docker, docker-compose, op, sbx, branchbox-local-vm) fail, and the bare `branchbox` lookup fails without the embedded binary
GUI apps get PATH=/usr/bin:/bin:/usr/sbin:/sbin (launchctl has no PATH override). CLICompat runs `/usr/bin/env <binary>` with the inherited environment. Without BRANCHBOX_CLI_PATH or an embedded binary it resolves 'branchbox', which isn't found. With the embedded binary, list, detect and start run, but the CLI spawns docker and docker-compose by bare name. On a compose-enabled repo the compose module failed while the CLI still exited 0. git works because /usr/bin/git exists. docker (/usr/local/bin), devcontainer (~/.nvm), op, sbx, branchbox and bb (/opt/homebrew/bin) all fail to resolve. The app does not need BRANCHBOX_SKIP_HOST_VALIDATION: validate_host_environment only checks /.dockerenv and DOCKER_CONTAINER (core/src/validation.rs:79-97).

**Evidence:** CLICompat.swift:106-107 and 132-148. `env -i HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin xctest ...` gave 'CLI fallback failed: env: branchbox: No such file or directory'. In the same env with the embedded CLI, the compose-repo start reported 'STEP start(gui-compose) OK' and 'mod=0 ok / 1 fail'. The registry note reads 'Failed to run Docker Compose (docker compose error: No such file or directory (os error 2); docker-compose error: No such file or directory (os error 2))'. Bare-name spawns: core/src/modules/compose.rs:37, 89, 225, 239 and core/src/runtime/mod.rs:428. `env -i ... /usr/bin/env docker --version` gives 'env: docker: No such file or directory'.

**Suggested fix:** Build the child environment explicitly. Prepend /opt/homebrew/bin, /usr/local/bin and ~/.local/bin, or capture the login-shell PATH once via `$SHELL -lic 'printf %s "$PATH"'`. Search known install locations for branchbox. Surface missing-tool diagnostics in the UI (e.g. a doctor or preflight screen).

### RT-05 [high/bug] App reports 'Feature started … is ready' even when modules failed; start summary discarded
Both transports throw away the start result: the gRPC StartResponse and the CLI's --json summary, which carries warnings, module_outcomes[].notes, feature_url, skipped_modules and default_agent. A start where compose failed (docker not found) produced a success notification and no alert. The CLI decode model doesn't include module notes either.

**Evidence:** AgentBridge.swift:222 `_ = try await client.start(request)`. CLICompat.swift:39 `_ = try run(arguments: args, ...)`. CLICompat.swift:169-172 decodes ModuleOutcomeRecord with module and status only. FeatureListViewModel.swift:247-249 notifies 'is ready' unconditionally. e2e-gui-compose.log: start OK, then 'mod=0 ok / 1 fail'.

**Suggested fix:** Decode the StartSummary (gRPC) or the start --json payload (CLI). Show warnings and failed modules with their notes in a post-start sheet, and only send the 'ready' notification when there are no failures.

### RT-06 [high/bug] Teardown with app-default options always fails on a fresh feature; gRPC error text is opaque
TeardownOptions defaults to force=false. On a just-started feature, `feature start` leaves untracked .devcontainer/ files in the worktree (observed: .devcontainer/.branchbox.env, .vscode/, docs/). Teardown then refuses without --force. Over the CLI the user sees the CLI message. Over gRPC they see only 'The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)'. There is no 'retry with force' path. I observed this in uninitialised disposable repos; whether `branchbox init`-ed repos gitignore these files is unverified.

**Evidence:** FeatureListViewModel.swift:442-446 and TeardownSheetView.swift:12. CLI run: 'teardown(e2e-cli, app defaults force=false deleteBranch=false) FAIL … Error: Devcontainer/compose changes detected; rerun this command with --force to proceed.' gRPC VM run: 'vm teardown(defaults) … alert=Teardown failed: The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)'. The underlying status was 'internal error (13): Worktree has module-managed changes: … (dirty entries: [".devcontainer/"])'. git status in the worktree: '?? .devcontainer/ ?? .vscode/ ?? docs/'.

**Suggested fix:** Use teardown --json (now supported) to show the residue. Offer a one-click 'Force teardown' on this specific error, and map GRPCStatus.message into the alert (see RT-08). Consider fixing the CLI so its own generated files don't count as dirty.

### RT-07 [high/drift] 'Delete branch' toggle behaves differently per transport; CLI path ignores the toggle being off
The CLI now deletes branches by default (config feature.teardown.delete_branch_by_default=true) unless --keep-branch is passed. The app never passes --keep-branch, so leaving 'Delete branch' unchecked still deletes the branch. The agent's gRPC handler passes the raw bool and ignores the config, so the same UI choice keeps the branch over gRPC.

**Evidence:** CLICompat.swift:42-55 (no --keep-branch). cli/src/commands/feature.rs:827-836 (keep_branch / delete_branch / default_delete_branch). core/src/config.rs:155 and 169. agent/src/ops.rs:91-108. CLI run: 'branches after teardown (deleteBranch=false requested): * main'. gRPC run: 'branches after teardown (deleteBranch=false requested): feature/e2e-grpc,* main'.

**Suggested fix:** Make the toggle three-state (project default / keep / delete), or always pass an explicit --keep-branch or --delete-branch. Align the agent with the CLI config default.

### RT-08 [medium/ux] gRPC errors surface as 'GRPC.GRPCStatus error 1.'
Alerts use error.localizedDescription. GRPCStatus has no LocalizedError conformance, so the server message (Status::internal(err.to_string())) is hidden.

**Evidence:** FeatureListViewModel.swift:133-136, 252 and 314. agent/src/grpc.rs:330-332. e2e-grpc-bridge.log: 'error=The operation couldn’t be completed. (GRPC.GRPCStatus error 1.) | debug=internal error (13): Worktree has module-managed changes…'.

**Suggested fix:** Unwrap GRPCStatus (status.message ?? code description) in AgentBridge before it reaches the view model.

### RT-09 [medium/bug] Automatic mode re-executes mutating operations via CLI after any gRPC error and masks the original error
startFeature and teardownFeature fall back to the CLI on any error, including business errors returned by a healthy agent. The operation runs twice, and the user sees a misleading 'CLI fallback failed: …' message. For partially completed operations this could compound side effects.

**Evidence:** AgentBridge.swift:223-229 and 259-271. e2e-auto-dup.log (agent up): 'auto start#1(e2e-dup) OK', then 'auto start#2(e2e-dup) duplicate FAIL … error=CLI fallback failed: Error: Worktree already exists at: …/e2e-dup'.

**Suggested fix:** Fall back only on transport-level failures (UNAVAILABLE, connect or deadline errors before the request was sent), never on server-returned application errors.

### RT-10 [medium/bug] CLICompat.run deadlocks when CLI output exceeds the ~64KB pipe buffer
run() calls process.waitUntilExit() before reading stdout or stderr. A child that writes more than the pipe buffer blocks forever, and so does the caller. The real CLI emits about 2064 bytes per feature, so `feature list --json` (with or without --all) hangs at roughly 32 features. Very verbose stderr would hang the same way.

**Evidence:** CLICompat.swift:115-127 (waitUntilExit at line 121, readDataToEndOfFile at lines 122 and 124). Fake CLI through the real CLICompat.featureList: FAKE_N=30 (28,933 bytes) returned 30 records in 0.04s. FAKE_N=100 (96,483 bytes) gave 'large-output waiter result=2 (timedOut) after 15.06s'. The main repo with --all already has 8 features at 16,514 bytes.

**Suggested fix:** Read both pipes concurrently (readabilityHandler or async bytes) before or while waiting. Add a timeout and cancellation, and run off the main actor.

### RT-11 [medium/drift] CLI JSON drift: nested tunnel object and 'success' module status are mis-mapped; new fields ignored
The CLI emits tunnel{provider,status,notes,last_updated}, but FeatureRecord expects flat tunnel_status/tunnel_provider/tunnel_hostname, so tunnel data is nil in CLI mode and Home shows 'Unknown provider/Unknown status'. moduleSummary counts only 'ok' and 'failed', but the CLI and agent emit 'success', 'skipped' and 'failed', so a healthy feature shows '0 ok'. Not modelled at all: runtime{provider}, default_agent, base_branch, color, created_at, compose_project_name, env_path, last_commit, and the newer statuses. The gRPC proto (unchanged since 2025-11-10) also lacks runtime, default_agent and port mappings.

**Evidence:** CLICompat.swift:150-167. FeatureModels.swift:39-51. CLI VM run: 'features=["e2e-vm-cli:active:0 ok"] … tunnelSummary=Unknown provider/Unknown status'. gRPC run: 'mod=0 ok tunnel=disabled/cloudflared'. Real JSON from `branchbox feature list --json`: '"tunnel": {"provider": "cloudflared", "status": "disabled", …}' and '"status": "success"'.

**Suggested fix:** Model the current CLI JSON (ideally a generated or shared schema) and treat 'success' as ok. Extend agent.proto with runtime, default_agent, ports and the new statuses, or make the CLI JSON the single contract.

### RT-12 [medium/bug] 'Removed' filter never shows removed features; non-active statuses vanish over gRPC
The view model always calls listFeatures(includeRemoved: nil), and the config default comes only from the BRANCHBOX_SHOW_REMOVED env var, so removed features are never fetched. The FeaturesView 'Removed' filter (status != active) can therefore only ever show degraded, failed_retained or orphaned features, and only in CLI mode: the agent's list drops everything that isn't Active. Status labels use .capitalized, so 'failed_retained' would render as 'Failed_Retained'.

**Evidence:** FeatureListViewModel.swift:128. AgentBridge.swift:22. FeaturesView.swift:62-68. agent/src/ops.rs:17-19. The CLI help says 'retained and orphaned features are shown by default'. Run: 'list#2 … count=0' versus 'list#2(includeRemoved) … count=1 … status=removed'.

**Suggested fix:** Drive includeRemoved from the filter selection. Make the agent match CLI semantics. Map statuses to human-readable labels.

### RT-13 [medium/ux] No progress UI for start/teardown/load; the only ProgressView lives in dead code
FeatureListView (the only view with a ProgressView, shown when isLoading) is no longer used; MainAppView shows Home, Features, Agent and Settings. The live views only disable buttons while isWorking. There's no streaming output, step progress, cancel or timeout. Combined with RT-01 the app just looks empty and inert. Measured start time is 0.15–0.33s on a plain repo. Real devcontainer or compose starts take longer (not measured, since that needs docker).

**Evidence:** `grep -rn "FeatureListView("` finds no instantiation. ProgressView appears only at FeatureListView.swift:49-50 and 175-177. HomeView.swift:75 and 77 use .disabled(viewModel.isWorking). VM probe: 'vm.isWorking immediately after startFeature()=true', then 'vm start finished after 0.33s (samples=6)'.

**Suggested fix:** Show an activity indicator or status row bound to isLoading and isWorking. Stream CLI stderr or agent events into a progress log, and add cancel and timeout.

### RT-14 [medium/bug] Stale or invalid workspace: bundled defaults point at a nonexistent path, and Finder cwd '/' is the fallback
The dev.branchbox.app domain (packaged app) stores branchbox.workspace=~/projects/branchbox-suite/branchbox/milestone2, which doesn't exist. Every CLI call then fails with 'CLI not runnable: The file “milestone2” doesn’t exist.' The swift-run domain 'BranchBoxApp' has a different value (/main). With no stored value, AgentConfiguration falls back to FileManager.currentDirectoryPath, which is '/' for Finder-launched apps (standard macOS behaviour, not executed here), and the CLI then fails with 'Not a git repository: /'. There is no workspace validation beyond an existence check.

**Evidence:** `defaults read dev.branchbox.app` shows '"branchbox.workspace" = "~/projects/branchbox-suite/branchbox/milestone2"'. `ls` on that path reports 'No such file or directory'. `defaults read BranchBoxApp` shows '"branchbox.workspace" = "~/projects/branchbox-suite/branchbox/main"'. AgentBridge.swift:19-21 and FeatureListViewModel.swift:84 and 190-192. Probe: 'CLI fallback failed: CLI not runnable: The file “milestone2” doesn’t exist.' `branchbox feature list --json --repo /` gives 'Error: Validation error: Not a git repository: /'.

**Suggested fix:** Validate that the workspace is a BranchBox or git repo on launch and route to onboarding or the workspace picker. Support multiple recent workspaces, and drop the cwd fallback in bundled mode.

### PKG-01 [medium/distribution] Packaged .app is ad-hoc signed and fails codesign --verify and Gatekeeper; arm64-only, no icon, version 0.1.0
package-macos-app.sh hand-writes Info.plist with CFBundleShortVersionString 0.1.0 (the CLI is 0.13.4) and CFBundleName 'BranchBoxApp'. It sets no CFBundleIconFile, no LSUIElement and no CFBundleDisplayName. It never runs codesign, so the main binary keeps only the linker's ad-hoc signature with Info.plist not bound, and strict verification fails. There's no notarization. Copies downloaded with quarantine would be blocked (inference). The build is single-arch (host arm64). SwiftPM resource bundles (only PrivacyInfo.xcprivacy here, with no Bundle.module use, so no crash) are not copied in.

**Evidence:** scripts/package-macos-app.sh:17, 28, 58-59, 62, 71 and 73. `plutil -p` shows CFBundleIdentifier dev.branchbox.app, LSMinimumSystemVersion 13.0 and CFBundleShortVersionString 0.1.0, with no icon or LSUIElement keys. `codesign -dv` shows 'Signature=adhoc', 'flags=0x20002(adhoc,linker-signed)', 'Info.plist=not bound'. `codesign --verify --deep --strict` and `spctl -a -vv` both report 'code has no resources but signature indicates they must be present' (spctl exit=1). `lipo -info` gives arm64 for both the app and the embedded CLI. Embedded CLI 'branchbox 0.13.4' is at Contents/Resources/bin/branchbox. Resource bundles left behind: SwiftProtobuf_SwiftProtobuf.bundle, swift-nio-ssl_NIOSSL.bundle and swift-nio_NIOPosix.bundle.

**Suggested fix:** Read the version from the workspace Cargo.toml, add an AppIcon.icns, and build universal (--arch arm64 --arch x86_64, plus a universal CLI via lipo). Sign the nested CLI, then the app, with Developer ID and hardened runtime, notarize and staple. Copy SwiftPM bundles. Consider an Xcode project or xcodebuild archive, and a Homebrew cask.

### RT-15 [low/bug] JSONDecoder .iso8601 with fractional-second timestamps probably fails on macOS 13–14 (PLAUSIBLE, not reproduced)
The CLI emits timestamps like '2026-03-17T03:37:57.979509Z'. On macOS 26 the .iso8601 strategy decoded them fine (verified). Older Foundation's .iso8601 (the ISO8601DateFormatter-based implementation) historically rejects fractional seconds. That would make the whole CLI list decode throw on the app's minimum OS. The existing unit test uses a non-fractional timestamp, so it wouldn't catch this. Unverified: no macOS 13/14 host was available.

**Evidence:** CLICompat.swift:15. BranchBoxAppTests.swift:15 uses "2024-02-01T12:34:56Z". On macOS 26: 'STEP CLICompat.featureList(~/projects/branchbox-suite/branchbox/main) OK (0.02s)', records prine and remotion.

**Suggested fix:** Use a custom date strategy that tries ISO8601 with and without .withFractionalSeconds. Add a unit test with real CLI output.

### RT-16 [low/ux] CLI error alerts contain the whole raw stderr, including ANSI colour codes and INFO tracing lines
On a non-zero exit, CLICompat puts the entire stderr into the alert. The CLI writes coloured tracing logs to stderr even when it's not a TTY, so any failure after some progress would show escape-code noise. The failures observed in this run happened to print only the final error line. This is a code-level observation (PLAUSIBLE).

**Evidence:** CLICompat.swift:123-126. `cat -v reuse.stderr` shows '^[[2m2026-10-01T23:19:58.462003Z^[[0m ^[[32m INFO^[[0m ^[[2mworktree_core::workflows::feature…'.

**Suggested fix:** Set NO_COLOR=1 / RUST_LOG=warn for child processes, use --json error payloads, and show only the last 'Error:' line with an expandable log.

### RT-17 [low/performance] Detect runs a blocking Process on the main actor; CLI-mode refresh spawns 3 processes
runDetect calls the synchronous CLICompat.detectProject inside a Task that inherits @MainActor, so the UI freezes while it runs. Each CLI-mode list also spawns `branchbox --help` and `agent status --json`, and the latter always fails without an agent. The help check `help.contains("agent")` is always true.

**Evidence:** FeatureListViewModel.swift:352-365. CLICompat.swift:57-70 and AgentBridge.swift:316-326. Detect took 0.01s here, so it's minor in practice.

**Suggested fix:** Move detect into the bridge (nonisolated or detached). Cache agent capability, or skip the status probe when no agent socket exists.

### DRIFT-01 [low/drift] Feature-set gap: app exposes none of the post-0.4 CLI features
The app models only start, teardown, list, devcontainer sync and detect. The CLI now also has runtime providers (--runtime container|sbx|local-vm), --base, --no-worktree, --devcontainer-reuse, --keep-runtime-on-failure, --reuse-runtime, --allow-container, feature exec, exec-provider, dispatch-tool, feature prune and prune, teardown --json and --force-delete-branch, and default_agent. The comment saying teardown has no --json is stale.

**Evidence:** `branchbox feature start --help` and `branchbox feature --help` list these. CLICompat.swift:19-55 and 43: '// The CLI does not support --json for teardown'. FeatureStartIntent is at FeatureModels.swift:149-157.

**Suggested fix:** Add runtime, base and devcontainer-reuse options to the start sheet. Add exec and open-shell, prune, and per-feature runtime and port display, using CLI --json as the contract.

### AGENT-01 [low/bug] Agent daemon exits entirely if its IPC socket path exceeds SUN_LEN
With BRANCHBOX_AGENT_DIR under a long temp path, the agent bound gRPC, then failed to bind the unix socket and exited, taking gRPC down with it. This makes isolated or sandboxed runs fragile. It worked once socket_path was set to a relative path in agent.toml.

**Evidence:** agent.log: 'gRPC server listening on 127.0.0.1:50616', then 'Error: Failed to bind Unix socket …/agent-state/branchbox-agent.sock Caused by: path must be shorter than SUN_LEN'. agent/src/config.rs:76-78.

**Suggested fix:** Fail with guidance, or fall back to a short path (e.g. $TMPDIR/bbx-<uid>.sock). Don't abort gRPC because IPC failed.

### TEST-01 [low/test_gap] No tests cover the data layer; the existing 4 tests are pure model unit tests
None of the failures above (hang, crash, deadlock, decode drift, flag semantics) would be caught by the current suite. The scratch E2E harness shows these paths can be tested headlessly with a disposable repo and env-driven backends.

**Evidence:** macos/Tests/BranchBoxAppTests/BranchBoxAppTests.swift (4 tests: 1 decode, 3 devcontainer summary). Scratch harness: runtime-e2e/macos/Tests/BranchBoxAppTests/E2ETests.swift and runtime-e2e/run-xctest.sh.

**Suggested fix:** Adopt a version of E2ETests (CLI mode against a temp git repo; gRPC mode against a cargo-built agent on a random port), with deadlines, in CI on macOS runners.

## Experiments

- [pass] **(a) swift build in scratch copy** — `rsync macos (excl .build) to runtime-e2e/macos && swift build`
  - Build complete! (96.70s); no warnings in BranchBoxApp sources after touch+rebuild

- [pass] **(a) swift test** — `cd runtime-e2e/macos && swift test`
  - Executed 4 tests, with 0 failures (BranchBoxAppTests: testCLIRecordDecodes, 3x devcontainer summary)

- [pass] **cargo build agent+cli (debug) and cli (release) into scratch target** — `CARGO_TARGET_DIR=runtime-e2e/target cargo build --locked -p branchbox-agent -p branchbox-cli; cargo build --locked -p branchbox-cli --release`
  - dev profile Finished in 3m 02s; release Finished in 3m 11s; repo git status clean

- [pass] **(b) package .app in scratch (adapted package-macos-app.sh)** — `CARGO_TARGET_DIR=runtime-e2e/target bash runtime-e2e/pkg/package-scratch.sh`
  - Done: runtime-e2e/pkg/build/BranchBoxApp.app (32M); Contents/{Info.plist,MacOS/BranchBoxApp,Resources/bin/branchbox}

- [partial] **(b) Info.plist** — `plutil -p BranchBoxApp.app/Contents/Info.plist`
  - CFBundleIdentifier dev.branchbox.app; CFBundleShortVersionString 0.1.0; LSMinimumSystemVersion 13.0; no LSUIElement, no CFBundleIconFile

- [fail] **(b) codesign / spctl** — `codesign -dv --verbose=2 app; codesign --verify --deep --strict -v app; spctl -a -vv app`
  - Signature=adhoc, flags=0x20002(adhoc,linker-signed), Info.plist=not bound; verify and spctl both: 'code has no resources but signature indicates they must be present' (spctl exit=1)

- [partial] **(b) architectures and embedded CLI** — `lipo -info / file on MacOS/BranchBoxApp and Resources/bin/branchbox; otool -l LC_BUILD_VERSION`
  - both arm64 only; app minos 13.0 sdk 26.2; CLI minos 11.0; embedded CLI prints 'branchbox 0.13.4'; SwiftPM resource bundles (PrivacyInfo only) not copied

- [pass] **(c) read-only CLI decode against main repo** — `BRANCHBOX_CLI_PATH=/opt/homebrew/bin/branchbox E2E_READONLY_REPO=<main> swift test --filter E2ETests/testCLIFeatureListDecodeReadOnly`
  - CLICompat.featureList OK (0.02s) records=[prine, remotion] (fractional-second dates decode on macOS 26); agentStatusOrDefault -> defaults; detect OK

- [partial] **(c) Bridge lifecycle via CLI fallback (forced)** — `BRANCHBOX_CLI_PATH=/opt/homebrew/bin/branchbox E2E_REPO=repos/clirun/main E2E_MODE=cli ... testBridgeLifecycle`
  - list#0 OK 0.07s; start OK 0.25s; list#1 OK (mod=0 ok, tunnel=nil/nil); teardown(force=false) FAIL 'Error: Devcontainer/compose changes detected; rerun this command with --force to proceed.'; teardown(force) OK; branch deleted despite deleteBranch=false; list#2 count=0, includeRemoved count=1

- [partial] **(c) ViewModel lifecycle via CLI (forced pref)** — `... E2E_MODE=cli E2E_SWIZZLE_NOTIFIER=1 run-xctest.sh 120 E2ETests/testViewModelLifecycle`
  - load OK; isWorking=true then start finished 0.33s, alert=nil; features=[e2e-vm-cli:active:0 ok]; tunnelSummary=Unknown provider/Unknown status; teardown(defaults) alert 'Teardown failed: CLI fallback failed: Error: Devcontainer/compose changes detected…'; force teardown OK

- [fail] **(c) Automatic transport, no agent listening (list)** — `E2E_GRPC_PORT=59999 run-xctest.sh 45 E2ETests/testAutomaticTransportFallbackTiming`
  - Only 'E2E| STEP auto list BEGIN' printed; exit=142 (SIGALRM after 45s) - never fell back to CLI

- [fail] **(c) Automatic transport, no agent listening (start)** — `E2E_GRPC_PORT=59999 run-xctest.sh 30 E2ETests/testAutomaticDuplicateStart`
  - 'STEP auto start#1(auto-noagent) BEGIN' then exit=142 after 30s; no branch/worktree created

- [partial] **(c) Agent daemon from source, isolated** — `BRANCHBOX_AGENT_DIR=runtime-e2e/agent-state target/debug/branchbox-agent (agent.toml: socket_path="agent.sock", grpc_addr=127.0.0.1:50616)`
  - First attempt died: 'Failed to bind Unix socket … path must be shorter than SUN_LEN'; with relative socket: 'gRPC server listening on 127.0.0.1:50616', 'IPC server listening on agent.sock'

- [partial] **(c) Bridge lifecycle via gRPC (forced)** — `E2E_REPO=repos/grpcrun/main E2E_MODE=grpc E2E_GRPC_PORT=50616 run-xctest.sh 180 E2ETests/testBridgeLifecycle`
  - list#0 OK 0.06s; start OK 0.32s; list#1 OK (tunnel=disabled/cloudflared, mod=0 ok); teardown(force=false) FAIL 'The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)' (debug: internal error (13): Worktree has module-managed changes … [".devcontainer/"]); teardown(force) OK; branch KEPT (feature/e2e-grpc); list#2 count=0

- [partial] **(c) ViewModel lifecycle via gRPC (forced pref)** — `... E2E_MODE=grpc E2E_GRPC_PORT=50616 E2E_SWIZZLE_NOTIFIER=1 testViewModelLifecycle`
  - start 0.28s alert=nil, tunnelSummary=cloudflared/disabled; teardown(defaults) alert 'Teardown failed: The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)'

- [pass] **(c) Automatic transport with agent up** — `E2E_GRPC_PORT=50616 testAutomaticTransportFallbackTiming`
  - auto list OK (0.12s) transport=grpc; 2nd call 0.04s

- [fail] **(c) Automatic mode duplicate start (gRPC business error -> CLI rerun)** — `E2E_GRPC_PORT=50616 testAutomaticDuplicateStart`
  - start#1 OK via grpc; start#2 FAIL 'CLI fallback failed: Error: Worktree already exists at: …/e2e-dup' (op re-executed via CLI)

- [fail] **(c) Agent stopped mid-session** — `E2E_KILL_PID=<my agent pid> testAgentGoesAwayMidSession`
  - list (agent up) OK; SIGTERM rc=0; next list: 'waiter result=2 (timedOut) elapsed=40.03s'

- [pass] **CLI agent status JSON shape (agent up)** — `BRANCHBOX_AGENT_SOCKET=agent.sock branchbox agent status --json`
  - {control_plane_configured:false, control_plane_connected:false, last_*:null, last_sent_batch_id:null …} - matches CLICompat.AgentStatusRecord

- [fail] **LocalNotifier outside .app (xctest)** — `E2E_NOTIFIER=1 swift test --filter E2ETests/testLocalNotifierUnbundled`
  - NSInternalInconsistencyException 'bundleProxyForCurrentProcess is nil: mainBundle.bundleURL file:///Applications/Xcode.app/Contents/Developer/usr/bin/' (bundleIdentifier=com.apple.dt.xctest.tool)

- [fail] **UNUserNotificationCenter in unbundled executable (swift run equivalent)** — `swiftc probes/notifier.swift && ./notifier`
  - bundleIdentifier=nil; 'Terminating app due to uncaught exception NSInternalInconsistencyException … bundleProxyForCurrentProcess is nil'; exit=134

- [fail] **Pipe-buffer deadlock in CLICompat.run** — `FAKE_N={30,100} BRANCHBOX_CLI_PATH=fakecli/branchbox E2E_LARGE_OUTPUT=1 testLargeCLIOutputDoesNotHang`
  - N=30 (28,933 B): returned in 0.04s; N=100 (96,483 B): 'waiter result=2 (timedOut) after 15.06s'

- [fail] **(d) Finder env, no CLI override, no embedded CLI** — `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin run-xctest.sh 60 E2ETests/testCLIFeatureListDecodeReadOnly`
  - 'CLI fallback failed: env: branchbox: No such file or directory' (list and detect)

- [pass] **(d) Finder env, absolute (embedded) CLI** — `env -i … BRANCHBOX_CLI_PATH=<app>/Contents/Resources/bin/branchbox testCLIFeatureListDecodeReadOnly + testBridgeLifecycle (plain repo)`
  - list/detect OK; lifecycle same as normal env (start OK 0.16s; default teardown needs --force)

- [fail] **(d) Finder env, compose-enabled disposable repo** — `env -i … BRANCHBOX_CLI_PATH=<embedded> E2E_REPO=repos/compose/main testBridgeLifecycle`
  - start OK (CLI exit 0) but 'mod=0 ok / 1 fail'; registry note: 'Failed to run Docker Compose (docker compose error: No such file or directory (os error 2); docker-compose error: No such file or directory (os error 2))'; no containers created

- [fail] **(d) Tool resolution under GUI PATH** — `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env <tool> --version`
  - git 2.50.1 OK; docker, docker-compose, devcontainer, op, sbx, branchbox-local-vm, cloudflared: 'No such file or directory' (login shell has docker=/usr/local/bin, op/sbx/branchbox=/opt/homebrew/bin, devcontainer=~/.nvm)

- [pass] **(d) read-only CLI under GUI env with absolute path** — `env -i HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /opt/homebrew/bin/branchbox feature list --json --repo <main>; … detect --path <main>`
  - ok features=['prine','remotion']; detect shows modules devcontainer, compose, tunnel, specs (compose would need docker)

- [pass] **launchd GUI PATH** — `launchctl getenv PATH`
  - empty (no override) -> GUI apps get default /usr/bin:/bin:/usr/sbin:/sbin

- [partial] **(e) UserDefaults** — `defaults read dev.branchbox.app; defaults read BranchBoxApp`
  - dev.branchbox.app: branchbox.workspace=~/projects/branchbox-suite/branchbox/milestone2 (does not exist); BranchBoxApp: branchbox.workspace=…/branchbox/main, teardown.* = 0; no transportPreference key in either

- [fail] **Stale workspace through real CLICompat** — `E2E_READONLY_REPO=repos/does-not-exist/milestone2 testCLIFeatureListDecodeReadOnly`
  - 'CLI fallback failed: CLI not runnable: The file “milestone2” doesn’t exist.'

- [partial] **Manual CLI semantics (disposable repo)** — `branchbox feature start demo-one --repo R --json --no-summary; feature teardown demo-one --repo R [--force]`
  - start exit=0 in 0.21s (stderr has ANSI INFO logs); teardown w/o --force exit=1 'Devcontainer/compose changes detected'; with --force: 'Branch deleted: yes' although --delete-branch not passed

- [pass] **Cleanup** — `kill my agent pids; teardown remaining disposable features; git status in repo`
  - my agents stopped; all disposable repos active=0; repo `git status --porcelain` empty; other agents' processes (verify-agent-grpc, VerifyTests xctest) left untouched

## Open questions
- Does `branchbox init` add .devcontainer/.branchbox.env (and the generated .vscode/ and docs/) to .gitignore? If not, the default teardown refusal (RT-06) applies to every real project, not just uninitialised repos.
- Should the agent remain part of the mac app story at all? If yes, the app needs to bundle and launch it (a LaunchAgent or a child process) and the proto needs updating. If no, the gRPC path should be removed and the CLI --json made the single contract.
- Does JSONDecoder .iso8601 reject fractional seconds on macOS 13/14 (the app's minimum)? This needs a run on an older-OS host or CI image.
- How long do real feature starts take (devcontainer and compose, sbx, local-vm)? This determines how much progress or streaming UI is needed, but it couldn't be measured without creating docker containers.
- Should the app derive PATH from the user's login shell (it has to run zsh -l once) or ship fixed fallback directories? This matters for 1Password (op), sbx and devcontainer CLIs installed via nvm.
- Do UNUserNotificationCenter authorization and delivery work for an ad-hoc-signed bundle? Untested because launching the app was out of scope.