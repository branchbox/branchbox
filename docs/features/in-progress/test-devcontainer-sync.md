---

branch: feature/test-devcontainer-sync
created: 2025-10-30
status: in-progress
work_feature: test-devcontainer-sync
worktree: /workspaces/test-devcontainer-sync
---
# Test Devcontainer Sync

## Overview

- Validate that the new `DevcontainerModule` copies `.devcontainer/` into feature worktrees.
- Ensure shared credentials (`.gh`, `.claude`, `.codex`) stay mounted after sync.
- Exercise the CLI flow (`branchbox devcontainer sync`) and manual VS Code reopen path.

## Tasks

- [ ] Spin up main repo in devcontainer.
- [ ] Run `branchbox feature start "test-devcontainer-sync"` and confirm `.devcontainer/` present.
- [ ] Authenticate `gh`, `claude`, `codex` once; verify availability inside feature container.
- [ ] Update main `.devcontainer/compose.yaml`; run `branchbox devcontainer sync --dry-run` to inspect diffs.
- [ ] Execute `branchbox devcontainer sync` and confirm feature worktree picks up changes.
- [ ] Capture screenshots / CLI transcripts for onboarding docs.
- [ ] File follow-up issues for any gaps discovered.
