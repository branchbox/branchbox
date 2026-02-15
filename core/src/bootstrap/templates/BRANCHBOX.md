# BranchBox Development Workflow

This project uses [BranchBox](https://github.com/branchbox/branchbox) for managing feature development with git worktrees and devcontainers.

## Quick Start

```bash
# Start a new feature
branchbox feature start "my-feature"

# List active features
branchbox feature list

# Teardown a feature when done
branchbox feature teardown my-feature
```

## How It Works

BranchBox creates isolated workspaces for each feature using git worktrees:

```
project/
├── main/           # Main branch (you are here)
├── my-feature/     # Feature worktree
└── another-feature/
```

Each worktree:
- Has its own `.devcontainer/` synced from main
- Shares credentials (`.gh/`, `.claude/`, `.codex/`) across all worktrees
- Can refresh GitHub PAT + signing keys from 1Password at container start
- Can run its own devcontainer independently

## Common Commands

| Command | Description |
|---------|-------------|
| `branchbox feature start "name"` | Create a new feature worktree |
| `branchbox feature start "name" --minimal` | Quick start without full provisioning |
| `branchbox feature list` | List all active features |
| `branchbox feature teardown name` | Remove a feature worktree |
| `branchbox feature teardown name --delete-branch` | Also delete the git branch |
| `branchbox devcontainer sync` | Sync devcontainer changes to all worktrees |
| `branchbox detect` | Show detected stack and modules |

## Devcontainer

Open this project in VS Code or Cursor and use "Reopen in Container" to start developing.

The devcontainer is configured to:
- Mount the parent directory so all worktrees are accessible at `/workspaces/`
- Share credentials across worktrees via mounted config directories
- Run `.devcontainer/scripts/init-host.sh` (host) + `setup-git.sh` (container) for 1Password-backed git auth/signing
- Use Docker-in-Docker for container operations

## Learn More

- [BranchBox Documentation](https://branchbox.dev)
- [GitHub Repository](https://github.com/branchbox/branchbox)
