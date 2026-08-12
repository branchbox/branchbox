---
sidebar_position: 1
---

# Architecture Deep Dive

:::info For Contributors
This document covers internal architecture details. For user-focused documentation, see [How It Works](../how-it-works.md).
:::

# BranchBox — Distributed Architecture

## Overview

A distributed development environment orchestrator that manages git worktrees and devcontainers via a Rust CLI, always-on agent, and SwiftUI macOS client.

## System Architecture

```
┌──────────────────────────────────────────────┐
│                User Device                   │
│ ┌──────────────┐   ┌──────────────┐          │
│ │  Mac App     │   │    CLI       │          │
│ │  (SwiftUI)   │   │   (Rust)     │          │
│ └──────┬───────┘   └──────┬───────┘          │
│        │                  │                  │
│        └───────┬──────────┘                  │
│                │                             │
│        ┌───────▼──────────┐                  │
│        │  BranchBox Agent │                  │
│        │  (Rust daemon)   │                  │
│        └───────┬──────────┘                  │
│                │                             │
│        ┌───────▼──────────┐                  │
│        │ Worktree Core    │                  │
│        │ (Rust library)   │                  │
│        └───────┬──────────┘                  │
│                │ RuntimeProvider             │
│      ┌─────────┼──────────┐                  │
│      │         │          │                  │
│ container   local-vm     sbx                 │
│ (default)  (reserved) (experimental)         │
│      └─────────┴──────────┘                  │
└──────────────────────────────────────────────┘
                 │
                 │ Batched events / heartbeats
                 ▼
      HTTPS drain (control-plane endpoint or stub)
```

## Components

### 1. Worktree Core (Rust Library)

**Location**: `core/`

**Purpose**: Shared business logic for git worktree and devcontainer orchestration

**Modules**:
- `naming`: Generate DNS-safe, dasherized feature names
- `validation`: Validate environment, git state, configuration
- `adapters`: Auto-detect and configure for different stacks (Rails, Node.js, etc)
- `modules`: Composable feature components (tunnel, database, compose, specs)
- `git`: Git worktree operations
- `docker`: Docker Compose orchestration
- `cloudflare`: Cloudflare Tunnel API client
- `runtime`: Outer workspace isolation providers, lifecycle metadata, command routing, and host-port publication

**Key Features**:
- Stack detection (Rails, Node.js, Generic)
- Adapter plugin system
- Module plugin system
- Environment variable management
- Template rendering for devcontainer configs
- Opinionated devcontainer layout that mounts the parent worktree tree at `/workspaces` so per-feature folders resolve consistently inside containers
- Reads optional `APP_NAME`/`APP_SLUG` settings from `.env` to align compose/devcontainer naming with the host project and propagates them to Docker Compose container names
- Keeps the devcontainer definition independent of the outer runtime boundary. The default provider
  preserves host-container behavior; experimental SBX starts the devcontainer and its nested Compose
  stack inside a named sandbox, routes commands into the devcontainer, and bridges published ports.

**Distribution**:
- Published to crates.io as `worktree-core`
- Embedded in agent, CLI, and available for FFI bindings

### 2. Agent (Rust Daemon)

**Location**: `agent/`

**Purpose**: Long-running daemon on the user's device that executes worktree operations. Milestone 1 delivered the macOS/Linux/devcontainer daemon plus CLI bridge; Milestone 2 added the control-plane HTTP drain and `branchbox agent status` reporting. Windows transport support is tracked internally (see `docs/features/backlog/agent-windows-support.md` in the repo).

**Features**:
- gRPC server listening on localhost (configurable bind address)
- Executes commands from local clients (CLI + macOS preview) over IPC/gRPC
- Offline operation with SQLite queue + durable control-plane acknowledgements (`control_plane_status.last_ack_event_id`)
- Configurable HTTP drain (`BRANCHBOX_CP_ENDPOINT`/`BRANCHBOX_CP_TOKEN`) with exponential backoff/jitter and telemetry surfaced via `branchbox agent status`
- Periodic heartbeat + event batching for the configured drain endpoint
- State synchronization
- Auto-update capability
- CLI fallback bridge when the daemon is offline so macOS preview + CLI stay usable

**Communication**:
- **Local**: Unix domain socket (BranchBox agent-managed path)
- **Remote**: TCP gRPC (configurable address) for CLI/macOS clients
- **Drain**: HTTPS POSTs to the configured endpoint (control-plane or stub) for telemetry/state sync

**Installation**:
```bash
# Homebrew
brew install branchbox-agent

# Initialize
branchbox-agent init

# Install as system service
sudo branchbox-agent install
```

### 3. CLI Tool (Rust)

**Location**: `cli/`

**Purpose**: Command-line interface for local worktree management

**Commands**:
```bash
branchbox feature start "Add OAuth Integration"
branchbox feature list
branchbox feature teardown oauth-integration
branchbox devcontainer sync
```

**Distribution**:
- Homebrew: `brew install worktree`
- Cargo: `cargo install --path cli --locked`
- Direct binary download from GitHub releases

### 4. Mac App (SwiftUI)

**Location**: `macos/`

**Purpose**: Native macOS application for worktree management

**Features**:
- View local worktrees with adapter/module metadata, prompt history, tunnel telemetry, and transport badges
- Start/stop/teardown features (minimal mode toggles plus `--force` / `--complete-spec` confirmations)
- Visual indicators for agent gRPC vs CLI fallback plus drain health surfaced via `branchbox agent status`
- Monitor Docker containers + module output snippets
- Workspace picker + configuration UI for CLI/gRPC endpoints

