---
status: completed
created: 2025-10-30
updated: 2026-01-11
priority: high
complexity: low
reviewed: true
---

# README Devcontainer Documentation Enhancement

## Overview

This document specifies improvements to `README.md` to better explain the devcontainer workflow and shared configuration mechanism.

## Problem

Current `README.md`:
- ❌ Doesn't explain how to open feature worktrees in containers
- ❌ Doesn't document the "Reopen in Container" workflow
- ❌ Doesn't explain shared credential mechanism
- ❌ Doesn't mention troubleshooting devcontainer issues
- ❌ Missing step-by-step guide for VSCode/Cursor users

## Proposed Changes

### 1. Add New Section: "Devcontainer Workflow"

**Location**: After "Usage Examples" (line 102), before "Installation" (line 146)

**Content**:

```markdown
## Devcontainer Workflow

BranchBox features work seamlessly with VS Code/Cursor devcontainers, giving each feature its own isolated Docker environment while sharing tool credentials.

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
- **Cloudflared** (`cloudflared`) - Credentials in `~/.cloudflared/` (Milestone 1)

**How it works:**

1. **First time** - Authenticate in your main worktree:
   ```bash
   cd ~/projects/myapp  # main worktree
   code .  # Reopen in Container
   gh auth login  # Authenticate once
   claude login  # Authenticate once
   ```

2. **All features inherit** - Open any feature worktree:
   ```bash
   branchbox feature start "new feature"
   cd ../myapp-new-feature
   code .  # Reopen in Container
   gh repo view  # Already authenticated!
   claude chat  # Already authenticated!
   ```

3. **Credentials persist** - Stored in parent directory (`~/projects/`), mounted read-write to all containers via `SHARED_CONFIG_DIR` environment variable.

### How Shared Configs Work

The `.devcontainer/compose.yaml` file contains volume mounts that share tool configurations:

```yaml
volumes:
  - ${SHARED_CONFIG_DIR:-../..}/.gh:/home/vscode/.config/gh
  - ${SHARED_CONFIG_DIR:-../..}/.claude:/home/vscode/.claude
  - ${SHARED_CONFIG_DIR:-../..}/.codex:/home/vscode/.codex
  - ${SHARED_CONFIG_DIR:-../..}/.cloudflared:/home/vscode/.cloudflared
```

**Directory structure:**
```
~/projects/
├── .gh/              # Shared GitHub CLI credentials
├── .claude/          # Shared Claude session
├── .codex/           # Shared Codex config
├── .cloudflared/     # Shared Cloudflare tunnel credentials
├── myapp/            # Main worktree devcontainer mounts these
├── myapp-feature1/   # Feature 1 devcontainer mounts same directories
└── myapp-feature2/   # Feature 2 devcontainer mounts same directories
```

**Override location** (optional):
```bash
# .env file
SHARED_CONFIG_DIR=/custom/path  # Change where configs are mounted from
```

**Default**: `../..` (parent directory of your project)

### Troubleshooting Devcontainers

**Problem**: "Reopen in Container" option not available

**Solution**: Check that `.devcontainer/` exists in feature worktree:
```bash
ls .devcontainer/
# Should show: devcontainer.json  compose.yaml  Dockerfile
```

If missing, manually copy from main repo:
```bash
cp -r ../myapp/.devcontainer .
```

**Problem**: Tools require re-authentication in each container

**Solution**: Verify shared config mounts are active:
```bash
# Inside container
mount | grep -E '(gh|claude|codex)'
# Should show volume mounts from host
```

Check `SHARED_CONFIG_DIR` in `.env`:
```bash
grep SHARED_CONFIG_DIR .env
```

**Problem**: Container fails to start

**Solution**: Validate devcontainer configuration:
```bash
# Check JSON syntax
cat .devcontainer/devcontainer.json | jq .

# Check YAML syntax
cat .devcontainer/compose.yaml | yq .
```

Rebuild without cache:
```bash
# In VS Code: Cmd/Ctrl+Shift+P
# → "Dev Containers: Rebuild Container Without Cache"
```
```

### 2. Update "Working on Your Feature" Example

**Location**: Line 72-89 in "Working on Your Feature" section

**Current**:
```markdown
```bash
cd ../oauth-integration/

# Your feature runs in complete isolation:
# - Separate database (oauth_integration_development)
# - Separate Docker network (oauth-integration_default)
# - Separate port allocation
# - Independent configuration

# Make changes
git add .
git commit -m "Add OAuth provider configuration"
git push -u origin feature/oauth-integration

# Meanwhile, your main worktree keeps running without conflicts!
```
```

**Add after line 75** (after `cd ../oauth-integration/`):
```markdown
```bash
cd ../oauth-integration/

# Open in VS Code/Cursor
code .
# → Click "Reopen in Container" when prompted
# → Your feature now runs in an isolated container

