---
sidebar_position: 3
---

# Architecture Overview

## Current Architecture

BranchBox is currently implemented as a Rust-based CLI tool that orchestrates git worktrees and development environments. The system consists of two main components:

### 1. Core Library (`worktree-core`)

**Location**: `core/`

**Purpose**: Shared business logic for git worktree and devcontainer orchestration

**Key Modules**:

- **`naming`**: Generate DNS-safe, dasherized feature names from free-form titles
- **`validation`**: Validate environment, git state, and configuration before operations
- **`adapters`**: Auto-detect and configure for different project stacks (Rails, Node.js, Rust, Generic)
- **`modules`**: Composable feature components that manage lifecycle (compose, database, tunnel, specs, devcontainer)
- **`git`**: Git worktree operations using the `git2` crate
- **`workflows`**: High-level orchestration of feature worktree lifecycle

**Key Features**:
- Stack detection based on project markers (Gemfile, package.json, Cargo.toml, etc.)
- Pluggable adapter system for stack-specific behavior
- Pluggable module system for optional features
- Environment variable management and propagation
- Dependency-ordered module execution
- Feature state registry (JSON-based)

### 2. CLI Tool (`branchbox`)

**Location**: `cli/`

**Purpose**: Command-line interface for worktree management

**Available Commands**:
```bash
# Initialize a project with BranchBox
branchbox init

# Detect project stack and modules
branchbox detect

# Generate and validate feature names
branchbox name generate "OAuth Integration"
branchbox name validate oauth-integration

# Manage feature worktrees
branchbox feature start oauth-integration
branchbox feature list
branchbox feature teardown oauth-integration

# Sync devcontainer configuration
branchbox devcontainer sync
```

**Distribution**:
- Pre-built binaries for Linux, macOS, and Windows
- Install script for Linux
- Future: Homebrew tap for macOS
- Future: Scoop package for Windows

## How It Works

### Feature Workflow

```
┌─────────────────────────────────────────────────────────┐
│                  branchbox feature start                │
└───────────────────┬─────────────────────────────────────┘
                    │
        ┌───────────▼──────────┐
        │  Validate Request    │
        │  - Git repo check    │
        │  - Feature name      │
        └───────────┬──────────┘
                    │
        ┌───────────▼──────────┐
        │  Detect Stack        │
        │  - Rails             │
        │  - Node.js           │
        │  - Rust              │
        │  - Generic           │
        └───────────┬──────────┘
                    │
        ┌───────────▼──────────┐
        │  Detect Modules      │
        │  - Compose           │
        │  - Database          │
        │  - Tunnel            │
        │  - Specs             │
        │  - Devcontainer      │
        └───────────┬──────────┘
                    │
        ┌───────────▼──────────┐
        │  Create Worktree     │
        │  - Git branch        │
        │  - Directory         │
        └───────────┬──────────┘
                    │
        ┌───────────▼──────────┐
        │  Run Adapter Setup   │
        │  - Copy secrets      │
        │  - DB setup          │
        └───────────┬──────────┘
                    │
        ┌───────────▼──────────┐
        │  Run Module Setup    │
        │  (in dependency      │
        │   order)             │
        └───────────┬──────────┘
                    │
        ┌───────────▼──────────┐
        │  Register Feature    │
        │  (.branchbox/        │
        │   registry.json)     │
        └───────────┬──────────┘
                    │
        ┌───────────▼──────────┐
        │  Return Summary      │
        └──────────────────────┘
```

### Adapters

Adapters provide stack-specific behavior and are automatically detected:

| Adapter | Detection | Confidence | Features |
|---------|-----------|------------|----------|
| **Rails** | `Gemfile`, `config/database.yml` | 90 | Database setup, Rails-specific secrets |
| **Node.js** | `package.json` | 80 | npm/yarn/pnpm detection, .env handling |
| **Rust** | `Cargo.toml` | 80 | Cargo workspace handling |
| **Generic** | Always matches | 10 | Basic .env copying |

