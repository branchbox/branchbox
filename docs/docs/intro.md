---
sidebar_position: 1
slug: /
---

# Welcome to BranchBox

BranchBox is a CLI tool that orchestrates git worktrees and development environments, enabling you to work on multiple features simultaneously with complete isolation.

## What is BranchBox?

BranchBox helps you manage feature development by:

- **Creating isolated worktrees** for each feature with git branches
- **Auto-detecting your stack** (Rails, Node.js, Rust, or generic projects)
- **Managing development environments** with devcontainer synchronization
- **Isolating Docker Compose projects** to avoid port conflicts
- **Tracking feature specifications** through backlog → in-progress → completed workflow
- **Provisioning Cloudflare tunnels** for shareable URLs (optional)

## Quick Start

```bash
# Install BranchBox
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash

# Initialize your project
cd your-project
branchbox init

# Start working on a feature
branchbox feature start oauth-integration

# List active features
branchbox feature list

# Tear down when done
branchbox feature teardown oauth-integration
```

## Getting Started

- **New to BranchBox?** Start with the [Installation Guide](getting-started/installation.md)
- **Want to understand the architecture?** Visit the [Architecture Overview](architecture.md)
- **Need CLI reference?** Check the [CLI Reference](reference/cli.md)
- **Working with feature specs?** See the [Specs Workflow](reference/specs-workflow.md)

## Key Features

### Automatic Stack Detection

BranchBox detects your project type and configures itself accordingly:

- **Rails** - Database setup, Rails-specific secrets
- **Node.js** - npm/yarn/pnpm detection, .env handling
- **Rust** - Cargo workspace handling
- **Generic** - Basic .env copying

### Composable Modules

Enable only the features you need:

- **Compose** - Docker Compose project isolation
- **Database** - Database-level isolation (Rails/Django)
- **Tunnel** - Cloudflare tunnel provisioning
- **Specs** - Feature specification lifecycle
- **Devcontainer** - Devcontainer config synchronization

### Simple State Management

All feature state is tracked locally in `.branchbox/registry.json` - no external services required.

## Quick Links

- [CLI Reference](reference/cli.md) - Complete command documentation
- [GitHub Repository](https://github.com/branchbox/branchbox) - Source code and issues
- [Release Notes](https://github.com/branchbox/branchbox/releases) - Latest updates

## Community & Support

- Report bugs or request features on [GitHub Issues](https://github.com/branchbox/branchbox/issues)
- Contribute improvements via pull requests
- Check the repository's `CLAUDE.md` and `AGENTS.md` for contributor guidelines

:::note
BranchBox is actively developed. The current CLI-based architecture is fully functional and will remain the foundation for future enhancements.
:::
