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

# Output:
# Active features:
#   oauth-integration       feature/oauth-integration       /Users/you/projects/your-app-oauth-integration/
#   api-refactor           feature/api-refactor           /Users/you/projects/your-app-api-refactor/
#   payment-integration    feature/payment-integration    /Users/you/projects/your-app-payment-integration/
```

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

### Troubleshooting
- **No “Reopen in Container” prompt**: confirm `.devcontainer/` exists in the feature (`ls .devcontainer/`); rerun `branchbox devcontainer sync` if it is missing files.
- **Tools ask to re-authenticate**: verify shared mounts inside the container (`mount | grep -E '(codex|claude|gh)'`) and ensure `SHARED_CONFIG_DIR` points to the directory holding your credentials.
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

## What Works Now

**✅ Milestone 0 Complete** - Core worktree orchestration:
- Full feature lifecycle (`start`, `teardown`, `list`)
- Stack detection (Rails, Node.js, Generic)
- Module system (Docker Compose, Database, Specs)
- Environment configuration (.env copying with `APP_URL` injection)
- State tracking (JSON registry at `.branchbox/registry.json`)

**🚧 Coming Soon:**
- Cloudflare tunnel provisioning (Milestone 1)
- Agent daemon for background orchestration (Milestone 1)
- Native macOS app (Milestone 2)
- Multi-device coordination via control plane (Milestone 3)
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
