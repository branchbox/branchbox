---
work_feature: cloudflared-tunnel-integration
status: in-progress
created: 2025-11-02
updated: 2025-11-02
---

# Cloudflared Tunnel Integration

## Overview

Introduce first-class tunnel provisioning through Cloudflare so every BranchBox feature worktree receives an addressable HTTPS endpoint by default. This spec tracks the incremental migration from the legacy shell scripts to the Rust workflow, keeping the door open for additional providers (ngrok, localhost.run) later on.

## Background

- Legacy automation for tunnels still lives under `legacy/` alongside other shell-based workflows.
- Milestone 0 delivered feature start/teardown orchestration in Rust but left tunnel setup as a manual step.
- Core design principle: offline-first with optional automation when credentials exist. We must surface clear guidance when auto-provisioning cannot run.

## Goals

1. Persist project-level tunnel preferences and credentials gathered during `branchbox init`.
2. Provide a provider abstraction that supports Cloudflared now and additional vendors in the future.
3. Extend feature start/teardown to provision and destroy tunnels automatically when enabled, while allowing per-invocation overrides.
4. Expose CLI affordances to inspect tunnels (`branchbox tunnel ...`) and surface tunnel status alongside `branchbox feature list`.
5. Document setup, troubleshooting, and manual fallbacks.

## Non-Goals

- Implement provider-specific UI beyond the CLI.
- Ship additional tunnel providers in this milestone (stubs acceptable for future work).
- Replace the existing devcontainer networking; only augment with tunneling.

## Work Plan

- [x] Establish tunnel configuration schema and `branchbox init` prompts (includes persistence, validation, and documentation updates).
- [x] Add tunnel provider trait with a Cloudflared-backed implementation plus manual fallback messaging.
- [x] Extend `FeatureStateStore` and feature lifecycle to persist tunnel metadata and respect CLI overrides.
- [ ] Introduce `branchbox tunnel` CLI subcommands and enrich `feature list` output with tunnel status.
- [ ] Backfill tests, update documentation, and migrate any relevant legacy references.

## Current Status

- 2025-11-02 — Drafted in-progress spec outlining goals, constraints, and incremental plan.
- 2025-11-02 — Added tunnel config scaffolding, secure credential storage, and `branchbox init` prompts that default to Cloudflared with manual fallbacks.
- 2025-11-02 — Introduced tunnel provider abstraction with a Cloudflared stub that surfaces manual instructions until API integration lands.
- 2025-11-02 — Feature state registry now captures tunnel metadata, storing manual setup instructions and respecting per-feature skips.
