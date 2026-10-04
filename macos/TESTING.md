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

## Earlier baseline: 2026-10-04 (VER-1, automated)

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

## Latest component audit and live review: 2026-10-04

App/runtime source: `f637b8f` on `feature/mac-app-revamp`; website and audit-record changes are documented separately.

The packaged **BranchBox Dev** app was exercised with the branch-built 0.13.4 CLI (contract version 1,
13 capabilities), using a disposable Git repository and an image-only `python:3.12-alpine` devcontainer.
This was a real CLI backend, not the showcase preview backend. Subsequent CLI/Docker retests used the
final source build with 14 capabilities, including `host-container-teardown-verified`. The development
bundle's signature passed `codesign --verify --deep --strict`.

| Component | Observed result | Scope |
|---|---|---|
| Add project / detection | Git repository appeared in the sidebar with the Generic stack; a tracked Rails source snapshot detected Rails | Native UI + real CLI |
| Initialize project | Preview left `git status` empty; Apply created setup files and kept the existing folder layout | Native UI |
| Start feature | Quick resolved the title to a feature name/branch/folder, created the worktree, and reported four skipped modules | Native UI + CLI list |
| Devcontainer sync | Preview listed one workspace; Apply updated it and cleared the outdated-config warning | Native UI |
| Container lifecycle | Start showed Running; Stop, restart and confirmed Rebuild succeeded; Docker inspection verified the new container IDs | Native UI + real Docker, disposable container |
| Command runner | Python ran in **Dev container** and returned stdout/exit 0; a shell command returned exit 3 and its stderr without a transport-error alert | Native UI + real Docker |
| Teardown safety | A modified README caused refusal before removal; the file and running container remained. An unmerged commit selected Keep; Delete-if-merged disabled Tear Down | Native UI; discard/force execution covered by CLI integrations |
| Prune | A dirty feature was unchecked; a clean feature with an unmerged commit remained selectable under Keep | Native UI planning; execution covered by integrations |
| External CLI changes | A feature created directly by the CLI appeared without pressing Refresh | Native filesystem watcher |
| Navigation / settings | Quick Open search opened Diagnostics; General, Tools, Coding Agent and Refresh tabs were inspected | Native UI |
| Diagnostics | CLI capabilities and installed-tool/runtime rows rendered; Run Checks Again refreshed their timestamp | Native UI |
| Existing worktree health | Three Active registry records had folders with broken `.git` pointers. The revised app showed attention badges and the missing metadata path, disabled Git-dependent actions, and retained folder/editor access | Read-only existing Rails checkout + real temporary-repository regression |
| Standalone Stop / volumes | Two exact owned containers were removed each time; default Stop retained anonymous volumes. Explicit deletion removed attached anonymous volumes and preserved a shared named volume and neighboring container | Real Docker, disposable image containers |
| Image / Dockerfile feature teardown | Dirty README refusal preserved the worktree and both owned containers. After committing, Keep removed both containers and the worktree, retained the branch, and preserved the neighbor | Real Docker, both active configuration types |
| External Compose Stop / teardown | An external Dev Containers project used the `_devcontainer` suffix and `/tmp` alias. Stop kept its data; restart recovered the marker; explicit deletion removed its volume. A later feature teardown recovered stopped-project ownership and removed retained volumes; neighbor survived | Real Docker, isolated internal networks; no application services |
| Native Compose Start / restart | BranchBox installed stable ownership labels; Stop kept the data marker across native restart; feature teardown removed all owned containers, networks and volumes | Real Docker through the same CLI used by the app |
| Shell / active configuration | Root user and workspace came from the active config rather than unused scaffolds. The exact shell launch command opened `sh` in Alpine when Bash was unavailable | Real Docker PTY; opening the macOS Terminal window remains unverified |
| Runtime / module contracts | 288 CLI, 695 core unit, 5 agent, 3 workflow and 17 doc tests passed | 1,008 executed cases; fake-tool contracts are not live service verification |
| Individual view renders | All 45 gated render declarations passed, producing 296 private light/dark PNGs including added broken-Git and whole-window feature samples; selected Start, Teardown, feature and health states were visually inspected | Offscreen smoke/visual review; native vibrancy/titlebar composition is not reproduced |

The final Cargo suite reports 993 passed declarations and 27 ignored cases; two opt-in Docker smoke tests
return early and are excluded from the 991 executed workspace cases. The 17 doc tests bring the executed Rust
count to 1,008. The live Docker checks in this table did execute. Swift's final full run reported 700 registered
cases in 92 suites: 632 enabled unit/component cases and 68 disabled render/integration/live-fixture cases.
Separate final real-CLI runs reported 24 declarations each: contract executed 23 (one gated fixture), while legacy
executed 21 (one gated fixture and two capability early returns). Each includes two support cases. The new
cleanup regressions include 19 host/feature declarations (14 cleanup cases and five shared helper cases), ten
Down declarations with failure variants, and five runtime unit cases.
The final health integration exercised three real Git scenarios with each CLI: missing administrative metadata,
missing `commondir`, and a healthy unborn/orphan branch that must remain usable. The last scenario prevents a
false warning based only on Git's zero HEAD hash. Sync Preview remains available on damaged rows, while Apply
and tunnel provisioning wait for inspection; tunnel removal remains available.

The audit found and fixed real cleanup defects: standalone feature teardown previously left a running
container while returning `verified: true` and `residue_free: true`; Compose Stop assumed a project name;
and partial Compose cleanup could lose its project identity after removing containers. Cleanup now checks
command outcomes and exact ownership probes, persists workspace-scoped project identity before mutations,
retains failed worktrees for retry, and does not infer ownership from a basename. Copied/malformed/symlinked
history, aliases and literal `$` paths have hermetic regression coverage. Force can still remove a workspace
with an explicitly incomplete receipt. Older CLI container receipts are downgraded in the app when the
executing CLI lacks the verification capability. Standalone volumes/custom networks remain outside the
feature-teardown container check.

