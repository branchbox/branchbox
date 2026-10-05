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

Normal feature start runs the configured modules, which may include:
- Devcontainer sync
- Docker Compose project
- Database naming configuration
- Tunnel configuration

**Minimal mode skips devcontainer sync, Compose isolation, and specs by default:**

```bash
branchbox feature start "Quick spike" --minimal
```

You still get a Git worktree and branch, and the normal environment-file handling. The start result names the modules that ran or were skipped.

Minimal mode is not a blanket switch that disables every module or runtime. Database and tunnel behavior depends on project configuration and explicit skips; policy-enforced modules can override minimal defaults. Use `--skip-module database --skip-module tunnel` when you want to request those skips too, and review the result for enforced policy.

The Mac app's **Quick** setup choice uses this same mode. It does not guarantee a freshly synced devcontainer. See the [Mac app guide](mac-app.md#start-a-feature).

## Sync the Devcontainer Later

If your spike turns into real work, sync the devcontainer:

```bash
cd ../quick-spike
branchbox devcontainer sync
```

This syncs the devcontainer configuration for active features. It does not run every skipped module or start a container. Start the devcontainer separately when you need it, and review the other feature setup requirements.

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
- `database` — Database naming configuration in an existing `.env`
- `tunnel` — Cloudflare tunnel provisioning
- `specs` — Feature spec lifecycle

## Startup Cost

Minimal mode avoids the default devcontainer sync, Compose isolation, and spec work. Actual startup time and resource use depend on the repository, runtime, enabled modules, and policy. Use the start result's module durations to compare runs in your own project.

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

Review the start summary to confirm which modules and runtime setup ran for this project.

---

**Next:** [Sharing with Tunnels](sharing-with-tunnels.md) — Let reviewers see your feature live
