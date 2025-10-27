# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

BranchBox is a distributed development environment orchestrator that manages git worktrees and devcontainers. The project is migrating from bash scripts to a Rust-based implementation with a distributed architecture (local agents + control plane).

**Current state**: Milestone 0 complete - Core workflow orchestration for feature worktrees is implemented in Rust. The CLI supports `branchbox feature start/teardown/list` commands with full lifecycle management.

## Essential Commands

### Building & Testing

```bash
# Build entire workspace
cargo build

# Build with optimizations
cargo build --release

# Run CLI locally (without installing)
cargo run -p branchbox-cli -- feature list
cargo run -p branchbox-cli -- --help

# Run all tests
cargo test

# Run specific test
cargo test feature_commands

# Run tests with output visible
cargo test -- --nocapture

# Run with all features enabled
cargo test --all-features

# Run doctests
cargo test --doc
```

### Code Quality

```bash
# Format (must pass in CI)
cargo fmt --all -- --check

# Lint (must pass with -D warnings in CI)
cargo clippy --all-targets --all-features -- -D warnings

# Quick compile check (fast iteration)
cargo check

# Security audit
cargo audit

# Coverage report (CI enforces 90%)
cargo tarpaulin --out Html --all-features --workspace
```

### Development Workflow

```bash
# Watch mode (requires cargo-watch)
cargo watch -x test
cargo watch -x clippy

# Generate and view documentation
cargo doc --open
cargo doc --no-deps
```

## Architecture Deep Dive

### Repository Structure

The workspace has two active members defined in root `Cargo.toml`:

- **`core/`** - The `worktree-core` library containing all business logic
- **`cli/`** - The `branchbox-cli` binary that exports the `branchbox` command

Future planned members (currently commented out):
- **`agent/`** - Long-running daemon for distributed operation
- **`control-plane/`** (separate repo) - Rails app for multi-device coordination
- **`macos/`** (separate repo) - Native SwiftUI app

### Core Library (`core/src/`)

The `worktree-core` library is organized into these key subsystems:

#### 1. Workflows (`workflows/feature.rs`)

The `FeatureWorkflow` orchestrator manages the complete feature lifecycle:

- **Start workflow**: Creates git worktree → runs adapter setup → initializes modules → provisions env → registers state
- **Teardown workflow**: Runs module teardown → runs adapter cleanup → removes worktree → optionally deletes branch → updates state
- **List**: Queries the state store for tracked features

Key types:
- `StartRequest` / `TeardownRequest` - Input parameters
- `StartSummary` / `TeardownSummary` - Results with warnings/errors
- `FeatureStateStore` - JSON-based registry tracking worktree metadata

The workflow uses a `GitWorktree` wrapper and orchestrates adapters + modules.

#### 2. Adapters (`adapters/mod.rs`, `adapters/{rails,nodejs,generic}.rs`)

Stack-specific adapters auto-detect project type and handle:

- **Detection**: Return confidence 0-100 based on markers (Gemfile, package.json, etc.)
- **Service URL**: Provide the local URL for Cloudflare tunnel ingress
- **Secret copying**: Copy `.env`, `.env.local`, credentials to worktree
- **Database setup**: Stack-specific DB initialization (Rails migrations, etc.)
- **Cleanup**: Remove temp files, stop services

The `detect_adapter()` function tries all adapters and returns the highest-confidence match (Generic always returns 10 as fallback).

Adapters are trait objects (`Box<dyn Adapter>`), making the system pluggable.

#### 3. Modules (`modules/mod.rs`, `modules/{compose,database,tunnel,specs}.rs`)

Composable feature components that run during worktree lifecycle:

- **Compose** - Docker Compose project name isolation
- **Database** - Database-level isolation for Rails/Django projects
- **Tunnel** - Cloudflare tunnel provisioning (planned)
- **Specs** - Feature specification lifecycle (backlog → in-progress → completed)

Each module implements the `Module` trait:
- `detect()` - Should this module run for this project?
- `init()` - Initialize module config (mutates module state)
- `setup()` - Run during feature start (provision resources)
- `teardown()` - Run during feature teardown (cleanup resources)
- `validate()` - Validate configuration
- `dependencies()` - Declare dependency on other modules

The `detect_modules()` function returns a `ModulePlan` with:
- Dependency-sorted module handles (topological sort)
- Warnings for missing dependencies or circular deps

**Important**: Modules execute in dependency order. The specs module currently has no dependencies, but tunnel depends on compose in the future architecture.

#### 4. Naming (`naming.rs`)

