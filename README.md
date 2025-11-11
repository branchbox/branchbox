# BranchBox

[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
[![CI](https://github.com/branchbox/branchbox/workflows/CI/badge.svg)](https://github.com/branchbox/branchbox/actions)
[![License](https://img.shields.io/github/license/branchbox/branchbox)](LICENSE)

Stop context switching. Run multiple features in parallel—safely.

Isolated git worktrees with per‑feature devcontainers, databases, Docker networks, and configuration. Perfect for solo engineers and agent‑assisted workflows—you can “yolo” big refactors without touching your main workspace.

▶ Watch 60s teaser: https://example.com/branchbox-teaser  
<!-- Replace the link above with your video URL. Optional: add a thumbnail image here. -->

## Quick Start

```bash
# Install (Linux/macOS)
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash

# Initialize project (creates registry, checks environment)
branchbox init

# Start a fully isolated feature workspace
branchbox feature start "Add OAuth Integration"

# Open and work in the new worktree
cd ../oauth-integration/
# Your feature has its own DB, Docker network, and ports
```

Prefer a disposable sample? Use the bundled harness: `./scripts/setup-sample-workspaces.sh` then run `branchbox init` → `branchbox feature start`.

## Aha Moments

- Multiple features running simultaneously with zero collisions (DB, network, ports).
- One command spins up a complete, stack‑aware feature workspace (Rails, Node, generic).
- Edit `.devcontainer/` once; replay changes everywhere with `branchbox devcontainer sync`.
- Minimal mode for quick spikes or agent “yolo” experiments; add full provisioning later.
- Safety nets: dirty devcontainer/compose guard on teardown; JSON registry for automations.
- Agent‑friendly: shared credentials mount across containers; copy or symlink sync strategy.

## Core Commands

- `branchbox feature start "<title>"` — Create isolated worktree + provision modules
- `branchbox feature list [--json]` — Show feature registry (machine‑readable when needed)
- `branchbox feature teardown <name> [--complete-spec] [--keep-branch]` — Clean up safely
- `branchbox devcontainer sync [--strategy copy|symlink] [--dry-run]` — Replay config across features
- `branchbox detect` — Print detected adapter/modules for the current repo
- `branchbox name generate|validate` — Naming helpers for features

Minimal mode (fast spikes and agent hand‑off):

```bash
branchbox feature new backlog-quick-fix \
  --minimal \
  --default-prompt \
  --json
```

## Devcontainers, Simplified

- New features copy `.devcontainer/` automatically; open in VS Code/Cursor and accept “Reopen in Container”.
- Update all features after editing `.devcontainer/` in the main repo:

```bash
branchbox devcontainer sync
# Optional: --strategy copy|symlink, --dry-run
```

- Shared tool credentials (`.gh`, `.claude/`, `.codex/`) mount from `SHARED_CONFIG_DIR` (default `../..`). Authenticate once; every feature reuses it.

## Examples

Rails:
```bash
branchbox feature start "Add User Dashboard"
# ✓ Rails detected (Gemfile, config/application.rb)
# ✓ DB: user-dashboard_development · Next: rails db:create db:migrate
```

Node.js:
```bash
branchbox feature start "Add GraphQL API"
# ✓ Node.js detected (package.json) · Next: npm install
```

Generic:
```bash
branchbox feature start "Docs Refresh"
# ✓ Generic adapter · Basic isolation applied
```

## Devcontainer Workflow

BranchBox ships a full VS Code/Cursor devcontainer setup and propagates it to every feature worktree. It focuses on Rust + CLI workflows; macOS SwiftUI builds still happen on a Mac host because the devcontainer intentionally omits Apple-only SDKs.

### Reopen Features in Containers
- `branchbox feature start "<name>"` copies `.devcontainer/` into the new worktree.
- `cd ../<worktree>/` then run `code .` (or open in Cursor) and accept the “Reopen in Container” prompt.
- Each container inherits feature-specific `.env` values (`APP_URL`, `COMPOSE_PROJECT_NAME`, etc.) and isolated Docker resources.

### Shared Tool Credentials
- Shared configs (`.codex/`, `.claude/`, `.gh/`) live outside the worktree and mount into every container via `SHARED_CONFIG_DIR` (default `../..`).
- Authenticate once inside the main worktree (`gh auth login`, `claude login`); subsequent feature containers reuse the same credentials.
- Adjust the sync behaviour with `BRANCHBOX_DEVCONTAINER_STRATEGY=copy|symlink branchbox feature start`; persist your preference by adding the env var to `.env`.

### Keeping Devcontainers in Sync
- When the main `.devcontainer/` changes, update all active worktrees:  
  ```bash
  branchbox devcontainer sync
  # Optional flags:
  #   --dry-run    preview changes
  #   --strategy   copy|symlink (overrides env var for this run)
  ```
- The sync command copies new files, refreshes existing ones, and prunes stale artifacts.
- Need a safe playground? Run `./scripts/setup-sample-workspaces.sh` to spin up sample repositories under `test/workspaces/local/`; the script reads each template’s metadata and prints the exact `branchbox init --stack <stack>` command before you proceed with `feature start`, `devcontainer sync`, and `feature teardown`.

### Troubleshooting
- **No “Reopen in Container” prompt**: confirm `.devcontainer/` exists in the feature (`ls .devcontainer/`); rerun `branchbox devcontainer sync` if it is missing files.
- **Tools ask to re-authenticate**: verify shared mounts inside the container (`mount | grep -E '(codex|claude|gh)'`) and ensure `SHARED_CONFIG_DIR` points to the directory holding your credentials.

## Agent + Control Plane Stub

Milestone 2 begins the control-plane integration by draining all agent events (feature starts, teardowns, and heartbeats) to a configurable HTTP endpoint.

- Configure the target endpoint via `BRANCHBOX_CP_ENDPOINT`, provide a bearer token in `BRANCHBOX_CP_TOKEN`, and set `BRANCHBOX_CP_VERIFY_TLS=0` when hitting staging systems without valid certificates. The agent automatically adds host metadata (hostname, OS, architecture, agent version) to every batch so the Rails control plane can tag devices without probing the socket directly.
- Events continue to land in the on-disk SQLite queue under `~/.branchbox/agent/agent.db`. When the HTTP drain is enabled the event loop flushes in batches (`event_batch_size`, default 50) and only logs to stdout if the control plane is disabled.
- Heartbeats snapshot every registered worktree and keep emitting even if the HTTP endpoint is unavailable; delivery retries back off in the queue so you can restart the control plane without losing history.
- Durable acknowledgements are stored per batch (`control_plane_status` table) so the agent can resume from the last acked event after restarts. Failed deliveries use exponential backoff + jitter before retrying so unhealthy endpoints don’t get hammered.
- `scripts/manual-agent-e2e.sh` now accepts the same environment variables so you can point the smoke test at a local Rails stub (or `webhook.site`) before promoting the change to CI. Pass `--cp-stub` to spin up the embedded Python webhook and print the persisted `last_ack_event_id` cursor when the run finishes.
- Inspect control-plane connectivity any time with `branchbox agent status` (add `--json` for scripts). The command reports whether the drain is configured/connected plus the last delivery/failure timestamps so you can debug mismatched tokens or endpoints quickly.

## macOS App Preview

The new `macos/` Swift package hosts a SwiftUI app that talks to the agent over gRPC and falls back to the CLI when the daemon is offline. It is intentionally minimal so we can validate the IPC surface before layering on UI polish.

```bash
cd macos
# Configure the workspace and optional gRPC address
defaults write dev.branchbox.app workspace "$(pwd)/.."
export BRANCHBOX_AGENT_GRPC_ADDR=127.0.0.1:50515

# Run the preview app (opens a SwiftUI window on macOS)
swift run BranchBoxApp
```

- **Requires macOS**: SwiftUI builds need Apple’s SDKs (`OSLog`, SwiftUI, etc.), so the Linux devcontainer cannot compile or test the app. Run UI/E2E checks from a macOS host (or CI runner) that has the Xcode command line tools installed.

- The app lists every feature via `FeatureService/List`, exposes start/teardown actions, and displays whether the data came from gRPC or the CLI fallback.
- When the agent socket is missing the view transparently shells out to `branchbox feature list --json` so testing can continue on machines that only have the CLI installed.
- Workspace picker + transport badge live in the toolbar (choose a repo via `NSOpenPanel`, see whether the data came from gRPC or the CLI fallback). You can also force a transport (Automatic/gRPC/CLI) when debugging agent connectivity. Start form now includes branch prefix, reuse toggle, module skip list, and prompt history chips so the mac app stays in feature parity with the CLI.
- The Home dashboard shows workspace health, tunnel status (with copyable hostname), and active feature quick actions so new users can start/teardown without digging into diagnostics tabs.
- Rows now surface adapter metadata (name, service URL, warnings) plus module/tunnel health so you can spot misconfigured stacks without dropping to the CLI.
- Teardown actions open a confirmation sheet with `--force` + `--complete-spec` toggles so you don’t accidentally drop worktrees.
- Configuration happens through environment variables (`BRANCHBOX_AGENT_GRPC_ADDR`, `BRANCHBOX_WORKSPACE`, `BRANCHBOX_CLI_PATH`) or the stored `defaults` domain `dev.branchbox.app`. See `macos/Sources/BranchBoxApp/Agent/AgentBridge.swift` for the full precedence order.
- Manual validation steps live in [docs/docs/getting-started/manual-cli-e2e.md](docs/docs/getting-started/manual-cli-e2e.md) and cover: launching the agent, running the SwiftUI preview, starting a feature, tearing it down, and confirming the control-plane stub receives batched events.
- **Custom devcontainer behaviour**: override per run with `BRANCHBOX_DEVCONTAINER_STRATEGY` or update `.env` if you always prefer symlinks.

## Installation

**Quick install (Linux/macOS):**
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
branchbox --help
```

Other methods: Homebrew (coming soon), Scoop (coming soon), prebuilt binaries via Releases, or `cargo install --path cli --locked`. See docs/INSTALLATION.md for details.

Note: After installing, open a new terminal session (or run `hash -r`) so your shell picks up the updated PATH.

## Safety & Agent‑Ready

- Dirty guard on teardown refuses to delete changed devcontainer/compose files unless `--force` is confirmed.
- Copy strategy by default (avoids macOS prompts); opt‑in to symlink for always‑up‑to‑date worktrees.
- Prompt seeds (up to 2,000 chars) stored in the registry; `--default-prompt` available in minimal mode.
- Shared mounts are never deleted; BranchBox preserves host‑side credentials.

## What’s Built

- Milestone 0: Core worktree orchestration (start, teardown, list), stack detection, module system (compose, database, specs), env provisioning, JSON registry.
- Milestone 1 (in progress): Agent daemon for background workflows; CLI bridge; telemetry.

Roadmap highlights: Windows agent transport, native macOS app, Rails control plane, Tailscale mesh. See docs/ARCHITECTURE.md.

## Troubleshooting

- No “Reopen in Container”? Ensure `.devcontainer/` exists in the feature; run `branchbox devcontainer sync`.
- Tools ask to re‑auth? Verify mounts inside the container (`mount | grep -E '(codex|claude|gh)'`) and `SHARED_CONFIG_DIR`.
- Prefer symlinks? Set `BRANCHBOX_DEVCONTAINER_STRATEGY=symlink` (or persist in `.env`).

## Contributing

- Devcontainer ships a ready toolchain. Run `cargo fmt && cargo clippy && cargo test` before PRs.
- Use conventional commits (e.g., `feat(modules): …`). See CONTRIBUTING.md and AGENTS.md.

## License

MIT — see LICENSE.
