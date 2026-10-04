---
sidebar_position: 1
---

# Architecture Deep Dive

:::info For Contributors
This document covers internal architecture details. For user-focused documentation, see [How It Works](../how-it-works.md).
:::

# BranchBox — Distributed Architecture

## Overview

A development environment orchestrator that manages git worktrees and devcontainers. The Rust core library does the work; the `branchbox` CLI is its primary interface, and its `--json` output is the integration API for everything else: the native macOS app, scripts, CI and external orchestrators. An optional agent daemon runs the same workflows in the background and drains telemetry to a control-plane endpoint.

## System Architecture

```
┌──────────────────────────────────────────────┐
│                User Device                   │
│ ┌──────────────┐  argv + --json  ┌─────────┐ │
│ │  Mac App     │ ──────────────▶ │  CLI    │ │
│ │  (SwiftUI)   │ ◀────────────── │ (Rust)  │ │
│ └──────────────┘  one JSON doc   └────┬────┘ │
│                                       │      │
│ ┌──────────────┐               ┌──────▼────┐ │
│ │ Agent daemon │ ─────────────▶│ Worktree  │ │
│ │ (optional)   │  in-process   │ Core (lib)│ │
│ └──────┬───────┘               └──────┬────┘ │
│        │          .branchbox/registry.json   │
│        │          (locked, atomic writes)    │
│        │                              │      │
│        │                RuntimeProvider      │
│        │              ┌───────┼────────┐     │
│        │         container local-vm   sbx    │
│        │         (default)(Firecracker)(exp.)│
└────────┼─────────────────────────────────────┘
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
- Linked into the CLI and the agent

**Registry integrity**: feature state lives in `{repo_root}/.branchbox/registry.json`, shared by every CLI run, the Mac app (through the CLI) and the agent. Every write takes an exclusive advisory lock on the `.branchbox` state directory (`atomic_fs::lock_state_dir`; the OS releases it if the holder dies) and lands through one atomic rename (`atomic_fs::write_atomic`), so concurrent starts and teardowns never lose entries and readers never see a torn file. A separate per-repository lock serializes git worktree and branch changes. `feature start` registers the feature (with a `setup` record) as soon as its worktree exists, so an interrupted start stays visible as `interrupted`.

### 2. Agent (Rust Daemon)

**Location**: `agent/` (binary `branchbox-agent`)

**Purpose**: Optional long-running daemon that runs worktree workflows in the background and forwards telemetry. Milestone 1 delivered the macOS/Linux/devcontainer daemon; Milestone 2 added the control-plane HTTP drain and `branchbox agent status` reporting. Windows transport support is tracked internally (see `docs/features/backlog/agent-windows-support.md` in the repo).

**Features**:
- Links the core library in-process, so it runs the same workflows as the CLI
- JSON IPC on a Unix domain socket (`~/.branchbox/agent/branchbox-agent.sock` by default) and a tonic gRPC server on `127.0.0.1:50515`
- SQLite event queue with durable control-plane acknowledgements (`control_plane_status.last_ack_event_id`)
- Configurable HTTP drain (`BRANCHBOX_CP_ENDPOINT`/`BRANCHBOX_CP_TOKEN`) with exponential backoff/jitter and telemetry surfaced via `branchbox agent status`
- Periodic heartbeat + event batching for the configured drain endpoint

**Status**: the agent is not distributed: the Homebrew formula and release archives ship only the CLI. Run it from source (`cargo run -p branchbox-agent`, or `scripts/manual-agent-e2e.sh`). Today only `branchbox agent status` talks to it; the CLI and the Mac app run workflows directly. A Mac app backend that talks to the agent is planned for later, once the agent's handlers and transport are hardened.

**Configuration**: `~/.branchbox/agent/agent.toml` (override the path with `BRANCHBOX_AGENT_CONFIG`, the state directory with `BRANCHBOX_AGENT_DIR`, and the gRPC address with `BRANCHBOX_AGENT_GRPC_ADDR`). See [Agent Configuration](#agent-configuration).

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

**Machine interface**: every command with `--json` prints exactly one JSON document on stdout (human text goes to stderr), never prompts, and reports failures as an error envelope with a stable code. `branchbox version --json` lists capability strings so clients can gate features. See the [JSON contract](../reference/json-contract.md).

**Distribution**:
- Homebrew: `brew install branchbox/tap/branchbox`
- Install script: `curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash`
- Cargo: `cargo install --path cli --locked`
- Direct binary download from GitHub releases

### 4. Mac App (SwiftUI)

**Location**: `macos/` (a Swift package; see `macos/README.md`)

**Purpose**: Native macOS front end for BranchBox: every project's features at a glance, safe teardown and prune, health remediation, editor/terminal/agent launch, project setup and settings.

**How it works**:
- **CLI-JSON first.** The app spawns the user's installed `branchbox` (found on the login-shell `PATH`, never embedded by default) and decodes its `--json` output. It never writes repository files itself: settings go through `config apply`, the tunnel token through `tunnel credentials set --api-token-stdin`.
- **Capabilities, not versions.** `branchbox version --json` decides which features are enabled. A 0.13.x CLI runs in legacy mode, where the app performs the teardown safety checks itself.
- **Layers**: `BranchBoxKit` (contracts, models, pure planning) → `BranchBoxCLI` (process runner, environment, `CLIBackend`) and `BranchBoxStores` (observable state) → `BranchBoxApp` (SwiftUI). A `BranchBoxBackend` protocol keeps the UI independent of the transport; an agent-backed conformer can be added later.
- **Freshness.** FSEvents on `.branchbox/` refreshes the app within about a second of a CLI change made in Terminal.

**Distribution**: an ad-hoc signed universal `.app` built by `scripts/package-macos-app.sh` and uploaded by CI (`.github/workflows/macos-app.yml`). The app is not sandboxed: it must run `branchbox`, `git` and `docker` and read repositories anywhere. Developer ID signing, notarization and a Homebrew cask come later.

## Communication Protocols

### Mac App ↔ CLI

```
Mac App → branchbox <command> --json --repo <path> → Worktree core → .branchbox/registry.json
        ← one JSON document on stdout (payload or error envelope), exit code
