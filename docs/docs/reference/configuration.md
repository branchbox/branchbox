---
sidebar_position: 3
---

<!-- Generated from the key registry in core/src/config_edit.rs. Do not edit by hand: run `UPDATE_CONFIG_REFERENCE=1 cargo test -p worktree-core config_edit` to regenerate. -->

# Configuration Reference

Each project keeps its BranchBox settings in `.branchbox/config.json` at the repository root. Every setting is optional: a key the file does not set takes its default.

Read and change the settings with `branchbox config`:

- `branchbox config get [KEY] [--json]` shows the effective value of every key (or one key), its default and whether the file sets it.
- `branchbox config set KEY VALUE` sets a key. Booleans are `true` or `false`; a list is comma-separated (`web,worker`) or a JSON array.
- `branchbox config unset KEY` removes a key, so its default applies again.
- `branchbox config apply --file PATCH.json [--dry-run] [--json]` applies a JSON merge patch (RFC 7386); `--file -` reads it from standard input. `null` unsets a key.

Changes are checked before anything is written: an unknown key or an invalid value is refused, naming the key and the accepted values, and the file is left as it was. Edits keep the file's formatting, its permissions and any keys BranchBox does not know. The file must be strict JSON: comments and trailing commas are refused with their line and column.

## Keys

