---
sidebar_position: 1
slug: /
---

# BranchBox

**Parallel development for humans and AI agents. Real environments, zero collisions.**

## The Problem

Modern development means parallel work — multiple features, AI agents exploring code, experiments running alongside production work. But nothing was built for this:

- **Git branches collide** when you switch contexts
- **Docker ports conflict** across features
- **Databases leak** data between environments
- **AI agents break** your main workspace
- **Devcontainers drift** out of sync

You've felt this. The "which branch am I on?" moment. The port 3000 conflict. The agent that modified the wrong files.

## The Solution

**One command creates a complete, isolated environment:**

```bash
branchbox feature start "Add OAuth"
```

That's it. You now have:

- ✅ Dedicated git worktree and branch
- ✅ Isolated Docker network and ports
- ✅ Separate database
- ✅ Synced devcontainer
- ✅ Copied environment variables
- ✅ Optional Cloudflare tunnel for sharing

Work in `../add-oauth/`. Your main workspace stays pristine.

## See It Work

```bash
# One-time setup
branchbox init

# Start an isolated feature
branchbox feature start "OAuth Integration"

# See what's running
branchbox feature list

# Clean teardown when done
branchbox feature teardown oauth-integration
```

## Who It's For

- **Engineers juggling multiple features** — work on 3 things at once without collisions
- **Teams using AI coding agents** — give Claude/Copilot a safe sandbox
- **Anyone tired of "it works on my machine"** — real isolation, not hope

## Quick Links

- **[Quick Start](getting-started/quick-start.md)** — Get running in 2 minutes
- **[Installation](getting-started/installation.md)** — All install methods
- **[Use Cases](guides/parallel-features.md)** — Common workflows
- **[Demo Assets](guides/demo-assets)** — Website/docs/social demo pipeline
- **[CLI Reference](reference/cli.md)** — Every command documented

---

*Built in Rust. Open source. MIT licensed.*
