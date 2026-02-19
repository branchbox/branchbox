# Release Guide for Maintainers

This document describes the process for creating and publishing new releases of BranchBox.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Release Types](#release-types)
- [Release Process](#release-process)
- [Emergency Rollback](#emergency-rollback)
- [Post-Release Tasks](#post-release-tasks)
- [Troubleshooting](#troubleshooting)

## Prerequisites

Before creating a release, ensure you have:

1. **Required tools installed:**
   ```bash
   # Install cargo-release (for version management)
   cargo install cargo-release --locked

   # Install git-cliff (for changelog generation)
   cargo install git-cliff --locked

   # Install GitHub CLI (for release management)
   # macOS: brew install gh
   # Linux: https://github.com/cli/cli#installation
   # Windows: https://github.com/cli/cli#installation
   ```

2. **Proper permissions:**
   - Write access to the repository
   - Ability to push tags
   - Ability to create GitHub releases

3. **Clean working directory:**
   ```bash
   git status  # Should show no uncommitted changes
   ```

## Release Types

### Stable Release

Stable releases follow semantic versioning: `MAJOR.MINOR.PATCH`

- **MAJOR**: Breaking changes
- **MINOR**: New features (backwards compatible)
- **PATCH**: Bug fixes

Examples: `v1.0.0`, `v1.2.3`, `v2.0.0`

### Pre-Release

Pre-releases are used for testing before stable release:

- **Beta**: `v1.0.0-beta.1`, `v1.0.0-beta.2`
- **Alpha**: `v1.0.0-alpha.1`
- **Release Candidate**: `v1.0.0-rc.1`

Pre-releases are marked with the `prerelease` flag on GitHub and don't update the "latest" release.

## Release Process

### Step 1: Prepare the Release

1. **Ensure you're on the main branch and up-to-date:**
   ```bash
   git checkout main
   git pull origin main
   ```

2. **Run all quality checks:**
   ```bash
   # Format check
   cargo fmt --all -- --check

   # Linting
   cargo clippy --all-targets --all-features -- -D warnings

   # Tests
   cargo test --all-features

   # Documentation
   cargo doc --no-deps --all-features

   # Documentation site (Docusaurus)
   cd docs
   npm install
   npm run build
   cd ..

   # Demo assets (website + docs + social variants)
   ./scripts/remotion-docs-all.sh --stack rust --target both

   # Final combined site bundle sanity check
   ./scripts/build-site.sh --skip-demo-assets
   ```

3. **Update CHANGELOG.md and documentation:**
   - Edit `CHANGELOG.md` to capture highlights for the version you're publishing
   - Update end-user docs (`README.md`, `docs/docs/**`) if behavior changed
   - Keep marketing demo assets in sync by re-running `./scripts/remotion-docs-all.sh --stack rust --target both`
   - Confirm landing page demo embeds and social links still resolve (`website/index.html`)
   - If `branchbox init` prompts/defaults or devcontainer bootstrap behavior changed (including 1Password/git wiring), update first-run docs in `docs/docs/getting-started/quick-start.md` with exact behavior and caveats
   - If CLI flags changed, regenerate `docs/docs/reference/cli.md` using `branchbox --help`
   - Commit documentation updates before tagging

4. **Run the manual CLI smoke harness (recommended):**
   ```bash
   # Quick pretend-mode validation (no Docker required)
   ./scripts/manual-cli-e2e.sh --mode pretend

   # Full validation with Docker (if available)
   ./scripts/manual-cli-e2e.sh
   STACK=generic ./scripts/manual-cli-e2e.sh
   ```

   The harness exercises init → feature lifecycle → tunnel permutations → teardown. See `docs/docs/getting-started/manual-cli-e2e.md` for details. For major releases, run all stack/mode combinations.

5. **Changelog preview (optional):**
   ```bash
   git-cliff --unreleased
   ```

### Step 2: Create Version Tag

#### For Stable Release

```bash
# Dry-run to see what will happen
cargo release --workspace --dry-run

# Execute the release (this will bump version and create tag)
cargo release --workspace --execute

# Push the tag (this triggers the CI workflow)
git push --follow-tags
```

#### For Pre-Release (Beta)

```bash
# Dry-run for beta release
cargo release --workspace --pre-release beta --dry-run

# Execute beta release
cargo release --workspace --pre-release beta --execute

# Push the tag
git push --follow-tags
```

#### Manual Tag Creation (Alternative)

If you prefer manual control:

```bash
# Update version in Cargo.toml manually
# Update CHANGELOG.md manually

# Commit changes
git add .
git commit -m "chore(release): prepare v1.0.0"

# Create tag
git tag -a v1.0.0 -m "Release v1.0.0"

# Push
git push origin main
git push origin v1.0.0
```

### Step 3: Monitor the Workflow

1. **Watch the GitHub Actions workflow:**
   ```bash
   # Using gh CLI
   gh run watch

   # Or visit GitHub web interface
   # https://github.com/branchbox/branchbox/actions
   ```

2. **The workflow will:**
   - Generate changelog with git-cliff
   - Create a draft GitHub release
   - Build binaries for all platforms (Linux, macOS, Windows)
   - Package binaries with documentation
   - Generate SHA256 checksums
   - Upload all artifacts to the release
   - Publish the release (remove draft status)

3. **Expected duration:** ~15-20 minutes

### Step 4: Verify the Release

1. **Check the GitHub release page:**
   ```bash
   gh release view v1.0.0 --web
   ```

2. **Verify artifacts are present:**
   - `branchbox-1.0.0-x86_64-unknown-linux-gnu.tar.gz`
   - `branchbox-1.0.0-aarch64-unknown-linux-gnu.tar.gz`
   - `branchbox-1.0.0-x86_64-apple-darwin.tar.gz`
   - `branchbox-1.0.0-aarch64-apple-darwin.tar.gz`
   - `branchbox-1.0.0-x86_64-pc-windows-msvc.zip`
   - `checksums.txt`

3. **Test installation on each platform:**

   **Linux/macOS:**
   ```bash
   # Download artifact
   curl -fsSL https://github.com/branchbox/branchbox/releases/download/v1.0.0/branchbox-1.0.0-x86_64-unknown-linux-gnu.tar.gz -o branchbox.tar.gz

   # Verify checksum
   curl -fsSL https://github.com/branchbox/branchbox/releases/download/v1.0.0/checksums.txt -o checksums.txt
   sha256sum -c checksums.txt --ignore-missing

   # Extract and test
   tar xzf branchbox.tar.gz
   cd branchbox-1.0.0-x86_64-unknown-linux-gnu
   ./branchbox --version
   ./branchbox --help
   ```

   **Windows (PowerShell):**
   ```powershell
   # Download artifact
   Invoke-WebRequest -Uri "https://github.com/branchbox/branchbox/releases/download/v1.0.0/branchbox-1.0.0-x86_64-pc-windows-msvc.zip" -OutFile branchbox.zip

   # Download checksums
   Invoke-WebRequest -Uri "https://github.com/branchbox/branchbox/releases/download/v1.0.0/checksums.txt" -OutFile checksums.txt

   # Verify checksum
   $hash = (Get-FileHash branchbox.zip -Algorithm SHA256).Hash.ToLower()
   $expectedHash = (Get-Content checksums.txt | Select-String "branchbox-1.0.0-x86_64-pc-windows-msvc.zip" | ForEach-Object { $_.Line.Split(' ')[0] })
   if ($hash -eq $expectedHash) {
       Write-Host "✓ Checksum verification passed" -ForegroundColor Green
   } else {
       Write-Host "✗ Checksum verification failed!" -ForegroundColor Red
       Write-Host "  Expected: $expectedHash"
       Write-Host "  Got:      $hash"
       exit 1
   }

   # Extract and test
   Expand-Archive branchbox.zip
   cd branchbox\branchbox-1.0.0-x86_64-pc-windows-msvc
   .\branchbox.exe --version
   .\branchbox.exe --help
   ```

### Step 5: Post-Release Verification

1. **Check that the release is visible:**
   - GitHub Releases page shows the new release
   - Release is marked as "Latest" (for stable) or "Pre-release" (for beta)

2. **Verify badges are updated:**
   - Visit the README on GitHub
   - Check that version badge shows the new version

3. **Test basic commands:**
   ```bash
   branchbox --version  # Should show new version
   branchbox feature list
   ```

4. **Verify demo surfaces are live:**
   - Landing page `https://branchbox.dev` "See the magic" player loads reel + chapter cuts
   - Docs page `https://branchbox.dev/docs/guides/demo-assets` loads embedded videos
   - Social preview image resolves: `https://branchbox.dev/media/demos/branchbox-teaser-rust-social-card.jpg`

## Emergency Rollback

If a critical issue is discovered after release:

### Option 1: Mark Release as Draft (Hide it)

```bash
# Mark the release as draft to hide it
gh release edit v1.0.0 --draft

# This removes it from the "latest" release
# Users can't see it, but it's not deleted
```

### Option 2: Delete the Release and Tag

```bash
# Delete the GitHub release
gh release delete v1.0.0 --yes

# Delete the local tag
git tag -d v1.0.0

# Delete the remote tag
git push origin :refs/tags/v1.0.0
```

### Option 3: Create Hotfix Release

For critical bugs, create a patch release immediately:

```bash
# Create hotfix branch
git checkout -b hotfix/v1.0.1 v1.0.0

# Fix the issue
git add .
git commit -m "fix: critical issue description"

# Merge to main
git checkout main
git merge hotfix/v1.0.1

# Create patch release
cargo release --workspace patch --execute
git push --follow-tags

# Delete hotfix branch
git branch -d hotfix/v1.0.1
```

## Post-Release Tasks

### 1. Verify Homebrew Formula Update

The release workflow **automatically updates** the Homebrew formula for stable releases (non-pre-release only).

**What happens automatically:**
- The `update-homebrew` job runs after `publish-release` completes
- Downloads checksums.txt from the new release
- Updates the formula in [branchbox/homebrew-tap](https://github.com/branchbox/homebrew-tap)
- Updates version, URLs, and SHA256 checksums for both Intel and ARM macOS builds
- Commits and pushes changes with message: `chore: update formula to vX.Y.Z`

**Verify the update:**
```bash
# Check the homebrew-tap repository for the update
gh repo view branchbox/homebrew-tap --web

# Or check the commit history
gh api repos/branchbox/homebrew-tap/commits --jq '.[0] | {message: .commit.message, date: .commit.author.date}'

# Test installation locally (requires adding the tap first)
brew tap branchbox/tap
brew install branchbox
branchbox --version
```

**Troubleshooting:**
- If the job fails, check the workflow logs: `gh run view`
- Ensure `HOMEBREW_TAP_TOKEN` secret is set in repository settings
- The job only runs for stable releases (skipped for pre-releases)
- See workflow file: `.github/workflows/release.yml` (lines 354-496)

### 2. Update Scoop Manifest (Future)

Scoop support is planned for a future milestone. When implemented, it will follow a similar automated pattern.

### 3. Announce the Release

- Post to project communication channels
- Update documentation site (if applicable)
- Social media announcements
- Blog post for major releases

### 4. Monitor for Issues

- Watch GitHub Issues for bug reports
- Monitor download metrics
- Check for platform-specific problems

### 5. Verify Documentation Deployment

- Confirm the `docs-deploy` workflow finished successfully (`gh run list --workflow docs-deploy`)
- Visit https://branchbox.github.io/branchbox/ (or the preview environment) to ensure the Docusaurus site reflects the new release notes and CLI reference
- Regenerate `docs/docs/reference/cli.md` (capture `branchbox --help` output) if CLI help text changed and include it in the release PR before tagging

## Troubleshooting

### Build Failures

**Problem:** One or more platform builds fail in CI

**Solution:**
1. Check the workflow logs: `gh run view`
2. Common issues:
   - Cross-compilation setup failed → Check `cross` installation
   - Tests failed → Fix tests and push to trigger rebuild
   - Disk space issues → Workflow cleans up automatically, retry

### Tag Already Exists

**Problem:** `error: tag 'v1.0.0' already exists`

**Solution:**
```bash
# Delete the local tag
git tag -d v1.0.0

# Delete the remote tag if pushed
git push origin :refs/tags/v1.0.0

# Try again
cargo release --workspace --execute
```

### Release Already Exists on GitHub

**Problem:** GitHub release already exists but workflow failed

**Solution:**
```bash
# Delete the release
gh release delete v1.0.0 --yes

# Delete the tag
git tag -d v1.0.0
git push origin :refs/tags/v1.0.0

# Try again
cargo release --workspace --execute
git push --follow-tags
```

### Checksum Verification Fails

**Problem:** Downloaded binary fails checksum verification

**Solution:**
1. Re-download the artifact (might be network corruption)
2. If still fails, check the workflow logs
3. If checksums were generated incorrectly, delete release and retry

### Version Mismatch

**Problem:** `branchbox --version` shows wrong version after release

**Solution:**
1. Check that version was updated in all `Cargo.toml` files
2. Verify that workspace-level version is set correctly
3. Ensure cargo-release updated all crates: `cargo release --workspace`

## Release Checklist

Use this checklist for each release:

- [ ] Checkout main branch and pull latest changes
- [ ] Run quality checks: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- [ ] Build docs: `cargo doc --no-deps && cd docs && npm run build`
- [ ] Update `CHANGELOG.md` with release highlights
- [ ] Update docs if CLI behavior changed
- [ ] Run E2E harness: `./scripts/manual-cli-e2e.sh --mode pretend`
- [ ] Dry-run: `cargo release --workspace --dry-run`
- [ ] Execute: `cargo release --workspace --execute`
- [ ] Push: `git push --follow-tags`
- [ ] Monitor: `gh run watch`
- [ ] Verify release artifacts on GitHub
- [ ] Verify docs deployed: `gh run list --workflow docs-deploy`

## Version History

| Version | Date | Type | Notes |
|---------|------|------|-------|
| 0.7.0 | 2026-01-14 | Minor | Devcontainer CLI commands, .ai-agents directory, release skill |
| 0.6.0 | 2026-01-13 | Minor | Pre-built devcontainer images, mise runtime management |
| 0.5.0 | 2026-01-07 | Minor | Git worktree compatibility, Claude Code mounts |
| 0.4.1 | 2025-12-15 | Patch | Init fixes, tunnel commands |
| 0.4.0 | 2025-11-15 | Minor | Agent daemon, macOS app, gRPC |
| 0.3.0 | 2025-11-09 | Minor | Default agent, specs automation |
| 0.2.2 | 2025-11-08 | Patch | .branchbox.env seeding |
| 0.2.0 | 2025-11-03 | Minor | Devcontainer module, Cloudflared |
| 0.1.0 | 2025-10-27 | Initial | Core workflow orchestration |

## Questions?

If you encounter any issues not covered in this guide:

1. Check GitHub Actions logs: `gh run view`
2. Review workflow file: `.github/workflows/release.yml`
3. Open an issue: `gh issue create`
