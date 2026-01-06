---
branch: feature/coding-agents
created: 2026-01-03
status: in-progress
work_feature: coding-agents
---
# Share AI Coding Agent Settings Across Worktrees

## Overview

This feature integrates shared credential management for AI coding agents into
`branchbox init`. When using BranchBox with devcontainers, each isolated
container now shares authentication and configuration with AI coding tools,
eliminating repeated login processes across worktrees.

## Problem

When using BranchBox with devcontainers, each isolated container previously
required separate authentication to AI coding agents and CLI tools, resulting
in:

- Repeated login processes across worktrees
- Lost command history and session data
- Inconsistent configurations between containers

## Solution

The compose templates now configure Docker volume mounts to bind host
directories into container paths for AI coding tools. A centralized settings
directory at the project root level (using `SHARED_CONFIG_DIR` environment
variable, defaulting to `../..`) is shared across all worktrees.

### Supported Tools

| Tool | Host Path | Container Path |
|------|-----------|----------------|
| Claude Code | `.claude/` | `~/.claude/` |
| Claude Code | `.claude.json` | `~/.claude.json` |
| Codex CLI | `.codex/` | `~/.codex/` |
| GitHub CLI | `.gh/` | `~/.config/gh/` |

### Volume Mounts Added

All compose.yaml templates now include these mounts:

```yaml
volumes:
  - ${SHARED_CONFIG_DIR:-../..}/.codex:/home/vscode/.codex
  - ${SHARED_CONFIG_DIR:-../..}/.claude:/home/vscode/.claude
  - ${SHARED_CONFIG_DIR:-../..}/.claude.json:/home/vscode/.claude.json
  - ${SHARED_CONFIG_DIR:-../..}/.gh:/home/vscode/.config/gh
```

### Environment Variable Configuration

The `SHARED_CONFIG_DIR` variable is set in `.devcontainer/.branchbox.env`, which is
auto-generated during `branchbox init`:

```bash
# In .branchbox.env (auto-managed by BranchBox)
SHARED_CONFIG_DIR=../..
```

This keeps BranchBox infrastructure separate from application configuration in `.env`.
For non-worktree setups, edit `.branchbox.env` and change to `SHARED_CONFIG_DIR=..`.

### Key Implementation Details

- **Claude Code Authentication**: Claude Code requires both `~/.claude/`
  directory AND `~/.claude.json` file for proper authentication. Both are now
  mounted.
- **SHARED_CONFIG_DIR**: Explicitly set in `.branchbox.env` and `.env.sample`.
  Defaults to `../..` (worktree parent directory).
- **Container User Detection**: When injecting mounts into existing projects,
  BranchBox parses the Dockerfile to detect the container user and uses the
  correct home path (e.g., `/home/node`, `/root`).
- **Worktree Compatibility**: Settings are shared at the worktree parent level,
  so all feature worktrees share the same credentials.

### First-Time Setup

The `.claude.json` file is created automatically when you first authenticate
with Claude Code **on the host machine**. For proper container operation:

1. **Recommended**: Run `claude` once on your host machine to create
   `~/.claude.json` before starting devcontainers
2. **Alternative**: Create an empty file manually:
   ```bash
   touch ~/.claude.json
   ```

> **Important**: If the source file doesn't exist when Docker starts, it will
> create a **directory** at the mount target instead of a file. This will cause
> Claude Code to fail inside the container. Ensure the file exists on the host
> before starting your devcontainer.

## Benefits

- Authentication and history persist across all worktrees
- Authenticate once with `gh auth login` and credentials are available
  everywhere
- Session state for AI coding agents is preserved across container rebuilds
- Consistent tool configurations between all development environments

## Testing

Run the template tests to verify the mounts are correctly configured:

```bash
cargo test compose_template_includes_workspace_mount_and_shared_configs
```
