# Repository Guidelines

## Project Overview & Current State
BranchBox is a distributed development environment orchestrator managing git worktrees and devcontainers. Milestone 0 is complete—core workflow orchestration for feature worktrees is implemented in Rust. Milestone 1 shipped the Rust agent daemon (macOS/Linux/devcontainer) plus CLI bridge + telemetry so long-running workflows can run in the background. Milestone 2 added the control-plane HTTP drain, `branchbox agent status`, and a SwiftUI macOS preview app that rides on the same gRPC surface as the CLI. Next up: Windows transport, full Rails control plane UX, and tighter Tailscale coordination so the offline-first contract holds across devices.

## Project Structure & Module Organization
The workspace roots at `Cargo.toml` and currently ships two members: the core library in `core/` and the CLI in `cli/`. Core modules live under `core/src/` (notably `adapters/`, `modules/`, `bootstrap/`, and cross-cutting helpers like `git.rs`). The CLI entry point is `cli/src/main.rs`, exporting the `branchbox` binary on behalf of the library. Shared documentation sits in `docs/`, CI workflows in `.github/workflows/`, and reproducible tooling in `.devcontainer/`. Future members (agent, control-plane, macos) are commented out in the root workspace until their respective milestones.

## Build, Test, and Development Commands
Run workspace builds with `cargo build` and optimize releases via `cargo build --release`. Execute the CLI locally with `cargo run -p branchbox-cli -- --help` to validate argument wiring. Use `cargo fmt --all -- --check` to enforce formatting, `cargo clippy --all-targets --all-features -- -D warnings` for linting, and `cargo check` for quick iteration. Security and dependency scanning is covered by `cargo audit`.

## Coding Style & Naming Conventions
Rust files follow `rustfmt` defaults (4-space indentation, 100-column soft limit). Modules and files use `snake_case`; types are `UpperCamelCase`; constants are `SCREAMING_SNAKE_CASE`. Prefer explicit `Result<T, Error>` aliases plus the `?` operator for flow control, and leverage `thiserror` for rich domain errors. Branch names should stay action-oriented, e.g., `feature/bootstrap-cleanup` or `fix/git-lock-race`.

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
- Avoid fixed compose `name` or `container_name` values in templates; worktrees must remain parallel-safe.
- Preserve compatibility by supporting both `docker compose` and `docker-compose` in workflow/module orchestration.
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

When touching the macOS client or anything gRPC-related, run the “Mac App ↔ Agent Loop” from `docs/docs/getting-started/manual-cli-e2e.md`: start the agent locally, point the SwiftUI preview at your workspace, issue start/teardown operations from the UI, and verify the HTTP drain logs show the matching events.

The harness intentionally edits the feature devcontainer before teardown to exercise the dirty-worktree guard, so an initial `feature teardown` failure followed by the scripted `--force` retry is expected. Use `KEEP_E2E_TMP=1` when you need to inspect the generated workspace for failures, and block merges until the script succeeds.
CI runs the harness for `rust`, `generic`, `rails`, and `node`; if you touch another stack locally, mirror that by passing `--stack <stack>` when running the script.

### Release workflow
- Follow `RELEASING.md` verbatim. The short version: ensure `main` is up to date, run fmt/clippy/tests/docs, then execute the six manual CLI harness permutations listed above (regular/verbose/pretend × rust/generic/rails/node). Releases are blocked until every combination passes locally.
- Always refresh demo media before tagging: `./scripts/remotion-docs-all.sh --stack rust --target both`, then verify the combined site bundle with `./scripts/build-site.sh --skip-demo-assets`.
- Update `CHANGELOG.md` with highlights, refresh `README.md` + `docs/docs/**` (especially the manual CLI E2E pages, first-run `branchbox init` UX + 1Password/git bootstrap caveats in quick start, and CLI reference pages), and capture any new expectations here in `AGENTS.md` before tagging. Regenerate `docs/docs/reference/cli.md` by pasting `branchbox --help` output whenever flags change.
- Confirm social/share variants remain available (`branchbox-teaser-*-social-square.mp4`, `branchbox-teaser-*-social-vertical.mp4`, and `branchbox-teaser-*-social-card.jpg`) before release handoff.
- Keep `docs/docs/getting-started/manual-cli-e2e.md` + `scripts/manual-cli-e2e.md` and `docs/docs/getting-started/manual-1password-e2e.md` + `scripts/manual-1password-e2e.md` synchronized with the actual harness steps—future contributors should be able to trace every required validation from those docs.
- Run `cargo release --workspace --dry-run` before `--execute` so you can catch version bumps or git state issues early. Push with `git push --follow-tags` and monitor the release workflow with `gh run watch`.
- After tagging, confirm the docs build (`cd docs && npm run build`), the GitHub Pages deployment, and downstream taps (Homebrew) before announcing the release.

