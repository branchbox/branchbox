# BranchBox for Mac: revamp design (final)

**Implementation status, 2026-10-05:** [PR #105](https://github.com/branchbox/branchbox/pull/105) merged as `872e1bd`. This document and its work packages retain the original design and audit history. Current behavior and requirements are in the [Mac guide](../../../docs/guides/mac-app.md), [testing record](../../../../macos/TESTING.md) and [outcome/E2E readiness matrix](../../../docs/getting-started/release-readiness.md). The preview requires macOS 26/Xcode 26 and remains ad hoc signed and unnotarized. Earlier macOS 14/Xcode 16 and 90% line-coverage requirements below are superseded; they are not current release gates.

Branch: `feature/mac-app-revamp` (from `main` @ v0.13.4). Worktree: `~/projects/branchbox-suite/branchbox/mac-app-revamp`.
Status: the final design for implementation. It merges the "Incremental Salvage" winner, every compatible must-graft from the two judges, all of their critical corrections, the UX/IA spec, and the core/CLI change spec.

Scratch evidence root, written `$SP` below: `$SCRATCH`, the design session's scratch directory. It is not part of the repository; the audits that matter are copied into `audit/`.

| Kind | Location under `$SP` |
|---|---|
| Audits | `audit/*.md` |
| Real CLI 0.13.4 captures | `cli-contract/captures/` |
| Spikes you may port from | `design/arch-incremental/spike/Sources/BranchBoxCore/{ProcessRunner,Models,ChildEnvironment,Contract,StoreSketch}.swift`, `design/arch-clean-slate/spike/Sources/SpikeKit/{ProcessRunner,Models,OperationCenter,Watcher}.swift`, `design/arch-contract-first/spike/Sources/BranchBoxCLI/{ProcessRunner,Environment,RegistryWatcher}.swift`, `design/cli/proto/classify.py` (S4 classifier prototype), `design/cli/locktest/*.rs` (flock), `design/cli/cstprobe` (jsonc CST) |

Build scratch convention for every verification command: `export BB_BUILD=$SP/build/<package-id>`. The Swift scratch path is `$BB_BUILD/swift`. Cargo uses `CARGO_TARGET_DIR=$BB_BUILD/cargo`.

---

## 1. Goals and non-goals

### Goals (this iteration; all four owner scopes)
1. **Stabilize on a new foundation.** Fix every confirmed app breakage:
   - gRPC hang
   - Finder PATH
   - `swift run` notification crash
   - 64 KiB pipe deadlock
   - JSON drift
   - god view model
   - stale selection
   - sheet-on-sheet
   - persisted Force

   Replace them with a CLI process layer that is tested, models that match the current JSON, safe teardown, a multi-feature and multi-project UI, operation progress and logs, and tests against real CLI output.
2. **Feature parity wave.** Runtime picker and ports; open in editor or terminal; launch a coding agent; run a command via `feature exec`; health remediation for degraded, failed_retained, orphaned and interrupted features; prune with preview; base-branch picker; devcontainer up, down and rebuild; doctor and prerequisite check.
3. **Core safety fixes** in Rust:
   - Teardown never deletes user work without `--force` or `--discard-changes`.
   - BranchBox's own files are not counted as user changes.
   - Registry lock and atomic write.
   - Unmerged-branch refusal happens before anything is removed.
   - Clean `--json` stdout plus an error envelope.
   - Teardown uses the recorded branch name.
   - A write-ahead start record, so interrupted starts are visible.
4. **Onboarding and settings.** Add project and init; a `.branchbox/config.json` editor through `config get/apply`; tunnel credentials; tunnel open and remove; the Doctor/Diagnostics window; app Settings.
5. Ship a **sealed, ad-hoc-signed, universal `BranchBox.app`** from one script, with a CI job that builds, tests and uploads the artifact.

### Non-goals (this iteration)
- Agent-daemon backend (the protocol is ready for it; the `agent/` crate is untouched).
- Developer ID signing, notarization and the Homebrew cask (the script has flags reserved).
- Sandboxing and the Mac App Store.
- Clone-from-URL onboarding.
- PR/GitHub integration.
- Interactive sbx shells.
- JSONL progress (C8).
- Decoupling `--force` from `git branch -D` in the CLI (deferred to a deprecation cycle; see D-27).

---

## 2. Decisions

| # | Topic | Decision | Why |
|---|---|---|---|
| D-1 | Transport | The `BranchBoxBackend` protocol. `CLIBackend` is the only conformer and spawns the user's installed `branchbox`. grpc-swift, swift-nio, swift-protobuf, `Generated/` and `Package.resolved` are removed. | Owner decision. The gRPC path hangs forever (MAC-01). |
| D-2 | Minimum macOS | **14.0** (`LSMinimumSystemVersion 14.0`). | `@Observable`, `ContentUnavailableView`, `.inspector`, `NSApp.activate()`, `SettingsLink`/`openSettings`. Internal audience. Dates use our own RFC 3339 parser, so Foundation formatter differences don't matter. |
| D-3 | Swift | `swift-tools-version: 6.0`, `swiftLanguageModes: [.v6]` (complete strict concurrency). No Swift 6.1+/6.2 features: no `.defaultIsolation`, explicit `@MainActor`. **Public APIs use untyped `throws`**; every thrown error is a `BackendError`, normalized by `BackendError.normalize(_:)`. | Builds on Xcode 16 (Swift 6.0.x) on the macos-14 runner. The judges flagged typed-throws inference differences between Swift 6.0 and 6.2 as a CI-only risk. Untyped throws plus a documented contract removes it. The Swift 6.0 compile is a CI gate from the first PR (SW-0). |
| D-4 | Dependencies | **Zero** SwiftPM dependencies. | Removes all 51+ plugin warnings and makes `-warnings-as-errors` feasible. |
| D-5 | Targets | `BranchBoxKit` (Foundation only: contracts, models, pure planning), `BranchBoxCLI` (process runner, environment, `CLIBackend`), `BranchBoxStores` (`@MainActor @Observable`; depends on Kit **only**), `BranchBoxPreview` (`PreviewBackend`; Kit only), `BranchBoxApp` (executable product `BranchBox`; SwiftUI/AppKit; the composition root is the only place that constructs `CLIBackend`). Test support target plus five test targets. | Compile-time layering (graft): stores cannot import the CLI implementation. The UI-free planning layer is unit-testable (graft). |
| D-6 | Sandbox and signing | Not sandboxed. Hardened runtime on. Ad-hoc signed. No entitlements; the file is an empty placeholder. Plain absolute paths, with no security-scoped bookmarks. | The app must exec branchbox, git and docker and read arbitrary repos. |
| D-7 | Scenes | `Window("BranchBox", id: "main")` (single instance), `MenuBarExtra` in **`.menu` style**, `Settings`, `WindowGroup("Run Command", id: "run", for: FeatureRef.self)`, `Window("Activity", id: "activity")`, `Window("Diagnostics", id: "diagnostics")`. | From the UX spec. A `.menu` extra is native, keyboard- and VoiceOver-friendly, and structurally cannot present sheets. |
| D-8 | Locating the CLI | `BRANCHBOX_CLI_PATH`, then the Settings override, then the login-shell PATH (searched in Swift), then well-known dirs, then an embedded `Contents/Helpers/branchbox` (only if present). Paths are kept **unresolved** (no Cellar realpath). No embedding by default. | Avoids version skew on the shared registry. Survives `brew upgrade` (graft). |
| D-9 | CLI floor and capabilities | Hard minimum is **0.13.4**. Capabilities come from **`branchbox version --json`** → `{version, contract_version, capabilities[]}` (core spec C7). On clap exit 2 the app falls back to `--version`, and 0.13.x gets the empty capability set ("legacy mode"). Cached by (unresolved path, inode, mtime, size). | Satisfies the "probe with version-table fallback" graft. Gates by feature, not by version number (judge correction against version-only thresholds). |
| D-10 | Error contract | In `--json` mode every failure prints exactly **one error envelope on stdout** (`{"schema_version":1,"error":{code,message,causes,details}}`). Exit codes are unchanged. Commands that report failure in-band (`feature exec`, `devcontainer up/down/build/exec`, `dispatch-tool`) keep their payload and exit code. The old `BRANCHBOX_ERROR_FORMAT` env-var idea is dropped. | Core spec S3. Older CLIs simply don't emit it; the app falls back to the stderr classifier. |
| D-11 | Teardown | Every teardown passes exactly one branch flag and the `--branch-prefix` derived from the recorded `branch_name`. **The first attempt never carries discard flags.** Discard happens only through a recovery after a refusal: an in-app refusal on legacy CLIs, or a CLI refusal on 0.14+. On **legacy CLIs the CLI is always called with `--keep-branch`**, and branch deletion is an app-side `git branch -d` (or `-D` only on explicit Force-delete) after the teardown succeeds. On 0.14+ the app sends explicit `--keep-branch` / `--delete-branch [--force-delete-branch]`, plus `--discard-changes` from recovery, and never sends `--force` except for locked or unreadable-worktree recoveries, which are always paired with `--keep-branch`. | Closes BUG-04, MAC-03, DRIFT-01, DRIFT-07 and the `--force`→`-D` coupling on every CLI version without relying on the fixes landing first. |
| D-12 | Prune | The app **never** invokes `branchbox prune` / `feature prune` (it hard-codes `force_remove: true` and `force_delete_branch`, cli/src/commands/feature.rs:955-960). Prune is an app-orchestrated, sequential loop of safe teardowns: per-row preflight, Keep as the default policy, skip-and-continue on refusal. | Judge correction; both judges graft this. |
| D-13 | Generated-file classification | A deterministic **content-based classifier** (core spec S4 rules R1–R7) instead of a sha256 manifest. On 0.14+ the CLI's `teardown --dry-run --json` is authoritative. On legacy CLIs a Swift port of the same rules runs in `CLIBackend`. `.env` is never "managed" by path alone. | Meets the manifest graft's goal (user edits to a tracked `.vscode/settings.json` count as user changes; no path allow-list). It avoids storing digests of secret-bearing files and needs no registry schema change (core spec rationale). |
| D-14 | Interrupted starts | Rust gets a **write-ahead registry record** (`setup: {state:"in_progress", pid, started_at}`), reported as `interrupted` when the pid is dead (capability `write-ahead-start`). The app also keeps **stray reconciliation** from `git worktree list --porcelain`, restricted to the BranchBox layout and prefix (judge correction), for legacy CLIs and pre-record crashes. | Both judges graft the write-ahead record. The stray restriction avoids flagging the user's own worktrees. |
| D-15 | No new `FeatureStatus` values | The serde enum is closed (core feature.rs:5268), so new state goes only in optional `#[serde(default)]` fields. | Graft (Contract-first). Keeps 0.13.x CLIs reading the shared registry. |
| D-16 | Concurrency | One mutating operation per feature. Project-wide operations (prune, sync, init, config) are exclusive within a project. **Without the `registry-lock` capability, every registry writer in a project is serialized FIFO**: start, teardown, prune rows, tunnel open/remove, devcontainer sync, init, config apply, credentials. | Judge correction: sync was missing from the winner's queue. |
| D-17 | Refresh | Single-flight with **one coalesced re-run** (never cancel-and-restart) and a generation guard. Triggers: FSEvents on `<root>/.branchbox`, app activation (>5 s stale), after every operation (success, failure or cancel), timers, ⌘R. At most 2 concurrent `feature list` processes across projects. | Judge correction: cancel-and-restart starves under FSEvents bursts. |
| D-18 | Cancel and quit | Cancel is always offered for running operations, behind a confirmation. On CLIs without `registry-lock` **and** `write-ahead-start`, the confirmation warns explicitly about a partial worktree and possible registry corruption. Cancel sends SIGINT, then SIGTERM, then SIGKILL to the process group, waits for the group to exit, then refreshes. Quitting with running operations asks "Cancel and Quit / Keep Running" and terminates children. | Judges: Contract-first disabling cancel wedges projects; Clean-slate's lifecycle handling is grafted. |
| D-19 | Repo file writes | The app never writes repo files directly. Config goes through `config get/apply --json`; tunnel token through `tunnel credentials set --api-token-stdin`; 1Password refs through `init --op-*` flags. On legacy CLIs those editors are read-only and explain why. | One owner per file format: the CLI. |
| D-20 | Secrets | The Cloudflare token goes from a SecureField to CLI stdin and into `.branchbox/secure/cloudflared.env` at mode 0600. The app never stores it. Keychain storage is deferred. Settings "extra env" values are never logged or shown in "Copy as Command". | Matches the existing core design. |
| D-21 | Init | The GUI always passes `-y`. The repo moves only when the user opts in to `--reorganize` after a `--dry-run` preview. Note: `-y` alone never moves the repo (init.rs:305-319; judge correction of the Incremental and core-spec claims). | Without `-y`, an empty stdin answers "Y" to the move prompt (init.rs:2112-2127). |
| D-22 | Persistence | Projects are stored in `~/Library/Application Support/BranchBox/projects.json` (atomic write). Preferences go to `UserDefaults.standard` when running as a `.app` bundle, otherwise to the suite `dev.branchbox.app.dev`. A one-time legacy-key migration deletes the persisted Force keys. | — |
| D-23 | Salvage mechanic | **Port, don't move.** Old views are reference material, read with `git show v0.13.4:macos/Sources/BranchBoxApp/Views/<File>.swift`. There is no excluded `Legacy/` folder, no stub→`git mv` choreography, and no duplicate-symbol hazard. The salvage ledger (§3.4) lists what to port. | Simplifies the winner's mechanic; addresses the judges' duplicate-symbol and coordination concerns. |
| D-24 | Fixtures | Real 0.13.4 captures (path-scrubbed) live in `macos/Tests/BranchBoxTestSupport/Fixtures/cli-0.13.4/`. **Golden contract fixtures** are generated by the Rust tests into `cli/tests/fixtures/contract/<area>/` (normalized), compared in nextest (mismatch fails unless `UPDATE_CONTRACT_FIXTURES=1`), and decoded by the Swift tests through `#filePath`. | Graft: one source of truth, with drift caught in both languages. |
| D-25 | Changelog | Each package writes `changelog.d/<package-id>.md`. DOC-1 folds them into `CHANGELOG.md` `[Unreleased]` and deletes `changelog.d/`. | No shared-file conflicts. |
| D-26 | Versioning | No workspace version bump in this iteration; release prep is the release process. The app gates on capabilities, not on 0.14.0. | — |
| D-27 | `--force` semantics | **Not decoupled** from `-D` in this iteration. The judge-1 graft (WP-4(b)) is declined: dozens of tests, the local-vm harness and in-guest orchestrators run `teardown --force` and expect full cleanup. Instead: `--discard-changes` (new, safe) is added; `--force` help text states that it force-deletes when deleting; refusal messages suggest `--discard-changes` first; and when `--force` causes `-D` of an unmerged branch, the summary gets a warning naming the commit count. The app never relies on the coupling (D-11). | Compatibility. Recorded as deferred. |
| D-28 | Menu bar freshness | `.menu` style has no open hook. Data stays fresh through FSEvents (works while inactive), a 300 s background timer and the "Updated n s ago" line. | UX spec. |
| D-29 | Global Overview | Dropped. The detail column shows Welcome, Project or Feature. Cross-project status lives in the menu bar. | UX spec IA. |

### 2.1 Graft ledger

| Source | Graft | Status |
|---|---|---|
| J1 | Never invoke prune; loop of non-force teardowns | Adopted (D-12) |
| J1 | Teardown invariants (one branch flag, derived prefix, first attempt without force, Force only as recovery and never persisted, forced retry `--keep-branch` + `git branch -d`, `.partial` mapping) | Adopted (D-11). Legacy mode generalizes the keep-branch-then-app-delete rule to every legacy teardown. `.partial` is kept as defensive classification. |
| J1 | Core WP-4(b) decouple `-D`, WP-4(c) recorded branch | (c) adopted (C10c). (b) **declined** (D-27). |
| J1/J2 | Stray reconciliation restricted to the BranchBox layout | Adopted (D-14, §6.6) |
| J1/J2 | Capability probe with version-table fallback, cached by inode/mtime/size | Adopted (D-9) |
| J1/J2 | CI: minimum-OS leg, source-built CLI leg, v0.13.4 tarball leg, live fixture capture and decode | Adopted (§12.2, §13.2) |
| J1 | Error envelope enabled by capability | Adopted as the stdout envelope under `--json` (D-10) |
| J1/J2 | Normalize to the main worktree via `--git-common-dir`; parent-container check | Adopted (§6.4) |
| J1 | Porcelain v1 `-z` classification; unmerged count via `rev-list`, never `git branch --merged` | Adopted. "Merged" uses `git branch -d` semantics: `merge-base --is-ancestor <branch> <upstream or HEAD>`. |
| J1/J2 | Dev defaults suite and legacy-key migration | Adopted (D-22) |
| J1/J2 | Runner semantics (pre-spawn cancellation check, detached drain collector, group-exit wait, child registry plus `terminateAll`, typed stdoutTooLarge, `\r` splitting, EOF/drain grace, group reaping) | Adopted (§7) |
| J1 | Child env additions (`GIT_TERMINAL_PROMPT=0`, `RUST_BACKTRACE=0`, Docker paths, preserved HOME/USER/TMPDIR/SSH_AUTH_SOCK, overrides last and never logged) | Adopted |
| J1/J2 | Pure planning layer; typed `RecoveryAction` carrying the concrete request | Adopted (Kit/Planning, `RecoveryAction.retry(OperationRequestContext)`) |
| J1 | Lifecycle (no terminate on last window close, reopen via captured `openWindow` with an `NSApp.windows` fallback, terminate-later flow) | Adopted (§8.7) |
| J1/J2 | LogArchive with redaction; Copy diagnostic report | Adopted |
| J1/J2 | PreviewBackend plus `BRANCHBOX_BACKEND=preview` | Adopted (DEBUG builds only) |
| J1 | CLIJSON strict-then-preamble decode, preamble surfaced as a warning | Adopted |
| J1/J2 | Dev bundle launched via `open` to reproduce the launchd environment | Adopted (`scripts/macos-dev.sh --open`) |
| J1/J2 | Path-scrubbed fixtures; unknown-JSON-key annotation | Adopted |
| J1/J2 | Golden contract fixtures from Rust, freshness enforced; stable error codes | Adopted (D-24) |
| J1/J2 | No new `FeatureStatus` values | Adopted (D-15; recorded in AGENTS.md) |
| J1/J2 | Write-ahead registry record plus "Resume setup" | Adopted (D-14) |
| J1/J2 | Generated-files manifest (sha256) | Goal adopted via the content classifier; mechanism declined (D-13) |
| J1/J2 | EnvironmentProvider (provisional env for reads; mutations await capture) | Adopted |
| J1 | Compile-time layering | Adopted (D-5) |
| J1 | Ordered event delivery through one AsyncStream consumer | Adopted (§8.3) |
| J1 | Explicit `--repo/--path`; flags before `--` | Adopted |
| J1 | Non-TTY unmerged check before mutation, after fixing `local_branch_is_merged` | Adopted (S5 replaces the helper with `branch_merge_state`) |
| J1 | Unresolved CLI path; embedded CLI in `Contents/Helpers`, signed inside-out | Adopted |
| J1 | Exec inner exit code is data | Adopted app-side: payload accepted on exit 1. The CLI exit code is unchanged for compatibility. |
| J2 | WP-0 split or two-engineer staffing; end-of-wave vertical slice; store skeleton in wave 1 | Adopted: SW-0 is staffed by two people in two ordered PRs; the store API skeleton is in SW-0; milestone M1 is SW-4's first PR. |

---

## 3. Architecture

### 3.1 Layers

```
BranchBoxApp (SwiftUI/AppKit, @MainActor views)   ── composition root constructs CLIBackendBootstrapper
   │ uses
BranchBoxStores (@MainActor @Observable)          ── AppModel, ProjectsStore, ProjectStore, OperationStore, ActionDispatcher
   │ depends only on
BranchBoxKit (Foundation only, Sendable)          ── Backend contract, models, process contract, Planning (pure)
   ▲ implemented by                     ▲ implemented by
BranchBoxCLI (runner, env, CLIBackend)   BranchBoxPreview (PreviewBackend, scriptable)
   │ spawns
user's `branchbox` CLI (+ git, docker)  ── JSON contract (§5)
```

### 3.2 Package.swift (owned by SW-0; frozen after wave 1)

```swift
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "BranchBox",
    defaultLocalization: "en",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "BranchBox", targets: ["BranchBoxApp"])],
    targets: [
        .target(name: "BranchBoxKit"),
        .target(name: "BranchBoxCLI", dependencies: ["BranchBoxKit"]),
        .target(name: "BranchBoxStores", dependencies: ["BranchBoxKit"]),
        .target(name: "BranchBoxPreview", dependencies: ["BranchBoxKit"]),
        .executableTarget(name: "BranchBoxApp",
                          dependencies: ["BranchBoxKit", "BranchBoxCLI", "BranchBoxStores", "BranchBoxPreview"]),
        .target(name: "BranchBoxTestSupport",
                dependencies: ["BranchBoxKit", "BranchBoxCLI", "BranchBoxPreview"],
                path: "Tests/BranchBoxTestSupport",
                resources: [.copy("Fixtures")]),
        .testTarget(name: "BranchBoxKitTests", dependencies: ["BranchBoxKit", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxCLITests", dependencies: ["BranchBoxCLI", "BranchBoxKit", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxStoresTests", dependencies: ["BranchBoxStores", "BranchBoxPreview", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxAppTests", dependencies: ["BranchBoxApp", "BranchBoxStores", "BranchBoxPreview", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxIntegrationTests",
                    dependencies: ["BranchBoxCLI", "BranchBoxKit", "BranchBoxStores", "BranchBoxTestSupport"]),
    ],
    swiftLanguageModes: [.v6]
)
```

`swift run BranchBox` replaces `swift run BranchBoxApp`. `Package.resolved` is deleted. `macos/.swiftpm/` is untracked and gitignored.

### 3.3 File tree and owners (wave-1 owner → later owner)

```
macos/
├── Package.swift                                        SW-0
├── README.md                                            DOC-1
├── TESTING.md                                           VER-1 (manual checklist + results)
├── Packaging/Info.plist.template, BranchBox.entitlements, AppIcon.iconset/**, make-iconset.sh   PK-1
├── Sources/
│   ├── BranchBoxKit/
│   │   ├── Backend/ Identity.swift Progress.swift Requests.swift BackendError.swift Recovery.swift
│   │   │            Preferences.swift BranchBoxBackend.swift Bootstrap.swift          SW-0 → SW-1 (additive) → VER-1 (fixes)
│   │   ├── Process/ ProcessTypes.swift                                                 SW-0 → SW-1
│   │   ├── Models/  (§4.7 list)                                                        SW-0 → SW-1 → VER-1
│   │   └── Planning/ RecoveryPlanner.swift TeardownDraft.swift PrunePlanner.swift StartDraft.swift
│   │                 Remediation.swift HostLaunchPlan.swift NameRules.swift            SW-3 → SW-4 (additive)
│   ├── BranchBoxCLI/                                                                  SW-0 placeholder → SW-1
│   │   ├── Process/ ProcessRunner.swift Escalator.swift PipeReader.swift LineSplitter.swift ANSI.swift ChildRegistry.swift
│   │   ├── Environment/ LoginShellEnvironment.swift ChildEnvironment.swift EnvironmentProvider.swift CLILocator.swift CLIProbe.swift FileSystemProbing.swift
│   │   ├── Backend/ CLIBackend.swift CLIBackendBootstrapper.swift CLICommand.swift CLIOutput.swift CLIErrorClassifier.swift
│   │   │            TracingLineParser.swift PhaseMapper.swift LegacyTeardown.swift TextParsers.swift HostToolProbe.swift
│   │   └── Git/ GitInspector.swift WorktreeChangeClassifier.swift StrayDetector.swift BranchMergeState.swift
│   ├── BranchBoxStores/                                                               SW-0 skeleton → SW-2 → SW-4 (additive)
│   │   AppModel.swift EnvironmentStore.swift ProjectsStore.swift ProjectStore.swift OperationStore.swift
│   │   OperationRecord.swift ActionDispatcher.swift AppSettings.swift Navigation.swift Notifier.swift
│   │   (+ SW-2: ProjectsRepository.swift LegacyDefaultsMigration.swift RegistryWatcher.swift RefreshCoordinator.swift
│   │    ListLimiter.swift MutationQueue.swift EventBatcher.swift LogBuffer.swift LogArchive.swift)
│   ├── BranchBoxPreview/ PreviewBackend.swift PreviewScenarios.swift PreviewBootstrapper.swift   SW-0 → SW-2
│   └── BranchBoxApp/
│       ├── App/ BranchBoxApp.swift (SW-0 skeleton → SW-4) AppDelegate.swift AppCommands.swift CompositionRoot.swift
│       │        FocusedValues.swift WindowOpener.swift                                 SW-4
│       ├── Navigation/ SheetRoute.swift SceneID.swift (SW-0 → SW-4) PresentationRouter.swift SheetHost.swift   SW-4
│       ├── MainWindow/ MainWindow.swift Sidebar.swift SidebarRows.swift EnvironmentGate.swift QuickOpenPalette.swift  SW-4
│       ├── MenuBar/ MenuBarContent.swift MenuBarLabel.swift                            SW-4
│       ├── Notifications/ UserNotificationNotifier.swift                               SW-4
│       ├── Components/ (UI kit)                                                        SW-3 → SW-4 (additive)
│       ├── Presentation/ FeatureRecord+Presentation.swift BackendError+Presentation.swift
│       │                 OperationRecord+Presentation.swift DiagnosticReport.swift      SW-3 → SW-4
│       ├── Services/ HostLauncher.swift Pasteboard.swift                               SW-3 → SW-4
│       ├── Feature/ FeatureDetailView.swift FeatureActionsMenu.swift (SW-0 stubs) + cards   SW-5
│       ├── RunCommand/ RunCommandWindow.swift (SW-0 stub)                              SW-5
│       ├── Flows/ StartFeatureSheet.swift TeardownSheet.swift PruneSheet.swift StrayWorktreeSheet.swift (stubs) + results  SW-6
│       ├── Activity/ ActivityInspector.swift ActivityWindow.swift (stubs) ActivityPopover.swift   SW-6
│       ├── Projects/ WelcomeView.swift ProjectDetailView.swift AddProjectSheet.swift InitProjectSheet.swift
│       │             SyncDevcontainersSheet.swift ProjectSettingsSheet.swift (stubs)    SW-7
│       ├── Settings/ AppSettingsView.swift (stub) + tabs                               SW-7
│       └── Diagnostics/ DiagnosticsWindow.swift (stub) DoctorChecklist.swift           SW-7
└── Tests/
    ├── BranchBoxTestSupport/ ScriptedProcessRunner.swift Fixtures.swift Fixtures/cli-0.13.4/**   SW-0 → SW-1 → VER-1
    ├── BranchBoxKitTests/   SW-0 (models), SW-3 (Planning/**), VER-1 (ContractFixtureDecodeTests.swift)
    ├── BranchBoxCLITests/   SW-1 (incl. FakeCLI/**)
    ├── BranchBoxStoresTests/ SW-2
    ├── BranchBoxAppTests/   SW-0 SmokeTests; SW-3 PresentationTests; SW-4 RouterTests; SW-5 FeatureSurfaceTests; SW-6 FlowTests; SW-7 ProjectsSettingsTests
    └── BranchBoxIntegrationTests/ SW-0 GatingTests → SW-1 (CLISmokeTests, Support/TempRepo) → VER-1 (all)
```

### 3.4 Salvage ledger (port by reading `git show v0.13.4:<path>`)

| Old file | Port into | What to keep |
|---|---|---|
| `Views/MainAppView.swift` (`StatusBadge` 124-137, split skeleton) | `Components/StatusBadge.swift`, `MainWindow/MainWindow.swift` | Badge layout, split-view shape |
| `Views/FeatureDetailView.swift` (GroupBox sections, `FlowLayout` 156-218) | `Components/FlowLayout.swift`, `Feature/*Card.swift` | Section layout, FlowLayout |
| `Views/FeaturesView.swift` | `MainWindow/Sidebar.swift` | Search, context menu |
| `Views/HomeView.swift` | `Projects/ProjectDetailView.swift` | Card grid, status rows (not the "active feature" idea) |
| `Views/StartFeatureSheet.swift` | `Flows/StartFeatureSheet.swift` | Form layout only |
| `Views/SettingsView.swift`, `CommandPaletteView.swift`, `DetectOutputView.swift` | `Settings/*`, `MainWindow/QuickOpenPalette.swift`, `Projects/ProjectDetailView.swift` | Form, palette list UI, detect chips |
| `Menu/StatusMenuView.swift` | `MenuBar/MenuBarContent.swift` | Grouping ideas only (`.menu` style differs) |
| Everything else (`Agent/*`, `Generated/*`, `ViewModels/*`, `FeatureListView`, `AgentStatusView`, `TeardownSheetView`, `LocalNotifier`, `Shared/AppSection`, old tests) | — | Deleted. Never ported. |

---

## 4. Shared Swift contracts (verbatim)

Rules for every contract type:
- (a) public value types are `Sendable` (plus `Hashable` where listed);
- (b) public async APIs use untyped `throws`, and only ever throw `BackendError`;
- (c) cancellation is Swift `Task` cancellation, and a method that is cancelled throws `.cancelled` only **after** the underlying process group has exited;
- (d) bodies shown as `{ … }` are implementation, not contract;
- (e) wave-2 owners may **add** members but must not change or remove anything listed here.

### 4.1 `BranchBoxKit/Backend/Identity.swift`

```swift
import Foundation

/// A project is identified by its MAIN worktree root. Paths are standardized; symlinks are NOT resolved.
public struct ProjectRef: Hashable, Sendable, Codable {
    public let root: URL
    public init(root: URL) { self.root = root.standardizedFileURL }
    public var path: String { root.path }
    public var displayName: String { root.lastPathComponent }
}

public struct FeatureRef: Hashable, Sendable, Codable {
    public let project: ProjectRef
    public let name: String                          // work_feature slug
    public init(project: ProjectRef, name: String) { self.project = project; self.name = name }
}

public struct SemVer: Hashable, Sendable, Comparable, Codable, CustomStringConvertible {
    public let major: Int, minor: Int, patch: Int
    public let prerelease: String?                   // "dev" for 0.14.0-dev+abc (build metadata dropped)
    public init(_ major: Int, _ minor: Int, _ patch: Int, prerelease: String? = nil) { … }
    /// Accepts "0.13.4", "branchbox 0.13.4", "0.14.0-dev+abc123"; nil otherwise.
    public init?(parsing text: String) { … }
    /// A prerelease sorts before the release of the same triple.
    public static func < (lhs: SemVer, rhs: SemVer) -> Bool { … }
    public var description: String { … }
}

/// Capability strings exactly as printed by `branchbox version --json` (§5.3).
public struct Capability: RawRepresentable, Hashable, Sendable, Codable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public static let jsonErrorEnvelope        = Capability(rawValue: "json-error-envelope")
    public static let registryLock             = Capability(rawValue: "registry-lock")
    public static let writeAheadStart          = Capability(rawValue: "write-ahead-start")
    public static let teardownPlan             = Capability(rawValue: "teardown-plan")
    public static let teardownDiscardChanges   = Capability(rawValue: "teardown-discard-changes")
    public static let teardownUnmergedPreflight = Capability(rawValue: "teardown-unmerged-preflight")
    public static let pruneJSON                = Capability(rawValue: "prune-json")
    public static let detectJSON               = Capability(rawValue: "detect-json")
    public static let devcontainerSyncJSON     = Capability(rawValue: "devcontainer-sync-json")
    public static let config                   = Capability(rawValue: "config")
    public static let tunnelCredentials        = Capability(rawValue: "tunnel-credentials")
    public static let doctor                   = Capability(rawValue: "doctor")
    public static let initJSON                 = Capability(rawValue: "init-json")
}

public enum CLISource: String, Sendable, Hashable, Codable {
    case environmentOverride      // BRANCHBOX_CLI_PATH
    case settingsOverride         // Settings › Tools › Locate…
    case loginShellPath           // found on the captured login-shell PATH
    case wellKnownPath            // /opt/homebrew/bin, /usr/local/bin, ~/.cargo/bin, ~/.local/bin
    case embedded                 // Contents/Helpers/branchbox (only if packaged with --embed-cli)
}

public struct RejectedCandidate: Sendable, Hashable { public let path: String; public let reason: String }

public struct CLIResolution: Sendable, Hashable {
    public let path: String                          // unresolved, e.g. /opt/homebrew/bin/branchbox
    public let source: CLISource
    public let rejected: [RejectedCandidate]         // shown in Diagnostics
}

public struct BackendIdentity: Sendable, Hashable {
    public enum Kind: Sendable, Hashable { case cli(CLIResolution), agent(endpoint: String), preview }
    public static let minimumCLI = SemVer(0, 13, 4)
    public let kind: Kind
    public let version: SemVer
    public let contractVersion: Int?                 // nil = legacy CLI without `version --json`
    public let capabilities: Set<Capability>         // empty for legacy 0.13.x
    public init(kind: Kind, version: SemVer, contractVersion: Int?, capabilities: Set<Capability>) { … }
    public func supports(_ capability: Capability) -> Bool { capabilities.contains(capability) }
    public var isLegacy: Bool { contractVersion == nil }
}

public struct EnvironmentSummary: Sendable, Hashable {
    public enum Source: String, Sendable, Hashable { case interactiveLogin, login, processEnvironment, cachedPath }
    public let source: Source
    public let shell: String?
    public let captureDuration: Duration?
    public let pathEntries: [String]                 // child PATH (non-secret)
    public let capturedAt: Date?
    public let isProvisional: Bool                   // true until the login-shell capture finished
}
```

### 4.2 `BranchBoxKit/Backend/Progress.swift`

```swift
public enum LogLevel: String, Sendable, Hashable, Codable { case trace, debug, info, warn, error, output }

public struct LogLine: Sendable, Hashable {
    public enum Source: String, Sendable, Hashable { case stdout, stderr, app }
    public let timestamp: Date?
    public let level: LogLevel
    public let source: Source
    public let target: String?                       // tracing target, e.g. worktree_core::modules::compose
    public let message: String                       // ANSI-stripped, CR-trimmed, ≤ 16 KiB
    public init(timestamp: Date?, level: LogLevel, source: Source, target: String?, message: String) { … }
}

public enum OperationPhase: Sendable, Hashable {
    case preparing, creatingWorktree, module(String), runtime(String), startingEnvironment
    case removingWorktree, deletingBranch, cleaningRuntime, building, provisioningTunnel, detectingAdapter
    case item(index: Int, of: Int, name: String)     // prune rows
    case step(String)
}

public enum ProgressEvent: Sendable, Hashable {
    case log(LogLine)
    case phase(OperationPhase)
    case warning(String)
}

/// Called from any thread, in order, by a single producer per operation.
public typealias ProgressSink = @Sendable (ProgressEvent) -> Void
```

### 4.3 `BranchBoxKit/Backend/Requests.swift`

```swift
// RuntimeProvider is declared in Models (§4.8) and used here.

public enum BranchPolicy: String, Sendable, Hashable, Codable { case keep, deleteIfMerged, forceDelete }

public enum DevcontainerReusePolicy: String, Sendable, Hashable, Codable { case fail, preserve, overwrite, inspect }

public struct StartFeatureRequest: Sendable, Hashable, Codable {
    public enum Mode: String, Sendable, Hashable, Codable { case full, minimal }
    public enum Reuse: Sendable, Hashable, Codable { case none, existingWorktree(DevcontainerReusePolicy), retainedRuntime }
    public var project: ProjectRef
    public var name: String                          // resolved slug; `--title` is NEVER sent
    public var base: String?                         // nil = current HEAD (omit --base)
    public var branchPrefix: String?
    public var runtime: RuntimeProvider              // always explicit (--runtime)
    public var mode: Mode
    public var prompt: String?                       // ≤ 2000 chars (validated by StartDraft)
    public var useDefaultPrompt: Bool                // --default-prompt (minimal only)
    public var skipModules: [String]                 // compose | database | tunnel | specs
    public var reuse: Reuse                          // .retainedRuntime → --reuse-runtime
    public var keepRuntimeOnFailure: Bool            // sbx only
    public var verbose: Bool                         // RUST_LOG=debug + --telemetry
    public init(project: ProjectRef, name: String, runtime: RuntimeProvider) { … }  // defaults: full, none, false
}

/// The user's explicit, per-presentation consent to discard changes. Built ONLY by RecoveryPlanner
/// from a refusal. Never defaulted, never persisted.
public struct DiscardConsent: Sendable, Hashable, Codable {
    public let userFiles: [String]                   // exact paths confirmed; empty = BranchBox-generated files only
    public let confirmedAt: Date
    public init(userFiles: [String], confirmedAt: Date = .now) { … }
}

public struct TeardownRequest: Sendable, Hashable, Codable {
    public var feature: FeatureRef
    public var recordedBranch: String?               // FeatureRecord.branchName
    public var branch: BranchPolicy                  // ALWAYS explicit
    public var discard: DiscardConsent?              // nil on every first attempt
    public var forceRemoval: Bool                    // only from locked / status-unavailable / worktree-missing recoveries
    public var completeSpec: Bool
    public init(feature: FeatureRef, recordedBranch: String?, branch: BranchPolicy) { … }   // discard nil, false, false
}

public struct ExecRequest: Sendable, Hashable, Codable {
    public enum Target: Sendable, Hashable, Codable { case featureRuntime, devcontainer }
    public var feature: FeatureRef
    public var command: [String]                     // argv; "Run through shell" = ["/bin/sh","-lc",text]
    public var target: Target
    public var timeout: Duration?                    // nil = cancellable only
    public init(feature: FeatureRef, command: [String], target: Target = .featureRuntime, timeout: Duration? = nil) { … }
}

public enum DevcontainerAction: Sendable, Hashable, Codable {
    case up(removeExisting: Bool, buildNoCache: Bool)   // Rebuild = .up(removeExisting: true, buildNoCache: true)
    case down(removeVolumes: Bool)
    case build(noCache: Bool)
}

public enum SyncStrategy: String, Sendable, Hashable, Codable { case copy, symlink }

public struct SyncRequest: Sendable, Hashable, Codable {
    public var project: ProjectRef
    public var strategy: SyncStrategy?
    public var dryRun: Bool
    public var features: [String]                    // requires .devcontainerSyncJSON; empty = all
}

public struct InitRequest: Sendable, Hashable, Codable {
    public enum OnePassword: Sendable, Hashable, Codable { case unchanged, skip, configure(githubRef: String, signingKeyRef: String?, verify: Bool) }
    public var folder: URL
    public var stack: String?                        // rails | nodejs | rust | generic; nil = auto
    public var skipDevcontainer: Bool
    public var skipEnv: Bool
    public var codingAgents: Bool                    // false → --no-coding-agents
    public var reorganize: Bool                      // explicit opt-in only (moves the repo)
    public var dryRun: Bool
    public var mode: Mode
    public enum Mode: String, Sendable, Hashable, Codable { case initialize, update, validate }
    public var onePassword: OnePassword              // requires .initJSON; ignored (hidden) on legacy
    public var tunnelsEnabled: Bool?                 // applied after init via config apply (requires .config)
}

public enum JSONValue: Sendable, Hashable, Codable {
    case null, bool(Bool), number(Double), string(String), array([JSONValue]), object([String: JSONValue])
}

public struct ConfigChange: Sendable, Hashable, Codable {
    public let key: String                           // dotted key from the key registry, e.g. "feature.branch_prefix"
    public let value: JSONValue?                     // nil = unset
}
public struct ConfigPatch: Sendable, Hashable, Codable { public var changes: [ConfigChange] }

/// Token wrapper whose description is always redacted.
public struct SecretString: Sendable, Hashable, CustomStringConvertible, CustomDebugStringConvertible {
    public let value: String
    public init(_ value: String) { self.value = value }
    public var description: String { "••••" }
    public var debugDescription: String { "••••" }
}

public struct TunnelCredentialsRequest: Sendable, Hashable {
    public var accountID: String
    public var apiToken: SecretString?               // sent on stdin; never argv/log
    public var clear: Bool
}

public struct StrayWorktree: Sendable, Hashable, Codable {
    public let path: String
    public let branch: String?                       // refs/heads/ stripped
    public let head: String?
    public let locked: Bool
    public let prunable: Bool
}

public struct PruneSelection: Sendable, Hashable { public var project: ProjectRef; public var rows: [TeardownRequest] }

/// Everything an OperationRecord can be asked to run; also the payload of `RecoveryAction.retry`.
public enum OperationRequestContext: Sendable, Hashable {
    case start(StartFeatureRequest)
    case teardown(TeardownRequest)
    case prune(PruneSelection)
    case exec(ExecRequest)
    case devcontainer(DevcontainerAction, FeatureRef)
    case syncDevcontainers(SyncRequest)
    case tunnelOpen(FeatureRef)
    case tunnelRemove(FeatureRef, force: Bool)
    case initProject(InitRequest)
    case applyConfig(ConfigPatch, ProjectRef)
    case tunnelCredentials(TunnelCredentialsRequest, ProjectRef)
    case deleteBranch(String, ProjectRef, force: Bool)
    case removeStray(StrayWorktree, ProjectRef, discardChanges: Bool)
}
```

### 4.4 `BranchBoxKit/Backend/BackendError.swift`

```swift
public struct Diagnostics: Sendable, Hashable {
    public var summary: String                       // the CLI's own cause line (or envelope message); never raw stderr
    public var causes: [String]                      // anyhow "Caused by:" / envelope causes
    public var exitCode: Int32?
    public var signal: Int32?
    public var logTail: [String]                     // last ≤ 50 ANSI-stripped stderr lines
    public var invocation: String?                   // redacted argv (prompt/env/token values removed; args > 200 chars truncated)
    public var cliVersion: String?
    public init(summary: String, causes: [String] = [], exitCode: Int32? = nil, signal: Int32? = nil,
                logTail: [String] = [], invocation: String? = nil, cliVersion: String? = nil) { … }
}

public struct ChangedFile: Sendable, Hashable, Codable {
    public let path: String
    public let kind: String                          // untracked|modified|added|deleted|typechange|conflicted|staged
    public let area: String                          // devcontainer|compose|vscode|spec|env|other
}

public enum RefusalCause: Sendable, Hashable {
    case uncommittedChanges(files: [ChangedFile])
    case moduleFilesDirty(files: [String], userChanges: [ChangedFile])   // 0.13.x banner; userChanges = re-preflight result
    case unmergedBranch(branch: String, ahead: Int?)
    case worktreeLocked(reason: String?)
    case statusUnavailable(cause: String)
    case worktreeRemovalFailed(cause: String)
    case worktreeExists(path: String)
    case worktreeNotFound(String)
    case featureNotFound(String)
    case branchExists(String)
    case invalidName(String)
    case notGitRepository(String)
    case runtimePrerequisite(provider: String, detail: String)   // "Sign in with: sbx login", "local-vm requires a Linux host"
    case registryLocked(path: String)
    case confirmationRequired
    case configInvalid(key: String?, detail: String)
    case devcontainerSourceMissing
    case other(code: String)
}

public struct Refusal: Sendable, Hashable {
    public let cause: RefusalCause
    public let message: String                       // cause-naming text shown to the user
    public let diagnostics: Diagnostics
    public let plan: TeardownPlanDocument?           // present for teardown refusals (0.14 details.plan or app preflight)
}

public struct PartialFailure: Sendable, Hashable {
    public let completed: [String]                   // e.g. ["Worktree removed"]
    public let remaining: Refusal
}

public enum ProjectProblem: Sendable, Hashable {
    case missing(String)
    case notGitRepository(String)
    case notInitialized(String)
    case workingDirectoryMissing(String)
}

public enum BackendError: Error, Sendable, Hashable {
    case cliNotFound(searched: [String])
    case cliTooOld(found: SemVer, minimum: SemVer, path: String)
    case cliUnusable(path: String, reason: String)
    case launchFailed(executable: String, reason: String)
    case projectInvalid(ProjectProblem)
    case refused(Refusal)
    case partial(PartialFailure)
    case commandFailed(Diagnostics)
    case decodeFailed(what: String, detail: String, diagnostics: Diagnostics)
    case registryCorrupted(path: String, diagnostics: Diagnostics)
    case unsupported(Capability, minimumCLI: String)        // e.g. (.config, "0.14.0")
    case timedOut(operation: String, after: Duration, diagnostics: Diagnostics)
    case cancelled(note: String?)                            // e.g. "may have left a partial worktree"
    /// CancellationError → .cancelled(nil); BackendError passthrough; anything else → .commandFailed.
    public static func normalize(_ error: any Error) -> BackendError { … }
}
```

### 4.5 `BranchBoxKit/Backend/Recovery.swift` (recoveries, attention) and `Preferences.swift`

```swift
/// Typed recoveries carry the concrete request to re-run. Built only by RecoveryPlanner (Kit/Planning).
public enum RecoveryAction: Sendable, Hashable, Identifiable {
    case retry(OperationRequestContext, label: String, destructive: Bool, confirmation: String?)
    case runInTerminal(command: [String], workingDirectory: String?, label: String)
    case revealInFinder(path: String)
    case copyCommand(String, label: String)
    case openDoctor
    case locateCLI
    case refresh(ProjectRef)
    case showLog(operation: UUID)
    public var id: String { … }                      // stable: case name + label
}

public enum EditorChoice: Sendable, Hashable, Codable { case vscode, cursor, custom(appPath: String) }
public enum EditorOpenMode: String, Sendable, Hashable, Codable { case folder, devContainer }
public enum TerminalChoice: Sendable, Hashable, Codable { case terminal, iTerm, custom(template: String) } // {path} {command}
public enum AgentChoice: Sendable, Hashable, Codable { case claude, codex, custom(command: String) }
public enum RefreshInterval: Int, Sendable, Hashable, Codable { case s30 = 30, m1 = 60, m5 = 300, m15 = 900, manual = 0 }

public enum AttentionReason: Sendable, Hashable { case degraded, failedRetained, orphaned, interrupted, setupIncomplete(module: String), folderMissing, unknownStatus(String), unregisteredWorktree }
public struct AttentionItem: Sendable, Hashable, Identifiable { public let id: String; public let featureOrPath: String; public let reason: AttentionReason }
```

### 4.6 `BranchBoxKit/Backend/BranchBoxBackend.swift` and `Bootstrap.swift`

```swift
public protocol BranchBoxBackend: Sendable {
    // Identity & environment
    func identity() async throws -> BackendIdentity
    func doctor(_ project: ProjectRef?) async -> DoctorReport                 // never throws; per-check status

    // Projects
    func resolveProject(at folder: URL) async throws -> ProjectResolution      // normalizes to the MAIN worktree
    func detect(_ folder: URL) async throws -> DetectReport
    func readConfig(_ project: ProjectRef) async throws -> ProjectConfigDocument
    func applyConfig(_ patch: ConfigPatch, to project: ProjectRef, dryRun: Bool) async throws -> ConfigApplyResult   // .config
    func setTunnelCredentials(_ request: TunnelCredentialsRequest, in project: ProjectRef) async throws -> TunnelCredentialsResult // .tunnelCredentials
    func initProject(_ request: InitRequest, progress: @escaping ProgressSink) async throws -> InitReport

    // Feature reads
    func listFeatures(in project: ProjectRef, includeRemoved: Bool) async throws -> FeatureListing   // + strays
    func listBranches(in project: ProjectRef) async throws -> BranchList
    func previewName(_ input: String, in project: ProjectRef) async throws -> NamePreview
    func planTeardown(_ request: TeardownRequest) async throws -> TeardownPlanDocument               // CLI --dry-run or app preflight
    func devcontainerStatus(for feature: FeatureRef) async throws -> DevcontainerStatus

    // Feature mutations (cancellable; progress streamed)
    func startFeature(_ request: StartFeatureRequest, progress: @escaping ProgressSink) async throws -> StartSummary
    func teardownFeature(_ request: TeardownRequest, progress: @escaping ProgressSink) async throws -> TeardownOutcome
    func exec(_ request: ExecRequest, progress: @escaping ProgressSink) async throws -> ExecResult     // non-zero inner exit is DATA
    func devcontainer(_ action: DevcontainerAction, for feature: FeatureRef, progress: @escaping ProgressSink) async throws -> DevcontainerResult
    func syncDevcontainers(_ request: SyncRequest, progress: @escaping ProgressSink) async throws -> SyncReport
    func openTunnel(_ feature: FeatureRef, progress: @escaping ProgressSink) async throws -> TunnelChange
    func removeTunnel(_ feature: FeatureRef, force: Bool, progress: @escaping ProgressSink) async throws -> TunnelChange
    func deleteBranch(_ branch: String, in project: ProjectRef, force: Bool) async throws
    func removeStray(_ stray: StrayWorktree, in project: ProjectRef, discardChanges: Bool) async throws

    /// Shell-escaped, redacted command line for "Copy as Command"; nil for non-CLI backends.
    func previewCommandLine(_ request: OperationRequestContext) -> String?
}

public struct BackendSettings: Sendable, Hashable {
    public var cliPathOverride: String?
    public var extraEnvironment: [String: String]    // never logged; redacted in diagnostics
    public var verboseLogs: Bool                     // RUST_LOG=debug
    public var agentCommand: String?                 // → BRANCHBOX_DEFAULT_AGENT_CMD
    public var agentName: String?                    // → BRANCHBOX_DEFAULT_AGENT_NAME
    public init(cliPathOverride: String? = nil, extraEnvironment: [String: String] = [:], verboseLogs: Bool = false,
                agentCommand: String? = nil, agentName: String? = nil) { … }
}

public enum BackendBootstrap: Sendable {
    case ready(any BranchBoxBackend, BackendIdentity)
    case unavailable(BackendError, CLIResolution?)
}

/// Implemented by CLIBackendBootstrapper (BranchBoxCLI) and PreviewBootstrapper (BranchBoxPreview).
public protocol BackendBootstrapping: Sendable {
    func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap
    func environmentSummary() async -> EnvironmentSummary?
    func recaptureEnvironment() async
    func terminateAllProcesses() async               // app quit
}
```

### 4.7 Process contract: `BranchBoxKit/Process/ProcessTypes.swift`

```swift
public struct ProcessSpec: Sendable {
    public var executable: URL                       // absolute; never /usr/bin/env
    public var arguments: [String]
    public var environment: [String: String]         // complete child env (ChildEnvironment.make)
    public var workingDirectory: URL?                // validated to exist before spawn
    public var standardInput: Data?                  // nil = /dev/null; else written then closed (config patch, token)
    public var timeout: Duration?                    // nil = cancellable only
    public var interruptGrace: Duration = .seconds(5)    // SIGINT → SIGTERM
    public var terminateGrace: Duration = .seconds(3)    // SIGTERM → SIGKILL
    public var drainGrace: Duration = .seconds(2)        // max wait for pipe EOF after the leader exits
    public var stdoutLimit: Int = 64 << 20               // exceeding → .stdoutTooLarge
    public var stderrTailLines: Int = 400
    public var streamStdout: Bool = false                // text commands (legacy init/sync/detect) stream stdout lines too
    public init(executable: URL, arguments: [String], environment: [String: String], workingDirectory: URL?) { … }
}

public enum Termination: Sendable, Hashable { case exited(Int32), signaled(Int32) }

public struct OutputLine: Sendable, Hashable {
    public enum Channel: String, Sendable, Hashable { case stdout, stderr }
    public let channel: Channel
    public let text: String                          // ANSI-stripped; split on \n, \r\n and bare \r; ≤ 16 KiB
}

public struct ProcessResult: Sendable {
    public let termination: Termination
    public let stdout: Data
    public let stderrTail: [String]
    public let duration: Duration
}

public enum ProcessRunError: Error, Sendable {
    case workingDirectoryMissing(String)
    case launchFailed(executable: String, reason: String)
    case stdoutTooLarge(limit: Int)
    case cancelled(partial: ProcessResult)           // thrown only after the whole process group is gone (bounded)
    case timedOut(after: Duration, partial: ProcessResult)
}

public protocol ProcessRunning: Sendable {
    /// Throws only ProcessRunError. Never blocks a thread; never calls waitUntilExit.
    /// If the calling Task is already cancelled, throws .cancelled WITHOUT spawning.
    func run(_ spec: ProcessSpec, onLine: @escaping @Sendable (OutputLine) -> Void) async throws -> ProcessResult
    func terminateAll() async                        // SIGINT→SIGTERM→SIGKILL every live group; bounded 10 s
}

/// Implemented by EnvironmentProvider (BranchBoxCLI).
public enum EnvironmentPurpose: Sendable { case read, mutation }
public protocol EnvironmentProviding: Sendable {
    /// .read returns immediately (provisional env if capture not finished); .mutation awaits the capture (≤ 8 s + fallbacks).
    func childEnvironment(for purpose: EnvironmentPurpose, settings: BackendSettings) async -> [String: String]
    func summary() async -> EnvironmentSummary
    func recapture() async
}
```

### 4.8 Models: `BranchBoxKit/Models/*`

Decoding rules (apply to every model):
- **R-1** Explicit `CodingKeys`; never `keyDecodingStrategy`. Feature, start, teardown, tunnel and plan JSON is snake_case. Devcontainer up/down/build/exec JSON is camelCase.
- **R-2** Only identity keys are required (`work_feature`, `module`, `path`). Everything else uses `decodeIfPresent` with defaults. Malformed optional sub-objects decode via `try?` to nil.
- **R-3** Arrays of records decode through `Lossy<T>`. Dropped elements are counted (`FeatureListing.droppedRecords`) and never fail the list.
- **R-4** Enums are **open**: they decode unknown raw values into `.unknown(String)` and round-trip them.
- **R-5** Dates decode as `String` and are parsed by `RFC3339.parse`: 0–9 fraction digits, `Z` or `±HH:MM`. A bad date becomes nil, never a throw. `ISO8601DateFormatter` is not used.
- **R-6** Unknown keys are ignored.
- **R-7** `CLIJSON.decode<T>(_ data: Data) throws -> (value: T, preamble: String?)` decodes strictly first. If that fails, it decodes from the first line starting with `{` or `[` and returns the skipped text as `preamble` (BUG-11).

```swift
// OpenStringEnum.swift
public protocol OpenStringEnum: Codable, Hashable, Sendable { init(raw: String); var raw: String { get } }
public enum FeatureStatus: OpenStringEnum { case active, degraded, failedRetained, orphaned, removed, unknown(String) }   // "failed_retained"
public enum ModuleStatus: OpenStringEnum { case success, skipped, failed, unknown(String) }        // "ok" decodes as .success
public enum RuntimeProvider: OpenStringEnum { case container, sbx, localVM, inGuest, unknown(String) } // "local-vm","in-guest"
public enum TunnelStatus: OpenStringEnum { case pending, active, manual, disabled, unknown(String) }
public enum AgentPlanStatus: OpenStringEnum { case ready, waiting, blocked, disabled, unknown(String) }
public enum SetupState: OpenStringEnum { case inProgress, interrupted, unknown(String) }            // "in_progress"

// RFC3339.swift / Lossy.swift / CLIJSON.swift
public enum RFC3339 { public static func parse(_ text: String) -> Date? { … } }
public struct Lossy<T: Decodable & Sendable>: Decodable, Sendable { public let value: T?; public let error: String? }
public enum CLIJSON {
    public static func decoder() -> JSONDecoder { … }
    public static func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> (value: T, preamble: String?) { … }
}

// FeatureRecord.swift — element of `feature list --json`
public struct FeatureRecord: Decodable, Sendable, Hashable, Identifiable {
    public var id: String { workFeature }
    public let workFeature: String                   // work_feature (required)
    public let branchName: String                    // branch_name ("" if absent)
    public let worktreePath: String?                 // worktree_path
    public let baseBranch: String?                   // base_branch
    public let featureURL: String?                   // feature_url (scheme-less)
    public let composeProjectName: String?           // compose_project_name
    public let envPath: String?                      // env_path
    public let status: FeatureStatus
    public let createdAt: Date?, updatedAt: Date?, removedAt: Date?, lastSyncAt: Date?
    public let tunnel: TunnelState?                  // NESTED object
    public let color: String?                        // "#e67e22"
    public let lastCommit: String?
    public let prNumber: Int?
    public let devcontainerOutdated: Bool
    public let syncStrategy: String?
    public let startMode: String?                    // "full" | "minimal"
    public let promptSeed: String?
    public let moduleOutcomes: [ModuleOutcome]
    public let adapter: AdapterInfo?
    public let runtime: RuntimeInfo                  // default .container if absent
    public let defaultAgent: DefaultAgentPlan?       // default_agent
    public let setup: SetupInfo?                     // NEW (write-ahead; §5.4). nil on legacy CLIs / completed starts
    public var branchPrefix: String? { … }           // strip "/<work_feature>"; "" when branch == name; nil if underivable
    public var urls: FeatureURLs { … }
}
public struct SetupInfo: Codable, Sendable, Hashable { public let state: SetupState; public let pid: Int?; public let startedAt: Date? }
public struct ModuleOutcome: Codable, Sendable, Hashable { public let module: String; public let status: ModuleStatus
    public let durationMs: Int?; public let notes: [String]; public let forced: Bool; public let recordedAt: Date? }
public struct TunnelState: Codable, Sendable, Hashable { public let provider: String?; public let hostname: String?
    public let serviceURL: String?; public let status: TunnelStatus; public let instructions: [String]; public let notes: String?
    public let lastUpdated: Date? }
public struct AdapterInfo: Codable, Sendable, Hashable { public let name: String?; public let serviceURL: String?; public let warnings: [String] }
public struct PublishedPort: Codable, Sendable, Hashable { public let host: Int; public let runtime: Int }
public struct RuntimeInfo: Codable, Sendable, Hashable { public let provider: RuntimeProvider; public let runtimeID: String?
    public let publishedPorts: [PublishedPort]; public let containerID: String?; public let workspaceFolder: String?
    public let containerUser: String?; public let configPath: String?; public static let containerDefault: RuntimeInfo }
public struct DefaultAgentPlan: Codable, Sendable, Hashable { public let status: AgentPlanStatus; public let label: String?
    public let command: String?; public let detail: String?; public let followup: String? }

// FeatureURLs.swift
public struct FeatureURLs: Sendable, Hashable {
    public let primary: URL?                         // feature_url; keeps scheme if present, else https:// (CLI parity)
    public let primaryHTTP: URL?                     // "Open with http://" alternative
    public let tunnel: URL?                          // https://<tunnel.hostname> when it differs
    public let ports: [PortLink]                     // http://localhost:<host>, subtitle "→ container :<runtime>"
    public let inContainerServiceURL: String?        // adapter.service_url — display/copy only, never a link
}
public struct PortLink: Sendable, Hashable { public let label: String; public let url: URL; public let runtimePort: Int }

// FeatureListing.swift
public struct FeatureListing: Sendable, Hashable {
    public let features: [FeatureRecord]
    public let strays: [StrayWorktree]
    public let droppedRecords: Int
    public let warnings: [String]
}
public struct BranchList: Sendable, Hashable { public let current: String?; public let local: [String]; public let remote: [String] }
public struct NamePreview: Sendable, Hashable { public let input: String; public let slug: String?; public let valid: Bool
    public let branchName: String?; public let worktreePath: String?; public let problem: String? }
public struct ProjectResolution: Sendable, Hashable {
    public enum Normalization: Sendable, Hashable { case none, fromFeatureWorktree, fromParentContainer }
    public let project: ProjectRef; public let requested: URL; public let normalization: Normalization; public let initialized: Bool
}

// StartSummary.swift — `feature start --json`
public struct StartSummary: Decodable, Sendable, Hashable { public let workFeature: String; public let branchName: String
    public let worktreePath: String?; public let mode: String?; public let promptSeed: String?; public let featureURL: String?
    public let composeProjectName: String?; public let runtime: RuntimeInfo?; public let envPath: String?; public let color: String?
    public let moduleOutcomes: [ModuleOutcome]; public let skippedModules: [SkippedModule]; public let warnings: [String]
    public let adapter: AdapterInfo?; public let tunnel: TunnelState?; public let promptBridgeEnabled: Bool?
    public let generatedAt: Date?; public let defaultAgent: DefaultAgentPlan?
    public var preambleWarning: String?              // set by CLIBackend from CLIJSON preamble
}
public struct SkippedModule: Codable, Sendable, Hashable { public let module: String; public let reason: String? }

// TeardownSummary.swift — `feature teardown --json` (+ additive 0.14 fields)
public struct TeardownSummary: Decodable, Sendable, Hashable { public let workFeature: String; public let branchName: String?
    public let worktreeRemoved: Bool; public let branchDeleted: Bool; public let adapterCleanupWarnings: [String]
    public let moduleReports: [ModuleReport]; public let runtimeTeardown: RuntimeTeardownReport?; public let warnings: [String]
    public let branchAction: String?                 // 0.14: keep|delete|force_delete
    public let branchDeleteError: String?            // 0.14
    public let discardedChanges: [ChangedFile]       // 0.14
    public let preserved: [PreservedFile]            // 0.14
    public let registryUpdated: Bool?                // 0.14
}
public struct ModuleReport: Codable, Sendable, Hashable { public let name: String; public let teardownOk: Bool; public let errors: [String] }
public struct RuntimeTeardownReport: Codable, Sendable, Hashable { public let provider: String?; public let runtimeID: String?
    public let verified: Bool; public let residueFree: Bool; public let residue: [ResidueItem] }
public struct ResidueItem: Codable, Sendable, Hashable { public let kind: String; public let identifiers: [String] }
public struct PreservedFile: Codable, Sendable, Hashable { public let path: String; public let destination: String }
public struct TeardownOutcome: Sendable, Hashable {
    public let summary: TeardownSummary
    public let branch: BranchOutcome
    public let worktreeGone: Bool                    // verified on disk by CLIBackend after return
}
public enum BranchOutcome: Sendable, Hashable {
    public enum Deleter: String, Sendable, Hashable { case cli, app }
    case kept(String?)
    case deleted(String, by: Deleter)
    case deleteFailed(String, reason: String)        // UI offers .retry(.deleteBranch(force: true)) only if unmerged
    case notFound(String)
}

// TeardownPlanDocument.swift — mirrors §5.5 JSON exactly
public struct TeardownPlanDocument: Decodable, Sendable, Hashable {
    public enum Source: String, Sendable, Hashable { case cli, appPreflight }
    public var source: Source                        // not in JSON; CLIBackend sets it (decoder default .cli)
    public let workFeature: String
    public let registered: Bool
    public let status: FeatureStatus?
    public let worktree: Worktree
    public let changes: Changes
    public let branch: Branch?
    public let defaults: Defaults?
    public let runtime: RuntimeRef?
    public let tunnel: TunnelRef?
    public let blockers: [Blocker]
    public let warnings: [String]
    public struct Worktree: Codable, Sendable, Hashable { public let path: String; public let exists: Bool; public let locked: Bool; public let lockReason: String? }
    public struct Changes: Codable, Sendable, Hashable { public let statusAvailable: Bool; public let truncated: Bool
        public let user: [ChangedFile]; public let generated: [GeneratedFile]; public let preserved: [PreservedFile] }
    public struct GeneratedFile: Codable, Sendable, Hashable { public let path: String; public let rule: String }
    public struct Branch: Codable, Sendable, Hashable { public let name: String; public let source: String; public let exists: Bool
        public let upstream: String?; public let reference: String; public let referenceName: String; public let merged: Bool
        public let mergedIntoHead: Bool; public let ahead: Int; public let action: String }
    public struct Defaults: Codable, Sendable, Hashable { public let deleteBranchByDefault: Bool; public let forceDeleteUnmergedByDefault: Bool }
    public struct RuntimeRef: Codable, Sendable, Hashable { public let provider: String?; public let runtimeID: String? }
    public struct TunnelRef: Codable, Sendable, Hashable { public let status: TunnelStatus? }
    public struct Blocker: Codable, Sendable, Hashable { public let kind: String; public let message: String; public let override: String?
        public let count: Int?; public let branch: String?; public let ahead: Int?; public let cause: String? }
}

// ExecResult.swift — accepts `exit_code` (feature exec) and `exitCode` (devcontainer exec)
public struct ExecResult: Decodable, Sendable, Hashable { public let exitCode: Int32; public let stdout: String; public let stderr: String
    public let outcome: String? }

// DevcontainerResults.swift (camelCase)
public struct DevcontainerResult: Decodable, Sendable, Hashable { public let outcome: String; public let containerID: String?
    public let remoteUser: String?; public let remoteWorkspaceFolder: String?; public let composeProjectName: String?
    public let removedContainers: [String]; public let imageName: String?; public let message: String? }
public struct DevcontainerStatus: Sendable, Hashable {
    public enum State: String, Sendable, Hashable { case running, stopped, notCreated, unknown }
    public let state: State; public let containerID: String?; public let service: DevcontainerServiceInfo?
}
public struct DevcontainerServiceInfo: Decodable, Sendable, Hashable { public let serviceName: String?; public let port: Int?
    public let serviceURL: String?; public let containerUser: String? }    // devcontainer detect --json (snake_case)

// TunnelChange.swift — open: {work_feature,state,warnings}; remove: {work_feature,previous_state,updated_state,warnings}
public struct TunnelChange: Decodable, Sendable, Hashable { public let workFeature: String; public let state: TunnelState?
    public let previousState: TunnelState?; public let warnings: [String] }
public struct TunnelCredentialsResult: Decodable, Sendable, Hashable { public let credentialsPath: String; public let accountID: String?
    public let tokenPresent: Bool }

// ProjectConfig.swift
public struct ProjectConfig: Decodable, Sendable, Hashable {     // typed read model of the effective config (defaults applied)
    public let runtimeProvider: RuntimeProvider; public let sbxRunServices: [String]
    public let branchPrefix: String; public let deleteBranchByDefault: Bool; public let forceDeleteUnmergedByDefault: Bool
    public let promptForceDeleteUnmerged: Bool; public let tunnelEnabled: Bool; public let tunnelDefaultProvider: String?
    public let cloudflared: CloudflaredConfig?; public let editorDefaultAgent: String?; public let autoLaunchAgentTerminal: Bool
}
public struct CloudflaredConfig: Codable, Sendable, Hashable { public let accountID: String?; public let tunnelNamePrefix: String?
    public let dnsZone: String?; public let serviceURL: String?; public let manualInstructions: Bool?; public let apiTokenPath: String? }
public struct ConfigKeyDescriptor: Decodable, Sendable, Hashable { public let key: String; public let type: String  // bool|string|enum|string_list
    public let allowed: [String]; public let defaultValue: JSONValue?; public let value: JSONValue?; public let source: String; public let description: String }
public struct ProjectConfigDocument: Sendable, Hashable {
    public let path: String; public let exists: Bool; public let effective: ProjectConfig; public let keys: [ConfigKeyDescriptor]
    public let editable: Bool                        // false on legacy CLIs (read-only from config.json)
}
public struct ConfigApplyResult: Decodable, Sendable, Hashable {
    public struct Change: Decodable, Sendable, Hashable { public let key: String; public let old: JSONValue?; public let new: JSONValue? }
    public let changed: [Change]; public let effective: ProjectConfig
}

// DetectReport.swift / SyncReport.swift / DoctorReport.swift / InitReport.swift
public struct DetectReport: Sendable, Hashable { public let project: String?; public let gitRepository: Bool; public let initialized: Bool
    public let stack: String?; public let adapter: String?; public let modules: [String]; public let hasDevcontainer: Bool?
    public let hasEnv: Bool?; public let warnings: [String]; public let rawText: String? }   // rawText on legacy
public struct SyncReport: Sendable, Hashable {
    public struct Row: Sendable, Hashable { public enum Status: String, Sendable, Hashable { case synced, wouldSync = "would_sync", skipped, failed, unknown }
        public let feature: String; public let worktreePath: String?; public let status: Status; public let files: [String]
        public let skipReason: String?; public let error: String? }
    public let dryRun: Bool; public let strategy: String?; public let rows: [Row]
    public var failedCount: Int { … }; public let rawText: String?
}
public struct DoctorCheck: Sendable, Hashable { public enum Status: String, Sendable, Hashable { case ok, warn, error, skipped }
    public let id: String; public let title: String; public let required: Bool; public let status: Status; public let path: String?
    public let version: String?; public let detail: String?; public let remediation: String? }
public struct DoctorReport: Sendable, Hashable { public enum Source: String, Sendable, Hashable { case cli, app, merged }
    public let source: Source; public let checks: [DoctorCheck]; public let generatedAt: Date }
public struct InitReport: Sendable, Hashable { public let workspacePath: String?; public let reorganized: Bool; public let stack: String?
    public let adapter: String?; public let modules: [String]; public let warnings: [String]; public let nextSteps: [String]
    public let onePasswordStatus: String?; public let log: [String] }

// ErrorEnvelope.swift / VersionInfo.swift
public struct ErrorEnvelope: Decodable, Sendable, Hashable {
    public struct Body: Decodable, Sendable, Hashable { public let code: String; public let message: String; public let causes: [String]; public let details: JSONValue? }
    public let schemaVersion: Int; public let error: Body
}
public struct VersionInfo: Decodable, Sendable, Hashable { public let version: String; public let contractVersion: Int; public let capabilities: [String] }
```

### 4.9 Planning API: `BranchBoxKit/Planning/*` (SW-3; pure, no I/O)

```swift
public enum RecoveryPlanner {
    /// Recoveries appear ONLY for the matching refusal/partial. Destructive retries carry a confirmation naming what is lost.
    public static func recoveries(for error: BackendError, after context: OperationRequestContext?) -> [RecoveryAction]
}

public struct TeardownDraft: Sendable, Hashable {
    public init(feature: FeatureRef, recordedBranch: String?, plan: TeardownPlanDocument, config: ProjectConfig?)
    public var branch: BranchPolicy                  // default: deleteBranchByDefault ? .deleteIfMerged : .keep; .keep if unmerged
    public var completeSpec: Bool
    public var visibleBranchOptions: [BranchPolicy]  // .forceDelete only when the branch exists and is unmerged
    public var blockingReason: String?               // e.g. "feature/x has 3 commits not in main; choose Keep or Force-delete"
    public var pendingDiscardWarning: String?        // "2 uncommitted changes — you'll be asked to confirm discarding them"
    public func makeRequest() -> TeardownRequest     // discard == nil, forceRemoval == false — always
}

public struct PrunePlanner: Sendable {
    public struct Row: Sendable, Hashable { public let feature: FeatureRecord; public let plan: TeardownPlanDocument?
        public var selected: Bool; public let defaultReason: String? }
    public static func rows(features: [FeatureRecord], plans: [String: TeardownPlanDocument]) -> [Row]  // safe set preselected
    public static func selection(project: ProjectRef, rows: [Row], policy: BranchPolicy, completeSpec: Bool) -> PruneSelection
}

public struct StartDraft: Sendable, Hashable {           // one value per sheet presentation; discarded on cancel (MAC-18)
    public var project: ProjectRef?; public var input: String; public var preview: NamePreview?; public var base: String?
    public var runtime: RuntimeProvider; public var mode: StartFeatureRequest.Mode; public var prompt: String
    public var useDefaultPrompt: Bool; public var branchPrefix: String; public var skipModules: Set<String>
    public var reuse: StartFeatureRequest.Reuse; public var keepRuntimeOnFailure: Bool; public var verbose: Bool
    public static let promptLimit = 2000
    public func validationErrors(existing: [FeatureRecord], folderExists: Bool) -> [String]
    public func makeRequest() -> StartFeatureRequest?    // nil when invalid
}

public enum RemediationAction: Sendable, Hashable {
    case resumeSetup(StartFeatureRequest)            // setup interrupted → start --reuse
    case retryRetainedRuntime(StartFeatureRequest)   // failed_retained / degraded sbx → --runtime sbx --reuse-runtime
    case recreateRuntime(StartFeatureRequest)        // orphaned with worktree present → start --reuse
    case startEnvironment(FeatureRef)                // container: devcontainer up
    case teardown(FeatureRef, preselect: BranchPolicy)
    case cleanUpMissingFolder(TeardownRequest)       // forceRemoval + .keep
    case syncDevcontainers(ProjectRef)
    case runDoctor
    case deleteLeftoverBranch(String, ProjectRef)
}
// AttentionReason / AttentionItem are declared in Kit/Backend/Recovery.swift (§4.5, SW-0).
public enum Remediation {
    public static func attention(for record: FeatureRecord, folderExists: Bool) -> AttentionReason?
    public static func actions(for record: FeatureRecord, project: ProjectRef, identity: BackendIdentity?, folderExists: Bool) -> [RemediationAction]
}

public struct HostLaunchPlan: Sendable, Hashable {     // executed by App/Services/HostLauncher
    public enum Kind: Sendable, Hashable { case openFolder(appBundleID: String?, appPath: String?, path: String)
        case openURL(URL); case terminalScript(script: String, terminal: TerminalChoice); case disabled(reason: String) }
    public let kind: Kind
    public static func editor(_ choice: EditorChoice, mode: EditorOpenMode, record: FeatureRecord) -> HostLaunchPlan
    public static func terminal(_ choice: TerminalChoice, record: FeatureRecord) -> HostLaunchPlan          // sbx → disabled
    public static func agent(_ choice: AgentChoice, terminal: TerminalChoice, record: FeatureRecord, passPrompt: Bool) -> HostLaunchPlan
    public static func devcontainerShell(_ status: DevcontainerStatus, terminal: TerminalChoice) -> HostLaunchPlan
}
```

### 4.10 Stores API: `BranchBoxStores/*` (skeleton by SW-0, implemented by SW-2)

```swift
public enum BackendState: Sendable, Hashable { case resolving, ready(BackendIdentity), unavailable(BackendError) }
public enum LoadState: Sendable, Hashable { case idle, loading, loaded(Date), failed(BackendError, lastGood: Date?) }
public enum RefreshReason: Sendable, Hashable { case initial, registryChanged, appActivated, timer, afterOperation, manual, settingsChanged }

@MainActor @Observable public final class AppModel {
    public let settings: AppSettings
    public let environment: EnvironmentStore
    public let projects: ProjectsStore
    public let operations: OperationStore
    public let actions: ActionDispatcher
    public let notifier: any Notifier
    public private(set) var pendingIntent: WindowIntent?
    public private(set) var intentToken: Int                       // bumps on every post
    public init(settings: AppSettings, bootstrapper: any BackendBootstrapping, notifier: any Notifier)
    public func start() async                                     // bootstrap, migrate legacy defaults, load projects, start watchers/timers
    public func backend() throws -> any BranchBoxBackend          // throws the unavailable BackendError
    public func post(_ intent: WindowIntent)
    public func takePendingIntent() -> WindowIntent?
    public func appDidBecomeActive()
    public func prepareForTermination() async                     // cancelAll + terminateAllProcesses, bounded 10 s
}

@MainActor @Observable public final class EnvironmentStore {
    public private(set) var backendState: BackendState
    public private(set) var identity: BackendIdentity?
    public private(set) var summary: EnvironmentSummary?
    public private(set) var doctor: DoctorReport?
    public func rebootstrap() async                               // after Settings changes; no relaunch needed
    public func recaptureEnvironment() async
    public func runDoctor(for project: ProjectRef?) async
    public func supports(_ capability: Capability) -> Bool
}

public enum AddProjectOutcome: Sendable, Hashable {
    case added(ProjectRef, note: String?)                         // note e.g. "This is a feature worktree of …/main; added …/main"
    case alreadyPresent(ProjectRef)
    case needsInit(ProjectRef)
    case refused(BackendError)
}

@MainActor @Observable public final class ProjectsStore {
    public private(set) var projects: [ProjectStore]              // pinned first, then lastOpenedAt desc
    public func project(_ ref: ProjectRef) -> ProjectStore?
    public func add(folder: URL) async -> AddProjectOutcome
    public func remove(_ ref: ProjectRef)                         // never deletes files
    public func relocate(_ ref: ProjectRef, to folder: URL) async -> AddProjectOutcome
    public func setPinned(_ ref: ProjectRef, _ pinned: Bool)
    public func markOpened(_ ref: ProjectRef)
    public func refreshAll(_ reason: RefreshReason)
    public var attentionCount: Int { get }
}

@MainActor @Observable public final class ProjectStore: Identifiable {
    public let ref: ProjectRef
    public nonisolated var id: String { ref.path }
    public private(set) var features: [FeatureRecord]             // attention first, then updatedAt desc
    public private(set) var strays: [StrayWorktree]
    public private(set) var droppedRecords: Int
    public private(set) var listWarnings: [String]
    public private(set) var loadState: LoadState                  // failed keeps last good features
    public private(set) var rootExists: Bool                      // false → grey row with Locate…/Remove
    public var includeRemoved: Bool                               // toggling triggers a refresh with --all
    public private(set) var config: ProjectConfigDocument?
    public private(set) var detect: DetectReport?
    public func feature(named name: String) -> FeatureRecord?
    public func folderExists(for record: FeatureRecord) -> Bool
    public func requestRefresh(_ reason: RefreshReason)           // non-blocking, coalesced
    public func refresh(_ reason: RefreshReason) async            // single-flight + one coalesced re-run + generation guard
    public func reloadConfig() async
    public func reloadDetect() async
    public var attention: [AttentionItem] { get }
}

public enum OperationKind: String, Sendable, Hashable, Codable {
    case start, teardown, prune, exec, devcontainerUp, devcontainerDown, devcontainerRebuild, devcontainerBuild
    case syncDevcontainers, tunnelOpen, tunnelRemove, initProject, applyConfig, tunnelCredentials, deleteBranch, removeStray
    public var writesRegistry: Bool { get }   // start, teardown, prune, tunnelOpen/Remove, syncDevcontainers, initProject, applyConfig, tunnelCredentials
    public var isMutating: Bool { get }       // all but exec
    public var isProjectWide: Bool { get }    // prune, syncDevcontainers, initProject, applyConfig, tunnelCredentials
}
public enum OperationTarget: Sendable, Hashable { case feature(FeatureRef), project(ProjectRef), global }
public enum OperationState: Sendable, Hashable { case queued(behind: String), running, succeeded, succeededWithWarnings, partial, failed(BackendError), cancelled(note: String?) }
public struct StepProgress: Sendable, Hashable { public let completed: Int; public let total: Int }
public enum PruneRowOutcome: Sendable, Hashable { case removed(TeardownOutcome), refused(BackendError), failed(BackendError), skipped(String), cancelled }
public struct PruneRow: Sendable, Hashable { public let feature: String; public let outcome: PruneRowOutcome }
public struct PruneResult: Sendable, Hashable { public let rows: [PruneRow] }
public enum OperationResult: Sendable, Hashable {
    case start(StartSummary), teardown(TeardownOutcome), prune(PruneResult), exec(ExecResult), devcontainer(DevcontainerResult)
    case sync(SyncReport), tunnel(TunnelChange), initProject(InitReport), config(ConfigApplyResult)
    case credentials(TunnelCredentialsResult), message(String)
}

@MainActor @Observable public final class LogBuffer {
    public private(set) var lines: [LogLine]                      // ring buffer ≤ 10_000
    public private(set) var revision: Int                         // bumped ≤ 10 Hz
    public private(set) var archiveURL: URL?                      // full log on disk (LogArchive)
}

@MainActor @Observable public final class OperationRecord: Identifiable {
    public let id: UUID
    public let kind: OperationKind
    public let target: OperationTarget
    public let title: String                                      // "Starting oauth"
    public let context: OperationRequestContext
    public let startedAt: Date
    public private(set) var state: OperationState
    public private(set) var result: OperationResult?
    public private(set) var phase: OperationPhase?
    public private(set) var stepProgress: StepProgress?
    public private(set) var warnings: [String]
    public let log: LogBuffer
    public private(set) var finishedAt: Date?
    public private(set) var acknowledged: Bool                     // failed/partial count toward menu-bar attention until viewed
    public var isCancellable: Bool { get }
    public func acknowledge()
}

public enum Admission: Sendable, Hashable { case allowed, queued(behind: String), rejected(reason: String) }

@MainActor @Observable public final class OperationStore {
    public private(set) var records: [OperationRecord]            // newest first; history cap 100 (persisted summaries)
    public var running: [OperationRecord] { get }
    public func records(for target: OperationTarget) -> [OperationRecord]
    public func active(for target: OperationTarget) -> OperationRecord?
    public func admission(for kind: OperationKind, target: OperationTarget) -> Admission
    public func cancel(_ id: OperationRecord.ID)                  // UI confirms first; see D-18
    public func cancelAll() async
}

public enum DispatchResult { case started(OperationRecord), queued(OperationRecord, behind: String), rejected(reason: String), unavailable(BackendError) }

@MainActor public final class ActionDispatcher {
    /// The ONLY way views start operations. Builds the OperationRecord, applies admission, streams progress
    /// through one ordered AsyncStream consumer, refreshes the project after success/failure/cancel, notifies.
    @discardableResult public func dispatch(_ request: OperationRequestContext) -> DispatchResult
    @discardableResult public func perform(_ recovery: RecoveryAction) -> DispatchResult?   // .retry only; nil for UI-only recoveries
    public func title(for request: OperationRequestContext) -> String
}

public protocol Notifier: Sendable {
    var isAvailable: Bool { get }                                 // false unless bundle id != nil && bundleURL.pathExtension == "app"
    func requestAuthorizationIfNeeded() async -> Bool
    func post(_ note: UserNote) async
}
public struct UserNote: Sendable, Hashable { public let title: String; public let body: String; public let intent: WindowIntent?; public let threadID: String }
public struct NoopNotifier: Notifier { public init() }

@MainActor @Observable public final class AppSettings {
    public static func defaultsForCurrentProcess() -> UserDefaults  // .standard if bundled .app, else UserDefaults(suiteName: "dev.branchbox.app.dev")!
    public init(defaults: UserDefaults)
    public var cliPathOverride: String?
    public var extraEnvironment: [String: String]
    public var verboseLogs: Bool
    public var preferredEditor: EditorChoice
    public var editorOpenMode: EditorOpenMode
    public var preferredTerminal: TerminalChoice
    public var agentChoice: AgentChoice
    public var agentDisplayName: String?
    public var passPromptToAgent: Bool
    public var notificationsEnabled: Bool
    public var notifyOnlyOnProblems: Bool
    public var notifyAttentionChanges: Bool
    public var watchProjectFiles: Bool
    public var selectedProjectRefresh: RefreshInterval            // default .m1
    public var otherProjectsRefresh: RefreshInterval              // default .m5
    public var showMenuBarIcon: Bool
    public var logRetention: Int                                  // default 100
    public var quickCommands: [String: [String]]                  // project path → commands
    public var promptHistory: [String]                            // last 10
    public var backendSettings: BackendSettings { get }
}
```

`Navigation.swift` (Stores):

```swift
public enum SidebarSelection: Hashable, Codable, Sendable {
    case welcome
    case project(path: String)
    case feature(projectPath: String, name: String)
    case stray(projectPath: String, path: String)
}
public enum InitSheetMode: String, Hashable, Codable, Sendable { case setUp, repair }
public enum WindowIntent: Hashable, Sendable {
    case select(SidebarSelection)
    case startFeature(project: ProjectRef?, prefill: StartFeatureRequest?)
    case teardown(FeatureRef, preselect: BranchPolicy?)
    case prune(ProjectRef)
    case stray(ProjectRef, StrayWorktree)
    case addProject(URL?)
    case initProject(URL, mode: InitSheetMode)
    case projectSettings(ProjectRef)
    case syncDevcontainers(ProjectRef)
    case showActivity(operation: UUID?)
    case showDiagnostics
    case quickOpen
}
```

### 4.11 App navigation contract and screen stubs (SW-0 creates; wave-3 owners fill)

Views read stores via `@Environment(AppModel.self)`. Views never present a sheet from inside a sheet and never own a router. To chain flows, they `model.post(_:)` an intent and dismiss. The window's router presents the next sheet after `onDismiss`.

```swift
// Navigation/SceneID.swift (SW-0 → SW-4)
enum SceneID { static let main = "main", run = "run", activity = "activity", diagnostics = "diagnostics" }

// Navigation/SheetRoute.swift (SW-0 → SW-4). Exactly one presented per main window.
enum SheetRoute: Identifiable, Hashable {
    case startFeature(project: ProjectRef?, prefill: StartFeatureRequest?)
    case teardown(FeatureRef, preselect: BranchPolicy?)
    case prune(ProjectRef)
    case stray(ProjectRef, StrayWorktree)
    case addProject(URL?)
    case initProject(URL, mode: InitSheetMode)
    case projectSettings(ProjectRef)
    case syncDevcontainers(ProjectRef)
    case quickOpen
    var id: String { … }
}

// Stub initializers (final names; each stub renders Text("…") until filled):
struct FeatureDetailView: View { init(feature: FeatureRef) }                       // SW-5 Feature/FeatureDetailView.swift
struct FeatureActionsMenu: View { init(feature: FeatureRef, style: Style); enum Style { case contextMenu, menuBar, toolbarOverflow } } // SW-5
struct RunCommandWindow: View { init(feature: FeatureRef?) }                       // SW-5 RunCommand/RunCommandWindow.swift
struct StartFeatureSheet: View { init(project: ProjectRef?, prefill: StartFeatureRequest?) }   // SW-6 Flows/
struct TeardownSheet: View { init(feature: FeatureRef, preselect: BranchPolicy?) }   // SW-6
struct PruneSheet: View { init(project: ProjectRef) }                               // SW-6
struct StrayWorktreeSheet: View { init(project: ProjectRef, stray: StrayWorktree) }  // SW-6
struct ActivityInspector: View { init(target: OperationTarget?) }                   // SW-6 Activity/
struct ActivityWindow: View { init() }                                              // SW-6
struct WelcomeView: View { init() }                                                 // SW-7 Projects/
struct ProjectDetailView: View { init(project: ProjectRef) }                        // SW-7
struct AddProjectSheet: View { init(initialFolder: URL?) }                          // SW-7
struct InitProjectSheet: View { init(folder: URL, mode: InitSheetMode) }            // SW-7
struct SyncDevcontainersSheet: View { init(project: ProjectRef) }                   // SW-7
struct ProjectSettingsSheet: View { init(project: ProjectRef) }                     // SW-7
struct AppSettingsView: View { init() }                                             // SW-7 Settings/
struct DiagnosticsWindow: View { init() }                                           // SW-7 Diagnostics/
```

---

## 5. CLI JSON contract (verbatim)

### 5.1 Existing 0.13.4 shapes (decoded unchanged; fixtures in `cli-0.13.4/`)

| Command | Shape |
|---|---|
| `feature list --json` | Array of records: `work_feature`, `branch_name`, `worktree_path`, `base_branch`, `feature_url` (scheme-less), `compose_project_name`, `env_path`, `status` (`active\|degraded\|failed_retained\|orphaned\|removed`), RFC 3339 dates (`created_at`, `updated_at`, `removed_at`, `last_sync_at`), nested `tunnel{provider,hostname,service_url,status: pending\|active\|manual\|disabled,instructions[],notes,last_updated}`, `color`, `pr_number`, `last_commit`, `devcontainer_outdated`, `sync_strategy`, `start_mode`, `prompt_seed`, `module_outcomes[{module,status: success\|skipped\|failed,duration_ms,notes[],forced,recorded_at}]`, `adapter{name,service_url,warnings[]}`, `runtime{provider: container\|sbx\|local-vm\|in-guest,runtime_id,published_ports[{host,runtime}],container_id,workspace_folder,container_user,config_path}`, `default_agent{status,label,command,detail,followup}`. |
| `feature start --json` | StartSummary (§4.8). May carry a non-JSON preamble line on 0.13.x (BUG-11). |
| `feature teardown --json` | `{work_feature,branch_name,worktree_removed,branch_deleted,adapter_cleanup_warnings[],module_reports[{name,teardown_ok,errors[]}],runtime_teardown{provider,runtime_id,verified,residue_free,residue[{kind,identifiers[]}]},warnings[]}` |
| `feature exec --json` | `{exit_code,stdout,stderr}`; process exits 1 when the inner command fails, payload still printed. |
| `tunnel open --json` | `{work_feature,state,warnings}` |
| `tunnel remove --json` | `{work_feature,previous_state,updated_state,warnings}` |
| `devcontainer up --json` | camelCase `{outcome,containerId,remoteUser,remoteWorkspaceFolder,composeProjectName}` |
| `devcontainer down --json` | camelCase `{outcome,removedContainers}` |
| `devcontainer build --json` | camelCase `{outcome,imageName}` |
| `devcontainer exec --json` | camelCase `{outcome,exitCode,stdout,stderr}` |
| `devcontainer up/down/build --json` with Docker unavailable | `{"outcome":"error","message":"Docker is not available"}` on **stdout**, exit 1, nothing on stderr. Decode stdout on failure. |
| `devcontainer detect -p W --json` | `{service_name,port,service_url,container_user,home_path}` |

Tunnel status vocabulary is the JSON values. The app displays pending→"Starting", active→"Online", manual→"Manual setup needed", disabled→"Off". It never uses the CLI's text-mode "degraded".

### 5.2 Machine-mode rules (0.14+, capability `json-error-envelope`)

1. Any `--json` flag sets machine mode right after clap parsing. In machine mode:
   - stdout carries exactly one JSON document: the success payload or the error envelope.
   - Human text goes to stderr.
   - There are no prompts (`output::is_interactive() == false`).
   - A prompt-gated behaviour takes its documented non-interactive outcome, usually a cause-naming refusal.
2. Error envelope, on stdout and only in `--json` mode:
   ```json
   {"schema_version":1,"error":{"code":"teardown_refused","message":"Refusing to tear down 'eta'; nothing was removed. …","causes":["…"],"details":{ … }}}
   ```
   stderr keeps today's `Error: {err:?}` text, byte-compatible.
3. Exit codes are unchanged:
   - 0: success, including documented partial success
   - 1: failure or refusal
   - 2: clap usage
   - 75: dispatch-tool not-pending
   - 101: panic, which also emits an `internal_panic` envelope in machine mode

   Clients branch on `error.code`.
4. In-band failure payloads keep their shape and exit code, with no envelope added: `feature exec --json`, `devcontainer up/down/build/exec --json`, `feature dispatch-tool`, `doctor --json` (required check failed), `devcontainer sync --json` (a worktree failed) and `prune --yes --json` (a row failed). Each prints its full payload and then exits 1, and clients must decode stdout whenever it parses. Every other `process::exit` in `cli/` is replaced with an error return, so the envelope is printed.
5. Existing payloads only gain keys. New payloads carry `"schema_version": 1`.
6. Stable error codes:

| code | Raised by | details |
|---|---|---|
| `teardown_refused` | `Error::TeardownRefused` (and legacy `WorktreeDirty`) | `{plan, changed_anything, completed_steps[]}` |
| `worktree_not_found` | `Error::WorktreeNotFound` | `{name}` |
| `feature_not_found` | `CliError` (unregistered feature in tunnel/teardown paths) | `{name, registry}` |
| `invalid_feature_name` | `Error::InvalidFeatureName` | `{name}` |
| `worktree_exists` | `Error::WorktreeExists` | `{path}` |
| `branch_exists` | `Error::BranchExists` | `{branch}` |
| `not_a_git_repository` | new `Error::NotAGitRepository(PathBuf)`; Display byte-identical to today's "Validation error: Not a git repository: …" | `{path}` |
| `validation_failed` | `Error::Validation` | null |
| `config_invalid` | `Error::Config` / config engine | `{key?, line?, column?, expected?}` |
| `config_unknown_key` | config engine | `{key}` |
| `registry_locked` | new `Error::RegistryLocked` | `{path, waited_secs}` |
| `confirmation_required` | `CliError` (prune without `--yes` in machine mode) | `{count}` |
| `devcontainer_source_missing` | `CliError` (sync without a main `.devcontainer`) | `{path}` |
| `agent_unreachable` | `CliError` | null |
| `git_failed`, `io_error`, `command_failed`, `module_failed`, `env_var_not_set`, `adapter_not_found` | the matching core variants | null |
| `internal`, `internal_panic` | everything else | null |

### 5.3 `branchbox version --json` (capability discovery; new subcommand)

```json
{"version":"0.13.4","contract_version":1,"capabilities":["json-error-envelope","registry-lock","write-ahead-start","teardown-plan","teardown-discard-changes","teardown-unmerged-preflight","prune-json","detect-json","devcontainer-sync-json","config","tunnel-credentials","doctor","init-json"]}
```
- A capability string is added **in the same PR that implements it**. The list is aggregated from per-module `CAPABILITIES` consts (§10.1).
- `contract_version` is bumped only on breaking changes.
- On a pre-0.14 CLI, `version` is an unknown subcommand: clap exits 2. The app then parses `branchbox --version` (`branchbox 0.13.4`) and uses the empty capability set.

### 5.4 `feature list --json` additions (write-ahead, D-14/D-15)

An optional per-record key `setup`, present only while a start has not completed:
```json
"setup": {"state":"in_progress","pid":48211,"started_at":"2026-10-01T22:50:29.222458Z"}
```
- The record's `status` stays `active`; no new status values.
- At list time, `state` is reported as `"interrupted"` when the pid is not alive (`kill(pid,0)` → ESRCH) or `started_at` is more than 24 h old.
- Additionally, `active` and `failed_retained` records whose `worktree_path` does not exist are reported as `"status":"orphaned"` (C10d).

### 5.5 `feature teardown --dry-run --json` (TeardownPlan; capability `teardown-plan`; always exit 0)

```json
{"schema_version":1,"work_feature":"eta","registered":true,"status":"active",
 "worktree":{"path":"/r/eta","exists":true,"locked":false,"lock_reason":null},
 "changes":{"status_available":true,"truncated":false,
   "user":[{"path":"README.md","kind":"modified","area":"other"},{"path":"notes.txt","kind":"untracked","area":"other"}],
   "generated":[{"path":".devcontainer/.branchbox.env","rule":"reserved_name"},{"path":".vscode/settings.json","rule":"vscode_managed_keys"}],
   "preserved":[{"path":"docs/features/in-progress/eta.md","destination":"docs/features/backlog/eta.md"}]},
 "branch":{"name":"feature/eta","source":"registry","exists":true,"upstream":null,"reference":"HEAD","reference_name":"main",
   "merged":false,"merged_into_head":false,"ahead":3,"action":"delete"},
 "defaults":{"delete_branch_by_default":true,"force_delete_unmerged_by_default":false},
 "runtime":{"provider":"container","runtime_id":null},"tunnel":{"status":"disabled"},
 "blockers":[{"kind":"uncommitted_changes","count":2,"message":"…","override":"--discard-changes"},
             {"kind":"unmerged_branch","branch":"feature/eta","ahead":3,"message":"…","override":"--keep-branch | --force-delete-branch"}],
 "warnings":[]}
```

| Field | Values |
|---|---|
| Blocker `kind` | `uncommitted_changes`, `unmerged_branch`, `worktree_locked`, `status_unavailable`, `worktree_removal_failed` |
| Generated `rule` | `reserved_name`, `devcontainer_baseline`, `derived_from_main`, `devcontainer_env_link`, `env_feature_block`, `vscode_managed_keys`, `vscode_managed_tasks` |
| `branch.source` | `explicit_prefix`, `registry`, `config_prefix` |
| `branch.action` | `keep`, `delete`, `force_delete` |
| Change `kind` | `untracked`, `modified`, `added`, `deleted`, `typechange`, `conflicted`, `staged` |
| Change `area` | `devcontainer`, `compose`, `vscode`, `spec`, `env`, `other` |

The dry run accepts the same policy flags as a real teardown.

### 5.6 `feature teardown --json` additions and the teardown refusal envelope

The summary gets these additive keys:
```json
"branch_action":"delete", "branch_delete_error":"error: cannot delete branch … used by worktree at …",
"discarded_changes":[{"path":"notes.txt","kind":"untracked","area":"other"}],
"preserved":[{"path":"docs/features/in-progress/eta.md","destination":"docs/features/backlog/eta.md"}],
"registry_updated":true
```
- Partial success means exit 0 with `branch_action != "keep"` and `branch_deleted == false`, with `branch_delete_error` set.
- A refusal is exit 1 plus an envelope with code `teardown_refused`, where `details = {"plan": <§5.5 plan>, "changed_anything": false, "completed_steps": []}`. It becomes `changed_anything: true` only if new user changes appeared during the run, after the runtime and modules were already stopped. Even then the worktree and the registry entry are kept.
- New flags: `--discard-changes` discards the worktree's uncommitted changes and does **not** imply `-D`. `--dry-run` prints the plan.

### 5.7 `detect --json` (capability `detect-json`)
```json
{"schema_version":1,"project":"/abs","git_repository":true,"initialized":true,"stack":"rust","adapter":"generic",
 "modules":["devcontainer","compose","specs","tunnel"],"has_devcontainer":true,"has_env":false,"warnings":[]}
```

### 5.8 `devcontainer sync --json [--feature NAME]...` (capability `devcontainer-sync-json`)
```json
{"schema_version":1,"dry_run":false,"strategy":"copy",
 "results":[{"work_feature":"eta","worktree_path":"/r/eta","status":"synced","files":["devcontainer.json"],"skip_reason":null,"error":null,"registry_updated":true}],
 "synced":1,"failed":0,"skipped":0}
```
- `status` is one of `synced`, `would_sync`, `skipped`, `failed`.
- **Any `failed` result exits 1**, in both text and JSON mode.
- A missing main `.devcontainer` gives the `devcontainer_source_missing` envelope.

### 5.9 `prune --dry-run --json` / `prune --yes --json [--feature N]...` (capability `prune-json`; CLI users only, the app never calls prune)
- Dry run: `{"schema_version":1,"dry_run":true,"policy":{"delete_branch":bool,"force_delete_branch":bool,"discard_changes":true,"complete_spec":bool},"candidates":[{"work_feature","status","branch_name","worktree_path","plan":{…§5.5…}}],"at_risk":{"uncommitted_changes":[…],"unmerged_commits":[…]}}`
- Execute: `{"schema_version":1,"dry_run":false,"results":[{"work_feature","outcome":"removed|failed","summary":{…}|null,"error":{"code","message"}|null}],"pruned":n,"failed":n}`, exit 1 if any row failed. In machine mode without `--yes`, it returns `confirmation_required`.

### 5.10 `config` (capability `config`)
- `config get [KEY] [--repo R] --json` → `{"schema_version":1,"path":"…/.branchbox/config.json","exists":true,"effective":{…full config with defaults…},"file":{…raw…},"keys":[{"key":"runtime.provider","type":"enum","allowed":["container","sbx","local-vm","in-guest"],"default":"container","value":"sbx","source":"file","description":"…"}]}`
- `config apply --file <PATH|-> [--dry-run] [--repo R] --json`: RFC 7386 merge patch on stdin, where `null` unsets. It returns `{"schema_version":1,"changed":[{"key","old","new"}],"effective":{…}}`. An unknown key gives `config_unknown_key`. An invalid value gives `config_invalid` naming the key and the expected values.
- `config set <KEY> <VALUE>` and `config unset <KEY>` are for humans and use the same engine.
- The registry of keys:
  - `runtime.provider`
  - `runtime.sbx.run_services`
  - `feature.branch_prefix`
  - `feature.teardown.{delete_branch_by_default, force_delete_unmerged_by_default, prompt_force_delete_unmerged}`
  - `tunnel.enabled`
  - `tunnel.default_provider`
  - `tunnel.providers.cloudflared.{account_id, tunnel_name_prefix, dns_zone, service_url, manual_instructions, api_token_path}`
  - `editor.{default_agent, auto_launch_agent_terminal, preferred_sidebar_view, hide_secondary_sidebar}`
- Writes are format-preserving (jsonc-parser CST), atomic, and done under the `.branchbox` lock. Unknown keys and file mode are preserved. Comments are refused.

### 5.11 `tunnel credentials set --account-id ID --api-token-stdin [--clear] [--repo R] --json` (capability `tunnel-credentials`)
- Output: `{"schema_version":1,"credentials_path":"…/.branchbox/secure/cloudflared.env","account_id":"…","token_present":true}`.
- The file is 0600 from creation and its directory is 0700. Other lines in the file are preserved. The command then sets `api_token_path` and `manual_instructions=false` through the config engine.
- An empty or whitespace-only token is refused, and the existing file is left untouched.
- The token never appears in argv, stdout, stderr or logs.

### 5.12 `doctor [--repo R] [--check-auth] --json` (capability `doctor`)
```json
{"schema_version":1,"cli":{"version":"…","contract_version":1,"path":"…"},"host":{"os":"macos","arch":"aarch64"},
 "checks":[{"id":"docker.daemon","title":"Docker daemon","required":true,"status":"error","path":"/usr/local/bin/docker","version":null,
            "detail":"timed out after 3s","remediation":"Start Docker Desktop"}],
 "summary":{"ok":7,"warn":1,"error":1}}
```
- Check ids:
  - `git`
  - `docker.cli`, `docker.daemon`, `docker.compose`
  - `devcontainer.cli`
  - `runtime.sbx`, `runtime.local_vm`
  - `op`, `gh`
  - `host.in_container`
  - `env.path_entries`
  - with `--repo`: `repo.git`, `repo.initialized`, `repo.config`, `repo.registry`, `repo.gitignore`
- Status is one of `ok`, `warn`, `error`, `skipped`.
- The command exits 1 if any required check is `error`. The JSON is printed either way.

### 5.13 `init --json` (capability `init-json`; implies non-interactive)
```json
{"schema_version":1,"workspace_path":"/r/main","repository_state":{"kind":"ready_to_initialize","warn_location":false},"reorganized":false,
 "stack":"rust","adapter":"generic","modules":["devcontainer","specs"],"devcontainer_status":{"kind":"created"},"registry_initialized":true,
 "onepassword":{"status":"configured|skipped|not_configured"},"warnings":[],"next_steps":[]}
```
- New flags: `--op-github-ref <op://…>`, `--op-signing-key-ref <op://…>`, `--skip-1password`, `--no-verify-op-refs`.
- The three `read_line` prompts (init.rs:747, 802, 2122) are gated on `output::is_interactive()`.

---

## 6. CLI backend behaviour (BranchBoxCLI, SW-1)

### 6.1 Modes
- **Contract mode** applies when `identity.contractVersion != nil`. Individual features are still gated on capabilities.
- **Legacy mode** is 0.13.x (≥ 0.13.4). The empty capability set selects legacy fallbacks per method.

### 6.2 argv table

Every spawn uses `cwd = project root` (unless noted), stdin `/dev/null` (unless noted), the child env from §7.3, and an absolute executable path. The app always passes `--repo/--path` explicitly and never relies on cwd for repo selection. Flags always come before `--`.

| Method | Contract mode | Legacy fallback | Timeout |
|---|---|---|---|
| identity | `version --json` | exit 2 or a decode failure leads to `--version` | 10 s |
| listFeatures | `feature list --json --repo R [--all]`, plus `git -C R worktree list --porcelain` for strays | same | 30 s |
| resolveProject | `git -C P rev-parse --path-format=absolute --git-common-dir --show-toplevel`; parent of the common dir is the main root. If P is not a git repo but `P/main/.git` exists, it is a parent container. Initialized means `<root>/.branchbox` exists. | same | 15 s |
| detect | `detect -p F --json` | `detect -p F` parsed as text, with `rawText` | 15 s |
| readConfig | `config get --repo R --json` | read `<R>/.branchbox/config.json` strictly with defaults; `editable=false`; no `keys` table (built-in descriptor list used read-only) | 15 s |
| applyConfig | `config apply --repo R --file - --json [--dry-run]`, with the patch on stdin | throws `.unsupported(.config, "0.14.0")` | 30 s |
| setTunnelCredentials | `tunnel credentials set --repo R --account-id ID --api-token-stdin --json`, with the token on stdin; or `--clear` | `.unsupported` | 30 s |
| initProject | cwd = folder: `init -y --json [-s S] [--skip-devcontainer] [--skip-env] [--no-coding-agents] [--reorganize] [--dry-run] [--update\|--validate] [--op-github-ref R [--op-signing-key-ref R] [--no-verify-op-refs] \| --skip-1password]`, then `config apply` for tunnelsEnabled and a re-resolve of `workspace_path` | the same without `--json`/`--op-*`, stdout streamed as log; re-resolve (`<folder>/main` if reorganized) | none |
| listBranches | `git -C R for-each-ref --format=%(refname:short) refs/heads refs/remotes` + `git -C R symbolic-ref --short -q HEAD` | same | 15 s |
| previewName | `name validate <in>` (exit 0 means valid as-is), else `name generate <in>` (empty output means invalid). branch = `<prefix>/<slug>`; path = `<parent(root)>/<slug>` | same | 5 s |
| planTeardown | `feature teardown <n> --repo R --dry-run --json [--branch-prefix P] <branch flags>` | app preflight (§6.5) synthesizes the same document (`source=.appPreflight`) | 30 s |
| startFeature | `feature start <slug> --repo R --json --runtime <p> [--base B] [--branch-prefix P] [--minimal [--default-prompt]] [--prompt T] [--skip-module M]… [--reuse [--devcontainer-reuse POL]] [--reuse-runtime] [--keep-runtime-on-failure] [--telemetry]`. Never `--title`, never `--no-summary`. | same; CLIJSON preamble tolerance | none |
| teardownFeature | §6.5 | §6.5 | none |
| exec | `feature exec --repo R --json <n> -- <cmd…>`, with the payload accepted on any exit code; target `.devcontainer` uses `devcontainer exec -w <worktree> --json -- <cmd…>` | same | `request.timeout` |
| devcontainer | cwd = worktree: `devcontainer up <W> --json [--remove-existing-container] [--build-no-cache]` / `down <W> --json [-v]` / `build <W> --json [--no-cache]`. The stdout payload is decoded even on exit 1; outcome `error` becomes `.commandFailed(message)`. | same | none |
| devcontainerStatus | `docker ps -a --filter label=devcontainer.local_folder=<W> --format '{{json .}}'` plus `devcontainer detect -p <W> --json` | same; missing docker gives `.unknown` | 10 s |
| syncDevcontainers | `devcontainer sync -p R [-s S] [-n] [--feature N]… --json` | text: stdout streamed; `✓ synced`, `would sync`, `✗ failed: …` and `N error(s) occurred` are parsed; any failure becomes failed **even with exit 0** (DRIFT-09) | none |
| openTunnel / removeTunnel | `tunnel open <n> --repo R --json` / `tunnel remove <n> --repo R --json [--force]` | same | 180 s |
| deleteBranch | `git -C R branch -d\|-D <b>` | same | 15 s |
| removeStray | `git -C <stray> status --porcelain=v1 -z --untracked-files=all` (refuses if dirty and not `discardChanges`), then `git -C R worktree remove [--force] <path>` | same | 30 s |
| doctor | `doctor [--repo R] --json`, merged with HostToolProbe | HostToolProbe only: `git --version`, `docker --version`, `docker info --format {{.ServerVersion}}`, `docker compose version`, sbx presence (`BRANCHBOX_SBX_PATH` or PATH) plus `sbx ls --quiet`, `devcontainer --version`, `op --version`, `gh --version`; 5 s each | 60 s total |

`--branch-prefix` is derived from `recordedBranch`: strip `/<name>` from the end. When the branch equals the name, pass `""`. Omit it when it can't be derived.

### 6.3 Output and exit algorithm (one private `invoke`)

1. Map runner errors first:
   - `ProcessRunError.cancelled` becomes `.cancelled(note)`. For start, teardown, prune and init on CLIs without write-ahead and registry lock, the note is "may have left a partial worktree; see Needs attention".
   - `.timedOut` becomes `.timedOut`.
   - `.workingDirectoryMissing` becomes `.projectInvalid(.workingDirectoryMissing)`.
   - `.launchFailed` becomes `.launchFailed`.
   - `.stdoutTooLarge` becomes `.commandFailed`.
2. On `exited(0)`, decode with `CLIJSON.decode`; a preamble becomes `.warning` plus `StartSummary.preambleWarning`.
3. On a non-zero exit, try `ErrorEnvelope` from stdout first. Map `code` to `RefusalCause` (§6.4); for `teardown_refused`, take `details.plan` as `Refusal.plan` and choose the cause from its first blocker.
4. For non-zero commands that report failure in-band (exec, devcontainer, doctor, sync; §5.2 rule 4), return or interpret the payload. The envelope is tried first, and an in-band payload never has an `error` top-level key.
5. Otherwise use the stderr anyhow block (last `Error:` line plus `Caused by:`) and the **0.13.4 known-message table**:

| Message | Becomes |
|---|---|
| "Devcontainer/compose changes detected; rerun …" | `.moduleFilesDirty(files: <stdout "    • path" lines>, userChanges: <re-preflight>)` |
| "Branch 'X' could not be deleted without force" | `.partial(completed: ["Worktree removed"], remaining: .unmergedBranch(X))` (defensive; legacy mode never sends `--delete-branch`) |
| "Not a git repository: P" | `.notGitRepository(P)` |
| "Worktree already exists at: P" | `.worktreeExists(P)` |
| "Worktree not found: X" | `.worktreeNotFound(X)` |
| "Invalid feature name" | `.invalidName` |
| "Branch already exists" | `.branchExists` |
| "Refusing to prune in non-interactive mode" | `.confirmationRequired` |
| "Sign in with: sbx login", "local-vm requires a Linux host" | `.runtimePrerequisite` |
| "Devcontainer directory not found" | `.devcontainerSourceMissing` |
| message mentions `registry.json` together with a JSON/parse/EOF error | `.registryCorrupted(path)` |

6. Anything else becomes `.commandFailed(Diagnostics)`, whose `summary` is the CLI's `Error:` line and **never** the INFO log.
7. A signal termination becomes `.commandFailed("terminated by signal N")`.

Additional rules:
- **Phase mapping** from tracing targets in stderr:

| Target | Phase |
|---|---|
| `worktree_core::git` "Created worktree" | creatingWorktree |
| `…modules::<m>` | module(m) |
| `…runtime` | runtime |
| `…adapters` | detectingAdapter |
| `…tunnel` | provisioningTunnel |
| teardown "Removed worktree" | removingWorktree |
| runtime destroy lines | cleaningRuntime |

- **Line format.** Lines parse as `^(\S+)\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+([\w:]+):\s(.*)$`; anything else is `.output`.

### 6.4 Envelope code → RefusalCause

| Code | RefusalCause |
|---|---|
| `teardown_refused` | from plan blockers: `uncommitted_changes`→`.uncommittedChanges(plan.changes.user)`, `unmerged_branch`→`.unmergedBranch`, `worktree_locked`→`.worktreeLocked`, `status_unavailable`→`.statusUnavailable`, `worktree_removal_failed`→`.worktreeRemovalFailed` |
| `worktree_not_found` | `.worktreeNotFound` |
| `feature_not_found` | `.featureNotFound` |
| `invalid_feature_name` | `.invalidName` |
| `worktree_exists` | `.worktreeExists` |
| `branch_exists` | `.branchExists` |
| `not_a_git_repository` | `.notGitRepository` |
| `registry_locked` | `.registryLocked` |
| `confirmation_required` | `.confirmationRequired` |
| `config_invalid` / `config_unknown_key` | `.configInvalid` |
| `devcontainer_source_missing` | `.devcontainerSourceMissing` |
| `validation_failed` | runtime-prerequisite message table, else `.other("validation_failed")` |
| `git_failed`, `io_error`, `command_failed`, `internal*` | `.commandFailed` (not a refusal) |

### 6.5 Teardown algorithm (the safety core)

**planTeardown.**
- In contract mode with `teardown-plan`, run the CLI dry run.
- Otherwise use the **app preflight**:
  - `git -C W status --porcelain=v1 -z --untracked-files=all --no-renames --ignore-submodules=none`, classified by `WorktreeChangeClassifier` (rules below).
  - Branch existence: `git -C R show-ref --verify --quiet refs/heads/<recordedBranch>`.
  - Merge state with `git branch -d` semantics: the reference is `<branch>@{upstream}` if configured and it resolves, otherwise `HEAD` of the main worktree. Then `merged = merge-base --is-ancestor <branch> <ref>` and `ahead = rev-list --count <ref>..<branch>`. Never parse `git branch --merged`; it has the `+` worktree-marker bug.
  - Worktree existence and lock (`git worktree list --porcelain` `locked`).
  - Blockers synthesized exactly as §5.5 (with `override` strings adapted to the app's recoveries).

**WorktreeChangeClassifier** (Swift port of core S4, used in legacy mode only). Precedence, applied per status entry:
- **R1 reserved names:** `.devcontainer/.branchbox.env`, `.devcontainer/.cloudflared.env`, `.devcontainer/.devcontainer.json`, `.devcontainer/.branchbox-sbx-compose.yaml` → generated.
- **R2 spec:** this feature's spec (`docs/features/{in-progress,backlog,completed}/<name>.md`, including the ` D` of a promoted backlog file) → preserved. Core moves it to main before removal.
- **R3 devcontainer baseline:** a path under `.devcontainer/` whose FNV-1a-64 `stable_content_hash` (core/src/modules/devcontainer.rs:74-78; port byte-exact) equals the entry in `<main>/.branchbox/devcontainer-sync/<name>.json` → generated.
- **R4 derived from main:** a regular file of at most 8 MiB that is byte-identical to `<main>/<path>`, or a symlink with the same target → generated. Symlinks are never followed; paths with `..` are rejected.
- **R5 devcontainer env link:** `.devcontainer/.env` that is a symlink to `../.env`, or equal to the worktree `.env` → generated.
- **R6 env feature block:** `.env` whose base (before "# Feature-specific configuration (managed by branchbox)") equals main's, and whose block holds only the managed comment lines, blanks and `KEY=` for KEY ∈ {WORK_FEATURE, APP_URL, COMPOSE_PROJECT_NAME, DEVCONTAINER_NAME, GIT_BRANCH, DATABASE_NAME} → generated.
- **R7 VS Code:** `.vscode/settings.json` equal to `git show HEAD:.vscode/settings.json` (or `{}` when untracked) after removing `peacock.color`, `peacock.remoteColor`, `window.title` and `workbench.colorCustomizations` → generated. `.vscode/tasks.json` whose tasks are exactly `["Open Feature URL"]` → generated.
- **Otherwise** it is a **user change**, with `kind` and `area`.
- More than 2000 entries: the rest count as user changes and `truncated=true`.

**teardownFeature(request)**:
1. Run `plan = planTeardown(request)` again. This is the re-validation immediately before spawning.
2. `request.discard == nil` **and** the plan has user changes → throw `.refused(.uncommittedChanges(files), plan)`. In legacy mode this is an **in-app refusal; nothing is spawned**. In contract mode the CLI would refuse identically; the app refuses first to save a spawn.
3. `request.discard != nil` → every current user path must be in `discard.userFiles`. Otherwise throw `.refused(.uncommittedChanges(newFiles))` (race protection).
4. `branch == .deleteIfMerged` and the branch exists unmerged → throw `.refused(.unmergedBranch)`. The sheet blocks this case, so this is defensive.
5. **Contract mode:** `feature teardown <n> --repo R --json --branch-prefix P <flags>`.
   - Branch flags: keep → `--keep-branch`; deleteIfMerged → `--delete-branch`; forceDelete → `--delete-branch --force-delete-branch`.
   - Plus `--discard-changes` iff `discard != nil`, and `--complete-spec`.
   - `forceRemoval` → `--force --keep-branch`, followed by the app branch step.
   - Partial branch failure is mapped from `branch_delete_error` to `BranchOutcome.deleteFailed`.
6. **Legacy mode:** `feature teardown <n> --repo R --json --branch-prefix P --keep-branch [--force] [--complete-spec]`.
   - `--force` is sent iff `discard != nil || forceRemoval`.
   - The first attempt never carries `--force`. A `moduleFilesDirty` refusal re-preflights, and recoveries offer "Discard BranchBox-generated files and tear down" only when the re-preflight shows zero user changes.
   - Then the **app branch step**: `.deleteIfMerged` → `git branch -d <recordedBranch>` if it exists; `.forceDelete` → `git branch -D`; `.keep` → nothing. A failure becomes `BranchOutcome.deleteFailed` (not an error).
7. Verify `worktreeGone` on disk.

**forceRemoval guard (both modes).** `forceRemoval` is honoured only when one of these holds: `plan.worktree.exists == false`, a `worktree_locked` or `status_unavailable` blocker is present, or the user changes are covered by `discard`. Otherwise the backend refuses with `.uncommittedChanges`. `forceRemoval` therefore never bypasses consent for user files.
8. Return `TeardownOutcome`. If the summary warnings contain "removed manually after git removal failed", add the warning and surface it in red. That is the old data-loss path and must not occur behind the preflight.

### 6.6 Strays (`StrayDetector`, both modes)

Take the entries of `git worktree list --porcelain` that meet all of these:
- the path is not the main root and the entry is not bare;
- the canonical path is not any registry record's `worktree_path`;
- the **BranchBox layout** holds: `parent(path) == parent(main root)`;
- `branch == "<p>/<basename(path)>"` for p ∈ {config `feature.branch_prefix` (default "feature")} ∪ {prefixes derived from registry records}, or `branch == basename(path)` when the prefix is empty.

These are labelled **"Unregistered worktree"**. Removal refuses dirty strays unless the user explicitly discards.

### 6.7 Process-level redaction
- `previewCommandLine` and `Diagnostics.invocation` redact `--prompt` values (`--prompt '<redacted 812 chars>'`), extra-env values and tokens.
- Tokens only ever travel on stdin.

---

## 7. Process runner and environment (SW-1)

### 7.1 `ProcessRunner` (actor, conforms to `ProcessRunning`)

Port from `$SP/design/arch-clean-slate/spike/Sources/SpikeKit/ProcessRunner.swift`, with the Incremental and Contract-first fixes applied.

- **Before spawning:**
  - If `Task.isCancelled`, throw `.cancelled` without spawning. This is the judge-reproduced early-cancel bug: the Incremental spike ran `sleep 3` to completion and reported cancelled.
  - Validate that the working directory exists.
- **Spawn:**
  - Foundation `Process` with an absolute `executableURL`.
  - `standardInput` is `FileHandle.nullDevice`, or a pipe that receives `spec.standardInput` and is then closed.
  - Separate stdout and stderr pipes.
- **Drain:**
  - `readabilityHandler` on both pipes feeds `AsyncStream<Data>` (via `PipeReader`), registered **before** `run()`.
  - Collection runs in a `Task.detached` collector awaited under `withTaskCancellationHandler`, because iterating an AsyncStream ends when its consumer is cancelled.
  - Termination is reported through `terminationHandler`; `waitUntilExit` is never used.
  - The run finishes when the child has exited **and** both pipes reached EOF, or when `drainGrace` expires after the leader exits (grandchild pipe holders).
- **Escalation** (`Escalator`, a lock-guarded final class):
  - `kill(-pgid, SIGINT)`, then after `interruptGrace` SIGTERM, then after `terminateGrace` SIGKILL.
  - Foundation makes the child its own process-group leader (verified).
  - The group keeps being signalled after the leader exits while members remain.
  - `cancelled` and `timedOut` are thrown only after the group is gone (bounded).
  - After a successful run, leftover group members are **not** killed; a warning is logged instead.
  - The escalator records the pid atomically at publish time. A cancel that arrives during launch signals as soon as the pid is published. This fixes the race where `escalating && pid == 0` returned early.
- **Lines:** `LineSplitter` splits on `\n`, `\r\n` and bare `\r`, is UTF-8 safe, and caps lines at 16 KiB. `ANSI.strip` removes CSI and OSC sequences. stderr lines are always streamed; stdout lines are streamed only when `streamStdout`.
- **Limits:** stdout is capped at 64 MiB (`.stdoutTooLarge`); the stderr tail keeps the last N lines.
- **Child registry:** `ChildRegistry` tracks live pgids, and `terminateAll()` escalates all of them within 10 s.

### 7.2 Login-shell environment and `EnvironmentProvider`
- **Capture command:** the shell comes from `getpwuid(getuid()).pw_shell`, run as `<shell> -l -i -c "printf '\n__BRANCHBOX_ENV_BEGIN__\n'; /usr/bin/env -0; printf '\n__BRANCHBOX_ENV_END__\n'"` with stdin `/dev/null`, `TERM=dumb` and an 8 s timeout.
- **Parsing:** NUL-separated output between the sentinels, so rc-file noise is ignored.
- **Fallbacks:** `-l -c` (5 s), then the process env plus well-known dirs. `-lc` misses nvm (verified).
- **Persistence:** only PATH is persisted (Application Support `environment-cache.json`). The full environment stays in memory.
- **Provisional environment:** `childEnvironment(for: .read)` returns immediately, with the provisional env (process env + cached PATH + well-known dirs) if the capture hasn't finished.
- **Mutations:** `.mutation` awaits the capture. Measured capture times are 1.0–1.7 s. Capture starts asynchronously at launch. `recapture()` serves Settings › "Re-capture".

### 7.3 `ChildEnvironment.make`
1. **Base:** the captured login env if available, otherwise the process env. HOME, USER, LOGNAME, TMPDIR and SSH_AUTH_SOCK are preserved from the process env if missing.
2. **Drop:** PWD, OLDPWD, SHLVL, `_`, TERM*, TERM_PROGRAM*, ITERM_*, COLORTERM, CLICOLOR_FORCE, PS1, PS2, PROMPT, XPC_*, `__CFBundleIdentifier`.
3. **PATH** = dedupe of: `[dir(cli)]` + base PATH + `/opt/homebrew/bin`, `/opt/homebrew/sbin`, `/usr/local/bin`, `~/.cargo/bin`, `~/.local/bin`, `~/.docker/bin`, `/Applications/Docker.app/Contents/Resources/bin` + `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`.
4. **Set:**
   - `NO_COLOR=1`, `CLICOLOR=0`, `TERM=dumb`
   - `GIT_TERMINAL_PROMPT=0`, `RUST_BACKTRACE=0`
   - `RUST_LOG=info`, or `debug` when verbose, unless the user set it
   - `BRANCHBOX_DEFAULT_AGENT_CMD` / `BRANCHBOX_DEFAULT_AGENT_NAME` from Settings
5. **User extra env is applied last** and its values are never logged.

### 7.4 `CLILocator` and `CLIProbe`
- **Resolution order (D-8).** PATH is searched in Swift and the result is never symlink-resolved. Every rejected candidate is recorded with its reason.
- **Probe:** `version --json`, falling back to `--version`. Below 0.13.4 → `.cliTooOld`. Results are cached in memory and in Application Support, keyed by (path, inode, mtime, size). The binary is re-checked on app activation so that `brew upgrade` is noticed.
- **Embedded CLI:** used only if `Contents/Helpers/branchbox` exists. The UI then labels it "bundled CLI vX".

---

## 8. State management (SW-2)

### 8.1 AppModel composition
`AppModel` owns:
- `AppSettings`
- `EnvironmentStore`, which bootstraps through `BackendBootstrapping`
- `ProjectsStore` (backed by `ProjectsRepository` → projects.json)
- `OperationStore`
- `ActionDispatcher`
- a `Notifier`
- `pendingIntent`

On `start()`, AppModel does the following:
1. Bootstraps.
2. Runs `LegacyDefaultsMigration`:
   - imports `branchbox.workspace` if it resolves, dropping the stale `/workspaces/milestone2`;
   - imports `branchbox.promptHistory`;
   - deletes `branchbox.transportPreference`, `branchbox.teardown.force`, `branchbox.teardown.deleteBranch`, `branchbox.teardown.completeSpec` and `branchbox.devcontainerStrategy`.
3. Validates every project, which may set `rootExists=false` → grey row with Locate…/Remove.
4. Starts watchers and timers.

### 8.2 Refresh (`ProjectStore.refresh`)
- **Single flight:** if a refresh is already in flight, it sets `rerun` and returns, and the active loop runs exactly once more.
- **Generation:** a generation counter drops stale results, for example when `includeRemoved` changes mid-flight.
- **Failure:** keeps the last good data, with `.failed(error, lastGood:)` and a stale banner ("Showing data from 2 min ago").
- **Limiter:** `ListLimiter` allows at most 2 concurrent `listFeatures` across projects.
- **Triggers:**
  1. `RegistryWatcher` FSEvents on `<root>/.branchbox` (`FileEvents|NoDefer`, 0.2 s latency, 300 ms debounce). It catches both in-place truncation and atomic rename. If `.branchbox` doesn't exist yet, there is no watcher; the init completion starts it.
  2. App activation, when the data is more than 5 s stale (it also re-checks the CLI binary).
  3. After every operation, whether it succeeded, failed or was cancelled.
  4. A timer for the selected project (`selectedProjectRefresh`, default 60 s) while the app is active.
  5. All projects (`otherProjectsRefresh`, default 300 s) **regardless of activation**, for menu-bar freshness.
  6. ⌘R.
- **No feedback loop:** `feature list` never writes the registry.

### 8.3 Operations
- **ActionDispatcher.dispatch(context):**
  - computes the target and kind, then `admission`;
  - creates an `OperationRecord`;
  - runs the work in a Task that captures the backend;
  - feeds the `ProgressSink` into an `AsyncStream` continuation consumed by **one** MainActor task (ordered), with log appends batched by `EventBatcher` every 100 ms (≤ 10 UI invalidations per second for docker logs);
  - streams the full log to `LogArchive` (`~/Library/Logs/BranchBox/operations/<ISO>-<kind>-<feature>-<id>.log`, retention N, redacted);
  - sets the state;
  - **always** calls `project.requestRefresh(.afterOperation)`;
  - posts notifications per §11.
- **Prune:** dispatch runs N teardowns sequentially inside one record. Each row reports `.phase(.item(i, of: n, name))` and records a `PruneRowOutcome`. A refusal skips the row and continues. Cancellation stops between rows; the current row is cancelled through the runner.
- **Admission (D-16):**
  - At most one mutating operation per feature; exec is always allowed.
  - Project-wide operations are exclusive with every mutating operation in the project.
  - Without `registry-lock`, registry writers (`OperationKind.writesRegistry`) in a project run FIFO through `MutationQueue`, and the waiting record shows `.queued(behind: "Starting alpha")`.
  - Reads are never blocked.

### 8.4 Selection by ID
- `SidebarSelection` is persisted per window as JSON in `@SceneStorage("selection")`.
- Detail views resolve `projectStore.feature(named:)` on every render. If it is nil, they show a `ContentUnavailableView("This feature no longer exists")` with [Show Removed Features].

### 8.5 Projects
- **Persistence:** `projects.json` holds `{"version":1,"projects":[{"root","displayName","addedAt","lastOpenedAt","pinned","collapsed"}]}` and is written atomically.
- **Adding:** goes through `backend.resolveProject`, which normalizes to the main worktree (`--git-common-dir`; the judge correction: `resolve_repo_root` uses `--show-toplevel`). A feature worktree adds main with the note "This is a feature worktree of …/main; adding …/main instead". A parent container adds `<dir>/main`. A non-git folder gets a cause-naming refusal.
- **Duplicates:** detected by canonical path.

### 8.6 Notifier
- `isAvailable` requires `Bundle.main.bundleIdentifier != nil && Bundle.main.bundleURL.pathExtension == "app"`. Checking the bundle id alone is not enough: xctest has one and still crashes.
- If unavailable, `NoopNotifier` is used.

### 8.7 Lifecycle (SW-4)
- `applicationShouldTerminateAfterLastWindowClosed` returns false.
- `applicationShouldHandleReopen` reopens the main window through `WindowOpener`, which captures `openWindow` from an always-alive view (`MenuBarLabel`). The fallback is `NSApp.windows.first { $0.identifier?.rawValue.hasPrefix("main") == true }?.makeKeyAndOrderFront(nil)`.
- `applicationShouldTerminate` returns `.terminateLater` when operations are running and shows "N operations are running. Quitting stops them and may leave partial state. [Cancel and Quit] [Keep Running]". Cancel and Quit calls `AppModel.prepareForTermination()` and then `reply(toApplicationShouldTerminate:)`.
- There is no `activate` at launch.

---

## 9. UX: screens (condensed from the UX spec, reconciled)

Global conventions:
- Status vocabulary: active (green dot), degraded (orange triangle), failed_retained "Failed (kept)" (red octagon), orphaned (purple diamond), removed (archivebox), unknown (raw value with underscores shown as spaces, grey).
- Derived attention: "Setup incomplete" (a failed module), "Folder missing", "Interrupted" (setup), "Unregistered worktree" (stray).
- Runtime glyphs: container `shippingbox`, sbx `lock.shield`, local-vm `cpu`, in-guest `square.dashed`.
- Module summary "3 ok · 1 skipped · 0 failed".
- URL rules per `FeatureURLs`.
- Never colour-only; semantic colours; one combined accessibility label per row; identifiers `sidebar.project.<id>`, `sidebar.feature.<p>.<n>`, `feature.action.<verb>`, `sheet.teardown.confirm`.
- State catalogue on every screen: loading (local spinner), refreshing (stale data visible), empty (`ContentUnavailableView` with one action), error (inline banner with cause plus Retry/Details/Copy), blocked (disabled with a reason in `.help`), partial (orange checklist), queued ("Waiting for …").
- **Success never shows a modal alert. Errors are inline.** Modal confirmations are reserved for destructive steps, inside the sheet.

**Scenes and menus (SW-4):**
- The main window is a two-column `NavigationSplitView`: sidebar plus detail, with an Activity `.inspector`. Default size 1100×720, minimum 800×520, sidebar 220–260.
- Menus:

| Menu | Command | Shortcut |
|---|---|---|
| File | Start Feature… | ⌘N (replaces `.newItem`) |
| File | Add Project… | ⌘O |
| View | Refresh | ⌘R |
| View | Toggle Sidebar | ⌃⌘S |
| View | Inspector | ⌥⌘I |
| View | Show Removed | ⇧⌘. |
| View | Quick Open | ⌘K |
| View | Find | ⌘F |
| Feature | Open in Editor | ⌃⌘E |
| Feature | Terminal | ⌃⌘T |
| Feature | Launch Agent | ⌃⌘A |
| Feature | Open URL | ⌃⌘O |
| Feature | Reveal | ⌃⌘R |
| Feature | Run Command | ⌥⌘R |
| Feature | Copy Path | ⌥⌘C |
| Feature | Copy Branch | ⇧⌥⌘C |
| Feature | Environment ▸ / Sharing ▸ | — |
| Feature | Tear Down… | ⌘⌫ |
| Project | Settings…, Prune…, Update All Workspaces…, Set Up…, Repair…, Remove… | — |
| Window | Activity | ⌥⌘L |
| Window | Diagnostics | — |

- Commands target the key main window via `@FocusedValue`. With no main window, they post an intent and open it.
- In sheets: Return is the default non-destructive action, Esc cancels or runs in background, ⌘. stops.

| Screen | Owner | Key behaviour |
|---|---|---|
| **Sidebar** | SW-4 | `List(selection:)`, one `DisclosureGroup` per project. **Project row:** name, attention badge, refresh spinner, stale icon; context menu Start/Settings/Prune/Update All Workspaces/Reveal/Terminal/Pin/Remove. **Feature rows:** swatch, name, status badge, runtime glyph, "Quick" capsule (minimal), op spinner; double-click opens the editor; context menu = `FeatureActionsMenu(.contextMenu)`. Sorted attention-first, then `updatedAt` desc. Strays appear as attention rows. Provisional "oauth: starting…" row while starting. Footer "Show removed (n)" toggles `includeRemoved`. `.searchable(placement: .sidebar)`. **States:** loading row, "No features yet" with a Start row, error row with Retry (cached rows stay, marked stale), CLI missing → `EnvironmentGate` in detail. |
| **EnvironmentGate** | SW-4 | `ContentUnavailableView` for: CLI not found (searched paths, [Locate…], [Copy `brew install branchbox/tap/branchbox`], [Re-detect]); CLI too old ([Copy `brew upgrade branchbox`]); unusable. The menu bar shows the same blocking line. A legacy CLI gets a dismissible banner: "BranchBox CLI 0.13.x: some features need a newer CLI (config editing, safe teardown in core, …)". |
| **Welcome / onboarding** | SW-7 | Three-step checklist in the detail area: (1) CLI path, source and version, with Locate/Install/Recheck. (2) Prerequisites doctor rows, required or optional, each with a remediation (Open Docker Desktop, Sign in to sbx → Terminal `sbx login`, Install CLT, Allow notifications requested in context). (3) Add your first project. Reachable again from Diagnostics. |
| **Add Project** | SW-7 | NSOpenPanel (directories) or folder drop → `projects.add` → outcome messaging. `needsInit` → card [Set Up BranchBox…] / [Add Anyway]. Missing folder later → grey row [Locate…] [Remove]. |
| **Set Up BranchBox (init) sheet** | SW-7 | Fields: stack (auto/rails/nodejs/rust/generic), devcontainer, .env, coding-agent mounts, **Layout "Keep where it is" (default) / "Move into parent folder"** (`--reorganize`, warns and previews the path), Tunnels toggle (default Off; 0.14: config apply after init), 1Password refs (0.14: `--op-*`; hidden on legacy), [Preview] (`--dry-run -y` log). Run → one operation; result shows stack/adapter/modules/warnings/next steps with [Start Your First Feature…]. Repair mode = `--update -y`; Check Setup = `--validate -y`. Init failure shows core's message verbatim. |
| **Project detail** | SW-7 | Header: path, Reveal/Terminal, "Updated n s ago", chips Stack/Adapter/Modules (detect; raw popover on legacy), default runtime and branch prefix. Status counts plus a sortable `Table` (Feature, Status, Runtime, Mode, Branch, Updated) that selects the sidebar row. Toolbar: Start, Prune, Update All Workspaces, Project Settings, More (Repair, Check Setup, Remove). Empty: "No features yet" [Start Feature…]. |
| **Update All Workspaces (sync) sheet** | SW-7 | Strategy, then **Preview** (`-n`), then Apply. Per-worktree results; failures shown even with exit 0. Copy: "Only active features are updated; each .devcontainer is replaced". No per-feature sync on legacy; `--feature` is only used on 0.14 from the feature detail's "Config out of date" link. |
| **Feature detail** | SW-5 | **Header:** swatch, name, status, runtime, Quick, "branch from base · created". **Toolbar:** Editor split button (VS Code/Cursor/Open in Dev Container), Terminal, Launch Agent, Open URL menu, Run Command…, overflow (Environment ▸, Sharing ▸, Copy ▸, Reveal, Tear Down…). **Health callout** (§9.1). **Cards (2 columns > 900 pt):** Overview (branch, base, folder with "Missing" state, last commit plus live subject via git log, created/updated, compose project, env path); Open (feature URL, tunnel, ports, container service "inside container, open if forwarded", in-container URL copy-only); Runtime; Environment (§9.2); Sharing (§9.3); Setup checklist (modules with notes, duration, forced); Coding agent and prompt (default_agent status; prompt seed with Show All/Copy/Launch with Prompt); Adapter warnings; PR (`#n` if present, else "No pull request linked"). Removed feature: read-only with a banner and [Delete Branch…] if the branch still exists. Missing: "This feature no longer exists". |
| **Feature actions** | SW-5 | Editor via NSWorkspace bundle id (`com.microsoft.VSCode`, `com.todesktop.230313mzl4w4u92`); Dev Container URI `vscode-remote://dev-container+<hex(worktree)><workspace_folder>` (container only); Terminal via a 0700 `.command` script in `~/Library/Caches/BranchBox/launch/` opened with Terminal (iTerm/custom template); Launch Agent (command = project `editor.default_agent` → Settings agent; optional prompt argument; shows `default_agent.status` hint); Open Shell in Dev Container (`docker exec -it -u <remoteUser> -w <remoteWorkspaceFolder> <containerId> bash -l`); Copy Branch/Path/Name; Reveal. sbx terminal and agent are **disabled** with "needs `branchbox feature exec --interactive` (planned)" and [Copy Command] (`sbx exec <runtime_id> bash`). Everything is disabled with a reason when the folder is missing. |
| **Run Command window** | SW-5 | `WindowGroup(id:"run", for: FeatureRef.self)`, title "Run in <feature>". Command field with per-feature history and per-project Quick Commands. "Run through shell" (default on → `/bin/sh -lc`). Target: Feature runtime / Dev container (container, running only). Run ⌘↩, Stop ⌘. Note "Output appears when the command finishes". Output/Errors segmented, exit-code badge, duration, Copy/Save, 32 MiB cap note. Non-zero exit is not an error alert. [Open in Terminal Instead]. |
| **Start Feature sheet** | SW-6 | Project picker when ambiguous. Title/slug field with a 250 ms debounced `previewName` ("Feature **oauth** · Branch **feature/oauth** · Folder …/oauth"), "Edit name" override. Collision checks: registry duplicate → error with [Show]; removed → info; folder exists → error unless Reuse; branch exists → info. Base picker (Current HEAD default; local/remote; searchable). Runtime radio (Container with Docker status; Docker Sandbox if sbx installed, with sign-in state; Local VM disabled "Linux with KVM only"; in-guest hidden; default from config). Full/Quick. Prompt editor with **hard cap 2000** and counter, Recent prompts, default-prompt toggle (Quick). Advanced (remembered per project): branch prefix, skip modules (tunnel locked when tunnels off), Reuse + devcontainer conflict policy, Keep sandbox on failure (sbx), Verbose. Footer: Copy as Command, Cancel, Start (queued label). Start turns the sheet into a progress view (Run in Background / Stop). **Result:** resolved `work_feature` (never the typed name), checklist, warnings plus adapter warnings, skipped modules, links; [Open in Editor] default; auto-launch agent if configured. A failure shows cause lines, [Edit and Retry] and [Show Feature] if a partial entry exists. |
| **Teardown sheet** | SW-6 | Runs `planTeardown`, shown with "Checking for unsaved work…". Sections: user changes ("will be permanently deleted" — the in-app refusal flow below), generated (safe), preserved spec destination, unmerged commits (count plus ≤ 5 subjects via `git log --oneline`), runtime resources, active tunnel, folder already gone. Branch radio from `TeardownDraft` (Keep / Delete if merged / Force-delete, the last only when unmerged). Complete spec only if the spec exists. Footer: Copy as Command, Cancel (default focus), Tear Down (destructive, no Return). **Flow:** Tear Down → dispatch(request without discard) → on `.refused(.uncommittedChanges)` the same sheet shows a refusal card listing the files with [Discard N changes and tear down…] (confirmationDialog naming the files) → `RecoveryAction.retry`. **Force-delete** needs a confirmationDialog listing the commits. **Result:** worktree removed (verified), branch kept/deleted/delete failed ([Force-delete Branch…] only if unmerged), runtime cleanup verified/residue list with [Copy Cleanup Commands]/couldn't verify, module reports, warnings. |
| **Prune sheet** | SW-6 | Rows = features with status ≠ removed. Per-row async preflight (concurrency 4). Columns: ☐, Feature, Status, Runtime, Your changes, Unmerged, Updated. Default selection is the **safe set**: no user changes and (merged or policy Keep), plus failed_retained/orphaned. [Select Safe] [All] [None]. Batch branch policy Keep (default) / Delete if merged (unmerged rows kept and labelled) / Force-delete (confirmation lists rows). Checking a dirty row opens a popover to confirm discarding its files (per-row DiscardConsent). Sequential execution with per-row states, [Stop After Current]. Result "5 torn down · 1 partial · 0 failed" with expandable rows. Empty: "Nothing to prune". |
| **Stray sheet** | SW-6 | Path, branch, dirty state; [Reveal] [Open in Terminal] [Remove Worktree…] (refuses dirty unless "Discard N changes" is confirmed); option to delete the branch if merged. |
| **Activity** | SW-6 | Inspector (selected feature or project ops), toolbar popover (running plus recent), Activity window ⌥⌘L (master-detail, filters). Progress view: title, elapsed, phase label, live log (level icons, timestamps toggle, auto-scroll that pauses on scroll-up with "Jump to Latest", warnings filter, Find, Copy Log, Reveal Log File), [Run in Background], [Stop] (confirmation per D-18), result views per kind, [Copy diagnostic report]. |
| **Project Settings sheet** | SW-7 | Data-driven from `config get` `keys` (bool → Toggle, enum → Picker, string → TextField, string_list → token field). Tabs: Features (branch prefix validated with `git check-ref-format --branch <p>/x`), Teardown, Runtime, Sharing (non-secret cloudflared fields plus an **API token SecureField** → `tunnel credentials set`), Coding Agent. Save → `applyConfig` (dry-run diff preview, then apply), inline key errors from `config_invalid`. Legacy: read-only with "Requires BranchBox CLI with config support" and [Open config.json]. A git-tracked `config.json` shows a note. |
| **App Settings** | SW-7 | General (menu bar icon, Dock icon via activation policy, attention count, open last project); Tools (CLI path/source/version, Locate/Reset/Recheck; login-shell PATH preview, Re-capture; extra env table "not for secrets"); Editors & Terminal; Coding Agent (claude/codex/custom, display name, pass prompt); Notifications; Refresh (intervals, watch files); Advanced (verbose, log retention, Reveal Logs, Reset Onboarding, "Backend: BranchBox CLI"). Changes rebuild the backend environment without a relaunch (`environment.rebootstrap()`). |
| **Diagnostics window** | SW-7 | CLI (path, source, version, minimum, contract version, capabilities, rejected candidates); environment (shell, source, duration, PATH copyable); tool table (doctor checks plus app checks for editors and terminals); runtimes; per-project health (initialized, config decode, registry readable, feature count, dropped records, last error); recent operations; [Run Checks Again] [Copy Report] (redacted Markdown) [Show Logs in Finder]. |
| **Menu bar (.menu)** | SW-4 | Label: `shippingbox` / with a dot while working / with an exclamation badge plus count on attention (statuses, derived attention, unacknowledged failed or partial ops, CLI blocked). Accessibility label such as "BranchBox, 1 feature needs attention, 1 operation running". Content: header summary, blocking CLI line (→ Diagnostics), last 3 operations, per-project sections (≤ 8 non-removed features, then "More in BranchBox…"), each feature a submenu = `FeatureActionsMenu(.menuBar)` (Open in Editor/Terminal/Agent, Open URL ▸, Copy, Reveal, remediation item, Tear Down…, Show in BranchBox), "Start Feature in <project>…", then Start Feature… ⌘N, Open BranchBox, Refresh ⌘R, Settings… (`SettingsLink`), Quit. Sheet-requiring items do `openWindow(id:"main")` + `NSApp.activate()` + `model.post(intent)`. "Updated n s ago" line. No `.sheet`/`.alert` in `MenuBar/` (CI grep). |
| **Quick Open ⌘K** | SW-4 | Overlay palette (not a sheet): features, projects, global commands; stable ids; ↑↓/Return; returns an intent performed after dismissal. |

### 9.1 Health remediation (`Remediation`, SW-3; rendered by SW-5 and the menu bar)

| Condition | Callout | Actions |
|---|---|---|
| `setup.state == interrupted` | "Setup of oauth was interrupted." | [Resume Setup] (`start --reuse`, runtime from the record), [Tear Down…] |
| `setup.state == in_progress` and pid alive | "Setting up…" | none (shows elapsed time) |
| degraded (sbx/local-vm/in-guest only; unreachable for container) | "The environment for oauth isn't running." | sbx: [Retry Setup] (`start --runtime sbx --reuse-runtime`; verified in VER-1 on a machine with sbx, else shipped as [Copy Command]); [Tear Down…] |
| failed_retained | "Setup failed; the sandbox was kept so you can inspect it." | [Retry] (`--runtime sbx --reuse-runtime`, mini prefill from the record), [Copy Inspect Command] (`sbx exec <id> bash`), [Discard…] (Teardown, Keep preselected) |
| orphaned with folder present | "The runtime no longer exists; your files are untouched." | [Recreate Runtime] (`start --reuse`), [Clean Up…] (Teardown, Keep) |
| orphaned with folder missing / derived "Folder missing" | "The folder … is gone." | [Clean Up] (`forceRemoval` + Keep; nothing to discard) |
| active with failed modules | "Setup finished with problems: compose failed (<note>)." | [Re-run Setup…] (`start --reuse --devcontainer-reuse preserve`), [Show Log] |
| devcontainer_outdated | info only | [Update All Workspaces…] |
| removed and branch still exists | banner | [Delete Branch…] (`-d`; `-D` after a confirmation listing commits) |
| unknown status | "Status <raw> isn't recognised by this app version." | [Run Doctor] |

### 9.2 Environment card (container runtime)
- **Status** comes from `devcontainerStatus`: Running / Stopped / Not created / Unknown.
- **Buttons:**
  - [Start]: `devcontainer up`.
  - [Stop]: `down`. "Also delete volumes" adds `-v` and needs a confirmation.
  - [Rebuild…]: confirmation, then `up --remove-existing-container --build-no-cache`.
- **Explanation line:** "`feature start` doesn't start the dev container on the container runtime."
- **sbx:** status from the list; Start Environment per §9.1; Stop and Rebuild are disabled ("requires `branchbox feature env`", deferred).
- **local-vm / in-guest:** read-only.

### 9.3 Sharing card
- **Fields:** provider, status label, hostname (https link + copy), service URL, notes, instructions (numbered when manual), last updated.
- **Tunnels off in config:** "Tunnels are off for this project" with [Project Settings…].
- **Disabled or none:** [Share via Tunnel] → `tunnel open`.
- **Pending or manual:** [Re-provision], [Show Instructions].
- **Active:** [Stop Sharing…] → confirmation → `tunnel remove`. A provider failure offers [Remove Anyway] (`--force`), which needs a second confirmation.

### 9.4 Error presentation (`BackendError+Presentation`, SW-3)

| Error | Where | Presentation |
|---|---|---|
| cliNotFound / cliTooOld / cliUnusable | EnvironmentGate | Blocking `ContentUnavailableView` |
| load failure | Inline row or banner | Cached data kept and marked stale |
| refused | Card in the sheet or operation row | Cause title (e.g. "Teardown stopped: uncommitted changes would be lost"), message, file list, recoveries from `RecoveryPlanner` |
| partial | Orange card | Completed steps plus the remaining refusal and its recoveries |
| commandFailed / decodeFailed / registryCorrupted | Red card | Summary, causes, [Show Log], [Copy diagnostic report] (app version and SHA, CLI path/source/version, capabilities, child PATH, redacted argv, exit/signal, last 50 stderr lines). registryCorrupted names the file and says "every feature in this project is hidden until it is repaired". |
| unsupported | Disabled control | `.help("Requires BranchBox CLI with <capability>")` |
| cancelled / timedOut | Grey card | The note is shown |

---

## 10. Core and CLI changes (Rust)

All changes keep backward-compatible deserializers. The agent crate compiles unchanged: `TeardownRequest` keeps its shape, and `teardown()` delegates to `teardown_with_options(req, TeardownOptions::legacy(&req))`. Text-mode output stays byte-identical except ANSI on non-TTY stderr, so the manual harness phrase "Detected devcontainer/compose changes" is preserved. Every refusal names its cause.

### 10.1 RS-1: Rust foundations (wave 1)

- **Output layer** (`core/src/output.rs`):
  - `set_machine_mode`, `machine_mode`;
  - `is_interactive() = !machine && stdin.is_terminal() && stdout.is_terminal()`;
  - `write_human` (stdout in text mode, stderr in machine mode; ignores EPIPE);
  - `emit_json<T: Serialize>` (pretty, one document, flush, sets `DOCUMENT_EMITTED`; `debug_assert` against a second document);
  - `document_emitted()`;
  - `#[macro_export] humanln!/human!` via `format_args!`.
- **Mechanical sweep** of every `println!`/`print!` in `cli/src/**` (~253) and `core/src/workflows/init.rs` (91) → `humanln!`/`human!`. The ~20 JSON sites become `output::emit_json`.
  - `feature.rs:453`: the prompt-truncation notice goes into `summary.warnings` plus `humanln!`.
  - Every `Term::stdout().is_term()` prompt gate and the three init `read_line`s are gated on `output::is_interactive()`.
  - `core/clippy.toml` and `cli/clippy.toml` get `disallowed-macros = [std::println, std::print]`, with `#![cfg_attr(test, allow(clippy::disallowed_macros))]` at crate roots and in `cli/tests/*`.
- **`cli/src/main.rs`:**
  - parse, then `set_machine_mode(cli.command.wants_json())`;
  - tracing to stderr with `.with_ansi(stderr.is_terminal() && NO_COLOR unset)`;
  - machine-mode panic hook (`internal_panic` envelope);
  - `run()` → `report_failure`: the envelope if no document was emitted, then the same `Error: {err:?}` on stderr; exit 1;
  - `name validate`'s `process::exit` (main.rs:157) becomes an error return;
  - the remaining `process::exit` sites are documented in-band failures (§5.2 rule 4).
- **`cli/src/json_error.rs`:** `CliError { code, message, details }` plus the anyhow → envelope mapping (`downcast_ref::<worktree_core::Error>()` → `code()`; `CliError`; otherwise `internal`).
- **`core/src/error.rs`:**
  - `TeardownRefused { work_feature, message, plan: Box<TeardownPlan> }`;
  - `RegistryLocked { path, waited_secs }`;
  - `NotAGitRepository(PathBuf)` (Display byte-identical; replaces the Validation at feature.rs:5255 and git.rs:31);
  - `pub fn code(&self) -> &'static str` per §5.2;
  - `WorktreeDirty` kept (doc legacy).
- **`core/src/workflows/teardown_plan.rs` (types only, the Rust contract):**
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)] pub struct TeardownPlan {
      pub schema_version: u32, pub work_feature: String, pub registered: bool, pub status: Option<FeatureStatus>,
      pub worktree: WorktreeState, pub changes: ChangeSet, pub branch: Option<BranchPlan>, pub defaults: TeardownDefaults,
      pub runtime: Option<RuntimeRef>, pub tunnel: Option<TunnelRef>, pub blockers: Vec<Blocker>, pub warnings: Vec<String> }
  pub struct WorktreeState { pub path: PathBuf, pub exists: bool, pub locked: bool, pub lock_reason: Option<String> }
  pub struct ChangeSet { pub status_available: bool, pub truncated: bool, pub user: Vec<UserChange>, pub generated: Vec<GeneratedChange>, pub preserved: Vec<PreservedFile> }
  pub struct UserChange { pub path: String, pub kind: ChangeKind, pub area: ChangeArea }
  #[serde(rename_all = "snake_case")] pub enum ChangeKind { Untracked, Modified, Added, Deleted, Typechange, Conflicted, Staged }
  #[serde(rename_all = "snake_case")] pub enum ChangeArea { Devcontainer, Compose, Vscode, Spec, Env, Other }
  pub struct GeneratedChange { pub path: String, pub rule: GeneratedRule }
  #[serde(rename_all = "snake_case")] pub enum GeneratedRule { ReservedName, DevcontainerBaseline, DerivedFromMain, DevcontainerEnvLink, EnvFeatureBlock, VscodeManagedKeys, VscodeManagedTasks }
  pub struct PreservedFile { pub path: String, pub destination: String }
  pub struct BranchPlan { pub name: String, pub source: BranchSource, pub exists: bool, pub upstream: Option<String>, pub reference: String,
      pub reference_name: String, pub merged: bool, pub merged_into_head: bool, pub ahead: u32, pub action: BranchAction }
  #[serde(rename_all = "snake_case")] pub enum BranchSource { ExplicitPrefix, Registry, ConfigPrefix }
  #[serde(rename_all = "snake_case")] pub enum BranchAction { Keep, Delete, ForceDelete }
  pub struct TeardownDefaults { pub delete_branch_by_default: bool, pub force_delete_unmerged_by_default: bool }
  pub struct RuntimeRef { pub provider: Option<String>, pub runtime_id: Option<String> }
  pub struct TunnelRef { pub status: Option<FeatureTunnelStatus> }
  #[serde(tag = "kind", rename_all = "snake_case")] pub enum Blocker {
      UncommittedChanges { count: usize, message: String, #[serde(rename = "override")] override_hint: String },
      UnmergedBranch { branch: String, ahead: u32, message: String, #[serde(rename = "override")] override_hint: String },
      WorktreeLocked { reason: Option<String>, message: String, #[serde(rename = "override")] override_hint: String },
      StatusUnavailable { cause: String, message: String, #[serde(rename = "override")] override_hint: String },
      WorktreeRemovalFailed { cause: String, message: String } }
  #[derive(Debug, Clone, Copy, Default)] pub struct TeardownOptions { pub discard_changes: bool, pub require_mergeable_branch: bool }
  impl TeardownOptions { pub fn legacy(r: &TeardownRequest) -> Self { Self { discard_changes: r.force_remove, require_mergeable_branch: false } } }
  pub const CAPABILITIES: &[&str] = &[];   // RS-2 sets ["teardown-plan","teardown-discard-changes","teardown-unmerged-preflight"]
  ```
- **Registry integrity (S2)** in `core/src/atomic_fs.rs`:
  - `lock_state_dir(dir, 30 s)` uses std `File::lock`/`try_lock` (Rust 1.89+; `rust-version = "1.89"` in `[workspace.package]`). On Unix the lock is the `.branchbox` directory fd (nothing shows up in git status); elsewhere `.branchbox/.lock`. A thread-local reentrancy guard prevents nested locks; a timeout gives `RegistryLocked`.
  - `write_atomic(path, bytes, new_mode)`: keep `ensure_not_symlink` (preflight requires it to stay in feature.rs; call it via `pub(crate)`), create a tempfile in the same dir, chmod to the existing mode or `new_mode`, write, fsync, persist (rename), fsync the dir, and sweep `.registry.*.tmp` files older than 1 h.
  - `FeatureStateStore::mutate(|reg| …)` wraps `record_start`, `record_teardown`, `update_feature` and `record_devcontainer_sync`; `save_registry` uses `write_atomic(…, 0o644)`; readers take no lock.
  - The devcontainer baseline write (modules/devcontainer.rs:153-163) also goes through `write_atomic`, and that module gains `pub read_baseline` / `pub record_baseline`.
  - `atomic_fs::CAPABILITIES = &["registry-lock"]`.
- **Write-ahead start (D-14):**
  - Add `setup: Option<SetupRecord{state, pid, started_at}>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`) to `FeatureMetadata`.
  - Right after the worktree is created in `start`, a provisional `record_start` writes `status: active` plus `setup.state = in_progress`. The final `record_start` (~981) and the failed_retained path (~895) clear `setup`.
  - Every error path after the provisional record either removes the entry (when it also removes the worktree) or leaves it with `setup`. `list_features` reports `interrupted` per §5.4.
  - Capability `write-ahead-start`.
- **List reconciliation (C10d):** before the provider checks, an Active/FailedRetained entry whose `worktree_path` does not exist → `Orphaned`.
- **C9 messages:** `tunnel_open`'s ".env" error names the path. An unregistered feature in tunnel paths returns `feature_not_found` naming `<repo>/.branchbox/registry.json`.
- **`init.rs`:** `update_gitignore` adds `.branchbox/devcontainer-sync/`.
- **Command skeleton:**
  - `cli/src/commands/version.rs` is complete. It aggregates `worktree_core::capabilities()` (atomic_fs, the write-ahead const in feature.rs, teardown_plan) plus `commands::{feature,detect,devcontainer,config,doctor,init,tunnel}::CAPABILITIES`, plus `json-error-envelope`.
  - `cli/src/commands/detect.rs` holds the moved text implementation; `--json` returns `CliError{code:"unsupported"}` until RS-3.
  - `cli/src/commands/doctor.rs` and `config.rs` have their full clap argument surfaces and stub runs returning `unsupported`.
  - Every command file declares `pub const CAPABILITIES: &[&str] = &[];`.
- **Contract test harness:**
  - `cli/tests/support/mod.rs`: `branchbox_cmd!`, `init_test_repo` with fixed `GIT_*_DATE`, `assert_single_json`, `normalize_json` (timestamps → `"<ts>"`, durations → 0, temp roots → `"<repo>"`, version → `"<version>"`, color → `"#000000"`, ports → 0), and `assert_fixture(area, name, value)` (compare to `cli/tests/fixtures/contract/<area>/<name>.json`; rewrite when `UPDATE_CONTRACT_FIXTURES=1`).
  - `cli/tests/json_contract.rs` covers: envelopes (`list --repo /nonexistent` → `not_a_git_repository`; `teardown nope` → `worktree_not_found`; `tunnel remove nope` → `feature_not_found`; `agent status` without a socket → `agent_unreachable`), `start --prompt <2100>` (single doc, warning), `exec … exit 3` (payload only, exit 1), `version --json`, list with `setup` and orphaned.
  - `cli/tests/registry_concurrency.rs` (`cfg(unix)`): 12 concurrent `teardown --force --json` leave all entries removed; 12 concurrent `start --minimal` leave 12 entries.

### 10.2 RS-2: teardown safety (wave 2; S1, S4, S5, C10a/b/c, C3)

- **`plan_teardown(&req, &opts) -> Result<TeardownPlan>`** (read-only) and **`teardown_with_options(req, opts)`**:
  - Branch resolution: explicit prefix, then the registry `branch_name`, then the config prefix (C10c).
  - The plan is built **before** any tunnel, module, spec, adapter or runtime step. Non-empty blockers → `Err(TeardownRefused)`.
  - Blockers: `uncommitted_changes` (S4 user changes and `!discard_changes`), `unmerged_branch` (S5), `worktree_locked`, `status_unavailable`.
  - The legacy `force_remove_modules` still permits module-area discards only.
- **Removal** (replacing feature.rs ~1201-1229):
  - Re-classify unless discard/force. New user changes → stop with `changed_anything=true` and `completed_steps`, keeping the worktree and the registry entry.
  - Otherwise `git worktree remove --force` (level 1, safe because everything left is verified generated or preserved), or `-f -f` only under `--force`.
  - The `remove_dir_all` fallback runs **only under `--force`**. Without force, a git failure gives `TeardownRefused{worktree_removal_failed}`.
  - `record_teardown` runs only when the worktree is gone (or was missing).
  - The baseline file is deleted after removal.
- **S4 classifier:** rules R1–R7 as in §6.5. `VSCODE_MANAGED_SETTINGS` is a shared `pub(crate) const` used by `setup_vscode_workspace`, and `is_module_managed_path` moves into teardown_plan.rs. Ignored files never count.
- **S5:**
  - `GitWorktree::branch_merge_state(branch)` follows `git branch -d` semantics.
  - Non-interactive (no TTY or `--json`) + delete requested + unmerged + no force flags:
    - `force_delete_unmerged_by_default` → `-D`;
    - otherwise **refuse before any change**, naming `--keep-branch` / `--force-delete-branch`.
  - Interactive behaviour is unchanged; the prompt's "yes" sets only `force_delete_branch`.
  - A post-preflight `-d` failure gives exit 0 with `branch_deleted=false` and `branch_delete_error`.
  - When `--force` causes `-D` of an unmerged branch (D-27), a warning names the commit count.
- **CLI:**
  - New `--discard-changes` (discard without implying `-D`) and `--dry-run`. The `--force` help is made accurate.
  - `run_teardown` is rewritten around the plan. The CLI-local `build_branch_name`, `local_branch_exists`, `local_branch_is_merged` and the post-hoc bail (860-872) are deleted. The config prefix is no longer injected at 839.
  - The text banner keeps its first line "⚠️  Detected devcontainer/compose changes inside <path>:" when module areas are involved (scripts/manual-cli-e2e.sh:951).
  - Interactive confirm sets only `discard_changes`.
- **C3 prune:**
  - `prune --json`, `--dry-run --json` and `--feature` selection.
  - Each candidate gets a per-candidate plan; the text listing is annotated with what will be lost.
  - `confirmation_required` in machine mode without `--yes`.
  - Prune passes `branch_prefix: None`.
  - Prune keeps its documented forced semantics (CLI users only).
- **Capabilities:** `teardown_plan::CAPABILITIES = ["teardown-plan","teardown-discard-changes","teardown-unmerged-preflight"]` and `commands::feature::CAPABILITIES = ["prune-json"]`.
- **Fixtures:** `cli/tests/fixtures/contract/teardown/{plan_dirty_unmerged,plan_fresh,summary_discard,refusal_envelope,prune_dry_run,prune_execute}.json`.

### 10.3 RS-3: CLI JSON surface (wave 2; C1, C2, C4, C5, C6, C9 tests, C10e)

| Item | What |
|---|---|
| C1 detect | Core `workflows/detect.rs` `detect_project`. `detect --json`. `stack` comes from `Stack::as_str()`, not the Debug format. |
| C2 devcontainer sync | `--json` and `--feature` (repeatable). Per-worktree results. `record_baseline` after each `sync_to`. **Exit 1 on any failure** (both modes). `devcontainer_source_missing`. |
| C4 config | Core `config_edit.rs`: jsonc-parser 0.27 `cst` feature; key registry; validators that name the key and the expected values; comment refusal with line and column; atomic write under the lock; mode preserved. `BranchBoxConfig::save` → `write_atomic`. `docs/docs/reference/configuration.md` is generated from the registry and kept in sync by a test. |
| C10e credentials | `tunnel credentials set`. |
| C5 doctor | Core `doctor.rs`: probe helper (spawn, poll `try_wait`, kill at the deadline; no new crate). Checks per §5.12. |
| C6 init | `init --json`: Serialize on `InitSummary`/`RepositoryState`/`DevcontainerStatus` (snake_case, tagged). `--op-*` flags mapped onto `write_op_env`. The token write at init.rs:1176-1180 is routed through the credentials writer (0600). |
| C9 | Tunnel contract fixtures. |

Capabilities: `detect-json`, `devcontainer-sync-json`, `config`, `tunnel-credentials`, `doctor`, `init-json`. Fixtures go under `cli/tests/fixtures/contract/commands/`.

### 10.4 Nice-to-haves: decisions

| Item | Decision | Reason |
|---|---|---|
| C8 `BRANCHBOX_PROGRESS=jsonl` | **Defer** | Text tracing parsing plus phase mapping is enough. Instrumenting it touches the busiest code (start). |
| C10f `feature exec --interactive` | **Defer** | sbx terminal and agent ship disabled with [Copy Command]; the container runtime uses a host terminal. |
| C10g start stash hazard | **Defer** (flagged) | The app shows every start warning prominently. A structured warning or core fix comes later. |
| C3 `--feature` selection | **Include** (in RS-2) | Cheap; the same file region. |
| C4 `config set/unset` | **Include** | Same engine; human ergonomics. |
| CR-14 `feature env` | **Defer** | Container env controls use `devcontainer up/down`. |
| CR-17 `name preview --json`, CR-19 `feature_url_href`, CR-21, CR-23, CR-28 | **Defer** | App-side fallbacks exist. |
| Decouple `--force` from `-D` | **Defer** to a deprecation cycle (D-27) | Compatibility. |

---

## 11. Notifications and the dev loop

- **Notifications** (SW-4 `UserNotificationNotifier`; bundled builds only):
  - authorization is requested lazily, at the first background completion or from onboarding step 2;
  - a delegate's `willPresent` suppresses banners while the app is frontmost, posting an accessibility announcement and an in-window toast instead;
  - sent when an operation ends while the app is inactive or the main window is hidden (respecting Settings), and when it took more than 10 s or failed;
  - text comes from typed results: "oauth is ready", "oauth started with problems", "Couldn't start oauth", "oauth torn down", "Teardown of oauth needs attention";
  - category actions: Show (opens the main window, selects the feature, shows the result) and Open in Editor;
  - failures are soft (logged).
- **Dev loop:**
  - `cd macos && swift run BranchBox`: unbundled, so notifications are no-ops, the `dev.branchbox.app.dev` suite is used and a "DEV" badge shows. `BRANCHBOX_APP_LOG_STDERR=1` mirrors `os.Logger` output. `BRANCHBOX_CLI_PATH=$PWD/../target/debug/branchbox` tests a branch CLI. `BRANCHBOX_BACKEND=preview` (DEBUG) runs fully on `PreviewBackend`.
  - `scripts/macos-dev.sh` builds debug and wraps it as `macos/build/dev/BranchBox Dev.app` (`dev.branchbox.app.dev`, ad-hoc signed). It runs in the foreground (logs stream; notifications work). `--open` launches it through LaunchServices (`open`) to reproduce the Finder/launchd environment.
  - Xcode: `open macos/Package.swift`.

---

## 12. Packaging and CI

### 12.1 `scripts/package-macos-app.sh` (rewritten in place; PK-1)

Flags: `[--universal|--native] [--configuration release|debug] [--embed-cli PATH] [--sign IDENTITY] [--notarize] [--zip] [--out DIR]`.

1. `VERSION` comes from `[workspace.package] version` (via `cargo metadata --no-deps` when cargo is present, else awk). `BUILD` = `git rev-list --count HEAD`. `SHA` = `git rev-parse --short HEAD`.
2. Build with `swift build --package-path macos -c <cfg> --product BranchBox [--arch arm64 --arch x86_64]`. Universal is the default. The binary path comes from `--show-bin-path`.
3. Bundle layout: `BranchBox.app/Contents/{MacOS/BranchBox, Info.plist, Resources/AppIcon.icns}`.
   - The plist is rendered from `macos/Packaging/Info.plist.template` with these keys:
     - `CFBundleIdentifier dev.branchbox.app`
     - `CFBundleName`/`DisplayName BranchBox`, `CFBundleExecutable BranchBox`, `CFBundlePackageType APPL`
     - `CFBundleShortVersionString=$VERSION`, `CFBundleVersion=$BUILD`
     - `CFBundleIconFile AppIcon`, `LSMinimumSystemVersion 14.0`
     - `LSApplicationCategoryType public.app-category.developer-tools`
     - `NSHighResolutionCapable`, `NSPrincipalClass NSApplication`, `NSHumanReadableCopyright`
     - `BranchBoxGitSHA`, `BranchBoxMinimumCLIVersion 0.13.4`
     - no `LSUIElement`
   - Validate with `plutil -lint`. Generate the icns with `iconutil` from the checked-in `AppIcon.iconset`, produced once by `make-iconset.sh` from `assets/icons/png/logo-darkmode-*` (rsvg-convert from `assets/icons/logo-darkmode-circle.svg` if available, for 1024).
4. `--embed-cli PATH` copies to `Contents/Helpers/branchbox` and signs it **first**.
5. `codesign --force --sign "${IDENTITY:--}" --options runtime --identifier dev.branchbox.app [--timestamp|--timestamp=none] [--entitlements macos/Packaging/BranchBox.entitlements] BranchBox.app`.
6. Gates (any failure fails the script):
   - `codesign --verify --deep --strict --verbose=2`
   - `lipo -archs` == "x86_64 arm64" (universal)
   - the plist version equals `VERSION`
7. `--zip`: `ditto -c -k --sequesterRsrc --keepParent` → `macos/build/BranchBox-$VERSION-$BUILD-$SHA.zip` plus `.sha256`.
8. `--notarize` is a documented stub: `xcrun notarytool submit --wait`, `stapler staple`, `spctl -a -vv`. It needs a Developer ID identity and secrets that don't exist yet; the stub exits with a cause-naming message.

### 12.2 `.github/workflows/macos-app.yml`
SW-0 creates the `test` job; PK-1 adds `integration`, `integration-floor` and `package`.

- **Triggers:** push to main, PRs, `workflow_dispatch`. Path filters: `macos/**`, `cli/**`, `core/**`, `scripts/package-macos-app.sh`, `scripts/macos-*.sh`, `.github/workflows/macos-app.yml`.
- **`test`:** matrix `{runner: macos-14, xcode: '16.2'}` and `{runner: macos-15, xcode: latest-stable}` (`maxim-lobanov/setup-xcode@v1`).
  - Steps: `swift --version`; `swift build --package-path macos --build-tests -Xswiftc -warnings-as-errors`; `swift test --package-path macos --parallel` (integration suites self-skip).
  - Guard steps: `! grep -rn 'import GRPC\|SwiftProtobuf\|NIOCore' macos/Sources` and `! grep -rnE '\.sheet\(|\.alert\(' macos/Sources/BranchBoxApp/MenuBar`.
  - *If the macos-14 image is retired, use macos-15 with `xcode-version: '16.2'` to keep the Swift 6.0 compile gate.*
- **`integration`** (macos-15): rust toolchain plus `Swatinem/rust-cache`; `cargo build --release --locked -p branchbox-cli`; `BRANCHBOX_IT=1 BRANCHBOX_IT_CLI=$GITHUB_WORKSPACE/target/release/branchbox swift test --package-path macos --filter BranchBoxIntegrationTests`; then `scripts/macos-capture-fixtures.sh target/release/branchbox $RUNNER_TEMP/live` and `BRANCHBOX_LIVE_FIXTURES=$RUNNER_TEMP/live swift test --filter LiveFixtureDecodeTests`. Unknown-key annotations are printed as `::warning::`.
- **`integration-floor`** (macos-15): `gh release download v0.13.4 -p 'branchbox-*-aarch64-apple-darwin.tar.gz'`; extract; run the suite with `BRANCHBOX_IT_CLI` pointing at the floor binary (legacy mode).
- **`package`** (macos-15, needs test): `scripts/package-macos-app.sh --universal --zip`; `codesign --verify --deep --strict`; `actions/upload-artifact@v4` `BranchBox-macOS-${{ github.sha }}` with `macos/build/BranchBox-*.zip*` and 14-day retention.
- **`ci.yml`:** SW-0 removes the old `macos_swift` job (lines 264-282) and nothing else.
- **Rust checks:** unchanged in `ci.yml` (fmt, clippy `-D warnings`, nextest, the 90% llvm-cov gate, review preflight). Contract fixture freshness is enforced inside nextest (`assert_fixture`), so no workflow change is needed.

---

## 13. Testing and verification plan

### 13.1 Swift unit tests (always run; `swift test`)

| Suite | Owner | Covers |
|---|---|---|
| BranchBoxKitTests | SW-0 | Decode every `cli-0.13.4` fixture (parameterized). `main_feature_list_all` gives 8 records with dates. prine: `.active`, `tunnel.status == .disabled`, 3× success, `browserURL == https://dev-prine.localhost`, `branchPrefix == "feature"`. Synthetic statuses `[.degraded,.failedRetained,.orphaned]` with port 49152→3000. Unknown enum round-trip; missing `work_feature` dropped and counted; `sandbox_start_gamma_longprompt` decodes with a non-nil preamble. camelCase devcontainer fixtures; `devcontainer up` error outcome. RFC 3339 table (0/6/9 digits, Z, +00:00, -04:00, whole seconds, garbage → nil). FeatureURLs. Fixtures contain no `/Users/<owner>` or `/private/tmp/<scratch>`. |
| BranchBoxKitTests/Planning | SW-3 | TeardownDraft defaults (unmerged → Keep even if config says delete; force never preselected; delete-if-merged + unmerged blocks), `makeRequest().discard == nil` always; RecoveryPlanner matrix (dirty → "Discard N changes" retry with consent of exactly those files, destructive with confirmation; moduleFilesDirty with zero user changes → "Discard BranchBox-generated files"; moduleFilesDirty with user changes → user-changes recovery; unmergedBranch → Keep / Force-delete; commandFailed → [] except showLog/openDoctor); PrunePlanner safe set; StartDraft (2001 chars blocks; name XOR title; reset per presentation); Remediation table per status × runtime × setup state; HostLaunchPlan quoting (spaces, quotes, prompt escaping) and sbx disabled. |
| BranchBoxCLITests | SW-1 | See the next table. |
| BranchBoxStoresTests (PreviewBackend) | SW-2 | 3 overlapping refreshes → exactly 2 list calls; stale generation dropped; failed load keeps the last good list; selection lookup nil after removal; refresh after success, failure and cancel; per-feature exclusivity; FIFO registry writers without `.registryLock` (incl. sync) vs concurrent with it; project-wide exclusivity; prune skip-and-continue and stop-between-rows; ≤ 25 store mutations for 2000 log lines; ordered events; legacy migration; intent token; FSEvents temp-dir test (in-place and rename within 2 s); ListLimiter ≤ 2; Notifier unavailable under xctest; projects.json round-trip and ordering. |
| BranchBoxAppTests | SW-3/4/5/6/7 | Presentation mappings; router (palette intent presented after dismissal; two intents never stack; intent posted while closed consumed on appear); command enablement; feature surfaces (availability matrix, URL normalizer use, remediation buttons dispatch the right context); flows (teardown sheet never dispatches discard on the first attempt — asserted via the PreviewBackend call log; prune row selection); settings form → ConfigPatch; init sheet argv intent. |

**BranchBoxCLITests (SW-1) detail:**

| Area | Tests |
|---|---|
| Runner (FakeCLI scripts in `Tests/BranchBoxCLITests/FakeCLI/`) | 200 KB stdout + 200 KB stderr interleaved, no deadlock (< 5 s), every line streamed; 24 concurrent runs; stdin EOF; stdin data delivered; non-zero exit keeps stdout; ANSI, CSI/OSC and `\r` splitting; cancel → SIGINT-ignoring grandchild reaped (`kill(pid,0)` fails), returns within grace + 1 s; **cancel-before-launch never spawns** (marker file absent); **cancel-during-launch** signals once the pid is published; timeout escalates to SIGTERM; grandchild holding stdout returns within drainGrace + 0.5 s; missing cwd is typed; launch failure is typed; stdoutTooLarge; `terminateAll`; no leaked processes. |
| Environment | Sentinel parser with rc-file noise before and after, missing end sentinel → fallback; denylist; PATH order and dedupe; overrides last; persisted snapshot holds PATH only. |
| Locator | Temp dirs and fake executables: precedence, unresolved paths kept, rejected reasons, 0.13.3 → tooOld, `version --json` parse, exit-2 fallback, cache invalidated by mtime. |
| CLICommand golden argv | Every method × option. Teardown always has exactly one branch flag plus `--json` plus `--repo` plus derived `--branch-prefix` (`spike/zeta` → `spike`; branch == name → `""`). Legacy teardown always `--keep-branch` and never `--force` without discard/forceRemoval. Contract-mode discard → `--discard-changes`, never `--force`. exec flags before `--`. Never `--title`, never `--no-summary`. init always `-y`, `--reorganize` only when requested. |
| Classifier | Real stderr fixtures: alpha → `.moduleFilesDirty([".devcontainer/"])`; beta_default → `.partial(.unmergedBranch("feature/beta"))`; "Not a git repository: /" → `.notGitRepository`; envelope beats text; unknown failure → `.commandFailed` whose summary is the `Error:` line. |
| CLIBackend with ScriptedProcessRunner | exec exit 1 → `ExecResult(exitCode: 3)`; legacy teardown with user dirt throws `.refused(.uncommittedChanges)` **without spawning the CLI**; discard consent mismatch refuses; legacy branch step runs `git branch -d` after `--keep-branch`; devcontainer error outcome → commandFailed("Docker is not available"); legacy sync `✗ failed` with exit 0 → failed row; cancellation → `.cancelled`; decode error → `.decodeFailed` with CLI version; registry parse error → `.registryCorrupted`. |
| Git (temp repos, real `/usr/bin/git`) | Feature worktree → main root; parent container; non-git refusal; WorktreeChangeClassifier R1–R7 positive and negative (untracked notes.txt → user; `.devcontainer/.branchbox.env` → generated; edited tracked `.vscode/settings.json` → user; untracked `.env` with an extra key → user); merge state (merged, unmerged ahead=1, upstream-pushed not merged into HEAD → merged per `-d`, `+`-marked worktree branch handled); stray detection excludes a user worktree outside the layout. |

### 13.2 Gated integration against the real CLI (`BRANCHBOX_IT=1`, CLI from `BRANCHBOX_IT_CLI` or the locator)

Docker-free, run in disposable temp git repos (`TempRepo`: `git init`, commit with `-c user.email`, `.gitignore`). The child env starts from a launchd-like PATH (`/usr/bin:/bin:/usr/sbin:/sbin`) plus augmentation. Hard per-test time limits; cleanup in `defer` with a temp-dir sweep, leaving no worktrees or branches.

- **SW-1 smoke (wave 2):** identity → empty list → `start --minimal --skip-module tunnel` → list shows it → exec ok → teardown flow (legacy or contract) → list `--all` shows removed.
- **VER-1 full suite (wave 4):**
  - **Lifecycle:** exec ok and exit 3 (payload); untracked file → plan shows user change → teardown refused without spawning on legacy / CLI envelope on 0.14 → discard recovery → removed; branch policy Keep keeps the branch; Delete-if-merged deletes a merged branch; unmerged + Force-delete deletes; custom prefix (`--branch-prefix spike`) deletes `spike/<n>` (DRIFT-07).
  - **Refusals:** duplicate start → `.worktreeExists`; no .gitignore on 0.13.4 → `.moduleFilesDirty` → generated-only recovery.
  - **LargeRegistry:** 40 features push `list --json` past 64 KiB; it completes in < 10 s.
  - **Cancellation:** a `post-checkout` hook sleeps; cancel 2 s into start returns in < 6 s. 0.13.4 → stray reported and `removeStray` cleans it; 0.14 → record with `setup.state == interrupted`, and Resume works.
  - **Concurrency (0.14 only):** two parallel starts plus a sync keep every entry.
  - **resolveProject** on a feature worktree normalizes to main.
  - **planPrune** over 3 features with one dirty.
  - **detect.**
  - **config (0.14):** get/apply round-trip preserves an unknown key; invalid enum → `.configInvalid` naming the allowed values.
  - **Watcher:** a CLI write in Terminal triggers `ProjectStore` refresh within 2 s.
  - **ContractFixtureDecodeTests** (KitTests, ungated): decode every file in `cli/tests/fixtures/contract/**` via `#filePath`.
  - **LiveFixtureDecodeTests:** decode `BRANCHBOX_LIVE_FIXTURES` output from `scripts/macos-capture-fixtures.sh`; print unknown keys.

### 13.3 Rust
- `cargo fmt --all -- --check`; `cargo clippy --all-targets --all-features -- -D warnings` (with disallowed-macros); `cargo nextest run --all-features` (including agent tests and contract fixtures); `cargo llvm-cov` ≥ 90%; `./scripts/review-preflight.sh`.
- **RS-1 tests:**
  - lost-update proof: two threads with a cfg(test) 200 ms hook, both entries survive;
  - lock timeout names the path;
  - `write_atomic` modes, symlink refusal, no temp leftovers;
  - concurrent reader loop: 500 iterations of ~200 KB never fails to parse;
  - registry_concurrency integration;
  - write-ahead: SIGKILL a start after worktree creation → list shows `setup.state == interrupted`; an old registry without `setup` loads; a registry with `setup` loads in a struct copy of 0.13.4's `FeatureMetadata`;
  - C10d orphaned;
  - json_contract envelopes and fixtures;
  - text-mode harness phrases are unchanged.
- **RS-2 and RS-3 tests:** per §10.2 / §10.3 and the core spec test lists (BUG-04 regression, E4 fresh-feature teardown exit 0, unmerged non-TTY refusal before removal, `--discard-changes` with merged and unmerged branches, locked worktree, status_unavailable, dry-run with no FS change, config/doctor/init/sync/detect/credentials). The **existing** `cli/tests/feature_commands.rs` `--force` tests keep passing (D-27).

### 13.4 Manual: "Mac App ↔ CLI Loop"

DOC-1 writes it into both `docs/docs/getting-started/manual-cli-e2e.md` and `scripts/manual-cli-e2e.md`. VER-1 executes it on the packaged app and records results in `macos/TESTING.md`.

0. `cargo build -p branchbox-cli`, then `scripts/package-macos-app.sh --native --zip`. Remove quarantine (`xattr -dr com.apple.quarantine`) if the build came from CI.
1. Launch from Finder with a stripped PATH. Onboarding finds `/opt/homebrew/bin/branchbox` (or use Settings › Locate for the branch CLI); the doctor rows render.
2. Add a disposable `git init` repo → Set Up BranchBox (`init -y`, Keep layout) → project appears; the repo did not move.
3. Start a minimal feature → live log → result shows the resolved name → `branchbox feature list --json --repo …` in Terminal shows it.
4. Start another feature from Terminal → the app shows it within about 1 s.
5. Run Command `echo hi`, then `sh -c 'exit 3'` (exit 3 shown; no alert).
6. `touch notes.txt` in the worktree → Tear Down → refusal card names notes.txt → Discard → removed; the branch follows the chosen policy (`git branch --list`).
7. Commit in a worktree → Tear Down with Delete-if-merged is blocked → Force-delete with confirmation → deleted.
8. Prune with 3 features (one dirty) → the dirty row is unchecked → results.
9. Cancel a start (sleeping `post-checkout` hook) → confirmation copy → Interrupted/Unregistered row → Resume or Remove.
10. Close the main window → menu bar Open BranchBox reopens it; menu bar Tear Down… opens the window and the sheet.
11. Quit while an operation runs → prompt → Cancel and Quit leaves no `branchbox` process (`pgrep branchbox`).
12. (0.14 CLI) Project Settings branch prefix → `branchbox config get feature.branch_prefix --json` shows it.
13. `cd macos && swift run BranchBox` starts a feature without crashing (notifications are no-ops).

---

## 14. Docs to update (DOC-1)

| File | Change |
|---|---|
| `macos/README.md` | Rewrite: requirements (macOS 14, branchbox ≥ 0.13.4), how the CLI is found, Doctor, dev loops (`swift run BranchBox`, `BRANCHBOX_BACKEND=preview`, `scripts/macos-dev.sh [--open]`), packaging, the quarantine note, the non-sandboxed rationale, troubleshooting. Remove milestone2, gRPC and embedding text. |
| `AGENTS.md` | Drop "macOS Proto Bindings" (195-200). Rewrite the line-86 loop instruction (Mac App ↔ CLI Loop). Add a "macOS app" section (targets, `swift build --build-tests -Xswiftc -warnings-as-errors`, CI job, fixtures). Add an "API/JSON contract" section: machine mode, envelope, codes, capability strings added with their implementation, additive-only, **no new FeatureStatus variants**, `UPDATE_CONTRACT_FIXTURES=1`. Add teardown and stdout guardrails ("teardown never deletes uncommitted user work without --force/--discard-changes; classification lives in workflows/teardown_plan.rs"; "stdout in --json mode is one JSON document; use humanln!/emit_json"). Update State Management (lock and atomic writes; remove the race TODO). Fix the line-7 workspace blurb and the `devcontainer sync --json` references. |
| `README.md` | Replace "macOS app integration over gRPC". |
| `docs/docs/getting-started/manual-cli-e2e.md` and `scripts/manual-cli-e2e.md` | Replace "Mac App ↔ Agent Loop" with the §13.4 loop in **both** (identical mac sections). |
| `docs/docs/getting-started/development.md` | Mac app section. |
| `docs/docs/getting-started/installation.md` | App install note (CI artifact, quarantine; cask later). |
| `docs/docs/getting-started/quick-start.md`, `docs/docs/guides/parallel-features.md` | Teardown safety, `--discard-changes`, `--dry-run`, the `--force` semantics note. |
| `docs/docs/internals/architecture.md` | The mac app is CLI-JSON-first and the agent comes later; registry locking; fix the stale Unix-socket/agent-install claims. |
| `docs/docs/reference/cli.md` | **Regenerate once** from recursive `--help` (`version`, `doctor`, `config`, `tunnel credentials`, new teardown/prune/sync/detect/init flags). |
| `docs/docs/reference/json-contract.md` (new) | §5 rules, envelope, codes, exit codes, payload index. |
| `docs/IMPLEMENTATION_STATUS.md`, `docs/features/backlog/mac-app-polish.md`, `docs/features/backlog/mac-app-proto-codegen.md` (archive note), `docs/features/backlog/milestone3.md` | Status updates. |
| `docs/features/in-progress/mac-app-revamp.md` | Fill the spec: overview, decisions, status. |
| `CHANGELOG.md` | Fold `changelog.d/*` into `[Unreleased]`. **Fixed:** BUG-04, UX-13, CORE-01, start --json stdout, DRIFT-07, app breakages. **Changed:** non-interactive unmerged refusal before removal, sync exit 1, --json non-interactive with human text to stderr, orphaned for missing worktrees, macOS 14 minimum, gRPC removed from the app. **Added:** version/doctor/config/credentials/detect --json/sync --json/prune --json/teardown --dry-run/--discard-changes/init --json, error envelope, the app features. |
| `scripts/generate-swift-protos.sh` | Delete. |

`docs/docs/reference/configuration.md` is owned by RS-3 (generated).

---

## 15. Work packages (summary; full briefs in the work_packages output)

| Wave | Packages (parallel within a wave) |
|---|---|
| 1 | **SW-0** Swift contracts, package reset and skeleton · **RS-1** Rust foundations |
| 2 | **SW-1** BranchBoxCLI · **SW-2** Stores · **SW-3** Planning, presentation and UI kit · **RS-2** Teardown safety · **RS-3** CLI JSON surface |
| 3 | **SW-4** App shell and menu bar (+ milestone M1 vertical slice in its first PR) · **SW-5** Feature surfaces · **SW-6** Operations and lifecycle flows · **SW-7** Projects, onboarding, settings, diagnostics |
| 4 | **PK-1** Packaging, dev loop, CI completion · **DOC-1** Docs and changelog · **VER-1** Integration tests and end-to-end verification (merges last; depends on PK-1 and DOC-1) |

Milestone **M1** is SW-4's first PR, within about 2 days of wave 3 starting: the composition root wired to the real CLI, sidebar with a real `feature list`, read-only feature detail stub, and a teardown dispatched through `ActionDispatcher` using the `TeardownSheet` stub's default request.

**Cross-wave handoffs** are the only shared files:

| Files | Handoff |
|---|---|
| Kit `Backend/`, `Models/`, `Process/` | SW-0 → SW-1 (additive) → VER-1 (fixes) |
| Stores | SW-0 skeleton → SW-2 → SW-4 (additive in wave 3) |
| `BranchBoxPreview` | SW-0 → SW-2 |
| Kit `Planning/`, App `Components/`, `Presentation/`, `Services/` | SW-3 → SW-4 (additive) |
| `App/BranchBoxApp.swift`, `Navigation/{SheetRoute,SceneID}.swift` | SW-0 → SW-4 |
| Screen stubs | SW-0 → SW-5/6/7 |
| TestSupport | SW-0 → SW-1 → VER-1 |
| `BranchBoxIntegrationTests/` | SW-0 → SW-1 → VER-1 |
| `macos-app.yml` | SW-0 → PK-1 |
| `scripts/macos-dev.sh` | SW-4 → PK-1 |
| Rust: `core/src/workflows/{feature.rs,teardown_plan.rs}`, `cli/src/commands/feature.rs`, `cli/tests/feature_commands.rs` | RS-1 → RS-2 |
| Rust: `core/src/{lib.rs,workflows/mod.rs,workflows/init.rs}`, `cli/src/commands/{detect,doctor,config,devcontainer,init,tunnel}.rs`, `Cargo.toml`, the remaining `cli/tests/*_commands.rs` | RS-1 → RS-3 |

`Package.swift` is frozen after SW-0.

---

## 16. Deferred (explicit)

- **Distribution:** Developer ID signing, notarization, stapling, the tag-triggered release job, and the Homebrew cask `Casks/branchbox.rb` with `depends_on formula: "branchbox/tap/branchbox"`. The script flags are reserved.
- **AgentBackend:** an agent-daemon conformer, which needs the agent fixes first: blocking tokio handlers → `spawn_blocking`, process-global env options, an owner-only UDS instead of TCP, and a List filter that hides statuses. The recommended first version relays to the CLI.
- **CLI features:** C8 JSONL progress; C10f `feature exec --interactive` (sbx terminal and agent); C10g start stash hazard; CR-14 `feature env start|stop|status`; CR-13 exec streaming; CR-17 name preview JSON; CR-19 `feature_url_href`; CR-21 the CLI reading `editor.default_agent`; CR-23 main tunnel provisioning; CR-28 `list --no-reconcile`; `devcontainer sync` also running `configure_workspace_settings`.
- **`--force` decoupling** from `git branch -D` (a deprecation warning in a later release, then the change).
- **App features:** Keychain storage for the Cloudflare token and secret extra env; clone-from-URL onboarding; PR card and `gh` integration; container compose port-mapping discovery; a global cross-project Overview.
- **Quality and process:** Swift coverage gating (reported only); the workspace version bump and release notes.

## 17. Risks

1. **Swift 6.0 vs 6.2.** The dev machine only has 6.2.4, so 6.0 compile differences show up only in CI. *Mitigation:* the macos-14/Xcode 16.2 leg gates from SW-0's first PR; untyped throws; no 6.1+ APIs. If it's unworkable, record a decision to require Xcode 16.3+ (Swift 6.1) for CI while keeping the macOS 14 runtime minimum.
2. **Legacy (0.13.4) data-loss race.** A user edit between the app preflight and the CLI run can be deleted by `remove_dir_all`. *Mitigation:* re-preflight immediately before spawning; RS-2 closes it in core; the UI recommends upgrading.
3. **Registry corruption on cancel with 0.13.x** (truncate-in-place, no signal handler). *Mitigation:* the explicit cancel warning, the `.registryCorrupted` diagnosis, and RS-1's atomic writes landing in wave 1.
4. **Login-shell capture** may hang, be slow, or meet exotic shells. *Mitigation:* sentinels, stdin `/dev/null`, timeouts, fallbacks, the provisional env, Re-capture, and the CLI path override.
5. **sbx remediation paths (`--reuse-runtime` for degraded) are unverified.** *Mitigation:* VER-1 manual check on an sbx machine; otherwise ship as a copy-command action.
6. **Scope (14 packages, two languages).** *Mitigation:* the wave-1 contracts and PreviewBackend decouple UI from the CLI; the app is safe on 0.13.4, so the Rust work does not block shipping; changelog fragments; strict file ownership.
7. **Contract drift** between Rust and Swift. *Mitigation:* golden fixtures enforced in nextest and decoded in Swift; the same-commit CLI integration leg plus the v0.13.4 floor leg; unknown-key annotations.
8. **FSEvents gaps** (network volumes, sleep/wake) and runtime-derived health that never touches the registry. *Mitigation:* activation refresh, timers, after-operation refresh, ⌘R.
9. **`.menu` MenuBarExtra limitations** (no open hook, no rich rows). *Mitigation:* timers plus the watcher; keep rich UI in the main window.
10. **Notifications on ad-hoc bundles** are untested. *Mitigation:* the bundle guard, soft failure, and the Activity window as the source of truth.
11. **Gatekeeper quarantine** on CI zips (macOS 15 removed the right-click bypass). *Mitigation:* document `xattr -dr com.apple.quarantine` and System Settings › Open Anyway.
12. **The macos-14 GitHub image may be retired.** *Mitigation:* the fallback leg (§12.2).
13. **`--force` compatibility confusion** (D-27). *Mitigation:* accurate help text, `--discard-changes` recommended in messages, and a warning when `--force` deletes unmerged commits.
14. **S1 behaviour change for agent IPC and in-guest supervisors** that tear down without `--force`: they now get a refusal instead of silent deletion. *Mitigation:* the repo's harnesses and tests use `--force`; a CHANGELOG "Changed" entry.

## 18. Decisions needing the owner
None are blocking. The defaults are recorded above. Notable defaults the owner may want to revisit later:
- D-2 (macOS 14)
- D-27 (`--force` stays coupled to `-D` for now)
- D-7 (`.menu`-style menu bar)
- D-20 (token in a 0600 secure file, not the Keychain)
- tunnels Off by default in the GUI init sheet
- prune's default safe selection

## Implementation deviations (wave 1)

Wave 1 (SW-0, RS-1) landed with the deviations below. Each one is deliberate. Later packages code against the shipped behaviour, not the earlier text above. §4.8's `SyncReport.Row.Status` line was corrected in place, and that is the only contract text that changed.

### Swift (SW-0)

**Contract text and identity**
- **`SyncReport.Row.Status.wouldSync`** has the raw value `"would_sync"`, matching §5.8. §4.8 now says so.
- **`ProjectRef` equality and hashing** compare the standardized `path`, not `root`. Its decoder goes through `init(root:)`. Two equal refs can carry `file:///r/main/` and `file:///r/main`. So key, persist and compare by `ref.path`, never by `root.absoluteString`; this covers `projects.json` and SceneStorage.

**Decoding**
- **`BackendError.normalize`** also maps `ProcessRunError` case by case:
  - `.cancelled` → `.cancelled(note: nil)`
  - `.timedOut` → `.timedOut(operation: "command", …)`
  - `.launchFailed` → `.launchFailed`
  - `.workingDirectoryMissing` → `.projectInvalid(.workingDirectoryMissing)`
  - `.stdoutTooLarge` → `.commandFailed`
- **Every non-identity field decodes leniently**, and arrays go through `Lossy`.
  - A missing `status` reads as `.unknown("")`.
  - Codable sub-models write dates back as RFC 3339 strings.
  - `Fixtures.data(_:)` throws instead of trapping.
- **`TeardownPlanDocument` gains `droppedBlockers`**, and `Changes` gains `droppedEntries`. These count array elements that could not be decoded.
  - A dropped user change also forces `changes.statusAvailable = false`.
  - SW-3's `TeardownDraft` and `PrunePlanner` must treat `droppedBlockers > 0` as blocked.
- **`ProjectConfig.tunnelDefaultProvider`**: null and absent both read as `"cloudflared"`, as core's `ensure_defaults()` does.
- **Devcontainer payloads decode any JSON object.** `DevcontainerResult.message` falls back to an `error` string key. New `DevcontainerResult.isRecognized` (outcome present) and `DevcontainerServiceInfo.isRecognized` (service name present) flag that case. SW-1 decodes `ErrorEnvelope` first (§6.3) and treats an unrecognized payload as `.decodeFailed`.
- **`DetectReport`, `SyncReport`, `DoctorReport` and `InitReport` stay non-`Decodable`**, as in §4.8. SW-1 decodes §5.7, §5.8, §5.12 and §5.13 with its own DTOs, or adds the conformance.

**Additive public API (rule e)**
- Public memberwise inits with defaults.
- `FeatureListing(decoding:strays:warnings:)`, `FeatureURLs(featureURL:tunnel:runtime:adapter:)` and `StartSummary.urls`.
- `DevcontainerResult.isError`, `VersionInfo.capabilitySet` and `RFC3339.format`.
- `ProjectConfig.defaults` and `TeardownPlanDocument.Changes.unavailable`.
- `LogLine.maxMessageBytes` (16 KiB) and `Diagnostics.logTailLimit` (50).
- `SecretString: CustomReflectable` and `AppBundle.isBundledApp(_:)`.
- Public inits for `UserNote`, `StepProgress`, `PruneRow` and `PruneResult`.
- `SheetRoute.init?(_: WindowIntent)` and `BranchBoxApp.makeBootstrapper(environment:)`.
- The `PreviewScenario` struct, plus the extra `PreviewBackend` and `PreviewSamples` helpers.

**Stores skeleton (SW-2 replaces)**
- **`LogBuffer` lives in `Stores/OperationRecord.swift`.** SW-2 may move it to `LogBuffer.swift`.
- **Store initializers are internal.**
- **`OperationStore.admission`** always returns `.allowed`.
- **`ProjectStore.refresh`** has no single-flight. Its attention list does not detect missing folders.
- **`AppModel.backend()`** throws `.cliUnusable(path: "", reason: …)` while the CLI is still being located.
- **`prepareForTermination`** has no 10 s bound.

**Tests and CI**
- **`ScriptedProcessRunner`** is a lock-based `final class`, not an actor.
  - It streams stdout lines only when `spec.streamStdout` is set.
  - A run is visible in `launched` only once its delay or hang gate is registered, so `terminateAll()` after an observed launch always ends it.
- **Store tests write `UserDefaults` suites as absolute-path plists** under `$TMPDIR/branchbox-tests/`, never in `~/Library/Preferences`.
- **The macos-app.yml Test step** also fails unless Swift Testing reported a non-zero `Test run with N tests … passed`. Under SwiftPM 6.0 `--parallel`, the gate cannot pass on an empty run.
- **Not yet verified:** the Swift 6.0.3 / Xcode 16.2 compile (macos-14 leg) has not run anywhere, because only Swift 6.2.4 exists locally. The first CI run of the branch is the gate. The likely failure spots if 6.0 rejects something:
  - `didSet` on `@Observable` properties;
  - `public let id: UUID` on the `@MainActor` `OperationRecord`;
  - `nonisolated var id` on `ProjectStore`;
  - continuations in the `@unchecked Sendable` `PreviewWait` and `RunGate`.

### Rust (RS-1)

**Error envelope and codes**
- **`Error::TeardownRefused`** also carries `changed_anything` and `completed_steps`.
- **New `Error` variants:** `FeatureNotFound{name, registry}`, `ConfigInvalid{message, key, line, column, expected}`, `ConfigUnknownKey{key}` and `WorktreeMissing{name, path}`.
  - `WorktreeMissing` has code `worktree_not_found` and details `{name, path}` (one key beyond §5.2).
  - `Error::details()` builds the §5.2 detail objects.
- **`json_error::recode(err, code, details)`** gives an error a contract code without changing its text.
- **Envelope mapping beyond the §5.2 table:**
  - A plain `std::io::Error` in the chain maps to `io_error`.
  - `Http`, `Json` and the tool-request variants map to `internal`.
  - The panic envelope is printed only for main-thread panics.
- **The legacy dirty-module refusal** is `teardown_refused` with `details.plan = null`, plus `worktree` and `files`. Without the `teardown-plan` capability, SW-1 maps a null plan to `.moduleFilesDirty(files: details.files)`. RS-2 removes this path.
- **Legacy in-band `{"error": "<string>"}` payloads** are an exception to §5.2 rule 4 and §6.3 rule 4.
  - They come from `devcontainer detect`, `configure`, `add-tunnel` and `inject-agents --json` when no `.devcontainer` exists, with exit 1.
  - SW-1's envelope probe requires `schema_version` plus an object `error`.
  - RS-3, which owns `devcontainer.rs`, may convert them to envelopes.
- **Argument refusals are coded `validation_failed`**, with the text unchanged. These are `feature start --default-prompt` without `--minimal`/`--fast`, and `feature list --status <bad>`.
- **Bails RS-2 must still code (now `internal`):**
  - the non-interactive unmerged-branch refusal;
  - the post-hoc "could not be deleted without force" check;
  - the prune refusals.
- **`feature teardown <unknown> --json` gives `worktree_not_found`**, as `cli/tests/json_contract.rs` asserts. If RS-2 switches it to `feature_not_found`, that test's owner updates the test and `envelope_worktree_not_found.json`.

**Output and commands**
- **`version --json`** has no `schema_version` key, following §5.3.
- **The truncation notice** goes into `summary.warnings` in `--json` mode only.
- **Compact JSON outputs are now pretty-printed by `emit_json`.** These were `tunnel open/remove --json`, the Docker-unavailable payload and the devcontainer `{"error"}` payloads.
- **`name validate` failures** print an extra `Error:` line on stderr.
- **`init` without a terminal (or with `--json`)** keeps the current location and names `--yes`. It no longer reads piped stdin.
- **Each command module declares `wants_json()` and `CAPABILITIES`.** Add each new capability in the module and the change that implements it.
  - A new `--json` flag must also be reported by its `wants_json()`, or machine mode stays off. For RS-2 that is `FeaturePruneArgs` for `prune --json`. For RS-3 it is `InitArgs`, `DetectArgs`, `ConfigCommands` and `DoctorArgs`.
  - Use `CliError` or `json_error::recode` so refusals keep their codes in `--json` mode.

**Locking**
- **New per-repository worktree lock** in `git.rs`, on the shared git directory.
  - It covers worktree add, remove and prune, and branch deletion.
  - It times out after 10 minutes with `command_failed`.
  - Create and attach re-check existence under the lock, so a concurrent same-name start still gets `worktree_exists`.
- **Lock waits log one line.** A contended registry or worktree lock prints one `info` line on stderr: "Waiting for another BranchBox process to release <path>".
- **Filesystems without advisory locks** (ENOLCK, EOPNOTSUPP/ENOTSUP or `Unsupported`) proceed unlocked, as 0.13 did, after one warning. `registry-lock` is still advertised.
- **On NFS under Linux**, `flock` is per-process, so two threads of one process (the agent) do not exclude each other there.
- **`write_atomic` does not create a missing parent directory**, and it has no exact-mode variant. To tighten an existing file, narrow it with `set_permissions` first; the rewrite keeps the narrower mode.

**Write-ahead start**
- **On a live entry** (`--reuse`, `--reuse-runtime`), the write-ahead record only adds `setup`. Discarding restores the previous entry. A failed provisional write only warns.
- **`list` skips the provider orphaned/degraded checks** for entries that have `setup`. `setup` is read leniently.
- **The prepared runtime is recorded early.** Once a non-in-guest runtime's `prepare` returns (sbx, local-vm), the entry records the runtime metadata (`runtime_id`), and `setup` stays `in_progress`. So an interrupted entry may name its runtime, and `teardown --force` removes it. After a failed environment start whose runtime was destroyed cleanly, the identity is cleared again.
- **PID reuse** can show a dead start as `in_progress` for up to 24 h. This is accepted and not fixed.

**Repository and build**
- **`init`'s `.gitignore`** also adds `.branchbox/.registry.*.tmp` and `.branchbox/.lock`.
- **`rust-version = "1.89"`** is inherited by core, cli and agent through `rust-version.workspace = true`.
- **Coverage:** workspace line coverage is 74.49% (integration run), against a 90% brief gate. The v0.13.4 baseline is 71.83%, so this is not a regression. The gate should read "no regression, and ≥ 90% for new modules". It needs `LLVM_COV`/`LLVM_PROFDATA` from `xcrun` locally, because llvm-tools-preview is not installed.

## Implementation deviations (wave 2)

Wave 2 (SW-1, SW-2, SW-3, RS-2, RS-3) landed, was reviewed, and was then integrated in one tree. The integration also fixed the confirmed review findings. Each deviation below is deliberate. Wave-3 packages (SW-4 app shell and menu bar, SW-5 feature surfaces, SW-6 flows, SW-7 projects, settings and diagnostics) code against the shipped behaviour, not the earlier text above. The contract text in §4 and §5 was not edited: where it and this section disagree, this section wins.

### Capabilities a current CLI advertises

`branchbox version --json` from this tree lists all 13 §5.3 capabilities:
- `json-error-envelope`, `registry-lock`, `write-ahead-start` (RS-1);
- `teardown-plan`, `teardown-discard-changes`, `teardown-unmerged-preflight`, `prune-json` (RS-2);
- `detect-json`, `devcontainer-sync-json`, `config`, `tunnel-credentials`, `doctor`, `init-json` (RS-3).

The version is still `0.13.4` until the release bump. Contract mode means `contract_version` is present, not a version number. CLISmokeTests passes in contract mode against this CLI and in legacy mode against the installed 0.13.4.

### Integration changes (after review)

**Teardown (Rust core and CLI, RS-2 findings)**
- **New blocker `not_a_worktree`** (fields `cause`, `message`, no `override`). It is the first blocker whenever the folder at `<parent>/<name>` is not a linked worktree of this repository: the main worktree (`teardown main`), an unrelated repository, or a plain folder. Neither `--force` nor `--discard-changes` overrides it.
  - Before this fix, `teardown main` emptied main's `tmp/` and `.cache/`, and `--force` then deleted the whole main repository or the sibling repository.
  - Two folders still count as worktrees. The first is a folder whose `.git` link points into this repository's `worktrees/`, also through the in-guest view `/workspaces/main/.git/worktrees`. The second is a registered feature's folder with no `.git` at all, which is what a forced teardown that failed halfway leaves behind. That one needs `--force`, through `status_unavailable`.
- **New blocker `spec_not_preserved`** (`path`, `cause`, `message`, `override: "--force"`). When moving the feature spec to the main worktree fails (for example a read-only `docs/features/backlog/`), teardown stops before the runtime goes, with `changed_anything: true`. It stops under `--discard-changes` too, because "preserved" is a promise. Only `--force` removes the worktree anyway.
- **R2 now preserves exactly one spec**: the one teardown moves, the first of `in-progress/`, `backlog/` and `completed/` that exists. A second copy goes through R3–R7 and is normally a user change. A spec path that no longer exists (a promoted spec's deletion) is still preserved. An in-guest teardown moves no spec, so an edited in-guest spec is a user change.
- **The adapter cleanup moved later.** It now runs after the runtime is destroyed and after the last re-check, immediately before `git worktree remove`. A teardown that stops keeps `tmp/`, `build/` and `dist/`, and files written there meanwhile stop the teardown.
- **Dry run of a missing worktree without `--force`** prints "✗ Teardown would refuse: the worktree … is missing; rerun with --force". The JSON plan is unchanged: `worktree.exists: false` and no blocker.
- **R4 reads less.** It reads main's copy only when the sizes match.

**CLI JSON surface (RS-3 findings)**
- **The obsolete test was removed.** RS-1's `json_contract` test asserting the `unsupported` stubs is deleted. `CliError::unsupported` keeps an item-level `allow(dead_code)`, and the stale `changelog.d/rs-1.md` bullet is gone.
- **`"tunnel": {"providers": {"cloudflared": null}}`** (what `init` saves when tunnels are declined) no longer blocks `config set`, `config apply` or `tunnel credentials set`: the null is replaced by an object. Other non-object section values are still `config_invalid`.
- **`init` treats a project as initialized only when `.branchbox/registry.json` exists**, the same rule as `detect --json` and `doctor`. A `.branchbox/` holding only `config.json` or `secure/` gets a full init, which keeps that configuration. Before, `init` returned `already_initialized` and did nothing.
- **`BranchBoxConfig::save` over an existing file** edits only the keys that changed, through the `config_edit` CST, so unknown keys and the formatting survive `init --update`. A dropped provider section is removed instead of written as `null`. A new file is still written whole.
- **`init --validate --json`** reports the detected `stack`, `adapter` and `modules`.
- **`detect` / `doctor --repo` from a subfolder** normalize the relative `--git-common-dir` lexically, so paths read `<main>/.branchbox/…`.
- **`doctor` deadlines.** Each doctor check has one 3 s deadline shared by its two probes (Compose then `docker-compose`, a CLI's version then its sign-in). The whole report takes about 3 s at most; set SW-1's process timeout above that.
- **`devcontainer sync` text mode** prints each worktree's row as it finishes again. JSON mode still collects, then prints one document.
- **`config apply` messages.** Its oversized and unparsable-patch refusals no longer contain a run of spaces.

**BranchBoxCLI (SW-1 findings)**
- **Legacy teardown sends `--force` for generated-only worktrees.** In legacy mode the first attempt carries `--force` when the re-plan just taken has:
  - a readable, untruncated status;
  - no user changes, no lock and no blocker;
  - at least one generated or preserved file.

  0.13.4 then removes those files with `git worktree remove --force` instead of refusing over its own `.devcontainer` files, or deleting the folder by `remove_dir_all` (the red "manual removal" warning). This amends §6.5 step 6, "the first attempt never carries `--force`".
  - The data-loss window is unchanged. Without `--force`, 0.13.4 deletes the folder anyway once `git worktree remove` fails. The app preflight is the guard either way.
  - A `moduleFilesDirty` refusal can still happen (module files changed between the preflight and the CLI's own check), and its recovery is unchanged.
  - CLISmokeTests now asserts one-attempt teardowns with no manual-removal warning, in both modes. That includes a project with BranchBox's `init` `.gitignore`.
- **The false spec-loss warning is removed.** The app preflight no longer warns that 0.13.x loses the spec: 0.13.4 moves it to `<main>/docs/features/backlog/` (the smoke test asserts this). The R2 "decision for the DESIGN owner" from the SW-1 report is withdrawn, and R2 "preserved" stands.
- **`feature start` sends `--prompt=<text>` as one argument**, so a prompt beginning with `-` (a pasted bullet list) is not read as a flag. Redaction renders it as `--prompt='<redacted N chars>'`. Golden argv tests use the `=` form.
- **Exit-2 failures use clap's usage line.** A failure with no anyhow `Error:` line takes clap's `error: …` line as `Diagnostics.summary` (prefix removed). CLIProbe failure reasons quote an `Error:`/`error:` line, else the last non-tracing stderr line, else only the status: never an INFO line.
- **`GitInspector.status(of:)` checks the work tree first.** It runs `git rev-parse --show-toplevel` and refuses (`.commandFailed`, so the preflight's `status_unavailable` blocker) when the folder is not its own work tree. Otherwise a worktree with a missing `.git` inside another repository would report that repository's empty status.
- **`RedactedCommandLine` gains `init(secrets:tokens:)`.**
  - Extra-environment values shorter than 4 characters are no longer redacted (`DEBUG=1` left `1` unusable in "Copy as Command").
  - Tokens are always redacted, whatever their length.
- **Branch-step summaries.** A cancellation during the legacy app branch step returns `BranchOutcome.kept` with a warning; the worktree is already gone. `TeardownPreflight.summary(of:)` names every `BackendError` case in words, never as an enum dump.
- **`CLIBackend.identity()` is documented.** The backend keeps the mode of the identity it was bootstrapped with. `EnvironmentStore.recheckIfStale` rebootstraps on activation when the last bootstrap is more than 5 s old, which picks up a replaced binary.

**BranchBoxStores (SW-2 findings)**
- **The registry watcher heals itself.** A project wants watching while Settings › Watch project files is on. A watcher that could not start (no `.branchbox` yet, folder missing) is retried on every refresh pass and root check (`validateRoots`, activation, Locate… to the same folder). `branchbox init` from Terminal or a volume mounted later is picked up on the next activation or refresh.
- **Secrets are redacted everywhere they are shown.** Extra-environment values (≥ 4 characters) and the request's token become `<redacted>` in the in-memory `LogBuffer`, the record's warnings and `OperationSummary.detail` in `operations.json`, as in the archive.
- **`prepareForTermination()` stops new work first.** It sets a terminating flag, so no refresh, after-operation refresh or `didInitialize` add runs. `dispatch` returns `.rejected(reason: "BranchBox is quitting")`. `terminateAllProcesses` gets the rest of the 10 s budget instead of a fixed half.
- **A legacy prune stopped mid-row adds the D-18 note.** Its note becomes "Stopped after N of M features. Stopped while tearing down; a feature may be partly removed" (only without `registry-lock` + `write-ahead-start`).
- **Admission rejects a second request on the same branch or stray.** `deleteBranch` of a branch that another unfinished request deletes (another `deleteBranch`, a teardown or prune row with a delete policy and that recorded branch) is rejected with `busyReason`, and so is a second `removeStray` of the same path.
- **`ActionDispatcher.isAppActive` follows the app by default.** `AppModel` sets it to `{ AppModel.isAppActive }`. SW-4 may replace it to add "and the main window is visible", and must still call `appDidBecomeActive()` / `appDidResignActive()`.
- **Storage folders.** Only the released bundle (`AppBundle.isBundledApp()` and bundle id `dev.branchbox.app`) uses `~/Library/Application Support/BranchBox` and `~/Library/Logs/BranchBox`. `BranchBox Dev.app` (`dev.branchbox.app.dev`), `swift run` and `swift test` use the "BranchBox Dev" folders.
- **`start()` order.** `start()` loads `projects.json` and the operation history before the bootstrap, so returning users see their projects while the CLI is located. Root checks, the legacy migration, watchers and timers still wait for the bootstrap. Gate empty-state UI (Welcome) on `hasStarted`, not on an empty project list.
- **Unchanged refreshes stay quiet.** A refresh pass assigns `features`, `strays`, `droppedRecords`, `listWarnings` and `rootExists` only when they changed, so an unchanged listing does not re-render observers.

**Planning and UI kit (SW-3 findings)**
- **`editor.default_agent` is a slug, never shell text.** `HostLaunchPlan.agentChoice(projectDefault:fallback:)` maps "claude"/"codex" to the built-ins and another bare executable name (`[A-Za-z0-9_][A-Za-z0-9._+-]*`, ≤ 64) to that one word. Anything else (spaces, `;`, `$()`, `|`, a leading `-`) falls back to the app setting. `.custom(command:)` text from App Settings is still the user's own shell text.
- **Built-in agents get the prompt after `--`** (`claude -- '<prompt>'`).
- **The custom terminal template expands in one pass**: a substituted `{path}` is never re-scanned for `{command}`.
- **`LogView(lines:archiveURL:firstIndex:revision:)`.** Following is driven by `LogBuffer.revision`, because the line count stops changing at 10,000. Ids are `LogBuffer.droppedLines` plus the position, and `LogFilter.apply(to:firstIndex:)` reports those indices. `OperationProgressView(record:)` passes both.
  - A scroll-wheel or trackpad scroll up over the log pauses following at once (an `NSEvent` local monitor behind the scroll view).
  - Scrolling down while the bottom is visible, the bottom marker reappearing, or Jump to Latest resumes it.
  - **Not verified in a running app** (no GUI in this session); SW-6 must check it by hand.
- **Feature names starting with `-`** are refused by `StartDraft` ("A feature name can't start with “-”"), via the new public `NameRules.unusableSlugProblem(_:)`. Core still accepts them.
- **`StartDraft.makeRequest()` is nil when the current `previewName` answer is invalid**, not only for intrinsic problems. Filler-only titles get "“The and” has no words BranchBox can use for a name (filler words such as “the”, “and” and “feature” are dropped)". `reuse == .retainedRuntime` on a runtime other than sbx is an error.
- **Remediation offers start-based actions only for runtimes the app can start.** `resumeSetup`, `recreateRuntime` and `rerunSetup` are limited to container, sbx and local-vm. `retryRetainedRuntime` is sbx only: core's `--reuse-runtime` and `--keep-runtime-on-failure` are sbx-only. In-guest and unknown runtimes get Tear Down (or Show Log for failed modules).
- **A running setup never gets a cleanup.** A setup still in progress is checked before "Folder missing", so a live start shows "Setting up…" and is never offered a forced cleanup.
- **Truncated change lists.** When `plan.changes.truncated`, the discard retry is labelled "Discard more than N changes and tear down…". Its confirmation adds that the list is truncated and other changes are deleted too.

### Swift (SW-1): BranchBoxCLI

**Environment and locating the CLI**
- **The interactive capture runs `<shell> -l -i +m -c`** for zsh, bash, sh, ksh, mksh and dash. Job control is off so an app launched from a terminal does not stop its own shell. Other shells (fish, tcsh, nu) keep `-l -i -c` and may time out after 8 s in a terminal-launched dev build.
- **Cache formats.** `environment-cache.json` is `{"PATH", "captured_at"}` only. `cli-probe-cache.json` holds the 16 most recent entries, keyed by path, inode, mtime and size.
- **Capture failure.** When both captures fail, the base is the process environment. The cached PATH only seeds the provisional `.read` environment.
- **First launch waits for the capture.** A first launch (no remembered PATH) waits for the login-shell capture (1–2 s, at most 8 s + 5 s) before locating the CLI, so the D-8 order holds. Later launches search the cached PATH at once.

**Process output**
- **Escape-only lines** (a cursor move between redraws) are dropped. Blank lines are kept.
- **Long lines.** A line over 16 KiB is cut at a character boundary with `…`, and the rest of that line is dropped.
- **Escalation.** The escalator sends SIGCONT after SIGINT and after SIGTERM. `terminateAll` caps those stages at 5 s and 3 s so its 10 s bound holds.
- **A timeout that fires after the leader exited is ignored**; drainGrace bounds that wait. A cancel during the drain still stops the group.
- **Launch errors name their cause.** The runner checks the executable before spawning, so `.launchFailed` says "No such file or directory", "It is a directory" or "The file is not executable".
- **CLIProbe errors.** CLIProbe falls back to `--version` only on exit 2 or undecodable exit-0 output. Any other failure is `.cliUnusable` with a cause.

**Teardown and git**
- **Contract-mode teardown flags** (`--delete-branch`, `--force-delete-branch`, `--discard-changes`) are gated on `teardown-discard-changes`, and the CLI dry run on `teardown-plan`. A contract CLI without them gets the legacy flags, the app preflight and the app branch step.
- **Extra in-app refusals.** A locked worktree or unreadable status is refused in the app unless `forceRemoval`. The forceRemoval guard runs first, and step 2 applies after it.
- **Envelope mapping.**
  - `teardown_refused` with `changed_anything: true` becomes `.partial(completed:, remaining:)`.
  - The teardown cause comes from the first blocker. `not_a_worktree` and `spec_not_preserved` map to `RefusalCause.other(code:)` with the CLI's message; SW-6 shows the message.
  - `registryCorrupted` is also matched from 0.13.4's "Failed to parse feature registry".
- **`Diagnostics.summary`** is the `Error:` message without the prefix.
- **Upstream and branch lookups.** Upstream detection uses `for-each-ref --format='%(upstream) %(upstream:short)'` and then `rev-parse --verify --quiet <upstream>^{commit}`. Branch listing uses `--format=%(refname)`.
- **Name preview.** `name validate -- <in>` and `name generate -- <in>` use `--`. The branch and path preview use the config's `branch_prefix` (default `feature`).
- **Worktree lookup.** devcontainer actions, status and exec with `.devcontainer` find the worktree through `feature list --json` (fallback `<parent of main>/<name>`). exec uses the `.mutation` environment.
- **Strays.** `removeStray` on a stray whose folder is gone runs `git worktree remove --force` (bookkeeping only). A present, dirty stray is refused unless `discardChanges`.
- **Legacy init** derives `reorganized` from `<folder>/main/.git`.

**Doctor and API**
- **HostToolProbe.** git and Docker (CLI and daemon) are required. Missing op, gh or sbx is `.skipped`, and a missing devcontainer CLI is `.warn`. Each probe is limited to 5 s.
- **Additive public API (rule e)**:
  - Process: `ProcessRunner.liveRunCount`.
  - Environment: `EnvironmentProvider.Configuration`, `configuration`, `startCapture()`, `setCLIPath(_:)`, `baseEnvironment(for:)`.
  - Locator and probe: `CLILocator.Outcome`, `locate`, `resolve`; `CLIProbe.forgetCachedResults()`; `FileSystemProbing`/`LocalFileSystem`/`FileKind`/`FileIdentity`/`ApplicationSupport`; public `LoginShellEnvironment` and `ChildEnvironment`.
  - Backend: `CLIBackend` (`executable`, `currentIdentity`, `partialWorktreeNote`), `CLIBackendBootstrapper`, `CLICommand` (+`TeardownMode`, and `onlyGeneratedChanges:`), `CLIErrorClassifier` (+`clapError(in:)`), `TracingLineParser`, `PhaseMapper`, `TextParsers`, `HostToolProbe`, `RedactedCommandLine` (+`tokens:`).
  - Git: `GitInspector`, `GitStatusEntry`/`GitStatusParser`, `WorktreeEntry`/`WorktreeListParser`, `BranchMergeState`, `WorktreeChangeClassifier`, `ChangeContext`, `ChangeTree`, `ChangeItem`, `FileSystemChangeContext`, `StrayDetector`.
  - Test support: `BranchBoxTestSupport.StaticEnvironment`.
  - `TempRepo.make(ignoring:)` and `TempRepo.branchBoxIgnores`, for the gated suites.

### Swift (SW-2): BranchBoxStores

**Queueing and refresh**
- **D-16: project-wide conflicts are queued**, not rejected. A project-wide operation waits FIFO behind every mutation in its project, and later mutations wait behind it, shown as `.queued(behind:)`. Only a second mutation of the same feature, an identical request already active, or a request on the same branch or stray (above) is rejected.
- **`ProjectStore.refresh(_:)` waits.** Called during a refresh, it marks the coalesced re-run and returns after that pass. `requestRefresh(_:)` is the non-blocking variant.
- **Preview folders.** Under the preview backend every folder counts as present: the samples are fictional. The registry watcher always checks the real disk.

**Records and history**
- **Persisted history** is `OperationStore.history: [OperationSummary]` in `operations.json` (100 at most). `OperationRecord` itself is not persisted.
- **Prune progress.** Prune emits `.phase(.item(index:of:name:))` with a 1-based index. Rows never started are `.skipped("Not started: the prune was stopped")`, the row in flight is `.cancelled`, and the record ends `.cancelled(note: "Stopped after N of M features")` with the partial result.
- **Result mapping.**
  - exec with a non-zero inner exit is `.succeededWithWarnings`.
  - sync with failed rows is `.partial`.
  - `BackendError.partial` is `.failed(error)` (RecoveryPlanner reads its details).
  - Progress warnings make a success `.succeededWithWarnings`.
- **Cancelling a queued operation** finishes it `.cancelled(note: nil)` without a refresh. A running one always gets exactly one refresh.
- **After a successful init** (not a dry run), the project is added if missing (from `report.workspacePath`), its watcher started and it is refreshed.

**Projects and migration**
- **Legacy import.** The legacy workspace import drops the key when resolution says `needsInit` or refuses, and keeps it while no backend is available.
- **Display names.** Display names default to the container folder for `…/main` roots ("branchbox"). Titles for sync, config and credentials use that name.

**Additive public API (rule e)**
- `AppModel`: `Configuration`, `init(settings:bootstrapper:notifier:configuration:)`, `hasStarted`, `isAppActive`, `appDidResignActive()`.
- `EnvironmentStore`: `resolution`, `isBootstrapping`, `lastBootstrapAt`, `isRunningDoctor`.
- `ProjectsStore`: `selectedProject`, `setCollapsed`, `rename`.
- `ProjectStore`: `displayName`, `isPinned`, `isCollapsed`, `addedAt`, `lastOpenedAt`, `isRefreshing`, `lastLoadedAt`, `configError`, `detectError`.
- Operations: `LogBuffer.droppedLines`; `OperationRecord.runningSince` and `needsAttention`; `OperationStore.history` and `record(_:)`; `OperationSummary`; `OperationTarget.project`; `OperationRequestContext.operationKind` and `operationTarget`; `ActionDispatcher.isAppActive`.
- `AppSettings`: `recordPrompt`, `promptHistoryLimit`.
- Preview: `PreviewBackend.setResolution` and `currentListing`; `PreviewScenario.dirtyFeatures`, `unmergedBranches` and `with(identity:name:)`; new scenarios (`interruptedSetup`, `strays`, `dirtyWorktree`, `legacyDirtyWorktree`) and `PreviewSamples` (strays, interrupted feature, dirty files).

### Swift (SW-3): Planning, presentation and UI kit

**`RemediationAction` gains three cases**
- `rerunSetup(StartFeatureRequest)`: `start --reuse --devcontainer-reuse preserve`.
- `copyCommand(String, label:)`: `sbx exec <id> bash`.
- `showLog(FeatureRef)`.

Wave-3 switches over the enum must handle them.

**Additive Planning API**
- `RecoveryPlanner.recoveries(for:after:operation:)`; Show Log needs the operation id.
- `Remediation.actions(…branchExists:)` and `Remediation.callout(for:folderExists:)`.
- `PrunePlanner.rows(features:plans:policy:)`, `selection(…consents:)`, `branchPolicy(for:batchPolicy:)` and a public `Row` init.
- `TeardownDraft.preselect(_:)` and the public `feature`/`recordedBranch`/`plan`/`config`.
- `StartDraft` initializers and `notices(existing:branches:)`; `resolvedName`, `branchName`, `currentPreview`, `trimmedPrompt` and `promptLength`.
- `HostLaunchPlan`: the `folderExists:` overloads, `isEnabled`/`disabledReason`, `devContainerURI`/`devContainerDeepLink`/`workspaceFolder(for:)`, `shellQuote`/`shellCommand`/`script(cd:exec:)`, and `sandboxShellCommand`.
- `NameRules.unusableSlugProblem(_:)`.

**Host launching**
- **Missing folders.** Planning is pure. Callers that know the disk state must use the `folderExists:` overloads, fed by `ProjectStore.folderExists(for:)`.
- **Dev Container.** The editor plan opens `vscode://vscode-remote/dev-container+<hex><folder>` (`cursor://` for Cursor), not the raw `vscode-remote://` URI, which no app handles. The workspace folder is `runtime.workspace_folder`, else `/workspaces/<basename>`. A custom editor or a non-container runtime is disabled.
- **`HostLauncher.launch(_:)`** is `async throws` and throws only `HostLaunchError`. A custom terminal template gets `{path}` and `{command}` quoted, and must contain `{command}`.

**Start, teardown and prune**
- **`StartDraft.makeRequest()` leaves `branchPrefix` nil** while it equals the project's configured prefix. The runtime is always explicit.
- **Prune safe set.**
  - In progress, unread or unreadable plans, locked worktrees, user changes, and unmerged rows under Force-delete are excluded.
  - Under Delete-if-merged, active unmerged rows are excluded, but failed_retained and orphaned unmerged rows are preselected and keep their branch.
  - Missing-worktree rows are preselected with `forceRemoval = true`.
- **RecoveryPlanner race case.** A refused retry that already carried consent gets consent for the earlier files plus the new ones, and the confirmation lists only the new ones.
- **TeardownDraft.** When the plan says the branch is gone, the options are `[.keep]`.
- **`ResultCard`** copies `.copyCommand` recoveries in place and does not forward them to `onRecovery`.

### Rust (RS-2): teardown safety

**Messages**
- **0.13 text first.** The text-mode refusal over devcontainer/compose files keeps 0.13's first `Error:` line, with the new refusal as a `Caused by:` line. JSON mode gets the §5.2 "Refusing to tear down …" message.
- **Banner bullets** show the kind (`• README.md (modified)`).
- **Blocker overrides.** `worktree_locked` and `status_unavailable` name `--force`, which `--discard-changes` does not imply. `not_a_worktree` has no override, and `spec_not_preserved` names `--force`.

**What blocks**
- **Deleted tracked files.** At the pre-removal re-check, deleting a tracked file alone does not stop teardown: its content is in `HEAD`. The initial plan still lists it as a user change.
- **Staged entries.** R3–R7 apply only to entries with nothing staged. Staged, conflicted, renamed, absolute or `..` entries are always user changes. R1 applies whatever the state.
- **Unreadable merge state.** `plan.branch` is null with a warning, and no unmerged blocker is raised.
- **A registry `branch_name` that is not a plain branch name** is ignored with a warning, and the config prefix is used.
- **Missing worktree.** A missing worktree's `--dry-run` is a plan (exit 0). A real teardown without `--force` is still `worktree_not_found`.

**Plan and prune JSON**
- **The upstream reference.** `plan.branch.reference` is the full upstream ref; `reference_name` and `upstream` are the short names.
- **prune JSON**:
  - `at_risk.uncommitted_changes[]` carries `{work_feature, count, truncated, paths}`, and `at_risk.unmerged_commits[]` carries `{work_feature, branch, ahead}`.
  - A candidate whose plan failed has `plan: null` plus `plan_error`.
  - An unknown `--feature` is `feature_not_found`.

**Other**
- **The teardown summary.** `summary.registry_updated` is false without a registry entry. `discarded_changes` lists the classified changes only (≤ 2,000), with a warning when more were discarded.
- **git status flags.** `git status` runs with `--no-optional-locks` and `GIT_CEILING_DIRECTORIES`. `setup_vscode_workspace` parses JSONC.
- **Ignored files are still deleted with the worktree** and are not listed in the plan (§6.5: "Ignored files never count"). This covers an ignored nested repository with unpushed commits. Not changed; see Open items.

### Rust (RS-3): CLI JSON surface

**detect and devcontainer sync**
- **detect.** `adapter` is a lowercase id (`generic`, `rails`, `nodejs`), and `project` is `std::path::absolute` (symlinks not resolved). `initialized` means the main worktree's `registry.json` exists. A folder that does not exist is `validation_failed`.
- **sync, dry run.** A dry run does not require the main `.devcontainer`, and a run with no targets exits 0 without one. `synced` also counts `would_sync` rows.
- **sync, failures.** A sync whose baseline could not be recorded is a failed row (`devcontainer_outdated=true`).

**Credentials and config**
- **`tunnel credentials set`.** It also sets `tunnel.providers.cloudflared.account_id`.
  - `--clear` removes the token lines, unsets `api_token_path` and sets `manual_instructions: true`.
  - Tokens with whitespace or over 4096 bytes are refused.
  - A terminal stdin is refused in machine mode.
  - `--clear` creates an empty `.branchbox/` (lock) in a repository that had none.
- **config apply, `changed[]`.** It lists file values (null = not set), not effective values, and registry keys only.
- **config get.** `config get KEY --json` is the full document with `keys` narrowed. `value` falls back to the registry default when the effective value is null. `source` is `file` whenever the file has the key.
- **config types.** Key types are `bool|string|enum|string_list`. `tunnel.default_provider` is an enum of `["cloudflared"]`, and `tunnel_name_prefix`'s registry default is `"branchbox"`.
- **Merge patches.** `null` on a section removes the whole object, unknown keys included. A non-object section value is `config_invalid`, and `version` may only repeat `"1"`. An unparsable patch is `validation_failed`.
- **Unsetting the last key of a section** leaves the empty section.
- **Validators.** `account_id`, `tunnel_name_prefix`, `dns_zone`, `run_services` and the other string keys have their own validators, besides the branch-prefix `check-ref-format`.

**init**
- **`workspace_path`** is absolute (null when unknown), also for the early exits and `--validate`. The early exits detect stack, adapter and modules read-only.
- **1Password references.** `--op-*` references must be one-line `op://…`, also with `--no-verify-op-refs`, and are checked before any change (also in `--dry-run`).
  - `--op-signing-key-ref` and `--no-verify-op-refs` need `--op-github-ref`, and `--skip-1password` conflicts with it.
  - Flags that cannot apply produce a warning naming `--update`.
  - `onepassword.status` is read from `.devcontainer/.env` after the run.
  - `op read` has no timeout (existing behaviour).

**doctor**
- **Required checks:** git, docker.cli/daemon/compose, host.in_container and repo.git/initialized/config/registry. Missing sbx, op, gh or local-vm is `skipped`, and a missing devcontainer CLI is `warn`. `host.in_container` is `warn` under `BRANCHBOX_SKIP_HOST_VALIDATION`.

### Handoff notes for wave 3

**SW-4 (app shell, menu bar)**
- Call `model.appDidBecomeActive()` and `appDidResignActive()` from `NSApplication` notifications. Activation also rebootstraps when stale, which picks up a `brew upgrade`d CLI, and retries registry watchers.
- `actions.isAppActive` already follows `AppModel.isAppActive`. Replace it only to add "main window visible".
- Set `model.projects.selectedProject` from the sidebar selection; it drives the selected-project timer.
- On quit, `prepareForTermination()` refuses new dispatches ("BranchBox is quitting") and spawns no refreshes.
- Show Welcome only when `model.hasStarted && model.projects.projects.isEmpty`: projects load before the bootstrap now.
- The "BranchBox Dev" storage applies to `BranchBox Dev.app` too.

**SW-5 (feature surfaces)**
- Render `Remediation.actions` as returned: start-based actions are absent for in-guest and unknown runtimes, and retained-runtime retries appear for sbx only.
- Agent launch: pass the project's `editor.default_agent` through `HostLaunchPlan.agentChoice(projectDefault:fallback:)`. Never build a command from it yourself.
- Use the `folderExists:` overloads with `ProjectStore.folderExists(for:)`.

**SW-6 (flows, Activity)**
- Use `OperationProgressView(record:)`, or pass `firstIndex: record.log.droppedLines, revision: record.log.revision` to `LogView`.
- Check the scroll-up pause by hand in the running app: scroll up during a busy start, and lines keep arriving without pulling the view down.
- Teardown refusals can carry `not_a_worktree` and `spec_not_preserved`. Both arrive as `RefusalCause.other(code:)` with the CLI's message, and `RecoveryPlanner` offers no recovery for them yet. Show the message.
  - For `not_a_worktree`, never offer a forced retry.
  - For `spec_not_preserved`, the CLI names `--force`. A forced-removal retry is acceptable only behind an explicit confirmation that the spec will be lost.
- `plan.warnings` from the app preflight no longer include a spec-loss warning.
- Legacy teardowns of fresh features succeed on the first attempt (no "Discard BranchBox-generated files" round trip), but the `moduleFilesDirty` recovery must stay.
- Start sheet: show `StartDraft.validationErrors`. A leading `-` and an invalid `previewName` answer block Start, and `makeRequest()` is nil then.
- A rejected dispatch can now say a branch or stray is busy (`busyReason`).

**SW-7 (projects, settings, diagnostics)**
- `config apply`, `config set` and `tunnel credentials set` work on configs with `"cloudflared": null`.
- `init` on a project whose `.branchbox/` holds only `config.json` performs a full init.
- `init --validate --json` reports the real stack.
- Doctor reports are bounded at about 3 s per check on contract CLIs. `HostToolProbe` probes take 5 s each.
- "Copy as Command" leaves extra-environment values shorter than 4 characters visible and redacts tokens always.
- `BranchBoxAppTests` that create an `AppModel` must use `AppModel.Configuration.isolated(in:)`.

### Open items (not fixed in wave 2)
- **Ignored files are deleted with the worktree without a mention** (RS-2 F4), including ignored nested repositories with unpushed commits. This follows §6.5's "Ignored files never count" and 0.13's behaviour; changing it needs an owner decision.
- **Names starting with `-` reach the CLI unescaped.** `feature start`/`teardown` argv does not put `--` before the positional name, which `exec`'s own `--` makes unsafe to add generally. The app refuses such names at Start. A registry entry named `-x` (made with the CLI) cannot be torn down from the app.
- **The legacy `--force` race.** With `--force` on a generated-only worktree, a change made between the app's re-plan and 0.13.x's removal is deleted. 0.13.x deleted such changes anyway through its `remove_dir_all` fallback; contract CLIs re-check themselves.
- **Unverified toolchain.** The Swift 6.0.3 / Xcode 16.2 compile is still unverified (only 6.2.4 is installed). New code avoided typed throws, `sending`, and `didSet` on observed properties; `MainActor.assumeIsolated` (macOS 14) is used in the log view's scroll monitor.
- **Shared `$TMPDIR`.** Core tests that create worktrees at fixed `$TMPDIR/<name>` paths can collide when two runs share `$TMPDIR`.

## Implementation deviations (wave 3)

Wave 3 delivered SW-4 (app shell, menu bar), SW-5 (feature surfaces), SW-6 (flows, Activity) and SW-7 (projects, onboarding, settings, diagnostics). The integrated tree builds with `-warnings-as-errors` on Swift 6.2.4 and passes `swift test --parallel` (660 tests in 77 suites). The gated `CLISmokeTests|GatingTests` pass against both the contract CLI and Homebrew 0.13.4. All render suites pass (146 screens, light and dark).

### Deviations from the design

**Shell (SW-4)**
- **Activity popover.** The toolbar's Activity popover is SW-6's `ActivityPopover`: running operations plus the 5 most recent, then [Show All]. SW-4's interim `RecentActivityList` was removed.
- **`.showActivity(operation:)`.**
  - An operation on a feature or project selects it and opens the inspector.
  - A global, unknown or past-only operation opens the Activity window, after `ActivitySelection.select(id)`.
  - `ActivitySelection` uses the same defaults domain as `AppSettings.defaultsForCurrentProcess()`.
- **Selections.** Selecting a sidebar row does not call `markOpened`. With projects but no selection, the first project is selected. If the selected project is removed, the selection moves to the first project, or to Welcome when none are left. Until `hasStarted`, the detail shows a neutral spinner, not Welcome.
- **Unregistered worktrees.** Selecting one shows a detail with [Review Worktree…] instead of opening the sheet, so arrow-key navigation never pops sheets.
- **Quit confirmation (D-18).** The app activates before showing it. "Keep Running" is the Return default. "Cancel and Quit" is destructive and has no key equivalent. Quitting with nothing running still runs `prepareForTermination`.
- **Notifications.**
  - A banner is suppressed only when the app is active *and* the main window is visible. Otherwise the banner shows even while Settings or Activity is frontmost.
  - The notifier becomes the delegate when it is built at launch.
  - Notes carry their intent in `userInfo` (operation UUID or selection JSON), so a click after a relaunch still routes.
  - With no recoverable intent, a click opens the main window.
- **View menu.** It has a single "Show Inspector" item (⌥⌘I). The built-in `InspectorCommands` was dropped to avoid a duplicate.
- **⌘⌫ Tear Down….** Typed while a text field is first responder, ⌘⌫ is handed back to the field editor as `deleteToBeginningOfLine`. Choosing the menu item with the mouse still tears down.
- **Dock icon.** `settings.showDockIcon == false` (SW-7's General tab) is applied at launch with `.accessory`, but only while the menu bar icon is shown.
- **Sidebar.**
  - Accessibility identifiers use the project path: `sidebar.project.<path>` and `sidebar.feature.<path>.<name>`.
  - A running operation keeps the status word ("Degraded · Tearing down…").
  - The menu bar's "Folder missing — Locate…" opens the Locate panel after showing the project.
- **DEBUG teardown item.** The DEBUG-only "Debug: Tear Down (keep branch)" sidebar item stays for M1 smoke checks. Release builds use `FeatureActionsMenu`'s Tear Down….
- **Find (⌘F)** relies on `.searchable` and `TextEditingCommands`. Programmatic search focus needs macOS 15.

**Feature surfaces (SW-5)**
- **Layout.** Editor, Terminal, Launch Agent and Open URL live in the detail header. The window toolbar keeps Run Command… and More.
- **Re-run Setup….** It is now confirmed (`confirmThenDispatch`), as §9.1's ellipsis requires. Clean Up (missing folder) still dispatches without a confirmation.
- **Recoveries and dispatches.**
  - Every recovery button on feature surfaces (detail, Environment, Sharing, Run Command) goes through `HostLaunchFeedback.perform` → `FlowActions`. Terminal, Finder, Activity, Diagnostics, Refresh and Retry all work.
  - A failed launch or a rejected retry is reported inline.
  - A rejected or unavailable dispatch from the callout, cards or menus shows a toast (`FeatureCommands.report`).
- **Card failures.** Dismissing a failure in a card acknowledges its record, and acknowledged failures no longer show.
- **Environment card.** A container feature whose folder is missing (or which was removed) shows "Unavailable" with a reason instead of an endless spinner.
- **Delete Branch… for removed features.** Branch existence is re-read after a Delete Branch of that branch finishes.
- **Run Command output.** The view shows the last 256 KB of each stream, starting at a line start. Copy and Save… keep the full text, up to the 32 MiB cap.
- **Sharing ▸ Share via Tunnel** is disabled when the project config turns tunnels off.
- **Editor chevron.** The menu is disabled when none of its entries can run.
- **Accessibility.** Fact rows use `.contain`, so Copy and Reveal stay separately focusable under VoiceOver.
- **Run Command history** is kept under the defaults key `runCommand.history.<project>#<feature>`, not in `AppSettings`.

**Flows and Activity (SW-6)**
- **Prune Stop.** Prune offers a confirmed [Stop…], not [Stop After Current]. No stores API exists for "finish this row, then stop".
- **Stopped or failed prunes.** A prune stopped while queued, or failed before any row ran, shows a finished state with Close. It no longer stays on the running layout.
- **Prune checks.** The sheet re-runs `loadPlans()` on appear, skipping rows already planned, and offers [Check Again] for rows whose check failed.
- **Retries in result views.**
  - A branch-delete retry from the Teardown result's card is adopted (`TeardownFlow.adoptBranchRetry`), so its progress and outcome show.
  - A prune row whose retry is refused again shows its cause card.
- **Activity acknowledgement.** A failure that happens while the operation is open in the inspector or Activity window is acknowledged at once.
- **Teardown sheet.**
  - Its "folder already gone" notice now says a confirmation may follow. The first attempt still never forces removal (D-11).
  - A locked worktree is explained before Tear Down.
  - Cancel is not the default-focus button.
- **Start sheet.**
  - With no available projects, it shows a spinner until `hasStarted`.
  - It tells "no projects" apart from "your projects' folders are missing".
  - Advanced options are remembered under `flows.startAdvanced.<project>`, not in `AppSettings`.
- **Destructive styling.** Secondary destructive buttons (Force-Delete Branch…, the spec override) and `ResultCard`'s destructive recoveries use `.foregroundStyle(.red)`. On macOS `.tint`/`role` alone does not colour a bordered button.

**Projects, settings, diagnostics (SW-7)**
- **Project detail.** It reloads `detect` and `config` when an `initProject`, `applyConfig` or `tunnelCredentials` operation on the project finishes, and on the header's refresh. A just-set-up project therefore leaves the "not set up" state.
- **Toolbar.** Start, Prune and Update All Workspaces are disabled on a project that isn't set up ("Set up BranchBox in this project first").
- **Moving the repository (D-21).** With "Move into a parent folder", Set Up stays disabled until a dry run of the identical request succeeded.
- **Project Settings.**
  - The tunnel token is sent only after the config patch it depends on succeeded.
  - The typed token is kept if the patch is refused, and cleared once the credentials operation is admitted.
  - The branch-prefix footer says an empty prefix falls back to the default ("feature"). The CLI rejects `""`.
- **Update All Workspaces.** It offers a confirmed Stop (⌘.) while running. Switching Copy/Link clears a stale preview.
- **Tools tab.** Extra-environment rows with an invalid name show "Not used: …" instead of being dropped silently.
- **Doctor checklist.** Copy Command fixes show the command they copy.
- **Add Project.** [Add Anyway] for an uninitialized repository is omitted: `ProjectsStore` has no API for it.
- **General tab.** The Dock-icon choice lives under `settings.showDockIcon`. "Attention count" and "open last project" are not shipped.
- **Ownership.** `ProjectsSettingsRenderTests.swift` was accepted as owned by SW-7 at integration.

**Tests**
- **Render waits.** Render-suite wait deadlines were raised to 30 s. The integrated `--filter RenderTests` run shares the main actor across four suites, and the 5 s waits timed out.
- **New test.** `FlowTests` gained `aQueuedPruneThatIsStoppedFinishesWithoutRowStates`.

### Handoff notes for wave 4

**PK-1 (packaging, CI)**
- **Dev bundle.** `scripts/macos-dev.sh` builds `BranchBox Dev.app` with an inline Info.plist: `dev.branchbox.app.dev`, LSMinimumSystemVersion 14.0, ad-hoc signed with the hardened runtime. Release packaging needs:
  - the real bundle ID;
  - Developer ID signing and notarization;
  - an icon set;
  - `NSUserNotificationsUsageDescription`, if Apple requires it.
- **Notifications.** `UserNotificationNotifier` is used only inside a real `.app` (`AppBundle.isBundledApp`). Under xctest and `swift run`, a `NoopNotifier` is used.
- **CI.** Add the Swift 6.0.3 / Xcode 16.2 leg, which no wave has compiled yet. Risk points:
  - the async `UNUserNotificationCenterDelegate` methods;
  - the `NSImage` drawing-handler closure;
  - `nonisolated(unsafe) static var` in `ActivitySelection`;
  - `.onChange` with two parameters.
- **Running the suites.** Render tests run only with `BRANCHBOX_RENDER_DIR` set; keep them out of the default CI job or run them serialized. Gated real-CLI tests need `BRANCHBOX_IT=1 BRANCHBOX_IT_CLI=<path>` and a disposable `TMPDIR`.

**DOC-1 (docs)**
- **To document:** the menu bar (four icon states), Quick Open (⌘K), the Activity window, the quit prompt and the notification behaviour.
- **Dev preview.** In DEBUG builds, `BRANCHBOX_BACKEND=preview BRANCHBOX_PREVIEW_SCENARIO=<name>` runs a preview backend. `showcase` is the screenshot scenario.
- **Copy.** User-facing copy uses "the branchbox tool" and avoids "control plane".

**VER-1 (verification)** — manual checks nobody could run, because launching the app was not allowed:
- Dock reopen and menu bar "Open BranchBox" with the main window closed (and with the menu bar icon hidden).
- ⌘N in the key window.
- Menu bar Tear Down… with the window closed.
- The quit prompt with a running operation, then `pgrep branchbox`.
- PATH under launchd through `scripts/macos-dev.sh --open`.
- ↑/↓ in Quick Open's field (`onKeyPress` on a TextField, macOS 14).
- ⌘⌫ in the sidebar filter clears the line rather than opening Tear Down.
- Run Command `sh -c 'exit 3'` and a multi-MB output.
- Dev container Start/Stop on a compose repo updating the Environment card.
- Log auto-scroll, pause and Jump to Latest with a 2,000-line operation.
- Real-CLI flows on disposable repos with 0.13.4 and the contract CLI:
  - untracked file → refusal → discard;
  - unmerged branch → force-delete;
  - prune with a dirty row;
  - cancelled start → stray removal;
  - `git init` → Add → Set Up in place;
  - legacy read-only config;
  - `branch_prefix` saved and confirmed by `config get`;
  - Settings › Locate… switching the CLI live;
  - Diagnostics with Docker down.
- Primary-button prominence and red destructive styling in a key window. Offscreen renders are never key.
- Composed main-window screenshots (sidebar List and toolbar, which render blank offscreen) for the PR, from `scripts/macos-dev.sh --open --preview showcase`.
- Suggested automation: an `AppModel` + `CLIBackendBootstrapper` + `ActionDispatcher` start/teardown test in `BranchBoxIntegrationTests`, to automate milestone M1.

### Open items (not fixed in wave 3)
- **Stores APIs that don't exist yet:**
  - `OperationStore.stopAfterCurrent` (prune);
  - `ProjectsStore.add(folder:allowUninitialized:)` (Add Anyway);
  - `AppSettings` fields for Start advanced memory, Run Command history, `showDockIcon`, attention count and open-last-project.
- **Missing backend reads:** the last commit subject (Overview card) and the commit list for the force-delete confirmations.
- **Recovery wording.** Recoveries performed from feature surfaces open the Activity window for Show Log, rather than selecting the feature in the inspector.
- **Notification toast.** A note that arrives while the main window is visible but is not key still becomes a toast.
- **Host launch failures** from the menu bar with no main window only beep.
- **Duplicated "Update All Workspaces…"** appears in both the health callout and the Environment card's outdated row (SW-5 F11). Left as is.
- **`ErrorBanner` Retry.** `ErrorBanner` offers Retry only for transient errors (`BackendError.isTransient`). This is a shared-kit behaviour change other screens inherit.
- **Swift 6.0.3 compile** is still unverified.
