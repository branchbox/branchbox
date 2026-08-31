# Changelog

All notable changes to BranchBox will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add an Agentify-oriented `in-guest` runtime that reconciles a BranchBox worktree and explicit
  devcontainer facade inside an already-owned Firecracker guest, validates opaque lease file paths
  and digests, publishes loopback-only ports, exposes correlated container/runtime identity, and
  returns deterministic teardown residue evidence.
- Add `feature exec-provider` for the fixed Codex executable with name-only
  `OPENAI_API_KEY` inheritance into the configured devcontainer user/workspace.
- Deliver one digest-verified, canonical project-environment materialization only to the primary
  Compose service through a raw env file, without mounting or serializing its values.
- Add run-owned shared-directory and Unix tool-endpoint leases with exact read-only primary-container
  bind evidence and residue-checked directory/socket cleanup.
- Bind the Firecracker kernel config into the image manifest, require built-in virtio-vsock
  support, and prove a trusted guest-to-host transfer without exposing `/dev/vsock` to coding
  devcontainers.

### Security

- The in-guest facade runs before host-side repository lifecycle behavior, disables checkout hooks
  and filter drivers, strips ambient host env/mount authority plus outside/in/from-Docker feature
  aliases, replaces repository volume and port publication declarations with the exact canonical
  task-worktree/Git projection and primary-container-only signed loopback proxies, starts only the
  primary service and its validated dependency closure, omits repository tunnel sidecars, rejects dangerous
  Compose/build/extends/secondary-service authority and local or remote
  Docker/containerd/Podman/BuildKit endpoints, and verifies the resolved and running container have
  no supervisor socket, credential-directory mount, privileged host namespace/device/capability,
  disabled confinement, persisted daemon endpoint, or model key.

### Fixed

- In-guest failed starts now pre-record and recover Dev Containers/Compose ownership, remove
  dependency-only containers, networks, volumes, materializations, worktrees, and task branches,
  and bypass repository modules/adapters during no-registry cleanup.
- In-guest coding containers now receive a private, bounded 1 GiB shared-memory allocation while
  repository shared-memory overrides and host IPC remain disabled.
- Release changelog generation now authenticates git-cliff GitHub API requests with the scoped
  workflow token, avoiding anonymous API rate-limit failures during publication.

## [0.12.1] - 2026-08-20

### Fixed

- CLI diagnostics now use stderr so `--json` stdout remains valid machine-readable JSON even when
  `RUST_LOG` enables warnings or informational events.

## [0.12.0] - 2026-08-20

### Added

- SBX failed-start diagnostics can retain the sandbox and nested build cache with
  `--keep-runtime-on-failure`, retry it with `--reuse-runtime`, report retained/degraded/orphaned
  health in `feature list`, and remove retained runtimes through normal teardown/prune lifecycle.
- The account-free `local-vm` provider now runs repository devcontainers and Compose stacks inside
  fresh jailed Firecracker VMs on x86_64 Linux/KVM, including captured/interactive execution,
  collision-safe ports and TAP networks, explicit scoped credential injection, digest-addressed
  guest artifacts, workspace synchronization, restricted host/private-network access, and
  deterministic teardown.

### Fixed

- SBX execution now reconciles the devcontainer after sandbox resume, refreshes port proxies when
  the container identity changes, and runs commands through the devcontainer user's login shell so
  Mise/asdf/nvm-managed tools receive the same environment as an interactive terminal.
- Copy-mode `feature start --reuse` now detects feature-local devcontainer divergence against the
  last BranchBox sync baseline and requires an explicit fail/preserve/overwrite/inspect decision.
- SBX can exclude incompatible optional Compose services through `runtime.sbx.run_services`,
  preflights `/dev/net/tun` usage, and keeps required Compose dependencies intact.
- SBX feature startup now provisions a required Cloudflare tunnel environment before creating the
  sandbox or starting the devcontainer, and fails its credential preflight before an expensive
  sandbox build when that required environment cannot be populated.
