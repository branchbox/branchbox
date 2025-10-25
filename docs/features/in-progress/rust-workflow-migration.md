---
worktree: /Users/rbarazi/projects/branchbox
branch: feat/rust-workflow-m0
work_feature: rust-workflow-migration
status: in-progress
updated: 2025-10-31
created: 2025-10-29
---

# Rust Workflow Parity with Legacy Shell Toolkit

## Overview

Migrate the Bash-based feature lifecycle in `lib/` (`feature-start`, `feature-teardown`, adapters, modules, utilities) into first-class Rust commands so the new core library and CLI can replace the scripting layer entirely.

## Current State Analysis

✅ **Rust Provides:**
- Foundational crates (`core/`, `cli/`) with naming, validation, git worktree primitives, adapter and module traits, and devcontainer bootstrap templating.
- Basic CLI (`branchbox`) for stack detection, bootstrap generation, and naming utilities.

⚠️ **Still Shell-Only (lib/):**
- Interactive feature start/teardown workflows (branching, worktree creation, spec management, copy/sanitize `.env` and `.devcontainer`).
- Module implementations with real side effects: compose validation, database env/volume hints, Cloudflare tunnel provisioning via API or manual prompts, specs lifecycle moves.
- Adapter glue (secret copying, service URL propagation, cleanup) and Cloudflare API utilities.
- Rich prompts/logging, stash handling, `.git` relative-path fixer, and conflict detection.

## Migration Goals

1. **Orchestrate Workflows in Rust**
   - Add `branchbox feature start` and `feature teardown` commands (or equivalent agent gRPC handlers) that mirror `lib/feature-start` & `lib/feature-teardown` flows end-to-end.
   - Support sub-feature branching, base-branch prompts, stash capture/apply, and worktree pruning.

2. **Port Module & Adapter Behavior**
   - Move shell module logic into `worktree_core::modules` with real implementations: compose validation, database setup instructions & cleanup, Cloudflare automation, specs file moves/frontmatter updates.
   - Extend adapters to expose secret copy, service URL, teardown semantics without relying on shell hooks.

3. **Integrate Cloudflare API Client**
   - Replace TODOs in `TunnelModule` with concrete API requests, DNS cleanup, and manual fallback prompts using a Rust HTTP client (reqwest).

4. **Environment & Config Handling**
   - Reproduce `.env` split/linking, `.devcontainer` sync, `.cloudflared.env` generation, and git path fix script creation within Rust commands.
   - Standardize devcontainer mounts so every worktree is available at `/workspaces/<feature>` (matching VS Code defaults) while the parent repository remains mounted for git metadata.
   - Allow `.env` to declare `APP_NAME`/`APP_SLUG` to control compose/devcontainer naming; fall back to the parent repo folder when unset.

5. **Parity-Level UX**
   - Provide colored logging, confirmation prompts with defaults, and non-interactive `--yes` support akin to `lib/utils/*.sh`.

## Deliverables

- Updated CLI (and later agent) exposing feature lifecycle commands.
- Expanded `worktree_core` modules/adapters with functional parity plus tests.
- New Cloudflare API integration module with unit/integration coverage.
- Documentation and examples showing shell -> Rust migration completion.

## Architecture Blueprint

- **CLI Surface (`cli/src/commands/feature.rs`)** routes `feature start/teardown/list` into a thin orchestration layer, handling argument parsing (`--base`, `--yes`, `--skip-*`) and streaming progress events to the terminal.
- **Workflow Coordinator (`core/src/workflows/feature.rs`)** owns the high-level state machine: git discovery, branch creation, worktree operations, module/adapter dispatch, and failure unwinding. This layer composes existing `GitWorktree` helpers plus new `PromptService` abstractions.
- **Module & Adapter Registry** exposes dynamic lookup via typed registries (e.g., `ModuleRegistry::resolve("cloudflare_tunnel")`) backed by `serde`-driven config. Modules implement trait-based start/teardown hooks with shared context objects for environment mutations.
- **State & Config Store** persists feature metadata (spec path, base branch, module decisions) in `.branchbox/feature.json`, enabling resume/retry flows and future analytics. Store is versioned to allow migrations.
- **I/O Abstractions** introduce `TerminalUi` (for colored output, spinners, prompts) and `NonInteractiveUi` (for automation). Both share a `LogEvent` enum so the CLI/UI layer can decide presentation (TTY, JSON, gRPC).
- **Error & Recovery Strategy** centralizes retryable vs fatal errors, ensuring partial worktrees are cleaned or instructions surfaced to users for manual fixes.

