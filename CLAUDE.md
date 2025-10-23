# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**BranchBox** (formerly worktree-manager) is a Rust workspace that manages git worktrees and devcontainer environments. The project consists of:
- `core/` - Reusable Rust library (`worktree-core`)
- `cli/` - Command-line interface (`branchbox-cli`)

The core library is designed for reuse across future components (agent daemon, macOS app, control plane).

## Essential Commands

### Build & Development
```bash
# Build entire workspace
cargo build

# Build with optimizations
cargo build --release

# Build specific package
cargo build --package worktree-core
cargo build --package branchbox-cli

# Quick check without building
cargo check
```

### Testing
```bash
# Run all tests (unit, integration, doc tests)
cargo test

# Run tests with output visible
cargo test -- --nocapture

# Run specific test by name
cargo test naming
cargo test test_generate_work_feature

# Run doc tests only
cargo test --doc

# Watch mode (if cargo-watch installed)
cargo watch -x test
```

### Code Quality
```bash
# Format code (required before commits)
cargo fmt

# Lint with Clippy (fix warnings before committing)
cargo clippy

# Security audit
cargo audit
```

### Documentation
```bash
# Generate and open documentation
cargo doc --open

# Generate without opening
cargo doc --no-deps
```

### CLI Tool Usage
```bash
# Run the CLI directly (from workspace root)
cargo run --package branchbox-cli -- <command>

# Or build and use the binary
cargo build --release
./target/release/branchbox init
./target/release/branchbox detect
./target/release/branchbox name generate "OAuth Integration"
```

## Architecture Overview

### Workspace Structure
The project uses Cargo workspace with two members:
- **Core Library** (`worktree-core`): Shared business logic
- **CLI Tool** (`branchbox-cli`): User-facing command interface

This separation enables future reuse of core library in agent daemon, macOS app, and control plane.

### Core Library Modules (7 Primary Components)

**1. naming.rs** - Feature name generation
- `generate_work_feature()`: Converts titles to DNS-safe names ("OAuth Integration" → "oauth-integration")
- `validate_work_feature()`: Validates naming rules (max 4 words, no filler words)
- Naming convention critical for consistent worktree/tunnel/container naming across system

**2. validation.rs** - Environment validation
- `validate_git_worktree()`: Checks git repository state
- `validate_host_environment()`: Detects host vs. container execution
- `parse_env_file()`: Reads .env files
- `AppUrl`: Parses service URLs with scheme/host/port
- `CloudflareCredentials`: Reads Cloudflare API tokens

**3. git.rs** - Git worktree operations
- Create/remove/list worktrees programmatically
- Branch management (create, delete, exists checks)
- Uses `git2` crate for programmatic access
- Parses porcelain output for reliable parsing