### Compatibility & template hygiene
- When touching JSON/state schemas (e.g., `.branchbox/registry.json`), add backward-compatible deserializers or migrations before landing the change. Existing workspaces must continue working without manual edits.
- When editing code-generated assets (devcontainer or compose templates), re-run `cargo test` to catch expectation drift (the template tests in `core/src/bootstrap/templates.rs` enforce current bind mounts and shared volume paths).

## Commit & Pull Request Guidelines
Recent history favors concise, imperative summaries (e.g., `Refactor CLI to 'branchbox' with grouped subcommands`). Continue that tone while adopting the Conventional Commit prefix expected in `CONTRIBUTING.md`, such as `feat(modules): add docker compose planner`. Before opening a PR, rebase on `main`, rerun fmt/clippy/tests/doc checks, and attach context: problem statement, scope, linked issues, and any relevant CLI transcripts or screenshots. Ensure the CI suite is green before requesting review.

For user-facing changes (CLI UX, website, onboarding/docs flows), treat demo freshness as required PR hygiene: run `./scripts/remotion-docs-all.sh --stack rust --target both`, run `./scripts/build-site.sh --skip-demo-assets`, and fill the PR template's demo lifecycle checklist.

Prefer the GitHub CLI (`gh pr create --fill`) for opening PRs after pushing the branch so reviewers get the templated context and automation can rely on consistent metadata.

## Architecture Essentials

### Adapters vs Modules
Adapters provide stack-specific behavior (Rails vs Node.js vs Generic), detecting project type via marker files and returning confidence scores 0-100. Modules are composable cross-cutting features (compose, database, tunnel, specs) that run during worktree lifecycle. Both use trait objects for polymorphism; adapters are detected once per workflow, while modules are detected and executed in dependency order via topological sort.

### State Management
The `FeatureStateStore` tracks worktrees in `{repo_root}/.branchbox/registry.json` with schema: `work_feature`, `branch_name`, `worktree_path`, `feature_url`, `status` (Active|Removed), `created_at`, `updated_at`. This JSON store will migrate to SQLite when the agent daemon is introduced.

Future enhancement: when PRs are opened via `gh`, persist their number/URL back into the feature registry so the agent/control plane can display review status without re-querying GitHub.

### Specs Module Behavior
During feature start, the specs module promotes `docs/features/backlog/{name}.md` to `docs/features/in-progress/{name}.md` (or creates a stub with front matter if missing). During teardown with `--complete-spec`, it moves from `in-progress/` to `completed/`.

## Environment & Configuration Tips
Use the provided devcontainer (`.devcontainer/`) for a consistent toolchain; it preinstalls Rust, Clippy, cargo-nextest, cargo-llvm-cov, and Docker. The container runs privileged for Docker-in-Docker. Tool configurations (`.codex/`, `.claude-code/`, `.gh/`) are volume-mounted via the `SHARED_CONFIG_DIR` environment variable (defaults to `../..`, the parent directory), ensuring credentials and session state persist across container rebuilds and are shared across all feature worktrees—authenticate once with `gh auth login` in any worktree and credentials are available everywhere. Non-worktree users can override with `SHARED_CONFIG_DIR=..` in `.env`. Local setups should copy `.env.sample` into a private `.env` and avoid committing secrets. Tests should set `BRANCHBOX_SKIP_HOST_VALIDATION=1` to bypass host checks. During feature start, the workflow copies `.env` from repo root to worktree and injects `APP_URL` and `COMPOSE_PROJECT_NAME`.

## Module Implementation: Devcontainer
- **Detection**: `DevcontainerModule::detect` returns true when `.devcontainer/` exists in the main worktree. Agent bootstrap should ensure the directory is present before queuing the module.
- **Init/Setup flow**: `init` captures the source `.devcontainer/` path and picks a sync strategy (`copy` by default, override via `BRANCHBOX_DEVCONTAINER_STRATEGY`). `setup` invokes `sync_to(feature_dir)` to mirror files into each worktree, skipping excluded entries like `.env`.
- **Strategies**: Copy keeps feature-specific edits isolated; symlink keeps worktrees auto-updated. Agents may expose a policy knob but must default to copy to avoid permission prompts on macOS.
- **Sync command**: `branchbox devcontainer sync [--strategy copy|symlink] [--dry-run]` replays the module across all registered worktrees. Agents should call this after updating `.devcontainer/` in the main repo or during migrations.
- **Telemetry hooks**: The module emits tracing spans (`module.devcontainer.sync`) with outcome, duration, and strategy. Capture these for observability dashboards and to flag stale worktrees (module failures should surface as soft errors).
- **Feature flags**: Gate early rollouts with `BRANCHBOX_ENABLE_DEVCONTAINER_MODULE`. Agents can toggle this per-workspace to coordinate canary deploys.
- **Failure handling**: If sync fails, mark the worktree as `devcontainer_outdated` in registry metadata and warn the user instead of aborting the workflow. Agents should surface remediation guidance in the CLI/UX.
- **Shared credentials**: Confirm shared mounts remain intact (`.gh`, `.claude`, `.codex`) after sync or teardown. Agents must never delete host-side shared directories.