Formatting, Clippy with warnings as errors, nextest, doc tests, debug/release builds, coverage generation,
and rustdoc with warnings as errors passed. Local line coverage is **79.08%**, below the repository's 90% target.
The guardrail preflight and pretend harness passed. The destructive ignored database/container suite was not
run against the developer's live Docker engine; isolated CI owns that check. Local regular/verbose stack
harnesses and the full agent control-plane stub harness remain unrun, so this is a component audit rather
than release sign-off. The combined website/docs build passed with 184 local references and 88 anchors valid.

Final native package: `BranchBox-0.13.4-429-f637b8f.zip` (arm64), with the same-source release CLI
embedded. Bundle and helper signature checks passed; ZIP SHA-256:
`200ba98879c0dc8dce2900e0eae88fbed9b886a8dbde5e4546f12992163f3daf`. It is ad hoc signed,
without notarization. The locator still prefers an explicitly selected or installed CLI over the embedded fallback.

The existing Rails checkout was not initialized, repaired or used to launch application services. Its pre-existing
schema modification, BranchBox configuration and registry hashes were unchanged. A credential-free archive of
tracked HEAD was tested separately through detect, init preview, minimal start, exec and keep-branch teardown.
That command ran in the host worktree; the Python devcontainer check above supplied the actual Docker test.

Public media use only the disposable workspace and contain no customer source or credentials:

- [Feature overview](../docs/static/img/mac-app/feature-overview.png)
- [Start feature](../docs/static/img/mac-app/start-feature.png)
- [Teardown plan](../docs/static/img/mac-app/teardown-plan.png)
- [Command execution](../docs/static/img/mac-app/run-command.png)
- [Silent walkthrough](../docs/static/media/mac-app-walkthrough.mp4): actual screen capture; main sequence at 2×,
command execution at normal speed; H.264, 1600×1048, 30 fps, 60 seconds. Full decode completed without errors.

**Media quality follow-up:** native captures contain a persistent translucent horizontal band across the feature
detail, obscuring part of the Overview and Environment cards. The standalone overview figure is withheld from
the guide; the recording remains review evidence, not a polished release asset. Feature-only offscreen renders
at 820 pt and 1180 pt widths are clean. This suggests native hosting/material composition, but the cause has not
been established. The Mac locked before resize/scroll/inspector comparisons and the remaining Activity and
Diagnostics captures; those require a manual unlock. No compositor fix is claimed by this audit.

Remaining live checks: real Rails/Postgres/Sidekiq application behavior, external database cleanup, public tunnels,
1Password credential acquisition, SBX/Local VM/in-guest provisioning, OS notification delivery, and the additional
manual-window behaviors below. Registry module outcomes and recorded tunnel state do not establish those results.

## Mac App ↔ CLI Loop (manual) — partial live coverage

**Status: the live component pass above covers part of the contract-CLI loop.** The original VER-1 checklist
below remains a record of the full two-mode loop, including destructive confirmation and quit/cancellation
steps that have not all been performed through the native UI. Run the remaining steps as written in
`docs/docs/getting-started/manual-cli-e2e.md` ("Mac App ↔ CLI Loop", steps 0–13) on the packaged app, once with
each CLI, and fill in the tables (✅ / ❌ with a note, or n/a). Private evidence stays outside the repository;
the explicitly requested, credential-free website screenshots are checked in under `docs/static/img/mac-app`.

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

### Real-window preview check: 2026-10-04

Built `BranchBox Dev.app` with Xcode 26.3 / Swift 6.2.4, verified its signature with
`codesign --verify --deep --strict`, and launched it through LaunchServices with
`BRANCHBOX_BACKEND=preview` and `BRANCHBOX_PREVIEW_SCENARIO=showcase`.
Inspected the running window through accessibility and screenshots:

- Quick Open (⌘K) opened and Return selected the highlighted feature.
- The feature sidebar, interrupted-setup banner, environment cards and icon toolbar rendered clearly.
- Start Feature showed its form, focused the name field and disabled Start until a name was entered.
- Tear Down showed the changes summary, explicit branch choices and removed resources; Cancel returned to the feature.

This checks the showcase layout and presentation flow. Real CLI operations, notification delivery,
other window sizes and the remaining manual checklist still need their own verification.

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
   `registry-lock`, the initial implementation dropped the "a partial worktree may be left behind" note on them.
   **Resolved in the component audit:** start cancellation always keeps that warning, with dispatcher and real
   hook-cancellation regression coverage. DESIGN §13.4 step 9 allows either outcome.
2. **SwiftPM replaces `TMPDIR` for the test process**, so `TMPDIR=… swift test` does not move the TempRepos. The
   CI integration jobs now set `BRANCHBOX_IT_TMP` instead.
3. **0.13.x's dirty-module refusal is no longer reached by the app's own teardown** (since wave 2 the app sends
   `--force` for generated-only worktrees). `RefusalTests` therefore provokes the refusal with the CLI directly and
   checks the classification and the generated-files recovery against it.
4. **Doubled prefix in core's sbx failure message:** "Validation error: Validation error: Docker Sandboxes could not
   start the devcontainer …", coded `validation_failed`. Cosmetic; for the RS owner.
5. **Unread CLI keys** (from `LiveFixtureDecodeTests`): `last_summary_rendered_at` and `tunnel.removed_at` in list
   records, and `schema_version` in the contract teardown plan. None is needed by the app today.
