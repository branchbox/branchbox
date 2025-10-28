# Homebrew Tap Automation

Automated Homebrew formula updates when stable releases are published.

## Quick Start

### 1. Create Fine-Grained Token

[Generate token](https://github.com/settings/tokens?type=beta) with these settings:
- **Name**: `Homebrew Tap Updates`
- **Expiration**: 1 year
- **Repository**: Only `branchbox/homebrew-tap`
- **Permissions**: Contents → Read and write

<details>
<summary>Using classic token instead? (not recommended)</summary>

[Classic token](https://github.com/settings/tokens/new) with `repo` scope works but grants broader access.
</details>

### 2. Add Secret

Add to [repository secrets](https://github.com/branchbox/branchbox/settings/secrets/actions):
- Name: `HOMEBREW_TAP_TOKEN`
- Value: Your token from step 1

That's it. Formula updates automatically on each stable release.

---

## How It Works

On stable release:
1. Download checksums from GitHub Release
2. Update `Formula/branchbox.rb` with new version and SHA256s
3. Commit to [homebrew-tap](https://github.com/branchbox/homebrew-tap)

Skips pre-releases (`-alpha`, `-beta`, `-rc`).

---

## Verification

After a release, check:

**GitHub Actions**: [Actions tab](https://github.com/branchbox/branchbox/actions) → `Update Homebrew Formula` job ✓

**Homebrew Tap**: [Commits](https://github.com/branchbox/homebrew-tap/commits/main) → Look for `chore: update formula to vX.Y.Z`

**Test Install**:
```bash
brew update && brew upgrade branchbox
branchbox --version  # Should show new version
```

---

## Troubleshooting

### Job fails with "could not read from remote repository"
Token issue. Check:
- Secret exists at Settings → Secrets → Actions
- Token hasn't expired
- Token has Contents write permission

Fix: Regenerate token and update secret.

### Job fails with "Failed to extract checksums"
Release missing `checksums.txt` or format incorrect.

Verify: `curl -fsSL https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/checksums.txt`

Expected format: `{sha256}  branchbox-{version}-{target}.tar.gz`

### Formula update fails
Formula structure changed. View [current formula](https://raw.githubusercontent.com/branchbox/homebrew-tap/main/Formula/branchbox.rb) and ensure it matches expected structure (see below).

<details>
<summary>Show expected formula structure</summary>

```ruby
class Branchbox < Formula
  version "X.Y.Z"  # Must be this format

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/branchbox-X.Y.Z-x86_64-apple-darwin.tar.gz"
      sha256 "..."  # Must be on separate line
    elsif Hardware::CPU.arm?
      url "https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/branchbox-X.Y.Z-aarch64-apple-darwin.tar.gz"
      sha256 "..."  # Must be on separate line
    end
  end
end
```
</details>

---

## Advanced

<details>
<summary>Manual formula update</summary>

If automation fails:

```bash
git clone https://github.com/branchbox/homebrew-tap
cd homebrew-tap

# Update Formula/branchbox.rb with new version and checksums
# Get checksums: curl -fsSL https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/checksums.txt

git commit -am "chore: update formula to vX.Y.Z"
git push
```
</details>

<details>
<summary>Rollback formula</summary>

Quick revert:
```bash
cd homebrew-tap
git revert HEAD && git push
```

Or point to previous version by editing `Formula/branchbox.rb`.
</details>

<details>
<summary>Monitoring setup</summary>

Enable workflow failure notifications:
- Repository Settings → Notifications → Enable "Actions"
- Optional: [Slack integration](https://github.com/marketplace/actions/slack-send)
</details>

---

## Security

- Token scoped to single repo (fine-grained recommended)
- Stored as GitHub Actions secret only
- Rotate every 6-12 months
- Consider using bot account for production

---

## Reference

- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [Feature Spec](../features/completed/homebrew-tap-distribution.md)
- Issues: [branchbox/homebrew-tap](https://github.com/branchbox/homebrew-tap/issues)
