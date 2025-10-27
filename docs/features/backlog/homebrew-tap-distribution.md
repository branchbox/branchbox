---
work_feature: homebrew-tap-distribution
status: backlog
created: 2025-10-27
updated: 2025-10-27
---

# Homebrew Tap Distribution for macOS

## Overview

Create a Homebrew tap repository to enable easy installation of BranchBox on macOS via `brew install`. The tap will provide a formula that automatically detects Intel vs Apple Silicon architectures and downloads the appropriate binary from GitHub Releases.

## Background

With the release automation infrastructure complete (see `docs/features/completed/release-automation.md`), BranchBox now publishes pre-built binaries for macOS Intel and Apple Silicon. The next step is to make these binaries easily installable via Homebrew, the de facto package manager for macOS.

## Goals

1. **Create Homebrew Tap Repository**
   - Set up `github.com/branchbox/homebrew-tap` repository
   - Configure repository with appropriate documentation
   - Set up branch protection and CI (optional)

2. **Write Homebrew Formula**
   - Create `branchbox.rb` formula
   - Implement architecture detection (Intel vs Apple Silicon)
   - Download binaries from GitHub Releases
   - Verify SHA256 checksums
   - Install binary and shell completions (future)

3. **Automate Formula Updates**
   - Add workflow step to main repo's release pipeline
   - Automatically update formula when new releases are published
   - Generate correct download URLs and checksums

4. **Documentation**
   - Update main repo README with Homebrew installation instructions
   - Create tap repo README with usage and troubleshooting
   - Document formula maintenance procedures

## Technical Requirements

### Repository Setup

**Repository:** `github.com/branchbox/homebrew-tap`

**Structure:**
```
homebrew-tap/
├── README.md
├── Formula/
│   └── branchbox.rb
└── .github/
    └── workflows/
        └── tests.yml (optional)
```

**Branch protection:**
- Require PR reviews for main branch
- Run tests before merge (optional)

### Formula Structure

**Formula location:** `Formula/branchbox.rb`

**Formula template:**
```ruby
class Branchbox < Formula
  desc "Distributed development environment orchestrator"
  homepage "https://github.com/branchbox/branchbox"
  version "0.2.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/branchbox/branchbox/releases/download/v0.2.0/branchbox-0.2.0-x86_64-apple-darwin.tar.gz"
      sha256 "CHECKSUM_INTEL"
    elsif Hardware::CPU.arm?
      url "https://github.com/branchbox/branchbox/releases/download/v0.2.0/branchbox-0.2.0-aarch64-apple-darwin.tar.gz"
      sha256 "CHECKSUM_ARM"
    end
  end

  def install
    bin.install "branchbox"

    # Future: Install shell completions
    # bash_completion.install "completions/branchbox.bash"
    # zsh_completion.install "completions/branchbox.zsh"
    # fish_completion.install "completions/branchbox.fish"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/branchbox --version")
  end
end
```

**Key features:**
- Architecture detection using `Hardware::CPU.intel?` and `Hardware::CPU.arm?`
- Separate download URLs for each architecture
- SHA256 verification for each binary
- Version test to validate installation

### Automated Updates

**Add to `.github/workflows/release.yml` in main repo:**

