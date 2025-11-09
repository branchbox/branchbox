# Changelog

All notable changes to BranchBox will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

-

## [0.3.0] - 2025-11-09

### Added
- Default agent hand-off: `branchbox feature start` now surfaces and (optionally) auto-launches the command defined in `BRANCHBOX_DEFAULT_AGENT_CMD`, propagates the label via `BRANCHBOX_DEFAULT_AGENT_NAME`, and reports readiness in both the checklist and JSON summary so automation can react immediately.
- `.branchbox/config.json` picks up an `editor` block that tracks preferred agent slugs, sidebar focus, and terminal auto-launch hints, letting future agent daemons stamp consistent workspace preferences.
- Specs automation now discovers backlog entries in `docs/features/backlog/`, promotes them into feature worktrees, generates stubs (with frontmatter) when a spec is missing, and honors `FEATURES_DIR` overrides across start/teardown.

### Changed
- `branchbox feature start` and `feature list` ship richer summaries: new checklist rows for prompt seeds, module health, and default agents plus list output that shows start mode, prompt status, and module outcome counts at a glance.
- The manual CLI harness (`scripts/manual-cli-e2e.sh`) now drives every mode (regular/verbose/pretend) across Rust, generic, Rails, and Node stacks so releases validate adapters and tunnel permutations consistently.

### Fixed
- Feature teardown refuses to delete worktrees when devcontainer/module-managed files are dirty unless `--force` or `BRANCHBOX_FORCE_REMOVE_MODULES=1`, preventing accidental loss of template edits.
- Registry reconciliation no longer trips over git porcelain parsing, and backlog specs stay in sync when moving between in-progress and completed folders.

### Documentation
- Expanded devcontainer docs with telemetry, Cloudflared wiring, and troubleshooting guidance plus refreshed README sections covering default agent auto-launch.
- Added a detailed manual CLI E2E guide (modes + stack matrix) and release runbook updates so maintainers know exactly which commands to run before tagging.

### Testing
- Introduced targeted CLI unit tests for prompt/default-agent summaries, dirty teardown guards, and specs promotion logic.
- Beefed up the manual CLI harness to assert Cloudflared/manual tunnel flows, `branchbox devcontainer sync` dry-runs, registry JSON output, and dirty teardown retries.

## [0.2.2] - 2025-11-08

### Added
- Seed `.devcontainer/.branchbox.env` from bootstrap so new repositories ship a template for per-worktree overrides and agent-driven env injection *(bootstrap)*

### Changed
- `branchbox feature start` now copies and customizes `.branchbox.env` for every worktree, keeping secrets isolated and ready for devcontainer sync *(workflows)*

### Bug Fixes
- Align devcontainer workspace/git mounts across strategies and skip syncing env overlays to avoid clobbering per-worktree state *(modules/devcontainer)*
- Ensure release automation keeps Cloudflared tunnel jobs enabled by default *(workflows)*

### Documentation
- Rebuilt the documentation site on Docusaurus with refreshed architecture, getting started, and reference sections *(docs)*

### Testing
- Added regression coverage for Docker/devcontainer sync paths, including per-workflow env propagation *(tests)*

## [0.2.1] - 2025-11-03

### Bug Fixes
- Accept JSONC devcontainer configs so comment-preserving scaffolds sync cleanly *(modules)*

### Miscellaneous
- Streamline Homebrew formula updates to keep tap automation stable *(release)*

## [0.2.0] - 2025-11-03

### Added
- Introduced the devcontainer module with sync tracking and a `branchbox devcontainer sync` command supporting dry-run and strategy overrides to keep feature worktrees aligned.
- Implemented Cloudflared tunnel automation, including provisioning, DNS updates, credential management, and manual fallbacks driven by `.branchbox/config.json`.
- Expanded feature lifecycle workflows with richer registry metadata, improved list output (status filters, JSON), and env linking for devcontainer compatibility.

### CI/CD
- Reworked primary and legacy pipelines, promoted `llvm-cov`, and added a documentation deploy workflow to publish the mdBook site.

### Documentation
- Published an mdBook-powered documentation site with refreshed theming, CLI reference generation, devcontainer rollout guidance, and Cloudflared integration specs.

### Testing
- Added devcontainer and Cloudflared smoke fixtures, sample workspaces, and supporting scripts to exercise new automation paths under coverage.

## [0.1.0-alpha.1] - 2025-10-27

### Features
- Add automated release workflow with cross-platform builds
- Add cargo-release configuration for version management
- Add git-cliff for automated changelog generation

### CI/CD
- Create release workflow for GitHub Actions
- Support Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64) builds
- Implement binary packaging with tar.gz (Unix) and zip (Windows)
- Generate SHA256 checksums for all release artifacts
- Support pre-release tags (beta, alpha, rc)

### Documentation
- Add RELEASING.md with comprehensive maintainer release guide
- Add installation instructions to README with badges
- Create feature specs for Homebrew tap and install scripts

### Fixed
- Correct repository URL in Cargo.toml
- Update author metadata from placeholder

## [0.1.0] - 2025-10-27

### Features
- Implement core workflow orchestration for feature worktrees in Rust
- Add `branchbox feature start/teardown/list` commands with full lifecycle management
- Migrate from bash scripts to Rust-based implementation

<!-- generated by git-cliff -->
