# Changelog

All notable changes to BranchBox will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `branchbox init` now automatically configures devcontainer.json and compose.yaml for git worktree compatibility, ensuring `workspaceFolder` uses dynamic `${localWorkspaceFolderBasename}` and compose mounts use `../..:/workspaces:cached`.
- `branchbox init` generates `docs/BRANCHBOX.md` quickstart guide for new projects.
- Init next steps now suggest committing the BranchBox configuration with a ready-to-use git command.
- `branchbox init` automatically adds cloudflared tunnel service to compose file when tunnels are enabled, with `.cloudflared.env` template for configuration.
- `branchbox init` can now provision the tunnel for `main` immediately when API credentials are provided, populating `.cloudflared.env` with the actual `TUNNEL_TOKEN`.
- Compose file name detection now reads `dockerComposeFile` from devcontainer.json and falls back to common names (`compose.yaml`, `compose.yml`, `docker-compose.yaml`, `docker-compose.yml`).

### Changed
- Parent structure (`use_parent_structure`) is now the default for `branchbox init`, creating worktrees as siblings (project/main/, project/feature-x/). Use `--no-parent-structure` to opt out.

### Fixed
- Fixed duplicate volume mount entries in compose.yaml when transforming `..:/workspaces:cached` to `../..:/workspaces:cached`.

## [0.4.1] - 2025-12-15

### Fixed
- `branchbox init --update` now always repairs `.gitignore` entries for `.branchbox/` and devcontainer env/tunnel files.
- `branchbox feature teardown` now force-removes worktrees when you accept the interactive `--force` prompt.
- Cloudflare API errors no longer fail JSON decode when error payloads omit `result`; BranchBox surfaces the real Cloudflare message instead.

### Added
- `branchbox tunnel open` and `branchbox tunnel remove` for provisioning/removing tunnels on existing features.
- `.branchbox/config.json` `feature.*` defaults for branch prefix and teardown branch-delete policy.

## [0.4.0] - 2025-11-15

### Added
- Introduced the BranchBox agent daemon (`branchbox-agent`) with its own crate, control-plane HTTP drain, durable ack tracking, and a CLI bridge (`branchbox agent status`) so long-running workflows can keep syncing even when the CLI exits.
- Added a gRPC surface consumed by both the CLI and a redesigned SwiftUI macOS preview app; the app now shows adapter metadata, control-plane diagnostics, tunnel health, and one-click feature actions from the home dashboard and menu bar.
- Demo/devcontainer tooling now forwards agent/control plane ports inside the devcontainer, includes a teaser harness for quick recordings, and keeps macOS packaging reproducible even when the Rust toolchain is unavailable.

### Changed
- Refreshed README, architecture docs, and milestone plans to highlight the agent milestone, macOS app loop, and end-to-end telemetry expectations before tagging releases.
- The macOS experience received a full visual overhaul (shell, active cards, error states, background sync indicators) so testers can validate the agent/control-plane loop without diving into logs.

### Fixed
- `branchbox feature start` no longer rewrites devcontainer configs when nothing changed and the demo harness copies fallback assets when `rsync` is missing.
- Agent + macOS IPC defaults now correctly gate Unix-only mechanisms on Windows, trim helper output, and keep CLI fallbacks optional so the UI keeps running even when the CLI binary is absent.
- Devcontainer + tunnel scripts gained better permission handling (rsync fallback, Cloudflared defaults) and we fixed multiple regressions surfaced by the teaser/demo harness runs.

### Documentation
- Added a macOS developer README plus packaging instructions, refreshed the release guide with milestone expectations, and documented the 60s teaser workflow for future recordings.
- Expanded manual CLI E2E docs with verbose/pretend modes across stacks, clarified the PATH refresh requirement after install, and noted default agent scope for Unix platforms.

### Testing
- Added SwiftUI view helper tests that cover devcontainer status fallback logic and extended CI to install the Swift toolchain/macOS targets so the preview app keeps building in pull requests.
- Demo harness scripts now run under CI with Dracula-themed VHS recordings to confirm CLI output remains stable.

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
