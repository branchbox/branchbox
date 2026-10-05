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

## Component audit and live review: 2026-10-04

App source: `f63f2c0`; runtime source: `ee39d94` on `feature/mac-app-revamp`. The ignored Compose lifecycle
fixture was corrected at `40027ae`; this changes test setup only. Website and audit-record changes are
documented separately.

The packaged **BranchBox Dev** app was exercised with the branch-built 0.13.4 CLI (contract version 1,
13 capabilities), using a disposable Git repository and an image-only `python:3.12-alpine` devcontainer.
This was a real CLI backend, not the showcase preview backend. Subsequent CLI/Docker retests used the
final source build with 14 capabilities, including `host-container-teardown-verified`. The development
bundle's signature passed `codesign --verify --deep --strict`.

| Component | Observed result | Scope |
|---|---|---|
| Add project / detection | Git repository appeared in the sidebar with the Generic stack; a tracked Rails source snapshot detected Rails | Native UI + real CLI |
| Initialize project | Preview left `git status` empty; Apply created setup files and kept the existing folder layout | Native UI |
| Start feature | Quick resolved the title to a feature name/branch/folder, created the worktree, and reported four skips for the selected configuration | Native UI + CLI list; minimal defaults are devcontainer, compose and specs, with tunnel provisioning controlled separately |
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
| Compose ownership / legacy compatibility | Copied legacy identity into active image and Dockerfile fixtures refused before removing target or foreign Docker resources. A copied workspace binding also refused; restoring each owning record cleaned its target and preserved the neighbor. A stopped older managed Compose project cleaned its retained volume through the exact recorded project | Six real Docker ownership checks; fresh native Compose lifecycle repeated after the fix |
| Shell / active configuration | Root user and workspace came from the active config rather than unused scaffolds. The exact shell launch command opened `sh` in Alpine when Bash was unavailable | Real Docker PTY; opening the macOS Terminal window remains unverified |
| Runtime / module contracts | 290 CLI, 696 core unit, 5 agent, 3 workflow and 17 doc tests passed | 1,011 executed cases; fake-tool contracts are not live service verification |
| Individual view renders | All 45 gated render declarations passed, producing 296 private light/dark PNGs including added broken-Git and whole-window feature samples; selected Start, Teardown, feature and health states were visually inspected | Offscreen smoke/visual review; native vibrancy/titlebar composition is not reproduced |

The final Cargo suite reports 996 passed declarations and 27 ignored cases; two opt-in Docker smoke tests
return early and are excluded from the 994 executed workspace cases. The 17 doc tests bring the executed Rust
count to 1,011. The live Docker checks in this table did execute. Swift's final full run reported 700 registered
cases in 92 suites: 632 enabled unit/component cases and 68 disabled render/integration/live-fixture cases.
Separate final real-CLI runs reported 24 declarations each: contract executed 23 (one gated fixture), while legacy
executed 21 (one gated fixture and two capability early returns). Each includes two support cases. The new
cleanup regressions include 21 host/feature declarations (16 cleanup cases and five shared helper cases), ten
Down declarations with failure variants, and five runtime unit cases.
After the full Swift run, a copy-only follow-up clarified that teardown/prune do not list Git-ignored files and
that keeping a branch does not keep its worktree files. A warnings-as-errors build, all 11 focused flow tests,
and both existing flow render cases passed; 22 additional private light/dark PNGs were inspected for fit.
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

Generated Compose project identities now bind the exact project name to the canonical workspace in the
same managed env write. Legacy recovery requires the matching feature's recorded project and canonical
worktree, or established exact-label/history evidence. Copied/mismatched identity, duplicate fields and
ambiguous legacy identity refuse Docker removal. A failed host module also stops provider destruction so
the target container remains available for inspection. Isolated guest providers retain their verified
runtime-boundary cleanup; the full suite and four targeted guest regressions confirm that path.
Eight image/Dockerfile copy variants and Force's unverified receipt are covered hermetically. Differently
named native/external groups need their retained observed identity after Stop; deleting that history can
make remaining volumes undiscoverable. The live legacy case uses the exact recorded managed project.

