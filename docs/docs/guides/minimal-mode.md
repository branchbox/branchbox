---
sidebar_position: 3
---

# Minimal Mode

Skip heavyweight provisioning for quick explorations and spikes.

## When to Use Minimal Mode

- **Quick exploration** — "Let me just check something"
- **Documentation updates** — No containers needed
- **Agent experiments** — Fast feedback loops
- **Resource-constrained systems** — Skip Docker overhead

## How It Works

Normal feature start provisions everything:
- Devcontainer sync
- Docker Compose project
- Database initialization
- Tunnel configuration

**Minimal mode skips these heavyweight modules** for instant startup:

```bash
branchbox feature start "Quick spike" --minimal
```

Output:
```
🚀 Feature workspace ready (minimal)
  Feature: quick-spike

+------------------+------------+------------------------------------------+
| Step             | Result     | Details                                  |
+------------------+------------+------------------------------------------+
| Worktree         | ✓ ready    | ../quick-spike                           |
| Branch           | ✓ ready    | feature/quick-spike                      |
| .env             | ✓ copied   | ../quick-spike/.env                      |
| Modules          | ⏭ skipped  | 3 skip (minimal mode)                    |
+------------------+------------+------------------------------------------+

Skipped modules:
  - devcontainer (Skipped by minimal mode defaults)
  - compose (Skipped by minimal mode defaults)
  - database (Skipped by minimal mode defaults)

Next: run `branchbox devcontainer sync` when you're ready to fully provision.
```

You get:
- ✅ Git worktree and branch
- ✅ Copied `.env` file
- ⏭ No Docker containers
- ⏭ No database setup
- ⏭ No devcontainer sync

## Upgrade to Full Mode Later

If your spike turns into real work, sync the devcontainer:

```bash
cd ../quick-spike
branchbox devcontainer sync
```

Now you have full provisioning.

## Alias: --fast

`--fast` is a hidden alias for `--minimal`:

```bash
branchbox feature start "Quick check" --fast
```

Same behavior, shorter to type.

## Default Prompts for Agents

When using minimal mode with agents, use `--default-prompt` to set context:

```bash
branchbox feature start "Explore codebase" --minimal --default-prompt
```

This stores a default prompt explaining that modules were skipped:

> "You are the default BranchBox coding agent operating in minimal mode. Devcontainer, compose, and specs modules were skipped to keep setup lightweight—focus on quick tweaks or documentation updates, and run `branchbox devcontainer sync` later if full provisioning becomes necessary."

The agent knows to keep changes lightweight.

## Skip Specific Modules

For fine-grained control, skip individual modules:

```bash
# Skip just the tunnel module
branchbox feature start "Local only" --skip-module tunnel

# Skip multiple modules
branchbox feature start "No DB" --skip-module database --skip-module compose
```

Available modules to skip:
- `devcontainer` — Devcontainer sync
- `compose` — Docker Compose project isolation
- `database` — Database initialization
- `tunnel` — Cloudflare tunnel provisioning
- `specs` — Feature spec lifecycle

## Performance Comparison

| Mode | Startup Time | Resources |
|------|-------------|-----------|
| **Full** | ~10-30s (depends on Docker) | Full containers, DB |
| **Minimal** | ~1-2s | Just git worktree |

Minimal mode is **10-15x faster** for exploration.

## Example: Quick Documentation Fix

```bash
# Fast startup for docs-only change
branchbox feature start "Fix README typo" --minimal
cd ../fix-readme-typo

# Make the fix
vim README.md

# Commit and push
git add README.md
git commit -m "Fix typo in installation section"
git push -u origin feature/fix-readme-typo

# Clean up
cd ../main
branchbox feature teardown fix-readme-typo
```

No Docker. No waiting. Just the git worktree you need.

---

**Next:** [Sharing with Tunnels](sharing-with-tunnels.md) — Let reviewers see your feature live
