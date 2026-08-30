# BranchBox Devcontainer Gotchas

## 1Password + Docker Desktop (macOS)

| Symptom | Root cause | Correct pattern |
|---|---|---|
| SSH agent socket is mounted but `ssh-add -l` fails with `Connection refused` | Docker Desktop file sharing does not forward host Unix socket protocols in this flow | Use host `op read` in `initializeCommand`, mount token/key files, configure git in container |
| Compose fails before container starts due to missing mounted files | Bind mount target files do not exist on first run | `touch` mount targets in host init script before compose startup |
| Existing auth/signing suddenly disappears after transient 1Password outage | Secret file is truncated before replacement (`: > file`) and `op read` fails | Only replace files after successful read (write temp + `mv`) |
| Existing auth/signing disappears even when `op read` exits 0 | Returned secret is empty/whitespace and gets written as a blank file | Treat empty/whitespace reads as failures for rotation; keep existing file |
| Secret briefly readable with weak mode during update | Temp file created with default umask then chmodded later | Write temp files inside `umask 077`, then atomically move |
| Private key can be read by other host users | Host-side key file written with broad mode (for example `0644`) | Enforce `chmod 600` for token/signing files |
| `source .gitconfig.env` breaks on names with spaces | Shell parsing of unquoted values is fragile | Parse env-like files with `grep/cut`, set explicit git config keys |
| Credential helper becomes injection sink | Raw token value interpolated into persisted shell snippet | Use env reference in helper (`password=$GH_TOKEN`), export token at runtime |
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
| Compose service names diverge between runtime env and generated files | Raw compose name set in process env but sanitized value written later | Sanitize once, then reuse everywhere (`COMPOSE_PROJECT_NAME` + `DEVCONTAINER_NAME`) |
| Compose startup fails on stricter implementations | Sanitizer allows unsupported chars (for example `.`) | Keep compose project name allow-list aligned to Compose-safe charset (`[a-z0-9_-]`) |
| Generated `.env` gains unexpected extra variables | Unsanitized newline/carriage return from untrusted source value | Strip `\n`/`\r` before writing env values (for example `APP_URL`) |
| Generated env can still carry shell metacharacters in branch values | `GIT_BRANCH` sanitizer only strips control chars | Use explicit allow-list for branch env output (`[A-Za-z0-9._/-]`) |
| Harness resolves wrong service from devcontainer config | JSONC comments break naive JSON parsing | Strip comments or fallback to compose service detection |
| Harness works on one machine but not another | Script assumes plugin compose only | Detect and support both `docker compose` and `docker-compose` |
| Harness fixes land in one script but regress in another | Shared helper logic duplicated across multiple scripts | Extract helper functions to `scripts/lib/*.sh` and source from harnesses |
| Failed in-guest `postCreate` leaves dependency services and worktree behind | Provider ownership is recorded only after successful `devcontainer up` | Pre-record workspace/Compose/proxy identity, recover exact label ownership on error, and remove the failed worktree/task branch |
| No-registry in-guest teardown invokes Cloudflare/database/Compose modules | Teardown defaults missing registry runtime metadata to the host container provider | Recover owner-only in-guest provider state by exact worktree path and bypass every repository module/adapter |
| Project environment changes `$` or leaks into the facade | Ordinary/double-quoted dotenv interpolates values or configuration serializes assignments | Require Compose 2.30+ raw primary-service env files, canonical sorted single-line materialization, and path/digest-only evidence |

## Documentation drift traps

- Update script docs and docs-site docs in the same change (`scripts/manual-*.md` and `docs/docs/getting-started/manual-*.md`).
- Keep paired docs as plain files and enforce sync with `diff`; avoid symlink-only solutions that add cross-platform friction.
- Add release-impacting behavior changes to `CHANGELOG.md` before PR handoff.
- Add project-level guardrails to `AGENTS.md` when new patterns become mandatory.