```

- One process per operation, in its own process group so cancel and quit can stop it and its children.
- Concurrent operations are safe because the CLI locks the registry; with a 0.13.x CLI (no `registry-lock` capability) the app queues registry writers per project instead.

### Agent

- `branchbox agent status` talks to the agent over its Unix domain socket under `~/.branchbox/agent/`. The socket inherits the user's UID/GID and is not world-readable.
- The agent also serves gRPC on `127.0.0.1:50515` (`BRANCHBOX_AGENT_GRPC_ADDR` changes it). It has no authentication, so do not bind it to a non-loopback address; secure any remote access with SSH or WireGuard.

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

### Agent (from source)

```bash
cargo run -p branchbox-agent
# or exercise it with the drain stub
./scripts/manual-agent-e2e.sh --cp-stub
```

There is no packaged agent, service installer or LaunchDaemon yet.

### Agent Configuration

`~/.branchbox/agent/agent.toml` (every key is optional):
```toml
workspace_root = "/path/to/project/main"
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
```

## Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Core Library** | Rust | Fast, safe, embeddable, cross-platform |
| **Agent** | Rust + Tokio | Low resource, reliable, async I/O |
| **CLI** | Rust + Clap | Single binary, fast startup, great UX |
| **Mac App** | SwiftUI | Native macOS, best performance/UX |
| **App ↔ CLI** | `--json` process contract | One stable API for the app, scripts and CI |
| **Agent transport** | JSON IPC (Unix socket) + gRPC (tonic) | Local-only daemon access |
| **Feature registry** | JSON file, locked + atomic writes | Shared by every CLI version and the app |
| **Agent queue** | SQLite | Durable event queue for the drain |

## Development Setup

### Prerequisites

- Rust 1.89+
- Docker
- Node.js 20+ (for building the docs site)
- macOS 26 + Xcode 26 or later (for the Mac app)

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

# Build and run the agent (optional)
cd ..
cargo run -p branchbox-agent

# Build the CLI
cargo build -p branchbox-cli
./target/debug/branchbox --help

# Build and test the Mac app (macOS only)
swift build --package-path macos --build-tests -Xswiftc -warnings-as-errors
swift test --package-path macos --parallel
```

## References

- [Git Worktree Documentation](https://git-scm.com/docs/git-worktree)
- [gRPC Rust (Tonic)](https://github.com/hyperium/tonic)
- [Tokio Async Runtime](https://tokio.rs/)
