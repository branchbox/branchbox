---
sidebar_position: 1
---

# CLI Reference

The sections below document the current CLI commands. When CLI arguments change, regenerate this reference by running `branchbox --help` and its subcommands.

## branchbox

```text
Isolated development environments for every feature

Usage: branchbox <COMMAND>

Commands:
  init          Initialize project with devcontainer and BranchBox registry
  devcontainer  Manage devcontainer configuration
  agent         Agent and control-plane helpers
  detect        Detect project configuration
  name          Feature name utilities
  feature       Manage feature worktrees
  tunnel        Manage tunnels for existing features
  help          Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## branchbox init

```text
Initialize project with devcontainer and BranchBox registry

Usage: branchbox init [OPTIONS] [SOURCE]

Arguments:
  [SOURCE]  Repository URL or path (defaults to current directory)

Options:
  -p, --path <PATH>           Target directory for parent worktree
  -s, --stack <STACK>         Force specific stack (rails, nodejs, rust, generic)
      --skip-devcontainer     Skip devcontainer setup
      --skip-env              Skip environment setup
      --reorganize            Force reorganization into worktree structure
      --no-parent-structure   Disable parent structure (keep flat layout instead of container/main/)
      --update                Update existing setup without restructuring
      --validate              Validate only (no modifications)
      --dry-run               Dry run (show what would happen)
  -y, --yes                   Non-interactive mode (use defaults, answer yes to prompts)
  -v, --verbose               Verbose output
      --no-coding-agents      Disable AI coding agent mounts (.codex, .claude, .gh)
  -h, --help                  Print help
```

### `branchbox init` interactive behavior

When `branchbox init` runs in an interactive terminal (and without `-y/--yes`), it can prompt for:

- Reorganization decisions (`Move to permanent location?`, `Continue?`)
- Cloudflare tunnel setup (`Enable Cloudflare tunnels...`, prefix, DNS zone, credentials)
- Optional immediate main tunnel provisioning (`Provision tunnel for 'main' branch now?`)

Use `branchbox init -y` to skip prompts and apply defaults.

## branchbox devcontainer

```text
Manage devcontainer configuration

Usage: branchbox devcontainer <COMMAND>

Commands:
  sync  Sync devcontainer configuration to all feature worktrees
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox devcontainer sync

```text
Sync devcontainer configuration to all feature worktrees

Usage: branchbox devcontainer sync [OPTIONS]

Options:
  -p, --path <PATH>          Project directory (defaults to current directory)
  -s, --strategy <STRATEGY>  Sync strategy (copy or symlink)
  -n, --dry-run              Dry run - show what would be synced without making changes
  -h, --help                 Print help
```

## branchbox agent

```text
Agent and control-plane helpers

Usage: branchbox agent <COMMAND>

Commands:
  status  Show agent/control-plane status
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox agent status

```text
Show agent/control-plane status

Usage: branchbox agent status [OPTIONS]

Options:
      --json  Emit JSON output
  -h, --help  Print help
```

## branchbox detect

```text
Detect project configuration

Usage: branchbox detect [OPTIONS]

Options:
  -p, --path <PATH>  Project directory (defaults to current directory)
  -h, --help         Print help
```

## branchbox name

```text
Feature name utilities

Usage: branchbox name <COMMAND>

Commands:
  generate  Generate feature name from title
  validate  Validate feature name
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox name generate

```text
Generate feature name from title

Usage: branchbox name generate <TITLE>

Arguments:
  <TITLE>  Feature title (e.g., "OAuth Integration")

Options:
  -h, --help  Print help
```

## branchbox name validate

```text
Validate feature name

Usage: branchbox name validate <NAME>

Arguments:
  <NAME>  Feature name to validate (e.g., "oauth-integration")

Options:
  -h, --help  Print help
```

## branchbox feature

```text
Manage feature worktrees

Usage: branchbox feature <COMMAND>

Commands:
  start     Create a new feature worktree and run module setup
  teardown  Tear down an existing feature worktree
  list      List known feature worktrees from the registry
  prune     Tear down all active feature worktrees
  exec      Execute a command through a feature's runtime provider
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox feature start

Alias: `branchbox feature new`

```text
Create a new feature worktree and run module setup

Usage: branchbox feature start [OPTIONS] [NAME]

Arguments:
  [NAME]  Dasherized feature name (e.g., oauth-integration)

