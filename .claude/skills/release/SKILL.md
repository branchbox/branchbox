---
name: release
description: Release a new version of BranchBox. Use when the user says "release", "new version", "cut a release", "publish release", or "tag version". Handles version bumping, changelog updates, quality checks, and GitHub release workflow.
---

# Release Workflow for BranchBox

This skill guides you through releasing a new version of BranchBox with all necessary checks, documentation updates, and automation.

## Pre-Release Checklist

Before starting, verify:
- [ ] You are on the `main` branch
- [ ] Working directory is clean (`git status` shows no changes)
- [ ] You have push access to the repository
- [ ] GitHub CLI is authenticated (`gh auth status`)

## Release Process

### Step 1: Run Quality Checks

Run all these checks in parallel:

```bash
# Format check
cargo fmt --all -- --check

# Linting (allow up to 5 minutes)
cargo clippy --all-targets --all-features -- -D warnings

# Tests (allow up to 5 minutes)
cargo test --all-features

# Documentation
cargo doc --no-deps --all-features

# Docs site (Docusaurus)
cd docs && npm install --silent && npm run build && cd ..
```

**All checks must pass before proceeding.**

### Step 2: Determine Version Bump

Check the CHANGELOG.md `[Unreleased]` section to determine the appropriate version bump:

| Change Type | Version Bump | Example |
|-------------|--------------|---------|
| Breaking changes | `major` | 0.6.0 → 1.0.0 |
| New features (backwards compatible) | `minor` | 0.5.0 → 0.6.0 |
| Bug fixes only | `patch` | 0.5.1 → 0.5.2 |

### Step 3: Update Documentation

#### 3a. Update CHANGELOG.md

Change the `[Unreleased]` section header to include the new version and today's date:

```markdown
## [Unreleased]

## [X.Y.Z] - YYYY-MM-DD
```

Keep an empty `## [Unreleased]` section at the top for future changes.

#### 3b. Update RELEASING.md Version History

Add a new row to the version history table:

```markdown
| X.Y.Z | YYYY-MM-DD | Minor/Patch/Major | Brief description of release highlights |
```

#### 3c. Commit Documentation Updates

```bash
git add CHANGELOG.md RELEASING.md
git commit -m "chore(release): prepare vX.Y.Z

- Update CHANGELOG.md with vX.Y.Z release notes
- Update RELEASING.md version history table

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

### Step 4: Dry-Run Release

Always run a dry-run first to verify everything is correct:

```bash
cargo release --workspace <bump-level> --dry-run
```

Where `<bump-level>` is `minor`, `major`, or `patch`.

**IMPORTANT**: Do NOT use `cargo release --workspace --execute` without specifying the bump level. This will fail with "tag already exists" error.

### Step 5: Execute Release

```bash
cargo release --workspace <bump-level> --execute --no-confirm
```

**Key flags:**
- `--workspace`: Update all crates in the workspace
- `--execute`: Actually perform the release (dry-run is default)
- `--no-confirm`: Skip interactive confirmation (NOT `-y`)

This command will:
1. Bump versions in all Cargo.toml files
2. Run tests
3. Create a commit
4. Create and push the version tag
5. Push to origin

### Step 6: Monitor the Release Workflow

```bash
# List recent workflow runs
gh run list --limit 5

# Watch a specific workflow (get ID from list above)
gh run watch <run-id> --exit-status

# Or check status periodically
gh run view <run-id> --json status,conclusion,jobs | jq '{status, conclusion, jobs: [.jobs[] | {name, status, conclusion}]}'
```

The release workflow builds binaries for:
- Linux (x86_64, aarch64)
- macOS (x86_64, aarch64)
- Windows (x86_64)

Expected duration: ~10-20 minutes.

### Step 7: Verify the Release

```bash
# View the release
gh release view vX.Y.Z

# Verify all expected artifacts are present
gh release view vX.Y.Z --json assets | jq '.assets[].name'
```

Expected artifacts:
- `branchbox-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- `branchbox-X.Y.Z-aarch64-unknown-linux-gnu.tar.gz`
- `branchbox-X.Y.Z-x86_64-apple-darwin.tar.gz`
- `branchbox-X.Y.Z-aarch64-apple-darwin.tar.gz`
- `branchbox-X.Y.Z-x86_64-pc-windows-msvc.zip`
- `checksums.txt`
- `branchbox-local-vm-image-X.Y.Z-x86_64.tar.gz`

### Step 8: Verify Docs Deployment

```bash
gh run list --workflow "Deploy Site" --limit 1
```

## Common Mistakes and How to Avoid Them

### Mistake 1: Running cargo release without bump level

**Wrong:**
```bash
cargo release --workspace --execute
# Error: tag `vX.Y.Z` already exists
```

**Correct:**
```bash
cargo release --workspace minor --execute --no-confirm
```

### Mistake 2: Using -y for confirmation skip

**Wrong:**
```bash
cargo release --workspace minor --execute -y
# Error: unexpected argument '-y' found
```

**Correct:**
```bash
cargo release --workspace minor --execute --no-confirm
```

### Mistake 3: Forgetting to commit docs before release

cargo-release creates its own commit. If you have uncommitted changes to CHANGELOG.md or RELEASING.md, they won't be included in the release.

**Always commit documentation updates BEFORE running cargo release.**

### Mistake 4: Not specifying timeout for long commands

Clippy and tests can take several minutes. Always use appropriate timeouts:
- clippy: 5 minutes (300000ms)
- tests: 5 minutes (300000ms)
- cargo release: 10 minutes (600000ms)

## Rollback Procedures

### If the release workflow fails after tagging:

```bash
# Delete the GitHub release (if created)
gh release delete vX.Y.Z --yes

# Delete local tag
git tag -d vX.Y.Z

# Delete remote tag
git push origin :refs/tags/vX.Y.Z

# Fix the issue and retry
```

### If you need to create a hotfix:

```bash
# Create patch release
cargo release --workspace patch --execute --no-confirm
```

## Quick Reference

```bash
# Full release command sequence (after docs are updated and committed):
cargo release --workspace minor --execute --no-confirm
gh run watch  # Monitor the release workflow
gh release view vX.Y.Z  # Verify the release
```

## Known Issues

### GitHub Release Shows Full Changelog Instead of Release-Specific Notes

**Problem:** The GitHub release notes include the entire project changelog instead of just the changes for that specific release.

**Root Cause:** The release workflow uses `git-cliff --tag vX.Y.Z` which generates the full changelog up to that tag.

**Fix:** Update `.github/workflows/release.yml` line 67 to use `--latest` flag:

```yaml
# Before (generates full changelog):
git-cliff --tag v${{ steps.version.outputs.version }} > RELEASE_CHANGELOG.md

# After (generates only changes since last tag):
git-cliff --latest > RELEASE_CHANGELOG.md
```

**Workaround:** Manually edit the GitHub release notes after publishing:
```bash
gh release edit vX.Y.Z --notes-file <(git-cliff --latest)
```

## See Also

- [RELEASING.md](../../RELEASING.md) - Full release guide
- [CHANGELOG.md](../../CHANGELOG.md) - Version history