- Worktree pointer repair now derives and validates the relative target from authoritative Git
  metadata instead of trusting an unrelated sibling directory named `main`.

### Security

- SBX devcontainer startup failures now discard rendered Compose configuration, structurally redact environment assignments and expanded values, and return only a bounded actionable error tail with the exit status. This prevents arbitrary env-file credentials from reaching terminal, JSON, agent, CI, or telemetry logs.

- Upgrade the agent gRPC stack to remove the `h2` version affected by
  RUSTSEC-2026-0258.

## [0.11.1] - 2026-08-12

### Fixed
- `branchbox feature teardown` now discovers the Compose project created by the devcontainer CLI from the worktree's exact Docker label, restores BranchBox's persisted Compose identity across CLI processes, removes containers, networks, and volumes, and reports an error if owned resources remain.

## [0.11.0] - 2026-08-12

### Added
- Pluggable workspace runtime providers with the existing account-free container workflow as the default and experimental Docker SBX support for microVM-isolated workspaces.
- `branchbox feature start --runtime <container|sbx|local-vm>` runtime selection through the CLI and `.branchbox/config.json` defaults.
- `branchbox feature exec` for captured or interactive command execution through the runtime recorded for an active feature.
- Runtime provider, runtime identity, and resolved host-port mappings in feature registry records, CLI summaries, JSON output, and agent IPC payloads.

### Changed
- Docker SBX workspaces now start the repository devcontainer inside the sandbox, execute coding agents in that devcontainer, bridge nested Compose services to collision-safe host ports, recover mappings after sandbox restart, and remove provider-owned state during teardown.
- Worktree `.git` pointers are restored to portable relative paths after repository lifecycle hooks run inside a devcontainer.

### Fixed
- Upgraded the shared HTTP client to reqwest 0.12 and patched Rustls dependencies, resolving the `rustls-webpki` certificate-validation and CRL vulnerabilities reported by the repository security audit.
- Fixed the agent E2E harness on macOS Bash 3 when `--cp-stub` is used without additional CLI arguments.

### Documentation
- Documented the distinction between devcontainers and outer isolation runtimes, experimental SBX requirements, runtime configuration and command execution, and the account-free local VM direction using Colima/Lima.

### Testing
- Added a fake-SBX full CLI lifecycle test covering provisioning, port publication, devcontainer startup, agent execution, and teardown.
- Verified a real Agentify Compose/devcontainer stack inside Docker SBX, including Codex execution, Rails/PostgreSQL access, host-port bridging, restart/reuse, and cleanup.
- Kept the disposable multi-feature CLI and agent E2E harness portable across macOS Bash and stack-specific Compose service names.

## [0.10.1] - 2026-03-19

### Fixed
- `branchbox feature teardown` and `branchbox feature prune` now accept `--allow-container` (alias `--no-host-check`), matching `feature start`. Previously, features started inside a container could not be torn down from the same environment.

## [0.10.0] - 2026-03-16

