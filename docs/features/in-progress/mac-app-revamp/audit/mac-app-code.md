# Audit area: mac-app-code

## Summary
The BranchBox macOS app (macos/, about 2.4k hand-written Swift lines, untouched since 2025-11-14, roughly v0.4.0) compiles cleanly. Its behaviour is broken in every configuration a real user would hit.

**Two defaults combine to make it unusable.**
- **Automatic transport always tries gRPC first.** The ClientConnection uses grpc-swift's defaults: `callStartBehavior = .waitsForConnectivity`, `ConnectionBackoff(retries: .unlimited)`, and no CallOptions time limit. So when no agent is listening, `list`, `start` and `teardown` hang forever and never fall back to the CLI.
  - Measured: Automatic mode against an empty 127.0.0.1:50515 did not return within 45s or 180s.
  - Forced gRPC did not return within 30s.
  - When an agent died mid-session, the next call hung for more than 40s.
- **Homebrew does not ship `branchbox-agent`.** The installed Cellar bin holds only `bb`, `branchbox` and `branchbox-local-vm`. So "no agent" is the normal case.

**What the user actually sees.**
- An empty dashboard, while the toolbar shows a green "gRPC" badge and Home says "Agent connection • Online". `transportStatus` defaults to `.grpc` and is only updated on success.
- Clicking Start hangs with `isWorking = true`, which disables every action button for the rest of the session.
- The only escape is switching the toolbar picker to "Force CLI".

**The Force CLI path has its own blockers.**
- **Launched from Finder:** `/usr/bin/env branchbox` fails with "env: branchbox: No such file or directory", because the launchd PATH lacks /opt/homebrew/bin. Even an embedded CLI could not find docker (/usr/local/bin) or the devcontainer CLI (nvm).
- **Run via the documented `swift run`:** the first successful start, teardown or sync calls `UNUserNotificationCenter.current()` in an unbundled process. That crashes with NSInternalInconsistencyException ("bundleProxyForCurrentProcess is nil"); reproduced with a standalone binary.
- **Output handling:** `CLICompat.run` calls `waitUntilExit()` before draining pipes. Any stdout or stderr over about 64KB deadlocks; reproduced with a fake CLI writing 200KB. `feature list --json --all` is about 2KB per feature (16,514 bytes for 8), so a repo with about 32 or more historical features would freeze the list when BRANCHBOX_SHOW_REMOVED=1.
- **Detect:** `runDetect` runs the CLI synchronously on the MainActor.

**Data and semantics have drifted from the CLI (v0.13.4).**
- The CLI JSON nests `tunnel{provider,status,...}`, but the app decodes flat `tunnel_*` keys, so the tunnel is always nil in CLI mode.
- `last_sync_at` and `sync_strategy` no longer exist, so every devcontainer shows "Pending".
- Module statuses are `success|skipped|failed`, but the app counts "ok", so every feature reads "0 ok" and every chip is orange.
- `feature_url` is scheme-less ("dev-prine.localhost"), so the Open links have a nil scheme.
- New statuses (`degraded`, `failed_retained`, `orphaned`) are hidden entirely over gRPC (`ops::list_features` keeps only Active), shown over CLI, mislabelled by the "Removed" filter, and capitalised as "Failed_Retained".
- **Teardown is the most dangerous drift.** Over CLI the app never passes `--keep-branch`, and the CLI default is `delete_branch_by_default = true`. If "Force removal" is on (persisted across sessions), core calls `git.delete_branch(name, force_remove || ...)`, which is `git branch -D` on unmerged branches. Over gRPC the same toggles keep the branch. Same UI, opposite data-loss outcome.
- The proto and app surface stopped at List/Start/Teardown/Status. Missing:
  - runtime provider and host-port mappings
  - base branch, `--no-worktree`, `--devcontainer-reuse`, keep/reuse-runtime
  - `exec`, `prune`, `tunnel open/remove`, `devcontainer up/down/exec/build`
  - `default_agent`, teardown `--json` residue
  - any progress streaming

**Architecture and code quality.**
- One app-lifetime `@StateObject FeatureListViewModel` (@MainActor) drives all scenes: WindowGroup, Settings and MenuBarExtra. It owns a non-Sendable `AgentBridge` class whose mutable connection state is touched from both the MainActor and the global executor.
  - Strict-concurrency builds flag "sending 'self.bridge' risks causing data races" at 4 call sites, plus 2 non-Sendable static formatters. All 6 are errors in Swift 6 mode.
  - `resetConnection` does `connection.close().wait()` on the main thread.
- Long operations get no progress, logs, cancellation or result summaries.
  - StartSummary, TeardownSummary and CLI stdout are discarded, so the Dry Run sync is a no-op.
  - One shared `isWorking` Bool covers every operation.
  - There is no polling and no refresh after failures.
  - The selected feature is stored by full value, so the detail view goes stale after any refresh or teardown.
  - Sheets, alerts and selection hang off the shared VM, so they leak across windows and the menu bar.
  - The command palette opens sheets from inside a sheet.
- `FeatureListView.swift` (349 lines) is dead code, and it is the only consumer of `isLoading`, so the live UI has no loading indicator.

**What works.** With a real agent built from this repo and run on a scratch port (127.0.0.1:50615), gRPC List and Status work: 2 active features, 8 with include_removed, in 0.01–0.04s. Forced-gRPC errors surface as the opaque "The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)".

**Tests.** 4 trivial tests. One decodes a hand-made payload with the old flat schema, so it does not catch the current CLI drift. Nothing covers the bridge, transport fallback, CLI argument construction, the view model or the views.

**Docs.** They point to devcontainer-only paths (`/workspaces/milestone2`), the wrong defaults key (`defaults write dev.branchbox.app workspace`; the code uses `branchbox.workspace`), and a "CLI" row tag that only exists in the dead view.


