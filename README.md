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

### Using Devcontainer (Recommended)

This project includes a complete devcontainer setup that provides a consistent development environment.

**Prerequisites:**
- Docker Desktop or Docker Engine
- VS Code with Remote - Containers extension, or
- Cursor (has built-in devcontainer support)

**Steps:**

1. Clone the repository:
   ```bash
   git clone https://github.com/your-org/worktree-manager.git
   cd worktree-manager
   ```

2. Open in devcontainer:
   - **VS Code/Cursor**: Command Palette → "Reopen in Container"
   - **CLI**: `devcontainer up --workspace-folder .`

3. Inside the container:
   ```bash
   # Build the core library
   cd core
   cargo build

   # Run tests
   cargo test

   # Check documentation
   cargo doc --open
   ```

### Local Development (without devcontainer)

**Prerequisites:**
- Rust 1.75+ ([Install Rust](https://rustup.rs/))
- Git
- Docker (optional, for testing Docker operations)

**Setup:**

```bash
# Clone repository
git clone https://github.com/your-org/worktree-manager.git
cd worktree-manager

# Build core library
cd core
cargo build

# Run tests
cargo test

# Install development tools
cargo install cargo-watch cargo-edit cargo-expand
```

## Development

### Building

```bash
# Build all workspace members
cargo build

# Build with release optimizations
cargo build --release

# Build specific package
cargo build --package worktree-core
```

### Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test naming

# Run doctests
cargo test --doc

# Watch mode (requires cargo-watch)
cargo watch -x test
```

### Code Quality

```bash
# Format code
cargo fmt

# Run Clippy (linter)
cargo clippy

# Check without building
cargo check
```

### Documentation

```bash
# Generate and open docs
cargo doc --open

# Generate docs without opening
cargo doc --no-deps
```

## Meta Feature: Bootstrap System 🎯

The worktree-manager can bootstrap devcontainer configurations for **any project** - including itself! This meta capability is implemented in `core/src/bootstrap/`.

**Concept**: A tool that sets up development environments can set up its own development environment.

**Usage (planned):**

```bash
# Bootstrap devcontainer for a Rails project
worktree bootstrap --stack rails /path/to/rails-project

# Bootstrap devcontainer for a Node.js project
worktree bootstrap --stack nodejs /path/to/node-project

# Auto-detect stack and bootstrap
worktree bootstrap /path/to/project

# Bootstrap worktree-manager itself (meta!)
worktree bootstrap --stack rust .
```

**What it generates:**
- `.devcontainer/devcontainer.json` - VS Code/Cursor devcontainer configuration
- `.devcontainer/compose.yaml` - Docker Compose services
- `.devcontainer/Dockerfile` - Custom development image
- `.env.sample` - Environment variable template
- Stack-specific setup scripts

**Why this matters:**
- Self-documenting development setup
- Reproducible environments
- Onboard new developers instantly
- Same tool manages worktrees AND their development containers

## Project Structure

```
worktree-manager/
├── .devcontainer/          # Dev container configuration (meta!)
│   ├── devcontainer.json   # VS Code devcontainer config
│   ├── compose.yaml        # Docker Compose setup
│   └── Dockerfile          # Rust development image
├── core/                   # Core Rust library
│   ├── src/
│   │   ├── naming.rs       # Feature name generation
│   │   ├── validation.rs   # Environment validation
│   │   ├── adapters/       # Stack adapters (Rails, Node.js)
│   │   ├── modules/        # Feature modules (tunnel, database)
│   │   └── bootstrap/      # Self-bootstrapping system (meta!)
│   └── Cargo.toml
├── agent/                  # Local agent daemon (planned)
├── cli/                    # CLI tool (planned)
├── macos/                  # Mac app (planned)
├── docs/                   # Documentation
├── .env.sample             # Environment variable template
├── .gitignore
├── Cargo.toml              # Workspace configuration
└── README.md               # This file
```

## Migration from Bash Scripts

This project is a Rust reimplementation of the bash-based worktree workflow in `lib/` and `bin/feature-*`.

Migration plan:

1. ✅ Core library (naming, validation, adapters)
2. ⏳ Git operations, remaining adapters
3. ⏳ Modules (tunnel, database, compose, specs)
4. ⏳ Bootstrap system (generate devcontainer configs)
5. ⏳ Local agent
6. ⏳ CLI tool
7. ⏳ Mac app
8. ⏳ Control plane integration

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