Formatting, Clippy with warnings as errors, nextest, doc tests, debug/release builds, coverage generation,
and rustdoc with warnings as errors passed. The final full gate also passed with the developer's Docker
engine blocked through PATH; clean-receipt fixtures use checked, command-scoped empty inventory probes.
Isolated CI then exposed an ignored Compose lifecycle fixture that called the module directly without the
workspace-bound identity written by production's feature workflow. The corrected unique-project fixture
passed a strict fake engine (12 checked calls) and the single real-Docker ignored test. All 14 ordinary
Compose unit cases, scoped Clippy and formatting passed; production ownership validation is unchanged.
On `40027ae`, isolated CI passed all 27 ignored Docker integration cases, the normal four-stack CLI
harnesses (Rust, Generic, Node and Rails), signed Compose live/config checks, Swift tests, both CLI
compatibility modes, package verification and the build/quality/coverage jobs. The optional Firecracker
run on `0ba79f6` subsequently passed; these results do not complete the local release
matrix or native manual loop below.
On `0ba79f6`, isolated CI again passed all 27 ignored Docker cases, the four normal stack harnesses, signed Compose checks and Swift/CLI integrations. Its coverage tests and LCOV generation passed, but the Codecov action failed during an external TLS handshake, including one retry. The `c5d24f9` follow-up retains LCOV and the JSON summary as CI artifacts before the optional upload, and applies the existing nonblocking upload policy to bootstrap errors. Test and report failures still block the job.
Local line coverage is **79.18%**, below the repository's 90% target.
The guardrail preflight and pretend harness passed. The destructive ignored database/container suite was not
run against the developer's live Docker engine; isolated CI owns that check. Local regular/verbose stack
harnesses and the full agent control-plane stub harness were unrun at this snapshot, so this is a component audit rather
than release sign-off. The combined website/docs build passed; 320 local references and 172 anchors across
five reviewed pages resolved without errors.

Native package at this snapshot: `BranchBox-0.13.4-438-87e0f93.zip` (arm64), with the reviewed release CLI
embedded. Bundle and helper signature checks passed; ZIP SHA-256:
`e9608d15fbe19169590ead73e949b3829feb9928e20a4f89a3518738924ab457`. It is ad hoc signed,
without notarization. The locator still prefers an explicitly selected or installed CLI over the embedded fallback.

The existing Rails checkout was not initialized, repaired or used to launch application services. Its pre-existing
schema modification, BranchBox configuration and registry hashes were unchanged. An isolated archive of
tracked HEAD was tested separately through detect, init preview, minimal start, exec and keep-branch teardown.
That command ran in the host worktree; the Python devcontainer check above supplied the actual Docker test.

Public media were captured by window ID in the disposable workspace and visually checked:

- [Feature overview](../docs/static/img/mac-app/feature-overview.png): repaired native inspector placement,
  Quick feature recorded as Active while its dev container is Not created; app `87e0f93`, runtime `ee39d94`.
- [Start Full form](../docs/static/img/mac-app/start-feature.png)
- [Command execution](../docs/static/img/mac-app/run-command.png)
- [Teardown plan](../docs/static/img/mac-app/teardown-plan.png): Keep selected, planning only
- [Activity setup log](../docs/static/img/mac-app/activity-log.png)
- [Diagnostics identity](../docs/static/img/mac-app/diagnostics.png)
- [Silent walkthrough](../docs/static/media/mac-app-walkthrough.mp4): 24 seconds, H.264, 1600×1100, 30 fps.
  It shows Full setup and Python command execution. A caption explicitly explains that the container was
  started between the recorded clips; that action is not shown. Full decode and browser playback passed.

Three silent motion drafts were produced from those captures. The user rejected their creative quality against the Claude “Apple-style launch film” reference. They are withheld from the public website/docs build and preserved only as drafts:

- [Launch film](../docs/drafts/mac-app-motion/branchbox-launch.mp4): 27 seconds, 1,620 frames; opening/closing loop
  mean grayscale difference 0.029/255.
- [Quick and Full setup](../docs/drafts/mac-app-motion/branchbox-setup.mp4): 14 seconds, 840 frames. Quick's three
  default setup skips are distinguished from separately controlled tunnel provisioning in the text equivalent.
- [Teardown review](../docs/drafts/mac-app-motion/branchbox-teardown.mp4): 14 seconds, 840 frames; planning only.
  Keeping the branch does not preserve Git-ignored files in its removed worktree.

All three are 1440×1440 H.264 at 60 fps with four deterministic subframes per output frame. Full decode,
seek-order purity, every beat contact sample and asset hashes passed; no isolated frame-difference spikes
were found. Focused films have different endpoints and are not loops. These technical checks do not establish creative approval. Public embeds, posters and their motion-specific text were removed; the genuine walkthrough and six app screenshots remain. The five sheet/utility captures came from app `1e51154` and runtime `ee39d94`; the Overview uses `87e0f93` as recorded above. The subsequent LogView
copy change at `f63f2c0` does not change the sheet/Run pixels.

