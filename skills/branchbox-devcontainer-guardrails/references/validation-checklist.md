# Validation Checklist

Run the smallest relevant subset while iterating. Before PR/release handoff for these change types, run the full required set.

## Rust + workspace checks (devcontainer)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features --no-fail-fast
cargo test --doc --all-features
cargo build --all-features
cargo build --release --all-features
cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
cargo nextest run --tests --all-features --run-ignored ignored-only --no-fail-fast
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --document-private-items
```

For Agentify `in-guest` lifecycle or project-environment changes, also run the focused adversarial
coverage before the full suite:

```bash
cargo test -p worktree-core runtime::in_guest::tests --all-features
cargo test -p worktree-core runtime::in_guest::tests::tool_request_ --all-features
cargo test -p worktree-core runtime::in_guest::tests::trusted_relay_ --all-features
cargo test -p worktree-core workflows::feature::tests::test_in_guest --all-features
cargo test -p branchbox-cli --test feature_commands in_guest_ -- --nocapture
```

## Release harness matrix (devcontainer)

```bash
./scripts/manual-cli-e2e.sh
./scripts/manual-cli-e2e.sh --mode verbose
./scripts/manual-cli-e2e.sh --mode pretend
STACK=generic ./scripts/manual-cli-e2e.sh
STACK=rails ./scripts/manual-cli-e2e.sh
STACK=node ./scripts/manual-cli-e2e.sh
./scripts/manual-agent-e2e.sh --cp-stub
```

## 1Password/auth-signing changes (host)

Use a host-native `BRANCHBOX_BIN` (do not reuse Linux binary built inside devcontainer on macOS host).

```bash
ORIGIN_SSH_URL='git@github.com:<org>/<repo>.git' \
OP_GITHUB_REF='op://<vault>/<item>/token' \
OP_SIGNING_KEY_REF='op://<vault>/<item>/private key' \
./scripts/manual-1password-e2e.sh --check-failure-path
```

## Security + robustness preflight (host)

Run these quick checks before opening/updating the PR when touching bootstrap/auth/harness/env-writing code.

```bash
./scripts/review-preflight.sh

# A linked owner-only tool socket must never become a coding-container mount or permissive socket.
rg -n "linked_tool_endpoints|WritableRequestSpool|chmod 0?777|chmod 0?666" core/src/runtime/in_guest.rs

# No world-readable host signing key writes
rg -n "chmod 64[0-9].*(SIGNING_KEY|git-signing-key)|chmod 66[0-9].*(SIGNING_KEY|git-signing-key)" core/src/bootstrap scripts || true

# Secret temp-file writes should be created under restrictive umask
rg -n "umask 077.*(TOKEN_FILE|SIGNING_KEY_FILE|git_config_tmp)|\\(umask 077; printf" core/src/bootstrap/templates/common/init-host.sh

# Empty secret reads should not overwrite existing files
rg -n "was empty; keeping existing (token file|key file)" core/src/bootstrap/templates/common/init-host.sh

# No raw-token interpolation in git credential helper snippets
rg -n "password=\\$\\{?github_token\\}?" core/src/bootstrap/templates/common/setup-git.sh || true

# APP_URL writes should route through URL-specific sanitization
rg -n "APP_URL=.*sanitize_url_env_value|fn sanitize_url_env_value" core/src/workflows/feature.rs

# Compose + branch sanitizer policy checks
rg -n "fn sanitize_compose_project_name|matches!\\(ch, '-' \\| '_'\\)|trim_start_matches\\(\\['-', '_'\\]\\)" core/src/workflows/feature.rs
rg -n "fn sanitize_git_branch_env_value|matches!\\(ch, '\\\\.' \\| '-' \\| '_' \\| '/'\\)" core/src/workflows/feature.rs

# Harness should support plugin and legacy compose + robust service resolution
rg -n "docker-compose|resolve_devcontainer_service" scripts/manual-1password-e2e.sh
rg -n "read-configuration|detect_compose_service" scripts/manual-1password-e2e.sh scripts/manual-cli-e2e.sh
rg -n "source .*scripts/lib/devcontainer-service.sh" scripts/manual-1password-e2e.sh scripts/manual-cli-e2e.sh

# Keep harness docs synchronized
diff -u scripts/manual-1password-e2e.md docs/docs/getting-started/manual-1password-e2e.md
```

## Docs + macOS checks (host)

```bash
cd docs && npm run build
cd ../macos && /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift build -v
cd ../macos && /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift test -v
```

## Audit note

`cargo audit` may report allowed warnings (for explicitly accepted advisories). Record warning IDs in PR notes when present.
