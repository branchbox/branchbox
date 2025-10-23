# Repository Guidelines

## Project Structure & Module Organization
The workspace roots at `Cargo.toml` and currently ships two members: the core library in `core/` and the CLI in `cli/`. Core modules live under `core/src/` (notably `adapters/`, `modules/`, `bootstrap/`, and cross-cutting helpers like `git.rs`). The CLI entry point is `cli/src/main.rs`, exporting the `branchbox` binary on behalf of the library. Shared documentation sits in `docs/`, CI workflows in `.github/workflows/`, and reproducible tooling in `.devcontainer/`.

## Build, Test, and Development Commands
Run workspace builds with `cargo build` and optimize releases via `cargo build --release`. Execute the CLI locally with `cargo run -p branchbox-cli -- --help` to validate argument wiring. Use `cargo fmt --all -- --check` to enforce formatting, `cargo clippy --all-targets --all-features -- -D warnings` for linting, and `cargo check` for quick iteration. Security and dependency scanning is covered by `cargo audit`.

## Coding Style & Naming Conventions
Rust files follow `rustfmt` defaults (4-space indentation, 100-column soft limit). Modules and files use `snake_case`; types are `UpperCamelCase`; constants are `SCREAMING_SNAKE_CASE`. Prefer explicit `Result<T, Error>` aliases plus the `?` operator for flow control, and leverage `thiserror` for rich domain errors. Branch names should stay action-oriented, e.g., `feature/bootstrap-cleanup` or `fix/git-lock-race`.

## Testing Guidelines
Unit tests live beside their modules under `#[cfg(test)]`; grow integration coverage in a `core/tests/` harness when cross-cutting behaviour warrants it. Run `cargo test --all-features` for the default gate, `cargo test --doc` to validate examples, and `cargo test -- --nocapture` when debugging. CI enforces 90% line coverage via Tarpaulin, so periodically run `cargo tarpaulin --out Html --all-features --workspace` to catch regressions.

## Commit & Pull Request Guidelines
Recent history favors concise, imperative summaries (e.g., `Refactor CLI to 'branchbox' with grouped subcommands`). Continue that tone while adopting the Conventional Commit prefix expected in `CONTRIBUTING.md`, such as `feat(modules): add docker compose planner`. Before opening a PR, rebase on `main`, rerun fmt/clippy/tests/doc checks, and attach context: problem statement, scope, linked issues, and any relevant CLI transcripts or screenshots. Ensure the CI suite is green before requesting review.

## Environment & Configuration Tips
Use the provided devcontainer (`.devcontainer/`) for a consistent toolchain; it preinstalls Rust, Clippy, Tarpaulin, and Docker. Local setups should copy `.env.sample` into a private `.env` and avoid committing secrets. When adding new adapters or modules, document their env requirements in `docs/` so downstream agents stay aligned.