**4. adapters/** - Stack-specific detection and configuration
- **Trait-based system**: `StackAdapter` trait defines interface
- **Detection logic**: Confidence-based auto-detection (0.0-1.0)
- **Rails adapter** (`adapters/rails.rs`): Detects Gemfile, handles master.key/credentials
- **Node.js adapter** (`adapters/nodejs.rs`): Detects package.json, copies .env/.npmrc
- **Generic adapter** (`adapters/generic.rs`): Fallback for unknown stacks
- **Key insight**: Adapters handle per-stack secrets, env vars, and cleanup

**5. modules/** - Composable feature components
- **Trait-based plugins**: `Module` trait with lifecycle methods (setup/teardown/cleanup)
- **compose.rs**: Docker Compose network isolation per feature
- **database.rs**: Database volume isolation (PostgreSQL/MySQL/MongoDB)
- **tunnel.rs**: Cloudflare tunnel configuration and token management
- **specs.rs**: Feature specification tracking with lifecycle states
- **Detection**: Each module can detect if it applies to current project

**6. bootstrap/** - Self-bootstrapping devcontainer system
- **Meta-capability**: Tool generates devcontainer configs for any project, including itself
- Embedded templates for Rails, Node.js, Rust, Generic stacks (compiled into binary)
- Generates: devcontainer.json, compose.yaml, Dockerfile, .env.sample
- Stack detection uses same adapter system as runtime features

**7. error.rs** - Structured error handling
- Custom `Error` enum with `thiserror` for context
- `Result<T>` type alias used throughout codebase
- Errors preserve context through call chain

### CLI Architecture

Uses `clap` with derive macros and enum-based subcommands:
```rust
Commands {
    Init,           // Initialize devcontainer
    Detect,         // Show detected configuration
    Name(NameCommands),  // Grouped: generate, validate
}
```

Grouped subcommands allow logical organization (e.g., all naming operations under `name`).

### Key Design Patterns

**Trait-Based Plugins**: Both adapters and modules use traits for extensibility
- Add new stack: Implement `StackAdapter` trait
- Add new module: Implement `Module` trait

**Detection-Based Configuration**: System auto-detects rather than requiring config files
- Stack detection: Scans for Gemfile, package.json, Cargo.toml, etc.
- Module detection: Looks for docker-compose.yml, database configs, etc.
- Confidence scoring: Most confident adapter wins

**Lifecycle Management**: Modules follow setup → active → teardown → cleanup lifecycle
- Setup: Initial configuration
- Teardown: Graceful shutdown
- Cleanup: Remove all traces

**Error-as-Values**: Uses `Result<T, Error>` everywhere, no panics in library code

### Feature Lifecycle Flow

Understanding how a feature progresses through the system:

1. **Name Generation**: User title → validated feature name
2. **Stack Detection**: Analyze project files → select adapter (Rails/Node.js/Generic)
3. **Module Selection**: Scan for compose.yaml, databases, etc. → enable relevant modules
4. **Adapter Configuration**: Copy stack-specific secrets, set env vars
5. **Git Worktree Creation**: Create isolated worktree for feature branch
6. **Bootstrap (optional)**: Generate devcontainer for feature worktree
7. **Specs Tracking**: Record feature metadata with lifecycle state

This multi-stage flow coordinates across naming, validation, adapters, modules, git, and bootstrap components.

## Critical Implementation Details

### Naming Rules
Feature names must be:
- DNS-safe (lowercase alphanumeric + hyphens)
- Max 4 significant words (filler words removed)
- Valid for: branch names, container names, tunnel hostnames, database names
- Validated by `validate_work_feature()` before any operations

### Stack Adapter Selection
Adapters return confidence scores (0.0-1.0):
- Rails: 1.0 if Gemfile exists
- Node.js: 1.0 if package.json exists
- Generic: 0.5 always (fallback)

Highest confidence wins. System never fails detection (Generic always available).

### Module Independence
Modules are independent and composable:
- Compose module works with or without database module
- Tunnel module works with or without compose module
- Each module's setup/teardown is isolated
- Failures in one module don't affect others

### Bootstrap Templates
Templates are embedded in binary via `include_str!()`:
- Located in `core/src/bootstrap/templates/`
- Compiled into library (no runtime file dependencies)
- One template set per stack (rails/, nodejs/, rust/, generic/)

## Testing Approach

### Test Coverage Expectations
- Modules have 85-100% test coverage
- CI enforces 90% coverage threshold with cargo-tarpaulin
- All public APIs must have unit tests
- Complex logic requires parameterized tests using `rstest`

### Test Organization
- Unit tests: In-module `#[cfg(test)]` blocks
- Integration tests: Separate `tests/` directory
- Doc tests: Examples in doc comments (verified on CI)

### Testing Git Operations
Use `tempfile` crate for temporary git repositories:
```rust
use tempfile::TempDir;
let temp_dir = TempDir::new()?;
// Create test git repo in temp_dir
```

## CI/CD Pipeline

GitHub Actions workflow (`.github/workflows/ci.yml`) runs on every PR:

1. **Quality**: `cargo fmt --check`, `cargo clippy`, `cargo audit`
2. **Tests**: Unit + integration tests on Ubuntu + macOS
3. **Coverage**: cargo-tarpaulin with 90% threshold
4. **Build**: Multi-platform (Linux, macOS, Windows) on stable + beta Rust
5. **Docs**: Check for documentation warnings

All checks must pass before merge.

## Development Environment

### Devcontainer Setup (Recommended)
Project includes complete devcontainer configuration:
- Base image: Rust development container
- Features: GitHub CLI, Docker-outside-of-Docker
- Extensions: rust-analyzer, TOML support, crates, LLDB debugger
- Port 50051: Reserved for future gRPC server

This setup was generated by the bootstrap system itself (meta-capability).

### Key Dependencies
- `git2`: Programmatic git operations (not git CLI)
- `clap`: CLI argument parsing with derive macros
- `serde`/`serde_json`/`serde_yaml`: Serialization
- `thiserror`/`anyhow`: Error handling
- `rstest`: Parameterized testing
- `tracing`/`tracing-subscriber`: Structured logging

## Implementation Status

**Phase 2 Complete** (Current):
- Core library: 7 modules fully implemented
- CLI tool: Complete with init, detect, name commands
- Test coverage: 85-100% across modules
- Documentation: Comprehensive doc comments

**Future Phases** (Planned):
- Phase 3: Local agent daemon (gRPC server, SQLite offline queue)
- Phase 4: Enhanced CLI with agent communication
- Phase 5: Control plane (Rails backend, PostgreSQL, web dashboard)
- Phase 6: macOS app (SwiftUI native interface)

See `docs/IMPLEMENTATION_STATUS.md` for detailed phase breakdown.

## Common Development Workflows

### Adding a New Stack Adapter
1. Create `core/src/adapters/mystack.rs`
2. Implement `StackAdapter` trait
3. Add detection logic (return confidence 0.0-1.0)
4. Implement `configure()` for secrets/env setup
5. Add to `adapters/mod.rs` detection list
6. Write tests in module
7. Add template in `bootstrap/templates/mystack/`

### Adding a New Module
1. Create `core/src/modules/mymodule.rs`
2. Implement `Module` trait with lifecycle methods
3. Add detection logic (`is_applicable()`)
4. Implement setup/teardown/cleanup
5. Add to `modules/mod.rs`
6. Write unit tests with tempfile
7. Update CLI to show module in `detect` command

### Debugging Git Operations
Enable git2 tracing:
```bash
GIT_TRACE=1 cargo test -- --nocapture
```

Or use `tracing` instrumentation:
```bash
RUST_LOG=worktree_core=debug cargo test -- --nocapture
```

## Documentation Philosophy

- Doc comments explain **why**, code shows **what**
- Public APIs must have `///` doc comments with examples
- Use `# Examples` sections that double as doc tests
- Reference related functions with \[`function_name`\]
- Document panics, errors, and edge cases explicitly
