---
sidebar_position: 2
---

# Development Guide

This guide covers the development workflow, building, testing, and contributing to BranchBox.

## Development Environment Setup

### Using Devcontainer (Recommended)

This project includes a complete devcontainer setup that provides a consistent development environment.

**Prerequisites:**
- Docker Desktop or Docker Engine
- VS Code with Remote - Containers extension, or
- Cursor (has built-in devcontainer support)

**Steps:**

1. Clone the repository:
   ```bash
   git clone https://github.com/branchbox/branchbox.git
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

**Devcontainer sync strategy:**
- By default BranchBox copies the `.devcontainer/` directory into each feature worktree.
- Override behaviour per command with `BRANCHBOX_DEVCONTAINER_STRATEGY=copy|symlink branchbox feature start`.
- Persist a different default by setting `BRANCHBOX_DEVCONTAINER_STRATEGY` in your `.env` (template comment provided in generated `env.sample` files).
- Refresh existing worktrees whenever the main `.devcontainer/` changes: `branchbox devcontainer sync [--dry-run] [--strategy copy|symlink]`.

### Module Architecture Notes

- Devcontainer provisioning is handled by `DevcontainerModule`, one of the composable modules executed during feature start.
- The module copies or symlinks files from the main `.devcontainer/` directory, excluding `.env`, and prunes stale artifacts during sync.
- `branchbox devcontainer sync` reuses the module to propagate updates to all active feature worktrees, respecting the same strategy logic.

### Devcontainer Smoke Test Playgrounds

- Run `./scripts/setup-sample-workspaces.sh` to copy tracked templates from `test/workspaces/templates/` into the git-ignored `test/workspaces/local/`. Each template declares its stack via `template.json`, so the script prints the correct `branchbox init --stack <stack>` command.
- The repository currently ships `rust-cli` and `node-api` samples; add more templates as needed for other stacks.
- Example flow:
  ```bash
  ./scripts/setup-sample-workspaces.sh node-api
  cd test/workspaces/local/node-api
  BRANCHBOX_SKIP_HOST_VALIDATION=1 branchbox init --stack nodejs
  branchbox feature start devcontainer-smoke
  ```
- Open both the main sample and generated feature worktree in VS Code/Cursor to confirm devcontainer propagation, shared credential mounts, and module telemetry.
- After testing, clean up with `branchbox feature teardown devcontainer-smoke --complete-spec` and simply delete `test/workspaces/local/` if you want a fresh slate.

## Feature Start UX & Fast Path Modes

- `branchbox feature start` ships with a muscle-memory alias: `branchbox feature new`. Both commands accept the same flags.
- Minimal mode (`--minimal`, with hidden alias `--fast`) skips the devcontainer, compose, and specs modules for lightweight edits—no preview flag required.
- `--default-prompt` drops in the built-in BranchBox seed for minimal starts so agents have immediate context. Use `--prompt "seed text"` when you want to provide your own (still capped at 2,000 characters and annotated with the prompt-bridge flag).
- `--json` mirrors the entire summary (checklist, module table, warnings, prompt seed, timestamps) as structured JSON; pair it with `--no-summary` if you only want machine-readable output.

```bash
branchbox feature new backlog-quick-fix \
  --minimal \
  --default-prompt \
  --json
```

- Set `BRANCHBOX_DEFAULT_AGENT_CMD` (optionally with `BRANCHBOX_DEFAULT_AGENT_NAME`) to auto-launch your preferred coding agent after the devcontainer module finishes. Minimal starts record that state in the checklist so you know whether it will run immediately or is waiting for `branchbox devcontainer sync`.

### Start Summary Cheat Sheet

- Text mode now prints `🚀 Feature workspace ready (<mode>)` followed by a checklist table (`Step`, `Result`, `Details`) that calls out the worktree, branch, adapter, URLs, `.env`, prompt seed, module health, skipped modules, and default agent plan.
- A dedicated module table still lists every module with status (`success`, `skipped`, `failed`), duration, notes, and a forced marker for policy-required runs.
- Separate “Skipped modules” and “Warnings” sections provide reasoning and remediation; minimal runs end with a reminder to finish provisioning via `branchbox devcontainer sync`.

### Listing Features

- `branchbox feature list` now surfaces `Mode`, `Prompt`, and `Modules` columns so you can gauge module health at a glance (e.g., `3 ok / 1 skip`).
- `branchbox feature list --json` returns the same data structure captured in `.branchbox/registry.json`, including `start_mode`, `prompt_seed`, and each module outcome, so dashboards and agents stay in sync without re-running `feature start`.

### Local Development (without devcontainer)

**Prerequisites:**
- Rust 1.75+ ([Install Rust](https://rustup.rs/))
- Git
- Docker (optional, for testing Docker operations)

**Setup:**

```bash
# Clone repository
git clone https://github.com/branchbox/branchbox.git
cd branchbox

