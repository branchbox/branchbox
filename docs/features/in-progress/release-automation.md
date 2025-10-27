---

branch: feature/release-automation
created: 2025-10-27
status: in-progress
updated: 2025-10-27
work_feature: release-automation
worktree: /Users/rbarazi/projects/branchbox/release-automation
---
# Automated Release Management & Distribution

## Overview

Implement a comprehensive continuous deployment pipeline for BranchBox that automates building, packaging, and publishing releases across multiple platforms. The system should support both beta (pre-release) and stable releases, create GitHub releases with downloadable artifacts, and provide a seamless installation experience for users.

## Current State

**Existing Infrastructure:**
- ✅ Robust CI pipeline (`.github/workflows/ci.yml`) with quality checks, tests, and multi-platform builds
- ✅ Build validation across Linux, macOS, and Windows
- ✅ Code coverage tracking with Codecov integration
- ✅ Cargo workspace structure with `core` and `cli` crates

**Gaps:**
- ❌ No automated release workflow
- ❌ Manual versioning and changelog management
- ❌ No binary distribution mechanism
- ❌ No installation instructions or scripts
- ❌ README doesn't reference downloadable releases

**Current Version:** 0.1.0 (workspace-level in root `Cargo.toml`)

## Implementation Summary

**Date Completed:** 2025-10-27

**Milestone 1 Status:** ✅ Complete

### Files Created/Modified

- ✅ `.github/workflows/release.yml` - Complete release automation workflow
- ✅ `Cargo.toml` - Added `[workspace.metadata.release]` configuration
- ✅ `cliff.toml` - git-cliff configuration for changelog generation
- ✅ `CHANGELOG.md` - Initial changelog with 0.1.0 and unreleased sections
- ✅ `RELEASING.md` - Comprehensive maintainer release guide
- ✅ `README.md` - Added badges and installation instructions

### What Was Implemented

**Release Workflow Architecture:**
- Three-job pipeline: `create-release` → `build-release` → `publish-release`
- Tag-triggered releases matching `v*` pattern
- Manual `workflow_dispatch` with version input
- Pre-release detection for beta/alpha/rc versions
- Draft release creation, then auto-publish after all builds succeed

**Cross-Platform Builds:**
- Linux x86_64 (`x86_64-unknown-linux-gnu`)
- Linux ARM64 (`aarch64-unknown-linux-gnu`) with cross-compilation
- macOS Intel (`x86_64-apple-darwin`) on macos-13 runner
- macOS Apple Silicon (`aarch64-apple-darwin`) on macos-14 runner
- Windows x64 (`x86_64-pc-windows-msvc`)

**Binary Packaging:**
- Platform-specific archives: tar.gz (Unix), zip (Windows)
- Archive naming: `branchbox-{version}-{target}.{ext}`
- Contents: binary, README.md, LICENSE, CHANGELOG.md
- Binary stripping for smaller file sizes (Linux, macOS)

**Checksum Generation:**
- Individual SHA256 checksums per artifact
- Consolidated `checksums.txt` uploaded to release
- Verification instructions in release notes

**Version Management:**
- cargo-release configuration in workspace Cargo.toml
- Pre-release hooks run `cargo test --all`
- Conventional commit message templates
- Tag prefix: `v`

**Changelog Automation:**
- git-cliff with conventional commits parsing
- Commit grouping: Features, Bug Fixes, Documentation, etc.
- Breaking change detection and highlighting
- Auto-generated release notes with installation instructions

**Documentation:**
- Maintainer release guide with step-by-step process
- Emergency rollback procedures
- Troubleshooting guide
- Release checklist template
- README installation section for all platforms
- Shields.io badges for version, downloads, CI, license

### Testing Results

**Status:** ✅ Complete - All tests passed on v0.0.0-test.5

**Test Iterations:** 5 test tags (v0.0.0-test.1 through v0.0.0-test.5)

**Issues Found and Fixed:**

1. **Build Failure - Package Ambiguity**
   - Error: Cargo didn't know which package to build in workspace
   - Fix: Added `--package branchbox-cli` to build command

2. **Build Failure - Cargo.lock Sync**
   - Error: `the lock file needs to be updated but --locked was passed`
   - Fix: Removed `--locked` flag to allow dependency updates in CI

