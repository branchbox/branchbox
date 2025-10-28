# Homebrew Tap Distribution - Implementation Summary

## Completed: 2025-10-27

## Overview

Implemented automated Homebrew formula updates for the BranchBox CLI. When stable releases are published, the workflow automatically updates the [branchbox/homebrew-tap](https://github.com/branchbox/homebrew-tap) repository with new version numbers, download URLs, and SHA256 checksums.

## What Was Implemented

### 1. Automated Homebrew Update Job

**File**: `.github/workflows/release.yml`

Added the `update-homebrew` job that:
- Runs after `create-release`, `build-release`, and `publish-release` complete
- Only executes for stable releases (skips alpha, beta, rc)
- Checks out the homebrew-tap repository using `HOMEBREW_TAP_TOKEN` secret
- Downloads the `checksums.txt` from the GitHub release
- Extracts checksums for macOS Intel and Apple Silicon binaries
- Updates `Formula/branchbox.rb` with new version, URLs, and checksums
- Commits and pushes changes to the tap repository

**Key Features**:
- Error handling: Validates checksums were extracted successfully
- Conditional execution: Only commits if there are actual changes
- Clear logging: Shows checksums and updated formula for debugging
- Fail-safe: Exits with error if checksums are missing or empty

### 2. Setup Documentation

**File**: `docs/HOMEBREW_SETUP.md`

Comprehensive documentation covering:
- Prerequisites and requirements
- GitHub Personal Access Token setup
- Secret configuration instructions
- How the workflow operates
- Verification steps
- Troubleshooting guide
- Manual update procedures
- Security considerations

### 3. Reference Documentation

**Files**: `tmp/README.md`, `tmp/INITIAL_SETUP.md`, `tmp/HOMEBREW_AUTOMATION.md`

These files (provided by you) serve as reference material for:
- Initial formula setup with first release
- Homebrew tap repository structure
- Formula template and patterns
- Testing procedures

### 4. Feature Spec Completion

**File**: Moved from `docs/features/backlog/homebrew-tap-distribution.md` to `docs/features/completed/`

Updated front matter to reflect completion:
```yaml
status: completed
completed: 2025-10-27
```

## Integration Points

### Workflow Dependencies

```yaml
update-homebrew:
  needs: [create-release, build-release, publish-release]
```

The Homebrew update job depends on:
1. **create-release**: Provides version and prerelease status
2. **build-release**: Builds macOS binaries (Intel + ARM)
3. **publish-release**: Uploads binaries and checksums.txt to GitHub Releases

### Required Secret

**Name**: `HOMEBREW_TAP_TOKEN`
**Scope**: `repo` (full control of homebrew-tap repository)
**Created**: Needs to be added to repository secrets

📝 **Action Required**: Create and add this token before the next release.

## How It Works

### Release Workflow

```
1. Tag pushed (e.g., v0.2.0)
   ↓
2. create-release: Create GitHub Release (draft)
   ↓
3. build-release: Build binaries for all platforms
   ↓
4. publish-release: Upload binaries + checksums.txt, publish release
   ↓
5. update-homebrew: Update Formula/branchbox.rb in tap repo
   ↓
6. Users run: brew update && brew upgrade branchbox
```

### Formula Updates

The workflow uses `sed` to update:

```ruby
version "0.2.0"  # → Updated to new version

# Intel Mac
url "https://github.com/branchbox/branchbox/releases/download/v0.2.0/..."
sha256 "abc123..."  # → Updated with new checksum

# Apple Silicon Mac
url "https://github.com/branchbox/branchbox/releases/download/v0.2.0/..."
sha256 "def456..."  # → Updated with new checksum
```

## Testing Plan

Before the next release, verify:

1. **Secret exists**: Check repository Settings → Secrets → Actions
2. **Token works**: Test token has push access to homebrew-tap repo
3. **Formula structure**: Ensure `Formula/branchbox.rb` matches expected patterns
4. **Test release**: Consider a dry-run with a test tag

### Post-Release Verification

After the next release:

1. Check GitHub Actions for successful `update-homebrew` job
2. Verify commit in homebrew-tap: `chore: update formula to vX.Y.Z`
3. Test installation: `brew install branchbox/tap/branchbox`
4. Verify version: `branchbox --version`

## Files Changed

```
Modified:
  .github/workflows/release.yml     (+74 lines)

Added:
  docs/HOMEBREW_SETUP.md            (new file)
  tmp/README.md                     (reference)
  tmp/INITIAL_SETUP.md              (reference)
  tmp/HOMEBREW_AUTOMATION.md        (reference)

Moved:
  docs/features/backlog/homebrew-tap-distribution.md
  → docs/features/completed/homebrew-tap-distribution.md
```

## Checklist for Next Release

- [ ] Create `HOMEBREW_TAP_TOKEN` personal access token
- [ ] Add token to repository secrets as `HOMEBREW_TAP_TOKEN`
- [ ] Verify homebrew-tap repo exists at `github.com/branchbox/homebrew-tap`
- [ ] Ensure `Formula/branchbox.rb` has correct structure
- [ ] Tag a new release (e.g., `v0.2.0`)
- [ ] Monitor GitHub Actions for successful completion
- [ ] Verify formula update in homebrew-tap repo
- [ ] Test installation from Homebrew

## Benefits

1. **Zero manual work**: Formula updates happen automatically
2. **Fast availability**: New versions available via Homebrew within minutes
3. **Architecture support**: Seamless Intel/ARM detection
4. **Checksum verification**: Security through SHA256 validation
5. **User-friendly**: Standard Homebrew update/upgrade workflow

## References

- Main spec: `docs/features/completed/homebrew-tap-distribution.md`
- Setup guide: `docs/HOMEBREW_SETUP.md`
- Release workflow: `.github/workflows/release.yml`
- Homebrew tap: https://github.com/branchbox/homebrew-tap
- Homebrew docs: https://docs.brew.sh/Formula-Cookbook

## Next Steps

1. **Add the secret**: Create and configure `HOMEBREW_TAP_TOKEN`
2. **Test with next release**: Verify automation works end-to-end
3. **Update main README**: Add Homebrew installation instructions
4. **Announce**: Let users know about Homebrew availability
5. **Monitor**: Watch for any formula issues after first automated update

## Support

- Automation issues: Open issue in `branchbox/branchbox`
- Formula issues: Open issue in `branchbox/homebrew-tap`
- Questions: Check `docs/HOMEBREW_SETUP.md` troubleshooting section