# Your feature runs in complete isolation:
# - Separate database (oauth_integration_development)
# - Separate Docker network (oauth-integration_default)
# - Separate port allocation
# - Independent configuration
# - Same tool credentials as main worktree

# Make changes
git add .
git commit -m "Add OAuth provider configuration"
git push -u origin feature/oauth-integration

# Meanwhile, your main worktree keeps running without conflicts!
```
```

### 3. Update "What Just Happened" in Feature Start Example

**Location**: Line 64-70 in "Starting a New Feature" → "What just happened:"

**Current**:
```markdown
**What just happened:**
- Created git worktree at `../oauth-integration/`
- Created branch `feature/oauth-integration`
- Copied `.env` with `APP_URL` configured for this feature
- Set `COMPOSE_PROJECT_NAME` to isolate Docker containers
- Detected Rails stack and provided database setup instructions
- Created feature spec in `docs/features/in-progress/oauth-integration.md`
```

**Change to**:
```markdown
**What just happened:**
- Created git worktree at `../oauth-integration/`
- Created branch `feature/oauth-integration`
- **Copied `.devcontainer/` from main repo** (enables "Reopen in Container")
- Copied `.env` with `APP_URL` configured for this feature
- Set `COMPOSE_PROJECT_NAME` to isolate Docker containers
- Detected Rails stack and provided database setup instructions
- Created feature spec in `docs/features/in-progress/oauth-integration.md`
```

### 4. Add Devcontainer Note to "Next Steps"

**Location**: Line 59-62 in "Next steps:" output

**Current**:
```markdown
# Next steps:
#   cd ../oauth-integration/
#   bundle install
#   rails db:create db:migrate
```

**Change to**:
```markdown
# Next steps:
#   cd ../oauth-integration/
#   code .  # Open in VS Code/Cursor, then "Reopen in Container"
#   bundle install
#   rails db:create db:migrate
```

### 5. Update "Why BranchBox?" Section

**Location**: Line 36-40

**Current**:
```markdown
**BranchBox gives you:**
- ✅ **Multiple features running simultaneously** with complete isolation
- ✅ **Zero context switching** - each feature is a separate directory
- ✅ **Automatic environment provisioning** - database, Docker, and configuration
- ✅ **Stack-aware setup** - Rails, Node.js, or generic projects
```

**Add fifth bullet**:
```markdown
**BranchBox gives you:**
- ✅ **Multiple features running simultaneously** with complete isolation
- ✅ **Zero context switching** - each feature is a separate directory
- ✅ **Automatic environment provisioning** - database, Docker, and configuration
- ✅ **Stack-aware setup** - Rails, Node.js, or generic projects
- ✅ **Shared tool credentials** - authenticate once, work everywhere
```

### 6. Add Devcontainer to "What Works Now"

**Location**: Line 169-175

**Current**:
```markdown
**✅ Milestone 0 Complete** - Core worktree orchestration:
- Full feature lifecycle (`start`, `teardown`, `list`)
- Stack detection (Rails, Node.js, Generic)
- Module system (Docker Compose, Database, Specs)
- Environment configuration (.env copying with `APP_URL` injection)
- State tracking (JSON registry at `.branchbox/registry.json`)
```

**Add bullet**:
```markdown
**✅ Milestone 0 Complete** - Core worktree orchestration:
- Full feature lifecycle (`start`, `teardown`, `list`)
- Stack detection (Rails, Node.js, Generic)
- Module system (Docker Compose, Database, Specs)
- **Devcontainer propagation** (automatic copy to feature worktrees)
- **Shared tool configs** (gh, claude, codex credentials shared across worktrees)
- Environment configuration (.env copying with `APP_URL` injection)
- State tracking (JSON registry at `.branchbox/registry.json`)
```

## Implementation Steps

1. Create backup of `README.md`:
   ```bash
   cp README.md README.md.backup
   ```

2. Apply changes in order (sections 1-6 above)

3. Verify markdown rendering:
   ```bash
   # Install markdown linter if needed
   npm install -g markdownlint-cli

   # Check syntax
   markdownlint README.md

   # Preview in GitHub-flavored markdown
   grip README.md
   ```

4. Test all code examples:
   ```bash
   # Ensure all bash snippets are valid
   # Ensure all paths referenced exist
   ```

5. Create PR with clear diff showing documentation improvements

## Success Criteria

- ✅ README explains devcontainer workflow clearly
- ✅ Shared credentials mechanism is documented with examples
- ✅ Troubleshooting section covers common issues
- ✅ All code examples are tested and valid
- ✅ Markdown renders correctly on GitHub
- ✅ New users understand workflow within 5 minutes of reading

## Related Documents

- Feature spec: `docs/features/backlog/devcontainer-and-shared-config-management.md`
- Current README: `README.md`
- Architecture doc: `docs/ARCHITECTURE.md`

## Timeline

- **Day 1**: Apply changes to README.md (2 hours)
- **Day 2**: Review, test examples, polish (1 hour)
- **Total**: 3 hours
