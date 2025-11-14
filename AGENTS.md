# Repository Guidelines

## Project Overview & Current State
BranchBox is a distributed development environment orchestrator managing git worktrees and devcontainers. Milestone 0 is complete—core workflow orchestration for feature worktrees is implemented in Rust. The CLI supports `branchbox feature start/teardown/list` with full lifecycle management. Future milestones will add a Rust agent daemon, Rails control plane, and native macOS app, all coordinating via Tailscale in an offline-first architecture.

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

The harness intentionally edits the feature devcontainer before teardown to exercise the dirty-worktree guard, so an initial `feature teardown` failure followed by the scripted `--force` retry is expected. Use `KEEP_E2E_TMP=1` when you need to inspect the generated workspace for failures, and block merges until the script succeeds.
CI runs the harness for `rust`, `generic`, `rails`, and `node`; if you touch another stack locally, mirror that by passing `--stack <stack>` when running the script.

### Release workflow
- Follow `RELEASING.md` verbatim. The short version: ensure `main` is up to date, run fmt/clippy/tests/docs, then execute the six manual CLI harness permutations listed above (regular/verbose/pretend × rust/generic/rails/node). Releases are blocked until every combination passes locally.
- Update `CHANGELOG.md` with highlights, refresh `README.md` + `docs/docs/**` (especially the manual CLI E2E and CLI reference pages), and capture any new expectations here in `AGENTS.md` before tagging. Regenerate `docs/docs/reference/cli.md` by pasting `branchbox --help` output whenever flags change.
- Keep `docs/docs/getting-started/manual-cli-e2e.md` and `scripts/manual-cli-e2e.md` synchronized with the actual harness steps—future contributors should be able to trace every required validation from those docs.
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
- Keep CLI reference pages up to date: manually regenerate `docs/docs/reference/cli.md` by capturing `branchbox --help` output and its subcommands during releases or when command flags change.
- Engineers and coding agents must update documentation content as needed, mirror critical entry points in `README.md`, and document any automation adjustments in this file so future contributors know how docs are built and shipped.

## macOS Proto Bindings
- The macOS app now consumes generated SwiftProtobuf + gRPC Swift stubs checked into `macos/Sources/BranchBoxApp/Generated/`.
- Regenerate bindings after editing `agent/proto/agent.proto` by running `./scripts/generate-swift-protos.sh` (requires Docker; the script caches toolchains under `.build/swift-proto-tools`).
- Generated files must be committed with the proto change so the mac app stays in sync even when contributors skip the generator.
- Do not hand-edit the generated files; adjust the proto definitions instead.

## Known Issues & TODOs
Recent code review identified: incorrect repository URL in `Cargo.toml` (`branchbox-branchbox`), placeholder author metadata, generic `anyhow::Error` usage (migrate to `thiserror` domain errors), missing CLI input validation, registry race conditions (check + create isn't atomic), hardcoded config (Docker networks, port ranges, spec templates), and insufficient unit test coverage for registry operations and module implementations.
