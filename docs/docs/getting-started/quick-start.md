---
sidebar_position: 0
---

# Quick Start

Install BranchBox, initialize a project, and start your first feature workspace.

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

You're now in a separate feature worktree. Depending on the configured modules, it also has:
- **Own git branch** — `feature/add-user-authentication`
- **Compose project identity** — isolates owned container resources; fixed host ports still need conflict-free configuration
- **Database naming configuration** — the database module can add `DATABASE_NAME` to an existing `.env`, but your application must use it and run its own database setup
- **Own `.env`** — customized for this feature

For the container runtime, start the devcontainer separately when you need it. Run your project's database setup and application commands, make changes, and commit in this worktree. Feature setup alone does not prove those services are running.

## Optional: Use an SBX MicroVM

The default `container` runtime remains local and account-free. If you have installed and signed in
to Docker Sandboxes, you can place the complete devcontainer and Compose stack inside an SBX VM:

```bash
branchbox feature start "Isolated agent task" --runtime sbx
branchbox feature exec isolated-agent-task -- codex
```

BranchBox records the sandbox identity and actual host-port mappings for the feature. An SBX login
problem affects only `--runtime sbx`; it does not block the default workflow. Startup errors retain
an actionable error tail and exit status but omit rendered Compose configuration and redact expanded
environment values. Inspect the sandbox-local devcontainer logs when more detail is needed. If you
suspect an older BranchBox version copied a secret into terminal, agent, CI, or telemetry logs during
a failed startup, rotate the affected credential with its provider and remove the durable log copy.
Projects whose Compose stack requires `.devcontainer/.cloudflared.env` must configure Cloudflare
credentials first; BranchBox materializes that file before SBX creation and otherwise stops before
the sandbox build starts.
See
**[How It Works](../how-it-works.md#runtime-providers)** for provider configuration and lifecycle
details.

On an x86_64 Linux/KVM execution node, use the account-free Firecracker provider after installing
its pinned guest image:

```bash
branchbox feature start "Isolated local agent task" --runtime local-vm
branchbox feature exec isolated-local-agent-task -- codex
```

Unlike SBX, `local-vm` needs no Docker account. It requires `/dev/kvm`, Firecracker/jailer, and the
digest-verified image described in
**[How It Works](../how-it-works.md#account-free-firecracker-local-vm)**.

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

### Teardown won't delete your work

Teardown checks the worktree before it removes anything. If you left uncommitted files behind, or the branch has commits that aren't merged, it stops, changes nothing, and tells you what to do:

```
⚠️  Detected uncommitted changes inside ~/projects/myapp/add-user-authentication:
    • notes.txt (untracked)
    (BranchBox refuses to discard them without --discard-changes or --force)
Error: Refusing to tear down 'add-user-authentication'; nothing was removed. 1 uncommitted change in ~/projects/myapp/add-user-authentication would be lost: notes.txt (untracked). Commit or stash it, or rerun with --discard-changes to discard it. Branch 'feature/add-user-authentication' has 1 commit not merged into main; rerun with --keep-branch to keep it, or --force-delete-branch to delete it anyway.
```

Then choose:

- **Keep the work:** commit or stash it, or push the branch and merge it first.
- **Drop the changes:** `--discard-changes` discards the uncommitted files. It never force-deletes the branch.
- **Keep the branch:** `--keep-branch` removes the worktree and keeps the branch for later.
- **Delete unmerged commits too:** `--force-delete-branch` (`git branch -D`).

Not sure what a teardown would do? `--dry-run` prints the plan and changes nothing:

```bash
branchbox feature teardown add-user-authentication --dry-run
```

Files BranchBox generated in the worktree (such as `.devcontainer/.branchbox.env`) never count as your changes, and the feature's spec is moved back to the main worktree.

:::note[About `--force`]
`--force` still removes the worktree whatever its state, as in earlier releases, and when the branch is deleted it uses `git branch -D`, so unmerged commits are deleted as well. Prefer `--discard-changes` with `--keep-branch` or `--force-delete-branch`, which say exactly what you want to lose.
:::

---

## Next Steps

- **[Working with AI Agents](../guides/ai-agents.md)** — Give Claude/Copilot safe sandboxes
- **[Minimal Mode](../guides/minimal-mode.md)** — Quick spikes without full provisioning
- **[Sharing with Tunnels](../guides/sharing-with-tunnels.md)** — Let reviewers see your feature live
- **[CLI Reference](../reference/cli.md)** — Every command and flag
