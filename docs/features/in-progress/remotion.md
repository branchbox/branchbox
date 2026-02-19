---

branch: feature/remotion
created: 2026-02-17
status: in-progress
work_feature: remotion
worktree: /Users/rbarazi/projects/branchbox-suite/branchbox/remotion
---
# Remotion

## Overview

Add an in-repo Remotion demo package and wrapper script so BranchBox teaser videos can be rendered inside the devcontainer without relying on live screen recording.

## Scope

- Add `demos/remotion/` with a deterministic teaser composition.
- Add `scripts/remotion-demo.sh` helper to install deps, ensure browser binaries, and render MP4 or launch studio.
- Document usage in `README.md` and `docs/TEASER_SCRIPT.md`.
- Replace placeholder terminal text with curated real CLI output captured from `scripts/demo-teaser.sh` across supported stacks.
- Add docs/website publishing pipeline (`scripts/remotion-docs-assets.sh`) and per-section wrappers to render and publish documentation-ready cuts + manifest metadata.
