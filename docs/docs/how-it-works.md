---
sidebar_position: 2
---

# How It Works

A high-level overview of what happens when you run BranchBox commands.

## The Core Idea: Git Worktrees

BranchBox uses [Git worktrees](https://git-scm.com/docs/git-worktree) — a built-in Git feature that lets you have multiple working directories from the same repository.

```
your-project/           # Main worktree (main branch)
├── .git/
├── src/
└── ...

../add-oauth/           # Feature worktree (feature/add-oauth branch)
├── src/                # Same repo, different branch, different folder
└── ...

../fix-bug/             # Another feature worktree
├── src/
└── ...
```

Each worktree is a full working copy. You can run different branches simultaneously without switching.

## What `branchbox feature start` Does

When you run:

```bash
branchbox feature start "Add OAuth"
```

BranchBox:

1. **Creates a git worktree** at `../add-oauth/`
2. **Creates a branch** `feature/add-oauth` from your current HEAD
3. **Detects your stack** (Rails, Node.js, Rust, or Generic)
4. **Runs modules** based on what it detects:
   - **Devcontainer**: Syncs `.devcontainer/` to the new worktree
   - **Compose**: Sets `COMPOSE_PROJECT_NAME` for Docker isolation
   - **Database**: Initializes an isolated database
   - **Tunnel**: Provisions a Cloudflare tunnel (if available)
   - **Specs**: Moves feature spec from `backlog/` to `in-progress/`
5. **Copies your `.env`** with updated values (`APP_URL`, `COMPOSE_PROJECT_NAME`)
6. **Prepares the selected workspace runtime** (the existing container workflow by default)
   - With `sbx`, creates a project-scoped sandbox, publishes `forwardPorts`, and runs
     `devcontainer up` inside the sandbox's Docker daemon
   - Bridges each published VM port to the matching nested Compose service; occupied host ports
     are replaced with available ports and the resolved mappings are recorded in the registry
7. **Registers the feature** in `.branchbox/registry.json`

## Runtime Providers

Runtime providers describe the outer execution boundary for a workspace. This is separate from a
devcontainer, which describes the tools and services inside the developer environment.

```bash
# Existing behavior; this remains the default and requires no Docker Sandboxes account
branchbox feature start add-oauth --runtime container

# Experimental Docker Sandboxes microVM boundary
branchbox feature start add-oauth --runtime sbx
```

The `sbx` provider detects the Docker Sandboxes CLI and authentication before changing the
repository. If SBX is unavailable, only the explicitly selected SBX start fails; normal BranchBox
workflows remain account-free. `local-vm` is reserved for the future account-free microVM backend
and currently returns a clear not-implemented error.

Commands can be routed through the runtime recorded for an active feature. This is also how an
SBX-backed coding agent is kept inside the sandbox boundary:

```bash
branchbox feature exec add-oauth -- codex

# Capture stdout, stderr, and the exit code for automation
branchbox feature exec add-oauth --json -- codex --version
```

When `BRANCHBOX_DEFAULT_AGENT_CMD` is configured, the automatic agent launch uses the same runtime
route. Container-backed features retain the existing local execution behavior.

Projects can set a default in `.branchbox/config.json`:

```json
{
  "runtime": {
    "provider": "container"
  }
}
```

### Account-free local VM direction

The reserved `local-vm` provider has been evaluated against three local options:

- [Lima](https://lima-vm.io/docs/examples/containers/) provides the lowest-level building blocks:
  Linux VMs, filesystem sharing, container engines, and port forwarding. It offers the most control,
  but BranchBox would own more provisioning and Docker-context integration.
- [Colima](https://github.com/abiosoft/colima) layers named instances, a Docker runtime, mounts, and
  automatic port forwarding over Lima. It is the recommended first prototype because the existing
  Compose and devcontainer commands can continue to target a Docker-compatible socket without a
  hosted account.
- [Podman machine](https://podman.io/docs/installation) also supplies a local VM on macOS, but its
  Podman API introduces more Compose/devcontainer compatibility risk for the initial backend.

The intended next increment is therefore a project-scoped Colima profile behind the existing
provider trait. It must pass the same lifecycle contract proven for SBX: start a full Compose-based
devcontainer, execute the coding agent inside it, expose collision-safe host ports, survive restart,
and remove only provider-owned state. Direct Lima remains the fallback if Colima's profile or socket
model prevents per-worktree isolation.

## Stack Detection

BranchBox automatically detects your project type:

| Stack | Detection |
|-------|-----------|
| **Rails** | `Gemfile` with `rails` gem |
| **Node.js** | `package.json` present |
| **Rust** | `Cargo.toml` present |
| **Generic** | Fallback for everything else |

Each stack has an **adapter** that knows how to:
- Copy the right secrets (`.env`, `credentials.yml`, etc.)
- Set the correct service URL
- Run stack-specific setup

## Module System

Modules are composable features that run during the worktree lifecycle:

| Module | Purpose | When It Runs |
|--------|---------|--------------|
| **Devcontainer** | Sync `.devcontainer/` config | Always (if `.devcontainer/` exists) |
| **Compose** | Isolate Docker Compose project | Always (if `compose.yaml` exists) |
| **Database** | Isolate database | If `database.yml` or similar detected |
| **Tunnel** | Provision Cloudflare tunnel | Always (manual fallback if no cloudflared) |
| **Specs** | Feature spec lifecycle | If `docs/features/` exists |

Modules run in dependency order. You can skip any module with `--skip-module`.

## The Registry

BranchBox tracks features in `.branchbox/registry.json`:

```json
{
  "features": [
    {
      "work_feature": "add-oauth",
      "branch_name": "feature/add-oauth",
      "worktree_path": "/path/to/add-oauth",
      "status": "Active",
      "start_mode": "full",
      "runtime": {
        "provider": "container"
      },
      "created_at": "2025-01-07T10:30:00Z",
      "updated_at": "2025-01-07T10:30:00Z"
    }
  ]
}
```

This is how `branchbox feature list` knows what's running.

## What `branchbox feature teardown` Does

When you run:

```bash
branchbox feature teardown add-oauth
```

BranchBox:

1. **Runs module teardown** (in reverse order)
   - Specs: Optionally moves spec to `completed/`
   - Tunnel: Removes tunnel configuration
   - Compose: Discovers the worktree's actual devcontainer Compose project, then removes its containers, networks, and volumes
   - Devcontainer: (No-op)

:::note[Database Persistence]
The database module does **not** automatically delete databases on teardown. Feature databases persist to prevent accidental data loss. To clean them up, manually drop the database or use your database admin tools.
:::
2. **Removes the git worktree** at `../add-oauth/`
3. **Optionally deletes the branch** (prompts if unmerged)
4. **Updates the registry** to mark feature as `Removed`

## Isolation Boundaries

| Resource | How It's Isolated |
|----------|-------------------|
| **Git branch** | Each feature = own branch |
| **Working directory** | Each feature = own folder (`../feature-name/`) |
| **Docker network** | `COMPOSE_PROJECT_NAME` creates separate network |
| **Docker containers** | Compose project name prefixes all containers |
| **Ports** | Each Compose project gets its own port mappings |
| **Environment** | `.env` copied and customized per feature |
| **Database** | Database module creates feature-specific DB |

During teardown, BranchBox matches the exact `devcontainer.local_folder` Docker label before acting
on a devcontainer CLI project. It also restores the BranchBox-managed project name from
`.devcontainer/.branchbox.env`. Teardown verifies that no containers, networks, or volumes with
either owned project label remain before removing the worktree.

---

**Deep Dive:** For architecture details (SQLite schemas, gRPC protocols, module implementation), see [Internals](internals/architecture.md).
