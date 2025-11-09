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
  init     Initialize project with devcontainer and BranchBox registry
  detect   Detect project configuration
  name     Feature name utilities
  feature  Manage feature worktrees
  help     Print this message or the help of the given subcommand(s)

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
      --use-parent-structure  Use in-place parent structure (reorganize as container/main/)
      --update                Update existing setup without restructuring
      --validate              Validate only (no modifications)
      --dry-run               Dry run (show what would happen)
  -y, --yes                   Non-interactive mode (use defaults, answer yes to prompts)
  -v, --verbose               Verbose output
  -h, --help                  Print help
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
      --telemetry                      Emit verbose telemetry (e.g. Cloudflare operations)
      --skip-module <MODULE>           Skip specific modules during setup (can be specified multiple times) Available modules: compose, database, tunnel, specs
      --minimal                        Start feature workflow in minimal mode (skips devcontainer/compose/specs)
      --fast                           Hidden alias for --minimal
      --prompt <PROMPT>                Provide a prompt seed (trimmed to 2,000 characters) for future agent hand-off
      --default-prompt                 Pre-fill the built-in minimal-mode prompt (requires --minimal/--fast and conflicts with --prompt)
      --json                           Emit a JSON summary instead of human-readable output
      --no-summary                     Suppress the text summary (ignored when --json is set)
  -h, --help                           Print help
```

Minimal/fast mode skips heavyweight modules (`devcontainer`, `compose`, `specs`) and prints a reminder to finish provisioning later. Policy-enforced modules listed in `BRANCHBOX_POLICY_ENFORCED_MODULES` always run even when skipped elsewhere. Use `--prompt` (or the shortcut `--default-prompt` in minimal mode) to capture a seed hand-off for the forthcoming agent bridge; the CLI truncates it at 2,000 characters and records its length in the summary. Combine `--json` with `--no-summary` for automation-friendly output.

### Feature Start Summary

Every `feature start` (and `feature new`) run ends with:
- A headline plus a three-column checklist (`Step`, `Result`, `Details`) that surfaces the worktree path, branch, workspace color, adapter/service URL, feature URL, compose project, `.env`, prompt status, module health, skipped modules, and the default-agent plan.
- Adapter warnings continue to print below the checklist when present, and prompt rows reflect whether `BRANCHBOX_ENABLE_PROMPT_BRIDGE` is active.
- A tabular breakdown of all modules with status (`success`, `skipped`, `failed`), duration, notes, and forced markers for policy-required modules.
- Explicit “Skipped modules” and “Warnings” sections, along with next steps for minimal starts (`branchbox devcontainer sync`, etc.).

Pass `--json` to receive the same data as structured JSON (including `module_outcomes`, `skipped_modules`, and telemetry flags) and parse it inside automation such as the forthcoming agent daemon or control plane.
- The JSON payload includes a `default_agent` object (`status`, `label`, `command`, `detail`, `followup`) so automation can tell whether a CLI run will launch an agent, has to wait for devcontainer sync, or is blocked entirely.

### Auto-launching a default coding agent

- Set `BRANCHBOX_DEFAULT_AGENT_CMD` to the exact command you want BranchBox to run once the devcontainer module completes (for example, `cursor --workspace .` or `claude-code --agent branchbox`).
- Optionally set `BRANCHBOX_DEFAULT_AGENT_NAME` to control how the checklist row and log message refer to that command.
- The checklist explains whether the command will run immediately (`✅ ready`), is waiting for `branchbox devcontainer sync` (`… waiting`), or is blocked because the devcontainer module failed (`⚠️ blocked`).
- Text-mode runs launch the command after summaries finish printing; JSON-only runs skip auto-launch to keep output consumable while still surfacing the plan via the `default_agent` JSON section.

## branchbox feature list

```text
List known feature worktrees from the registry

Usage: branchbox feature list [OPTIONS]

Options:
      --repo <REPO>      Repository path (defaults to current directory)
      --status <STATUS>  Filter by status (active, removed)
      --all              Show all entries regardless of status
      --json             Emit JSON output (includes start_mode, prompt_seed, module_outcomes, etc.)
  -h, --help             Print help
```

The tabular output now surfaces:
- `Mode` — how the feature started (`full` vs `minimal`).
- `Prompt` — whether a prompt seed is stored (and its character count).
- `Modules` — summary counts for module outcomes (`ok/skip/fail/forced`), helping operators spot failures at a glance.

Combine `--json` with existing filters to feed workflows or dashboards. JSON entries include the stored `module_outcomes`, `prompt_seed`, and timestamps so downstream systems can render richer health signals without rerunning `feature start`.

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