3. **Build Failure - OpenSSL Missing (ARM Cross-Compilation)**
   - Error: `Could not find openssl` for aarch64-unknown-linux-gnu
   - Fix: Added `vendored-openssl` and `vendored-libgit2` features to git2 dependency

4. **Build Failure - macOS Checksum Command**
   - Error: `sha256sum: command not found` on macOS
   - Fix: Split checksum generation to use `shasum -a 256` on macOS, `sha256sum` on Linux

5. **Publish Failure - Bash Syntax Error**
   - Error: `syntax error near unexpected token '2'` in file upload loop
   - Fix: Added `shopt -s nullglob` and removed `2>/dev/null` from for loop

6. **GitHub Actions Syntax**
   - Error: Conditional `!matrix.use_cross` not working
   - Fix: Changed to `matrix.use_cross != true`

**Final Test Results (v0.0.0-test.5):**
- ✅ All 5 platform builds completed successfully
- ✅ All binaries packaged correctly (tar.gz for Unix, zip for Windows)
- ✅ SHA256 checksums generated for all artifacts
- ✅ All artifacts uploaded to GitHub release
- ✅ Consolidated checksums.txt created and uploaded
- ✅ Release published successfully (draft → published)

**Tested Platforms:**
- ✅ Linux x86_64 (x86_64-unknown-linux-gnu)
- ✅ Linux ARM64 (aarch64-unknown-linux-gnu) with cross-compilation
- ✅ macOS Intel (x86_64-apple-darwin)
- ✅ macOS Apple Silicon (aarch64-apple-darwin)
- ✅ Windows x64 (x86_64-pc-windows-msvc)

## Goals

1. **Automated Release Workflow**
   - Tag-triggered GitHub Actions workflow for stable releases (`v*.*.*`)
   - Pre-release support for beta versions (`v*.*.*-beta.*`)
   - Cross-platform binary compilation (Linux, macOS, Windows)
   - Automated artifact packaging and signing

2. **GitHub Release Management**
   - Automatic release creation with changelog
   - Upload compiled binaries as release assets
   - Support for multiple architectures (x86_64, ARM64)
   - Asset checksums for verification

3. **Version Management**
   - Semantic versioning enforcement
   - Automated version bumping across workspace crates
   - Changelog generation from commit history
   - Pre-release tagging conventions

4. **Distribution & Installation**
   - Homebrew tap for macOS
   - Linux install script (curl | sh pattern)
   - Windows installer or Chocolatey package
   - README badges and installation instructions

5. **Developer Experience**
   - Release scripts for maintainers
   - Version bumping automation
   - Release checklist and documentation

## Technical Requirements

### 1. GitHub Actions Release Workflow

Create `.github/workflows/release.yml` with:

- **Trigger conditions:**
  - `push` events on tags matching `v*` pattern
  - Manual `workflow_dispatch` with version input

- **Build matrix:**
  - Linux (x86_64, aarch64)
  - macOS (x86_64, aarch64)
  - Windows (x86_64)

- **Build steps:**
  - Checkout with full git history for changelog
  - Install Rust toolchain (stable)
  - Cross-compilation setup for ARM targets
  - Build with `--release` profile
  - Strip debug symbols for smaller binaries
  - Package binaries with README, LICENSE, and CHANGELOG

- **Release creation:**
  - Parse tag to extract version and pre-release status
  - Generate changelog from git commits since last release
  - Create GitHub release via `gh` CLI or actions/create-release
  - Upload packaged artifacts with architecture-specific names
  - Generate and upload SHA256 checksums

### 2. Binary Packaging

**Artifact naming convention:**
```
branchbox-{version}-{os}-{arch}.{ext}

Examples:
- branchbox-1.0.0-linux-x86_64.tar.gz
- branchbox-1.0.0-macos-aarch64.tar.gz
- branchbox-1.0.0-windows-x86_64.zip
- branchbox-1.0.0-beta.1-linux-x86_64.tar.gz
```

**Archive contents:**
```
branchbox-1.0.0-linux-x86_64/
├── branchbox              # binary
├── README.md              # installation & usage
├── LICENSE                # MIT license
├── CHANGELOG.md           # version-specific changelog
└── completions/           # shell completions (optional)
    ├── branchbox.bash
    ├── branchbox.zsh
    └── branchbox.fish
```

**Checksums file** (`checksums.txt`):
```
<sha256>  branchbox-1.0.0-linux-x86_64.tar.gz
<sha256>  branchbox-1.0.0-macos-aarch64.tar.gz
...
```

### 3. Version Management Tooling

**Options:**
- **cargo-release**: Automates version bumping, tagging, and publishing
- **cargo-smart-release**: Workspace-aware version management
- **Custom script**: Workspace version sync + git tagging

**Recommended approach:**
Use `cargo-release` with workspace configuration in root `Cargo.toml`:

```toml
[workspace.metadata.release]
sign-commit = false
sign-tag = false
pre-release-commit-message = "chore(release): prepare {{version}}"
post-release-commit-message = "chore(release): finalize {{version}}"
tag-message = "Release {{version}}"
tag-prefix = "v"
pre-release-hook = ["cargo", "test", "--all"]
```

**Workflow for maintainers:**
```bash
# Prepare release (dry-run)
cargo release --workspace --dry-run

# Create release tag
cargo release --workspace --execute

# Push tag (triggers CI)
git push --follow-tags
```

### 4. Changelog Generation

**Tool options:**
- `git-cliff`: Generates changelog from conventional commits
- `cargo-changelog`: Cargo-native changelog generator
- GitHub's auto-generated release notes

**Recommended:** Use `git-cliff` with conventional commit parsing

**Configuration** (`cliff.toml`):
```toml
[changelog]
header = """
# Changelog\n
All notable changes to BranchBox will be documented here.\n
"""
body = """
{% for group, commits in commits | group_by(attribute="group") %}
    ### {{ group | upper_first }}
    {% for commit in commits %}
        - {{ commit.message | split(pat="\n") | first }}\
          {% if commit.breaking %} [**BREAKING**]{% endif %}
    {% endfor %}
{% endfor %}
"""
```

### 5. Installation Methods

#### macOS (Homebrew)

Create Homebrew tap at `github.com/branchbox/homebrew-tap`:

```ruby
class Branchbox < Formula
  desc "Distributed development environment orchestrator"
  homepage "https://github.com/branchbox/branchbox"
  version "1.0.0"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/branchbox/branchbox/releases/download/v1.0.0/branchbox-1.0.0-macos-x86_64.tar.gz"
      sha256 "..."
    else
      url "https://github.com/branchbox/branchbox/releases/download/v1.0.0/branchbox-1.0.0-macos-aarch64.tar.gz"
      sha256 "..."
    end
  end

  def install
    bin.install "branchbox"
    bash_completion.install "completions/branchbox.bash"
    zsh_completion.install "completions/branchbox.zsh"
    fish_completion.install "completions/branchbox.fish"
  end
end
```

**Installation:**
```bash
brew install branchbox/tap/branchbox
```

#### Linux (Install Script)

Create `install.sh` script hosted at releases:

```bash
#!/bin/bash
# Usage: curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | sh

set -e

# Detect architecture
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

# Map to release naming
case $ARCH in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

# Download latest release
VERSION=$(curl -s https://api.github.com/repos/branchbox/branchbox/releases/latest | grep -oP '"tag_name": "\K[^"]+')
URL="https://github.com/branchbox/branchbox/releases/download/$VERSION/branchbox-${VERSION#v}-${OS}-${ARCH}.tar.gz"

echo "Downloading branchbox $VERSION..."
curl -fsSL "$URL" | tar -xz -C /tmp

# Install to /usr/local/bin (requires sudo)
sudo mv /tmp/branchbox-*/branchbox /usr/local/bin/
sudo chmod +x /usr/local/bin/branchbox

echo "branchbox installed successfully!"
branchbox --version
```

#### Windows

**Options:**
1. **Chocolatey package** (long-term)
2. **Scoop manifest** (faster to set up)
3. **MSI installer** (professional, requires WiX toolset)

**Recommended:** Start with Scoop manifest

