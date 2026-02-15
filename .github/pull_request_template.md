## Summary

- What problem does this PR solve?
- What changed (high level)?

## Scope

- Issue(s): <!-- e.g. Closes #45 -->
- Risk level: <!-- low / medium / high -->
- Rollback plan: <!-- one or two concrete steps -->

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo nextest run --all-features --no-fail-fast`
- [ ] `cargo test --doc`
- [ ] `cd docs && npm run build` (if docs changed)
- [ ] Updated `CHANGELOG.md` (if behavior changed)

## Devcontainer/Auth/Signing Changes (issue #45 flow)

> Complete this section only if the PR touches bootstrap/devcontainer auth-signing/harness logic.

- [ ] Preserve secret files on failed `op read` (no truncate-first behavior)
- [ ] Enforce `chmod 600` for host-side secret/key files
- [ ] Avoid raw token interpolation in persisted shell snippets (use runtime env vars like `GH_TOKEN`)
- [ ] Sanitize untrusted env-derived writes (strip CR/LF for values like `APP_URL`)
- [ ] Support both `docker compose` and `docker-compose` where relevant
- [ ] Use JSONC-safe devcontainer service resolution with compose fallback in harness scripts
- [ ] Keep runbook docs in sync:
  - [ ] `scripts/manual-*.md`
  - [ ] `docs/docs/getting-started/manual-*.md`
- [ ] Ran:
  - [ ] `ORIGIN_SSH_URL=... OP_GITHUB_REF=... OP_SIGNING_KEY_REF=... ./scripts/manual-1password-e2e.sh --check-failure-path`

## Notes for Reviewers

- Areas that need extra attention:
- Known limitations / follow-ups:
