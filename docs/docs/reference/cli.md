---
sidebar_position: 1
---

# CLI Reference

This page is generated from the `--help` output of every `branchbox` subcommand. When CLI arguments change, regenerate it from a fresh build: run `branchbox --help`, then `--help` on each subcommand, recursively.

Every command that takes `--json` prints exactly one JSON document on stdout, with human-readable text on stderr, and never prompts. A failure prints an error envelope with a stable code, except for the few commands that report failure inside their own payload. The [JSON contract](./json-contract.md) describes the rules and the payloads.

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
  prune         Tear down all active feature worktrees
  tunnel        Manage tunnels for existing features
  version       Show the BranchBox version and, with --json, its contract capabilities
  doctor        Check the host (and optionally a repository) for BranchBox prerequisites
  config        Read and change project configuration (.branchbox/config.json)
  help          Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## branchbox init

Alias: `branchbox bootstrap`

`branchbox init` asks questions only in an interactive terminal without `-y/--yes`. It can ask about:

- reorganization (`Move to permanent location?`, `Continue?`);
- Cloudflare tunnel setup (`Enable Cloudflare tunnels...`, prefix, DNS zone, credentials);
- provisioning a tunnel for `main` right away (`Provision tunnel for 'main' branch now?`);
- 1Password references for the devcontainer's GitHub token and signing key.

Without a terminal, with `-y`, or with `--json`, nothing is asked and the defaults apply. The repository stays where it is unless you pass `--reorganize`: `-y` alone never moves it. Pass the 1Password references with `--op-github-ref` / `--op-signing-key-ref`, or `--skip-1password`. `--json` prints one summary document (see the [JSON contract](./json-contract.md#init)).

```text
Initialize project with devcontainer and BranchBox registry

Usage: branchbox init [OPTIONS] [SOURCE]

Arguments:
  [SOURCE]  Repository URL or path (defaults to current directory)

Options:
  -p, --path <PATH>                  Target directory for parent worktree
  -s, --stack <STACK>                Force specific stack (rails, nodejs, rust, generic)
      --skip-devcontainer            Skip devcontainer setup
      --skip-env                     Skip environment setup
      --reorganize                   Force reorganization into worktree structure
      --no-parent-structure          Disable parent structure (keep flat layout instead of container/main/)
      --update                       Update existing setup without restructuring
      --validate                     Validate only (no modifications)
      --dry-run                      Dry run (show what would happen)
  -y, --yes                          Non-interactive mode (use defaults, answer yes to prompts)
  -v, --verbose                      Verbose output
      --no-coding-agents             Disable AI coding agent mounts (.codex, .claude, .gh)
      --op-github-ref <OP_REF>       1Password reference for the devcontainer's GitHub token (op://vault/item/field), saved to .devcontainer/.env without prompting
      --op-signing-key-ref <OP_REF>  1Password reference for the devcontainer's SSH signing key (with --op-github-ref)
      --skip-1password               Do not use 1Password for devcontainer credentials (recorded in .devcontainer/.env)
      --no-verify-op-refs            Save the --op-* references without checking them with `op read`
      --json                         Emit the summary as JSON (implies --yes: nothing is asked)
  -h, --help                         Print help
```

## branchbox devcontainer

```text
Manage devcontainer configuration

Usage: branchbox devcontainer <COMMAND>

Commands:
  up                  Create and run dev container
  exec                Execute a command on a running dev container
  down                Stop and remove dev container
  build               Build a dev container image
  read-configuration  Read and output devcontainer configuration
  sync                Sync devcontainer configuration to all feature worktrees
  configure           Configure devcontainer workspace settings for worktree compatibility
  detect              Detect main service, container user, and ports from devcontainer
  add-tunnel          Add cloudflared tunnel service to compose file
  inject-agents       Inject AI coding agent volume mounts into compose file
  help                Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox devcontainer up

```text
Create and run dev container

Usage: branchbox devcontainer up [OPTIONS] [WORKSPACE_FOLDER]

Arguments:
  [WORKSPACE_FOLDER]  Workspace folder path (defaults to current directory) [default: .]

Options:
      --docker-path <DOCKER_PATH>
          Docker CLI path
      --docker-compose-path <DOCKER_COMPOSE_PATH>
          Docker Compose CLI path
      --remove-existing-container
          Remove existing container before starting
      --build-no-cache
          Build with --no-cache
      --skip-post-create
          Skip post-create commands
      --remote-env <REMOTE_ENV>
          Remote environment variables for lifecycle commands (name=value)
      --json
          Output as JSON
  -h, --help
          Print help
```

## branchbox devcontainer exec

```text
Execute a command on a running dev container

Usage: branchbox devcontainer exec [OPTIONS] -- <CMD>...

Arguments:
  <CMD>...  Command to execute

Options:
  -w, --workspace-folder <WORKSPACE_FOLDER>
          Workspace folder path (defaults to current directory) [default: .]
      --docker-path <DOCKER_PATH>
          Docker CLI path
      --docker-compose-path <DOCKER_COMPOSE_PATH>
          Docker Compose CLI path
  -u, --user <USER>
          User to run command as
      --workdir <WORKDIR>
          Working directory in container
      --remote-env <REMOTE_ENV>
          Remote environment variables for this command (name=value; overrides remoteEnv)
      --json
          Output as JSON
  -h, --help
          Print help
```

## branchbox devcontainer down

```text
Stop and remove dev container

Usage: branchbox devcontainer down [OPTIONS] [WORKSPACE_FOLDER]

Arguments:
  [WORKSPACE_FOLDER]  Workspace folder path (defaults to current directory) [default: .]

Options:
      --docker-path <DOCKER_PATH>                  Docker CLI path
      --docker-compose-path <DOCKER_COMPOSE_PATH>  Docker Compose CLI path
  -v, --volumes                                    Remove volumes
      --remove-orphans                             Remove orphan containers
      --json                                       Output as JSON
  -h, --help                                       Print help
```

## branchbox devcontainer build

```text
Build a dev container image

Usage: branchbox devcontainer build [OPTIONS] [WORKSPACE_FOLDER]

Arguments:
  [WORKSPACE_FOLDER]  Workspace folder path (defaults to current directory) [default: .]

Options:
      --docker-path <DOCKER_PATH>                  Docker CLI path
      --docker-compose-path <DOCKER_COMPOSE_PATH>  Docker Compose CLI path
      --no-cache                                   Build with --no-cache
      --image-name <IMAGE_NAME>                    Image name (for Dockerfile builds)
      --json                                       Output as JSON
  -h, --help                                       Print help
```

## branchbox devcontainer read-configuration

```text
Read and output devcontainer configuration

Usage: branchbox devcontainer read-configuration [OPTIONS] [WORKSPACE_FOLDER]

Arguments:
  [WORKSPACE_FOLDER]  Workspace folder path (defaults to current directory) [default: .]

Options:
      --json  Output as JSON (always JSON for this command)
  -h, --help  Print help
```

## branchbox devcontainer sync

With `--json`, the per-worktree results are printed as one document. A failed worktree makes the command exit 1, in text and JSON mode alike. A sync with worktrees to update refuses with `devcontainer_source_missing` when the main worktree has no `.devcontainer/`; `--dry-run` does not need it.

```text
Sync devcontainer configuration to all feature worktrees

Usage: branchbox devcontainer sync [OPTIONS]

Options:
  -p, --path <PATH>          Project directory (defaults to current directory)
  -s, --strategy <STRATEGY>  Sync strategy (copy or symlink)
  -n, --dry-run              Dry run - show what would be synced without making changes
      --feature <NAME>       Sync only this feature (repeatable). Any registered feature that has not been removed can be named; without it, every active feature is synced
      --json                 Emit the per-worktree results as JSON
  -h, --help                 Print help
```

## branchbox devcontainer configure

```text
Configure devcontainer workspace settings for worktree compatibility

Usage: branchbox devcontainer configure [OPTIONS]

Options:
  -p, --path <PATH>  Project directory (defaults to current directory)
      --json         Output as JSON
  -h, --help         Print help
```

## branchbox devcontainer detect

```text
Detect main service, container user, and ports from devcontainer

Usage: branchbox devcontainer detect [OPTIONS]

Options:
  -p, --path <PATH>    Project directory (defaults to current directory)
  -s, --stack <STACK>  Stack hint for port detection (e.g., flask, rails, node)
      --json           Output as JSON
  -h, --help           Print help
```

## branchbox devcontainer add-tunnel

```text
Add cloudflared tunnel service to compose file

Usage: branchbox devcontainer add-tunnel [OPTIONS]

Options:
  -p, --path <PATH>        Project directory (defaults to current directory)
  -s, --service <SERVICE>  Main service name to add depends_on (auto-detected if not specified)
      --json               Output as JSON
  -h, --help               Print help
```

## branchbox devcontainer inject-agents

```text
Inject AI coding agent volume mounts into compose file

Usage: branchbox devcontainer inject-agents [OPTIONS]

Options:
  -p, --path <PATH>  Project directory (defaults to current directory)
      --json         Output as JSON
  -h, --help         Print help
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

`--json` prints the detected stack, adapter and modules plus whether the folder is a git repository and already initialized.

```text
Detect project configuration

Usage: branchbox detect [OPTIONS]

Options:
  -p, --path <PATH>  Project directory (defaults to current directory)
      --json         Emit JSON output instead of human-readable text
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

Alias: `branchbox features`

```text
Manage feature worktrees

Usage: branchbox feature <COMMAND>

Commands:
  start          Create a new feature worktree and run module setup
  teardown       Tear down an existing feature worktree
  list           List known feature worktrees from the registry
  prune          Tear down all active feature worktrees
  exec           Execute a command through a feature's runtime provider
  exec-provider  Execute an allowlisted coding provider with name-only environment inheritance
  dispatch-tool  Dispatch one capability-bound request to a trusted managed tool endpoint
  help           Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox feature start

Alias: `branchbox feature new`

```text
Create a new feature worktree and run module setup

Usage: branchbox feature start [OPTIONS] [NAME]

Arguments:
  [NAME]
          Dasherized feature name (e.g., oauth-integration)

Options:
      --title <TITLE>
          Free-form feature title (converted to dasherized name)

      --base <BASE>
          Base branch to branch from (defaults to current HEAD)

      --branch-prefix <BRANCH_PREFIX>
          Override branch prefix (defaults to "feature")

      --repo <REPO>
          Repository path (defaults to current directory)

      --reuse
          Allow reusing an existing worktree directory

      --no-worktree
          Check the feature branch out in the repository instead of a worktree.

          A worktree lets several features share one clone. A caller that clones per run and discards the clone has nothing to share it with, and pays for the second checkout identity anyway.

      --devcontainer-reuse <POLICY>
          Copy-mode conflict policy when reusing a worktree (fail, preserve, overwrite, inspect)

          [default: fail]

      --keep-runtime-on-failure
          Retain a failed SBX runtime and its build cache for inspection or retry

      --reuse-runtime
          Reuse a retained runtime (implies --reuse and --keep-runtime-on-failure)

      --telemetry
          Emit verbose telemetry (e.g. Cloudflare operations)

      --skip-module <MODULE>
          Skip specific modules during setup (can be specified multiple times) Available modules: compose, database, tunnel, specs

      --minimal
          Start feature workflow in minimal mode (skips heavyweight modules)

      --prompt <PROMPT>
          Provide an optional prompt seed for automation/agent hand-off

      --default-prompt
          Use the default minimal-mode prompt shortcut (only valid with --minimal/--fast)

      --json
          Emit JSON summary payload instead of human-readable text

      --allow-container
          Allow running feature start from inside a containerized environment

      --no-summary
          Suppress summary output (text mode only)

      --runtime <PROVIDER>
          Workspace isolation runtime (container, sbx, or Linux/KVM local-vm)

      --runtime-manifest <PATH>
          Absolute supervisor-authored assignment manifest (required by --runtime in-guest)

  -h, --help
          Print help (see a summary with '-h')
```

## branchbox feature teardown

Teardown checks reported Git changes and the branch's merge state before removal:

- When the worktree has changes of yours (modified, staged or untracked files), teardown refuses **before anything is removed**. The refusal lists the files and names `--discard-changes`.
- When the branch would be deleted but has commits that are not merged, teardown refuses the same way and names `--keep-branch` and `--force-delete-branch`. In a terminal it asks first when `feature.teardown.prompt_force_delete_unmerged` is on, and `feature.teardown.force_delete_unmerged_by_default` force-deletes without asking.
- `--discard-changes` discards the worktree's uncommitted changes. It does not force-delete the branch.
- `--dry-run` prints the teardown plan (with `--json`, the plan document) and changes nothing. It accepts the same flags as a real teardown, so you can preview exactly what a command would do.

Files BranchBox generated itself (such as `.devcontainer/.branchbox.env`) never count as your changes. Files git ignores are removed with the worktree and are not listed. The feature spec is moved back to the main worktree's `docs/features/backlog/` (or `completed/` with `--complete-spec`).

`--force` keeps its earlier meaning for compatibility: it removes the worktree whatever its state, and when the branch is being deleted it uses `git branch -D`, so unmerged commits go too (the summary then warns with the commit count). Prefer `--discard-changes`.

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
      --force                          Remove the worktree whatever its state: discard uncommitted changes, remove a locked or unreadable worktree (deleting the directory if git cannot), and force-delete the branch (`git branch -D`, even with unmerged commits) when deleting it. Prefer --discard-changes, which keeps unmerged commits safe
      --force-delete-branch            Force-delete the git branch even if it is not fully merged (`git branch -D`)
      --discard-changes                Discard the worktree's uncommitted changes (modified and untracked files) instead of refusing. Does not force-delete the branch
      --dry-run                        Print what teardown would do (the teardown plan) and change nothing
      --complete-spec                  Move spec to completed during teardown
      --telemetry                      Emit verbose telemetry (e.g. Cloudflare operations)
      --allow-container                Allow running feature teardown from inside a containerized environment
      --json                           Emit deterministic teardown and residue evidence as JSON (with --dry-run, the plan)
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

## branchbox feature prune

Prune is the bulk cleanup command, and it is destructive: it discards each feature's uncommitted changes and, when it deletes branches, force-deletes them, unmerged commits included. A feature whose teardown fails is reported and the rest continue; any failure makes the command exit 1. Run `--dry-run` first: it lists the uncommitted changes and unmerged commits that would be lost. Without a terminal, or with `--json`, prune refuses with `confirmation_required` unless you pass `--yes`.

To remove features one at a time with the safety checks, use `branchbox feature teardown` (the Mac app's Prune does exactly that, and never runs this command).

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
      --feature <NAME>   Prune only this feature (repeatable); by default every active or retained feature
      --json             Emit the dry-run candidates, or the prune results, as JSON
  -h, --help             Print help
```

## branchbox feature exec

Use `--` before the command when it has its own flags. With `--json`, a failing inner command still prints its `{exit_code, stdout, stderr}` payload, and `branchbox` exits 1.

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

## branchbox feature exec-provider

```text
Execute an allowlisted coding provider with name-only environment inheritance

Usage: branchbox feature exec-provider [OPTIONS] --provider <EXECUTABLE> <NAME> [COMMAND]...

Arguments:
  <NAME>        Dasherized feature name
  [COMMAND]...  Arguments passed to the fixed provider executable

Options:
      --repo <REPO>            Repository path (defaults to current directory)
      --provider <EXECUTABLE>  Exact provider executable declared by the managed runtime assignment
      --inherit-env <NAME>     Allowlisted environment name to inherit without transporting its value
  -h, --help                   Print help
```

## branchbox feature dispatch-tool

The dispatcher can run concurrently with the coding provider. Exit status `75` means the exact
atomic request file was not present before `--wait-seconds` expired; every malformed or failed
request is a terminal non-75 error. With `--json`, both success and not-pending outcomes are
structured for an orchestrator.

```text
Dispatch one capability-bound request to a trusted managed tool endpoint

Usage: branchbox feature dispatch-tool [OPTIONS] --lease <LEASE_ID> --request-id <REQUEST_ID> <NAME>

Arguments:
  <NAME>  Dasherized feature name

Options:
      --repo <REPO>              Repository path (defaults to current directory)
      --lease <LEASE_ID>         Exact tool-request lease declared by the managed runtime assignment
      --request-id <REQUEST_ID>  Exact request identifier and spool filename stem
      --json                     Emit the correlated response as JSON
      --wait-seconds <SECONDS>   Wait up to this many seconds for the atomic request file (maximum 300) [default: 0]
  -h, --help                     Print help
```

## branchbox prune

Same as `branchbox feature prune`.

```text
Tear down all active feature worktrees

Usage: branchbox prune [OPTIONS]

Options:
      --repo <REPO>      Repository path (defaults to current directory)
      --dry-run          Show which features would be removed without applying teardown
  -y, --yes              Skip confirmation prompt
      --keep-branch      Keep git branches after removing worktrees
      --delete-branch    Delete git branches after removing worktrees
      --complete-spec    Move specs to completed during teardown
      --telemetry        Emit verbose telemetry (e.g. Cloudflare operations)
      --allow-container  Allow running feature prune from inside a containerized environment
      --feature <NAME>   Prune only this feature (repeatable); by default every active or retained feature
      --json             Emit the dry-run candidates, or the prune results, as JSON
  -h, --help             Print help
```

## branchbox tunnel

```text
Manage tunnels for existing features

Usage: branchbox tunnel <COMMAND>

Commands:
  open         Provision (or re-provision) a tunnel for an existing feature
  remove       Remove tunnel metadata and attempt provider teardown
  credentials  Manage the Cloudflare API credentials used to provision tunnels
  help         Print this message or the help of the given subcommand(s)

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

## branchbox tunnel credentials

```text
Manage the Cloudflare API credentials used to provision tunnels

Usage: branchbox tunnel credentials <COMMAND>

Commands:
  set   Store the Cloudflare account ID and API token (.branchbox/secure/cloudflared.env, owner-only) and enable automatic tunnel provisioning
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox tunnel credentials set

The token is read from standard input only (`--api-token-stdin`), so it never appears in your shell history or the process list. It is written to `.branchbox/secure/cloudflared.env` with mode 0600, and the project config is pointed at it. An empty token is refused and leaves the existing file untouched. `--clear` removes the stored token and switches tunnels back to manual setup instructions.

```bash
printf '%s' "$CLOUDFLARE_API_TOKEN" | branchbox tunnel credentials set --account-id "$ACCOUNT_ID" --api-token-stdin
```

```text
Store the Cloudflare account ID and API token (.branchbox/secure/cloudflared.env, owner-only) and enable automatic tunnel provisioning

Usage: branchbox tunnel credentials set [OPTIONS]

Options:
      --account-id <ID>  Cloudflare account ID
      --api-token-stdin  Read the API token from standard input (it is never accepted as an argument)
      --clear            Remove the stored API token; tunnels fall back to manual setup instructions
      --repo <REPO>      Repository path (defaults to current directory)
      --json             Emit JSON output instead of human-readable summary
  -h, --help             Print help
```

## branchbox version

`branchbox version --json` prints `{version, contract_version, capabilities[]}`. Tools such as the Mac app use it to find out which JSON features this CLI supports; see the [JSON contract](./json-contract.md#capabilities). CLIs older than this command exit 2 on it, and `branchbox --version` still works everywhere.

```text
Show the BranchBox version and, with --json, its contract capabilities

Usage: branchbox version [OPTIONS]

Options:
      --json  Print the version, contract version and capabilities as JSON
  -h, --help  Print help
```

## branchbox doctor

Doctor checks git, Docker (CLI, daemon and Compose), the Dev Container CLI, the optional runtimes and tools (sbx, local-vm, `op`, `gh`) and your `PATH`. With `--repo`, it also checks the repository's git state, initialization, config, registry and `.gitignore`. Each check takes at most about 3 seconds. The command exits 1 when a required check fails; with `--json` the report is printed either way.

```text
Check the host (and optionally a repository) for BranchBox prerequisites

Usage: branchbox doctor [OPTIONS]

Options:
      --repo <REPO>  Also check this repository (git, initialization, config, registry, .gitignore)
      --check-auth   Also check that the tools that need credentials are signed in
      --json         Emit the check results as JSON
  -h, --help         Print help
```

## branchbox config

The settings live in `.branchbox/config.json`. See the [configuration reference](./configuration.md) for every key. Writes keep the file's formatting and unknown keys, are atomic, and refuse a file with comments.

```text
Read and change project configuration (.branchbox/config.json)

Usage: branchbox config <COMMAND>

Commands:
  get    Show the effective configuration (or one key) with defaults and sources
  set    Set a configuration key
  unset  Remove a configuration key so its default applies again
  apply  Apply a JSON merge patch (RFC 7386) to the configuration; `null` unsets a key
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## branchbox config get

```text
Show the effective configuration (or one key) with defaults and sources

Usage: branchbox config get [OPTIONS] [KEY]

Arguments:
  [KEY]  Dotted key to show (e.g. runtime.provider); omit to show every key

Options:
      --repo <REPO>  Repository path (defaults to current directory)
      --json         Emit JSON output instead of human-readable text
  -h, --help         Print help
```

## branchbox config set

```text
Set a configuration key

Usage: branchbox config set [OPTIONS] <KEY> <VALUE>

Arguments:
  <KEY>    Dotted key to set (e.g. feature.branch_prefix)
  <VALUE>  New value (parsed as the key's type)

Options:
      --repo <REPO>  Repository path (defaults to current directory)
  -h, --help         Print help
```

## branchbox config unset

```text
Remove a configuration key so its default applies again

Usage: branchbox config unset [OPTIONS] <KEY>

Arguments:
  <KEY>  Dotted key to remove (e.g. feature.branch_prefix)

Options:
      --repo <REPO>  Repository path (defaults to current directory)
  -h, --help         Print help
```

## branchbox config apply

```bash
echo '{"feature": {"branch_prefix": "spike"}, "tunnel": {"enabled": null}}' | branchbox config apply --file - --json
```

An unknown key is refused with `config_unknown_key`, and an invalid value with `config_invalid`, which names the key and the values it accepts.

```text
Apply a JSON merge patch (RFC 7386) to the configuration; `null` unsets a key

Usage: branchbox config apply [OPTIONS] --file <PATH>

Options:
      --file <PATH>  Merge patch to apply: a JSON file, or `-` to read it from stdin
      --dry-run      Report what would change without writing the file
      --repo <REPO>  Repository path (defaults to current directory)
      --json         Emit JSON output instead of human-readable text
  -h, --help         Print help
```
