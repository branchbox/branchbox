# BranchBox

[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
[![CI](https://github.com/branchbox/branchbox/workflows/CI/badge.svg)](https://github.com/branchbox/branchbox/actions)
[![License](https://img.shields.io/github/license/branchbox/branchbox)](LICENSE)

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

## Installation

### Quick Install

#### macOS (Homebrew)

*Coming soon - Homebrew tap will be available in a future release.*

```bash
brew install branchbox/tap/branchbox
```

#### Linux

Install with our automated script:

```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
```

**Options:**

```bash
# Install specific version
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | BRANCHBOX_VERSION=v0.1.0 bash

# Install to custom directory
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | INSTALL_DIR=$HOME/bin bash

# Download and inspect script first (recommended)
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh -o install.sh
less install.sh  # Review the script
chmod +x install.sh
./install.sh
```

**What the script does:**
- Detects your architecture (x86_64 or ARM64)
- Downloads the latest release from GitHub
- Verifies SHA256 checksums
- Installs to `/usr/local/bin` (with sudo) or `~/.local/bin` (without sudo)
- Provides clear output and error messages

#### Windows (Scoop)

*Coming soon - Scoop package will be available in a future release.*

```powershell
scoop bucket add branchbox https://github.com/branchbox/scoop-bucket
scoop install branchbox
```

### Download Binaries

Download pre-built binaries from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest):

#### Linux

```bash
# Download (replace VERSION and ARCH as needed)
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-x86_64-unknown-linux-gnu.tar.gz -o branchbox.tar.gz

# Verify checksum
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/checksums.txt -o checksums.txt
sha256sum -c checksums.txt --ignore-missing

# Extract
tar xzf branchbox.tar.gz

# Install (requires sudo)
sudo mv branchbox-VERSION-x86_64-unknown-linux-gnu/branchbox /usr/local/bin/
sudo chmod +x /usr/local/bin/branchbox

# Verify installation
branchbox --version
```

#### macOS

```bash
# Download (Intel)
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-x86_64-apple-darwin.tar.gz -o branchbox.tar.gz

# Download (Apple Silicon)
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-aarch64-apple-darwin.tar.gz -o branchbox.tar.gz

# Verify checksum
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/checksums.txt -o checksums.txt
shasum -a 256 -c checksums.txt --ignore-missing

# Extract
tar xzf branchbox.tar.gz

# Install (requires sudo)
sudo mv branchbox-VERSION-*/branchbox /usr/local/bin/
sudo chmod +x /usr/local/bin/branchbox

# Verify installation
branchbox --version
```

#### Windows

```powershell
# Download
Invoke-WebRequest -Uri "https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-x86_64-pc-windows-msvc.zip" -OutFile branchbox.zip

# Verify checksum
$hash = (Get-FileHash branchbox.zip -Algorithm SHA256).Hash.ToLower()
# Compare with checksums.txt

# Extract
Expand-Archive branchbox.zip

# Add to PATH or move to a directory in PATH
Move-Item branchbox\branchbox-VERSION-x86_64-pc-windows-msvc\branchbox.exe C:\Users\$env:USERNAME\bin\

# Verify installation
branchbox --version
```

### Build from Source

```bash
# Clone repository
git clone https://github.com/branchbox/branchbox.git
cd branchbox

# Build and install
cargo install --path cli --locked

# Verify installation
branchbox --version
```

### Verify Installation

After installation, verify that BranchBox is working:

```bash
# Check version
branchbox --version

# Display help
branchbox --help

# List available commands
branchbox feature --help
```

### Installation Troubleshooting

#### Linux Install Script

**Q: The script says my architecture is unsupported**

A: BranchBox currently supports x86_64 and aarch64 (ARM64) on Linux. For other architectures, try building from source.

**Q: Installation fails with permission denied**

A: Try installing to a user directory:
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | INSTALL_DIR=$HOME/.local/bin bash
```

Then add `~/.local/bin` to your PATH:
```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

**Q: Checksum verification fails**

A: This could indicate a corrupted download or a network issue. Try running the installer again. If the problem persists, download the binary manually from the [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest) page.

**Q: I don't want to pipe curl to sh**

A: You can download and inspect the script first:
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh -o install.sh
less install.sh  # Inspect the script
chmod +x install.sh
./install.sh
```

**Q: The script cannot find the release**

A: Make sure you have an active internet connection. If a specific version doesn't exist, you'll see an error. Check available versions at [GitHub Releases](https://github.com/branchbox/branchbox/releases).

#### Windows

**Q: Scoop install fails**

A: Make sure you have Scoop installed first:
```powershell
Set-ExecutionPolicy RemoteSigned -Scope CurrentUser
irm get.scoop.sh | iex
```

**Q: branchbox.exe is not recognized**

A: Restart your terminal or run:
```powershell
scoop reset branchbox
```

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
   git clone https://github.com/branchbox-branchbox.git
   cd branchbox
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

**Tooling baked into the devcontainer:**
- Rust stable toolchain with `cargo-watch`, `cargo-edit`, and `cargo-expand`
- Node.js 20 plus the `@openai/codex` and `@anthropic-ai/claude-code` CLIs via `ghcr.io/rbarazi/devcontainer-features/ai-npm-packages`
- Docker-in-Docker runtime (official Docker packages) for container orchestration tests
- Persistent Codex configuration/history stored on the host at `.codex/`

### Local Development (without devcontainer)

**Prerequisites:**
- Rust 1.75+ ([Install Rust](https://rustup.rs/))
- Git
- Docker (optional, for testing Docker operations)

**Setup:**

```bash
# Clone repository
git clone https://github.com/branchbox-branchbox.git
cd branchbox

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

The branchbox can bootstrap devcontainer configurations for **any project** - including itself! This meta capability is implemented in `core/src/bootstrap/`.

**Concept**: A tool that sets up development environments can set up its own development environment.

**Usage (planned):**

```bash
# Bootstrap devcontainer for a Rails project
worktree bootstrap --stack rails /path/to/rails-project

# Bootstrap devcontainer for a Node.js project
worktree bootstrap --stack nodejs /path/to/node-project

# Auto-detect stack and bootstrap
worktree bootstrap /path/to/project

# Bootstrap branchbox itself (meta!)
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
branchbox/
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

## Testing

The install script has automated tests. See [test/README.md](test/README.md) for details.

```bash
# Run tests
bats test/install.bats         # All tests
shellcheck install.sh          # Linting

# Test on specific distro
docker run -it --rm -v $(pwd)/install.sh:/install.sh:ro ubuntu:22.04 bash /install.sh
```

**Coverage**: Static analysis (shellcheck) + automated tests (bats) + CI (GitHub Actions)

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test --all` (Rust) or `./test/run-tests.sh` (install script)
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
