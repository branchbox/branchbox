---
title: Distributed Architecture Overview
sidebar_position: 1
description: Understand the distributed architecture that powers BranchBox across devices and the control plane.
---

# Worktree Manager - Distributed Architecture

## Overview

A distributed development environment orchestrator that manages git worktrees and devcontainers across multiple devices via a control plane and local agents.

## System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    User's Devices                       │
│         (connected via Tailscale network)               │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌──────────────────┐      ┌──────────────────┐       │
│  │   Mac App        │      │   CLI Tool       │       │
│  │   (SwiftUI)      │      │   (Rust)         │       │
│  └────────┬─────────┘      └────────┬─────────┘       │
│           │                         │                  │
│           └─────────┬───────────────┘                  │
│                     │                                  │
│           ┌─────────▼──────────┐                       │
│           │  Local Agent       │                       │
│           │  (Rust daemon)     │◄──────────┐           │
│           │                    │           │           │
│           │  - Receives cmds   │           │           │
│           │  - Executes ops    │      Tailscale        │
│           │  - Reports state   │       Tunnel          │
│           │  - Offline queue   │           │           │
│           └─────────┬──────────┘           │           │
│                     │                      │           │
│           ┌─────────▼──────────┐           │           │
│           │  Worktree Engine   │           │           │
│           │  (Rust core lib)   │           │           │
│           └────────────────────┘           │           │
└────────────────────────────────────────────┼───────────┘
                                             │
                                    Tailscale Network
                                             │
┌────────────────────────────────────────────┼───────────┐
│              Control Plane (Hosted)        │           │
├────────────────────────────────────────────┼───────────┤
│                                            │           │
│           ┌────────────────────┐           │           │
│           │  Web Dashboard     │           │           │
│           │  (Rails + Hotwire) │           │           │
│           └─────────┬──────────┘           │           │
│                     │                      │           │
│           ┌─────────▼──────────┐           │           │
│           │   Rails API        │───────────┘           │
│           │                    │                       │
│           │  - Device registry │                       │
│           │  - Job queue       │                       │
│           │  - State sync      │                       │
│           │  - User auth       │                       │
│           └─────────┬──────────┘                       │
│                     │                                  │
│           ┌─────────▼──────────┐                       │
│           │   PostgreSQL       │                       │
│           │   + Job Queue      │                       │
│           └────────────────────┘                       │
└─────────────────────────────────────────────────────────┘
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

**Key Features**:
- Stack detection (Rails, Node.js, Generic)
- Adapter plugin system
- Module plugin system
- Environment variable management
- Template rendering for devcontainer configs
- Opinionated devcontainer layout that mounts the parent worktree tree at `/workspaces` so per-feature folders resolve consistently inside containers
- Reads optional `APP_NAME`/`APP_SLUG` settings from `.env` to align compose/devcontainer naming with the host project and propagates them to Docker Compose container names

**Distribution**:
- Published to crates.io as `worktree-core`
- Embedded in agent, CLI, and available for FFI bindings

### 2. Agent (Rust Daemon)

**Location**: `agent/`

**Purpose**: Long-running daemon on user's device that executes worktree operations

**Features**:
- gRPC server listening on Tailscale IP + localhost
- Executes commands from control plane or local clients
- Offline operation with SQLite queue
- Periodic heartbeat to control plane
- State synchronization
- Auto-update capability

**Communication**:
- **Local**: Unix domain socket (`/var/run/worktree-agent.sock`)
- **Remote**: gRPC over Tailscale network
- **Control Plane**: Bi-directional gRPC streaming

**Installation**:
```bash
# Homebrew
brew install worktree-agent

# Initialize
worktree-agent init

# Install as system service
sudo worktree-agent install
```

### 3. CLI Tool (Rust)

**Location**: `cli/`

**Purpose**: Command-line interface for local worktree management

**Commands**:
```bash
# Local operations (talks to local agent)
worktree start "oauth integration"
worktree list
worktree teardown oauth-integration

# Remote operations (talks to control plane)
worktree devices
worktree remote start --device=macbook "oauth integration"
worktree remote list --device=macbook
```

**Distribution**:
- Homebrew: `brew install worktree`
- Cargo: `cargo install worktree-cli`
- Direct binary download from GitHub releases

### 4. Mac App (SwiftUI)

**Location**: `macos/`

**Purpose**: Native macOS application for worktree management

**Features**:
- View local worktrees
- Start/stop/teardown features
- Monitor Docker containers
- View logs
- Manage remote devices (optional)
- Beautiful native UI with live updates

**Communication**:
- Talks to local agent via Unix socket or localhost gRPC
- Optionally talks to control plane for multi-device management