## Agent Integration Plan
- **Daemon wiring**: Expose a `DevcontainerSyncJob` in the Rust agent that triggers when `.devcontainer/` changes in the main worktree (file watcher) or when a new worktree registers. Job should enqueue module execution via the existing workflow runner.
- **Command bridge**: Use the CLI as a fallback (`branchbox devcontainer sync --json`) until native library bindings are exported. Parse the sync outcome to update registry metadata and emit structured logs.
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
- Treat `branchbox devcontainer sync --json` as the canonical probe: run it after any agent-side change to confirm the CLI contract and registry metadata (`devcontainer_outdated`, `last_sync_at`, `sync_strategy`) remain consistent.
- Exercise watcher-triggered syncs by editing `.devcontainer/` in quick succession; healthy setups emit a single job thanks to debounced file events.
- Before coordinating with the control plane, rehearse the workflow locally: invoke the forthcoming `/v1/devcontainers/sync` equivalent via `curl` against a staging agent and verify authentication, rate limits, and payload schema.
- When rehearsing failure paths, walk through the error matrix manually (permission denied, missing source, disk full, unsupported symlink) and confirm log output plus registry flags match the documented expectations.
- Capture telemetry during each validation session—OpenTelemetry spans and metrics should surface strategy choice, duration, and outcome so the control plane dashboard mirrors reality.
- Keep operator documentation current: after every validation cycle, update runbooks and onboarding snippets so field teams can replicate the procedure without rediscovering steps.

## Documentation Website Workflow
- Publish user-facing documentation with `Docusaurus`. Source files live under `docs/docs/`; keep specs automation untouched in `docs/features/`.
- The devcontainer ships with Node.js 20; on bare-metal setups install Node.js and npm, then install dependencies with `cd docs && npm install`, and build locally with `cd docs && npm run build`.
- CI must always include a fast `npm run build` check on PRs (in the docs directory). A dedicated Pages workflow deploys the built site to GitHub Pages on successful pushes to `main`.
- Treat demos as first-class website/docs content: render with `./scripts/remotion-docs-all.sh --target both` (includes social variants) and verify packaging via `./scripts/build-site.sh`.
- The Pages deployment pipeline now uses `scripts/build-site.sh` so demo media is rendered during deploy, not manually copied.
- Keep CLI reference pages up to date: manually regenerate `docs/docs/reference/cli.md` by capturing `branchbox --help` output and its subcommands during releases or when command flags change.
- Engineers and coding agents must update documentation content as needed, mirror critical entry points in `README.md`, and document any automation adjustments in this file so future contributors know how docs are built and shipped.

## macOS Proto Bindings
- The macOS app now consumes generated SwiftProtobuf + gRPC Swift stubs checked into `macos/Sources/BranchBoxApp/Generated/`.
- Regenerate bindings after editing `agent/proto/agent.proto` by running `./scripts/generate-swift-protos.sh` (requires Docker; the script caches toolchains under `.build/swift-proto-tools`).
- Generated files must be committed with the proto change so the mac app stays in sync even when contributors skip the generator.
- Do not hand-edit the generated files; adjust the proto definitions instead.

## Known Issues & TODOs
Recent code review identified: incorrect repository URL in `Cargo.toml` (`branchbox-branchbox`), placeholder author metadata, generic `anyhow::Error` usage (migrate to `thiserror` domain errors), missing CLI input validation, registry race conditions (check + create isn't atomic), hardcoded config (Docker networks, port ranges, spec templates), and insufficient unit test coverage for registry operations and module implementations.

## Project Skills
- `skills/branchbox-devcontainer-guardrails/` captures repeatable implementation + validation guardrails for devcontainer/bootstrap/compose/harness/release-sensitive changes.
- Use the skill whenever a change touches 1Password auth/signing bootstrap, compose template naming/mount behavior, feature env/stash mechanics, or manual E2E harnesses.
- Keep the skill references (`references/gotchas.md`, `references/validation-checklist.md`) synchronized with AGENTS expectations and the corresponding manual harness docs.
