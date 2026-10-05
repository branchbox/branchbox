# Repository Guidelines

## Project Overview & Current State
BranchBox is a distributed development environment orchestrator managing git worktrees and devcontainers. Milestone 0 is complete—core workflow orchestration for feature worktrees is implemented in Rust. Milestone 1 shipped the Rust agent daemon (macOS/Linux/devcontainer) plus CLI bridge + telemetry so long-running workflows can run in the background. Milestone 2 added the control-plane HTTP drain and `branchbox agent status`. The macOS app (`macos/`) is a native SwiftUI front end that drives the installed `branchbox` CLI through its `--json` contract; an agent-daemon backend for it comes later. Next up: Windows transport, full Rails control plane UX, and tighter Tailscale coordination so the offline-first contract holds across devices.

## Project Structure & Module Organization
The Cargo workspace roots at `Cargo.toml` with three members: the core library in `core/`, the CLI in `cli/` and the agent daemon in `agent/`. Core modules live under `core/src/` (notably `adapters/`, `modules/`, `bootstrap/`, `workflows/`, and cross-cutting helpers like `git.rs`, `output.rs` and `atomic_fs.rs`). The CLI entry point is `cli/src/main.rs`, exporting the `branchbox` binary on behalf of the library. The macOS app in `macos/` is a separate Swift package (`macos/Package.swift`), not a Cargo member; see "macOS app" below. Shared documentation sits in `docs/`, CI workflows in `.github/workflows/`, and reproducible tooling in `.devcontainer/`.

## Build, Test, and Development Commands
Run workspace builds with `cargo build` and optimize releases via `cargo build --release`. Execute the CLI locally with `cargo run -p branchbox-cli -- --help` to validate argument wiring. Use `cargo fmt --all -- --check` to enforce formatting, `cargo clippy --all-targets --all-features -- -D warnings` for linting, and `cargo check` for quick iteration. Security and dependency scanning is covered by `cargo audit`.

## Coding Style & Naming Conventions
Rust files follow `rustfmt` defaults (4-space indentation, 100-column soft limit). Modules and files use `snake_case`; types are `UpperCamelCase`; constants are `SCREAMING_SNAKE_CASE`. Prefer explicit `Result<T, Error>` aliases plus the `?` operator for flow control, and leverage `thiserror` for rich domain errors. Branch names should stay action-oriented, e.g., `feature/bootstrap-cleanup` or `fix/git-lock-race`.

## Rust Build Cache Pilot
The `manual_cli_e2e` jobs in `ci.yml` and `manual-cli-e2e.yml` pin Mr. Boxington 1.22.0 and action commit `d0825fbaf3cc36ca2609aa38e71046265a1f1e37`, using GitHub target caching. Only the Rust matrix stack may save; other stacks restore compatible entries with `ACTIONS_CACHE_MODE=read` (preserving stricter inherited modes). Main CI opts Rust into same-repository PR-scoped saves; forks never save, and schedule/dispatch remain restore-only. Keep the CLI at `target/debug/branchbox` and review the Bash-only Cargo wrapper if harness Cargo calls change. Do not stack another cache action on these paths or extend the pilot to coverage/release jobs without new measurements. `scripts/benchmark-rust-cache.py` runs the isolated macOS comparison; methodology and results live in `docs/docs/internals/rust-build-cache.md`.

## Testing Guidelines
Unit tests live beside their modules under `#[cfg(test)]`; grow integration coverage in a `core/tests/` harness when cross-cutting behaviour warrants it. Run `cargo nextest run --all-features --no-fail-fast` for the default gate, `cargo test --doc` to validate examples, and `cargo nextest run --all-features --run-ignored ignored-only` when you need parity with CI’s integration configuration. CI enforces 90% line coverage via `cargo llvm-cov`, so periodically run `cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info` to catch regressions.

### Manual CLI regression requirement
Before any PR is marked ready or a release branch is cut, run the CLI smoke harness in all supported modes to cover `branchbox init`, multi-feature devcontainer sync, tunnel module permutations (manual fallback, Cloudflared, credential-loss), and teardown end-to-end:

```bash
./scripts/manual-cli-e2e.sh
./scripts/manual-cli-e2e.sh --mode verbose
./scripts/manual-cli-e2e.sh --mode pretend
# Target other stacks (e.g., generic, rails, node)
STACK=generic ./scripts/manual-cli-e2e.sh
STACK=rails ./scripts/manual-cli-e2e.sh
STACK=node ./scripts/manual-cli-e2e.sh
```