## Milestones & Timeline

1. **Milestone 0 — Foundations (Week 1)** ✅ *Completed 2025-10-31*
   - ✅ Scaffold CLI command module (`branchbox feature`) and feature workflow coordinator with initial handlers.
   - ✅ Port existing git helpers; implement feature metadata persistence (`.branchbox/feature.json`) and validation.
   - ✅ Create doc-driven test fixtures for simple start/teardown happy paths (`workflows::feature` unit tests).
2. **Milestone 1 — Core Workflow Parity (Weeks 2-3)**
   - Implement branching/worktree creation, stash capture/restore, spec lifecycle, `.env` split generation.
   - Wire module registry with placeholders; execute adapters in order; ensure teardown can unwind start artifacts.
   - Ship behind `BRANCHBOX_ENABLE_FEATURE_WORKFLOW=1` or `--experimental`.
3. **Milestone 2 — Module Migration (Weeks 4-6)**
   - Port Compose, database, specs, and Cloudflare modules with real side effects and targeted tests.
   - Add telemetry hooks and structured logging; ensure manual prompts map to shell parity.
   - Deliver integration tests covering multi-module flows and teardown idempotency.
4. **Milestone 3 — Cloudflare Automation & UX Polish (Weeks 7-8)**
   - Integrate Cloudflare API client, fallback prompts, and cleanup routines.
   - Add colored progress output, `--yes`, dry-run, and enhanced error recovery.
   - Produce docs, migration guides, and cut release flagging shell scripts as deprecated.

## Tooling & Dependency Updates

- Add crates: `inquire` (or `dialoguer`) for prompts, `indicatif` for spinners, `console` for styling, `reqwest` + `serde_json` for Cloudflare API, `serde_yaml` for spec manipulation, `tempfile`/`assert_cmd` for tests.
- Introduce feature-gated dependencies (e.g., `cloudflare` feature flag) so the base CLI remains lean.
- Extend `Cargo.toml` with workspace-level dev-dependencies for integration tests and fixture utilities.
- Define shared test helpers under `core/src/test_support/` for git repo setup, `.env` assertions, and fixture copying.

## Rollout & Compatibility Plan

- Ship experimental commands alongside shell scripts; expose `branchbox feature --use-shell` escape hatch until parity is validated.
- Provide migration script that compares outputs between shell and Rust flows for selected repositories.
- Capture telemetry (or structured logs) to identify slow steps and failure points during beta rollout.
- Once stable, update `README` and deprecate shell entry points with a clear sunset timeline communicated in release notes.

## Metrics & Observability

- Track runtime durations for start/teardown and sub-steps to detect regressions.
- Measure success/error rates per module, capturing Cloudflare API failures distinctly.
- Record prompt response patterns (skipped modules, non-interactive usage) to guide UX refinements.
- Add `branchbox doctor` report summarizing the last feature workflow run, persisted in `.branchbox/history.json`.

## Open Questions

- How do we support remote-only workflows (no local git worktree) while preserving parity?
- Should module configuration remain declarative (YAML/TOML) or transition to Rust plugin crates?
- What’s the long-term story for agents invoking the workflow—CLI subprocess, gRPC server, or library API?
- How do we secure Cloudflare credentials when running in CI or ephemeral environments?

## Testing Strategy