Generates DNS-safe, dasherized feature names:

```rust
naming::generate_work_feature("OAuth Integration") // → "oauth-integration"
```

Used for branch names (`feature/oauth-integration`), subdomain prefixes, Docker Compose project names, etc. Must be lowercase alphanumeric with hyphens only.

#### 5. Git Operations (`git.rs`)

`GitWorktree` provides safe wrappers around git worktree operations:

- `create()` - Create new worktree + branch
- `remove()` - Remove worktree (with safety checks)
- `list()` - List existing worktrees
- `branch_exists()` - Check if branch exists

Uses `git2` crate for native git operations (no shell commands).

#### 6. Validation (`validation.rs`)

Pre-flight checks before running workflows:

- `validate_git_worktree()` - Ensure we're in a git repo, not bare, etc.
- `validate_feature_name()` - Check DNS safety
- `AppUrl` type - Parse and validate `APP_URL` from .env files

#### 7. State Management (`workflows/feature.rs` - `FeatureStateStore`)

JSON-based registry tracking active/removed features:

```rust
// Stored at {repo_root}/.branchbox/registry.json
{
  "features": [{
    "work_feature": "oauth-integration",
    "branch_name": "feature/oauth-integration",
    "worktree_path": "/path/to/oauth-integration",
    "feature_url": "oauth-integration.example.com",
    "status": "Active",
    "created_at": "...",
    "updated_at": "..."
  }]
}
```

Status enum: `Active` | `Removed`

### CLI (`cli/src/`)

The CLI exports the `branchbox` binary with these subcommands:

- `branchbox init` - Bootstrap devcontainer (meta capability)
- `branchbox detect` - Show detected stack and modules
- `branchbox name generate|validate` - Feature naming utilities
- `branchbox feature start|teardown|list` - Feature worktree lifecycle

The `feature` subcommand implementation lives in `cli/src/commands/feature.rs`:

- Parses args (clap)
- Builds request objects
- Calls `FeatureWorkflow` methods
- Pretty-prints summaries with warnings/errors

**Integration tests** live in `cli/tests/feature_commands.rs` and test the full E2E flow using a real git repo in a temp directory.

## Key Design Patterns

### Error Handling

Currently using `anyhow::Error` everywhere. The `core/src/error.rs` defines a custom `Error` enum but it's not consistently used yet.

**TODO** (from code review): Migrate to `thiserror`-based domain errors for better error reporting.

### Module Initialization Pattern

Modules have two-phase initialization:

1. **Detection** - Happens before worktree creation (read-only, no mutations)
2. **Init + Setup** - Happens after worktree creation (mutates state, provisions resources)

This allows modules to gather context during detection, then use that context during setup.

### Adapter vs Module

- **Adapter** = Stack-specific behavior (Rails vs Node.js vs Generic)
- **Module** = Optional cross-cutting features (tunnel, database, specs, compose)

Both use trait objects for polymorphism. Adapters are detected once per workflow. Modules are detected, then executed in dependency order.

### Offline-First (Planned)

The future agent architecture will queue state updates to a control plane but always execute operations locally first. The current implementation already tracks state locally in JSON, which will migrate to SQLite in the agent.

## Testing Strategy

### Test Organization

- **Unit tests**: Live in `#[cfg(test)]` modules alongside code
- **Integration tests**: Live in `{crate}/tests/` directories
- **Doc tests**: Embedded in doc comments with `/// # Examples` sections

### Running Subset of Tests

```bash
# Single test by name
cargo test test_generate_work_feature

# All tests in a module
cargo test naming::tests

# Integration tests only
cargo test --test feature_commands

# Specific integration test
cargo test --test feature_commands feature_start_list_teardown
```

### Test Fixtures

Integration tests use `tempfile::TempDir` for isolated test repos. The `init_test_repo()` helper creates a minimal git repo with a commit:

```rust
fn init_test_repo() -> TestRepo {
    // Creates temp_dir/repo with initialized git, .env file, etc.
}
```

### CI Requirements

GitHub Actions CI enforces:
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --all`
- 90% code coverage (Tarpaulin)

The workflow runs tests with Docker-in-Docker for testing Docker operations.

## Common Development Patterns

### Adding a New Adapter

1. Create `core/src/adapters/mystack.rs`
2. Implement the `Adapter` trait
3. Add detection logic (check for marker files)
4. Implement `copy_secrets()` for stack-specific config files
5. Register in `adapters/mod.rs::detect_adapter()`
6. Add tests in `#[cfg(test)]` module

