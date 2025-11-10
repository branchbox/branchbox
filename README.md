# BranchBox

[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
[![CI](https://github.com/branchbox/branchbox/workflows/CI/badge.svg)](https://github.com/branchbox/branchbox/actions)
[![License](https://img.shields.io/github/license/branchbox/branchbox)](LICENSE)

**Stop context-switching between features. Start working in parallel.**

BranchBox manages isolated git worktrees with complete development environments—separate databases, Docker networks, and configurations—so you can work on multiple features simultaneously without conflicts or cleanup overhead.

## Quick Start

```bash
# Install (Linux/macOS)
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash

# Start a new feature with complete isolation
branchbox feature start "Add OAuth Integration"

# Work in the new worktree
cd ../oauth-integration/
# Your feature has its own database, Docker network, and configuration
```

See [docs/INSTALLATION.md](docs/INSTALLATION.md) for all installation methods.

## Documentation

- **Docs site:** https://branchbox.github.io/branchbox/ (built with Docusaurus, deployed from `main`)
- **Source:** `docs/` — run `cd docs && npm install && npm run build` to preview locally
- **CLI reference:** update `docs/docs/reference/cli.md` whenever command flags change (capture `branchbox --help` output)

## Why BranchBox?

Traditional git workflows force you to:
- Stash changes when switching branches
- Rebuild databases for each feature
- Restart Docker containers to avoid port conflicts
- Wait for CI to catch environment issues

**BranchBox gives you:**
- ✅ **Multiple features running simultaneously** with complete isolation
- ✅ **Zero context switching** - each feature is a separate directory
- ✅ **Automatic environment provisioning** - database, Docker, and configuration
- ✅ **Stack-aware setup** - Rails, Node.js, or generic projects

## Usage Examples

### Starting a New Feature

```bash
# Create an isolated worktree for your feature
branchbox feature start "Add OAuth Integration"

# Output:
# ✓ Created branch: feature/oauth-integration
# ✓ Created worktree: /Users/you/projects/your-app-oauth-integration/
# ✓ Copied .env with APP_URL=http://localhost:3000
# ✓ Set COMPOSE_PROJECT_NAME=oauth-integration
# ✓ Rails detected: Database setup instructions included
# ✓ Spec created: docs/features/in-progress/oauth-integration.md
#
# Next steps:
#   cd ../oauth-integration/
#   bundle install
#   rails db:create db:migrate
```

**What just happened:**
- Created git worktree at `../oauth-integration/`
- Created branch `feature/oauth-integration`
- Copied `.env` with `APP_URL` configured for this feature
- Set `COMPOSE_PROJECT_NAME` to isolate Docker containers
- Detected Rails stack and provided database setup instructions
- Created feature spec in `docs/features/in-progress/oauth-integration.md`

### Fast Path & Prompt Seeds

Need a lightweight worktree to spike an idea? Use the `feature new` alias plus minimal mode:

```bash
branchbox feature new backlog-quick-fix \
  --minimal \
  --default-prompt \
  --json
```

Highlights:
- Minimal mode skips the devcontainer, compose, and specs modules (you can still skip others via `--skip-module`). The CLI prints a reminder to run `branchbox devcontainer sync` when you want full provisioning.
- `--default-prompt` drops in a BranchBox-authored seed for your default coding agent so you can stay hands-free (`--prompt "<custom text>"` still works when you want to override it).
- Prompt seeds (up to 2,000 characters) are stored in the registry so the forthcoming agent bridge can resume context. The summary reports whether `BRANCHBOX_ENABLE_PROMPT_BRIDGE` is active.
- Set `BRANCHBOX_DEFAULT_AGENT_CMD="cursor --workspace ." BRANCHBOX_DEFAULT_AGENT_NAME=cursor` (or any command) to auto-launch your preferred agent once the devcontainer module finishes. The checklist row explains whether it will run immediately or wait for `branchbox devcontainer sync`.
- `--json` mirrors the richer on-screen summary (the checklist, module table, skipped modules, warnings, prompt metadata) and now includes a `default_agent` block so automation knows whether an agent will launch (`ready`, `waiting`, `blocked`, `disabled`). Add `--no-summary` for machine-only output.

### Working on Your Feature

```bash
cd ../oauth-integration/

# Your feature runs in complete isolation:
# - Separate database (oauth_integration_development)
# - Separate Docker network (oauth-integration_default)
# - Separate port allocation
# - Independent configuration

# Make changes
git add .
git commit -m "Add OAuth provider configuration"
git push -u origin feature/oauth-integration

# Meanwhile, your main worktree keeps running without conflicts!
```

### Checking Active Features

```bash
branchbox feature list

# Output (abbreviated):
# 📚 Feature registry — 3 active · 0 removed (showing 3/3)
# Feature             Status  Mode    Prompt            Modules           Branch                       URL                       Tunnel   Devcontainer  PR  Color    Updated
# oauth-integration   Active  full    —                 4 ok / 1 skip     feature/oauth-integration    https://dev-oauth.local   —        synced 2025-11-03  —  #7a6bff  2025-11-03 10:12
# backlog-quick-fix   Active  minimal seed (28 chars)   0 fail / 3 skip   feature/backlog-quick-fix    https://dev-backlog.local pending  outdated        —  #8d68ff  2025-11-05 14:20
# api-refactor        Active  full    seed (41 chars)   5 ok              feature/api-refactor         https://dev-api.local     active   synced 2025-11-04  #42c9f0  #ff6b6b  2025-11-04 09:55
```

Use `branchbox feature list --json` when you need machine-readable data; entries include `start_mode`, `prompt_seed`, each module outcome, and the last summary timestamp.

### Cleaning Up After Merge

```bash
# After merging your PR, tear down the feature worktree
branchbox feature teardown oauth-integration

# Output:
# ✓ Stopped Docker containers
# ✓ Removed worktree: /Users/you/projects/your-app-oauth-integration/
# ✓ Deleted branch: feature/oauth-integration
# ✓ Moved spec to: docs/features/completed/oauth-integration.md

# Or complete the spec without deleting the branch:
branchbox feature teardown oauth-integration --complete-spec --keep-branch
```

### Stack Detection Examples

BranchBox automatically detects your project type and adapts:

**Rails Project:**
```bash
branchbox feature start "Add User Dashboard"
# ✓ Rails detected (Gemfile, config/application.rb)
# ✓ Database name: user-dashboard_development
# ✓ Run: rails db:create db:migrate
```

**Node.js Project:**
```bash
branchbox feature start "Add GraphQL API"
# ✓ Node.js detected (package.json)
# ✓ Docker Compose project: graphql-api
# ✓ Run: npm install
```

**Generic Project:**
```bash
branchbox feature start "Documentation Update"
# ✓ Generic adapter (no specific stack detected)
# ✓ Basic worktree isolation applied
```

## Devcontainer Workflow

BranchBox ships a full VS Code/Cursor devcontainer setup and propagates it to every feature worktree.

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
- `scripts/manual-agent-e2e.sh` now accepts the same environment variables so you can point the smoke test at a local Rails stub (or `webhook.site`) before promoting the change to CI.

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

- The app lists every feature via `FeatureService/List`, exposes start/teardown actions, and displays whether the data came from gRPC or the CLI fallback.
- When the agent socket is missing the view transparently shells out to `branchbox feature list --json` so testing can continue on machines that only have the CLI installed.
- Configuration happens through environment variables (`BRANCHBOX_AGENT_GRPC_ADDR`, `BRANCHBOX_WORKSPACE`, `BRANCHBOX_CLI_PATH`) or the stored `defaults` domain `dev.branchbox.app`. See `macos/Sources/BranchBoxApp/Agent/AgentBridge.swift` for the full precedence order.
- Manual validation steps live in [docs/docs/getting-started/manual-cli-e2e.md](docs/docs/getting-started/manual-cli-e2e.md) and cover: launching the agent, running the SwiftUI preview, starting a feature, tearing it down, and confirming the control-plane stub receives batched events.
- **Custom devcontainer behaviour**: override per run with `BRANCHBOX_DEVCONTAINER_STRATEGY` or update `.env` if you always prefer symlinks.

## Installation

**Quick install (Linux/macOS):**
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
```

**Other methods:**
- **Homebrew** (macOS): `brew install branchbox/tap/branchbox` *(coming soon)*
- **Scoop** (Windows): `scoop install branchbox` *(coming soon)*
- **Pre-built binaries**: Download from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest)
- **From source**: `cargo install --path cli --locked`

**Verify installation:**
```bash
branchbox --version
branchbox --help
```

See [docs/INSTALLATION.md](docs/INSTALLATION.md) for detailed installation instructions, platform-specific guides, and troubleshooting.

## Agent Daemon (macOS/Linux)

Milestone 1 ships the BranchBox agent—a long-running daemon that owns feature orchestration, persistent state, and heartbeat telemetry. **It currently targets Unix-like hosts (macOS, Linux, devcontainers).** On Windows, keep using CLI direct mode (`BRANCHBOX_CLI_DIRECT=1`) until we land the backlog work described in [agent-windows-support](docs/features/backlog/agent-windows-support.md).

```bash
# Start the agent (defaults to ~/.branchbox/agent)
cargo run -p branchbox-agent --release

# CLI commands will now stream through the daemon
branchbox feature start "Add OAuth Integration"
```

- Customize the socket/state directory via `BRANCHBOX_AGENT_SOCKET` / `BRANCHBOX_AGENT_DIR`.
- To run the full manual harness against the live agent, use `./scripts/manual-agent-e2e.sh` (passes through any `--mode`/`STACK` flags from `scripts/manual-cli-e2e.sh` and preserves logs when `KEEP_AGENT_TMP=1`).
- Bypass the daemon when needed (CI, Windows) with `BRANCHBOX_CLI_DIRECT=1 branchbox feature …`.

## What Works Now

**✅ Milestone 0 Complete** - Core worktree orchestration:
- Full feature lifecycle (`start`, `teardown`, `list`)
- Stack detection (Rails, Node.js, Generic)
- Module system (Docker Compose, Database, Specs)
- Environment configuration (.env copying with `APP_URL` injection)
- State tracking (JSON registry at `.branchbox/registry.json`)

**✅ Milestone 1 (Agent) Highlights**
- `branchbox-agent` daemon exposes Unix-socket IPC + tonic gRPC for feature workflows.
- CLI bridges through the agent by default; manual harness + docs updated.
- Persistent registry + offline queue + heartbeat metrics ready for future control-plane wiring.

**🚧 Coming Soon:**
- Agent transport for Windows hosts (see backlog doc above)
- Native macOS app (Milestone 2)
- Multi-device coordination via Rust agent ↔ Rails control plane (Milestone 2/3)
- Tailscale mesh networking (Milestone 3)

See [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) for detailed progress.

## Architecture

```
Local Device ─┬─ Mac App (SwiftUI)              [Milestone 2]
              ├─ CLI Tool (Rust)                 [✓ Milestone 0]
              └─ Agent (Rust daemon) ──┬─ Core Library (Rust)  [✓ Milestone 0]
                                       │
                          Tailscale Network       [Milestone 3]
                                       │
Control Plane (Rails) ─────────────────┘         [Milestone 3]
  ├─ Web Dashboard
  ├─ API
  └─ PostgreSQL
```

**Current Architecture** (Milestone 0):
- **Core Library** (`core/`) - Rust library providing git worktree operations, stack adapters, and module system
- **CLI Tool** (`cli/`) - Command-line interface exposing feature lifecycle commands

**Future Architecture:**
- **Agent** - Long-running daemon for background operations and offline-first sync
- **Control Plane** - Rails app coordinating state across devices
- **Mac App** - Native SwiftUI interface for local and remote worktree management

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed design and [docs/PROTOCOL.md](docs/PROTOCOL.md) for communication protocols.

## Documentation

- **[Installation Guide](docs/INSTALLATION.md)** - All installation methods and troubleshooting
- **[Development Guide](docs/DEVELOPMENT.md)** - Building, testing, and contributing
- **[Architecture](docs/ARCHITECTURE.md)** - System design and components
- **[Protocol Spec](docs/PROTOCOL.md)** - gRPC and REST API documentation
- **[Implementation Status](docs/IMPLEMENTATION_STATUS.md)** - Feature completion tracking
- **[Homebrew Setup](docs/HOMEBREW_SETUP.md)** - Homebrew tap automation

## Contributing

We welcome contributions! To get started:

1. Check out the [Development Guide](docs/DEVELOPMENT.md)
2. Read [AGENTS.md](AGENTS.md) for repository guidelines
3. Fork the repository and create a feature branch
4. Make your changes with tests
5. Submit a pull request

**Development environment:**
- Use the included devcontainer for instant setup
- Run `cargo test --all-features` before submitting
- Follow conventional commit format: `feat(module): description`

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for detailed development workflow.

## Components

### Core Library (`core/`)
Rust library providing:
- Git worktree operations via `git2` crate
- Stack adapter system (Rails, Node.js, Generic)
- Module system (tunnel, database, compose, specs)
- Naming and validation utilities

### CLI Tool (`cli/`)
Command-line interface for:
- Feature lifecycle operations (`start`, `teardown`, `list`)
- Stack detection and configuration
- Local worktree management

### Future Components
- **Agent** (`agent/`) - Rust daemon for background operations [Milestone 1]
- **Mac App** (`macos/`) - SwiftUI native interface [Milestone 2]
- **Control Plane** (`control-plane/`) - Rails coordination service [Milestone 3]

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Status

**⚠️ Active Development**: Milestone 0 complete (core worktree orchestration). Milestones 1-3 (agent, native app, control plane) in progress.

Current version is suitable for **local development workflows** with manual CLI usage. Multi-device coordination and native apps coming in future milestones.

## Related Projects

- [Git Worktree](https://git-scm.com/docs/git-worktree) - Official git worktree documentation
- [Tailscale](https://tailscale.com/) - Secure mesh VPN for device connectivity (Milestone 3)
- [Cloudflare Tunnels](https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/) - Expose local services securely (Milestone 1)