## Architecture notes
- ENTRY/SCENES (App/BranchBoxMacApp.swift): @main BranchBoxMacApp holds one @StateObject FeatureListViewModel (line 22), shared by all three scenes via environmentObject. The scenes are: WindowGroup{MainAppView} (29-31); Settings{SettingsView} (54-56); MenuBarExtra("BranchBox", systemImage: "shippingbox"){StatusMenuView}.menuBarExtraStyle(.window) (61-65), with a static icon and no status. The AppDelegate calls NSApp.activate(ignoringOtherApps:) on launch and requests notification auth only when Bundle.main.bundleIdentifier != nil (8-16). Commands are inserted with CommandGroup(after: .appInfo), i.e. into the App menu: Refresh Features ⌘R, Start Feature ⌘N (toggles commandStartRequested), Command Palette… ⌘K (33-49).
- NAVIGATION (Shared/AppSection.swift, Views/MainAppView.swift): AppSection {home, features, agent, settings} lives in a sidebar List bound to viewModel.selectedSection (not persisted; no @SceneStorage). NavigationSplitView is three-column. The content column switches on section. The detail column shows FeatureDetailView only for .features with a selectedFeature value snapshot (@State, line 5); otherwise PlaceholderDetail or EmptyView. Settings appears both as a sidebar section and as the native Settings scene. MainAppView carries 4 sheets: command palette, start, teardown(item: pendingTeardown), detect output. It also has the only .alert(item: activeAlert) and a toolbar with workspace menu, transport Picker (Automatic/Force gRPC/Force CLI), transport and control-plane badges, and Refresh.
- VIEW MODEL OWNERSHIP/LIFETIME (ViewModels/FeatureListViewModel.swift): @MainActor final class, app-lifetime. It owns `private let bridge: AgentBridge` created by default arg AgentBridge() → AgentConfiguration.detect(). Its published state includes features, isLoading (consumed only by dead FeatureListView), and isWorking (one Bool for all ops). Start-form fields (newFeatureName/Title, useMinimalMode, promptSeed, branchPrefix, skipModules, reuseExisting) are shared by Home, menu bar, StartFeatureSheet and palette. Other state: workspacePath, transportStatus (defaults .grpc), pendingTeardown, teardownOptions (persisted), promptHistory (max 5), controlPlaneStatus, transportPreference, UI intents (commandStartRequested, syncDevcontainerRequested [unused], isCommandPalettePresented, selectedSection), devcontainerStrategy, detectOutput/isDetectSheetPresented. All async work is launched as unstructured `Task {}` that is never stored or cancelled.
- TRANSPORT SELECTION (Agent/AgentBridge.swift): transportOverride is nil (Automatic), .grpc or .cliFallback, set from the persisted preference 'branchbox.transportPreference' (VM 89-95, 112-123). listFeatures/startFeature/teardownFeature work like this: if the override is .cliFallback, use the CLI; otherwise ensureClient() then make the gRPC call. On a thrown error: if forced gRPC, rethrow; else fall back to the CLI (154-272). The switch happens only on a thrown error, and with default grpc-swift settings (waitsForConnectivity, unlimited reconnect retries, no deadline) a dead agent never throws, so in practice fallback never happens (experiments B/B2/B3/I). syncDevcontainer is CLI-only via Task.detached (274-285). There is no health probe, no cached decision and no timeout. setTransportOverride resets the connection unless .grpc (287-292).
- gRPC LIFECYCLE: MultiThreadedEventLoopGroup(numberOfThreads: 1) per bridge, created eagerly even in CLI mode (134). ensureClient lazily builds ClientConnection.insecure(group:).withConnectionBackoff(maximum: .seconds(5)).connect(host:port:) and memoizes it without synchronization (294-306). resetConnection runs connection.close().wait() synchronously; it is called from MainActor via updateWorkspacePath/setTransportOverride (148-152, 287-292, 308-314). deinit runs close().wait() and group.syncShutdownGracefully() (138-144). Per refresh it makes 2 unary RPCs (List then Status). Start/Teardown responses (StartSummary/TeardownSummary) are discarded (222, 258).
- CLI FALLBACK (Agent/CLICompat.swift): run() does Process(/usr/bin/env, [cliBinary]+args), cwd=workspacePath, separate stdout/stderr Pipes, then waitUntilExit(), then readDataToEndOfFile (103-130). stdin is inherited and there is no env/PATH augmentation, timeout or cancellation. Binary resolution (132-148): $BRANCHBOX_CLI_PATH, then Bundle Resources/bin/branchbox, then 'branchbox' on PATH. The CLI calls made are: `feature list --json --repo <ws> [--all]` (decoded with convertFromSnakeCase + .iso8601); `feature start <name> --repo <ws> --json --no-summary [--title] [--minimal] [--branch-prefix] [--reuse] [--skip-module …] [--prompt]` (output discarded); `feature teardown <name> --repo <ws> [--force] [--complete-spec] [--delete-branch]` (never --keep-branch; output discarded); `--help` then `agent status --json` (errors swallowed, defaults returned); `devcontainer sync --path <ws> [--strategy] [--dry-run]` (output discarded); `detect --path <ws>`. A CLI-mode refresh therefore spawns 3 processes.
- WORKSPACE PATH / AGENT ADDRESS: AgentConfiguration.detect (AgentBridge.swift:13-30) reads the address from env BRANCHBOX_AGENT_GRPC_ADDR, default hardcoded '127.0.0.1:50515' (it matches agent/src/config.rs:12 DEFAULT_GRPC_ADDR), parsed by naive split(":"). Workspace comes from env BRANCHBOX_WORKSPACE, else UserDefaults 'branchbox.workspace', else FileManager.currentDirectoryPath ('/' when launched from Finder). includeRemoved is only env BRANCHBOX_SHOW_REMOVED=='1', with no UI. The VM then re-reads defaults 'branchbox.workspace' and overrides the bridge, so a stored default beats the env var (VM 84, 94). Other defaults keys: branchbox.promptHistory, branchbox.transportPreference, branchbox.teardown.force/completeSpec/deleteBranch, branchbox.devcontainerStrategy (default 'copy'). README/scripts hardcode the devcontainer path /workspaces/milestone2 (macos/README.md:19,37; scripts/start-agent-local.sh:5). The packaged bundle id is dev.branchbox.app (scripts/package-macos-app.sh:9); swift-run builds have no bundle id.
- REFRESH CADENCE: there is no timer, file watching (.branchbox/registry.json), app-activation refresh or menu-open refresh. Loads happen on: first .task in MainAppView/StatusMenuView (loadIfNeeded guard); ⌘R; the toolbar, Features and menu bar refresh buttons; the palette's Refresh; after a successful start/teardown/sync; on workspace change; on transport change. There is no refresh after a failed start or teardown. loadFeatures has no re-entrancy guard, so concurrent loads race and the last finisher wins.
- NOTIFICATIONS (Services/LocalNotifier.swift): UNUserNotificationCenter.current().add(...) is called unconditionally after a successful start ('<name> is ready'), teardown and devcontainer sync. There is no UNUserNotificationCenterDelegate, so nothing is presented while the app is frontmost. It crashes in unbundled (swift run) processes (experiment notif-test). Authorization is requested at launch, and only when bundled.
- COMMAND PALETTE (Views/CommandPaletteView.swift): a sheet with a plain TextField and a List of Buttons, filtered by substring on title/subtitle. Items: Start Feature… (sets commandStartRequested while the palette sheet is still up), Switch Workspace… (NSOpenPanel), Run detect, Refresh, Sync Devcontainer (global strategy, all worktrees), Go to Home/Features/Agent/Settings. When an 'active' feature has a URL it adds Open Active Feature and Teardown Active Feature…. Item ids are a fresh UUID per evaluation. There is no arrow/return keyboard navigation, no cancelAction button, and no per-feature actions beyond the first 'active' one.
- SCREEN: HomeView. Header shows the workspace path plus Reveal in Finder (NSWorkspace.selectFile) and Open in Terminal (`open -a Terminal <ws>`, try?). The setup card appears only if the path does not exist; it has Choose workspace… (NSOpenPanel → updateWorkspace → bridge.updateWorkspacePath → resetConnection → loadFeatures) and Open Settings (section switch). Quick start: TextField + Start (.defaultAction) → startFeatureQuick → startFeature → bridge.startFeature (gRPC Start or CLI `feature start`). Options… opens StartFeatureSheet. Recent activity is the first 5 features. Workspace health card: Agent connection (button 'Check agent' → Agent tab), Cloud sync (button 'View log' → Agent tab; there is no log), Workspace sync (button 'Sync now' → `devcontainer sync` for ALL worktrees). Tunnels card: Copy hostname (pasteboard) or 'View modules' (→ Features). Active feature card (first status=='active'): Open (Link to scheme-less URL), Sync devcontainer (all worktrees), Teardown… (sheet), Reveal in Finder, Open in Terminal, Copy path.
- SCREEN: FeaturesView. Segmented filter All/Active/Removed ('Removed' = status != active), search, Refresh. List with selection by full FeatureViewData value. Context menu: Open feature (NSWorkspace.open), Copy branch, Reveal in Finder, Open in Terminal, Copy path, Sync devcontainer (passes feature.syncStrategy but syncs all worktrees), Teardown… (sheet).
- SCREEN: FeatureDetailView. Read-only GroupBoxes: Basics (branch, feature URL link, adapter name, adapter service URL shown as a label, e.g. container-internal http://dev:3000), Devcontainer (summary/strategy/last sync, always 'Pending' with current CLI data), Modules (chips; 'ok' is green, everything else including 'success' is orange), Adapter warnings. Actions: Open, Copy branch, Sync devcontainer (all worktrees), Teardown… (sheet). It shows a snapshot that does not update after refresh.
- SCREEN: StartFeatureSheet. Name, title, Minimal mode, Reuse existing worktree, Branch prefix, Prompt seed, recent prompt chips, a 'Skip modules' Menu (Label with systemImage "" when off). Start normalises the name (lowercase, non-alnum → '-', but keeps Unicode letters), calls viewModel.startFeature(), and dismisses immediately. There is no progress. Missing: base branch, runtime, --no-worktree, --devcontainer-reuse, keep/reuse-runtime, telemetry, default prompt, and name validation via `branchbox name validate`.
- SCREEN: TeardownSheetView. Toggles Force removal / Complete spec / Delete branch (persisted across features). Cancel or Teardown → performPendingTeardown → bridge.teardownFeature (gRPC Teardown, or CLI `feature teardown` without --keep-branch). There is no explanation of consequences, no dry-run/preview and no progress; the sheet closes immediately.
- SCREEN: AgentStatusView. Status (Direct gRPC vs Fallback CLI; CP connected/pending; last error; outdated devcontainer count). Control plane events rows with copy buttons: last ack, last batch/cursor, last send, delivery, failure, error. The lastSent* rows can only appear via CLI because proto AgentStatus lacks them. Sync menu: 'Copy strategy'/'Symlink strategy' persist the strategy AND immediately run `devcontainer sync` on all worktrees; 'Dry Run' runs `--dry-run` but discards the output and shows a 'Sync completed' alert. Refresh button. There is no start/stop agent, no logs, and no 'Retry now' (a milestone3 deliverable).
- SCREEN: SettingsView (sidebar + Settings scene). Workspace path with Choose…, Reveal in Finder, Open in Terminal, Run detect (CLICompat.detectProject synchronously on MainActor → DetectOutputView sheet with Copy output). Devcontainer sync strategy segmented picker. Missing: CLI path override, agent address, transport, notifications, teardown defaults, branch prefix, runtime defaults.
- SCREEN: StatusMenuView (MenuBarExtra .window). Header dot shows Direct/Fallback (color only) plus Refresh. Workspace card: Choose…, Reveal, Terminal. Tunnel card: Copy hostname. Active feature card: Open, Teardown… (its own .sheet(item: pendingTeardown), duplicating MainAppView's), Finder/Terminal/Copy path. Quick start TextField + Start + '⋯' (sheet from the menu bar window). 'Open BranchBox…' sets selectedSection=.home + NSApp.activate; it does not reopen a closed window. A Sync menu duplicates the Agent tab's (runs sync on selection).
- DEAD/PLACEHOLDER CODE: Views/FeatureListView.swift (entire 349-line view, never instantiated; it holds the only loading spinner, the 'CLI' row badge, an extra teardown sheet and an alert); FeatureAction enum (FeatureModels.swift:159-162); syncDevcontainerRequested (VM:63); suggestedFeatureName returns "" (VM:158); the 4-arg statusRow overload (HomeView:229-231); FeatureViewData.Source is only read by dead code; PlaceholderDetail.
- AGENT SIDE (agent/src/grpc.rs, ops.rs, ipc.rs): the gRPC handlers call blocking FeatureWorkflow::start/teardown inline in async tonic handlers. RPCs are unary with no progress. build_start_request hardcodes runtime: None, devcontainer_reuse default, keep_runtime_on_failure false, workspace_mode default (ops.rs:72-89). list_features retains only Active when !include_removed (ops.rs:17-19); the CLI hides only Removed. The IPC FeatureRecord carries `runtime: RuntimeMetadata` (published_ports host:runtime) and AgentStatus carries last_sent_*; gRPC carries neither. The proto (agent/proto/agent.proto, last changed 2025-11-10) matches the checked-in Generated stubs (2025-11-11), so codegen itself has not drifted; the proto surface is simply frozen.

## Findings

### MAC-01 [critical/bug] Automatic transport never falls back to CLI: gRPC calls hang forever when the agent is not running
ensureClient builds a grpc-swift 1.27 ClientConnection with only a backoff max. The defaults are callStartBehavior = .waitsForConnectivity and ConnectionBackoff(retries: .unlimited), and the generated async client uses CallOptions() with no timeLimit. With nothing listening, list/start/teardown wait for connectivity indefinitely and never throw, so the catch → CLI fallback branch is never reached. Because Homebrew ships no agent, this is the default experience:
- empty lists, with transportStatus still at its .grpc default → toolbar 'gRPC' green, Home 'Agent connection • Online', menu bar 'Direct';
- Start hangs with isWorking=true, disabling Start/Refresh/Sync for the rest of the session;
- if the agent dies mid-session, subsequent calls also hang.

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:294-306 (ClientConnection.insecure(group:).withConnectionBackoff(maximum: .seconds(5)).connect), 162-196, 211-229, 250-271; .build/checkouts/grpc-swift (1.27.0) Sources/GRPC/ClientConnection.swift:413 `connectionBackoff: ConnectionBackoff? = ConnectionBackoff()`, :438 `callStartBehavior: CallStartBehavior = .waitsForConnectivity`; ConnectionBackoff.swift:85 `retries: Retries = .unlimited`; Generated/agent.grpc.swift:319 `defaultCallOptions: CallOptions = CallOptions()`. Experiments: 'AUDIT-B HUNG: listFeatures did not return within 45s (automatic transport, agent not running)', 'AUDIT-B3 HUNG: automatic listFeatures did not return within 180s', 'AUDIT-B2 HUNG: forced gRPC listFeatures did not return within 30s', 'AUDIT-I second list HUNG >40s after agent died (no CLI fallback)'. FeatureListViewModel.swift:51 `transportStatus: AgentBridge.Transport = .grpc` is only updated on success (130).

**Suggested fix:** Use `.withCallStartBehavior(.fastFailure)` and set `CallOptions(timeLimit: .timeout(.seconds(2)))` for List/Status, plus a longer one for Start/Teardown. Before choosing a transport, probe health with a short-deadline Status call. Cache the decision with a TTL and re-probe on failure. Make transportStatus an explicit `.unknown/.probing` state until the first result. Consider the unix-socket IPC, or CLI-first, as the default since the agent is not distributed.

**Verifier:** confirmed — I reproduced this with throwaway XCTests in scratch (verify-mac-app-code/macos/Tests/BranchBoxAppTests/VerifyTests.swift). The bridge was set to automatic, with a fake CLI in BRANCHBOX_CLI_PATH that prints the real `feature list --json`, and nothing listening on 127.0.0.1:59917. Results:
- `listFeatures()` gave 'VERIFY-01 automatic list (closed port): TIMEOUT after 40.0s'.
- The same bridge with `.cliFallback` forced returned '2 features via cliFallback' at once.
- A control connection with `.withCallStartBehavior(.fastFailure)` threw ConnectionFailure in 0.007s.
- Default behaviour plus `CallOptions(timeLimit: .timeout(.seconds(2)))` threw RPCTimedOut at 2.0s.
So the hang comes from waitsForConnectivity with no deadline, and the catch→CLI branch is never reached.

Agent dying mid-session: I ran a real branchbox-agent (built from repo source) on 127.0.0.1:50715 against a disposable repo. The first list returned '[caf, hello-world] via grpc'. After SIGTERM to the agent, the next list gave 'TIMEOUT after 40.0s'.

Defaults checked: grpc-swift 1.27.0 ClientConnection.swift:413/438, ConnectionBackoff.swift:85 (`.unlimited`) and agent.grpc.swift:319 all match the finding.

UI effect: transportStatus defaults to .grpc (FeatureListViewModel.swift:51) and is only set on success (130). isAgentConnected therefore reads true, giving 'Online' on Home and 'Direct' in the menu bar. isWorking stays true, so Start, Refresh and Sync stay disabled.

The Homebrew Cellar 0.13.4 bin holds only bb, branchbox and branchbox-local-vm. Critical severity is justified: the default transport makes the app non-functional unless the user finds 'Force CLI'.


### MAC-02 [high/bug] LocalNotifier crashes the app when run via the documented `swift run` (unbundled executable)
AppDelegate guards the authorization request with `Bundle.main.bundleIdentifier != nil`, but LocalNotifier.notify calls UNUserNotificationCenter.current() unconditionally. In an unbundled executable that raises NSInternalInconsistencyException. So in the README/AGENTS dev loop (`swift run BranchBoxApp`), the first successful start, teardown or devcontainer sync terminates the app right after the operation succeeds.

**Evidence:** macos/Sources/BranchBoxApp/Services/LocalNotifier.swift:6-12; App/BranchBoxMacApp.swift:13-15 (guard only on auth); call sites FeatureListViewModel.swift:249, 275, 311. Repro (scratch notif-test binary calling the same API): "*** Terminating app due to uncaught exception 'NSInternalInconsistencyException', reason: 'bundleProxyForCurrentProcess is nil: mainBundle.bundleURL file:///private/tmp/.../notif/'" exit=134. README.md:19 and docs/docs/getting-started/manual-cli-e2e.md:109 instruct `swift run BranchBoxApp`.

**Suggested fix:** Guard notify with `Bundle.main.bundleIdentifier != nil` (or wrap it in a Notifier service that no-ops when unbundled). Set a UNUserNotificationCenterDelegate so notifications appear while the app is frontmost. Ask for authorization on the first long-running operation, not at launch.

**Verifier:** confirmed — I compiled a scratch binary that calls the same API as LocalNotifier.swift:6-12. It printed 'bundleIdentifier=nil' and then '*** Terminating app due to uncaught exception 'NSInternalInconsistencyException', reason: 'bundleProxyForCurrentProcess is nil: mainBundle.bundleURL file:///private/tmp/.../verify-mac-app-code/notif/''.

The SwiftPM-built BranchBoxApp executable has no embedded Info.plist (`otool -s __TEXT __info_plist` is empty), so under `swift run` its bundleIdentifier is nil.

AppDelegate guards only requestAuthorization (BranchBoxMacApp.swift:13-15). notify is called unconditionally after a successful start, sync or teardown (FeatureListViewModel.swift:249, 275, 311). README.md:19 and manual-cli-e2e.md:109 both document `swift run BranchBoxApp`.

Scope note: the packaged .app sets CFBundleIdentifier (package-macos-app.sh:50-51), so only the documented dev loop crashes.
Corrected: This affects only unbundled runs (`swift run` / the .build executable). The packaged .app from scripts/package-macos-app.sh has a bundle identifier and does not crash.

### MAC-03 [high/bug] Teardown 'Delete branch' toggle has opposite semantics on CLI vs gRPC; with Force it can `git branch -D` unmerged branches
The CLI now deletes the branch by default (`delete_branch_by_default = true`) unless --keep-branch is passed. The app passes --delete-branch when the toggle is on and nothing when it is off, so the CLI deletes the branch either way. When 'Force removal' is on (persisted across teardowns), core calls `delete_branch(name, force_remove || force_delete_branch)`, which force-deletes unmerged branches. A user who unchecks 'Delete branch' to keep unmerged work loses the branch ref. Over gRPC the same UI keeps the branch (delete_branch=false is passed explicitly).

Without Force, the non-interactive CLI removes the worktree and then bails with 'Branch ... could not be deleted without force'. The app reports 'Teardown failed' even though the worktree is gone, and does not refresh.

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:42-55 (no --keep-branch; stale comment line 43 'The CLI does not support --json for teardown'); `branchbox feature teardown --help`: '--keep-branch  Keep the git branch after removing the worktree (default is to delete it)'; cli/src/commands/feature.rs:828-836 (delete_branch = default when neither flag), 860-871 (non-tty bail); core/src/config.rs:176-178 `fn default_teardown_delete_branch() -> bool { true }`; core/src/workflows/feature.rs:1243-1246 `.delete_branch(&branch_name, force_remove || force_delete_branch)`; agent/src/ops.rs:91-108 (gRPC passes delete_branch as given); FeatureListViewModel.swift:53-57, 398-402 (force persisted); TeardownSheetView.swift:12-14.

**Suggested fix:** Always pass an explicit `--keep-branch` or `--delete-branch` (and `--force-delete-branch` only on a separate, explicit opt-in). Use `feature teardown --json` and show the residue/branch_deleted result. Do not persist 'Force'. Default the UI from the repo's `.branchbox/config.json` teardown policy and state the consequence in the sheet.

**Verifier:** confirmed — I reproduced this in a disposable scratch repo using minimal-mode features (no docker; the docker ps count was 12 before and after). Each feature got an unmerged commit. I then ran exactly the argv the app builds when 'Delete branch' is off, non-tty via /usr/bin/env:
- `feature teardown tkeep --repo R` (no flags) → exit=1 with 'Error: Branch 'feature/tkeep' could not be deleted without force; rerun with `--force-delete-branch` (or `--force`).' The worktree was nonetheless removed and the registry showed 'tkeep removed'. The app would report 'Teardown failed' and not refresh.
- `feature teardown tforce --repo R --force` ('Force' on, 'Delete branch' off) → exit=0. `git branch` afterwards lists only feature/tkeep and main, so the unmerged feature/tforce was force-deleted.

Code checked:
- cli/src/commands/feature.rs:828-836 (default delete when neither flag is passed) and 860-871 (non-tty bail);
- core/src/config.rs:176-178 (default true);
- core/src/workflows/feature.rs:1246 `.delete_branch(&branch_name, force_remove || force_delete_branch)`;
- agent/src/grpc.rs:115-118 and ops.rs:91-108, which pass delete_branch through, so gRPC keeps the branch.

The CLICompat.swift:43 comment about --json is stale: `feature teardown --help` lists `--json`.


### MAC-04 [high/bug] CLI cannot be found when the app is launched from Finder/Dock (launchd PATH), and CLI subprocesses cannot find docker/devcontainer/op
run() executes `/usr/bin/env branchbox` with the inherited environment. GUI apps get PATH=/usr/bin:/bin:/usr/sbin:/sbin, so the Homebrew CLI at /opt/homebrew/bin is not found. Even with an embedded CLI (Resources/bin/branchbox), the CLI and core shell out to `docker` (/usr/local/bin, Docker Desktop), the devcontainer CLI (nvm path) and `op` (/opt/homebrew/bin), all missing from that PATH. The project's own init-host.sh already works around exactly this.

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:106-107, 132-148. Shell: `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env branchbox --version` → 'env: branchbox: No such file or directory' exit=127; `launchctl getenv PATH` → empty (launchd default). Experiment AUDIT-D: 'AUDIT-D threw: CLI fallback failed: env: branchbox: No such file or directory'. `which docker` → /usr/local/bin/docker; devcontainer → ~/.nvm/versions/node/v22.16.0/bin/devcontainer; op → /opt/homebrew/bin/op. core/src/bootstrap/templates/common/init-host.sh:5 `export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:$HOME/.local/bin"`.

**Suggested fix:** Resolve the CLI to an absolute path: check the settings override, the embedded binary, then /opt/homebrew/bin, /usr/local/bin and ~/.cargo/bin, then `zsh -lc 'command -v branchbox'`. Build a child environment whose PATH is captured once from the user's login shell, or at least prepend /opt/homebrew/bin:/usr/local/bin. Expose the resolved CLI path and version in Settings with a 'Locate…' button.

**Verifier:** confirmed — `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env branchbox --version` gives 'env: branchbox: No such file or directory' exit=127. `launchctl getenv PATH` is empty, so the launchd default PATH applies.

CLICompat.swift:106-107 runs /usr/bin/env with the bare name 'branchbox' (resolveCLIBinary at 147) and the inherited environment.

Tool locations on this machine: `which` shows docker at /usr/local/bin/docker, devcontainer under ~/.nvm/... and op at /opt/homebrew/bin/op. Core invokes tools by bare name, which depends on PATH:
- `Command::new("git")` and `("op")`;
- docker_path defaults to "docker" (core/src/devcontainer_runtime/docker.rs:74).

init-host.sh:5 already prepends /usr/local/bin and /opt/homebrew/bin for exactly this reason. I did not launch the GUI from Finder; the PATH behaviour is the standard launchd environment.


### MAC-05 [high/bug] Process pipe deadlock: waitUntilExit() before draining stdout/stderr; no timeout or cancellation
CLICompat.run calls process.waitUntilExit() and only then reads stdout, and reads stderr only on failure. Once the child writes more than the pipe buffer (~64KB) to either stream, it blocks on write while the app blocks on exit: a permanent deadlock.

Reachable cases:
- `feature list --json --all`: about 2KB per feature, so roughly 32+ historical features with BRANCHBOX_SHOW_REMOVED=1;
- verbose RUST_LOG or telemetry on stderr;
- large detect/sync output.

Other problems in the same path:
- these synchronous calls run on Swift-concurrency cooperative threads (listFeatures/start/teardown are nonisolated async but call CLI synchronously), so they block the pool;
- there is no timeout and no way to cancel a minutes-long start;
- stdin is inherited (with swift run it is the terminal);
- on success, stderr warnings are discarded.

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:110-129 (`process.waitUntilExit()` at 121, then `readDataToEndOfFile()` at 122/124). Experiments: 'AUDIT-C[small] returned 6 bytes after 0.17s'; 'AUDIT-C[bigstdout] DEADLOCK: CLICompat.run did not return within 10s'; 'AUDIT-C[bigstderr] DEADLOCK: CLICompat.run did not return within 10s' (200KB fake CLI via BRANCHBOX_CLI_PATH). Sizes: `branchbox feature list --json | wc -c` → 4096 (2 features); `--all` → 16514 (8 features). Blocking calls from async context: AgentBridge.swift:157, 195, 205, 228, 238, 264, 316-326.

**Suggested fix:** Rewrite the runner as an async CLI runner. Read stdout and stderr concurrently (readabilityHandler or AsyncBytes) while the process runs. Use terminationHandler plus a continuation instead of waitUntilExit, and set standardInput = FileHandle.nullDevice. Support Task cancellation (terminate, then SIGKILL after a grace period) and per-command timeouts. Stream stderr lines to an activity log in the UI.

**Verifier:** confirmed — I ran CLICompat.detectProject through fake CLIs set via BRANCHBOX_CLI_PATH, with an 8s watchdog:
- fake-60k.sh: 'RETURNED 60000 bytes';
- fake-70k.sh: 'TIMEOUT after 8.0s';
- 200KB on stdout: 'TIMEOUT';
- 200KB on stderr followed by '[]': 'TIMEOUT'.
The deadlock appears just above the ~64KB pipe buffer, as described for CLICompat.swift:121-124 (waitUntilExit before readDataToEndOfFile).

The strict-concurrency build warns 'sending self.bridge risks causing data races' at FeatureListViewModel.swift:128/238/271/308. That confirms the bridge's async methods run off the main actor, where they block cooperative threads synchronously. stdin is not set, there is no timeout or cancel, and stderr is dropped on success; all confirmed in code.

Reachability caveat: real list output is about 2KB per feature (4096 bytes for 2 active; 16514 for 8 with --all). Minimal-start stderr in sandbox captures was 1-3KB, even though the CLI logs info-level tracing to stderr by default (cli/src/main.rs:78-82). Typical small repos will not hit the deadlock; many features, debug RUST_LOG or large output will. The 'high' rating is defensible given the permanent hang, but it is not an everyday occurrence.


### MAC-06 [medium/bug] runDetect executes the CLI synchronously on the MainActor
runDetect creates `Task { ... }` inside a @MainActor class, so the task inherits MainActor. It then calls the synchronous CLICompat.detectProject, which calls waitUntilExit() on the main thread. waitUntilExit spins the current run loop while waiting, so the UI either stalls or re-enters event handling. Strict concurrency does not flag this because the call is synchronous.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:352-365 (line 357 `let output = try CLICompat.detectProject(path: self.workspacePath)`); CLICompat.swift:98-101, 121.

**Suggested fix:** Route every CLI call through the async runner (MAC-05) off the main actor, e.g. a `nonisolated func detect() async throws` on a `CLIClient` actor.

**Verifier:** confirmed — In a @MainActor XCTest I built a FeatureListViewModel with a fake CLI that runs `sleep 3`, queued main-queue ticks for 0.5, 1.0, 1.5 and 2.0s, then called vm.runDetect(). Every tick fired late: 'tick1@3.17s main=y ... tick4@3.18s'. The main thread was blocked for the whole CLI run, and the result arrived (detectOutput='detect-ok', sheet=true).

The code matches: runDetect's `Task {}` inside the @MainActor class inherits MainActor, and it calls the synchronous CLICompat.detectProject (FeatureListViewModel.swift:357 → CLICompat.swift:98-101, 121). What we observed is a stall rather than re-entrancy.


### MAC-07 [high/drift] CLI JSON schema drift: tunnel always nil, devcontainer always 'Pending', modules always '0 ok', module chips orange
The CLI feature JSON (0.13.4) nests `tunnel{provider,status,notes,last_updated,hostname?}`. It no longer emits last_sync_at or sync_strategy, uses module statuses success/skipped/failed, and adds runtime, default_agent, base_branch, created_at, color, last_commit, last_summary_rendered_at and env_path.

The app's decoder expects flat tunnel_status/tunnel_provider/tunnel_hostname, so tunnel data is lost. devcontainerStatusSummary falls to 'Pending'. moduleSummary counts only 'ok', so every feature reads '0 ok', and detail chips colour anything but 'ok' orange. In CLI mode the tunnel card shows 'Unknown provider / Unknown status' for the active feature.

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:150-167 (flat tunnel fields, lastSyncAt, syncStrategy); FeatureModels.swift:39-51 (counts "ok"), 53-61; FeatureDetailView.swift:87; FeatureListViewModel.swift:201-215. Real output: `"tunnel": {"provider": "cloudflared","status": "disabled",...}`, `"module_outcomes": [{"module": "devcontainer","status": "success",...}]`, `"runtime": {"provider": "container"}`. core/src/workflows/feature.rs:59-66 (ModuleStatus Display success/skipped/failed). Experiment AUDIT-A: 'decoded 2 records; first tunnelStatus=nil tunnelProvider=nil'; AUDIT-A2: 'prine status=Active modules=0 ok devcontainer=Pending tunnelProvider=nil tunnelStatus=nil'; AUDIT-G: '[success,success,success,skipped] -> '0 ok''. The gRPC path also gives 'modules=0 ok devc=Pending' (AUDIT-H1).

**Suggested fix:** Define one Codable FeatureRecord mirroring the CLI's serde FeatureMetadata (nested tunnel, runtime.published_ports, default_agent, base_branch, created_at, color, last_commit), with tolerant decoding (decodeIfPresent, unknown-enum fallback). Map module status as an enum {success, skipped, failed, unknown}. Add a golden test that decodes captured `feature list --json` output from the current CLI.

**Verifier:** partially_confirmed — Most of this reproduced. I decoded the real captured `feature list --json` (0.13.4) through the app's own CLICompat.featureList and FeatureViewData(cli:). Both features gave 'modules=0 ok devc=Pending tunnelProvider=nil tunnelStatus=nil' with chips ['devcontainer:success:orange', 'compose:success:orange', 'specs:success:orange', 'tunnel:skipped:orange']. With prine active, tunnelSummary falls back to 'Unknown provider' / 'Unknown status' (FeatureListViewModel.swift:207-208). Over gRPC (real agent) I also got 'modules=0 ok', but the tunnel did come through ('disabled'), because the agent flattens it (agent/src/grpc.rs:193-206).

One claim is wrong: last_sync_at and sync_strategy were not removed. FeatureMetadata still has both as Option with skip_serializing_if (core/src/workflows/feature.rs:5478-5482). `devcontainer sync` records them (cli/src/commands/devcontainer.rs:619 → feature.rs:5647-5649), and they are then emitted. So the devcontainer badge is 'Pending' only until the first successful sync, not always.
Corrected: Tunnel data is lost on the CLI path (nested `tunnel{}` versus flat fields). Module summary reads '0 ok' and chips are orange on both transports, because statuses are success/skipped/failed. last_sync_at and sync_strategy still exist and appear in the JSON after a `devcontainer sync`, so 'Pending' shows only for never-synced features (the normal state right after start), not for every feature.

### MAC-08 [high/drift] New feature statuses (degraded, failed_retained, orphaned) are hidden on gRPC, mislabelled on CLI, and unstyled
The agent's list keeps only Active unless include_removed is set, while `feature list` hides only Removed. So degraded, failed_retained and orphaned features vanish over gRPC but appear over CLI.

In the UI:
- the Features 'Removed' filter is `status != active`, so on CLI it shows degraded/orphaned as 'Removed', and it can never show real removed features (includeRemoved is env-only);
- styling is binary: active is green, everything else grey;
- statusLabel is `.capitalized`, so 'Failed_Retained';
- activeFeature ignores degraded features;
- there is no affordance for the recovery actions these states need: teardown residue, reuse-runtime, inspect.

**Evidence:** agent/src/ops.rs:17-19 `entries.retain(|feature| feature.status == FeatureStatus::Active)`; cli/src/commands/feature.rs:552-554 (CLI retains != Removed); `branchbox feature list --help`: '--status <STATUS>  Filter by status (active, degraded, failed_retained, orphaned, removed)'; macos/Sources/BranchBoxApp/Views/FeaturesView.swift:12, 62-68; FeatureModels.swift:30-32; AgentBridge.swift:22 (BRANCHBOX_SHOW_REMOVED only). AUDIT-G: 'status failed_retained -> label 'Failed_Retained''.

**Suggested fix:** Model FeatureStatus as an enum with display names, colors and SF Symbols per state. Make the agent and the CLI agree on filtering (pass an explicit status filter). Replace the filter with Active / Needs attention / Removed, where Removed fetches with include_removed. Add state-specific actions (e.g. 'Retry with retained runtime', 'Clean up orphan').

**Verifier:** confirmed — Code and CLI checks:
- agent/src/ops.rs:17-19 retains only Active unless include_removed is set.
- cli/src/commands/feature.rs:552-553 retains `!= Removed`, and `feature list --help` says retained and orphaned features are shown by default.
- FeaturesView.swift:12, 62-68 defines 'removed' as `status != active`; the pills are binary green/grey (line 81).
- activeFeature matches only 'active' (VM:143-145).
- includeRemoved comes only from the BRANCHBOX_SHOW_REMOVED env var (AgentBridge.swift:22).

In a scratch test, statusLabel for 'failed_retained' printed 'Failed_Retained'. Over gRPC, the real agent listed only the two active sandbox features and hid the removed ones.


### MAC-09 [high/missing_feature] App/proto surface frozen at v0.4: missing runtime providers, host ports, base branch, exec, prune, tunnels, devcontainer up/down, default agent
The proto has only List/Start/Teardown/Status. The app sends neither base_branch nor telemetry, even though the proto has them. Missing versus CLI 0.13.4:
- `--runtime container|sbx|local-vm` and display of runtime.provider/runtime_id/published_ports (resolved host-port mappings, needed for real 'Open in browser' URLs);
- `--base`, `--no-worktree`, `--devcontainer-reuse <fail|preserve|overwrite|inspect>` (copy-mode `--reuse` now fails by default on divergence and the app has no UI to choose), `--keep-runtime-on-failure` / `--reuse-runtime`, `--default-prompt`;
- `feature exec` (open shell / run command), exec-provider, dispatch-tool;
- `prune` / `feature prune --dry-run`;
- `tunnel open/remove`;
- `devcontainer up/exec/down/build/detect` (start/stop a feature's containers);
- the `default_agent` block (status/label/command/detail);
- teardown `--json` residue evidence;
- `init` onboarding (including 1Password prompts);
- `name generate/validate`;
- last_commit, color, created_at, env_path, compose_project_name (most exist in the proto but are unused).
The gRPC agent also hardcodes runtime: None and other defaults.

**Evidence:** agent/proto/agent.proto:5-10 (4 RPCs), 21-32 (StartRequest has base_branch/telemetry), 52-75 (no runtime/ports/default_agent); macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:213-221 (no base_branch/telemetry); agent/src/ops.rs:72-89 (`runtime: None`, `devcontainer_reuse: Default::default()`, `keep_runtime_on_failure: false`); `branchbox feature --help` lists start/teardown/list/prune/exec/exec-provider/dispatch-tool; `branchbox devcontainer --help` lists up/exec/down/build/...; core/src/runtime/mod.rs:77-102 RuntimeMetadata with published_ports {host, runtime}; CHANGELOG.md 0.11.0 'Runtime provider, runtime identity, and resolved host-port mappings in feature registry records, CLI summaries, JSON output, and agent IPC payloads'; agent/src/ipc.rs:344 (IPC FeatureRecord has runtime, gRPC Feature does not).

**Suggested fix:** Choose one contract. Recommended: make the CLI's `--json` outputs (list/start/teardown/exec/prune/tunnel) the app's API, behind a typed CLIClient with golden tests. If gRPC stays, extend the proto (runtime, ports, statuses enum, default_agent, base branch, streaming progress RPCs) and regenerate. Add UI for runtime choice, base branch, exec/shell, prune, tunnel open/remove and devcontainer up/down.

**Verifier:** confirmed — agent.proto:5-10 has 4 RPCs. StartRequest has base_branch and telemetry (21-32). Feature (52-75) has no runtime, ports, default_agent or last_commit. The app never sets base_branch or telemetry (AgentBridge.swift:213-221).

The CLI help confirms the missing surface:
- `feature --help` lists prune/exec/exec-provider/dispatch-tool;
- `devcontainer --help` lists up/exec/down/build/detect;
- `feature start --help` has --base, --no-worktree, --devcontainer-reuse (default: fail), --keep-runtime-on-failure, --reuse-runtime, --default-prompt and --runtime;
- `feature prune --help` has --dry-run; `tunnel` has open/remove; `name` has generate/validate.

ops.rs:72-89 hardcodes `runtime: None`, `devcontainer_reuse: Default::default()` and `keep_runtime_on_failure: false`. RuntimeMetadata (core/src/runtime/mod.rs:77-102) carries published_ports, and the IPC FeatureRecord has `runtime` (agent/src/ipc.rs:344) while the gRPC Feature does not.

Minor nit: last_commit is not in the proto. The finding's 'most exist' wording is accurate.


### MAC-10 [high/ux] No progress, logs, results or cancellation for minutes-long start/teardown/sync
Long operations give the user almost nothing. The only feedback is a global `isWorking` Bool that disables buttons; there is no spinner because isLoading is only used by dead FeatureListView.
- StartFeatureSheet and TeardownSheet dismiss immediately.
- Results are thrown away: StartSummary (warnings, failed/skipped modules, URLs, adapter, tunnel), TeardownSummary/module_reports, CLI stdout and stderr warnings, and devcontainer sync/dry-run output.
- The success notification says '<name> is ready' even when modules failed.
- There is no cancel, no activity/log panel, and no per-operation state. Concurrent operations clobber isWorking: the first to finish re-enables the buttons.
- Errors arrive as one modal alert containing raw stderr.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:41, 234-255, 266-281, 304-317; AgentBridge.swift:222 `_ = try await client.start(request)`, 258; CLICompat.swift:39, 54, 95 (`_ = try run(...)`); StartFeatureSheet.swift:65-69; MainAppView.swift:47; Views/FeatureListView.swift:49,175 (only isLoading consumer; never instantiated).

**Suggested fix:** Introduce an Operation model (id, kind, feature, state, startedAt, log lines, result, cancel handle) held in an OperationsStore. Show it as a per-feature inline progress row plus an Activity/Log inspector. Stream CLI stderr lines (or add a server-streaming gRPC). Render the parsed StartSummary/TeardownSummary in the feature detail. Add a Cancel button that terminates the process. Send notifications from results (success, warnings, failures).

**Verifier:** confirmed — isLoading is consumed only by FeatureListView.swift:49/175. That view is never instantiated: grep finds only its own definition, and there is no other ProgressView.

Other points checked in code:
- StartFeatureSheet.start() calls startFeature() and then close() immediately (65-69).
- Results are discarded: `_ = try await client.start/teardown` (AgentBridge.swift:222, 258) and `_ = try run` (CLICompat.swift:39, 54, 95).
- The notification says '\(trimmedName) is ready' whatever the module outcomes (VM:249).
- Teardown… buttons and palette actions are not gated by isWorking, so operations can overlap, and each one sets isWorking=false when it finishes (VM:254, 280, 316).
- Errors appear as a single Alert containing raw stderr.


### MAC-11 [high/bug] Data races on AgentBridge mutable state; blocking NIO waits on the main thread; leaked connections
AgentBridge is a non-Sendable final class with mutable configuration/client/connection/transportOverride.
- It is mutated on the MainActor by updateWorkspacePath and setTransportOverride.
- It is read and mutated on the global executor inside the nonisolated async listFeatures/startFeature/teardownFeature → ensureClient.
- Two overlapping refreshes can both see client == nil and create two ClientConnections; one is never closed.
- resetConnection calls `connection.close().wait()` synchronously from the MainActor, a blocking future wait on the UI thread, and can nil out the client mid-call on another thread.
- deinit also calls `.wait()`; that crashes via an NIO precondition if it ever runs on an event-loop thread.
Swift 6 strict concurrency flags the crossings.

**Evidence:** swift build -Xswiftc -strict-concurrency=complete (scratch copy): 'ViewModels/FeatureListViewModel.swift:128:43: warning: sending 'self.bridge' risks causing data races; this is an error in the Swift 6 language mode' (same at :238:39, :271:39, :308:39). AgentBridge.swift:55 (non-Sendable class), 125-130 (mutable state), 138-144, 148-152, 287-292, 294-306, 308-314.

**Suggested fix:** Make AgentBridge an `actor` (or @MainActor-confined with async I/O); hold the GRPCChannel created once (GRPCChannelPool.with or ClientConnection) and shut it down with `try await channel.close().get()`; never `.wait()` on main; don't reset the gRPC connection on workspace change (address doesn't change).

**Verifier:** confirmed — A strict-concurrency build in scratch (`swift build -Xswiftc -strict-concurrency=complete`) reproduced exactly 'FeatureListViewModel.swift:128:43 / 238:39 / 271:39 / 308:39: warning: sending 'self.bridge' risks causing data races; this is an error in the Swift 6 language mode'.

AgentBridge is a plain final class (55) with mutable configuration/client/connection/transportOverride (125-130). It is mutated from the MainActor (updateWorkspacePath, and setTransportOverride → resetConnection with `connection.close().wait()` at 311) and read and written inside nonisolated async methods via ensureClient (294-306), so there is real unsynchronized access.

Caveats:
- The double-ClientConnection race window is narrow, since ensureClient has no suspension point.
- The deinit `.wait()` concern is hypothetical: the bridge is owned by a @StateObject VM for the app's lifetime and its deinit essentially never runs.
Corrected: The races and the main-thread `.wait()` in resetConnection are real. The leaked-connection race is low-probability. The deinit crash is theoretical, because the bridge lives for the whole app lifetime.

### MAC-12 [medium/bug] loadFeatures races and lies about status: concurrent loads, stale overwrites, transport badge never updated on error
refresh(), setTransportPreference and updateWorkspace each start a new unstructured Task { await loadFeatures() }. There is no de-duplication or cancellation, and the last load to finish wins. Switching workspace A→B while A's load is slow can show A's features under B's path. isLoading is cleared by the first finisher.

On error, features, transportStatus and controlPlaneStatus are left at their previous values. A failed CLI load therefore still shows 'gRPC' green, and an old list remains visible next to an error alert.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:106-110, 119-122, 125-139, 320-327; MainAppView.swift:85-95 badges derive from transportStatus.

**Suggested fix:** Keep a single `loadTask` and cancel it before starting a new one. Tag requests with a generation or workspace and drop stale results. Model load state explicitly (idle/loading/loaded(transport)/failed(error)) and render badges from that state.

**Verifier:** confirmed — refresh() (106-110), setTransportPreference (120-122) and updateWorkspace (324-326) each start an unstructured Task that calls loadFeatures, with no cancellation or generation check. Each load reads configuration.workspacePath when it starts (AgentBridge.swift:165/318) and writes features unconditionally (VM:129), so the last load to finish wins. isLoading is cleared by whichever load finishes first (VM:138).

In the catch path (132-137), features, transportStatus and controlPlaneStatus are left untouched, and the toolbar badges derive from transportStatus (MainAppView.swift:85-95). Verified by tracing the code; the GUI was not run.


### MAC-13 [medium/ux] Errors swallowed or shown poorly (opaque GRPCStatus, misleading titles, raw stderr in alerts)
The app's error handling loses or misstates most failures.
- Forced-gRPC errors display 'The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)', so the agent's message (e.g. 'Not a git repository') is lost.
- In Automatic mode the gRPC error is only logged; the user sees the CLI error.
- Every list failure is titled 'Unable to reach agent', even when it is a CLI or repo problem.
- A missing working directory surfaces as 'CLI not runnable: The file “X” doesn’t exist.'
- agentStatusOrDefault swallows all errors and returns 'not configured' defaults.
- openInTerminal uses `try?`, and revealInFinder ignores its Bool result.
- Long multi-line stderr lands in a modal Alert with no copy option or log.
- A successful sync produces a modal 'Sync completed' alert.

**Evidence:** Experiment AUDIT-H4: 'threw in 0.01s: The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)'; AUDIT-H3: 'CLI fallback failed: Error: Validation error: Not a git repository: /'. cwdtest: 'run threw: The file “milestone2” doesn’t exist.' Code: AgentBridge.swift:190-195; CLICompat.swift:115-119, 57-85 (71-73 `// ignore and return defaults`); FeatureListViewModel.swift:133-136, 272, 427-432; MainAppView.swift:57-59.

**Suggested fix:** Map GRPCStatus to `status.message`/code. Keep a structured AppError (source, command, exit code, stderr tail) with a 'Show details/Copy' affordance. Use inline banners instead of alerts for recoverable problems. Do not alert on success. Validate the cwd before spawning.

### MAC-14 [medium/bug] Workspace selection: inconsistent precedence, '/' default from Finder, no repo validation or onboarding
How the workspace is chosen is inconsistent and unvalidated.
- AgentConfiguration.detect prefers env BRANCHBOX_WORKSPACE over defaults, but the VM prefers defaults over the bridge's value and then overrides it, so the env var is ignored once a default exists.
- With nothing set, the workspace is the process cwd: '/' from Finder. `fileExists('/')` is true, so the setup card is hidden and the user gets 'Not a git repository: /'. workspaceDisplayName shows '/'.
- workspaceNeedsSetup only checks existence. There is no check that the path is a git repo or BranchBox-initialised, no `init` flow, no recent-workspaces list, and only a single workspace (the user has several repos).
- Picking the parent dir of a parent-structure layout (project/ containing main/ and feature dirs) fails.
- For swift-run builds UserDefaults uses the executable's domain, not dev.branchbox.app (likely; not runtime-verified).

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:19-21; FeatureListViewModel.swift:84-85, 94, 160-164, 190-192, 320-327; `cd / && branchbox feature list --json --repo /` → 'Error: Validation error: Not a git repository: /' exit=1; HomeView.swift:20-22.

**Suggested fix:** Use one precedence (explicit UI choice, then env, then last used) and validate with `branchbox detect`/`feature list` before accepting a path. Offer an onboarding flow (choose repo, detect, `init` if needed). Keep a recent-workspaces list and a workspace switcher. Normalise to the main worktree root.

**Verifier:** confirmed — Precedence: in a scratch test, AgentConfiguration.detect with both env and defaults set returned 'workspace=/from/env' (env wins at the bridge). The VM, however, takes `defaults.string(...) ?? bridge.workspacePath` (VM:84) and pushes it into the bridge (94), so env is ignored once a default exists.

Root path: `FileManager.fileExists("/")` is true and `URL(fileURLWithPath: "/").lastPathComponent` is '/', so the setup card is hidden and the display name is '/'.

CLI checks:
- `branchbox feature list --json --repo /` → 'Error: Validation error: Not a git repository: /' exit=1.
- The parent layout dir ~/projects/branchbox-suite/branchbox (containing main/prine/remotion) → 'Not a git repository'.

The UserDefaults-domain point under `swift run` is still unverified, as the finding says. Separately, manual-cli-e2e.md:107 writes the key `workspace`, not `branchbox.workspace`.


### MAC-15 [medium/bug] Devcontainer-only paths and host/container path mismatch break Finder/Terminal actions
README tells users to set BRANCHBOX_WORKSPACE=/workspaces/milestone2 and to run the agent inside the devcontainer (start-agent-local.sh binds 0.0.0.0:50515 with workspace /workspaces/milestone2). In that setup:
- worktree_path values are container paths, so Reveal in Finder, Open in Terminal and Copy path act on paths that do not exist on the host;
- workspaceNeedsSetup is true;
- a host path chosen via NSOpenPanel is sent as repo_path to a container agent, which fails.
The user's real registry already contains removed features with /workspaces/... worktree paths.

**Evidence:** macos/README.md:15-19, 37 ('the workspace path should be `/workspaces/milestone2`'); scripts/start-agent-local.sh:5,9; experiment AUDIT-H2 (real agent, include_removed): 'cli-e2e-rust-smoke status=removed ... worktree=Optional("/workspaces/cli-e2e-rust-smoke")'; FeatureListViewModel.swift:413-432 (no existence check before acting).

**Suggested fix:** Treat container-side agents as unsupported for the desktop app, or add path mapping. Disable host-file actions when `FileManager.fileExists(worktreePath)` is false. Remove the milestone2 references from README and scripts.

**Verifier:** confirmed — README.md:19 and :37 reference /workspaces/milestone2. scripts/start-agent-local.sh:5 and :9 default WORKSPACE=/workspaces/milestone2 and GRPC_ADDR=0.0.0.0:50515.

The real registry (`feature list --json --all`) contains removed cli-e2e-rust-smoke* entries with worktree_path /workspaces/... .

revealInFinder and openInTerminal (VM:413-432) do no existence check, and the UI only checks `worktreePath != nil`.

Scope: this hits the documented legacy devcontainer-agent setup, plus removed features, which appear only with BRANCHBOX_SHOW_REMOVED=1. The default host CLI path is unaffected. Medium severity is reasonable.


### MAC-16 [medium/bug] Feature selection by full value; detail view shows stale/removed feature after refresh
Selection is keyed on the whole struct rather than its id, so any refresh that changes a feature's fields breaks the link between list and detail.
- FeaturesView binds `List(selection:)` to FeatureViewData?, and rows are tagged with the full struct. FeatureViewData's Hashable is synthesised over all fields.
- After a refresh that changes updatedAt, devcontainerOutdated, status and so on, the stored selection matches no row, so the highlight is lost.
- MainAppView keeps passing the old snapshot to FeatureDetailView. After teardown, the detail still shows the removed feature with an enabled Teardown… button.

**Evidence:** macos/Sources/BranchBoxApp/Views/MainAppView.swift:5, 27-29; FeaturesView.swift:8, 29, 42; Agent/FeatureModels.swift:3 (`struct FeatureViewData: Identifiable, Hashable`).

**Suggested fix:** Store `selectedFeatureID: String?`. Look the feature up in viewModel.features for the detail view, and show an empty state when it disappears.

**Verifier:** confirmed — In a scratch test, two FeatureViewData values with the same workFeature and different updatedAt gave 'same id=true equal=false hashEqual=false'. FeaturesView tags rows with the full struct (42) and binds `List(selection: $selected)` of FeatureViewData? (8, 29).

After any refresh that changes fields, the stored selection matches no row. MainAppView passes the stale @State snapshot to FeatureDetailView (27-29), and that view's Teardown… button (FeatureDetailView.swift:122) is never disabled. Verified in code and by the equality test; GUI highlight behaviour was not observed.


### MAC-17 [medium/bug] No state refresh after failed start/teardown, even though failures often leave partial state
loadFeatures is only called on success. Failed starts can leave failed_retained or degraded entries and worktrees, and the CLI's non-interactive branch bail happens after the worktree is removed (MAC-03). The list stays stale until a manual refresh.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:237-253 (loadFeatures only in the do-branch), 307-315; cli/src/commands/feature.rs:860-871.

**Suggested fix:** Refresh in a `defer` or finally path after every mutating operation, and show the failed operation's result in context.

**Verifier:** confirmed — loadFeatures runs only in the do-branches (VM:247, 309), so nothing refreshes on failure.

My MAC-03 repro shows the partial-state case is real. The non-tty teardown without --keep-branch exited 1 ('Branch ... could not be deleted without force'), but the worktree was already removed and the registry already showed status 'removed'. The app would show 'Teardown failed' and keep listing the feature as active until a manual refresh.


### MAC-18 [medium/bug] Shared start-form state leaks between entry points; inconsistent and incorrect name normalisation
All start surfaces write into the same view-model form state, and only one of them normalises names.
- Home, menu bar quick start, StartFeatureSheet and the palette all write into the same VM fields.
- Cancelling the Options sheet does not reset title, promptSeed, branchPrefix, skipModules, minimal or reuse, so a later 'quick' start silently applies the hidden options.
- Quick start does not normalise names ('Hello World' fails CLI validation). The sheet's normaliser keeps Unicode letters and digits (é, ß, Arabic digits), but the CLI requires a-z, 0-9 and '-'.
- `branchbox name generate/validate` exist but are unused, and suggestedFeatureName is a placeholder returning "".

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:43-49, 158, 217-232, 258-263; StartFeatureSheet.swift:47, 65-85; HomeView.swift:224-227; StatusMenuView.swift:177; `branchbox name validate "Hello World"` → '✗ Invalid feature name: Hello World / Feature names must be DNS-safe (lowercase a-z, 0-9, hyphens only)'.

**Suggested fix:** Use a value-type StartFeatureDraft per presentation, reset on cancel. Validate and normalise with the same rules as the CLI (or call `name validate`) inline as the user types. Generate names from the title via `name generate`.

**Verifier:** partially_confirmed — Shared form state confirmed. Home quick start, the menu bar quick start, StartFeatureSheet and the palette all go through VM fields (43-49, 217-232, 258-263). Sheet Cancel (StartFeatureSheet.swift:47 → dismiss) never resets title, promptSeed, branchPrefix, skipModules, minimal or reuse, so a later quick start applies them silently. suggestedFeatureName returns "" (158).

The normalisation sub-claim is refuted. In a disposable repo, `branchbox feature start "Hello World" --minimal --json` exited 0 and created work_feature 'hello-world' on branch feature/hello-world. Core's resolve_work_feature (core/src/workflows/feature.rs:1592-1603) treats an invalid name as a title and slugifies it; only `name validate` rejects it. `feature start "café"` succeeded as 'caf'. The agent uses the same core path.

The real naming problem is a silent rename: the app's notification says '\(trimmedName) is ready' using the raw input, and Unicode letters are stripped.
Corrected: The hidden-options leak between start entry points is real. Unnormalised names do not fail: the CLI and agent auto-slugify them ('Hello World' → hello-world, 'café' → caf). The issue is an unannounced rename and mismatched notification text, not a validation error. Inline preview via `name generate` is still the right fix.

### MAC-19 [medium/ux] Single 'Active feature' concept is wrong for multi-feature workflows
activeFeature is simply the first feature whose status is 'active'. BranchBox supports many concurrent features; this user has prine and remotion both active.
- The Home 'Active feature' card, the menu bar card and the palette's 'Open/Teardown Active Feature' all target an arbitrary one.
- tunnelSummary prefers activeFeature even when it has no tunnel. So the 'No tunnels detected' empty state never shows while any feature is active, and you get 'cloudflared · disabled' (gRPC) or 'Unknown provider · Unknown status' (CLI) instead.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:143-145, 201-215; HomeView.swift:31-35, 144-163; StatusMenuView.swift:16-18; CommandPaletteView.swift:80-89; `branchbox feature list --json` shows 2 features with status "active".

**Suggested fix:** Replace it with a 'Running features' list (sorted by recency), each with quick actions. Show the tunnel card only for features whose tunnel status is not disabled. In the palette, offer per-feature commands ('Open prine', 'Teardown remotion…').

### MAC-20 [medium/ux] Teardown confirmation is unsafe: persisted Force, no consequences, no preview
Teardown options (including Force) persist across features and sessions. The sheet's three bare toggles have no explanation of what will be deleted (worktree, containers, volumes, branch, uncommitted changes) and no dirty-worktree or unmerged-branch warning. There is no dry-run preview, though `prune --dry-run` exists, and no keyboard default or cancel shortcut. Combined with MAC-03 this is a data-loss path.

**Evidence:** macos/Sources/BranchBoxApp/Views/TeardownSheetView.swift:9-23; FeatureListViewModel.swift:53-57, 390-402.

**Suggested fix:** Do not persist Force. Show what will be removed (git status, branch merged state, containers) before confirming. Use explicit 'Keep branch / Delete branch' radio buttons defaulted from the repo config. Bind Cancel to .cancelAction, and show a 'Teardown in progress' row afterwards.

### MAC-21 [medium/bug] Devcontainer sync UI: per-feature buttons sync all worktrees, strategy pick runs immediately, Dry Run is a no-op
`devcontainer sync` syncs ALL feature worktrees, but it is offered as a per-feature action (Detail, context menu, active card) and passes that feature's syncStrategy, which is always nil from CLI data. In the Agent tab and menu bar, choosing 'Copy strategy' or 'Symlink strategy' both changes the setting and immediately runs a sync. 'Dry Run' discards the CLI output and shows a 'Sync completed' alert, so the user learns nothing.

**Evidence:** `branchbox devcontainer sync --help`: 'Sync devcontainer configuration to all feature worktrees'; macos/Sources/BranchBoxApp/Agent/CLICompat.swift:87-96 (`_ = try run`); Views/AgentStatusView.swift:15-23; Menu/StatusMenuView.swift:165-173; FeatureDetailView.swift:119; FeaturesView.swift:53; HomeView.swift:180; FeatureListViewModel.swift:265-281.

**Suggested fix:** Make the sync action global ('Sync devcontainer config to all worktrees…') with a preview sheet that shows dry-run output, then lets the user apply. Keep strategy as a pure setting.

**Verifier:** confirmed — `devcontainer sync --help` says 'Sync devcontainer configuration to all feature worktrees', yet the action is offered per feature (FeatureDetailView.swift:119, FeaturesView.swift:53, HomeView.swift:180).

The 'Copy strategy' and 'Symlink strategy' menu items call setDevcontainerStrategy and then syncDevcontainer immediately (AgentStatusView.swift:16-17, StatusMenuView.swift:166-167). Dry Run discards output (`_ = try run`, CLICompat.swift:95) and shows 'Sync completed' (VM:272). Under `swift run`, the following notification would also crash (MAC-02).

Small correction: syncStrategy is not always nil from CLI data. It is recorded and emitted after the first successful `devcontainer sync` (feature.rs:5649), so it is nil only for never-synced features.
Corrected: feature.syncStrategy is nil until the feature has been synced once, not permanently nil.

### MAC-22 [medium/ux] Control-plane jargon and permanently orange 'CP pending' / 'Cloud sync • Pending' for the normal unconfigured case
Most users have no control plane configured (Status returns configured=false). The UI still shows an orange 'CP pending' toolbar badge, Home 'Cloud sync • Pending — Awaiting delivery' with a 'View log' button that leads to an Agent tab with no log, and the menu bar 'Fallback' label. The backlog doc itself calls out this jargon.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:149-152, 178-188; HomeView.swift:121-128; MainAppView.swift:91-95; docs/features/backlog/mac-app-polish.md 'Terminology such as “control plane” and “devcontainer strategy” leaks agent internals into the UI'; AUDIT-H1 'cp(configured=false connected=false)'.

**Suggested fix:** Hide control-plane UI unless it is configured. Use plain states: 'Not connected to cloud (optional)', 'Delivering', 'Delivery failing: <error>'. Move diagnostics to an advanced Agent pane.

### MAC-23 [medium/missing_feature] No automatic refresh: no polling, no registry file watch, no refresh on activation or menu open
Features started or torn down from the terminal, which is the CLI's main use, never appear until a manual refresh. The menu bar extra shows whatever was loaded at launch. Status changes (degraded/orphaned) are never noticed.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:98-110 (loadIfNeeded once); MainAppView.swift:36; StatusMenuView.swift:36-37 (onAppear only resets name). No Timer, DispatchSource, FSEvents or NSApplication.didBecomeActive usage anywhere in Sources.

**Suggested fix:** Watch `<repo>/.branchbox/` with DispatchSource/FSEvents, refresh on scenePhase or app activation and when the MenuBarExtra opens, and add a lightweight periodic refresh (e.g. 30–60s) for derived health states.

### MAC-24 [medium/bug] Scene/window plumbing: shared sheets across windows and menu bar, sheet-on-sheet from palette, ⌘N conflict, 'Open BranchBox…' can't reopen window
Several presentation problems follow from all scenes sharing one VM. Only the first one is certain from the code; the others are plausible and could not be verified because the audit could not run the GUI.
- One VM drives sheets and alerts for every WindowGroup window and the MenuBarExtra. pendingTeardown has .sheet(item:) in both MainAppView and StatusMenuView. With several windows open, palette/start/teardown/detect sheets and alerts appear in all of them, and selectedSection is global.
- The palette's 'Start Feature…' and 'Teardown Active Feature…' set another sheet binding while the palette sheet is still presented, then dismiss. SwiftUI cannot present a second sheet from the same view during that transition, so the action is likely dropped.
- Commands live in the App menu (after .appInfo), and 'Start Feature' ⌘N collides with WindowGroup's File ▸ New Window ⌘N.
- If no main window is open, ⌘N, ⌘K and 'Open BranchBox…' only flip flags or call NSApp.activate. No window opens (there is no openWindow(id:)), and a sheet may appear unexpectedly later.
- Sheets presented from MenuBarExtra(.window) content are fragile (the panel resigns key).

**Evidence:** macos/Sources/BranchBoxApp/App/BranchBoxMacApp.swift:22, 33-49; MainAppView.swift:37-59; StatusMenuView.swift:25-35, 160-163; CommandPaletteView.swift:31, 52-54, 86-88.

**Suggested fix:** Use a single `Window("BranchBox", id: "main")` (or WindowGroup with id) plus @Environment(\.openWindow) from the menu bar. Put presentation state in per-scene @State or a per-window coordinator. Have the palette return an intent that the presenter performs after onDismiss. Move commands into File/View or a 'Feature' CommandMenu, using ⇧⌘N for Start Feature.

**Verifier:** confirmed — The certain part holds in code. One @StateObject VM is injected into WindowGroup, Settings and MenuBarExtra (BranchBoxMacApp.swift:22, 29-65). pendingTeardown has `.sheet(item:)` in both MainAppView (43) and StatusMenuView (28), and other sheets and alerts bind to VM-global flags, so every MainAppView window observes them. selectedSection is global.

Also confirmed in code:
- the commands are in CommandGroup(after: .appInfo), i.e. the App menu, with ⌘N (34-43);
- 'Open BranchBox…' only sets selectedSection and calls NSApp.activate (StatusMenuView.swift:160-163), with no openWindow;
- the palette sets commandStartRequested or pendingTeardown and then dismisses (CommandPaletteView.swift:31, 52-54, 86-88).

Whether SwiftUI drops the sheet-after-sheet, which ⌘N binding wins, and how menu-bar sheets behave all depend on runtime. I could not observe them without the GUI; the finding already marks them plausible.


### MAC-25 [high/missing_feature] No agent lifecycle and agent not distributed: gRPC transport is effectively dev-only
Homebrew installs only bb, branchbox and branchbox-local-vm. `branchbox agent` has only `status`, with no start, stop or install. The app cannot start, stop, install (launchd) or diagnose the agent, and the packaging script embeds only the CLI. Yet gRPC is the default transport (MAC-01). The backlog's open question ('embed a lightweight agent launcher (launchd plist)?') is unresolved.

**Evidence:** `ls /opt/homebrew/Cellar/branchbox/0.13.4/bin` → 'bb branchbox branchbox-local-vm'; `branchbox agent --help` → 'Commands: status'; `branchbox agent status --json` → 'Error: failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock ... No such file or directory (os error 2)' exit=1; scripts/package-macos-app.sh:21-31, 71-75 (CLI only); docs/features/backlog/mac-app-polish.md Open Questions.

**Suggested fix:** Decide on a product direction. Either: (a) CLI-first app (recommended near-term), with the agent optional and auto-detected through a fast probe; or (b) ship branchbox-agent with the app as an SMAppService/launchd agent and an Agent pane (Install, Start, Stop, Logs).

**Verifier:** confirmed — `ls /opt/homebrew/Cellar/branchbox/0.13.4/bin` → bb, branchbox, branchbox-local-vm. `branchbox agent --help` → only `status`. `branchbox agent status --json` → 'Error: failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock ... No such file or directory (os error 2)' exit=1.

package-macos-app.sh:21-31 and 71-75 embed only the CLI. The tap formula's bin.install covers only the CLI. mac-app-polish.md:55 still lists the launchd-launcher question as open. Together with MAC-01 (gRPC is the default), the default transport targets a daemon that is not distributed.


### MAC-26 [medium/bug] Feature URLs are scheme-less and adapter service URL is container-internal; host-port mappings unused
feature_url is stored as 'dev-prine.localhost' with no scheme. URL(string:) yields a relative URL with scheme nil, so the Link, NSWorkspace.open and palette 'Open Active Feature' actions are unlikely to open a browser (expected NSWorkspace behaviour; not verified because the audit could not run the GUI). adapter.service_url is 'http://dev:3000', a compose service hostname valid only inside the docker network, yet it is shown as the service URL. The runtime's resolved host-port mappings, the only reliable host-reachable URLs for SBX and similar runtimes, are not decoded.

**Evidence:** Real CLI output: `"feature_url": "dev-prine.localhost"`, `"service_url": "http://dev:3000"`; experiment AUDIT-A2 'url=Optional("dev-prine.localhost") urlScheme=nil'; macos/Sources/BranchBoxApp/Views/HomeView.swift:177-179; FeaturesView.swift:44-46; FeatureDetailView.swift:37-43, 114-116; StatusMenuView.swift:117; CommandPaletteView.swift:80-85; core/src/runtime/mod.rs:83, 130-133.

**Suggested fix:** Normalise URLs: prefix http:// when no scheme is present, and prefer `http://localhost:<published_ports.host>`. Label the adapter URL 'in-container' and do not make it clickable.

**Verifier:** confirmed — Decoding the real CLI output gave url='dev-prine.localhost' with URL(string:).scheme == nil. This is by construction: AppUrl::parse strips http(s):// (core/src/validation.rs:213-218), and the cloudflared path formats '{prefix}-{feature}.{zone}' (feature.rs:1705-1712), so feature_url never has a scheme.

The adapter service_url is 'http://dev:3000', a compose-internal host. published_ports exist in RuntimeMetadata but are not decoded.

Minor correction: the adapter URL is shown as a non-clickable Label (FeatureDetailView.swift:43). The 'do not make it clickable' fix already holds; only the labelling is misleading. Whether Link/NSWorkspace fails on the scheme-less URL was not verified, since I could not open the GUI, which matches the finding's own hedge.
Corrected: The adapter service URL is displayed but already non-clickable. The fix is to label it as in-container. Feature URLs are always scheme-less, because core strips the protocol.

### MAC-27 [low/bug] Date parsing fragility and Swift 6 non-Sendable static formatters
Three related issues.
- FeatureViewData.parse uses an ISO8601DateFormatter with .withFractionalSeconds, which REQUIRES a fraction. chrono's to_rfc3339 omits the fraction when nanos are 0, so such timestamps parse to nil ('—'). AgentStatusView uses the same formatter.
- The CLI decoder uses JSONDecoder.iso8601. This accepted fractional seconds on macOS 26, but on macOS 13/14 (the declared minimum) the legacy strategy is believed to reject fractional seconds, which would fail every CLI list decode. Plausible; untested on old macOS.
- Static formatters are non-Sendable globals, which is an error in Swift 6 mode.

**Evidence:** macos/Sources/BranchBoxApp/Agent/FeatureModels.swift:137-146, 65-76; Views/AgentStatusView.swift:122-142; CLICompat.swift:15. Experiment AUDIT-F: 'with-fraction=Optional(2026-03-17 03:37:57 +0000) no-fraction=nil' and 'JSONDecoder.iso8601 fractional=Optional(...) plain=Optional(...)' (macOS 26). Strict build: 'Agent/FeatureModels.swift:142:24: warning: static property 'iso8601' is not concurrency-safe ...', ':72:24: ... 'relativeFormatter' ...'.

**Suggested fix:** Use Date.ISO8601FormatStyle with a custom decoding strategy that tries with and without fractional seconds, and make the formatters @MainActor or local.

**Verifier:** confirmed — Scratch checks:
- ISO8601DateFormatter with [.withInternetDateTime, .withFractionalSeconds] parsed 6- and 9-digit fractions and '+00:00', but returned nil for '2026-03-17T03:37:57+00:00' and '...57Z'.
- The agent uses chrono `to_rfc3339()` (agent/src/grpc.rs:208-214), which omits the fraction when nanos == 0, so failures are rare but possible. Low severity fits.
- On macOS 26, JSONDecoder.iso8601 accepted both fractional and plain timestamps.
- The strict build reproduced 'FeatureModels.swift:142:24 ... static property 'iso8601' is not concurrency-safe' and ':72:24 ... 'relativeFormatter''.

The macOS 13/14 JSONDecoder claim is untested. If it holds, every CLI list decode would fail on those OS versions, which would make it far more severe than 'low'.


### MAC-28 [medium/ux] HIG deficits: navigation, empty states, restoration, accessibility, menu bar, focus stealing
Compared with a well-made macOS app:
- **Navigation:** a three-column split whose detail column is EmptyView for 3 of 4 sections. Settings is duplicated as a sidebar section and the Settings scene.
- **Window:** no window restoration (@SceneStorage for section or selection), no defaultSize or minimum size.
- **Empty states:** none for 'agent not running', 'CLI not found' or 'workspace not a BranchBox repo'.
- **Toolbar:** a developer-only transport picker sits in the main toolbar.
- **Accessibility:** status is conveyed by colour-only dots (Home statusRow, menu bar header); icon-only copy and refresh buttons lack accessibility labels; the '⋯' button text; the empty systemImage "" in the Skip modules menu.
- **Menu bar:** static icon with no busy/error badge; no option to hide the Dock icon (no LSUIElement toggle).
- **Activation and permissions:** NSApp.activate(ignoringOtherApps:) at every launch (deprecated in macOS 14, steals focus); notification permission requested at launch rather than in context.
- **APIs:** deprecated Alert API.
- **Command palette:** no keyboard navigation and no Return-to-run.
- **Sheets:** most lack .cancelAction/.defaultAction shortcuts.
- **Duplication:** status UI is copy-pasted in about 5 places.
- **Dark mode:** mostly fine (semantic colours plus opacity tints).

**Evidence:** macos/Sources/BranchBoxApp/Views/MainAppView.swift:9-35, 60-105; HomeView.swift:241-244; StatusMenuView.swift:42-50, 151-152; AgentStatusView.swift:115-118; StartFeatureSheet.swift:40; CommandPaletteView.swift:12, 20-48; App/BranchBoxMacApp.swift:8-16, 61-65; DetectOutputView.swift:15; TeardownSheetView.swift:16-18.

**Suggested fix:** Use a two-column NavigationSplitView (sidebar: Features grouped by state, plus Activity and Agent; detail: feature inspector), a Settings scene only, @SceneStorage for selection, .defaultSize, a ContentUnavailableView-style empty state per failure mode, accessibilityLabel on status indicators, a dynamic MenuBarExtra label (spinner/badge), and .keyboardShortcut on sheet buttons.

### MAC-29 [high/test_gap] Tests cover almost nothing and assert a stale schema
There are 4 XCTest cases. One decodes a hand-written CLI payload using the OLD flat schema (tunnel_status at top level, whole-second timestamps), so it passes while real output loses tunnel data. Three exercise devcontainerStatusSummary string helpers. Nothing covers:
- AgentBridge transport selection or fallback (MAC-01 would have been caught);
- CLI argument construction (MAC-03);
- the process runner (MAC-05);
- view-model state transitions or concurrency;
- gRPC mapping, URL normalisation or status mapping;
- views (no snapshot or UI tests).
CI runs only `swift build -v` and `swift test -v`; there is no packaging or app-level E2E.

**Evidence:** macos/Tests/BranchBoxAppTests/BranchBoxAppTests.swift:5-25 (payload with "tunnel_status": "none", "updated_at": "2024-02-01T12:34:56Z"), 27-42; .github/workflows/ci.yml:264-282. Scratch run: 'Executed 4 tests, with 0 failures'.

**Suggested fix:** Add golden decoding tests from captured `feature list --json`, `feature start --json` and `teardown --json` output, plus tests for CLIClient argv. Add a fake-CLI test harness (BRANCHBOX_CLI_PATH) covering large-output, timeout and cancel. Test the bridge against a closed port with a 2s budget, and test the VM with an injectable bridge protocol. A gRPC test against an in-process server is optional.

**Verifier:** confirmed — Tests/BranchBoxAppTests/BranchBoxAppTests.swift has exactly 4 tests:
- testCLIRecordDecodes uses the old flat schema ("tunnel_status": "none", whole-second "updated_at": "2024-02-01T12:34:56Z") and asserts only workFeature;
- the other 3 test devcontainerStatusSummary.
A scratch run gave 'Executed 4 tests, with 0 failures'. ci.yml:264-282 (macos_swift) runs only `swift build -v` and `swift test -v`, with no packaging step. No test covers transport fallback, argv, the process runner or VM state; each would have caught MAC-01, -03 or -05.


### MAC-30 [medium/architecture] God view model and duplicated transport abstractions; dead code
FeatureListViewModel (446 lines) mixes loading, start-form drafts, teardown options, devcontainer settings, workspace pickers, pasteboard, Finder/Terminal shell-outs, detect and UI routing.
- AgentBridge and CLICompat are concrete with no protocol, so they are untestable.
- Transport and FeatureViewData.Source duplicate each other.
- Dead code: FeatureListView.swift (349 lines, never instantiated; it holds the only isLoading UI and the 'CLI' badge that docs reference); FeatureAction enum; syncDevcontainerRequested; the 4-arg statusRow overload; suggestedFeatureName placeholder.
- Status pills and devcontainer badges are copy-pasted across Home, Features, Detail, Menu and the dead view.

**Evidence:** macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift (whole file); Views/FeatureListView.swift:1-349 (grep shows no instantiation); Agent/FeatureModels.swift:4-7, 159-162; FeatureListViewModel.swift:63, 158; HomeView.swift:229-231; docs/docs/getting-started/manual-cli-e2e.md:109 ('Rows tagged “CLI”').

**Suggested fix:** Split into a FeatureStore (data, refresh, watch), an OperationsStore (long-running operations), a WorkspaceStore, and a `BranchBoxClient` protocol with CLI and gRPC implementations. Delete FeatureListView and the unused symbols. Extract StatusPill and DevcontainerBadge components.

### MAC-31 [medium/architecture] Agent gRPC service blocks async workers and offers only unary RPCs
The tonic handlers call synchronous FeatureWorkflow::start and teardown inline. These run for minutes, blocking a tokio worker thread, with no spawn_blocking. Being unary, they cannot stream progress or support cancellation. The gRPC Status also omits last_sent_batch_id, last_sent_event_id and last_sent_at, which IPC provides, so those Agent-tab rows never appear over gRPC.

**Evidence:** agent/src/grpc.rs:77-107, 109-134 (ops::start_feature/teardown_feature called directly in async fn), 136-161; agent/src/ops.rs:23-41; agent/proto/agent.proto:142-149 vs agent/src/ipc.rs:499-516; macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:100-110 (lastSent* = nil).

**Suggested fix:** Wrap the work in tokio::task::spawn_blocking, add server-streaming Start/Teardown progress events with cancellation, and align the Status fields with IPC.

### MAC-32 [low/performance] CLI-mode refresh spawns three processes, including a pointless `--help` probe
fetchFeaturesViaCLI runs `feature list`, then `--help`, then `agent status --json` on every refresh, synchronously on a cooperative thread. The `--help` probe's check (`help.contains("agent")`) is always true for current CLIs. agent status fails whenever no agent is running, which is the normal case.

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:316-326; CLICompat.swift:57-70; experiment AUDIT-E 'configured=false connected=false took 0.13s'; `branchbox agent status --json` exit=1 with no agent.

**Suggested fix:** Probe the agent once per session (or on demand), drop the --help probe, and run list and status concurrently in an async runner.

### MAC-33 [low/bug] CLI child process inherits stdin and is orphaned if the app quits mid-operation
standardInput is not set. Under `swift run` the CLI inherits the terminal and could read interactive prompts (dialoguer) from it. In practice prompts are tty-gated on stdout, so this is low risk. If the app quits during a minutes-long start, the CLI keeps running unobserved, or dies on SIGPIPE when writing to closed pipes. Either way it can leave failed_retained or partial state (plausible; not reproduced).

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:105-113; cli/src/commands/feature.rs:848-852 (prompts gated on Term::stdout().is_term()).

**Suggested fix:** Set standardInput = FileHandle.nullDevice. Track running child processes and, on quit, either warn ('A feature start is in progress') or terminate them cleanly.

**Verifier:** confirmed — CLICompat.swift:105-113 sets stdout and stderr pipes but no standardInput, so stdin is inherited. Interactive prompts are gated on `Term::stdout().is_term()` (cli/src/commands/feature.rs:849-851), and stdout is a pipe in the app, so the stdin risk is low, as the finding says. No child tracking or termination exists on quit. The orphan and SIGPIPE consequences are plausible but not reproduced, which the finding acknowledges. Low severity is appropriate.


### MAC-34 [medium/distribution] Packaging is a prototype: unsigned bundle, hardcoded 0.1.0, no icon, no CI artifact, no notarization
package-macos-app.sh copies the SwiftPM binary into a hand-written Info.plist bundle. CFBundleShortVersionString is hardcoded to 0.1.0 (the CLI is at 0.13.4). There is no codesign or notarize step, no app icon, no LSUIElement or menu-bar-only option, and no Sparkle/Homebrew cask. CI does not run packaging (a milestone3 deliverable). The Homebrew tap formula does not ship the app or the agent. User-specific Xcode state (xcuserdata) is committed.

**Evidence:** scripts/package-macos-app.sh:39-66 ('<string>0.1.0</string>'); .github/workflows/ci.yml:264-282 (no package step); docs/features/backlog/milestone3.md Goals 2; `git ls-files macos` includes 'macos/.swiftpm/xcode/package.xcworkspace/xcuserdata/<owner>.xcuserdatad/UserInterfaceState.xcuserstate'.

**Suggested fix:** Version from the Cargo workspace, add codesign --options runtime plus notarytool in CI, add an asset catalog icon, publish a zipped .app or a cask in the tap, and gitignore xcuserdata.

**Verifier:** confirmed — package-macos-app.sh:41-68 writes an Info.plist by hand with CFBundleShortVersionString '0.1.0' (line 59), while the CLI is at 0.13.4. The script has no codesign or notarytool step, no icon or asset catalog, and no LSUIElement. CI's macos_swift job has no packaging step (ci.yml:264-282), though milestone3.md Goal 2 calls for packaging and publishing artifacts. The tap formula ships the CLI tarball only. `git ls-files macos` includes macos/.swiftpm/xcode/package.xcworkspace/xcuserdata/<owner>.xcuserdatad/UserInterfaceState.xcuserstate and xcschememanagement.plist.


### MAC-35 [medium/doc_gap] README and manual E2E docs are stale or wrong for the app
Several documented steps do not match the code.
- README instructs BRANCHBOX_WORKSPACE=/workspaces/milestone2 with an agent inside the devcontainer, and calls that the right gRPC workspace path.
- manual-cli-e2e.md says `defaults write dev.branchbox.app workspace "$(pwd)"`, but the code reads key `branchbox.workspace`, and swift-run builds don't use the dev.branchbox.app domain.
- The docs say rows tagged 'CLI' indicate fallback, but that tag only exists in dead code.
- The backlog doc marks features ✅ that are broken in practice (transport indicator, CLI fallback).
- Nothing mentions the agent not being distributed, or the Finder PATH requirement.

**Evidence:** macos/README.md:15-19, 26-29, 36-38; docs/docs/getting-started/manual-cli-e2e.md:105-109; macos/Sources/BranchBoxApp/ViewModels/FeatureListViewModel.swift:73 (`branchbox.workspace`); docs/features/backlog/mac-app-polish.md:12-17.

**Suggested fix:** Rewrite the README around the supported loop (Homebrew CLI, then the app, with the agent optional), fix the defaults key and domain, and update the manual E2E 'Mac App ↔ Agent Loop' steps.

### MAC-36 [low/architecture] Swift 6 readiness: Swift 5 language mode with 6 strict-concurrency errors-to-be
Package.swift uses swift-tools-version 5.9, so the Swift 5 language mode. A complete strict-concurrency build produces 6 diagnostics in hand-written code (4 SendingRisksDataRace on self.bridge, 2 MutableGlobalVariable formatters), all errors in Swift 6 mode. The rest of the 86 warnings come from swift-protobuf/grpc-swift plugins (deprecated PackagePlugin Path APIs), matching milestone3's 'tame Swift warnings' goal.

**Evidence:** macos/Package.swift:1; scratch build log $SCRATCH/mac-app-code/build-strict.log ('exit=0', 86 'warning:' lines; hand-written ones listed under MAC-11/MAC-27).

**Suggested fix:** Fix MAC-11 and MAC-27, then move to swift-tools-version 6.0 with swiftLanguageModes [.v6]. Consider grpc-swift 2.x (async-native) or dropping gRPC from the app.

### MAC-37 [low/bug] AgentConfiguration address parsing breaks for IPv6 and host-only values
`address.split(separator: ":")` takes the first and last parts. An IPv6 address like '[::1]:50515' yields host '[', and a host-only value 'localhost' makes the last part 'localhost', so the port silently falls back to 50515. There is no validation or UI, and the address isn't shown anywhere.

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:15-18.

**Suggested fix:** Parse with URLComponents or split on the last ':', strip IPv6 brackets, and surface the address in Settings.

**Verifier:** confirmed — I ran AgentConfiguration.detect with BRANCHBOX_AGENT_GRPC_ADDR set to each value:
- '[::1]:50515' → host='[' port=50515;
- 'localhost' → host='localhost' port=50515 (silent default);
- '::1' → host='1' port=1;
- '127.0.0.1:6000' parsed correctly.
This matches AgentBridge.swift:15-18. The address is not surfaced anywhere in the UI.


## Experiments

- [pass] **Strict-concurrency build of scratch copy** — `cd $SCRATCH/macos && swift build -Xswiftc -strict-concurrency=complete`
  - exit=0; 6 hand-written diagnostics: FeatureModels.swift:142:24 and :72:24 MutableGlobalVariable; FeatureListViewModel.swift:128:43, :238:39, :271:39, :308:39 'sending 'self.bridge' risks causing data races; this is an error in the Swift 6 language mode'; 86 warnings total (rest from swift-protobuf/grpc-swift plugins)

- [pass] **Existing test suite** — `cd $SCRATCH/macos && swift test`
  - BranchBoxAppTests: 'Executed 4 tests, with 0 failures' (decode of stale-schema payload + 3 devcontainer summary helpers)

- [partial] **A: CLICompat.featureList against real CLI (read-only, main repo)** — `swift test (AuditExperiments.testA_CLIFeatureListDecodesRealOutput)`
  - 'AUDIT-A decoded 2 records; first tunnelStatus=nil tunnelProvider=nil' (nested tunnel{} not mapped)

- [fail] **A2: Map real CLI JSON through FeatureViewData** — `swift test (testA2_DecodeWithFractionalSecondsTolerantDecoder) on captured `branchbox feature list --json``
  - 'AUDIT-A2 prine status=Active modules=0 ok devcontainer=Pending tunnelProvider=nil tunnelStatus=nil url=Optional("dev-prine.localhost") urlScheme=nil'

- [fail] **B/B3: Automatic transport with no agent on 127.0.0.1:50515** — `swift test --filter AuditExperiments/testB_… and AuditLongExperiments (AgentBridge.listFeatures(), override nil)`
  - 'AUDIT-B HUNG: listFeatures did not return within 45s (automatic transport, agent not running)'; 'AUDIT-B3 HUNG: automatic listFeatures did not return within 180s'

- [fail] **B2: Forced gRPC with no agent** — `swift test (testB2_GrpcForcedNoAgent)`
  - 'AUDIT-B2 HUNG: forced gRPC listFeatures did not return within 30s'

- [fail] **C: Process pipe buffer deadlock via fake CLI (BRANCHBOX_CLI_PATH)** — `swift test (testC1/C2/C3): fake script printing 6B / 200KB stdout / 200KB stderr, called via CLICompat.detectProject`
  - 'AUDIT-C[small] returned 6 bytes after 0.17s'; 'AUDIT-C[bigstdout] DEADLOCK: CLICompat.run did not return within 10s'; 'AUDIT-C[bigstderr] DEADLOCK: CLICompat.run did not return within 10s' (fake children killed by test)

- [fail] **D: Finder/launchd PATH resolution** — `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env branchbox --version ; swift test (testD_FinderPathResolution)`
  - 'env: branchbox: No such file or directory' exit=127; 'AUDIT-D threw: CLI fallback failed: env: branchbox: No such file or directory'; launchctl getenv PATH → empty; docker=/usr/local/bin/docker, devcontainer=~/.nvm/.../bin/devcontainer, op=/opt/homebrew/bin/op

- [partial] **E: agentStatusOrDefault cost with no agent** — `swift test (testE_AgentStatusDefaultCost); branchbox agent status --json`
  - 'AUDIT-E configured=false connected=false took 0.13s'; CLI: 'Error: failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock ... No such file or directory (os error 2)' exit=1

- [partial] **F: ISO8601 parsing behaviour** — `swift test (testF_DateParsingWithoutFraction)`
  - 'AUDIT-F with-fraction=Optional(2026-03-17 03:37:57 +0000) no-fraction=nil'; 'JSONDecoder.iso8601 fractional=Optional(...) plain=Optional(...)' (macOS 26; macOS 13/14 untested)

- [fail] **G: Status/module label mapping** — `swift test (testG_Labels)`
  - 'status failed_retained -> label 'Failed_Retained''; 'modules [success,success,success,skipped] -> '0 ok''; '[success,failed] -> '0 ok / 1 fail''

- [partial] **H: Real agent (built from repo into scratch CARGO_TARGET_DIR, config in scratch, gRPC 127.0.0.1:50615, relative socket)** — `CARGO_TARGET_DIR=$SCRATCH/cargo-target cargo build --locked -p branchbox-agent; BRANCHBOX_AGENT_CONFIG=$SCRATCH/agent.toml branchbox-agent; swift test --filter AuditAgentExperiments`
  - 'AUDIT-H[grpc-auto] transport=grpc count=2 in 0.04s cp(configured=false connected=false)'; '[grpc-all] count=8' incl. 'cli-e2e-rust-smoke status=removed ... worktree=Optional("/workspaces/cli-e2e-rust-smoke")'; '[grpc-bad-ws-auto] threw: CLI fallback failed: Error: Validation error: Not a git repository: /'; '[grpc-bad-ws-forced] threw: The operation couldn’t be completed. (GRPC.GRPCStatus error 1.)'. (First attempt failed: absolute socket path 'path must be shorter than SUN_LEN'.)

- [fail] **I: Agent dies mid-session** — `swift test --filter AuditAgentDiesExperiment (list OK, SIGTERM own agent pid 10232, list again)`
  - 'AUDIT-I first list transport=Optional(...grpc) count=Optional(2)'; 'AUDIT-I second list HUNG >40s after agent died (no CLI fallback)'; agent log 'BranchBox agent shutting down' (agent stopped; no branchbox-agent process remains)

- [fail] **UNUserNotificationCenter in unbundled executable** — `swiftc main.swift -o notif-test && ./notif-test (calls UNUserNotificationCenter.current().add like LocalNotifier)`
  - "*** Terminating app due to uncaught exception 'NSInternalInconsistencyException', reason: 'bundleProxyForCurrentProcess is nil: mainBundle.bundleURL file:///private/tmp/.../notif/'" exit=134

- [fail] **Process with nonexistent working directory (README /workspaces/milestone2)** — `swiftc cwdtest; Process(currentDirectoryURL: /workspaces/milestone2).run()`
  - 'run threw: The file “milestone2” doesn’t exist.' → app would show 'CLI fallback failed: CLI not runnable: ...'

- [pass] **CLI help / teardown semantics (read-only)** — `branchbox feature teardown --help; branchbox devcontainer sync --help; branchbox feature list --help`
  - '--keep-branch  Keep the git branch after removing the worktree (default is to delete it)'; '--json  Emit deterministic teardown and residue evidence as JSON'; sync: 'Sync devcontainer configuration to all feature worktrees'; list: '--status <STATUS> (active, degraded, failed_retained, orphaned, removed)'

- [pass] **CLI list output size** — `branchbox feature list --json | wc -c ; branchbox feature list --json --all | wc -c`
  - 4096 bytes (2 active); 16514 bytes (8 incl. 6 removed) → ~2KB/feature, >64KB pipe buffer at ~32 features

- [fail] **Workspace '/' (Finder cwd) behaviour** — `cd / && branchbox feature list --json --repo /`
  - 'Error: Validation error: Not a git repository: /' exit=1

- [pass] **Repo untouched check** — `git -C ~/projects/branchbox-suite/branchbox/main status --porcelain | wc -l`
  - 0

## Open questions
- Product direction: should the app be CLI-first (the CLI's --json outputs become the app API), or should branchbox-agent be shipped and managed (SMAppService/launchd) with gRPC or unix-socket IPC as the primary transport? Today the default transport depends on a daemon Homebrew users don't have.
- Should the app follow the repo's .branchbox/config.json teardown policy (delete branch by default) or default to keeping branches? Either way it must pass --keep-branch or --delete-branch explicitly.
- Does JSONDecoder.dateDecodingStrategy = .iso8601 reject fractional seconds on macOS 13/14, the declared minimum? It only accepted them in testing on macOS 26. If it rejects them, CLI-mode listing fails entirely on older macOS.
- Not runtime-verified because the audit was not allowed to launch the GUI: sheet-on-sheet drops from the command palette, duplicate teardown sheets in MenuBarExtra, 'Open BranchBox…' failing to reopen a closed window, the ⌘N conflict with File ▸ New Window, and NSWorkspace failing on scheme-less URLs.
- Does gRPC Start/Teardown with an empty branch_prefix ignore a repo-configured feature.branch_prefix? The CLI fills it from config at cli/src/commands/feature.rs:839; the agent passes None. Not traced through core.
- With UserDefaults in unbundled swift-run builds, which defaults domain is actually used? Presumed to be the executable name. It was not verified, to avoid writing preferences outside scratch.