**Scoop manifest** (`branchbox.json`):
```json
{
  "version": "1.0.0",
  "description": "Distributed development environment orchestrator",
  "homepage": "https://github.com/branchbox/branchbox",
  "license": "MIT",
  "architecture": {
    "64bit": {
      "url": "https://github.com/branchbox/branchbox/releases/download/v1.0.0/branchbox-1.0.0-windows-x86_64.zip",
      "hash": "..."
    }
  },
  "bin": "branchbox.exe"
}
```

**Installation:**
```powershell
scoop bucket add branchbox https://github.com/branchbox/scoop-bucket
scoop install branchbox
```

### 6. README Integration

Update `README.md` with:

**Badges:**
```markdown
[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
```

**Installation section:**
```markdown
## Installation

### macOS (Homebrew)
```bash
brew install branchbox/tap/branchbox
```

### Linux
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | sh
```

### Windows (Scoop)
```powershell
scoop bucket add branchbox https://github.com/branchbox/scoop-bucket
scoop install branchbox
```

### From Source
```bash
cargo install --git https://github.com/branchbox/branchbox --locked branchbox-cli
```

### Download Binaries
Download pre-built binaries from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest).
```

## Implementation Milestones

### Milestone 1: Core Release Workflow (Week 1)

**Deliverables:**
- Create `.github/workflows/release.yml` with cross-platform builds
- Implement binary packaging logic (tar.gz for Unix, zip for Windows)
- Tag-triggered release automation (stable releases only)
- GitHub release creation with basic changelog

**Tasks:**
- [ ] Design release workflow YAML structure
- [ ] Set up cross-compilation for ARM targets
- [ ] Create packaging scripts for each platform
- [ ] Implement checksum generation
- [ ] Test workflow with pre-release tag

**Acceptance criteria:**
- Pushing a tag like `v0.2.0` triggers the workflow
- Workflow builds binaries for all platforms without errors
- GitHub release is created with uploaded artifacts
- Checksums file is generated and validated

### Milestone 2: Version Management (Week 2)

**Deliverables:**
- Integrate `cargo-release` for version bumping
- Configure workspace-level version synchronization
- Set up conventional commit parsing
- Document release process for maintainers

**Tasks:**
- [ ] Install and configure cargo-release
- [ ] Add release configuration to Cargo.toml
- [ ] Create maintainer release guide
- [ ] Test version bump workflow (dry-run)

**Acceptance criteria:**
- `cargo release --workspace --dry-run` validates successfully
- Version updates propagate to all workspace crates
- Git tags are created with proper format
- Documentation covers the full release flow

### Milestone 3: Changelog Automation (Week 2-3)

**Deliverables:**
- Install `git-cliff` for changelog generation
- Create `cliff.toml` configuration
- Integrate changelog into release workflow
- Generate CHANGELOG.md automatically

**Tasks:**
- [ ] Configure git-cliff with conventional commit groups
- [ ] Generate initial CHANGELOG.md from git history
- [ ] Add changelog generation to release workflow
- [ ] Include version-specific changelog in release archives

**Acceptance criteria:**
- Changelog is generated from commit history
- Sections are properly grouped (feat, fix, docs, etc.)
- Breaking changes are highlighted
- Changelog is attached to GitHub releases

### Milestone 4: Beta Release Support (Week 3)

**Deliverables:**
- Support pre-release tags (`v*.*.*-beta.*`)
- Mark GitHub releases as pre-release
- Document beta release workflow
- Test beta installation flow

**Tasks:**
- [ ] Extend workflow to detect pre-release tags
- [ ] Add `prerelease: true` flag to GitHub release
- [ ] Update release scripts to support `--pre-release` flag
- [ ] Validate beta artifacts are properly labeled

**Acceptance criteria:**
- Beta tags trigger the workflow correctly
- GitHub marks releases as "Pre-release"
- Beta versions are installable via direct download
- Beta releases don't update "latest" badge

### Milestone 5: Distribution Channels (Week 4-5)

**Deliverables:**
- Create Homebrew tap repository
- Write Linux install script
- Create Scoop manifest
- Automate Homebrew formula updates

**Tasks:**
- [ ] Set up `branchbox/homebrew-tap` repository
- [ ] Create Homebrew formula template
- [ ] Write and test install.sh script
- [ ] Create Scoop bucket repository
- [ ] Integrate formula updates into release workflow
- [ ] Document installation methods in README

**Acceptance criteria:**
- `brew install branchbox/tap/branchbox` works on macOS
- `curl | sh` install script works on Linux
- `scoop install branchbox` works on Windows
- Formula updates automatically on new releases

### Milestone 6: Documentation & Polish (Week 5-6)

**Deliverables:**
- Update README with installation instructions
- Add release badges
- Create RELEASING.md guide for maintainers
- Security: Binary signing (optional future enhancement)

**Tasks:**
- [ ] Update README installation section
- [ ] Add shields.io badges
- [ ] Write maintainer release guide
- [ ] Create release checklist template
- [ ] Test all installation methods end-to-end

**Acceptance criteria:**
- README clearly documents all installation options
- Badges reflect latest release and download counts
- RELEASING.md covers the full release process
- All installation methods are validated

## Architecture & Design Decisions

### Release Versioning Strategy

**Semantic Versioning:**
- `MAJOR.MINOR.PATCH` for stable releases
- `MAJOR.MINOR.PATCH-beta.N` for pre-releases

**Version bumping rules:**
- Breaking changes → MAJOR bump
- New features → MINOR bump
- Bug fixes → PATCH bump
- Pre-releases increment beta number

**Workspace versioning:**
All crates (`core`, `cli`, `agent`) share the same version number defined in `[workspace.package]`.

### Cross-Platform Build Strategy

**Target triples:**
- `x86_64-unknown-linux-gnu` (Linux x64)
- `aarch64-unknown-linux-gnu` (Linux ARM64)
- `x86_64-apple-darwin` (macOS Intel)
- `aarch64-apple-darwin` (macOS Apple Silicon)
- `x86_64-pc-windows-msvc` (Windows x64)

**Build optimization:**
- Use `--release` with LTO for smaller binaries
- Strip debug symbols with `strip` command
- Compress with tar/gzip (Unix) or zip (Windows)

**Cross-compilation:**
- Use `cross` tool for ARM Linux builds
- GitHub Actions provides native runners for x86_64
- macOS builds require macOS runners (GitHub provides both Intel and ARM)

### Release Artifact Security

**Checksums:**
- Generate SHA256 for each artifact
- Include checksums.txt in release assets
- Document verification in installation guides

**Future enhancements:**
- GPG signing of release artifacts
- Notarization for macOS binaries
- Code signing for Windows executables

### GitHub Release Automation

**Tools:**
- `gh` CLI for release creation
- `actions/upload-artifact` for asset management
- `softprops/action-gh-release` as alternative action

**Release notes format:**
```markdown
## What's Changed

