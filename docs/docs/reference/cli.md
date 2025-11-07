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
  detect        Detect project configuration
  name          Feature name utilities
  feature       Manage feature worktrees
  help          Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## branchbox init

*Alias: `branchbox bootstrap`*

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
      --use-parent-structure  Use in-place parent structure (reorganize as container/main/)
      --update                Update existing setup without restructuring
      --validate              Validate only (no modifications)
      --dry-run               Dry run (show what would happen)
  -y, --yes                   Non-interactive mode (use defaults, answer yes to prompts)
  -v, --verbose               Verbose output
  -h, --help                  Print help
```

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
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox feature start

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
      --telemetry                      Emit verbose telemetry (e.g. Cloudflare operations)
      --skip-module <MODULE>           Skip specific modules during setup (can be specified multiple times) Available modules: compose, database, tunnel, specs
  -h, --help                           Print help
```

## branchbox feature list

```text
List known feature worktrees from the registry

Usage: branchbox feature list [OPTIONS]

Options:
      --repo <REPO>      Repository path (defaults to current directory)
      --status <STATUS>  Filter by status (active, removed)
      --all              Include removed features even if --status is not provided
      --json             Emit JSON output instead of human-readable summary
  -h, --help             Print help
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
      --delete-branch                  Delete the git branch after removing the worktree
      --force                          Force removal even with local changes
      --complete-spec                  Move spec to completed during teardown
      --telemetry                      Emit verbose telemetry (e.g. Cloudflare operations)
  -h, --help                           Print help
```