- Unit tests for branching logic, stash workflows, spec frontmatter edits, Cloudflare response handling, and module detection.
- Integration tests that spin up temp git repos to exercise feature start/teardown flows (use `tempfile`, `assert_cmd`).
- Regression tests verifying `.env` diffing, compose validation, and Cloudflare fallback messaging.

## Risks & Mitigations

- **API changes**: start behind CLI flags/env to allow gradual rollout alongside shell scripts.
- **Permission issues**: emulate shell’s cautious prompts before destructive operations; add dry-run mode.
- **Environment drift**: use snapshot tests for generated config files and spec updates.

## Progress Log

- **2025-10-31**: Created `feat/rust-workflow-m0`, added `branchbox feature start/teardown` commands backed by new Rust workflow coordinator. Metadata now persisted in `.branchbox/feature.json` with `FeatureStatus` tracking. Added CI-friendly host validation override and refreshed naming heuristics to support metadata serialization.
- **2025-10-31**: Introduced `branchbox feature list` for registry introspection, including status filtering and local-time formatting. Added workflow API to enumerate feature metadata and expanded test coverage for listing semantics.
- **2025-10-31**: Added CLI integration test harness using `assert_cmd` to exercise `feature start/list/teardown` end-to-end. Manually verified workflow in a temporary git repository to confirm CLI ergonomics.
- **2025-10-31**: Ported spec lifecycle bootstrap: backlog specs are auto-discovered, moved to `in-progress`, frontmatter is refreshed during `feature start`, fresh specs are auto-generated when none exist, and `branchbox feature teardown --complete-spec` now moves specs to `completed`. Added unit and integration coverage for the new path.
- **2025-11-01**: Replaced shell-based Cloudflare tunnel provisioning with a Rust `CloudflareClient` and integrated it into `TunnelModule`. CLI now provisions tunnels, configures ingress/DNS, reuses existing tokens, and performs API-based teardown with graceful manual fallbacks. Added unit coverage for API parsing and token discovery.
- **2025-11-02**: Wired stack adapter orchestration into the Rust workflow: `feature start` now auto-detects adapters, copies stack secrets for new worktrees, injects service URLs for module setup, and `feature teardown` runs adapter cleanup hooks. CLI output reflects adapter details and associated warnings.
- **2025-11-02**: Hardened the module registry: detection now returns dependency-ordered plans with warnings, `feature start` rolls back prior modules on failure, and teardown executes in reverse order while surfacing cleanup issues.
- **2025-11-02**: Added automatic stash capture/restore to the Rust workflow so uncommitted changes are moved into the new worktree (with warnings when stash pop fails), ported spec lifecycle bootstrapping/completion into `FeatureWorkflow`, and ensured `.env` base configuration is copied without legacy feature sections. `.env` reuse now replaces existing branchbox blocks with warnings so manual edits are surfaced. Archived shell modules/adapters under `lib/migrated/` after confirming parity. Remaining shell assets (feature scripts, adapter glue, utilities) stay in place until their Rust counterparts ship.
- **2025-11-03**: Fixed repo-root discovery so CLI commands work from nested directories, reuse existing branches without resetting history, and derive Cloudflare tunnel hostnames from `APP_URL` to provision usable tunnels/DNS records. Added regression coverage for `.env` reuse and spec completion. Introduced optional telemetry logging (via `--telemetry`) that surfaces Cloudflare provisioning events.

## Next Actions

1. Polish telemetry UX (structured summaries, opt-in JSON stream) and add retries/dry-run semantics for Cloudflare module.
2. Expand integration test harness for start/teardown flows across modules (include rollback/error scenarios and Cloudflare mocks).
3. Document CLI usage (`branchbox feature`) and bridge legacy shell scripts with experimental flag guidance.
4. Refresh devcontainer guidance to call out the `/workspaces/<feature>` mount requirement, Cargo cache location, and the shared `/workspaces` bind used by the workflow runtime.
5. Plan shell deprecation timeline and removal sequence once parity validation completes.