The reusable [.claude/skills/branchbox-release-videos](../.claude/skills/branchbox-release-videos/SKILL.md)
was activated by a dedicated video subagent and installed in the user's Codex skills folder. Independent
forward-testing caught and corrected the Quick-default claim before the final setup render. The installed
skill validates; changed relevant assets refuse before browser/output creation, and a setup-only proof
works without unrelated Run/teardown captures. The skill now records the creative rejection and requires comparison against the actual reference for connected morphs, depth, camera movement and licensed sound when available. Release and launch instructions now request this workflow
for affected features while retaining valid unchanged media. Font licenses, capture/claim provenance,
source hashes and the current production QA receipt travel with the skill. Media preparation is separate
from tagging, deployment and announcements.

**Native review and media follow-up:** Tools → Locate selected the reviewed 14-capability CLI; a fresh Quick
feature, three-workspace sync and Python command succeeded. Its subsequent CLI Keep teardown was independently
checked: worktree/container gone and branch retained. A later Full feature reported three successful modules,
one skipped tunnel, no failed modules and an absent `.env` warning. Separate container startup succeeded and
Python returned stdout/exit 0. Activity's warning filter, message search and timestamps worked; Diagnostics
refresh updated its timestamp. The current ignored-file warning rendered in native teardown planning.
Activity intentionally stores command output in the Run result rather than progress logs; its misleading empty
log placeholder was changed to “No log messages” and compiled with warnings as errors.

The attempted rectangle-captured video and two screenshots showed the foreground Codex window; they were
quarantined and excluded. Corrected window-specific media replaced them. The older 60-second film is retained
privately. A scroll-edge-effect experiment did not remove the horizontal band over native project/feature
content and was reverted. A later isolated A/B test identified the nested Activity inspector: restoring its
original detail-column placement reproduced the band in the same feature, while moving it around the complete
NavigationSplitView cleared the band. The functional fix at `87e0f93` preserves the binding, selected target and
column widths. Native checks passed for project/feature selection, scrolling, inspector toolbar and keyboard
toggle, target changes, default/zoomed sizes, Start Feature sheets and Quick Open. Restoring the fixed build
again cleared the band; the new Overview capture is public. Focused warnings-as-errors Swift checks passed
36 cases in eight suites, including router, enablement, Activity and four render cases (36 private PNGs).
Close/reopen was attempted but not established by the returned accessibility state. Further native checks
paused when the Mac locked; Computer Use requires a manual unlock.
Computer Use denies access to macOS Terminal; native Open Shell interaction remains unverified, although the
exact Docker shell launch plan passed a real PTY check.

Remaining live checks include broader Rails business/authentication workflows, external database cleanup, public tunnels,
1Password credential acquisition, SBX/Local VM/in-guest provisioning, OS notification delivery, and the additional
manual-window behaviors below. Registry module outcomes and recorded tunnel state do not establish those results.

## Resumed audit: 2026-10-05 — partial

The resumed audit uses draft base `6d321eb`. This remains a bounded component audit, not merge or release
sign-off. Private receipts are retained outside the repository; the existing automated counts above are
historical, with the fresh Swift result recorded below.

### Automated and documentation checks

The warnings-as-errors build passed. With `BRANCHBOX_IT=1` and the reviewed contract CLI (binary SHA-256
starting `213d01d4`), the latest `swift-cancel-copy-final-tests.log` reports 701 tests in 93 suites
passing after 26.028 seconds, including the Stop confirmation wording correction. The earlier
`swift-final-tests.log` reports the same count at `c526066` after 24.379 seconds.
That reported count includes gated skips; render and live-fixture suites were not enabled in this invocation.
The combined website/docs build also passed. No new public media was added.

### Packaged native checks

`packaged-native-2026-10-05/results.json` records the initial four legacy-mode checks on package
`0.13.4 / 438 / 87e0f93`: initialization Preview left the disposable repository clean, Apply kept its
layout and created BranchBox/devcontainer configuration, unsupported project settings were disabled with
Open config.json/Done available, and ⌘N focused the Start title with empty Start disabled.