**Communication**:
- Talks to local agent via Unix socket or localhost gRPC
- Falls back to invoking the CLI directly when the daemon is unavailable

**Distribution**:
- Mac App Store
- Direct download (DMG)

## Communication Protocols

### Local Communication

- CLI and macOS clients talk to the agent over a Unix domain socket under `~/.branchbox/agent/` by default.
- Non-interactive tooling can opt into TCP gRPC by exporting `BRANCHBOX_AGENT_GRPC_ADDR=127.0.0.1:50515`.
- Authentication piggybacks on OS permissions—the socket inherits the user's UID/GID and is not world-readable.

```
CLI / Mac App → Unix socket → BranchBox agent → Worktree core
```

### Remote Communication

- The gRPC server can bind to alternative interfaces (e.g., devcontainer ↔ host) when `BRANCHBOX_AGENT_GRPC_ADDR` points at a non-loopback IP.
- Secure the connection via SSH or WireGuard tunnels if you expose the agent outside localhost.
- All APIs return structured errors so remote callers can fall back to CLI direct mode if the agent becomes unreachable.

### Telemetry Drain

- The agent batches heartbeats and workflow events, then POSTs them to `BRANCHBOX_CP_ENDPOINT` with `BRANCHBOX_CP_TOKEN` for authentication.
- Failures trigger exponential backoff (configurable via env) and surface via `branchbox agent status`.
- `scripts/manual-agent-e2e.sh --cp-stub` lets you observe the payloads without a real control plane.

## Data Models

### Agent (SQLite)

```sql
-- Local worktree state
CREATE TABLE worktrees (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    branch TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    url TEXT,
    status TEXT NOT NULL,
    metadata TEXT, -- JSON
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Offline queue
CREATE TABLE pending_updates (
    id INTEGER PRIMARY KEY,
    event_type TEXT NOT NULL,
    data TEXT NOT NULL, -- JSON
    created_at INTEGER NOT NULL,
    synced_at INTEGER
);

-- Agent configuration
CREATE TABLE config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

## Offline Operation

### Offline-First Design

The agent is designed to work **completely offline**:

1. **Local commands execute immediately**
   - Mac App/CLI → Agent → Worktree Core
   - No network required

2. **State updates queued for sync**
   - Agent writes to a durable SQLite queue
   - Periodically attempts to sync with the configured HTTP drain
   - Queue drains when the endpoint responds successfully

3. **Conflict resolution**
   - Drain failures leave events queued; agent logs the error and surfaces it via `branchbox agent status`
   - Operators inspect stub/endpoint logs and re-run the sync once the issue is resolved

### Queue Management

```rust
// Agent queues state update
queue.enqueue(StateUpdate {
    device_id: "...",
    worktree_name: "oauth-integration",
    status: WorktreeStatus::Running,
    timestamp: Utc::now(),
});

// Periodic sync task
loop {
    if drain.is_reachable() {
        queue.drain_all().await?;
    }
    tokio::time::sleep(Duration::from_secs(30)).await;
}
```

## Deployment

### Agent Installation (macOS)

```bash
# Homebrew
brew tap branchbox/tap
brew install branchbox-agent

# Manual
curl -L https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/branchbox-agent-darwin-arm64.tar.gz | tar xz
sudo mv branchbox-agent /usr/local/bin/

# Initialize
branchbox-agent init

# Install as LaunchDaemon
sudo branchbox-agent install
```

### Agent Configuration

`~/.branchbox/agent/config.toml`:
```toml
workspace_root = "/workspaces/main"
state_dir = "/Users/you/.branchbox/agent"
socket_path = "/Users/you/.branchbox/agent/branchbox-agent.sock"
heartbeat_interval_secs = 30
grpc_enabled = true
grpc_addr = "127.0.0.1:50515"
event_flush_interval_secs = 10
event_batch_size = 50
event_log_only = false

[control_plane]
enabled = true
endpoint = "https://example.test/hooks/devices"
api_token = "stub-token"
verify_tls = true

[logging]
level = "info"
file = "/Users/you/.branchbox/agent/agent.log"
```

## Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Core Library** | Rust | Fast, safe, embeddable, cross-platform |
| **Agent** | Rust + Tokio | Low resource, reliable, async I/O |
| **CLI** | Rust + Clap | Single binary, fast startup, great UX |
| **Mac App** | SwiftUI | Native macOS, best performance/UX |
| **Communication** | gRPC (tonic) | Type-safe, bi-directional streaming |
| **Database** | SQLite | Durable local registry + event queue |

## Development Setup

### Prerequisites

- Rust 1.75+
- Docker
- Node.js 20+ (for building the docs site)
- Swift toolchain + Xcode (for the macOS preview app)

### Local Development

```bash
# Clone repository
git clone https://github.com/branchbox/branchbox
cd branchbox

# Build core library
cd core
cargo build

# Run tests
cargo test

# Build agent
cd ../agent
cargo build
cargo run -- --config-file dev-config.toml

# Build CLI
cd ../cli
cargo build
./target/debug/worktree --help

## References

- [Git Worktree Documentation](https://git-scm.com/docs/git-worktree)
- [gRPC Rust (Tonic)](https://github.com/hyperium/tonic)
- [Tokio Async Runtime](https://tokio.rs/)
