# BranchBox
### *Parallel feature workspaces for humans and AI agents.*

[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
[![CI](https://github.com/branchbox/branchbox/workflows/CI/badge.svg)](https://github.com/branchbox/branchbox/actions)
[![License](https://img.shields.io/github/license/branchbox/branchbox)](LICENSE)

**[Website](https://branchbox.dev)** · **[Documentation](https://branchbox.dev/docs)** · **[GitHub](https://github.com/branchbox/branchbox)**

BranchBox is an open-source engine for **parallel feature worktrees and configurable development environments**, designed for engineers working with AI coding agents or juggling multiple features at once.

Every feature gets a dedicated Git worktree and branch. Configured setup modules and runtime providers can also supply:

- Synced devcontainer configuration
- A separate Compose project identity
- Feature-specific database naming
- Runtime port mappings
- A copied and customized environment file
- Optional Cloudflare tunnels
- Configured credential mounts or scoped runtime credentials

Start the container and application when needed, and run your project's database setup explicitly. A successful feature setup reports completed configuration steps; it does not prove that the application or database is running.

If you’ve ever run multiple features or agents in parallel and felt things colliding, leaking, or breaking — BranchBox solves that. It makes parallel development a first-class workflow.

---

## Why BranchBox exists

Modern engineering is shifting toward **agentic, parallel development**:

- Humans work on one feature  
- AI agents explore another  
- LLMs run migrations, refactors, codegen, and experiments  
- Several ideas progress at once  

The problem: none of our tools were built for this.

- Git branches collide  
- Docker networks & ports collide  
- Devcontainers drift  
- Databases clash  
- Shared credentials leak across contexts  
- And it’s too easy for one environment to break another

BranchBox adds the missing layer:

> **Separate feature code in Git worktrees, configure its environment, and choose a runtime boundary.**

Start one feature or ten.  
Work alone or with multiple agents.  
Shared mounts, external services, and fixed host ports follow your project configuration.

---

## What BranchBox gives you

### 1. Configurable isolation
Each feature has its own branch and directory. Configured container setups also use a feature-specific:

- Compose project  
- Docker network  
- Ports  
- `.env` file  
- Database name when the module can add it to `.env`
- Devcontainer  

Compose identities separate owned resources. Fixed host ports and external databases still need project configuration that keeps them separate.

### 2. Real development environments  
Not an agent-only sandbox — a **full stack** environment:

- Rails, Node, Python, or generic  
- Containers, Docker Compose, databases  
- Shared credentials mounts  
- VS Code + Cursor devcontainers  
- Built-in adapter system for detecting stacks  

Use the stack adapters or generic setup to configure your project's workspace.

### 3. Parallel workflows that feel effortless  
Keep multiple feature workspaces available at once. Start their environments as needed, and let an agent work in one feature while you code in another branch.

The mental overhead stays low, and the environments stay clean.

### 4. Agent-ready by design  
An optional source-built BranchBox agent daemon tracks:

- Feature events  
- Heartbeats  
- Stack metadata  
- Control-plane connectivity  

The native macOS app and scripts drive the same `branchbox` CLI through its stable `--json` contract (one JSON document per command, an error envelope with stable codes, and `branchbox version --json` capabilities), so the app, agents and Terminal always agree about what is on disk.

Agents can safely:

- Start new worktrees  
- Run minimal-mode spikes  
- Apply prompt seeds  
- Tear environments down  
- Reuse shared credentials  

Everything stays observable and isolated.

### 5. A workflow built from real-world usage  
BranchBox exists because of daily engineering pain:

- Running multiple projects in parallel  
- Hitting laptop limits  
- Letting agents work independently  
- Needing guaranteed isolation  
- Avoiding devcontainer drift  
- Avoiding "I broke main" moments  

It’s built to support how modern development actually works — especially when humans and LLM agents collaborate.


---

## Installation

BranchBox is available via **Homebrew** (recommended) or direct installation.

### **macOS (Homebrew)**
```bash
brew install branchbox/tap/branchbox
```

### **Linux/macOS (installer script)**
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
```

Then open a new terminal or run:
```bash
hash -r
```

### **Mac app (preview)**
BranchBox for Mac is a native front end for the CLI (macOS 26+, `branchbox` 0.13.4+). Add projects,
start features, inspect environments and operation logs, open your tools, and review changes before
teardown. Container controls depend on Docker and the Dev Container CLI; some project settings
require capabilities provided by newer CLIs.

Read the **[Mac app user guide](https://branchbox.dev/docs/guides/mac-app)** for the workflow and
runtime limits. Preview bundles are ad hoc signed and not notarized: obtain a
`BranchBox-macOS-<sha>` artifact from a successful
[macOS App CI run](https://github.com/branchbox/branchbox/actions/workflows/macos-app.yml), or build
from source. See [`macos/README.md`](macos/README.md) for installation and development.

---

## Quick Start

```bash
# Initialize once (creates registry, checks environment)
branchbox init

# Start a fully isolated feature workspace
branchbox feature start "Add OAuth Integration"

# Work inside the new isolated environment
cd ../oauth-integration/
```

On first run in an interactive shell, `branchbox init` may prompt for:
- moving out of temporary directories,
- confirming repository reorganization,
- optional Cloudflare tunnel setup (prefix/zone/credentials).

Use `branchbox init -y` for non-interactive defaults.

If `init` generates `.devcontainer/` from BranchBox templates, it also includes optional 1Password-backed git bootstrap hooks (`init-host.sh` + `setup-git.sh`). Those hooks are not auto-injected into pre-existing custom devcontainer configs.

The feature now has its own worktree and branch. Enabled modules can also configure:

- Feature-specific database naming
- Its own Docker network  
- Its own ports  
- Devcontainer configuration
- A customized environment file

Start the devcontainer separately for container-runtime features, then run your application's setup and database commands. Inspect the feature's module results and environment state rather than treating registry status as an application health check.

In current CLI source builds, `branchbox feature teardown <name>` removes devcontainers identified
by the worktree's exact workspace label, including standalone image/Dockerfile containers. Compose
cleanup resolves the actual project created for that worktree and removes its owned containers,
network, and volumes. Standalone volumes and custom networks are not removed by the container check.
Review the runtime cleanup result: failed or unverified cleanup of a possibly provisioned environment
keeps the worktree for retry unless removal is forced. Compose project identity is retained in that
workspace before cleanup, allowing retries after containers are gone. Missing or changed Compose
configuration can require restoration or manual inspection. Older CLI builds can leave standalone
devcontainers behind; stop those environments and check Docker separately before removing the workspace.

`branchbox devcontainer down` keeps volumes by default. `--volumes` deletes attached anonymous
standalone volumes or owned Compose volumes; named/shared standalone volumes are not inferred.

If the worktree has reported uncommitted changes, or the
branch has commits that are not merged, it refuses before removing anything and tells you which
flag to use: `--discard-changes` to drop the changes, `--keep-branch` or `--force-delete-branch`
for the branch. Preview any teardown with `--dry-run`. The current plan does not list Git-ignored
files, which are deleted with the worktree. Copy any local data you need first; keeping the branch
preserves commits, not ignored files or Compose volumes.

Prefer a disposable sample project?

```bash
./scripts/setup-sample-workspaces.sh
branchbox init
branchbox feature start "Demo Feature"
```

### Choose the workspace isolation boundary

BranchBox keeps the repository-defined devcontainer separate from the runtime that contains it.
Existing projects continue to use the local, account-free `container` provider by default:

```bash
# Existing behavior (default)
branchbox feature start "Add OAuth" --runtime container

# Experimental Docker Sandboxes microVM boundary
branchbox feature start "Add OAuth" --runtime sbx

# Account-free Firecracker boundary on x86_64 Linux/KVM hosts
branchbox feature start "Add OAuth" --runtime local-vm

# Run a coding agent inside the active feature's recorded runtime
branchbox feature exec add-oauth -- codex
```

The optional `sbx` provider requires an installed and authenticated Docker Sandboxes CLI. SBX
authentication affects only explicitly selected SBX workspaces; normal BranchBox workflows do not
require a Docker account. BranchBox filters rendered Compose configuration and environment values
from SBX startup errors; inspect detailed failures inside the sandbox instead of copying expanded
configuration into shared logs. When Compose requires `.devcontainer/.cloudflared.env`, BranchBox
materializes it before sandbox creation; missing Cloudflare credentials fail that preflight without
paying the sandbox/devcontainer build cost. SBX exec reconciles a stopped devcontainer and restores
its login-shell toolchain environment. Optional incompatible sidecars can be excluded with
`runtime.sbx.run_services`; required Compose dependencies still start. Failed sandboxes can be kept
with `--keep-runtime-on-failure` and retried with `--reuse-runtime`. If an older BranchBox version may have logged a credential during
a failed SBX startup, rotate that credential with its provider.

The `local-vm` provider directly creates a fresh jailed Firecracker VM on x86_64 Linux/KVM hosts,
then runs Docker, devcontainers, Compose dependencies, and coding tools inside the guest. It uses
digest-verified kernel/rootfs artifacts, copies back workspace changes after commands, allocates
collision-safe TAP networks and host ports, blocks guest-initiated access to host/private/metadata
networks, and provides a digest-bound virtio-vsock channel only to trusted outer-guest supervisors
for bounded guest-to-host data transfer. Coding devcontainers receive neither `/dev/vsock` nor a
Docker control socket. Teardown deletes the VM, TAP, proxies, key, and writable rootfs. It is
account-free and never mounts the host Docker socket or persistent human credential directories.
See the local-vm setup and image build instructions in
[How It Works](https://branchbox.dev/docs/how-it-works#account-free-firecracker-local-vm).
Current source builds also support signed workspace topology for the managed `in-guest` provider:
a version-3 assignment can pin the container workspace path and reviewed connector omissions.
`branchbox runtime-capabilities` reports whether a staged binary implements the contract. See the
[managed-runtime manifest](https://branchbox.dev/docs/internals/managed-runtime-manifest-v2#signed-workspace-topology-in-version-3)
for assignment fields and refusal conditions.

See [How It Works](https://branchbox.dev/docs/how-it-works) for runtime topology,
configuration, port publication, and lifecycle details.

---

## Devcontainer Workflow

BranchBox features work seamlessly with VS Code/Cursor devcontainers, giving each feature its own isolated Docker environment while sharing tool credentials.

### Pre-built Images for Instant Startup

BranchBox provides official pre-built devcontainer images for all supported stacks, published to GitHub Container Registry:

| Stack | Image |
|-------|-------|
| Rust | `ghcr.io/branchbox/branchbox/devcontainer-rust:latest` |
| Rails | `ghcr.io/branchbox/branchbox/devcontainer-rails:latest` |
| Node.js | `ghcr.io/branchbox/branchbox/devcontainer-nodejs:latest` |
| Generic | `ghcr.io/branchbox/branchbox/devcontainer-generic:latest` |

When you run `branchbox init`, the generated `compose.yaml` references these pre-built images by default. Containers start in seconds instead of minutes—no local build required.

**How it works:**
- First run: Docker pulls the pre-built image from GHCR
- Subsequent runs: Uses cached image instantly
- Fallback: If pull fails, automatically builds from local Dockerfile

**Override options** (in `.env` or environment):
```bash
# Use a custom image
DEVCONTAINER_IMAGE=my-registry.io/my-image:tag

# Control pull behavior
DEVCONTAINER_PULL_POLICY=missing  # (default) Pull if not cached
DEVCONTAINER_PULL_POLICY=always   # Always pull latest
DEVCONTAINER_PULL_POLICY=build    # Always build locally
```

### Opening a Feature in a Container

When you start a new feature, BranchBox automatically copies the `.devcontainer/` configuration from your main repository:

```bash
# In main repo
branchbox feature start "Add OAuth"

# Navigate to new worktree
cd ../myapp-oauth/

# Open in VS Code/Cursor
code .
```

**VS Code/Cursor will prompt**: "Reopen in Container?"

Click **"Reopen in Container"** and your feature will run in an isolated Docker environment with:
- ✅ Separate Docker network (no port conflicts)
- ✅ Isolated database (for Rails/Node.js projects)
- ✅ Same development environment as main repo
- ✅ Shared tool credentials (see below)

### Shared Tool Credentials

All feature worktrees share authentication for common development tools, so you only need to log in once:

**Supported tools:**
- **GitHub CLI** (`gh`) - Credentials stored in `~/.config/gh/`
- **Claude Code** (`claude`) - Session stored in `~/.claude/`
- **Codex** (`codex`) - Config stored in `~/.codex/`
- **Cloudflared** (`cloudflared`) - Credentials in `~/.cloudflared/`

**How it works:**

1. **First time** - Authenticate in your main worktree:
   ```bash
   cd ~/projects/myapp  # main worktree
   code .  # Reopen in Container
   gh auth login  # Authenticate once
   claude login   # Authenticate once
   ```

2. **All features inherit** - Open any feature worktree:
   ```bash
   branchbox feature start "new feature"
   cd ../myapp-new-feature
   code .  # Reopen in Container
   gh repo view  # Already authenticated!
   claude chat   # Already authenticated!
   ```

3. **Credentials persist** - Stored in parent directory (`~/projects/`), mounted read-write to all containers via `SHARED_CONFIG_DIR` environment variable.

**Directory structure:**
```
~/projects/
├── .gh/              # Shared GitHub CLI credentials
├── .claude/          # Shared Claude session
├── .codex/           # Shared Codex config
├── .cloudflared/     # Shared Cloudflare tunnel credentials
├── myapp/            # Main worktree (mounts shared dirs)
├── myapp-feature1/   # Feature 1 (mounts same dirs)
└── myapp-feature2/   # Feature 2 (mounts same dirs)
```

### Troubleshooting Devcontainers

**Problem**: "Reopen in Container" option not available

**Solution**: Check that `.devcontainer/` exists in feature worktree:
```bash
ls .devcontainer/
# Should show: devcontainer.json  compose.yaml  Dockerfile
```

If missing, sync from main:
```bash
branchbox devcontainer sync
```

**Problem**: Container takes too long to start

**Solution**: Pre-built images should start in seconds. If building locally:
```bash
# Check if using pre-built image
grep "ghcr.io/branchbox" .devcontainer/compose.yaml

# Force pull latest pre-built image
DEVCONTAINER_PULL_POLICY=always code .
```

**Problem**: Tools require re-authentication in each container

**Solution**: Verify shared config mounts are active:
```bash
# Inside container
mount | grep -E '(gh|claude|codex)'
```

Check `SHARED_CONFIG_DIR` in `.env`:
```bash
grep SHARED_CONFIG_DIR .env
```

**Problem**: Container fails to start

**Solution**: Rebuild without cache:
```bash
# In VS Code: Cmd/Ctrl+Shift+P
# → "Dev Containers: Rebuild Container Without Cache"
```

**Problem**: Need to force local Dockerfile builds (skip pre-built image)

**Solution**: Use one of these approaches:

```bash
# Option 1: Set environment variable
DEVCONTAINER_PULL_POLICY=build code .

# Option 2: Use compose override file (for BranchBox development)
COMPOSE_FILE=compose.yaml:compose.local-build.yaml docker compose up

# Option 3: Add to your .env file for persistent local builds
echo "DEVCONTAINER_PULL_POLICY=build" >> .env
```

### Upgrading Existing Projects

Projects initialized before the pre-built images feature need manual updates to benefit from faster startup times.

**Quick upgrade** - update your `.devcontainer/compose.yaml`:

```yaml
services:
  your-service:
    # Add these lines to use pre-built image with fallback
    image: ${DEVCONTAINER_IMAGE:-ghcr.io/branchbox/branchbox/devcontainer-<stack>:latest}
    pull_policy: ${DEVCONTAINER_PULL_POLICY:-missing}
    # Keep your existing build: section as fallback
    build:
      context: ..
      dockerfile: .devcontainer/Dockerfile
```

Replace `<stack>` with: `rust`, `rails`, `nodejs`, or `generic`.

**Rails/Node.js users**: The new templates use mise for runtime version management. If you want to adopt the new approach, re-run `branchbox init` to regenerate your `.devcontainer/` files. The templates use:
```dockerfile
FROM mcr.microsoft.com/devcontainers/base:debian
# mise reads .ruby-version, .nvmrc, .node-version, .tool-versions
```

After updating, your next `docker compose up` or "Reopen in Container" will pull the pre-built image instead of building locally.

## Manual Release Harnesses

The manual CLI CI jobs use plain Cargo with an ordinary dependency cache and one
Rust cache writer on main-branch pushes. The mbx experiment is withdrawn; see the
[measurements and reproduction guide](docs/docs/internals/rust-build-cache.md)
for historical results and the current cache policy. Replacement performance has
not yet been measured.

Before tagging a release, run the CLI smoke matrix:

```bash
./scripts/manual-cli-e2e.sh
./scripts/manual-cli-e2e.sh --mode verbose
./scripts/manual-cli-e2e.sh --mode pretend
STACK=generic ./scripts/manual-cli-e2e.sh
STACK=rails ./scripts/manual-cli-e2e.sh
STACK=node ./scripts/manual-cli-e2e.sh
```

If your change touches devcontainer 1Password auth/signing wiring (issue #45 flow), also run:

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
OP_GITHUB_REF='op://<vault>/<item>/token' \
OP_SIGNING_KEY_REF='op://<vault>/<item>/private key' \
./scripts/manual-1password-e2e.sh --check-failure-path
```

Details: `docs/docs/getting-started/manual-cli-e2e.md` and `docs/docs/getting-started/manual-1password-e2e.md`.