# Build core library
cd core
cargo build

# Run tests
cargo test

# Install development tools
cargo install cargo-watch cargo-edit cargo-expand
```

## Building

```bash
# Build all workspace members
cargo build

# Build with release optimizations
cargo build --release

# Build specific package
cargo build --package worktree-core
```

## Testing

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

### Install Script Tests

The install script has automated tests. See [test/README.md](https://github.com/branchbox/branchbox/tree/main/test/README.md) for details.

```bash
# Run tests
bats test/install.bats         # All tests
shellcheck install.sh          # Linting

# Test on specific distro
docker run -it --rm -v $(pwd)/install.sh:/install.sh:ro ubuntu:22.04 bash /install.sh
```

**Coverage**: Static analysis (shellcheck) + automated tests (bats) + CI (GitHub Actions)

## Code Quality

```bash
# Format code
cargo fmt

# Run Clippy (linter)
cargo clippy

# Check without building
cargo check
```

## Documentation

The documentation website is built using Docusaurus. The devcontainer ships with Node.js 20.

```bash
# Navigate to the docs directory
cd docs

# Install dependencies
npm install

# Start the local development server
npm start

# Build the static site for production
npm run build

# Generate and open Rust API docs
cargo doc --open

# Generate API docs without opening
cargo doc --no-deps
```

When updating CLI commands, manually regenerate the CLI reference by capturing `branchbox --help` output and updating `docs/docs/reference/cli.md`.

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
├── cli/                    # CLI tool
├── macos/                  # Mac app (planned)
├── docs/                   # Documentation
├── .env.sample             # Environment variable template
├── .gitignore
├── Cargo.toml              # Workspace configuration
└── README.md
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test --all` (Rust) or `./test/run-tests.sh` (install script)
5. Submit a pull request

### Commit Guidelines

Use conventional commit format:
- `feat(modules): add docker compose planner`
- `fix(cli): correct feature name validation`
- `refactor(adapters): simplify rails detection`
- `docs(readme): update installation guide`
- `test(core): add naming edge cases`

### Code Review Checklist

Before submitting a PR:
- [ ] Run `cargo fmt --all -- --check`
- [ ] Run `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Run `cargo test --all-features`
- [ ] Run `cargo doc --no-deps` (check for doc warnings)
- [ ] Update relevant documentation
- [ ] Add tests for new functionality
- [ ] Rebase on latest `main`

## Architecture Documentation

For detailed architecture information, see:
- [Architecture Overview](/architecture) - System design and components
- [PROTOCOL.md](https://github.com/branchbox/branchbox/blob/main/docs/PROTOCOL.md) - Communication protocols (gRPC, REST)
- [CLAUDE.md](https://github.com/branchbox/branchbox/blob/main/CLAUDE.md) - AI agent development guidelines
- [AGENTS.md](https://github.com/branchbox/branchbox/blob/main/AGENTS.md) - Repository guidelines and patterns

## Meta Feature: Bootstrap System

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

## Development Status

**Current Progress:**
- [x] Architecture design
- [x] Protocol specification
- [x] Core library implementation (Milestone 0)
- [x] CLI implementation (Milestone 0)
- [ ] Agent implementation (Milestone 1)
- [ ] Mac app implementation (Milestone 2)
- [ ] Control plane implementation (Milestone 3)

## Related Projects

- [Git Worktree](https://git-scm.com/docs/git-worktree) - Official git worktree documentation
- [Tailscale](https://tailscale.com/) - Secure mesh VPN for device connectivity
- [Cloudflare Tunnels](https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/) - Expose local services securely
