---
sidebar_position: 8
---

# Release readiness

BranchBox measures coverage by whether users can complete the supported workflows safely. Named
outcomes and real end-to-end evidence are the primary readiness measure. Rust line coverage helps
locate missing tests; it has no numeric shipping threshold. This replaces the former 90% target.

## Record outcomes before shipping

For a behavior-changing PR, cover the affected journeys and their failure paths. For a release,
review the supported workflow matrix and follow the [maintainer release guide](https://github.com/branchbox/branchbox/blob/main/RELEASING.md).
Keep passing quality/build checks alongside the outcome evidence.

Each outcome record must contain:

- The user action, expected result and independent observed state: files, Git refs, registry, processes
  or owned runtime resources, as appropriate.
- Source commit and app/CLI identity, including legacy or contract mode; stack, provider and environment.
- Test layer and command or CI run: unit/contract, disposable real CLI, real service, or native app.
  Mark mocks, capability skips and render-only checks explicitly.
- Status: **Passed**, **Failed**, **Partial**, **Unverified**, or **Not applicable** with a reason.
  Link the receipt or durable CI result and record any accepted gap and its owner/date.

Cover success and the applicable refusal, recovery, cancellation, persistence and cleanup paths.
For destructive operations, prove that rejected actions preserve user work, consent matches the current
plan, owned resources are removed, and neighboring resources survive. A passing happy path does not
establish these safeguards.

Failed critical outcomes block shipment. An unverified or partial outcome stays a gap unless the owner
explicitly accepts its documented scope; acceptance does not change its status to Passed. Preserve
earlier failures and retries as dated evidence. Reuse evidence only when its source and environment
still apply. Documentation-only changes do not require rerunning unchanged application journeys.

Do not convert test counts into a coverage percentage. A green job, a preview screenshot, a fake tool
or a skipped suite does not establish live E2E coverage. Native app checks and CLI harnesses complement
each other; record which interaction actually ran.

## PR #105 evidence snapshot: 2026-10-05

[PR #105](https://github.com/branchbox/branchbox/pull/105) merged as `872e1bd`, from reviewed head `80a29ab`.
The tested PR merge `573896a` had the same tree as the reviewed head. The table records bounded outcomes,
not a claim that every feature or provider is covered.

CI targets that final tree. Native evidence spans the dated earlier bundles and build 452 recorded in
the [detailed testing record](https://github.com/branchbox/branchbox/blob/main/macos/TESTING.md); those
earlier Run/settings checks are retained evidence, not a claim that every interaction was repeated on
the final build. Build 452 verified the final layout/selection and consented-removal repairs.

| User outcome and asserted state | Evidence | Status and scope |
|---|---|---|
| Set up a project: Preview leaves Git/files unchanged; Apply creates configuration while keeping the layout | Native disposable Git fixtures and the [init/start harness](manual-cli-e2e.md) | **Passed** for recorded legacy/contract fixtures; Sharing and 1Password were off in the native setup checks |
| Start and run feature work: worktree/registry agree, commands report output and exit status | [Regular four-stack CI](https://github.com/branchbox/branchbox/actions/runs/37355026718), [verbose four-stack CI](https://github.com/branchbox/branchbox/actions/runs/37355087512), [Mac real-CLI integration](https://github.com/branchbox/branchbox/actions/runs/37355026654) and native Quick/Full/Run checks | **Passed** for Rust, Generic, Node and Rails harness configurations; Quick setup does not itself prove a running devcontainer |
| Refuse unsafe teardown: dirty paths and unmerged commits survive; newly discovered or truncated changes require fresh consent | [CLI safety tests](https://github.com/branchbox/branchbox/blob/80a29ab/cli/tests/teardown_safety.rs), Swift planning/backend regressions and native refusal checks | **Passed** within real disposable Git and focused regression scopes; truncation/new-path races have automated evidence |
| Execute consented removal: Discard removes the selected worktree, Keep retains its branch, Force-delete follows explicit consent | [Mac lifecycle integration](https://github.com/branchbox/branchbox/blob/80a29ab/macos/Tests/BranchBoxIntegrationTests/CLIBackendLifecycleTests.swift) and native read-back | **Passed** for the recorded fixtures; the Force fixture's exact unmerged commit remains on a recovery ref |
| Prune only the selected safe features; exclude dirty or truncated rows and preserve neighbors | [Prune integration](https://github.com/branchbox/branchbox/blob/80a29ab/macos/Tests/BranchBoxIntegrationTests/PrunePlanningTests.swift), planner/flow regressions and native one-row Prune | **Passed** for those selections; native result was 1 torn down, 0 partial, 0 failed |
| Clean up the exact owned runtime: Compose feature teardown removes owned containers/networks/volumes; standalone teardown removes owned containers; neighbors survive | [Ignored Docker CI](https://github.com/branchbox/branchbox/actions/runs/37355026718) and recorded image/Dockerfile/native/external Compose checks | **Passed** for exercised cases. Standalone volumes/custom networks are outside the feature-teardown container check; Stop/Down volume retention follows the selected policy |
| Sync devcontainer configuration safely: only an unchanged, recorded link to the expected main source is generated; retargeted/unrecorded links stay protected | [Devcontainer baseline regressions](https://github.com/branchbox/branchbox/blob/80a29ab/core/src/modules/devcontainer.rs), [teardown classification](https://github.com/branchbox/branchbox/blob/80a29ab/core/src/workflows/teardown_plan.rs) and real Git sync/plan lifecycle | **Passed** in focused regressions; older symlink features require resync to establish the baseline |
| Deliver agent workflows: real IPC start/teardown produce events 2/3, retry an unchanged batch after HTTP 503 and persist ack 3 | [Full generic agent/CP run](https://github.com/branchbox/branchbox/actions/runs/37355087512) and [harness assertions](https://github.com/branchbox/branchbox/blob/80a29ab/scripts/lib/agent-e2e.py) | **Passed** for the real minimal IPC lifecycle and stub control-plane drain; the same wrapper separately passed the Generic Docker CLI harness. Production control-plane verification remains outside this proof |
| Cancel/recover operations: stop process groups, expose partial work and remove the stray without losing its kept branch | [Current-source cancellation integration](https://github.com/branchbox/branchbox/blob/80a29ab/macos/Tests/BranchBoxIntegrationTests/CancellationTests.swift) and native Run/Stop/quit/start-recovery receipts | **Passed** for automated cases and native Run/Stop/quit; **Partial** for current-source native start cancellation: Start used `6d321eb`, recovery used `80a29ab` |
| Persist settings and external changes: prefix changes survive read-back and restore; CLI-created features appear without Refresh | [Config](https://github.com/branchbox/branchbox/blob/80a29ab/macos/Tests/BranchBoxIntegrationTests/ConfigIntegrationTests.swift)/[watcher](https://github.com/branchbox/branchbox/blob/80a29ab/macos/Tests/BranchBoxIntegrationTests/WatcherIntegrationTests.swift) integration and native UI/CLI read-back | **Passed** for recorded branch-prefix and watcher scenarios; legacy settings remain read-only |
| Navigate and inspect output: sidebar/Quick Open remain usable, selected actions refresh, large-output export preserves every byte | Native layout/selection regressions, Run exit 0/3 and exact 3,000,000-byte export read-back | **Passed** for recorded window sizes and interactions; minimum-size and OS lifecycle checks below remain gaps |

All 18 standard CI jobs, four Mac jobs and four verbose stack jobs passed on the reviewed tree. The
optional [Firecracker lifecycle](https://github.com/branchbox/branchbox/actions/runs/37355026653) passed
separately. Mac CI covers both the current-source contract CLI and released 0.13.4 compatibility floor.
Universal preview build 453 matched its checksum and passed deep/strict signature verification for
arm64 and x86_64. These results do not constitute a new tagged or notarized release.

### Explicit gaps and claim boundaries

- **Unverified, accepted by the owner for this merge on 2026-10-05:** live host 1Password PAT/signing
  acquisition and failure-path execution. Offline reference/argv/safe-write checks passed; they do not
  establish live credential access. Use the [live harness](manual-1password-e2e.md) when credentials are available.
- **Unverified native interactions:** notification delivery, explicit Dock/menu-bar reopening, minimum-size
  interaction and native Terminal access. The app remains an ad hoc signed, unnotarized preview.
- Public tunnels and broad SBX/Local VM/managed in-guest provisioning are not established by the Mac
  component audit. A fake provider's remediation result remains a contract check. The separate Firecracker
  run establishes its exercised lifecycle only.
- The isolated Amidship Rails fixture passed recorded development/schema/Redis/Sidekiq outcomes and exact
  cleanup. Its test profile returned an asset-related 500, retained as a source finding. Tenant business
  flows and authentication are unverified; this is not full Amidship application coverage.

The [detailed Mac testing record](https://github.com/branchbox/branchbox/blob/main/macos/TESTING.md)
retains source identities, historical failures and native evidence scopes. Genuine screenshots and a
24-second walkthrough document the preview; cinematic drafts remain withheld and production is paused.

## Line coverage remains a diagnostic

The comparable Rust LLVM workspace report was **73.32%** on pre-PR main `d1dcca0`
([baseline CI](https://github.com/branchbox/branchbox/actions/runs/37304286512)) and **80.66%** on the
reviewed PR tree ([PR CI](https://github.com/branchbox/branchbox/actions/runs/37355026718)), an increase
of 7.34 percentage points. Both used:

```bash
cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
```

Keep the LCOV and JSON artifacts and review comparable regressions. This report includes instrumented
Rust workspace sources, including inline test code; it does not combine Swift, ignored Docker suites,
manual CLI/agent harnesses or native UI runs. The 27 ignored Docker cases and reported Swift case counts
are test inventory, not percentages of user outcomes.
