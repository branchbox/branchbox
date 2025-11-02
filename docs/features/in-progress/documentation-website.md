---
work_feature: documentation-website
status: in-progress
created: 2025-11-02
updated: 2025-11-02
owner: codex-agent
---

# Documentation Website Rollout

## Goal
Deliver a maintainable, mdBook-powered documentation site for BranchBox, complete with automated GitHub Pages deployment and contributor guidelines woven into the engineering workflow.

## Tasks
- [x] Audit existing documentation assets and CI automation to understand current state (2025-11-02)
- [x] Select mdBook structure (`docs/book.toml`, `docs/src/`) and migrate core guides (2025-11-02)
- [x] Add CLI reference generation script that captures `branchbox --help` output (2025-11-02)
- [x] Wire `mdbook build docs` into CI for PR validation (2025-11-02)
- [x] Create GitHub Pages deployment workflow targeting `gh-pages` (2025-11-02)
- [x] Update contributor docs (`DEVELOPMENT.md`, `README.md`) with new workflow details (2025-11-02)
- [ ] Verify local and CI builds, then announce rollout in CHANGELOG

## Progress Log
- 2025-11-02: Documented current docs + automation landscape and committed to mdBook + GitHub Pages approach. Added workflow expectations to `AGENTS.md` for engineers and coding agents.
- 2025-11-02: Laid out the execution checklist above so each step can ship independently without blocking feature work.
- 2025-11-02: Scaffolded mdBook (`docs/book.toml`, `docs/src/`) with includes for existing installation, development, and architecture guides to keep the site skeleton building cleanly.
- 2025-11-02: Added `docs/scripts/render-cli-reference.sh` to generate CLI documentation automatically and wired placeholder page awaiting generated content.
- 2025-11-02: Attempted local `mdbook build docs`, but `mdbook` binary is not yet installed in the environment; will rely on upcoming CI integration once toolchain is available.
- 2025-11-02: Updated `ci.yml` to install mdBook and run `mdbook build docs` so PRs catch static site issues alongside Rustdoc checks.
- 2025-11-02: Added `docs-deploy.yml` workflow to build the book and publish `docs/book` to GitHub Pages on `main`.
- 2025-11-02: Ran CLI reference generator and refreshed README/DEVELOPMENT docs to point contributors at the mdBook tooling and online site.
- 2025-11-02: Ran `cargo fmt --all -- --check` as a sanity check; will run `mdbook build docs` once the tool is available locally before closing out the feature.