Later native review exposed blank/offscreen split content in legacy mode. Removing the legacy banner
Text's `fixedSize` modifier in `EnvironmentGate.swift` restored visible content in build 449 at
1100×720 and 1800×1130. Native review verified initialization Preview/Apply, capability gating, Start/Done,
sidebar navigation, and Run Command with echo/exit 0 and an intentional exit 3. A 3 MB synthetic command
also completed with exit 0 and a 256 KB output preview. `large-output-export.json` verifies that its export
contains all 3,000,000 bytes with exact expected content. Copy Upgrade Command was verified by pasting
`brew upgrade branchbox` into an unexecuted optional Start prompt; Cancel left the features unchanged.
Banner dismissal preserved visible content. Sidebar filtering and ⌘⌫ clearing passed without opening teardown.
These are synthetic-fixture checks, not use of the live Rails checkout or completion of every native step.

Tools → Locate switched to the reviewed contract CLI and removed the legacy warning. Native project settings
Review/Apply changed the prefix from `feature` to `review`, verified in the file and reopened UI, then restored
`feature` through Apply; tunnels remained off. Native menu Quit while idle passed.

Further contract-CLI checks on build 450 verified Stop and quit during Run Command using an owned Python
sleep in the disposable Legacy feature. Keep Running preserved the command; Return in the quit alert
also chose Keep Running. Explicit Stop removed the exact CLI and Python process group, and the UI
reported "Stopped. No output was captured." A separate running command followed by Cancel and Quit
removed the app and both exact child PIDs. Before/after process receipts are retained as
`cancel-process-{before,after}.json` and `quit-process-{before,after}.json`. This verifies command
cancellation and termination, not cancellation during feature creation or recovery of a partial worktree.
After relaunch, Activity retained both cancelled commands as Stopped with their completion timestamps.

The Stop confirmation incorrectly promised that the command's output so far would be kept. The native
counterexample flushed stdout before sleeping, but the CLI had not returned its completed JSON result.
The copy now explains that interrupted commands may not return output. Twelve existing presentation,
Stop-flow and process-group cancellation tests passed with warnings as errors after the copy correction.

On the fresh Contract fixture, another native settings Review/Apply changed `feature` to `audit`.
`config get feature.branch_prefix --json` independently returned `audit` from the file; reopening the
sheet matched. Native Review/Apply restored `feature`, and the same CLI read-back confirmed restoration
with tunnels still off. This completes the contract-mode branch-prefix loop.

Build 450 also initialized a fresh Git-only Contract fixture with shared agent settings, Sharing and
1Password off and the layout kept. Preview left only `.git`/README and a clean Git status. Apply completed
in the same folder, created devcontainer/project configuration, left tunnels disabled with no op references,
and Done returned to the Contract sidebar row. A Quick Open `followup` query followed by Return also
selected the expected feature.

Quick Open opened with ⌘K and its arrows changed the highlighted row, but Return activated the initial row
in two native reproductions. A stale native TextField `onSubmit` callback was fixed; 31 focused Quick Open,
router and shell cases passed with warnings as errors. Packaged build 450 (`6d321eb` plus the working-tree
banner/Quick Open fixes) then passed native verification in contract mode: ⌘K → Down highlighted the second
feature, and Return selected it in the window heading and sidebar. Bundle and embedded-helper signatures
passed; the bundle is ad hoc signed with the hardened runtime and is not notarized.
The 800×520 minimum-size check remains inconclusive: resize attempts failed and the window remained 1100×720.
The native Close button hid the exact main window according to an independent Core Graphics read-back.
Accessibility observation then reactivated/reopened it; explicit Dock and menu-bar reopen actions remain unverified.

### Real Rails workflow

`rails-real-workflow/rails-compatibility.json` records a CLI-driven, isolated tracked-source fixture with
Ruby 4.0.1, Rails 8.1.2, PostgreSQL 16.12, Redis 7.4.7 and Sidekiq 8.1.0. Schema loading produced 117
tables, 366 recorded migration versions and zero pending migrations; one existing reversible migration
passed down/up. pgvector and Redis round trips passed. Development `/up`, `/` and `/en/users/sign_in`
returned 200, with a sign-in form. One source `CleanupUnconfirmedUsersJob` was queued and processed with
zero failures, deleting the old unconfirmed synthetic user while retaining the two control users.

The test profile returned 500 for `/` and sign-in because `application.js` was absent from the source's
declared precompiled assets; `/up` still returned 200. This source-profile limitation is retained, not
counted as a passing test. The fixture used an internal network without host ports, test mail, no cron
jobs and no external integrations. It did not replay all historical migrations or test tenant business
flows or authentication. This runtime proof is separate from native app initialization.