The adapter with the highest confidence score is selected. All adapters implement:
- `detect()` - Return confidence score 0-100
- `service_url()` - Provide local service URL for tunnel ingress
- `copy_secrets()` - Copy environment files to worktree
- `setup()` - Stack-specific initialization
- `cleanup()` - Stack-specific teardown

### Modules

Modules are composable features that can be enabled/disabled:

| Module | Purpose | Dependencies |
|--------|---------|--------------|
| **Compose** | Docker Compose project isolation | None |
| **Database** | Database-level isolation (Rails/Django) | None |
| **Tunnel** | Cloudflare tunnel provisioning | Compose |
| **Specs** | Feature spec lifecycle management | None |
| **Devcontainer** | Devcontainer config synchronization | None |

Modules execute in dependency order (topologically sorted). Each module implements:
- `detect()` - Should this module run?
- `init()` - Initialize module state
- `setup()` - Provision resources during feature start
- `teardown()` - Clean up resources during feature teardown
- `validate()` - Validate configuration
- `dependencies()` - Declare dependencies on other modules

### State Management

BranchBox tracks feature worktrees in a local registry at `{repo_root}/.branchbox/registry.json`:

```json
{
  "work_feature": "oauth-integration",
  "branch_name": "feature/oauth-integration",
  "worktree_path": "/path/to/repo-features/oauth-integration",
  "feature_url": "oauth-integration.example.com",
  "status": "Active",
  "created_at": "2025-01-15T10:30:00Z",
  "updated_at": "2025-01-15T10:30:00Z",
  "compose_project_name": "myapp-oauth-integration",
  "tunnel": {
    "status": "active",
    "hostname": "oauth-integration.tunnel.com"
  },
  "pr_number": 123,
  "color": "#FF5733",
  "devcontainer_outdated": false,
  "last_sync_at": "2025-01-15T10:30:00Z"
}
```

## Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Core Library** | Rust | Fast, safe, cross-platform |
| **CLI** | Rust + Clap | Single binary, fast startup |
| **Git Operations** | git2 (libgit2) | Native git operations |
| **State Storage** | JSON files | Simple, human-readable |
| **Docker** | Docker Compose | Container orchestration |
| **Tunnel** | Cloudflare Tunnel | Public URL provisioning |

## Development Setup

### Prerequisites

- Rust 1.75+ (`rustup install stable`)
- Git 2.30+
- Docker 24+ (optional, for compose module)
- Cloudflare account (optional, for tunnel module)

### Building from Source

```bash
# Clone repository
git clone https://github.com/branchbox/branchbox.git
cd branchbox

# Build core library
cd core
cargo build
cargo test

# Build CLI
cd ../cli
cargo build --release

# Install locally
cargo install --path . --locked

# Verify installation
branchbox --version
```

### Project Structure

```
branchbox/
├── core/               # worktree-core library
│   ├── src/
│   │   ├── adapters/   # Stack adapters
│   │   ├── modules/    # Feature modules
│   │   ├── workflows/  # High-level orchestration
│   │   ├── git.rs      # Git operations
│   │   ├── naming.rs   # Feature name utilities
│   │   └── validation.rs
│   └── Cargo.toml
├── cli/                # branchbox CLI
│   ├── src/
│   │   ├── commands/   # Command handlers
│   │   └── main.rs
│   └── Cargo.toml
├── docs/               # Documentation (Docusaurus)
├── .devcontainer/      # Dev container setup
└── Cargo.toml          # Workspace root
```

## Future Architecture

BranchBox is designed to evolve into a distributed system with:

- **Agent Daemon**: Long-running Rust daemon for background operations
- **Control Plane**: Rails-based web dashboard for multi-device management
- **Native Apps**: macOS/Windows native applications
- **Tailscale Network**: Secure mesh VPN for device communication
- **Offline-First**: Local-first with optional cloud sync

These components are planned for future milestones but not currently implemented. The current CLI-based architecture will remain fully functional and is the foundation for future enhancements.

## References

- [Git Worktree Documentation](https://git-scm.com/docs/git-worktree)
- [Rust Documentation](https://doc.rust-lang.org/)
- [Docker Compose Documentation](https://docs.docker.com/compose/)
- [Cloudflare Tunnel Documentation](https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/)
