---
work_feature: cloudflared-tunnel-integration
status: in-progress
created: 2025-11-02
updated: 2025-11-05
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
- 2025-11-05 — Replaced the Cloudflared stub with full Cloudflare API automation (create/configure/DNS/delete) guarded by credentials, plus `mockito`-backed coverage for all HTTP flows.
- 2025-11-05 — Feature workflow now streams tunnel provisioning through the provider, persists tokens to `.devcontainer/.cloudflared.env`, snapshots descriptors in `FeatureStateStore`, and promotes manual runs to `manual` status with step-by-step instructions.
- 2025-11-05 — BranchBox config gains interactive prompts for Cloudflare account id/token path, storing sanitized defaults in `.branchbox/config.json` and `.branchbox/secure/`, with automation disabled when `manual_instructions` is set.

## Implementation Notes

- **Cloudflare client** — `core/src/cloudflare.rs` now issues create/configure/DNS/delete calls with structured validation errors, while `mockito` fixtures exercise happy path and failure branches without hitting the real API.
- **Provider lifecycle** — `core/src/tunnel/cloudflared.rs` translates automation credentials into `ProvisioningOutcome`, writes connector tokens under `.branchbox/secure/tunnels/`, and emits actionable manual instructions whenever automation is skipped or misconfigured.
- **Workflow integration** — `core/src/workflows/feature.rs` routes feature start/teardown through the provider via `tunnel_open`/`tunnel_remove`, writes `.devcontainer/.cloudflared.env`, and records `FeatureTunnelState` (pending/active/manual/disabled) so future CLI surfaces can display tunnel health.
- **Configuration & init** — `branchbox init` now hydrates `BranchBoxConfig` defaults, prompts for Cloudflare account id plus token location, and flips `manual_instructions` whenever users opt out so automation stays disabled until credentials return.
- **Manual fallback** — Automation gaps yield deterministic guidance referencing `legacy/cloudflared/README.md`, preserving legacy workflows while we finish the dedicated CLI and UX polish.

## Follow-ups

- Wire `branchbox tunnel` subcommands onto `FeatureWorkflow::tunnel_open`/`tunnel_remove`, including non-interactive flags and status inspection.
- Surface tunnel status and hostnames inside `branchbox feature list`, making use of the stored `FeatureTunnelState` labels.
- Add workflow-level tests that mock provider interactions to cover provisioning success, manual fallback, and teardown cleanup without real API calls.
- Backfill docs (README, legacy migration guide) so manual setup reflects the new config file layout and token storage locations.
- Revisit credential UX for shared worktrees (e.g., env var overrides, `.branchbox/secure` permissions) before exposing the feature broadly.

## CLI UX Sketch

- `branchbox tunnel open --feature auth-mfa` — provisions using workspace defaults, prints hostname, provider, token path, and warning banner if manual steps remain.
- `branchbox tunnel remove --feature auth-mfa --force` — tears down the stored descriptor, clears DNS if possible, and updates registry status to `disabled`.
- `branchbox tunnel status` — lists active tunnels with columns: feature, provider, hostname, status badge (`online/degraded/manual/disabled`), last-updated timestamp, and opt-in verbose instructions.
- `branchbox feature list` — gains `Tunnel` column showing hostname plus status badge; hidden when tunnels disabled workspace-wide.
- Non-interactive mode (`--yes`, `BRANCHBOX_NON_INTERACTIVE=1`) respects stored credentials, returning actionable error messages instead of prompts.

## Configuration Matrix

- **Workspace defaults** — `.branchbox/config.json` tracks `tunnel.enabled`, `default_provider`, and `providers.cloudflared.*`.
- **Secure secrets** — `.branchbox/secure/cloudflared.env` stores API tokens with `0600` permissions; `.branchbox/secure/tunnels/*.env` holds per-feature connector tokens.
- **Overrides** — CLI flags (`--tunnel-provider`, `--tunnel-manual`, `--skip-module tunnel`) and env vars (`BRANCHBOX_TUNNEL_PROVIDER`, `BRANCHBOX_SKIP_TUNNEL`) fan out through `FeatureWorkflow`.
- **Fallback markers** — When automation is bypassed, `manual_instructions: true` stays persisted so future runs continue surfacing guidance without assuming credentials exist.

## Security Considerations

- Connector tokens live under `.branchbox/secure/tunnels/` with directory-level permission checks; workflow logs only sanitized paths.
- Cloudflare API responses are sanitized before logging, stripping tokens and PII fields.
- Ensure `branchbox init` warns when tokens are stored outside the secure directory or when file permissions are broader than `0600`.
- Future agent daemons must mount `.branchbox/secure` read-only for non-admin users; document OS-specific chmod instructions.

## Testing & Validation

- Unit: expand `core::cloudflare` mocks to assert request payloads and error propagation across credential, API, and DNS failure paths.
- Unit: cover `CloudflaredProvider` automation/manual branches, credential discovery, and token persistence (happy path + misconfigurations).
- Integration: fixture-driven feature start/teardown harness that fakes provider calls via dependency injection, asserting registry updates and `.cloudflared.env` contents.
- CLI smoke: snapshot tests for `branchbox tunnel status` output, ensuring manual vs automated states render as expected.
- Docs: link spec instructions to README snippets verified via `cargo test --doc` where relevant.

## Risks & Mitigations

- **Credentials missing or stale** — continue emitting manual instructions and maintain `manual_instructions: true`; provide `branchbox tunnel doctor` follow-up idea for quick diagnostics.
- **API rate limits** — cache existing tunnel descriptors in registry and retry provisioning with backoff; fall back to manual path after configurable attempts.
- **DNS propagation delays** — status command should mark newly created tunnels as `pending` until `curl` checks succeed; encourage manual verification in instructions.
- **Token sprawl** — add cleanup command that prunes tokens for removed features and document rotation steps.

## Rollout Plan

- **Alpha (internal)** — enable tunnels for maintainers running `branchbox init` in non-production repos, capture logs, and iterate on error messaging before public docs land.
- **Beta (opt-in)** — guard the CLI subcommands behind `BRANCHBOX_EXPERIMENTAL_TUNNELS=1`; publish README instructions and solicit feedback from 3 pilot teams.
- **General availability** — lift the flag once telemetry shows >95% automation success across pilots, documentation is complete, and cleanup commands are shipped.
- **Fallback handling** — keep legacy scripts available (`legacy/cloudflared`) with prominent pointers but mark them deprecated once GA criteria met.

## Telemetry & Observability

- Emit structured events for `tunnel_open`, `tunnel_status`, and `tunnel_remove`, including provider, outcome, latency buckets, and error categories.
- Add `BRANCHBOX_TUNNEL_TRACE=1` to enable verbose HTTP logging, redacting secrets before emission.
- Feed metrics into the planned agent daemon so remote coordinators can surface stuck tunnels or rate-limit incidents.
- Persist summarized run history in `.branchbox/history.json` (timestamp, outcome, manual vs automated) for offline troubleshooting and support escalation.

## Open Questions

- Should `branchbox tunnel status` perform active health checks (HTTP probe) or rely on Cloudflare API responses only?
- How do we reconcile per-feature tunnel tokens with shared CI agents—should there be a service account tunnel instead?
- What retention policy should be applied to `.branchbox/secure/tunnels/*.env` when a feature is archived?
- Do we need Terraform-style drift detection for DNS records, or is best-effort cleanup sufficient for Milestone 1?