```yaml
update-homebrew:
  name: Update Homebrew Formula
  needs: [create-release, build-release, publish-release]
  runs-on: ubuntu-latest

  steps:
    - name: Checkout homebrew-tap repo
      uses: actions/checkout@v4
      with:
        repository: branchbox/homebrew-tap
        token: ${{ secrets.HOMEBREW_TAP_TOKEN }}
        path: homebrew-tap

    - name: Download checksums
      run: |
        VERSION="${{ needs.create-release.outputs.version }}"
        curl -fsSL "https://github.com/branchbox/branchbox/releases/download/v$VERSION/checksums.txt" -o checksums.txt

    - name: Extract checksums
      id: checksums
      run: |
        INTEL_CHECKSUM=$(grep "x86_64-apple-darwin" checksums.txt | awk '{print $1}')
        ARM_CHECKSUM=$(grep "aarch64-apple-darwin" checksums.txt | awk '{print $1}')

        echo "intel=$INTEL_CHECKSUM" >> $GITHUB_OUTPUT
        echo "arm=$ARM_CHECKSUM" >> $GITHUB_OUTPUT

    - name: Update formula
      run: |
        VERSION="${{ needs.create-release.outputs.version }}"
        cd homebrew-tap/Formula

        # Update version
        sed -i "s/version \".*\"/version \"$VERSION\"/" branchbox.rb

        # Update Intel URL and checksum
        sed -i "s|url \"https://github.com/branchbox/branchbox/releases/download/v.*/branchbox-.*-x86_64-apple-darwin.tar.gz\"|url \"https://github.com/branchbox/branchbox/releases/download/v$VERSION/branchbox-$VERSION-x86_64-apple-darwin.tar.gz\"|" branchbox.rb
        sed -i "/Hardware::CPU.intel?/,/sha256/ s/sha256 \".*\"/sha256 \"${{ steps.checksums.outputs.intel }}\"/" branchbox.rb

        # Update ARM URL and checksum
        sed -i "s|url \"https://github.com/branchbox/branchbox/releases/download/v.*/branchbox-.*-aarch64-apple-darwin.tar.gz\"|url \"https://github.com/branchbox/branchbox/releases/download/v$VERSION/branchbox-$VERSION-aarch64-apple-darwin.tar.gz\"|" branchbox.rb
        sed -i "/Hardware::CPU.arm?/,/sha256/ s/sha256 \".*\"/sha256 \"${{ steps.checksums.outputs.arm }}\"/" branchbox.rb

    - name: Commit and push
      run: |
        cd homebrew-tap
        git config user.name "github-actions[bot]"
        git config user.email "github-actions[bot]@users.noreply.github.com"
        git add Formula/branchbox.rb
        git commit -m "chore: update formula to v${{ needs.create-release.outputs.version }}"
        git push
```

**Required secret:**
- `HOMEBREW_TAP_TOKEN`: GitHub Personal Access Token with `repo` scope for the tap repository

### Testing

**Formula testing:**
```bash
# Audit formula
brew audit --strict --online Formula/branchbox.rb

# Test installation locally
brew install --build-from-source Formula/branchbox.rb

# Test from tap
brew tap branchbox/tap
brew install branchbox

# Verify installation
branchbox --version
```

**GitHub Actions testing (optional):**
Create `.github/workflows/tests.yml` in tap repo:
```yaml
name: Tests

on:
  pull_request:
  push:
    branches: [main]

jobs:
  audit:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - name: Audit formula
        run: brew audit --strict --online Formula/branchbox.rb

  test-install:
    runs-on: macos-latest
    strategy:
      matrix:
        arch: [x86_64, arm64]
    steps:
      - uses: actions/checkout@v4
      - name: Test installation
        run: |
          brew install --build-from-source Formula/branchbox.rb
          branchbox --version
```

## Implementation Tasks

### Phase 1: Repository Setup (1-2 days)

- [ ] Create `branchbox/homebrew-tap` repository on GitHub
- [ ] Add README.md with tap usage instructions
- [ ] Create `Formula/` directory
- [ ] Set up branch protection on main
- [ ] Create initial `branchbox.rb` formula with current release
- [ ] Test formula installation manually

**Deliverables:**
- Tap repository with initial formula
- Formula tested on Intel and Apple Silicon Macs

### Phase 2: Automated Updates (1 day)

- [ ] Create Personal Access Token for tap repo access
- [ ] Add `HOMEBREW_TAP_TOKEN` secret to main repo
- [ ] Add `update-homebrew` job to release workflow
- [ ] Test automated update with a test release
- [ ] Validate checksums are correctly extracted and updated

**Deliverables:**
- Release workflow automatically updates Homebrew formula
- Formula version and checksums updated on each release

### Phase 3: Documentation & Testing (1 day)

- [ ] Update main repo README with Homebrew installation instructions
- [ ] Create tap repo README with detailed usage
- [ ] Document troubleshooting steps
- [ ] Add formula audit to tap CI (optional)
- [ ] Test installation from tap on clean systems

**Deliverables:**
- Comprehensive installation documentation
- Tested installation instructions
- Optional CI for formula validation

## User Experience

### Installation

**Adding the tap:**
```bash
brew tap branchbox/tap
```

