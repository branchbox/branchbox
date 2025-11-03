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
Use the provided devcontainer (`.devcontainer/`) for a consistent toolchain; it preinstalls Rust, Clippy, cargo-nextest, cargo-llvm-cov, and Docker. The container runs privileged for Docker-in-Docker. Tool configurations (`.codex/`, `.claude-code/`, `.gh/`) are volume-mounted via the `SHARED_CONFIG_DIR` environment variable (defaults to `../..`, the parent directory), ensuring credentials and session state persist across container rebuilds and are shared across all feature worktrees—authenticate once with `gh auth login` in any worktree and credentials are available everywhere. Non-worktree users can override with `SHARED_CONFIG_DIR=..` in `.env`. Local setups should copy `.env.sample` into a private `.env` and avoid committing secrets. Tests should set `BRANCHBOX_SKIP_HOST_VALIDATION=1` to bypass host checks. During feature start, the workflow copies `.env` from repo root to worktree and injects `APP_URL` and `COMPOSE_PROJECT_NAME`.

## Documentation Website Workflow
- Publish user-facing documentation with `mdBook`. Source files live under `docs/book.toml` + `docs/src/`; keep specs automation untouched in `docs/features/`.
- The devcontainer ships with mdBook `0.4.40`; on bare-metal setups install the same version via `cargo install mdbook --locked --version 0.4.40`, then build locally with `mdbook build docs`.
- CI must always include a fast `mdbook build docs` check on PRs. A dedicated Pages workflow deploys the rendered book to `gh-pages` on successful pushes to `main`.
- Keep CLI reference pages generated: use the helper script (check `docs/scripts/render-cli-reference.sh` once added) that captures `branchbox --help` output into `docs/src/reference/`. Regenerate during releases or when command flags change.
- Engineers and coding agents must update the book’s `SUMMARY.md` whenever new guides are added, mirror critical entry points in `README.md`, and document any automation adjustments in this file so future contributors know how docs are built and shipped.

## Known Issues & TODOs
Recent code review identified: incorrect repository URL in `Cargo.toml` (`branchbox-branchbox`), placeholder author metadata, generic `anyhow::Error` usage (migrate to `thiserror` domain errors), missing CLI input validation, registry race conditions (check + create isn't atomic), hardcoded config (Docker networks, port ranges, spec templates), and insufficient unit test coverage for registry operations and module implementations.
