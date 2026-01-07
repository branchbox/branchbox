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
6. **Registers the feature** in `.branchbox/registry.json`

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
      "mode": "full",
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
   - Compose: (No-op, containers stop when worktree removed)
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

---

**Deep Dive:** For architecture details (SQLite schemas, gRPC protocols, module implementation), see [Internals](internals/architecture.md).
