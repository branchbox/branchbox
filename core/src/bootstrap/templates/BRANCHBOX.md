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
- Use Docker-in-Docker for container operations

## 1Password Integration

BranchBox devcontainers include native support for [1Password CLI](https://developer.1password.com/docs/cli/). If you use 1Password to manage your GitHub tokens and SSH signing keys, they're injected automatically on container start — with biometric confirmation.

### Prerequisites

1. [1Password desktop app](https://1password.com/downloads) installed
2. [1Password CLI](https://developer.1password.com/docs/cli/get-started/) installed: `brew install 1password-cli`
3. Biometric unlock enabled: 1Password → Settings → Developer → Enable CLI integration

### Setup

Set the 1Password secret references for your GitHub token and (optionally) signing key. Add to your shell profile (`~/.zshrc` or `~/.bashrc`) or your project's `.env` file:

```bash
# GitHub PAT for git push/pull and gh CLI
export OP_GITHUB_REF="op://VaultName/GitHub PAT/credential"

# SSH signing key for verified commits (optional)
export OP_SIGNING_KEY_REF="op://VaultName/SSH Signing Key/private key"
```

Replace `VaultName`, item names, and field names with your actual 1Password references. See [1Password secret references](https://developer.1password.com/docs/cli/secret-references/) for the format.

### How It Works

On each container start, BranchBox runs a two-phase setup:

1. **Host phase** (`init-host.sh` via `initializeCommand`): Fetches secrets from 1Password using `op read` (triggers biometric prompt), writes them to files in `.devcontainer/` that get mounted into the container.

2. **Container phase** (`setup-git.sh` via `postStartCommand`): Reads the mounted secrets and configures git credential helper, GH_TOKEN, SSH commit signing, and git identity (inherited from host).

```
Host (initializeCommand)          Container (postStartCommand)
┌───────────────────────────┐     ┌──────────────────────────────┐
│ op read PAT               │──►  │ git config credential...     │
│ op read signing key       │──►  │ git config gpg.format ssh    │
│ git config user.name/email│──►  │ git config user.name/email   │
│ (biometric prompt)        │     │ export GH_TOKEN=...          │
└───────────────────────────┘     └──────────────────────────────┘
```

### What You Get

After container start:
- `git push` / `git pull` just work (HTTPS with PAT)
- `gh pr create` and other gh CLI commands just work
- Commits are signed and verified (if signing key configured)
- Git identity matches your host config
- No private keys or tokens stored in the repo

### Without 1Password

If you don't use 1Password or haven't set the env vars, the scripts gracefully skip — nothing breaks. You can still authenticate manually inside the container.

### Per-Developer Overrides

Each developer can use their own 1Password vault items by exporting different references in their shell profile:

```bash
export OP_GITHUB_REF="op://Personal/My GitHub Token/credential"
export OP_SIGNING_KEY_REF="op://Personal/My Signing Key/private key"
```

### Security

- Secret files (`.github-token.env`, `.git-signing-key`, `.gitconfig.env`) are in `.gitignore` — never committed
- Files are mounted read-only into the container
- Secrets are fetched fresh on each container rebuild (no stale tokens)
- Biometric confirmation required every time

## Learn More

- [BranchBox Documentation](https://branchbox.dev)
- [GitHub Repository](https://github.com/branchbox/branchbox)
