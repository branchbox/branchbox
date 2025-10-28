# Homebrew Tap Automation Setup

This document describes how the automated Homebrew formula updates are configured in the BranchBox release workflow.

## Overview

The release workflow (`.github/workflows/release.yml`) includes an `update-homebrew` job that automatically updates the [homebrew-tap](https://github.com/branchbox/homebrew-tap) repository when stable releases are published.

## Prerequisites

### 1. Homebrew Tap Repository

The tap repository must exist at: `https://github.com/branchbox/homebrew-tap`

Required structure:
```
homebrew-tap/
├── Formula/
│   └── branchbox.rb
└── README.md
```

### 2. GitHub Personal Access Token

**REQUIRED SECRET:** `HOMEBREW_TAP_TOKEN`

This token is used by the workflow to push updates to the homebrew-tap repository.

#### Creating the Token

**Recommended: Use a fine-grained personal access token** for better security and minimal permissions.

1. Go to: https://github.com/settings/tokens?type=beta
2. Click "Generate new token"
3. Configure:
   - **Token name**: `Homebrew Tap Updates`
   - **Expiration**: 1 year (set a calendar reminder to renew)
   - **Repository access**: Select "Only select repositories"
     - Choose: `branchbox/homebrew-tap`
   - **Permissions**:
     - Under "Repository permissions", find **Contents**
     - Set to: **Read and write**
4. Click "Generate token"
5. **Copy the token immediately** - you won't see it again

<details>
<summary>Alternative: Classic Token (Less Secure)</summary>

If you need to use a classic token:

1. Go to: https://github.com/settings/tokens/new
2. Configure:
   - **Note**: `Homebrew Tap Updates`
   - **Expiration**: 1 year
   - **Scopes**: ✅ `repo`
3. Generate and copy the token

**Note**: Classic tokens grant broader access. Fine-grained tokens are more secure.
</details>

For more security details, see the [Security Notes](#security-notes) section below.

#### Adding the Secret

1. Go to repository settings: `https://github.com/branchbox/branchbox/settings/secrets/actions`
2. Click "New repository secret"
3. Enter:
   - **Name**: `HOMEBREW_TAP_TOKEN`
   - **Value**: Paste the token from above
4. Click "Add secret"

### 3. Release Assets

The workflow expects the following assets to be published with each release:

- `branchbox-{VERSION}-x86_64-apple-darwin.tar.gz` (macOS Intel)
- `branchbox-{VERSION}-aarch64-apple-darwin.tar.gz` (macOS Apple Silicon)
- `checksums.txt` (SHA256 hashes for all binaries)

These are automatically created by the `build-release` and `publish-release` jobs.

## How It Works

### Workflow Trigger

The `update-homebrew` job runs:
- ✅ After `create-release`, `build-release`, and `publish-release` complete
- ✅ Only for stable releases (skips pre-releases with `-alpha`, `-beta`, `-rc`)
- ✅ Only when binaries and checksums are available

### Update Process

1. **Checkout tap repo**: Clone `branchbox/homebrew-tap` using the `HOMEBREW_TAP_TOKEN`
2. **Download checksums**: Fetch `checksums.txt` from the published release
3. **Extract checksums**: Parse checksums for Intel and ARM binaries
4. **Update formula**: Use `sed` to update version, URLs, and SHA256 hashes
5. **Commit & push**: Push changes to the tap repository

### Formula Updates

The workflow updates these fields in `Formula/branchbox.rb`:

```ruby
version "X.Y.Z"  # → New version

# Intel Mac
url "https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/branchbox-X.Y.Z-x86_64-apple-darwin.tar.gz"
sha256 "abc123..."  # → New checksum

# Apple Silicon Mac
url "https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/branchbox-X.Y.Z-aarch64-apple-darwin.tar.gz"
sha256 "def456..."  # → New checksum
```

## Verification

After a release is published, verify the automation worked:

### 1. Check GitHub Actions

1. Go to: https://github.com/branchbox/branchbox/actions
2. Click on the latest release workflow run
3. Verify the `Update Homebrew Formula` job succeeded

### 2. Check Homebrew Tap

1. Go to: https://github.com/branchbox/homebrew-tap/commits/main
2. Verify there's a new commit: `chore: update formula to vX.Y.Z`
3. View the commit to confirm version and checksums were updated

### 3. Test Installation

```bash
# Update Homebrew
brew update

# Upgrade BranchBox (if already installed)
brew upgrade branchbox

# Or install fresh
brew install branchbox/tap/branchbox

# Verify version
branchbox --version
```

## Troubleshooting

### Job Fails: "could not read from remote repository"

**Cause**: `HOMEBREW_TAP_TOKEN` is missing, expired, or has insufficient permissions

**Fix**:
1. Check the secret exists: Repository Settings → Secrets → Actions
2. Verify token hasn't expired (create a new one if needed)
3. Ensure token has `repo` scope
4. Regenerate token and update secret if necessary

### Job Fails: "Failed to extract checksums"

**Cause**: `checksums.txt` is missing or has unexpected format

**Fix**:
1. Check the release has `checksums.txt` asset
2. Verify format: `{hash}  branchbox-{version}-{target}.tar.gz`
3. Ensure filenames match exactly (especially hyphens vs underscores)

### Formula Update Fails: "sed: no such file"

**Cause**: Formula structure doesn't match expected patterns

**Fix**:
1. Compare current formula with expected structure:
   ```bash
   # View current formula in homebrew-tap repo
   curl -fsSL https://raw.githubusercontent.com/branchbox/homebrew-tap/main/Formula/branchbox.rb
   ```

2. Verify formula has this exact structure (required for sed patterns):
   ```ruby
   class Branchbox < Formula
     desc "..."
     homepage "..."
     version "X.Y.Z"  # ← Must be this format
     license "MIT"

     on_macos do
       if Hardware::CPU.intel?
         url "https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/branchbox-X.Y.Z-x86_64-apple-darwin.tar.gz"
         sha256 "abc123..."  # ← Must be on separate line
       elsif Hardware::CPU.arm?
         url "https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/branchbox-X.Y.Z-aarch64-apple-darwin.tar.gz"
         sha256 "def456..."  # ← Must be on separate line
       end
     end
   end
   ```

3. Ensure version/url/sha256 patterns match exactly
4. Test sed commands locally before updating workflow

### No Homebrew Update for Pre-Release

**Expected behavior**: The job is skipped for pre-releases (alpha, beta, rc)

**Verify**: Check the job condition in the workflow:
```yaml
if: needs.create-release.outputs.is_prerelease == 'false'
```

Pre-releases should only update the main repo, not Homebrew.

## Monitoring & Alerts

### GitHub Actions Notifications

Set up alerts to be notified when the workflow fails:

1. **Repository Settings**:
   - Go to: `https://github.com/branchbox/branchbox/settings/notifications`
   - Enable "Actions" notifications
   - Choose notification preferences (email, web, mobile)

2. **Watch Releases**:
   - Go to repository page
   - Click "Watch" → "Custom" → Select "Releases"
   - This will notify you when releases are published

3. **Slack/Discord Integration** (Optional):
   - Add webhook notifications for workflow failures
   - See [GitHub Actions Slack Integration](https://github.com/marketplace/actions/slack-send)

### Successful Update Example

After a stable release is published, you should see:

**GitHub Actions**:
- Workflow: `Release`
- Job: `Update Homebrew Formula` ✓ Passed
- Duration: ~30-60 seconds

**Homebrew Tap Commit**:
```
chore: update formula to v0.2.1

Author: branchbox-release-bot <noreply+release-bot@github.com>
Date:   2025-10-27 10:30:00 -0700

Files changed: 1 (Formula/branchbox.rb)
Lines changed: +3/-3
```

**Changed Lines**:
```diff
- version "0.2.0"
+ version "0.2.1"

- sha256 "abc123..."  # Intel
+ sha256 "def456..."  # Intel

- sha256 "xyz789..."  # ARM
+ sha256 "uvw012..."  # ARM
```

**Commit URL Format**:
```
https://github.com/branchbox/homebrew-tap/commit/[commit-hash]
```

### Verification Checklist

After each release, verify:
- [ ] GitHub Actions workflow completed successfully
- [ ] New commit in homebrew-tap repo with correct version
- [ ] Formula contains new checksums (view commit diff)
- [ ] Test installation: `brew reinstall branchbox/tap/branchbox`
- [ ] Version matches: `branchbox --version` shows new version

## Manual Update

If automation fails and you need to update manually:

```bash
# Clone the tap
git clone https://github.com/branchbox/homebrew-tap
cd homebrew-tap

# Get checksums from release
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/checksums.txt

# Edit Formula/branchbox.rb
# Update version, URLs, and sha256 values

# Test locally
brew audit --strict Formula/branchbox.rb
brew install --build-from-source Formula/branchbox.rb

# Commit and push
git add Formula/branchbox.rb
git commit -m "chore: update formula to vX.Y.Z"
git push
```

## Rollback Procedures

If a formula update causes issues, you can rollback:

### Quick Rollback (Revert Last Commit)

```bash
# Clone the tap
git clone https://github.com/branchbox/homebrew-tap
cd homebrew-tap

# View recent commits
git log --oneline -5

# Revert the problematic commit
git revert HEAD
git push

# Users can now reinstall
brew update
brew reinstall branchbox
```

### Rollback to Specific Version

```bash
# Find the commit for the working version
git log --oneline --grep="v0.2.0"

# Reset to that commit
git reset --hard <commit-hash>
git push --force

# Note: Force push requires admin access or disabled branch protection
```

### Emergency: Point to Previous Release

Edit `Formula/branchbox.rb` manually to point to previous release:

```ruby
version "0.2.0"  # Previous working version

on_macos do
  if Hardware::CPU.intel?
    url "https://github.com/branchbox/branchbox/releases/download/v0.2.0/..."
    sha256 "..." # Previous checksum
  # ...
end
```

Then commit and push normally.

## Security Notes

- The `HOMEBREW_TAP_TOKEN` has write access to the homebrew-tap repository
- Store it ONLY as a GitHub Actions secret (never commit it)
- Use a **fine-grained token** scoped only to the `homebrew-tap` repository:
  - Go to: Settings → Developer settings → Personal access tokens → Fine-grained tokens
  - Repository access: Only select `branchbox/homebrew-tap`
  - Permissions: `Contents` (Read and write)
- Rotate the token every 6-12 months
- GitHub will email warnings before token expiration
- Consider using a bot/service account instead of personal token

## Reference

- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [GitHub Actions Secrets](https://docs.github.com/en/actions/security-guides/encrypted-secrets)
- [Homebrew Tap Distribution Feature Spec](../features/completed/homebrew-tap-distribution.md)

## Support

- **Formula issues**: https://github.com/branchbox/homebrew-tap/issues
- **Automation issues**: https://github.com/branchbox/branchbox/issues
- **Slack**: #infrastructure (internal)
