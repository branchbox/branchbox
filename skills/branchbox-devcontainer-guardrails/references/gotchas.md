# BranchBox Devcontainer Gotchas

## 1Password + Docker Desktop (macOS)

| Symptom | Root cause | Correct pattern |
|---|---|---|
| SSH agent socket is mounted but `ssh-add -l` fails with `Connection refused` | Docker Desktop file sharing does not forward host Unix socket protocols in this flow | Use host `op read` in `initializeCommand`, mount token/key files, configure git in container |
| Compose fails before container starts due to missing mounted files | Bind mount target files do not exist on first run | `touch` mount targets in host init script before compose startup |
| `source .gitconfig.env` breaks on names with spaces | Shell parsing of unquoted values is fragile | Parse env-like files with `grep/cut`, set explicit git config keys |
| Signing key exists but commit signing fails | Key used directly from read-only mount or with weak permissions | Copy key to `~/.ssh`, set `chmod 600`, then configure `git config user.signingkey` to copied path |
| `gh` login/token auth is inconsistent in non-interactive flows | Interactive/device flow behavior varies | Prefer `GH_TOKEN` from mounted token env file |

## Compose + worktree isolation

| Symptom | Root cause | Correct pattern |
|---|---|---|
| Container name collisions across worktrees | Compose templates set fixed `name`/`container_name` | Do not pin project/container names in templates |
| Compose command works on one host but fails on another | Plugin-style compose unavailable | Support `docker compose` with `docker-compose` fallback |

## Feature workflow + harness reliability

| Symptom | Root cause | Correct pattern |
|---|---|---|
| False stash warnings during `feature start` | Untracked files included in stash behavior | Ignore untracked files for stash capture/apply and use explicit stash reference |
| Compose project identity missing when `.env` is absent | Feature env provisioning tied to source `.env` existence | Always compute/write compose/devcontainer identity in `.devcontainer/.branchbox.env` |
| Harness resolves wrong service from devcontainer config | JSONC comments break naive JSON parsing | Strip comments or fallback to compose service detection |

## Documentation drift traps

- Update script docs and docs-site docs in the same change (`scripts/manual-*.md` and `docs/docs/getting-started/manual-*.md`).
- Add release-impacting behavior changes to `CHANGELOG.md` before PR handoff.
- Add project-level guardrails to `AGENTS.md` when new patterns become mandatory.