Feature teardown and independent read-back found no owned containers, networks or volumes; the unique
app image and private fixture/temporary credentials were removed. Original checkout hashes and existing
live containers were unchanged. This cleanup result applies to the Rails fixture, not the failed manual
harness attempt below.

### Manual gate status

`manual-resume-2026-10-05/gate-status.json` is **incomplete**. Review preflight and the source CLI build
passed, as did all four pretend stack runs. Generic regular failed on infrastructure capacity: BuildKit
reported a read-only metadata database, then a host move reported no space left. Docker health and exact
resource queries stalled. At that failed snapshot the other seven regular/verbose runs and the canonical
agent control-plane stub harness were not run. Later Docker info succeeded with ServerVersion 29.8.0;
exact failed-fixture cleanup was verified, and Generic regular retry plus verbose passed. The Generic
control-plane attempt then aborted at the capacity gate (less than 2 GiB free). Its retained database and
owned stub log prove one heartbeat delivery and ack cursor 1; this is partial evidence, not completion of
the workflow-event drain. Both failed fixtures subsequently passed exact ownership cleanup/read-back.
The final receipt records about 6 GB free, but Rust/Rails/Node regular and verbose runs remain blocked
on capacity. A complete real agent/control-plane harness and final workflow-event/ack receipt are still
pending. These later results do not erase the original infrastructure failure.

A subsequent source review found that the agent wrapper sets its socket and unsets `BRANCHBOX_CLI_DIRECT`,
but the current CLI feature start/teardown commands execute the workflows directly. The wrapper does not
send those operations to agent IPC. Its stub check only prints the acknowledgement cursor, without
waiting for or asserting workflow-event delivery. Thus another successful CLI harness run alone would
not establish agent workflow-event coverage. This is a harness-routing gap; the retained heartbeat
receipt does not demonstrate that the agent's IPC workflows are broken. Exercise IPC explicitly and
verify delivered start/teardown events and their final acknowledgement before completing this gate.
The current CI coverage step reports the percentage without enforcing the repository's 90% target;
a green workflow does not establish that coverage requirement has been met.

The host 1Password PAT/signing failure-path gate applies to PR #105 because it changes initialization's
op-reference persistence. It remains pending a reachable SSH origin and configured GitHub/signing references.
The real matrix, control-plane stub, credential gate and remaining native checklist must be completed
before merge/release sign-off; earlier CI results do not complete this resumed local gate.

### Media boundary

The six genuine app screenshots and silent 24-second walkthrough remain unchanged. The private
10-second silent study based on the actual September 27 Claude “Apple-style launch film” reference passed
technical decode/frame/hash checks (`study-qa-receipt.json`). It uses explicitly dated older capture
placeholders, not fresh `6d321eb` captures. Creative review is pending; it is not embedded or published,
and the three previously rejected films remain withheld drafts.

## Mac App ↔ CLI Loop (manual) — partial live coverage

**Status: bounded native checks cover parts of both CLI modes.** The original VER-1 checklist
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
| 2 | Add a disposable `git init` repo → Set Up BranchBox (`init -y`, layout kept) → project appears; repo not moved | ✅ fresh packaged build 450 Preview/Apply/Done; layout kept, sharing/1Password off | ✅ packaged Preview/Apply, synthetic fixture; layout kept |
| 3 | Start a minimal feature → live log → result shows the resolved name; `branchbox feature list --json --repo …` lists it | ✅ Quick and Full, repeated with 14-capability CLI | partial: Start/Done and sidebar verified; full log/list loop pending |
| 4 | Start a feature from Terminal → the app shows it within about 1 s | ✅ direct CLI creation appeared without Refresh | pending |
| 5 | Run Command `echo hi`, then `sh -c 'exit 3'` → exit 3 shown, no alert | ✅ stdout/exit 0 and stderr/exit 3; Docker Python repeated | ✅ packaged echo/exit 0 and intentional exit 3 |
| 6 | `touch notes.txt` in a worktree → Tear Down → refusal card names notes.txt, nothing removed → Discard (confirm) → removed; branch per policy (`git branch --list`) | partial: dirty README refusal, source/container preserved; UI discard pending | pending |
| 7 | Commit in a worktree → Delete if merged is blocked → Force-delete (confirm) → branch deleted | partial: unmerged branch blocks Delete-if-merged; UI Force-delete pending | pending |
| 8 | Prune with 3 features, one dirty → the dirty row is unchecked → per-feature results | partial: dirty row unchecked and Keep selection checked; UI execution pending | pending |
| 9 | Sleeping `post-checkout` hook → start → cancel → confirmation copy → Unregistered worktree row → Remove (see finding 1: contract CLIs also show a stray here, not Interrupted) | pending | pending |
| 10 | Close the main window → menu bar Open BranchBox reopens it; menu bar Tear Down… opens the window and the sheet | partial: native Close independently verified; observation reactivated it, Dock/menu-bar actions unverified | pending |
| 11 | Quit during a start → prompt → Cancel and Quit → `pgrep branchbox` prints nothing | partial: native quit during Run Command removed app and exact CLI/Python children; start-specific case pending | pending |
| 12 | Project Settings branch prefix → `branchbox config get feature.branch_prefix --json --repo …` shows it | ✅ native Review/Apply, reopened UI and CLI get verified change and restoration | n/a |
| 13 | `cd macos && swift run BranchBox` starts a feature without crashing (no notifications) | pending | pending |

