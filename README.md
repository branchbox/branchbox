# BranchBox
### *Parallel development for humans and AI agents — real environments, zero collisions.*

[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
[![CI](https://github.com/branchbox/branchbox/workflows/CI/badge.svg)](https://github.com/branchbox/branchbox/actions)
[![License](https://img.shields.io/github/license/branchbox/branchbox)](LICENSE)

**[Website](https://branchbox.dev)** · **[Documentation](https://branchbox.dev/docs)** · **[GitHub](https://github.com/branchbox/branchbox)**

BranchBox is an open-source engine for **isolated, fully provisioned development environments** designed for engineers working with AI coding agents, or anyone juggling multiple features at once.

Every feature becomes its own **self-contained workspace** with:

- A dedicated Git worktree  
- Its own devcontainer  
- Its own Docker network  
- Its own database  
- Its own ports  
- Its own environment variables  
- Optional Cloudflare or SSH tunnels  
- Shared tool credentials mounted safely  

You get **real dev environments**, not lightweight sandboxes.  
Your agents get safe rooms to operate.  
Your main workspace stays clean.

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

> **A safe, predictable compute system where every feature has its own fully isolated, fully functional dev environment.**

Start one feature or ten.  
Work alone or with multiple agents.  
Nothing touches anything else unless you want it to.

---

## What BranchBox gives you

### 1. True isolation  
Every feature gets its own:

- Compose project  
- Docker network  
- Ports  
- `.env` file  
- Database  
- Devcontainer  

Zero collisions. Zero shared state unless explicitly mounted.

### 2. Real development environments  
Not an agent-only sandbox — a **full stack** environment:

- Rails, Node, Python, or generic  
- Containers, Docker Compose, databases  
- Shared credentials mounts  
- VS Code + Cursor devcontainers  
- Built-in adapter system for detecting stacks  

If it runs locally, BranchBox can isolate it cleanly.

### 3. Parallel workflows that feel effortless  
Start five features at once.  
Run five containers.  
Open five devcontainers.  
Agents run jobs while you code in another branch.

The mental overhead stays low, and the environments stay clean.

### 4. Agent-ready by design  
An always-on BranchBox Agent tracks:

- Feature events  
- Heartbeats  
- Stack metadata  
- Control-plane connectivity  
- macOS app integration over gRPC  

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

Your feature now has:

- Its own DB  
- Its own Docker network  
- Its own ports  
- Its own devcontainer  
- Its own environment variables  

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

# Run a coding agent inside the active feature's recorded runtime
branchbox feature exec add-oauth -- codex
```

The optional `sbx` provider requires an installed and authenticated Docker Sandboxes CLI. SBX
authentication affects only explicitly selected SBX workspaces; normal BranchBox workflows do not
require a Docker account. `local-vm` is reserved for the planned account-free Colima/Lima backend.
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
