# Worktree Manager

A distributed development environment orchestrator that manages git worktrees and devcontainers across multiple devices.

## Overview

Worktree Manager enables you to:

- **Manage multiple feature branches** with complete isolation
- **Work across devices** via a centralized control plane
- **Offline-first operation** with automatic state synchronization
- **Auto-provision infrastructure** (Cloudflare tunnels, Docker containers, databases)
- **Stack detection** (Rails, Node.js, etc.) with intelligent configuration

## Architecture

```
Local Device ─┬─ Mac App (SwiftUI)
              ├─ CLI Tool (Rust)
              └─ Agent (Rust daemon) ──┬─ Worktree Core (Rust library)
                                       │
                          Tailscale Network
                                       │
Control Plane (Rails) ─────────────────┘
  ├─ Web Dashboard
  ├─ API
  └─ PostgreSQL
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture.

## Components

### Core Library (`core/`)

Rust library providing:
- Git worktree operations
- Stack adapter system (Rails, Node.js, Generic)
- Module system (tunnel, database, compose, specs)
- Naming and validation utilities
- Cloudflare API client

### Agent (`agent/`)

Long-running Rust daemon that:
- Executes worktree operations locally
- Communicates with control plane via Tailscale
- Operates offline with SQLite queue
- Provides gRPC API for local clients

### CLI Tool (`cli/`)

Command-line interface for:
- Local worktree management
- Remote device management
- Feature lifecycle operations

### Mac App (`macos/`)

Native macOS application with:
- Beautiful SwiftUI interface
- Local worktree management
- Multi-device support
- Real-time updates

### Control Plane (`control-plane/`)

Rails web application for:
- User authentication
- Device management
- Remote worktree orchestration
- State aggregation across devices

## Quick Start

### Installation

```bash
# Install agent
brew tap your-org/worktree
brew install worktree-agent

# Install CLI
brew install worktree

# Install Mac app
brew install --cask worktree-app
```

### Initialize Agent

```bash
# Initialize agent
worktree-agent init

# Start agent
sudo worktree-agent install
```

### Start a Feature

```bash
# Local operation
worktree start "oauth integration"

# Remote operation
worktree remote start --device=macbook "oauth integration"
```

## Development

### Prerequisites

- Rust 1.75+
- Ruby 3.3+
- PostgreSQL 16+
- Docker
- Tailscale (optional, for remote features)

### Build Core Library

```bash
cd core
cargo build
cargo test
```

### Build Agent

```bash
cd agent
cargo build
cargo run -- --config-file dev-config.toml
```

### Build CLI

```bash
cd cli
cargo build
./target/debug/worktree --help
```

### Run Control Plane

```bash
cd control-plane
bin/rails db:setup
bin/dev
```

## Migration from Bash Scripts

This project is a Rust reimplementation of the bash-based worktree workflow in `lib/` and `bin/feature-*`.

Migration plan:

1. ✅ Core library (naming, validation, git operations)
2. ⏳ Adapters (Rails, Node.js)
3. ⏳ Modules (tunnel, database, compose, specs)
4. ⏳ Local agent
5. ⏳ CLI tool
6. ⏳ Mac app
7. ⏳ Control plane integration

## Documentation

- [Architecture](docs/ARCHITECTURE.md) - System design and components
- [Protocol](docs/PROTOCOL.md) - Communication protocols (gRPC, REST)
- [Original Docs](../docs/architecture/) - Bash implementation docs

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test --all`
5. Submit a pull request

## License

MIT License - see LICENSE file for details

## Status

**⚠️ In Development**: This project is actively being developed and is not yet ready for production use.

Current progress:
- [x] Architecture design
- [x] Protocol specification
- [ ] Core library implementation
- [ ] Agent implementation
- [ ] CLI implementation
- [ ] Mac app implementation
- [ ] Control plane implementation

## Related Projects

- [Git Worktree](https://git-scm.com/docs/git-worktree) - Official git worktree documentation
- [Tailscale](https://tailscale.com/) - Secure mesh VPN for device connectivity
- [Cloudflare Tunnels](https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/) - Expose local services securely