**Distribution**:
- Mac App Store
- Direct download (DMG)

### 5. Control Plane (Rails)

**Location**: `control-plane/` (or extend existing Agentify app)

**Purpose**: Central management and coordination of multiple devices

**Features**:
- User authentication and authorization
- Device registration and management
- Remote command execution
- State aggregation across devices
- Feature spec library (shared across devices)
- Real-time updates via Turbo Streams
- Device health monitoring
- Audit logs

**API Endpoints**:
```ruby
# Device management
GET    /api/v1/devices
POST   /api/v1/devices/register
DELETE /api/v1/devices/:id

# Worktree operations
GET    /api/v1/devices/:device_id/worktrees
POST   /api/v1/devices/:device_id/worktrees/start
DELETE /api/v1/devices/:device_id/worktrees/:name

# Agent endpoints
POST   /agent/heartbeat
POST   /agent/report_state
```

**Distribution**:
- Hosted service (Heroku, Fly.io, Railway)
- Self-hosted via Kamal
- Docker Compose for local development

## Communication Protocols

### Local Communication

**Mac App/CLI ↔ Local Agent**

- **Protocol**: gRPC or Unix domain socket
- **Transport**: Localhost (127.0.0.1:50051) or `/var/run/worktree-agent.sock`
- **Security**: Local user permissions
- **Latency**: <1ms

```
CLI Tool → Unix Socket → Local Agent → Worktree Core
```

### Remote Communication

**Control Plane ↔ Agent**

- **Protocol**: gRPC
- **Transport**: Tailscale network (encrypted mesh VPN)
- **Security**: Device token authentication + Tailscale encryption
- **Latency**: 10-100ms

```
Web UI → Rails API → gRPC/Tailscale → Agent → Worktree Core
```

### State Synchronization

**Agent → Control Plane**

- **Heartbeat**: Every 30 seconds (device status)
- **State Reports**: On every worktree operation (event-driven)
- **Offline Queue**: SQLite queue for operations when offline
- **Sync**: Drain queue when connection restored

## Data Models

### Control Plane (PostgreSQL)

```ruby
# Device
- id: uuid
- user_id: references users
- name: string (hostname)
- tailscale_ip: inet
- token: string (encrypted)
- agent_version: string
- status: enum (online, offline, error)
- last_seen_at: datetime
- metadata: jsonb (OS, architecture, etc)

# Worktree
- id: uuid
- device_id: references devices
- name: string (work_feature)
- branch: string
- url: string (Cloudflare tunnel)
- status: enum (starting, running, stopped, error)
- worktree_path: string
- metadata: jsonb (container names, volumes, etc)
- created_at: datetime
- updated_at: datetime

# FeatureSpec
- id: uuid
- user_id: references users
- name: string
- title: string
- content: text (markdown)
- status: enum (backlog, in_progress, completed)
- metadata: jsonb
```

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

## Security Model

### Device Registration

1. User logs into web dashboard
2. User generates 6-digit registration code (expires in 15 minutes)
3. User runs `worktree-agent init` on device and enters code
4. Agent exchanges code for long-lived device token
5. Device token stored in `~/.worktree/config.toml` (600 permissions)

```bash
# Web UI
Code: ABC123

# Device
$ worktree-agent init
Enter registration code: ABC123
✓ Device registered successfully
✓ Device ID: 550e8400-e29b-41d4-a716-446655440000
```

### Authentication

**Local**:
- Mac App/CLI: No auth required (local user permissions)
- Agent: Listens on localhost + Unix socket

**Remote**:
- Control Plane → Agent: Device token in gRPC metadata
- Web UI → Control Plane: Session-based auth (existing Rails auth)
- Tailscale network: Automatic encryption and ACLs

### Authorization

- Users can only manage their own devices
- Devices can only report state for themselves
- Control plane validates device ownership before accepting commands

## Offline Operation

### Offline-First Design

The agent is designed to work **completely offline**:

1. **Local commands execute immediately**
   - Mac App/CLI → Agent → Worktree Core
   - No network required

2. **State updates queued for sync**
   - Agent writes to SQLite queue
   - Periodically attempts to sync with control plane
   - Queue drains when connection restored

3. **Conflict resolution**
   - Control plane state is source of truth
   - Agent reports local state
   - Manual resolution via web UI if conflicts detected

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
    if control_plane.is_reachable() {
        queue.drain_all().await?;
    }
    tokio::time::sleep(Duration::from_secs(30)).await;
}
```

## Deployment

### Agent Installation (macOS)

```bash
# Homebrew
brew tap your-org/worktree
brew install worktree-agent

