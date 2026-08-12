---
sidebar_position: 0
---

# Quick Start

Get BranchBox running in 2 minutes.

## Install

import Tabs from '@theme/Tabs';
import TabItem from '@theme/TabItem';

<Tabs>
<TabItem value="homebrew" label="Homebrew (macOS)" default>

```bash
brew install branchbox/tap/branchbox
```

</TabItem>
<TabItem value="script" label="Script (Linux/macOS)">

```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
```

</TabItem>
</Tabs>

## Initialize Your Project

Navigate to any git repository and run:

```bash
branchbox init
```

BranchBox will:
- Detect your stack (Rails, Node.js, Rust, or Generic)
- Set up the `.branchbox/` registry
- Validate your devcontainer (if present)

### First-run prompts (`branchbox init`)

In an interactive terminal, `branchbox init` asks only for decisions that change behavior:

| Prompt | When it appears | Default |
| --- | --- | --- |
| `Move to permanent location? (Y/n)` | Repository is in a temporary path (for example under `/tmp`) | `Y` |
| `Continue? (y/N)` (with from/to paths) | Reorganization is about to move the repository | `N` unless you type `y` |
| `Enable Cloudflare tunnels for feature worktrees?` | Interactive run while tunnel config is being set | Current config value (first run defaults to enabled) |
| `Tunnel name prefix ...` | Tunnel support enabled | Pre-filled (`branchbox`) |
| `DNS zone for tunnel hostnames ...` | Tunnel support enabled | Empty allowed |
| `Provide Cloudflare API credentials now ...` | Tunnel support enabled | Uses whether credentials already exist |
| `Cloudflare account ID` / `Cloudflare API token` | You chose automated provisioning | Required |
| `Provision tunnel for 'main' branch now?` | Credentials were provided | `Yes` |
| `Service URL for tunnel ingress ...` | Provisioning main tunnel now | Auto-detected from compose/devcontainer |

For CI or fully scripted setup, use:

```bash
branchbox init -y
```

This skips prompts and applies defaults.

### Optional: 1Password + git bootstrap

If `branchbox init` **creates** `.devcontainer/` from BranchBox templates, it also wires:

- `.devcontainer/scripts/init-host.sh` (runs via `initializeCommand` on host)
- `.devcontainer/scripts/setup-git.sh` (runs via `postStartCommand` in container)
- mounted credential files: `.github-token.env`, `.git-signing-key`, `.gitconfig.env`

With `OP_GITHUB_REF` / `OP_SIGNING_KEY_REF` set, opening the container can auto-refresh token/key from 1Password, configure git HTTPS credentials, and enable SSH commit signing (when key material is valid).

Important: if your repo already has a custom `.devcontainer/`, `branchbox init` currently updates workspace compatibility but does **not** automatically retrofit these 1Password/git hooks into your existing files.

## Start Your First Feature

```bash
branchbox feature start "Add user authentication"
```

You'll see output like:

```
🚀 Feature workspace ready (full)
  Feature: add-user-authentication

+------------------+------------+------------------------------------------+
| Step             | Result     | Details                                  |
+------------------+------------+------------------------------------------+
| Worktree         | ✓ ready    | ../add-user-authentication               |
| Branch           | ✓ ready    | feature/add-user-authentication          |
| Adapter          | ✓ detected | Rails · http://localhost:3000            |
| Compose project  | ✓ isolated | branchbox-add-user-authentication        |
| .env             | ✓ copied   | ../add-user-authentication/.env          |
| Modules          | ✓ ready    | 4 ok                                     |
+------------------+------------+------------------------------------------+
```

## Work in Your Isolated Environment

As shown in the output, the new workspace is created in the parent directory. Change into it:

```bash
cd ../add-user-authentication
```

You're now in a fully isolated workspace:
- **Own git branch** — `feature/add-user-authentication`
- **Own Docker network** — no port conflicts
- **Own database** — no data leaks
- **Own `.env`** — customized for this feature

Run your app, make changes, commit freely. Your main workspace is untouched.

## Optional: Use an SBX MicroVM

The default `container` runtime remains local and account-free. If you have installed and signed in
to Docker Sandboxes, you can place the complete devcontainer and Compose stack inside an SBX VM:

```bash
branchbox feature start "Isolated agent task" --runtime sbx
branchbox feature exec isolated-agent-task -- codex
```

BranchBox records the sandbox identity and actual host-port mappings for the feature. An SBX login
problem affects only `--runtime sbx`; it does not block the default workflow. See
**[How It Works](../how-it-works.md#runtime-providers)** for provider configuration and lifecycle
details.

## See All Your Features

```bash
branchbox feature list
```

```
📚 Feature registry — 2 active · 0 removed (showing 2/2)
Feature                   Status  Mode  Branch                              Updated
------------------------  ------  ----  ----------------------------------- ----------------
add-user-authentication   Active  full  feature/add-user-authentication     2025-01-07 10:30
fix-payment-bug           Active  full  feature/fix-payment-bug             2025-01-07 09:15
```

## Clean Up When Done

```bash
branchbox feature teardown add-user-authentication
```

```
🧹 Feature teardown finished
  Worktree removed: yes
  Branch deleted: yes
```

Everything is gone. Clean slate.

---

## Next Steps

- **[Working with AI Agents](../guides/ai-agents.md)** — Give Claude/Copilot safe sandboxes
- **[Minimal Mode](../guides/minimal-mode.md)** — Quick spikes without full provisioning
- **[Sharing with Tunnels](../guides/sharing-with-tunnels.md)** — Let reviewers see your feature live
- **[CLI Reference](../reference/cli.md)** — Every command and flag