### Adding a New Module

1. Create `core/src/modules/mymodule.rs`
2. Implement the `Module` trait
3. Define `dependencies()` if it depends on other modules
4. Register in `modules/mod.rs::all_modules()`
5. Add detection logic in `detect()`
6. Implement lifecycle hooks: `init()`, `setup()`, `teardown()`
7. Add tests verifying dependency ordering

### Extending the CLI

1. Add new subcommand enum variant in `cli/src/main.rs::Commands`
2. Create handler in `cli/src/commands/` if complex
3. Call `FeatureWorkflow` or other core library functions
4. Add integration test in `cli/tests/`
5. Pretty-print results with clear sections and bullet points

## Environment Variables & Configuration

### Development Environment

The devcontainer (`.devcontainer/`) provides:

- Rust stable toolchain + `cargo-watch`, `cargo-edit`, `cargo-expand`
- Node.js 20 + Codex/Claude Code CLIs
- Docker-in-Docker (privileged mode required)
- Persistent `.codex/` config and `.cargo/` cache

**Important**: The container runs privileged for DinD. Be aware of security implications.

### Test Environment

Tests should set `BRANCHBOX_SKIP_HOST_VALIDATION=1` to bypass host environment checks (see `cli/tests/feature_commands.rs` for example).

### Feature Environment

During `feature start`, the workflow:

1. Copies `.env` from repo root to worktree (if exists)
2. Injects `APP_URL` and `COMPOSE_PROJECT_NAME` into worktree `.env`
3. Adapters may copy additional secrets (`.env.local`, `master.key`, etc.)

## Migration from Bash Scripts

The repo contains legacy bash scripts in `lib/` and `bin/feature-*`. These are being replaced by Rust:

- ✅ `lib/core/naming.sh` → `core/src/naming.rs`
- ✅ `lib/adapters/` → `core/src/adapters/`
- ✅ `lib/modules/` → `core/src/modules/`
- ✅ `bin/feature-start` → `branchbox feature start`
- ✅ `bin/feature-teardown` → `branchbox feature teardown`

When migrating bash code to Rust, maintain the same behavior but leverage Rust's type safety and error handling.

## Specs Module Behavior

The specs module manages feature specifications:

1. **Start workflow**: Promotes spec from `docs/features/backlog/{name}.md` → `docs/features/in-progress/{name}.md` (or creates stub if missing)
2. **Teardown workflow**: If `--complete-spec` flag is set, moves spec from `in-progress/` → `completed/`

Spec stubs are scaffolded with front matter:

```markdown
---
title: Feature Title
status: in-progress
created_at: 2025-10-24T...
---

## Overview

## Requirements

## Implementation Notes
```

## Commit Message Convention

Follow Conventional Commits:

```
<type>(<scope>): <description>

feat(modules): add tunnel dependency on compose
fix(cli): validate feature name before creating worktree
docs(architecture): document module dependency system
test(adapters): cover Rails secret copying
```

Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `chore`

Scopes: `modules`, `adapters`, `workflows`, `cli`, `core`, `devcontainer`, `ci`

See recent commit history for examples:
- `feat(workflows): migrate tunnel and adapter orchestration to rust`
- `feat(specs): support completing specs during teardown`
- `feat(modules): auto-promote backlog specs`

## Known Issues & TODOs

From the recent code review (PR #2):

1. **Repository URL is incorrect** - `Cargo.toml` has `branchbox-branchbox` instead of actual org/repo
2. **Placeholder author metadata** - `Cargo.toml` has `Your Name <you@example.com>`
3. **Generic error types** - Should migrate from `anyhow::Error` to `thiserror`-based domain errors
4. **Missing input validation** - CLI should validate that either `--name` or `--title` is provided
5. **Race conditions** - Registry check + worktree creation isn't atomic
6. **Hardcoded config** - Docker network names, port ranges, spec templates should be configurable
7. **Missing unit tests** - Registry operations, workflow components, module implementations need dedicated unit tests

## Future Architecture Notes

The long-term vision includes:

- **Agent** - Rust daemon running on each device (gRPC server)
- **Control Plane** - Rails app for multi-device orchestration (hosted)
- **Mac App** - SwiftUI native app talking to local agent
- **Tailscale** - Secure mesh VPN for agent ↔ control plane communication
- **Offline-first** - SQLite queue for operations when disconnected

See `docs/ARCHITECTURE.md` for the full distributed system design.

For now, focus on the local-first workflow: CLI → FeatureWorkflow → Git + Adapters + Modules.
