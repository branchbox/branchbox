# BranchBox

[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
[![CI](https://github.com/branchbox/branchbox/workflows/CI/badge.svg)](https://github.com/branchbox/branchbox/actions)
[![License](https://img.shields.io/github/license/branchbox/branchbox)](LICENSE)

Stop context switching. Run multiple features in parallel—safely.

Isolated git worktrees with per‑feature devcontainers, databases, Docker networks, and configuration. Perfect for solo engineers and agent‑assisted workflows—you can “yolo” big refactors without touching your main workspace.

▶ Watch 60s teaser: https://example.com/branchbox-teaser  
<!-- Replace the link above with your video URL. Optional: add a thumbnail image here. -->

## Quick Start

```bash
# Install (Linux/macOS)
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash

# Initialize project (creates registry, checks environment)
branchbox init

# Start a fully isolated feature workspace
branchbox feature start "Add OAuth Integration"

# Open and work in the new worktree
cd ../oauth-integration/
# Your feature has its own DB, Docker network, and ports
```

Prefer a disposable sample? Use the bundled harness: `./scripts/setup-sample-workspaces.sh` then run `branchbox init` → `branchbox feature start`.

## Aha Moments

- Multiple features running simultaneously with zero collisions (DB, network, ports).
- One command spins up a complete, stack‑aware feature workspace (Rails, Node, generic).
- Edit `.devcontainer/` once; replay changes everywhere with `branchbox devcontainer sync`.
- Minimal mode for quick spikes or agent “yolo” experiments; add full provisioning later.
- Safety nets: dirty devcontainer/compose guard on teardown; JSON registry for automations.
- Agent‑friendly: shared credentials mount across containers; copy or symlink sync strategy.

## Core Commands

- `branchbox feature start "<title>"` — Create isolated worktree + provision modules
- `branchbox feature list [--json]` — Show feature registry (machine‑readable when needed)
- `branchbox feature teardown <name> [--complete-spec] [--keep-branch]` — Clean up safely
- `branchbox devcontainer sync [--strategy copy|symlink] [--dry-run]` — Replay config across features
- `branchbox detect` — Print detected adapter/modules for the current repo
- `branchbox name generate|validate` — Naming helpers for features

Minimal mode (fast spikes and agent hand‑off):

```bash
branchbox feature new backlog-quick-fix \
  --minimal \
  --default-prompt \
  --json
```

## Devcontainers, Simplified

- New features copy `.devcontainer/` automatically; open in VS Code/Cursor and accept “Reopen in Container”.
- Update all features after editing `.devcontainer/` in the main repo:

```bash
branchbox devcontainer sync
# Optional: --strategy copy|symlink, --dry-run
```

- Shared tool credentials (`.gh`, `.claude/`, `.codex/`) mount from `SHARED_CONFIG_DIR` (default `../..`). Authenticate once; every feature reuses it.

## Examples

Rails:
```bash
branchbox feature start "Add User Dashboard"
# ✓ Rails detected (Gemfile, config/application.rb)
# ✓ DB: user-dashboard_development · Next: rails db:create db:migrate
```

Node.js:
```bash
branchbox feature start "Add GraphQL API"
# ✓ Node.js detected (package.json) · Next: npm install
```

Generic:
```bash
branchbox feature start "Docs Refresh"
# ✓ Generic adapter · Basic isolation applied
```

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
branchbox --help
```

Other methods: Homebrew (coming soon), Scoop (coming soon), prebuilt binaries via Releases, or `cargo install --path cli --locked`. See docs/INSTALLATION.md for details.

## Safety & Agent‑Ready

- Dirty guard on teardown refuses to delete changed devcontainer/compose files unless `--force` is confirmed.
- Copy strategy by default (avoids macOS prompts); opt‑in to symlink for always‑up‑to‑date worktrees.
- Prompt seeds (up to 2,000 chars) stored in the registry; `--default-prompt` available in minimal mode.
- Shared mounts are never deleted; BranchBox preserves host‑side credentials.

## What’s Built

- Milestone 0: Core worktree orchestration (start, teardown, list), stack detection, module system (compose, database, specs), env provisioning, JSON registry.
- Milestone 1 (in progress): Agent daemon for background workflows; CLI bridge; telemetry.

Roadmap highlights: Windows agent transport, native macOS app, Rails control plane, Tailscale mesh. See docs/ARCHITECTURE.md.

## Troubleshooting

- No “Reopen in Container”? Ensure `.devcontainer/` exists in the feature; run `branchbox devcontainer sync`.
- Tools ask to re‑auth? Verify mounts inside the container (`mount | grep -E '(codex|claude|gh)'`) and `SHARED_CONFIG_DIR`.
- Prefer symlinks? Set `BRANCHBOX_DEVCONTAINER_STRATEGY=symlink` (or persist in `.env`).

## Contributing

- Devcontainer ships a ready toolchain. Run `cargo fmt && cargo clippy && cargo test` before PRs.
- Use conventional commits (e.g., `feat(modules): …`). See CONTRIBUTING.md and AGENTS.md.

## License

MIT — see LICENSE.