### Added
- `branchbox feature start --allow-container` (alias `--no-host-check`) allows running feature start from inside a containerized environment (e.g., devcontainers with Docker socket mounted), enabling programmatic workspace provisioning for coding agent orchestration (#65).

## [0.9.3] - 2026-03-05

### Added
- `branchbox prune` and `branchbox feature prune` to tear down all active feature worktrees in one command, with `--dry-run` and `--yes` support for safe automation.
- Added a `features` alias for the `feature` command group so `branchbox features list` works.

### Changed
- Installer now creates a `bb` alias symlink to `branchbox` and warns when another `bb` command already exists in `PATH`.
- Release packaging now ships `bb`/`bb.exe` alongside `branchbox`, and Homebrew formula automation updates install lines to include both binaries.

## [0.9.0] - 2026-02-27

### Added
- `branchbox init` now interactively prompts for 1Password credential references (`OP_GITHUB_REF`, `OP_SIGNING_KEY_REF`) and persists them to `.devcontainer/.env`, eliminating the need to manually export environment variables.
- References are validated live via `op read` during setup; users can re-run `branchbox init --update` to reconfigure.
- `init-host.sh` now sources persisted references from `.devcontainer/.env` with automatic fallback to the main worktree's copy for feature worktrees.
- BranchBox's own devcontainer now includes the 1Password bootstrap flow (`initializeCommand`, `postStartCommand`, secret mounts) matching the stack templates.
- `GITHUB_TOKEN` is now injected via compose `env_file` so it is available to all container processes, not just interactive login shells.

### Fixed
- Pre-built devcontainer images for Rails and Node.js no longer have a broken `PATH` caused by `${containerEnv:PATH}` being baked literally into the image instead of resolved at runtime (#58).
- `write_op_env` now preserves existing non-BranchBox keys in `.devcontainer/.env` instead of truncating the file.
- Worktree fallback path for 1Password references now correctly resolves the main worktree name from `BRANCHBOX_MAIN_NAME` in `.branchbox.env`.
- Windows compilation fixed: Unix-specific file permission code gated behind `#[cfg(unix)]`.

## [0.8.0] - 2026-02-17

### Added
- `branchbox init` now scaffolds 1Password bootstrap assets in `.devcontainer/` (`scripts/init-host.sh`, `scripts/setup-git.sh`, `.github-token.env`, `.git-signing-key`, `.gitconfig.env`) and wires each stack template to run them during devcontainer startup.
- Compose templates for Rust, Generic, Rails, and Node now mount the generated 1Password credential files into the container.
- Added a focused manual 1Password regression harness (`scripts/manual-1password-e2e.sh`) plus runbook (`scripts/manual-1password-e2e.md`) to validate PAT + SSH-signing setup end-to-end.
- Added `scripts/review-preflight.sh` plus CI wiring to enforce security/sanitizer/harness-doc-sync guardrails before deeper test jobs run.

### Changed
- Devcontainer compose templates no longer pin top-level compose project names or `container_name`, preventing collisions across parallel worktrees.

### Fixed
- `branchbox feature start` now consistently derives `COMPOSE_PROJECT_NAME` / `DEVCONTAINER_NAME` from app slug + feature name (including when the source repo has no `.env`), while still writing `.devcontainer/.branchbox.env`.
- Feature-start stash handling now ignores untracked files and applies the exact stash reference, eliminating false “failed to apply stashed changes” warnings in common workflows.
- 1Password host bootstrap now preserves previously fetched token/signing files when `op read` fails and writes signing keys with owner-only permissions.
- 1Password host bootstrap now surfaces the final `op read` error output after retries so secret-fetch failures are diagnosable in `initializeCommand` logs.
- Devcontainer git credential bootstrapping now stores GitHub credentials via `git credential approve` + `store --file` (no shell helper interpolation of token content).
- Feature/bootstrap file generation now rejects symlink targets for managed writes and uses `O_NOFOLLOW`/file-handle permission hardening on Unix to prevent unintended host file overwrite via malicious repository links.
- Feature env generation now applies context-specific sanitization before writing `.env` files (`APP_URL` keeps URL-safe delimiters and is single-quoted when emitted, `GIT_BRANCH` is allow-listed to env-safe branch characters, and `COMPOSE_PROJECT_NAME` is normalized to Docker Compose-safe lowercase chars), and generated feature env files are written with owner-only permissions.
- VS Code feature URL tasks now use process-style launchers across platforms (`xdg-open`/`open`/`explorer`) instead of `cmd /C start` shell invocation.
- Compose lifecycle operations now fall back from `docker compose` to `docker-compose` when plugin-style compose is unavailable.
- `scripts/manual-cli-e2e.sh` now resolves devcontainer services via `devcontainer read-configuration` first (with JSONC/compose fallbacks), avoiding brittle JSONC parsing.
- `scripts/manual-1password-e2e.sh` now supports `docker compose`/`docker-compose` fallback and resolves devcontainer services via `devcontainer read-configuration` first (with JSONC/compose fallbacks).

### Documentation
- Updated manual E2E docs and release guidance to include the 1Password-specific harness and required environment inputs for issue #45 style validation.
## [0.7.0] - 2026-01-14

### Added
- New devcontainer CLI commands (`branchbox devcontainer up`, `exec`, `down`, `build`) for direct container management without entering the devcontainer environment.
- `.ai-agents/` directory structure for consolidated AI agent configurations (Claude Code, GitHub CLI, Codex) with automatic initialization during bootstrap.
- Release skill for guided version releases with automated quality checks and documentation updates.

### Changed
- AI agent configuration directories (`.claude/`, `.gh/`, `.codex/`) are now organized under `.ai-agents/` for cleaner workspace structure.
- Devcontainer builds for arm64 now only run on pushes to main branch to optimize CI performance.

### Fixed
- Handle empty `SHARED_CONFIG_DIR` environment variable gracefully in devcontainer configurations.
- Ensure `.ai-agents/` directory structure is created before Docker mount operations during bootstrap and devcontainer setup.

### Testing
- Added verification for `.claude.json` mount in feature worktree tests.
- Added comprehensive mount tests for root user scenarios in devcontainer module.

## [0.6.0] - 2026-01-13

### Added
- Official pre-built devcontainer images for all stacks (Rust, Rails, Node.js, Generic) published to GHCR at `ghcr.io/branchbox/branchbox/devcontainer-<stack>:latest`.
- `branchbox init` now generates compose.yaml files that use pre-built images by default with automatic fallback to local Dockerfile builds.
- New environment variables for devcontainer image control: `DEVCONTAINER_IMAGE` (custom image override) and `DEVCONTAINER_PULL_POLICY` (missing/always/build).
- GitHub Actions workflow (`devcontainer-build.yml`) that automatically builds and publishes all stack images when `.devcontainer/` or template Dockerfiles change on `main`.

### Changed
- Rails and Node.js devcontainer templates now use `mcr.microsoft.com/devcontainers/base:debian` with mise for runtime version management, reading `.ruby-version`, `.nvmrc`, `.node-version`, and `.tool-versions` files.
- All stack compose.yaml templates now include `init: true` and `ipc: host` for better container behavior.

### Upgrade Guide for Existing Projects

**New projects** created with `branchbox init` automatically use pre-built images.

**Existing projects** initialized before this release need manual updates to benefit from pre-built images:

1. **Update your compose.yaml** to reference the pre-built image:

   ```yaml
   services:
     your-service:
       image: ${DEVCONTAINER_IMAGE:-ghcr.io/branchbox/branchbox/devcontainer-<stack>:latest}
       build:
         context: ..
         dockerfile: .devcontainer/Dockerfile
       pull_policy: ${DEVCONTAINER_PULL_POLICY:-missing}
   ```

   Replace `<stack>` with your stack: `rust`, `rails`, `nodejs`, or `generic`.

2. **Optionally add to your `.env`** for customization:

   ```bash
   # Override image (optional)
   # DEVCONTAINER_IMAGE=my-custom-image:tag

   # Control pull behavior: missing (default), always, build
   # DEVCONTAINER_PULL_POLICY=missing
   ```

3. **Rails/Node.js users**: The new templates use mise for runtime version management. If you want to adopt the new approach, re-run `branchbox init` to regenerate your `.devcontainer/` files, or manually update your Dockerfile to use the base image with mise:

   ```dockerfile
   FROM mcr.microsoft.com/devcontainers/base:debian
   # mise will be installed and read .ruby-version, .nvmrc, etc.
   ```

**Feature worktrees** will automatically use the updated configuration from your main worktree when you run `branchbox feature start`.
## [0.5.0] - 2026-01-07

### Added
- `branchbox init` now automatically configures devcontainer.json and compose.yaml for git worktree compatibility, ensuring `workspaceFolder` uses dynamic `${localWorkspaceFolderBasename}` and compose mounts use `../..:/workspaces:cached`.
- Compose templates now mount `.claude.json` file for Claude Code authentication alongside the existing `.claude/` directory mount, ensuring proper authentication across worktrees.
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