| Key | Type | Default | Accepted values |
|---|---|---|---|
| [`runtime.provider`](#runtimeprovider) | enum | `"container"` | one of: container, sbx, local-vm, in-guest |
| [`runtime.sbx.run_services`](#runtimesbxrun_services) | string_list | `[]` | a list of distinct Compose service names (letters, digits, '.', '_' and '-') |
| [`feature.branch_prefix`](#featurebranch_prefix) | string | `"feature"` | a branch prefix such that '\<prefix\>/\<feature\>' is a valid git branch name |
| [`feature.teardown.delete_branch_by_default`](#featureteardowndelete_branch_by_default) | bool | `true` | true or false |
| [`feature.teardown.force_delete_unmerged_by_default`](#featureteardownforce_delete_unmerged_by_default) | bool | `false` | true or false |
| [`feature.teardown.prompt_force_delete_unmerged`](#featureteardownprompt_force_delete_unmerged) | bool | `true` | true or false |
| [`tunnel.enabled`](#tunnelenabled) | bool | `true` | true or false |
| [`tunnel.default_provider`](#tunneldefault_provider) | enum | `"cloudflared"` | one of: cloudflared |
| [`tunnel.providers.cloudflared.account_id`](#tunnelproviderscloudflaredaccount_id) | string | `null` | a Cloudflare account ID (letters, digits, '-' and '_') or $CLOUDFLARE_ACCOUNT_ID |
| [`tunnel.providers.cloudflared.tunnel_name_prefix`](#tunnelproviderscloudflaredtunnel_name_prefix) | string | `"branchbox"` | letters, digits and '-' |
| [`tunnel.providers.cloudflared.dns_zone`](#tunnelproviderscloudflareddns_zone) | string | `null` | a DNS zone such as example.com |
| [`tunnel.providers.cloudflared.service_url`](#tunnelproviderscloudflaredservice_url) | string | `null` | a non-empty string on one line |
| [`tunnel.providers.cloudflared.manual_instructions`](#tunnelproviderscloudflaredmanual_instructions) | bool | `false` | true or false |
| [`tunnel.providers.cloudflared.api_token_path`](#tunnelproviderscloudflaredapi_token_path) | string | `null` | a non-empty string on one line |
| [`editor.default_agent`](#editordefault_agent) | string | `null` | a non-empty string on one line |
| [`editor.auto_launch_agent_terminal`](#editorauto_launch_agent_terminal) | bool | `false` | true or false |
| [`editor.preferred_sidebar_view`](#editorpreferred_sidebar_view) | string | `null` | a non-empty string on one line |
| [`editor.hide_secondary_sidebar`](#editorhide_secondary_sidebar) | bool | `false` | true or false |

### `runtime.provider`

Runtime used when `feature start` gets no `--runtime`: `container` (Docker on the host), `sbx` (Docker Sandboxes microVMs), `local-vm` (local microVM driver) or `in-guest` (a devcontainer inside an isolation boundary the caller owns).

- Type: enum
- Default: `"container"`
- Accepted values: one of: container, sbx, local-vm, in-guest

### `runtime.sbx.run_services`

Compose services a devcontainer starts inside Docker Sandboxes. Compose still starts their declared dependencies; other host-integrated sidecars stay stopped.

- Type: string_list
- Default: `[]`
- Accepted values: a list of distinct Compose service names (letters, digits, '.', '_' and '-')

### `feature.branch_prefix`

Prefix of the branch a new feature gets: feature `eta` starts on `<prefix>/eta`.

- Type: string
- Default: `"feature"`
- Accepted values: a branch prefix such that '\<prefix\>/\<feature\>' is a valid git branch name

### `feature.teardown.delete_branch_by_default`

Delete the feature branch when the feature is torn down (`--keep-branch` overrides it for one teardown).

- Type: bool
- Default: `true`
- Accepted values: true or false

### `feature.teardown.force_delete_unmerged_by_default`

Delete a feature branch that has unmerged commits (`git branch -D`) without asking. When false, an unmerged branch is kept or the teardown is refused, depending on whether a prompt is possible.

- Type: bool
- Default: `false`
- Accepted values: true or false

### `feature.teardown.prompt_force_delete_unmerged`

In an interactive terminal, ask before force-deleting a branch with unmerged commits.

- Type: bool
- Default: `true`
- Accepted values: true or false

### `tunnel.enabled`

Provision a public tunnel for each feature.

- Type: bool
- Default: `true`
- Accepted values: true or false

### `tunnel.default_provider`

Tunnel provider for new tunnels.

- Type: enum
- Default: `"cloudflared"`
- Accepted values: one of: cloudflared

### `tunnel.providers.cloudflared.account_id`

Cloudflare account that owns the tunnels, or `$CLOUDFLARE_ACCOUNT_ID` to read it from the environment. `branchbox tunnel credentials set` sets it together with the API token.

- Type: string
- Default: `null`
- Accepted values: a Cloudflare account ID (letters, digits, '-' and '_') or $CLOUDFLARE_ACCOUNT_ID

### `tunnel.providers.cloudflared.tunnel_name_prefix`

Prefix of tunnel names and, with `dns_zone`, of feature hostnames (`<prefix>-<feature>.<dns_zone>`).

- Type: string
- Default: `"branchbox"`
- Accepted values: letters, digits and '-'

### `tunnel.providers.cloudflared.dns_zone`

Cloudflare DNS zone (for example `example.com`) in which tunnel hostnames are created. When unset, the zone is derived from the feature hostname.

- Type: string
- Default: `null`
- Accepted values: a DNS zone such as example.com

### `tunnel.providers.cloudflared.service_url`

Service the tunnel forwards to (for example `http://app:5001`). When unset, the project's adapter decides.

- Type: string
- Default: `null`
- Accepted values: a non-empty string on one line

### `tunnel.providers.cloudflared.manual_instructions`

Print manual tunnel setup steps instead of provisioning through the Cloudflare API.

- Type: bool
- Default: `false`
- Accepted values: true or false

### `tunnel.providers.cloudflared.api_token_path`

File holding `CLOUDFLARE_API_TOKEN`, relative to the repository root. `branchbox tunnel credentials set` writes `.branchbox/secure/cloudflared.env` (owner-only) and sets this key.

- Type: string
- Default: `null`
- Accepted values: a non-empty string on one line

### `editor.default_agent`

Coding agent the editor integration prefers (for example `codex` or `claude`).

- Type: string
- Default: `null`
- Accepted values: a non-empty string on one line

### `editor.auto_launch_agent_terminal`

Open a terminal running the default agent when the editor attaches to a feature.

- Type: bool
- Default: `false`
- Accepted values: true or false

### `editor.preferred_sidebar_view`

Editor view to focus on attach (for example `workbench.view.scm`).

- Type: string
- Default: `null`
- Accepted values: a non-empty string on one line

### `editor.hide_secondary_sidebar`

Hide the editor's secondary (right) sidebar on attach.

- Type: bool
- Default: `false`
- Accepted values: true or false
