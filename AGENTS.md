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
Unit tests live beside their modules under `#[cfg(test)]`; grow integration coverage in a `core/tests/` harness when cross-cutting behaviour warrants it. Run `cargo test --all-features` for the default gate, `cargo test --doc` to validate examples, and `cargo test -- --nocapture` when debugging. CI enforces 90% line coverage via Tarpaulin, so periodically run `cargo tarpaulin --out Html --all-features --workspace` to catch regressions.

## Commit & Pull Request Guidelines
Recent history favors concise, imperative summaries (e.g., `Refactor CLI to 'branchbox' with grouped subcommands`). Continue that tone while adopting the Conventional Commit prefix expected in `CONTRIBUTING.md`, such as `feat(modules): add docker compose planner`. Before opening a PR, rebase on `main`, rerun fmt/clippy/tests/doc checks, and attach context: problem statement, scope, linked issues, and any relevant CLI transcripts or screenshots. Ensure the CI suite is green before requesting review.

## Architecture Essentials

### Adapters vs Modules
Adapters provide stack-specific behavior (Rails vs Node.js vs Generic), detecting project type via marker files and returning confidence scores 0-100. Modules are composable cross-cutting features (compose, database, tunnel, specs) that run during worktree lifecycle. Both use trait objects for polymorphism; adapters are detected once per workflow, while modules are detected and executed in dependency order via topological sort.

### State Management
The `FeatureStateStore` tracks worktrees in `{repo_root}/.branchbox/registry.json` with schema: `work_feature`, `branch_name`, `worktree_path`, `feature_url`, `status` (Active|Removed), `created_at`, `updated_at`. This JSON store will migrate to SQLite when the agent daemon is introduced.

### Specs Module Behavior
During feature start, the specs module promotes `docs/features/backlog/{name}.md` to `docs/features/in-progress/{name}.md` (or creates a stub with front matter if missing). During teardown with `--complete-spec`, it moves from `in-progress/` to `completed/`.

## Environment & Configuration Tips
Use the provided devcontainer (`.devcontainer/`) for a consistent toolchain; it preinstalls Rust, Clippy, Tarpaulin, and Docker. The container runs privileged for Docker-in-Docker. Tool configurations (`.codex/`, `.claude/`, `.gh/`) are volume-mounted via the `SHARED_CONFIG_DIR` environment variable (defaults to `../..`, the parent directory), ensuring credentials and session state persist across container rebuilds and are shared across all feature worktrees—authenticate once with `gh auth login` in any worktree and credentials are available everywhere. Non-worktree users can override with `SHARED_CONFIG_DIR=..` in `.env`. Local setups should copy `.env.sample` into a private `.env` and avoid committing secrets. Tests should set `BRANCHBOX_SKIP_HOST_VALIDATION=1` to bypass host checks. During feature start, the workflow copies `.env` from repo root to worktree and injects `APP_URL` and `COMPOSE_PROJECT_NAME`.

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

## Operational Guidelines
- Capture `DevcontainerSyncOutcome` as a stable protobuf/JSON contract so downstream telemetry consumers remain compatible.
- Keep the registry schema in sync with sync metadata needs (`devcontainer_outdated`, `last_sync_at`, `sync_strategy`) and ship forward/backward-safe migrations whenever the shape evolves.
- Ensure CLI surfaces the latest registry fields (e.g., `devcontainer_outdated`) so operators can diagnose issues without dropping into raw JSON.
- Emit OpenTelemetry metrics/spans (`sync_success`, `sync_failed`, strategy labels) and forward them to the control plane for monitoring.
- Maintain runbooks for each error class in the handling matrix, covering detection, remediation, and escalation paths.
- Update onboarding/documentation promptly when the automated sync workflow changes to keep developer expectations aligned.
- Provide a CLI bridge for the agent (`branchbox devcontainer sync --json`) until native bindings are available; audit stdout/stderr for machine-readable consumption.
- Use debounced file watchers resilient to bursty writes so `.devcontainer/` edits trigger sync exactly once.
- Guard the `/v1/devcontainers/sync` control-plane endpoint with authentication/rate limits and mirror those policies in the agent.
- Validate the workflow in staging across multiple workspaces before promoting to production, exercising both success and failure paths.
- Coordinate security reviews for shared credential mounts and document mitigations before rolling out automation broadly.

## Known Issues & TODOs
Recent code review identified: incorrect repository URL in `Cargo.toml` (`branchbox-branchbox`), placeholder author metadata, generic `anyhow::Error` usage (migrate to `thiserror` domain errors), missing CLI input validation, registry race conditions (check + create isn't atomic), hardcoded config (Docker networks, port ranges, spec templates), and insufficient unit test coverage for registry operations and module implementations.