# Manual
curl -L https://github.com/branchbox-branchbox/releases/download/v1.0.0/worktree-agent-darwin-arm64.tar.gz | tar xz
sudo mv worktree-agent /usr/local/bin/

# Initialize
worktree-agent init

# Install as LaunchDaemon
sudo worktree-agent install
```

### Agent Configuration

`~/.worktree/config.toml`:
```toml
[agent]
device_id = "550e8400-e29b-41d4-a716-446655440000"
device_token = "long-secret-token"
device_name = "MacBook Pro"

[control_plane]
enabled = true
url = "https://worktree.example.com"

[tailscale]
auto_detect = true
ip = "100.64.0.1"

[local]
listen_addr = "127.0.0.1:50051"
unix_socket = "/var/run/worktree-agent.sock"
data_dir = "/Users/username/.worktree/data"

[logging]
level = "info"
file = "/Users/username/.worktree/agent.log"
```

### Control Plane Deployment

```bash
# Using Kamal (existing Agentify deployment)
bin/kamal deploy

# Or Heroku
git push heroku main

# Or Fly.io
fly deploy
```

## Migration Plan

### Phase 1: Core Library (Rust)

Migrate existing bash utilities to Rust:

1. ✅ `lib/core/naming.sh` → `core/src/naming.rs`
2. ✅ `lib/core/validation.sh` → `core/src/validation.rs`
3. ✅ `lib/core/git-operations.sh` → `core/src/git.rs`
4. ✅ `lib/adapters/` → `core/src/adapters/`
5. ✅ `lib/modules/` → `core/src/modules/`

### Phase 2: Local Agent

Build Rust daemon:

1. ✅ gRPC server setup
2. ✅ Unix socket server
3. ✅ Integrate core library
4. ✅ Local state storage (SQLite)
5. ✅ Command handlers

### Phase 3: CLI Tool

Build Rust CLI:

1. ✅ Argument parsing (clap)
2. ✅ Agent communication
3. ✅ Pretty output (indicatif, colored)
4. ✅ Interactive prompts (dialoguer)

### Phase 4: Control Plane

Extend Rails app (Agentify):

1. ✅ Device model and registration
2. ✅ Agent API endpoints
3. ✅ gRPC client for agent communication
4. ✅ Web UI for device management

### Phase 5: Mac App

Build SwiftUI app:

1. ✅ Local agent communication
2. ✅ Worktree list view
3. ✅ Start/stop/teardown actions
4. ✅ Settings and preferences
5. ✅ Optional: Control plane integration

## Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Core Library** | Rust | Fast, safe, embeddable, cross-platform |
| **Agent** | Rust + Tokio | Low resource, reliable, async I/O |
| **CLI** | Rust + Clap | Single binary, fast startup, great UX |
| **Mac App** | SwiftUI | Native macOS, best performance/UX |
| **Control Plane** | Rails 8 | Rapid development, great for business logic |
| **Web UI** | Hotwire (Turbo/Stimulus) | Real-time updates, minimal JS |
| **Communication** | gRPC (tonic) | Type-safe, bi-directional streaming |
| **Network** | Tailscale | Secure mesh VPN, NAT traversal |
| **Database** | PostgreSQL + SQLite | Postgres for control plane, SQLite for agent |
| **Queue** | Solid Queue (Rails) | Background jobs, agent command queue |

## Development Setup

### Prerequisites

- Rust 1.75+
- Ruby 3.3+
- PostgreSQL 16+
- Docker
- Tailscale account (optional for remote features)

### Local Development

```bash
# Clone repository
git clone https://github.com/branchbox-branchbox
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

# Run control plane
cd ../control-plane
bin/rails db:setup
bin/dev
```

## Future Enhancements

### Version 2.0

- [ ] Windows support (WSL2)
- [ ] Linux desktop app (Tauri)
- [ ] Team collaboration features
- [ ] Shared worktrees across devices
- [ ] Template library
- [ ] VS Code extension
- [ ] JetBrains IDE plugin
- [ ] Metrics and telemetry
- [ ] Cost optimization recommendations

### Enterprise Features

- [ ] SSO/SAML integration
- [ ] RBAC (role-based access control)
- [ ] Audit logging
- [ ] Compliance reports
- [ ] On-premise deployment
- [ ] High availability (HA) setup

## References

- [Git Worktree Documentation](https://git-scm.com/docs/git-worktree)
- [Tailscale Documentation](https://tailscale.com/kb/)
- [gRPC Rust (Tonic)](https://github.com/hyperium/tonic)
- [Tokio Async Runtime](https://tokio.rs/)
- [Rails 8 Documentation](https://guides.rubyonrails.org/)