Additional manual checks handed over by waves 3 and 4 (record each once, with either CLI):

| Check | Result |
|---|---|
| Finder launch with a stripped PATH through `scripts/macos-dev.sh --open` (launchd environment; notifications appear in a real bundle) | pending |
| Dock reopen, and menu bar Open BranchBox with the main window closed and with the menu bar icon hidden | pending: independent Close check passed, but observation reactivated the window rather than testing these actions |
| ⌘N in the key window; ↑/↓ in Quick Open's field (⌘K); ⌘⌫ in the sidebar filter clears the line instead of opening Tear Down | packaged legacy ⌘N, arrows and filter clearing passed; Return defect fixed and verified natively in contract build 450 |
| Log auto-scroll, scroll-up pause and Jump to Latest with a 2,000-line operation; Run Command with multi-MB output | partial: 3 MB Run completed, 256 KB preview, exact 3,000,000-byte export verified; log scrolling pending |
| Project toolbar at the 1100 pt default width (icon-only secondary actions, overflow) and primary-button prominence / red destructive styling in a key window | partial: default, inspector and maximized layout inspected; full key-window styling pending |
| Dev container Start/Stop on a compose repository updates the Environment card; Diagnostics with Docker stopped | pending |
| Settings › Tools › Locate… switches the CLI live (legacy ↔ contract); legacy project settings are read-only with Open config.json | partial: packaged Locate switched legacy to contract and cleared warning; legacy settings read-only, contract prefix round trip passed; full two-mode loop pending |
| Composed main-window screenshots for the PR from `scripts/macos-dev.sh --open --preview showcase` | pending |

### Native review resumed: 2026-10-05

Inspected the packaged app `0.13.4 / 438 / 87e0f93`: onboarding found the installed legacy
CLI at `/opt/homebrew/bin/branchbox`, showed the older-capability warning and reported Git/Docker ready.
At this initial snapshot no project was added to the production app; the later packaged checks above used
only a synthetic fixture. Quit through its native menu completed; a process check
found only the separate Dev bundle still running. This covers launch discovery and idle quit, not the
full packaged two-mode checklist or quit during an operation.

The Dev bundle `0.13.4 / 437 / c5d24f9` used the reviewed 14-capability CLI and the disposable Workspace
fixture. Its component code matches `87e0f93` except for the later Inspector placement fix. Verified:

- ⌘K opened Quick Open; ↓ changed the highlighted result, and ↑ then Return selected the expected feature.
- ⌘⌫ cleared a focused sidebar filter without opening Tear Down.
- ⌘N opened Start a Feature with its title focused and empty Start disabled; Cancel returned to the feature.
- Three new features created by the CLI appeared without a manual Refresh.
- A dirty worktree named `notes.txt` in the refusal and reported “Nothing was removed”. Its file and worktree remained.
- A branch with one unmerged commit showed that count; Delete if merged disabled Tear Down and requested Keep or Force-delete.

Discard, Force-delete and Prune execution remain pending explicit cleanup confirmation. The fixtures
retain a byte-identical notes backup and a recovery Git ref for the unmerged commit. Close/reopen remains
unverified because observing the app brought the window back; this does not establish a lifecycle defect.
Private fixture and result receipts remain outside the repository. No live Amidship runtime was changed.

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