[Auto-generated changelog from git-cliff]

## Installation

See [installation instructions](https://github.com/branchbox/branchbox#installation).

## Verify Downloads

```bash
# Download checksums
wget https://github.com/branchbox/branchbox/releases/download/v1.0.0/checksums.txt

# Verify archive
sha256sum -c checksums.txt
```

**Full Changelog**: https://github.com/branchbox/branchbox/compare/v0.1.0...v1.0.0
```

## Configuration Files

### Release Workflow (`.github/workflows/release.yml`)

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:
    inputs:
      version:
        description: 'Version to release (e.g., 1.0.0)'
        required: true

permissions:
  contents: write

jobs:
  create-release:
    name: Create Release
    runs-on: ubuntu-latest
    outputs:
      version: ${{ steps.version.outputs.version }}
      is_prerelease: ${{ steps.version.outputs.is_prerelease }}

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Parse version
        id: version
        run: |
          if [[ "${{ github.event_name }}" == "workflow_dispatch" ]]; then
            VERSION="${{ github.event.inputs.version }}"
          else
            VERSION="${GITHUB_REF#refs/tags/v}"
          fi

          echo "version=$VERSION" >> $GITHUB_OUTPUT

          if [[ "$VERSION" == *"-beta"* ]]; then
            echo "is_prerelease=true" >> $GITHUB_OUTPUT
          else
            echo "is_prerelease=false" >> $GITHUB_OUTPUT
          fi

      - name: Install git-cliff
        run: cargo install git-cliff

      - name: Generate changelog
        run: |
          git-cliff --tag v${{ steps.version.outputs.version }} > CHANGELOG.md

      - name: Create GitHub Release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release create "v${{ steps.version.outputs.version }}" \
            --title "v${{ steps.version.outputs.version }}" \
            --notes-file CHANGELOG.md \
            ${{ steps.version.outputs.is_prerelease == 'true' && '--prerelease' || '' }}

  build-release:
    name: Build ${{ matrix.target }}
    needs: create-release
    runs-on: ${{ matrix.os }}

    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            use_cross: true
          - target: x86_64-apple-darwin
            os: macos-13
          - target: aarch64-apple-darwin
            os: macos-14
          - target: x86_64-pc-windows-msvc
            os: windows-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross
        if: matrix.use_cross
        run: cargo install cross

      - name: Build
        run: |
          if [[ "${{ matrix.use_cross }}" == "true" ]]; then
            cross build --release --target ${{ matrix.target }}
          else
            cargo build --release --target ${{ matrix.target }}
          fi
        shell: bash

      - name: Package (Unix)
        if: runner.os != 'Windows'
        run: |
          cd target/${{ matrix.target }}/release
          tar czf branchbox-${{ needs.create-release.outputs.version }}-${{ matrix.target }}.tar.gz branchbox
          mv *.tar.gz $GITHUB_WORKSPACE/
        shell: bash

      - name: Package (Windows)
        if: runner.os == 'Windows'
        run: |
          cd target/${{ matrix.target }}/release
          7z a branchbox-${{ needs.create-release.outputs.version }}-${{ matrix.target }}.zip branchbox.exe
          mv *.zip $env:GITHUB_WORKSPACE/

      - name: Upload to Release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release upload "v${{ needs.create-release.outputs.version }}" branchbox-*
        shell: bash
```

### Cargo Release Config (root `Cargo.toml`)

```toml
[workspace.metadata.release]
sign-commit = false
sign-tag = false
pre-release-commit-message = "chore(release): prepare {{version}}"
tag-message = "Release {{version}}"
tag-prefix = "v"
consolidate-commits = true
pre-release-hook = ["cargo", "test", "--all"]
```

### git-cliff Config (`cliff.toml`)

```toml
[changelog]
header = """
# Changelog

All notable changes to BranchBox will be documented here.

"""

body = """
{% for group, commits in commits | group_by(attribute="group") %}
    ### {{ group | upper_first }}
    {% for commit in commits %}
        - {{ commit.message | split(pat="\n") | first | trim }}\
          {% if commit.breaking %} **[BREAKING]**{% endif %}
    {% endfor %}
{% endfor %}
"""

[git]
conventional_commits = true
filter_unconventional = false
split_commits = false
commit_parsers = [
  { message = "^feat", group = "Features" },
  { message = "^fix", group = "Bug Fixes" },
  { message = "^doc", group = "Documentation" },
  { message = "^perf", group = "Performance" },
  { message = "^refactor", group = "Refactoring" },
  { message = "^style", group = "Styling" },
  { message = "^test", group = "Testing" },
  { message = "^chore\\(release\\): prepare", skip = true },
  { message = "^chore", group = "Miscellaneous" },
]
```

## Testing Strategy

### Pre-Release Testing

**Manual validation checklist:**
- [ ] Run `cargo release --workspace --dry-run` succeeds
- [ ] All tests pass (`cargo test --all`)
- [ ] Clippy has no warnings
- [ ] Documentation builds without errors
- [ ] Version numbers are correct in all Cargo.toml files
- [ ] CHANGELOG.md is up to date

### Release Workflow Testing

**Test release process:**
1. Create test tag on feature branch: `git tag v0.0.0-test.1`
2. Push tag: `git push origin v0.0.0-test.1`
3. Verify workflow runs successfully
4. Download artifacts and test on each platform
5. Delete test release and tag

**Automated validation:**
- [ ] All platform builds complete successfully
- [ ] Artifacts are uploaded to release
- [ ] Checksums are generated correctly
- [ ] Release is marked pre-release for beta tags

### Installation Testing

**Validate each installation method:**
- [ ] Homebrew formula installs successfully
- [ ] Linux install script downloads and installs
- [ ] Scoop manifest installs on Windows
- [ ] Direct binary download works
- [ ] `branchbox --version` shows correct version
- [ ] Basic commands work after installation

## Dependencies

**Required tools/actions:**
- `cargo-release` (optional, for maintainers)
- `git-cliff` (for changelog generation)
- `cross` (for ARM cross-compilation)
- GitHub Actions (existing infrastructure)
- GitHub CLI (`gh`) for release management

**Optional future enhancements:**
- `cargo-dist` (alternative comprehensive solution)
- GPG for signing
- Notarization tools for macOS
- WiX toolset for Windows MSI

## Metrics & Success Criteria

**Key metrics:**
- Release automation success rate (target: 100%)
- Time from tag to published release (target: < 15 minutes)
- Download counts per platform
- Installation method adoption (Homebrew vs script vs direct)

**Success criteria:**
- Releases are fully automated (no manual steps except tagging)
- All platforms have downloadable binaries
- At least two installation methods available per platform
- README installation instructions are clear and tested
- Maintainer release process is documented

## Risks & Mitigations

**Risk: Cross-compilation failures**
- Mitigation: Use `cross` tool for ARM builds, test extensively

**Risk: GitHub rate limits**
- Mitigation: Use authenticated GitHub CLI, cache dependencies

**Risk: Breaking installation methods**
- Mitigation: Test installations in CI, maintain backwards compatibility

**Risk: Version mismatch across crates**
- Mitigation: Use workspace-level versioning, validate in CI

**Risk: Large binary sizes**
- Mitigation: Enable LTO, strip symbols, compress archives

## Open Questions

1. Should we sign binaries from day one, or add later?
2. Do we need Windows MSI installer, or is Scoop sufficient initially?
3. Should we publish to crates.io simultaneously with GitHub releases?
4. Do we want automatic Homebrew formula updates, or manual PR to tap?
5. Should we support automatic updates (self-update command)?

## Future Enhancements

### Phase 2: Distribution Expansion
- Publish to crates.io
- AUR package for Arch Linux
- Snap package for Ubuntu
- Docker images on Docker Hub/GHCR
- Chocolatey package for Windows

### Phase 3: Update Management
- `branchbox update` command for self-updating
- Update notifications
- Release channels (stable, beta, nightly)

### Phase 4: Enhanced Security
- Binary signing with GPG
- macOS notarization
- Windows code signing
- Supply chain security (SLSA)

### Phase 5: Analytics & Telemetry
- Anonymous download tracking
- Platform/architecture usage stats
- Version adoption rates
- Installation method analytics

## References

**Similar tools for inspiration:**
- [Rust `cargo-dist` documentation](https://github.com/axodotdev/cargo-dist)
- [GitHub Actions release examples](https://github.com/actions/starter-workflows/blob/main/ci/rust.yml)
- [Homebrew formula guidelines](https://docs.brew.sh/Formula-Cookbook)
- [Scoop manifest reference](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)

**Cargo tooling:**
- [cargo-release](https://github.com/crate-ci/cargo-release)
- [git-cliff](https://github.com/orhun/git-cliff)
- [cross](https://github.com/cross-rs/cross)

## Maintainer Release Guide (Draft)

### Creating a Release

1. **Prepare the release:**
   ```bash
   # Ensure main branch is clean and up-to-date
   git checkout main
   git pull origin main

   # Run all checks
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-features

   # Dry-run release
   cargo release --workspace --dry-run
   ```

2. **Create and push tag:**
   ```bash
   # For stable release
   cargo release --workspace --execute

   # For beta release
   cargo release --workspace --pre-release --execute

   # Push tags
   git push --follow-tags
   ```

3. **Monitor workflow:**
   ```bash
   # Watch release workflow
   gh run watch
   ```

4. **Verify release:**
   - Check GitHub release page
   - Download and test artifacts
   - Verify installation methods work
   - Update Homebrew formula if needed

5. **Announce:**
   - Post release notes
   - Update documentation
   - Notify community channels

### Emergency Rollback

If a release has critical issues:

1. Mark GitHub release as draft
2. Create hotfix branch
3. Fix issue and create new patch release
4. Delete problematic tag (if not widely distributed)