Options:
      --title <TITLE>                  Free-form feature title (converted to dasherized name)
      --base <BASE>                    Base branch to branch from (defaults to current HEAD)
      --branch-prefix <BRANCH_PREFIX>  Override branch prefix (defaults to "feature")
      --repo <REPO>                    Repository path (defaults to current directory)
      --reuse                          Allow reusing an existing worktree directory
      --devcontainer-reuse <POLICY>    Copy-mode conflict policy when reusing a worktree (fail, preserve, overwrite, inspect) [default: fail]
      --keep-runtime-on-failure        Retain a failed SBX runtime and its build cache for inspection or retry
      --reuse-runtime                  Reuse a retained runtime (implies --reuse and --keep-runtime-on-failure)
      --telemetry                      Emit verbose telemetry (e.g. Cloudflare operations)
      --skip-module <MODULE>           Skip specific modules during setup (can be specified multiple times) Available modules: compose, database, tunnel, specs
      --minimal                        Start feature workflow in minimal mode (skips heavyweight modules)
      --prompt <PROMPT>                Provide an optional prompt seed for automation/agent hand-off
      --default-prompt                 Use the default minimal-mode prompt shortcut (only valid with --minimal/--fast)
      --json                           Emit JSON summary payload instead of human-readable text
      --allow-container                Allow running feature start from inside a containerized environment
      --no-summary                     Suppress summary output (text mode only)
      --runtime <PROVIDER>             Workspace isolation runtime (container, sbx, or Linux/KVM local-vm)
  -h, --help                           Print help
```

## branchbox feature list

```text
List known feature worktrees from the registry

Usage: branchbox feature list [OPTIONS]

Options:
      --repo <REPO>      Repository path (defaults to current directory)
      --status <STATUS>  Filter by status (active, degraded, failed_retained, orphaned, removed)
      --all              Include removed features (retained and orphaned features are shown by default)
      --json             Emit JSON output instead of human-readable summary
  -h, --help             Print help
```

## branchbox feature exec

Use `--` before the command when it has its own flags.

```text
Execute a command through a feature's runtime provider

Usage: branchbox feature exec [OPTIONS] <NAME> <COMMAND>...

Arguments:
  <NAME>        Dasherized feature name
  <COMMAND>...  Command and arguments to execute

Options:
      --repo <REPO>  Repository path (defaults to current directory)
      --json         Emit captured command output as JSON
  -h, --help         Print help
```

## branchbox feature teardown

```text
Tear down an existing feature worktree

Usage: branchbox feature teardown [OPTIONS] <NAME>

Arguments:
  <NAME>  Dasherized feature name to tear down (e.g., oauth-integration)

Options:
      --branch-prefix <BRANCH_PREFIX>  Override branch prefix (defaults to "feature")
      --repo <REPO>                    Repository path (defaults to current directory)
      --keep-branch                    Keep the git branch after removing the worktree (default is to delete it)
      --delete-branch                  Delete the git branch after removing the worktree
      --force                          Force removal even with local changes
      --force-delete-branch            Force-delete the git branch even if it is not fully merged (`git branch -D`)
      --complete-spec                  Move spec to completed during teardown
      --telemetry                      Emit verbose telemetry (e.g. Cloudflare operations)
      --allow-container                Allow running feature teardown from inside a containerized environment
  -h, --help                           Print help
```

## branchbox feature prune

```text
Tear down all active feature worktrees

Usage: branchbox feature prune [OPTIONS]

Options:
      --repo <REPO>      Repository path (defaults to current directory)
      --dry-run          Show which features would be removed without applying teardown
  -y, --yes              Skip confirmation prompt
      --keep-branch      Keep git branches after removing worktrees
      --delete-branch    Delete git branches after removing worktrees
      --complete-spec    Move specs to completed during teardown
      --telemetry        Emit verbose telemetry (e.g. Cloudflare operations)
      --allow-container  Allow running feature prune from inside a containerized environment
  -h, --help             Print help
```

## branchbox tunnel

```text
Manage tunnels for existing features

Usage: branchbox tunnel <COMMAND>

Commands:
  open    Provision (or re-provision) a tunnel for an existing feature
  remove  Remove tunnel metadata and attempt provider teardown
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox tunnel open

```text
Provision (or re-provision) a tunnel for an existing feature

Usage: branchbox tunnel open [OPTIONS] <NAME>

Arguments:
  <NAME>  Dasherized feature name (e.g., oauth-integration)

Options:
      --repo <REPO>  Repository path (defaults to current directory)
      --json         Emit JSON output instead of human-readable summary
  -h, --help         Print help
```

## branchbox tunnel remove

```text
Remove tunnel metadata and attempt provider teardown

Usage: branchbox tunnel remove [OPTIONS] <NAME>

Arguments:
  <NAME>  Dasherized feature name (e.g., oauth-integration)

Options:
      --repo <REPO>  Repository path (defaults to current directory)
      --force        Continue even if provider teardown fails
      --json         Emit JSON output instead of human-readable summary
  -h, --help         Print help
```
