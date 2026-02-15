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
# No world-readable host signing key writes
rg -n "chmod 64[0-9].*(SIGNING_KEY|git-signing-key)|chmod 66[0-9].*(SIGNING_KEY|git-signing-key)" core/src/bootstrap scripts || true

# No raw-token interpolation in git credential helper snippets
rg -n "password=\\$\\{?github_token\\}?" core/src/bootstrap/templates/common/setup-git.sh || true

# APP_URL writes should sanitize CR/LF when sourced from env/config
rg -n "replace\\(\\['\\\\n', '\\\\r'\\], \"\"\\)" core/src/workflows/feature.rs

# Harness should support plugin and legacy compose + JSONC-safe service parsing
rg -n "docker-compose|resolve_devcontainer_service" scripts/manual-1password-e2e.sh
rg -nF "sed '/^[[:space:]]*\\/\\//d'" scripts/manual-1password-e2e.sh

# Keep harness docs synchronized
diff -u <(tail -n +5 docs/docs/getting-started/manual-1password-e2e.md) scripts/manual-1password-e2e.md
```

## Docs + macOS checks (host)

```bash
cd docs && npm run build
cd ../macos && /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift build -v
cd ../macos && /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift test -v
```

## Audit note

`cargo audit` may report allowed warnings (for explicitly accepted advisories). Record warning IDs in PR notes when present.
