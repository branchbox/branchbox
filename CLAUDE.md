# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENTS.md

## Architecture Deep Dive

This section provides detailed context for understanding the codebase structure and generating correct code. For basic commands, coding style, and project overview, consult AGENTS.md above.

### Core Library Architecture (`core/src/`)

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

### CLI Architecture (`cli/src/`)

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

## Migration from Bash Scripts

The repo contains legacy bash scripts in `lib/` and `bin/feature-*`. These are being replaced by Rust:

- ✅ `lib/core/naming.sh` → `core/src/naming.rs`
- ✅ `lib/adapters/` → `core/src/adapters/`
- ✅ `lib/modules/` → `core/src/modules/`
- ✅ `bin/feature-start` → `branchbox feature start`
- ✅ `bin/feature-teardown` → `branchbox feature teardown`

When migrating bash code to Rust, maintain the same behavior but leverage Rust's type safety and error handling.
