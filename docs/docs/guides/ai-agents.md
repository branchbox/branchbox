---
sidebar_position: 2
---

# Working with AI Agents

BranchBox is built for the agentic era. Give Claude, Copilot, Cursor, or any AI coding agent a safe sandbox to operate.

## The Problem with Agents in Shared Environments

AI coding agents are powerful but dangerous in your main workspace:

- They might modify the wrong files
- They can break your working branch
- They make changes you can't easily undo
- Multiple agents can conflict with each other
- You lose the ability to "try things" safely

## The Solution: Isolated Agent Workspaces

Each feature workspace is a safe room for an agent:

```bash
branchbox feature start "Refactor auth module"
cd ../refactor-auth-module

# Now let your agent loose here
# It can't touch your main workspace
```

Even if the agent makes a mess, you just tear it down:

```bash
branchbox feature teardown refactor-auth-module --force
```

Zero damage to your main workspace.

## Auto-Launch Your Agent

Set up BranchBox to automatically launch your preferred agent when a feature starts:

```bash
# In your shell profile (.bashrc, .zshrc, etc.)
export BRANCHBOX_DEFAULT_AGENT_CMD="claude"
export BRANCHBOX_DEFAULT_AGENT_NAME="Claude Code"
```

Now when you start a feature:

```bash
branchbox feature start "Fix payment validation"
```

Output:
```
🚀 Feature workspace ready (full)
  Feature: fix-payment-validation
  ...
🤖 Launching Claude Code via `claude` (cwd: ../fix-payment-validation)
```

The agent starts automatically in the isolated workspace.

## Pass Context to the Agent

Use `--prompt` to capture context for the agent:

```bash
branchbox feature start "Investigate login failures" \
  --prompt "Users report 500 errors on login. Check auth.rs and the session middleware. Look at recent changes to the User model."
```

The prompt is stored in the feature registry and can be retrieved by automation tools.

## Minimal Mode for Quick Agent Exploration

Use `--minimal` for lightweight agent experiments:

```bash
branchbox feature start "Try new approach" --minimal --default-prompt
```

This skips heavyweight modules (devcontainer, compose, database) for faster startup. The agent can explore code without waiting for full provisioning.

If the experiment looks promising, upgrade to full mode:

```bash
branchbox devcontainer sync
```

## JSON Output for Automation

Build agent orchestration pipelines with JSON output:

```bash
branchbox feature start "Agent task" --json
```

Returns structured data:
```json
{
  "work_feature": "agent-task",
  "branch_name": "feature/agent-task",
  "worktree_path": "/path/to/agent-task",
  "mode": "full",
  "prompt_seed": null,
  "module_outcomes": [...],
  "default_agent": {
    "status": "ready",
    "label": "Claude Code",
    "command": "claude"
  }
}
```

Parse this to drive downstream automation.

## Shared Credentials

AI tool credentials are mounted across all worktrees:

- `~/.gh/` — GitHub CLI authentication
- `~/.claude/` — Claude Code config
- `~/.codex/` — OpenAI Codex config

**Authenticate once, work everywhere.** No re-authenticating in each feature workspace.

## Multi-Agent Workflows

Run multiple agents in parallel, each in their own sandbox:

```bash
# Terminal 1: Claude explores the auth system
branchbox feature start "Claude: explore auth" --prompt "Map the authentication flow"
cd ../claude-explore-auth
claude

# Terminal 2: Copilot fixes tests
branchbox feature start "Copilot: fix tests" --prompt "Fix failing unit tests in tests/"
cd ../copilot-fix-tests
# Open in VS Code with Copilot

# Terminal 3: You review both
branchbox feature list
```

Each agent has:
- Its own git branch
- Its own containers
- Its own database state
- No interference with each other or your main workspace

## Example: Let an Agent Refactor Your Code

```bash
# Create isolated workspace for the agent
branchbox feature start "Agent refactor: extract service objects" \
  --prompt "Refactor the OrderController to use service objects. Move business logic to app/services/. Keep controllers thin."

# Agent works in isolation
cd ../agent-refactor-extract-service-objects
claude  # or your preferred agent

# Review the changes
git diff

# If good, commit and merge
git add . && git commit -m "Extract service objects from OrderController"

# If bad, return to your main worktree and tear it down
cd ../<your-project>  # Your main worktree
branchbox feature teardown agent-refactor-extract-service-objects --force
```

---

**Next:** [Minimal Mode](minimal-mode.md) — Quick spikes without full provisioning