Follow up with `./scripts/manual-agent-e2e.sh --cp-stub` to exercise the control-plane HTTP drain. The flag spins up a disposable stub endpoint, feeds it the agent’s batched events, and prints the ack cursor so you can confirm retries/metadata before cutting a release. For quick spot checks (without rerunning the harness) use `branchbox agent status --json`—it reports whether the drain is configured/connected plus the last delivery/failure timestamps.

If you touch devcontainer auth/signing bootstrap (1Password PAT + SSH signing flow from issue #45), also run:

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
OP_GITHUB_REF='op://<vault>/<item>/token' \
OP_SIGNING_KEY_REF='op://<vault>/<item>/private key' \
./scripts/manual-1password-e2e.sh --check-failure-path
```

### Devcontainer auth/signing guardrails (issue #45)
- Treat Docker Desktop on macOS as **no SSH-agent socket pass-through** for 1Password keys; prefer the PAT + mounted-file strategy.
- Keep host-only secret retrieval in devcontainer `initializeCommand` (`op read`), and keep container-only git/gh/signing setup in `postStartCommand`.
- Ensure mounted secret files exist before `docker compose up` (`touch` placeholders) or first-run compose startup will fail.
- Keep secret files (`.github-token.env`, `.git-signing-key`, `.gitconfig.env`) in `.gitignore` and template scaffolding so new repos are safe by default.
- Never truncate existing secret files before a successful `op read`; write to temp files and atomically move into place so transient 1Password failures do not wipe previously working credentials.
- Treat empty/whitespace `op read` results as failures for rotation purposes; preserve existing token/key files instead of writing blank replacements.
- Create temp secret files with restricted permissions from creation time (`umask 077`), then atomically `mv` into place.
- Enforce owner-only permissions (`chmod 600`) for host-side token/signing material; never leave private key files world-readable.
- Parse mounted env-style files with explicit `grep/cut` reads instead of `source` (names with spaces and quotes are common in real configs).
- Never interpolate raw token values into persisted shell snippets (for example git credential helper commands); reference environment variables like `GH_TOKEN` at runtime.
- For SSH signing, copy keys from read-only mounts into `~/.ssh` with strict permissions (`chmod 600`) before configuring `git config user.signingkey`.
- Convert `origin` from `git@github.com:*` / `ssh://git@github.com/*` to HTTPS when PAT-based auth is configured in-container.
- Degrade gracefully when secrets are missing/invalid: emit warnings and keep the workspace usable instead of hard-failing startup.
- Sanitize untrusted `.env`-derived values before writing generated env files (strip control chars such as `\n`/`\r`, and quote shell-sensitive values like `APP_URL`) to prevent variable-injection payloads.
- Sanitize compose identity once and reuse the same sanitized value for process env + generated files (`COMPOSE_PROJECT_NAME`, `DEVCONTAINER_NAME`) to avoid drift.
- Keep compose project names constrained to Compose-safe characters (`[a-z0-9_-]`).
- Keep `GIT_BRANCH` env writes allow-listed to env-safe ref characters (not only “remove control chars”).
- Preserve the Dev Container environment boundary: apply declared `containerEnv` only when creating the container (through a private invocation-local override for Compose), merge configured and explicitly supplied `remoteEnv` only into lifecycle or exec processes, never inherit ambient host variables implicitly, and never log environment values. Because `containerEnv` is static, bind only its canonical name set in a public container label—never a value-derived digest—and compare expected values directly with inspected container state. An existing mismatch must fail with explicit recreate guidance rather than silently claiming the new value was applied.
- Avoid fixed compose `name` or `container_name` values in templates; worktrees must remain parallel-safe.
- Preserve compatibility by supporting both `docker compose` and `docker-compose` in workflow/module orchestration.
- On feature teardown, validate the managed Compose identity in `.devcontainer/.branchbox.env` against its canonical-workspace binding. Legacy identity requires the matching registry project/worktree or established exact-label/history evidence; reject copied or ambiguous records before Docker mutations. Discover devcontainer CLI project names only through an exact `devcontainer.local_folder` match, and verify owned containers, networks, and volumes are gone before deleting the worktree.
- In Agentify `in-guest` mode, persist workspace/Compose/proxy ownership before `devcontainer up`; failed-start and no-registry teardown must recover only exact Dev Containers/Compose label ownership, bypass repository modules/adapters, and remove the failed worktree and task branch.
- Keep project Docker disabled in Agentify `in-guest` mode: strip outside/in/from-Docker feature aliases and daemon-bearing run arguments, reject Docker/containerd/Podman/BuildKit sockets or remote endpoint variables after merged-config resolution, and re-check the running container for supervisor mounts, endpoints, host namespaces/devices, elevated capabilities, and disabled confinement.
- Treat repository Compose mounts and publications as untrusted in Agentify `in-guest` mode: synthesize only the canonical task-worktree bind at a normalized effective folder below platform-owned `/workspaces` plus the canonical Git metadata bind, add only manifest-approved lease mounts, clear devcontainer port-forward policy plus every service's repository volumes/`ports`/`expose`, and start only the primary service with its validated dependency closure. Signed BranchBox loopback proxies are the sole published-port path and must target the inspected primary container; a future multi-service route requires an explicit signed service identity.
- Compose interpolates each input file before applying the generated `!override` facade. In managed `in-guest` mode, pass private sanitized copies to the Dev Containers CLI so repository volume sources, env files, devices, and publications never reach interpolation; reject `include`, `extends`, and `volumes_from` paths that could introduce unsanitized service mounts, and reject `provider` services because Compose runs their selected binary on the guest host. Keep repository source files untouched. If a signed workspace consumer can write the task worktree, stage the generated CLI config and Compose inputs in a runtime-owned 0700 directory outside that worktree, require a distinct consumer UID and complete preloaded image coverage, and fail closed where relative build paths cannot be preserved.
- Filter disabled connector dependencies in each sanitized Compose input, preserving Compose's ordered merge of runnable dependencies across files; do not derive a final `depends_on` replacement from only the last service definition.
- Reject ambient `$VAR` and `${VAR}` interpolation in retained and generated in-guest Compose fields, including the effective workspace folder. Reject value-less dependency `environment` and `build.args` entries because Compose inherits those names from its process environment; keep explicit static values and Compose-escaped `$$` literals. Clear the environment of managed in-guest Docker/Dev Containers children except for runtime paths and Docker connection settings, and disable automatic Compose `.env` loading; the signed raw project-environment file remains the explicit configuration path.
- Deliver typed `project-environment` materialization only to the primary Compose service with Compose 2.30+ raw `env_file`. Require canonical sorted uppercase single-line dotenv, reject runtime/provider control names, never shell-source or serialize values, and erase the source through provider teardown.
- Keep managed provider credentials provider-neutral: versioned assignments bind an arbitrary provider consumer to exact safe environment names and owner-only digest-bound materializations. Never infer provider names in BranchBox, never mount provider-environment files into the devcontainer, inject them only into the named provider process tree, and treat cleanup residue as teardown failure.
- Keep managed shared-directory and tool-endpoint leases run-scoped and provider-neutral: require owner-only exact-run directory/socket sources, mount only signed source/target pairs below the BranchBox lease namespace, prove read-only binds on the inspected primary container, and include their removal in residue-checked teardown receipts.
- Never relax or remap an owner-only trusted tool socket so a different devcontainer UID can reach it. Link it to a signed `tool-request` lease instead: keep the endpoint outside the coding container, expose only the exact per-run request volume as writable, resolve the actual non-root provider UID and require it to match `consumer_uid`, bind the immutable descriptor and endpoint-only capability to run/lease/consumer, validate and consume bounded regular files through a root-only staging directory, and residue-check the volume, capability source, endpoint, and replay ledger on teardown. Replay remains denied by default. An endpoint that durably implements exact-request idempotency may opt in through the signed provider-neutral `replay_policy: exact-digest-replay-v1`; BranchBox must then persist the capability-stripped request and digest before relay, serialize attempts with an owner-only process-crash-safe lock, accept only an exact request match, and persist the correlated response before committing it to the consumer spool.
- Never widen a runtime-owned worktree so a coding container can write it. Use the signed version 3 `workspace_consumer` delegation instead: require the consumer GID as a delegated supplementary group, change group ownership only, add group read/write plus directory traversal/setgid while preserving existing executable bits, give every delegated directory a POSIX default ACL so consumer-created paths stay reclaimable by the runtime whatever the consumer's umask, never add world access or follow a symlink, refuse any path the runtime does not own, and fail closed unless the resolved container UID and primary GID both equal the signed identity. Give the primary service only the two exact `safe.directory` Git ownership exceptions it needs (the effective task worktree folder and the canonical Git metadata target) through the generated facade's overridden `environment`; never a wildcard, and never merge a repository's `GIT_CONFIG_*` values, which project and provider environments already reserve.
- Make a refusal name its own cause. A validation error that reports only what was rejected, and not why, is unactionable: the reason has to be recovered by rebuilding the guest, which is how a single permission failure produced three wrong diagnoses. Include the OS error, the offending value, and the expectation it broke. Withhold a cause only when it could carry credential or consumer-supplied content, and annotate that decision with a `cause-withheld:` comment; the review preflight enforces the annotation.
- For manual harnesses, resolve devcontainer service names with JSONC-safe parsing plus compose-file fallback; do not assume strict JSON or plugin-only compose.
- When helper logic is shared across harnesses, extract it into `scripts/lib/*.sh` rather than duplicating functions.
- When editing harnesses/docs, keep `scripts/manual-*.md` and `docs/docs/getting-started/manual-*.md` in sync in the same change.

#### Review preflight for this area
- Run `./scripts/review-preflight.sh` before requesting review; treat failures as blockers.
- Run a quick security grep before PR handoff to catch known regressions: host key mode (`chmod 600`), no raw-token interpolation in credential helper strings, and sanitized `APP_URL` writes.
- Verify secret-write safety details directly in host init scripts: `umask 077` on temp-file writes and “preserve existing file on empty secret” handling.
- Verify compose/env sanitizers stay policy-aligned (`COMPOSE_PROJECT_NAME` charset, `GIT_BRANCH` allow-list) and tests cover those policies.
- Verify harness portability checks are present (`docker compose` + `docker-compose` fallback, JSONC-safe service detection).
- Check for duplicated shell helpers across `scripts/manual-*.sh`; move shared pieces to `scripts/lib/`.
- Diff paired docs (`scripts/manual-*.md` vs `docs/docs/getting-started/manual-*.md`) to confirm they remain synchronized after edits.

When touching the macOS app, the CLI's `--json` output, teardown, prune, `config`, `init` or the feature registry, run the “Mac App ↔ CLI Loop” from `docs/docs/getting-started/manual-cli-e2e.md` (mirrored in `scripts/manual-cli-e2e.md`) on a Mac: package the app, drive a disposable repository from both the app and Terminal, and record the results in `macos/TESTING.md`.

The harness intentionally edits the feature devcontainer before teardown to exercise the dirty-worktree guard, so an initial `feature teardown` failure followed by the scripted `--force` retry is expected. Use `KEEP_E2E_TMP=1` when you need to inspect the generated workspace for failures, and block merges until the script succeeds.
CI runs the harness for `rust`, `generic`, `rails`, and `node`; if you touch another stack locally, mirror that by passing `--stack <stack>` when running the script.

### Release workflow
- Follow `RELEASING.md` verbatim. The short version: ensure `main` is up to date, run fmt/clippy/tests/docs, then execute the six manual CLI harness permutations listed above (regular/verbose/pretend × rust/generic/rails/node). Releases are blocked until every combination passes locally.
- Update `CHANGELOG.md` with highlights, refresh `README.md` + `docs/docs/**` (especially the manual CLI E2E pages, first-run `branchbox init` UX + 1Password/git bootstrap caveats in quick start, and CLI reference pages), and capture any new expectations here in `AGENTS.md` before tagging. Regenerate `docs/docs/reference/cli.md` from the recursive `--help` output (every subcommand) whenever flags change.
- For changed-feature release or launch media, delegate a video producer using [.claude/skills/branchbox-release-videos/SKILL.md](.claude/skills/branchbox-release-videos/SKILL.md) alongside the documentation agent. Show the beat map and four proof stills before the full film, retain capture/claim/license provenance, and verify responsive playback after embedding the reviewed videos. Refresh affected clips while preserving valid unchanged media.
- Keep `docs/docs/getting-started/manual-cli-e2e.md` + `scripts/manual-cli-e2e.md` and `docs/docs/getting-started/manual-1password-e2e.md` + `scripts/manual-1password-e2e.md` synchronized with the actual harness steps—future contributors should be able to trace every required validation from those docs.
- Run `cargo release --workspace --dry-run` before `--execute` so you can catch version bumps or git state issues early. Push with `git push --follow-tags` and monitor the release workflow with `gh run watch`.
- After tagging, confirm the docs build (`cd docs && npm run build`), the GitHub Pages deployment, and downstream taps (Homebrew) before announcing the release.

### Compatibility & template hygiene
- When touching JSON/state schemas (e.g., `.branchbox/registry.json`), add backward-compatible deserializers or migrations before landing the change. Existing workspaces must continue working without manual edits.
- When editing code-generated assets (devcontainer or compose templates), re-run `cargo test` to catch expectation drift (the template tests in `core/src/bootstrap/templates.rs` enforce current bind mounts and shared volume paths).

## Commit & Pull Request Guidelines
Recent history favors concise, imperative summaries (e.g., `Refactor CLI to 'branchbox' with grouped subcommands`). Continue that tone while adopting the Conventional Commit prefix expected in `CONTRIBUTING.md`, such as `feat(modules): add docker compose planner`. Before opening a PR, rebase on `main`, rerun fmt/clippy/tests/doc checks, and attach context: problem statement, scope, linked issues, and any relevant CLI transcripts or screenshots. Ensure the CI suite is green before requesting review.

Prefer the GitHub CLI (`gh pr create --fill`) for opening PRs after pushing the branch so reviewers get the templated context and automation can rely on consistent metadata.

## Architecture Essentials

### Adapters vs Modules
Adapters provide stack-specific behavior (Rails vs Node.js vs Generic), detecting project type via marker files and returning confidence scores 0-100. Modules are composable cross-cutting features (compose, database, tunnel, specs) that run during worktree lifecycle. Both use trait objects for polymorphism; adapters are detected once per workflow, while modules are detected and executed in dependency order via topological sort.

### State Management
The `FeatureStateStore` tracks worktrees in `{repo_root}/.branchbox/registry.json`. Each entry carries `work_feature`, `branch_name`, `worktree_path`, `feature_url`, `status` (`active`, `degraded`, `failed_retained`, `orphaned`, `removed`), `created_at`, `updated_at`, plus optional runtime, tunnel, module and sync metadata.

- Every state write (registry, config, devcontainer baselines) runs under `atomic_fs::lock_state_dir`, an exclusive advisory lock on the `.branchbox` directory itself on Unix (`.branchbox/.lock` elsewhere), and lands through `atomic_fs::write_atomic` (temp file in the same directory, `fsync`, one rename). Readers take no lock and never see a torn file. A contended lock logs one "Waiting for another BranchBox process…" line and gives up after 30 s with `registry_locked`. Never write these files with plain `fs::write`.
- Git worktree add/remove/prune and branch deletion run under a separate per-repository worktree lock in the shared git directory.
- `feature start` registers the feature with `setup: {state, pid, started_at}` as soon as its worktree exists, so an interrupted start stays visible (`feature list` reports it as `interrupted`). An `active`/`failed_retained` entry whose worktree folder is gone is listed as `orphaned`.
- The Mac app and the CLI share this registry, and older CLIs read it too: add only optional `#[serde(default)]` fields and never a new `FeatureStatus` variant (see "API/JSON contract").

Future enhancement: when PRs are opened via `gh`, persist their number/URL back into the feature registry so the agent/control plane can display review status without re-querying GitHub.

### Specs Module Behavior
During feature start, the specs module promotes `docs/features/backlog/{name}.md` to `docs/features/in-progress/{name}.md` (or creates a stub with front matter if missing). During teardown with `--complete-spec`, it moves from `in-progress/` to `completed/`.

## Environment & Configuration Tips
Use the provided devcontainer (`.devcontainer/`) for a consistent toolchain; it preinstalls Rust, Clippy, cargo-nextest, cargo-llvm-cov, and Docker. The container runs privileged for Docker-in-Docker. Tool configurations (`.codex/`, `.claude-code/`, `.gh/`) are volume-mounted via the `SHARED_CONFIG_DIR` environment variable (defaults to `../..`, the parent directory), ensuring credentials and session state persist across container rebuilds and are shared across all feature worktrees—authenticate once with `gh auth login` in any worktree and credentials are available everywhere. Non-worktree users can override with `SHARED_CONFIG_DIR=..` in `.env`. Local setups should copy `.env.sample` into a private `.env` and avoid committing secrets. Tests should set `BRANCHBOX_SKIP_HOST_VALIDATION=1` to bypass host checks. During feature start, the workflow copies `.env` from repo root to worktree and injects `APP_URL` and `COMPOSE_PROJECT_NAME`.

## Module Implementation: Devcontainer
- **Detection**: `DevcontainerModule::detect` returns true when `.devcontainer/` exists in the main worktree. Agent bootstrap should ensure the directory is present before queuing the module.
- **Init/Setup flow**: `init` captures the source `.devcontainer/` path and picks a sync strategy (`copy` by default, override via `BRANCHBOX_DEVCONTAINER_STRATEGY`). `setup` invokes `sync_to(feature_dir)` to mirror files into each worktree, skipping excluded entries like `.env`.
- **Strategies**: Copy keeps feature-specific edits isolated; symlink keeps worktrees auto-updated. Agents may expose a policy knob but must default to copy to avoid permission prompts on macOS.
- **Sync command**: `branchbox devcontainer sync [--strategy copy|symlink] [--dry-run] [--feature NAME]... [--json]` replays the module across all active worktrees (or the named ones). Agents should call this after updating `.devcontainer/` in the main repo or during migrations.
- **Telemetry hooks**: The module emits tracing spans (`module.devcontainer.sync`) with outcome, duration, and strategy. Capture these for observability dashboards and to flag stale worktrees (module failures should surface as soft errors).
- **Feature flags**: Gate early rollouts with `BRANCHBOX_ENABLE_DEVCONTAINER_MODULE`. Agents can toggle this per-workspace to coordinate canary deploys.
- **Failure handling**: If sync fails, mark the worktree as `devcontainer_outdated` in registry metadata and warn the user instead of aborting the workflow. Agents should surface remediation guidance in the CLI/UX.
- **Shared credentials**: Confirm shared mounts remain intact (`.gh`, `.claude`, `.codex`) after sync or teardown. Agents must never delete host-side shared directories.

## Agent Integration Plan
- **Daemon wiring**: Expose a `DevcontainerSyncJob` in the Rust agent that triggers when `.devcontainer/` changes in the main worktree (file watcher) or when a new worktree registers. Job should enqueue module execution via the existing workflow runner.
- **Command bridge**: Use the CLI as a fallback (`branchbox devcontainer sync --json [--feature NAME]...`) until native library bindings are exported. It prints one document with a row per worktree (`synced`, `would_sync`, `skipped`, `failed`) and exits 1 when any row failed; the CLI already updates the registry metadata.
- **Registry extensions**: Add optional fields to worktree entries (`devcontainer_outdated`, `last_sync_at`, `sync_strategy`). Ensure schema migrations remain backward compatible for Milestone 0 installations.
- **Observability**: Forward module spans to the agent’s OpenTelemetry pipeline. Track counters for `sync_success`, `sync_skipped`, `sync_failed`, with labels for strategy and stack (rails/nodejs/rust/generic).
- **Policy management**: Introduce `AgentPolicy.devcontainer.strategy` config knob (defaults to `copy`). Allow per-workspace overrides via `.branchbox/agent.toml`.
- **Health reporting**: Surface stale sync warnings through the forthcoming control plane API (`/v1/worktrees/:id/health`). Include remediation actions in the payload.
- **Cross-platform validation**: Run agent regression suite on Linux (devcontainer), macOS (local), and Windows (WSL2). Verify symlink strategy behaves under each OS’s permission model.
- **User messaging**: Teach the agent to emit actionable CLI guidance when a sync fails (example: "Run `branchbox devcontainer sync --strategy copy` manually after fixing permissions").
- **Security review**: Coordinate with security to audit shared credential mounts and file permission expectations before enabling automated sync outside devcontainers.
- **Rollout**: Stage deployment—enable feature flag for internal repositories, monitor telemetry, then progressively roll out to early adopters before global enablement.

## Sync Workflow Blueprint
- **Trigger sources**:
  1. File watcher detects change under `.devcontainer/`.
  2. Registry mutation (`FeatureStateStore::register_worktree`) for new worktrees.
  3. Manual control-plane instruction (`/v1/devcontainers/sync`).
- **Job pipeline**:
  ```
  Trigger -> enqueue(Job::DevcontainerSync { workspace, strategy_override }) 
          -> rate_limit (per workspace) 
          -> Worker acquires registry read lock 
          -> For each worktree:
               - skip if removed or archived
               - call core::modules::devcontainer::sync_to()
               - collect SyncOutcome (files, duration, status)
          -> persist outcomes -> emit telemetry -> respond to caller
  ```
- **Backoff**: Use exponential backoff (base 2s, cap 2m) when sync encounters filesystem errors to avoid hammering disk on permission failures.
- **Concurrency**: Allow one active devcontainer sync per workspace to avoid conflicting writes; queue subsequent requests.
- **Configuration precedence**: `strategy_override` (CLI/HTTP) > `AgentPolicy.devcontainer.strategy` > env `BRANCHBOX_DEVCONTAINER_STRATEGY` > module default (`copy`).

## Error Handling Matrix
- **Permission denied** (`EACCES`, `EPERM`): Mark worktree `devcontainer_outdated`, emit warning, suggest manual remediation. Do not retry automatically until configuration changes.
- **Missing source** (`.devcontainer/` deleted): Downgrade to informational event, clear `last_sync_at`, notify control plane to prompt project maintainers.
- **Disk full** (`ENOSPC`): Abort job, escalate to control plane with severity `critical`, include disk usage snapshot if available.
- **Symlink unsupported** (Windows without developer mode): Force fallback to copy strategy, log downgrade, continue.
- **Unknown errors**: Capture stack trace, persist to `agent.log`, flag telemetry with `error.type`.

## Agent Test Plan
- **Unit**: Mock `ModuleExecutor` to verify job orchestrates strategy precedence and registry updates.
- **Integration**: Spin up ephemeral workspaces via devcontainer; run automated scenario:
  1. Modify `.devcontainer/devcontainer.json` → watch event triggers sync → verify feature worktree reflects change.
  2. Force permission error by chowning `.devcontainer/compose.yaml` to root → ensure job marks worktree `devcontainer_outdated`.
- **E2E smoke**: With control plane prototype, invoke `/v1/devcontainers/sync` and assert telemetry matches expected counts.
- **Regression**: Add cases to agent CI making sure `branchbox devcontainer sync --dry-run` returns zero exit status and does not mutate files.

## Manual Validation Guidelines
- Treat `branchbox devcontainer sync --json` (add `--dry-run` for a read-only probe) as the canonical probe: run it after any agent-side change to confirm the per-worktree results and registry metadata (`devcontainer_outdated`, `last_sync_at`, `sync_strategy`) remain consistent. Decode stdout even on exit 1: failed rows are reported in-band.
- Exercise watcher-triggered syncs by editing `.devcontainer/` in quick succession; healthy setups emit a single job thanks to debounced file events.
- Before coordinating with the control plane, rehearse the workflow locally: invoke the forthcoming `/v1/devcontainers/sync` equivalent via `curl` against a staging agent and verify authentication, rate limits, and payload schema.
- When rehearsing failure paths, walk through the error matrix manually (permission denied, missing source, disk full, unsupported symlink) and confirm log output plus registry flags match the documented expectations.
- Capture telemetry during each validation session—OpenTelemetry spans and metrics should surface strategy choice, duration, and outcome so the control plane dashboard mirrors reality.
- Keep operator documentation current: after every validation cycle, update runbooks and onboarding snippets so field teams can replicate the procedure without rediscovering steps.

## Documentation Website Workflow
- Publish user-facing documentation with `Docusaurus`. Source files live under `docs/docs/`; keep specs automation untouched in `docs/features/`.
- The devcontainer ships with Node.js 20; on bare-metal setups install Node.js and npm, then install dependencies with `cd docs && npm install`, and build locally with `cd docs && npm run build`.
- CI must always include a fast `npm run build` check on PRs (in the docs directory). A dedicated Pages workflow deploys the built site to GitHub Pages on successful pushes to `main`.
- Keep CLI reference pages up to date: regenerate `docs/docs/reference/cli.md` from `branchbox --help` and the `--help` of every subcommand (recursively) during releases or when command flags change. `docs/docs/reference/configuration.md` is generated from the config key registry (`UPDATE_CONFIG_REFERENCE=1 cargo test -p worktree-core config_edit`); never edit it by hand. When a `--json` payload or error code changes, update `docs/docs/reference/json-contract.md` in the same change.
- Engineers and coding agents must update documentation content as needed, mirror critical entry points in `README.md`, and document any automation adjustments in this file so future contributors know how docs are built and shipped.

## macOS app
- `macos/` is a Swift package with zero dependencies: swift-tools-version 6.2, Swift 6 language mode (complete strict concurrency), macOS 26 minimum. Build with the latest stable Xcode (Xcode 26); CI uses the same OS and toolchain.
- Targets: `BranchBoxKit` (Foundation only: backend protocol, models, pure planning), `BranchBoxCLI` (process runner, login-shell environment, CLI locator, `CLIBackend`), `BranchBoxStores` (`@MainActor @Observable` state; depends on Kit only), `BranchBoxPreview` (`PreviewBackend`), `BranchBoxApp` (product `BranchBox`; the only place a `CLIBackend` is constructed), `BranchBoxTestSupport` and five Swift Testing targets.
- The app talks to the user's installed `branchbox` only through `--json` commands and the JSON contract below. It never writes repository files itself (config goes through `config apply`, tokens through `tunnel credentials set --api-token-stdin`), never runs `branchbox prune`, and gates features on `version --json` capabilities, falling back to "legacy mode" for 0.13.x.
- Build and test: `swift build --package-path macos --build-tests -Xswiftc -warnings-as-errors` and `swift test --package-path macos --parallel`. Warnings are errors in CI.
- Integration suites run only with `BRANCHBOX_IT=1` and `BRANCHBOX_IT_CLI=<path to branchbox>` (they use disposable temp repos, no Docker). Render suites write offscreen screenshots only with `BRANCHBOX_RENDER_DIR` set.
- Fixtures: real, path-scrubbed 0.13.4 captures live in `macos/Tests/BranchBoxTestSupport/Fixtures/cli-0.13.4/` (never commit `/Users/<name>` or temp paths). The Kit tests also decode the Rust golden fixtures in `cli/tests/fixtures/contract/` via `#filePath`, so a payload change must keep both green.
- Dev loops: `cd macos && swift run BranchBox` (unbundled, no notifications), `BRANCHBOX_CLI_PATH=…` to test a branch CLI, `BRANCHBOX_BACKEND=preview BRANCHBOX_PREVIEW_SCENARIO=<name>` (debug builds) for a fake backend, and `scripts/macos-dev.sh [--open]` for a real dev bundle. Package with `scripts/package-macos-app.sh`.
- CI: `.github/workflows/macos-app.yml` runs `test` (macOS 26 / latest stable Xcode; guards against gRPC/SwiftProtobuf/NIO imports and sheets or alerts in the menu bar), `integration` (same-commit CLI), `integration-floor` (released 0.13.4) and `package` (uploads the app artifact).
- `macos/README.md` is the user and developer guide; keep it current when app behaviour or the dev loop changes.

## API/JSON contract
The `--json` output of the CLI is a public API: the Mac app, scripts and CI parse it. `docs/docs/reference/json-contract.md` documents it.
- **Machine mode.** Any `--json` flag switches the process into machine mode right after argument parsing: stdout carries exactly one JSON document (the payload or the error envelope), human text goes to stderr, and nothing prompts (`output::is_interactive()` is false). A new `--json` flag must also be reported by its command's `wants_json()`.
- **Error envelope.** In machine mode a failure prints `{"schema_version":1,"error":{"code","message","causes","details"}}` on stdout; stderr keeps the `Error: …` text. Exit codes do not change (0 success, 1 failure, 2 usage, 75 dispatch-tool not pending, 101 panic). Commands that report failure in-band (`feature exec`, `devcontainer up/build/exec`, `dispatch-tool`, `doctor`, `devcontainer sync`, `prune --yes`) print their payload and exit 1 without an envelope. `devcontainer down` cleanup failures use the error envelope.
- **Stable codes.** Error codes (`teardown_refused`, `worktree_not_found`, `feature_not_found`, `config_invalid`, `registry_locked`, …) are API. Use `CliError` or `json_error::recode` so a refusal keeps its code; never change an existing code's meaning.
- **Capabilities.** Add a capability string to the implementing module's `CAPABILITIES` in the same change that implements it; `branchbox version --json` aggregates them. Bump `contract_version` only for a breaking change.
- **Additive only.** Existing payloads only gain keys; new payloads carry `"schema_version": 1`. **Never add a `FeatureStatus` variant**: the serde enum is closed, and older CLIs and apps must keep reading a shared registry. Put new state in optional `#[serde(default)]` fields.
- **Golden fixtures.** Contract tests compare output to `cli/tests/fixtures/contract/<area>/*.json` (paths normalized). After an intended change, regenerate with `UPDATE_CONTRACT_FIXTURES=1 cargo nextest run -p branchbox-cli`, review the diff, and keep the Swift decode tests green.

## Teardown and output guardrails
- Teardown never deletes uncommitted user work without `--force` or `--discard-changes`, and never deletes unmerged commits without `--force-delete-branch` or `--force`. Refusals happen before anything is removed and name the override. The classification of changes (user vs BranchBox-generated vs preserved spec) lives in `core/src/workflows/teardown_plan.rs`; change it there, with tests, never by adding path allow-lists elsewhere.
- `--force` stays coupled to `git branch -D` for compatibility (harnesses and in-guest orchestrators rely on it). Do not decouple it without a deprecation cycle; recommend `--discard-changes` in messages instead.
- Stdout in `--json` mode is one JSON document. Print human text with `humanln!`/`human!` and JSON with `output::emit_json`. `println!`/`print!` are disallowed in `core` and `cli` by `clippy.toml` (`disallowed-macros`); do not `allow` them.

## Known Issues & TODOs
Recent code review identified: incorrect repository URL in `Cargo.toml` (`branchbox-branchbox`), placeholder author metadata, generic `anyhow::Error` usage (migrate to `thiserror` domain errors), missing CLI input validation, hardcoded config (Docker networks, port ranges, spec templates), and insufficient unit test coverage for registry operations and module implementations.

## Project Skills
- `.claude/skills/branchbox-release-videos/` produces BranchBox launch and focused documentation films from verified feature evidence. Use it when preparing release/launch media; delegate its production to a video subagent and coordinate reviewed embeds with the documentation agent.
- `skills/branchbox-devcontainer-guardrails/` captures repeatable implementation + validation guardrails for devcontainer/bootstrap/compose/harness/release-sensitive changes.
- Use the skill whenever a change touches 1Password auth/signing bootstrap, compose template naming/mount behavior, feature env/stash mechanics, or manual E2E harnesses.
- Keep the skill references (`references/gotchas.md`, `references/validation-checklist.md`) synchronized with AGENTS expectations and the corresponding manual harness docs.
