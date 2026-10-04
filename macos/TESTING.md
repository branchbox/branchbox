# Testing the BranchBox Mac app

How the Mac app is tested, and the record of the latest verification run. Update the "Latest run" and
"Mac App ↔ CLI Loop" sections whenever you run them (AGENTS.md asks for this before a PR that touches the
app, the CLI's `--json` output, teardown, prune, `config`, `init` or the registry).

## Test layers

| Layer | Command | Runs |
|---|---|---|
| Unit (all targets) | `swift test --package-path macos --parallel` | Always. Integration and render suites skip themselves. |
| Contract fixtures | part of the unit run (`ContractFixtureDecodeTests` in BranchBoxKitTests) | Always. Decodes every file in `cli/tests/fixtures/contract/**` with the app's models; a new fixture without a mapping fails. |
| Render | `BRANCHBOX_RENDER_DIR=<dir> swift test --package-path macos --filter RenderTests` | Offscreen PNGs, light and dark. Keep out of parallel runs. |
| Integration (real CLI) | `BRANCHBOX_IT=1 BRANCHBOX_IT_CLI=<path to branchbox> swift test --package-path macos --filter BranchBoxIntegrationTests` | Disposable git repos, no Docker. Run once with the released 0.13.4 CLI (legacy mode) and once with the branch-built CLI (contract mode). |
| Live fixtures | `scripts/macos-capture-fixtures.sh <cli> <dir>`, then `BRANCHBOX_LIVE_FIXTURES=<dir> swift test --package-path macos --filter LiveFixtureDecodeTests` | Decodes what a real CLI prints; keys the app never reads are printed as `::warning::` lines. |
| Packaging | `scripts/package-macos-app.sh --native` then `codesign --verify --deep --strict --verbose=2 macos/build/BranchBox.app` | The script fails on its own if any packaging check fails. |
| Manual loop | "Mac App ↔ CLI Loop" in `docs/docs/getting-started/manual-cli-e2e.md` | A person, on a Mac, with the packaged app. Results go below. |

Integration suites put their repositories in `$BRANCHBOX_IT_TMP` (default: the test process's temporary folder;
SwiftPM gives the test process its own `TMPDIR`, so set `BRANCHBOX_IT_TMP` rather than `TMPDIR`). Every test removes
its repository in `defer`, and containers older than an hour from a crashed run are swept on the next run.

### What the integration suites cover

All VER-1 suites are nested in one serialized `RealCLI` suite, so their time limits do not compete for the CPU.
Each test reads the CLI's identity and asserts what that mode must do.

| Suite | Covers | Legacy (0.13.x) | Contract |
|---|---|---|---|
| `CLISmokeTests` (SW-1) | identity, empty list, minimal start, exec, one-attempt teardown, list `--all` | ✓ | ✓ |
| `CLIBackendLifecycleTests` | exec ok and exit 3 as data; untracked file → plan lists it → refusal before any teardown spawn (registry unchanged) → discard retry from `RecoveryPlanner` → removed, Keep keeps the branch; unmerged branch blocks Delete-if-merged (TeardownDraft, and the CLI refuses on contract) → Force-delete deletes; `--branch-prefix spike` deletes `spike/<n>`; resolveProject from a worktree and a subfolder; detect | ✓ | ✓ + the CLI's own `teardown_refused` envelope |
| `RefusalTests` | duplicate start → `.worktreeExists`; project without BranchBox ignores tears down in one attempt; 0.13.x's own dirty-module refusal → `.moduleFilesDirty` → "Discard BranchBox-generated files" retry succeeds | ✓ (both parts) | ✓ (first part; contract CLIs never print that refusal) |
| `LargeRegistryTests` | 40 features, `list --json` > 64 KiB, backend list < 10 s, 20 concurrent lists, cancelled readers + `terminateAll` leave no process | ✓ | ✓ |
| `CancellationTests` | sleeping `post-checkout` hook, cancel 2 s in returns < 6 s, the hook and CLI are gone, stray → `removeStray`; contract: fake `sbx` sleeping after the worktree exists → `setup.state == interrupted` → Resume remediation succeeds | ✓ (stray) | ✓ (stray and interrupted → Resume) |
| `ConcurrencyTests` | two starts and a sync dispatched through `OperationStore` with `registry-lock` keep every entry | skipped (no `registry-lock`) | ✓ |
| `PrunePlanningTests` | 3 features, one dirty: PrunePlanner unchecks it; running the selection removes the clean two | ✓ | ✓ |
| `ConfigIntegrationTests` | get → apply (dry run, then real) round trip keeps unknown keys, the next start uses the new prefix; invalid `runtime.provider` → `.configInvalid` naming the allowed values, file untouched; legacy: read-only, apply is `.unsupported` | ✓ (legacy branch) | ✓ |
| `WatcherIntegrationTests` | a start run directly from "Terminal" reaches `ProjectStore.features` within 2 s | ✓ | ✓ |
| `SandboxRemediationTests` | fake `sbx`: failed_retained → Retry + Copy Inspect Command (`sbx exec <id> bash`) → Retry reuses the sandbox → active | ✓ | ✓ |
| `LiveFixtureDecodeTests` | gated on `BRANCHBOX_LIVE_FIXTURES`; plus an ungated self-test of the key recorder | n/a | n/a |

## Latest run: 2026-10-04 (VER-1, automated)

Machine: Apple M4 Pro, macOS 26.5.1 (25F80), Xcode 26.3, Swift 6.2.4, rustc 1.90.0, cargo-nextest 0.9.111,
cargo-llvm-cov 0.6.21. Worktree `feature/mac-app-revamp` at `a00b3ee` plus the uncommitted wave 1–4 changes.

CLIs:
- **Legacy:** Homebrew `/opt/homebrew/bin/branchbox` 0.13.4 (no `version --json`, no capabilities).
- **Contract:** release build of this tree (`cargo build --release --locked -p branchbox-cli`), version 0.13.4,
  `contract_version` 1, all 13 capabilities.

| Check | Result | Notes |
|---|---|---|
| `swift build --build-tests -Xswiftc -warnings-as-errors` | ✅ | no warnings |
| `swift test --parallel` | ✅ | 682 tests in 89 suites; ContractFixtureDecodeTests decodes all 37 contract fixtures |
| Integration, legacy (0.13.4) | ✅ | 22 tests in 13 suites, 17 s. 40-feature list: 92 KB, 0.05 s. Watcher refresh 0.39 s after the CLI wrote. Interrupted/resume and concurrency skipped by design (no write-ahead start, no registry lock) |
| Integration, contract (release CLI) | ✅ | 22 tests in 13 suites, 32 s. 40-feature list: 92 KB, 0.05 s. Watcher refresh 0.45 s |
| Live fixtures, both CLIs | ✅ | every captured payload decodes. Unread keys: `[].last_summary_rendered_at`, `[].tunnel.removed_at` (list), `schema_version` (contract teardown plan) |
| Leftovers after the suites | ✅ | no `branchbox-it-*` folders, no `branchbox feature` process (`pgrep`), `git worktree list` of this repository unchanged |
| `cargo fmt --all -- --check` | ✅ | |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ | |
| `cargo nextest run --all-features` | ✅ | 923 passed, 27 skipped (`--test-threads 4`; run with the default `TMPDIR`, a long `TMPDIR` overflows the agent socket's `SUN_LEN`) |
| `cargo llvm-cov nextest --all-features --workspace` | ✅ ≥ 79% / ❌ 90% | lines 79.14%, regions 79.57%, functions 77.57% (`LLVM_COV`/`LLVM_PROFDATA` from `xcrun`). Above the 79% bar set for this wave and the 74.49% wave-1 baseline; still below the brief's 90% |
| `./scripts/review-preflight.sh` | ✅ | |
| `scripts/package-macos-app.sh --native --zip` + `codesign --verify --deep --strict` | ✅ | `BranchBox-0.13.4-390-a00b3ee.zip` (arm64, ad hoc, hardened runtime); valid on disk, satisfies its Designated Requirement |

## Mac App ↔ CLI Loop (manual) — PENDING

**Status: not run yet.** Launching the app and showing windows was not possible in the verification session,
so every step below is pending for the owner. Run the loop as written in
`docs/docs/getting-started/manual-cli-e2e.md` ("Mac App ↔ CLI Loop", steps 0–13) on the packaged app, once with
each CLI, and fill in the tables (✅ / ❌ with a note, or n/a). Keep screenshots out of the repository; reference
them by file name in the PR.

Run details to record: date, macOS version, app version and build (`BranchBox-<version>-<build>-<sha>.zip`), CLI
path and `branchbox --version` for each run.

| Step | What to check | Contract CLI | Legacy 0.13.4 |
|---|---|---|---|
| 0 | `cargo build -p branchbox-cli`; `scripts/package-macos-app.sh --native --zip`; quarantine removed if from CI | pending | pending |
| 1 | Launch from Finder (launchd PATH): onboarding finds the CLI (Locate… for the branch CLI); Diagnostics lists the CLI, capabilities, doctor rows | pending | pending |
| 2 | Add a disposable `git init` repo → Set Up BranchBox (`init -y`, layout kept) → project appears; repo not moved | pending | pending |
| 3 | Start a minimal feature → live log → result shows the resolved name; `branchbox feature list --json --repo …` lists it | pending | pending |
| 4 | Start a feature from Terminal → the app shows it within about 1 s | pending | pending |
| 5 | Run Command `echo hi`, then `sh -c 'exit 3'` → exit 3 shown, no alert | pending | pending |
| 6 | `touch notes.txt` in a worktree → Tear Down → refusal card names notes.txt, nothing removed → Discard (confirm) → removed; branch per policy (`git branch --list`) | pending | pending |
| 7 | Commit in a worktree → Delete if merged is blocked → Force-delete (confirm) → branch deleted | pending | pending |
| 8 | Prune with 3 features, one dirty → the dirty row is unchecked → per-feature results | pending | pending |
| 9 | Sleeping `post-checkout` hook → start → cancel → confirmation copy → Unregistered worktree row → Remove (see finding 1: contract CLIs also show a stray here, not Interrupted) | pending | pending |
| 10 | Close the main window → menu bar Open BranchBox reopens it; menu bar Tear Down… opens the window and the sheet | pending | pending |
| 11 | Quit during a start → prompt → Cancel and Quit → `pgrep branchbox` prints nothing | pending | pending |
| 12 | Project Settings branch prefix → `branchbox config get feature.branch_prefix --json --repo …` shows it | pending | n/a |
| 13 | `cd macos && swift run BranchBox` starts a feature without crashing (no notifications) | pending | pending |

Additional manual checks handed over by waves 3 and 4 (record each once, with either CLI):

| Check | Result |
|---|---|
| Finder launch with a stripped PATH through `scripts/macos-dev.sh --open` (launchd environment; notifications appear in a real bundle) | pending |
| Dock reopen, and menu bar Open BranchBox with the main window closed and with the menu bar icon hidden | pending |
| ⌘N in the key window; ↑/↓ in Quick Open's field (⌘K); ⌘⌫ in the sidebar filter clears the line instead of opening Tear Down | pending |
| Log auto-scroll, scroll-up pause and Jump to Latest with a 2,000-line operation; Run Command with multi-MB output | pending |
| Project toolbar at the 1100 pt default width (icon-only secondary actions, overflow) and primary-button prominence / red destructive styling in a key window | pending |
| Dev container Start/Stop on a compose repository updates the Environment card; Diagnostics with Docker stopped | pending |
| Settings › Tools › Locate… switches the CLI live (legacy ↔ contract); legacy project settings are read-only with Open config.json | pending |
| Composed main-window screenshots for the PR from `scripts/macos-dev.sh --open --preview showcase` | pending |

### Docker Sandboxes (sbx) remediation

- **Automated (fake `sbx`):** ✅ `SandboxRemediationTests` and `CancellationTests` drive failed_retained → Retry and
  interrupted → Resume through a scripted `sbx` stand-in, in both modes.
- **Real sbx degraded / failed_retained: unverified.** `sbx` is installed on the verification machine, but creating
  real sandboxes was out of scope for an unattended run. The inspect action ships as **Copy Inspect Command**
  (copies `sbx exec <id> bash`; never runs it), as the brief requires. Open item for SW-5's owner: run a real sbx
  feature into failed_retained (`--keep-runtime-on-failure` with a broken dev container) and degraded, and confirm
  Retry and Copy Inspect Command from the feature detail.

## Findings from the VER-1 run

1. **A cancel during `git worktree add` leaves a stray on contract CLIs too.** The `post-checkout` hook runs inside
   `git worktree add`, before core writes its write-ahead entry (which needs the worktree to exist), so a start
   cancelled there leaves an unregistered worktree in both modes, not an Interrupted record. The listing reports
   the stray and Remove cleans it (tested). But because contract CLIs advertise `write-ahead-start` and
   `registry-lock`, `CLIBackend.cancelNote` and `ActionDispatcher.cancellationNote` drop the "a partial worktree may
   be left behind" note on them. Follow-up for SW-1/SW-2: keep the note for a start cancelled before the CLI
   printed its "Created worktree" line, or always for starts. DESIGN §13.4 step 9 already allows either outcome.
2. **SwiftPM replaces `TMPDIR` for the test process**, so `TMPDIR=… swift test` does not move the TempRepos. The
   CI integration jobs now set `BRANCHBOX_IT_TMP` instead.
3. **0.13.x's dirty-module refusal is no longer reached by the app's own teardown** (since wave 2 the app sends
   `--force` for generated-only worktrees). `RefusalTests` therefore provokes the refusal with the CLI directly and
   checks the classification and the generated-files recovery against it.
4. **Doubled prefix in core's sbx failure message:** "Validation error: Validation error: Docker Sandboxes could not
   start the devcontainer …", coded `validation_failed`. Cosmetic; for the RS owner.
5. **Unread CLI keys** (from `LiveFixtureDecodeTests`): `last_summary_rendered_at` and `tunnel.removed_at` in list
   records, and `schema_version` in the contract teardown plan. None is needed by the app today.