**Installing BranchBox:**
```bash
brew install branchbox
```

**One-line install:**
```bash
brew install branchbox/tap/branchbox
```

**Updating:**
```bash
brew update
brew upgrade branchbox
```

**Uninstalling:**
```bash
brew uninstall branchbox
brew untap branchbox/tap
```

### Verification

After installation:
```bash
branchbox --version
# Output: branchbox 0.2.0
```

## Documentation Updates

### Main Repo README

Update installation section:
```markdown
### macOS

#### Homebrew

```bash
brew install branchbox/tap/branchbox
```

Or add the tap first:
```bash
brew tap branchbox/tap
brew install branchbox
```

#### Direct Download

Download pre-built binaries from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest).
```

### Tap Repo README

```markdown
# BranchBox Homebrew Tap

Official Homebrew tap for [BranchBox](https://github.com/branchbox/branchbox).

## Installation

```bash
brew install branchbox/tap/branchbox
```

## Updating

```bash
brew update
brew upgrade branchbox
```

## Uninstalling

```bash
brew uninstall branchbox
```

## Troubleshooting

### Architecture Issues

BranchBox automatically installs the correct version for your Mac:
- Intel Macs: `x86_64-apple-darwin`
- Apple Silicon Macs: `aarch64-apple-darwin`

To verify which version is installed:
```bash
file $(which branchbox)
```

### Checksum Mismatch

If you encounter checksum errors, try:
```bash
brew update
brew reinstall branchbox
```

### Reporting Issues

Please report issues to:
- BranchBox: https://github.com/branchbox/branchbox/issues
- Homebrew formula: https://github.com/branchbox/homebrew-tap/issues
```

## Dependencies

**Required:**
- GitHub repository access (create `branchbox/homebrew-tap`)
- GitHub Personal Access Token with `repo` scope
- Homebrew (for testing)

**Tools:**
- Ruby (for formula development)
- `brew audit` (for validation)

## Risks & Mitigations

**Risk: Formula syntax errors**
- Mitigation: Test formula with `brew audit --strict`
- Mitigation: Add CI tests to validate formula

**Risk: Checksum extraction failures**
- Mitigation: Test automated update workflow thoroughly
- Mitigation: Add error handling and notifications

**Risk: Architecture detection issues**
- Mitigation: Test on both Intel and Apple Silicon Macs
- Mitigation: Use Homebrew's built-in `Hardware::CPU` checks

**Risk: Update workflow token expiration**
- Mitigation: Use fine-grained token with expiration alerts
- Mitigation: Document token refresh procedure

## Success Criteria

- [ ] Users can install BranchBox with `brew install branchbox/tap/branchbox`
- [ ] Correct binary installed for Intel and Apple Silicon
- [ ] Formula automatically updates on new releases
- [ ] Checksums correctly verified during installation
- [ ] Installation documented in main repo README
- [ ] Tap repository has comprehensive README

## Future Enhancements

### Shell Completions

Once shell completions are generated by the CLI:
```ruby
def install
  bin.install "branchbox"

  # Install completions
  bash_completion.install "completions/branchbox.bash"
  zsh_completion.install "completions/branchbox.zsh"
  fish_completion.install "completions/branchbox.fish"
end
```

### Man Pages

If man pages are added:
```ruby
def install
  bin.install "branchbox"
  man1.install "man/branchbox.1"
end
```

### Homebrew Core

Long-term goal: Submit formula to Homebrew core
- Requires: Stable release history
- Requires: Significant user adoption
- Process: https://github.com/Homebrew/homebrew-core/blob/master/CONTRIBUTING.md

## References

**Homebrew Documentation:**
- [Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [Acceptable Formulae](https://docs.brew.sh/Acceptable-Formulae)
- [Python for Formula Authors](https://docs.brew.sh/Python-for-Formula-Authors) (for syntax reference)

**Example Taps:**
- [Hashicorp Tap](https://github.com/hashicorp/homebrew-tap)
- [GitHub CLI Tap](https://github.com/cli/homebrew-tap)

**Related Specs:**
- `docs/features/completed/release-automation.md` - Release infrastructure
- `docs/features/backlog/install-scripts.md` - Linux/Windows installation

**Workflow:**
- `.github/workflows/release.yml` - Main repo release workflow
