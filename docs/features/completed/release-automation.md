---
work_feature: release-automation
branch: feature/release-automation
status: completed
created: 2025-10-27
completed: 2025-10-27
updated: 2025-10-27
---

# Automated Release Management & Distribution

## Overview

Implemented a continuous deployment pipeline for BranchBox that automates building, packaging, and publishing releases across multiple platforms. The system supports both beta (pre-release) and stable releases, creates GitHub releases with downloadable artifacts, and provides version management and changelog automation.

## What Was Implemented

### Core Release Infrastructure (Milestones 1-4)

**Release Workflow** (`.github/workflows/release.yml`)
- Three-job pipeline: `create-release` → `build-release` → `publish-release`
- Tag-triggered automation (`v*` pattern) and manual `workflow_dispatch`
- Draft-first pattern: creates draft release, builds all platforms, then publishes
- Pre-release detection for beta/alpha/rc versions (auto-flagged on GitHub)

**Cross-Platform Binary Builds**
- Linux x86_64 (`x86_64-unknown-linux-gnu`)
- Linux ARM64 (`aarch64-unknown-linux-gnu`) with `cross` tool
- macOS Intel (`x86_64-apple-darwin`) on macos-13 runner
- macOS Apple Silicon (`aarch64-apple-darwin`) on macos-14 runner
- Windows x64 (`x86_64-pc-windows-msvc`)

**Binary Packaging**
- Platform-specific archives: tar.gz (Unix), zip (Windows)
- Archive naming: `branchbox-{version}-{target}.{ext}`
- Archive contents: binary, README.md, LICENSE, CHANGELOG.md
- Binary stripping for smaller sizes (Linux, macOS)
- SHA256 checksums (individual + consolidated `checksums.txt`)

**Version Management**
- `cargo-release` configuration in `Cargo.toml`
- Pre-release hooks: `cargo test --all`
- Conventional commit message templates
- Tag prefix: `v`
- Workspace-level version synchronization

**Changelog Automation**
- `git-cliff` integration with conventional commits parsing
- `cliff.toml` configuration with commit grouping
- Breaking change detection and highlighting
- Auto-generated release notes with installation instructions
- `CHANGELOG.md` generated from git history

**Documentation**
- `RELEASING.md` - Comprehensive maintainer release guide (405 lines)
  - Step-by-step stable and pre-release workflows
  - Prerequisites and tool installation
  - Emergency rollback procedures
  - Troubleshooting guide
- README badges (Release, Downloads, CI, License)
- README installation section (direct download instructions)
- Checksum verification instructions in release notes

## Files Created/Modified

```
.github/workflows/release.yml    # Release automation workflow (352 lines)
Cargo.toml                        # Added [workspace.metadata.release]
cliff.toml                        # git-cliff configuration (93 lines)
CHANGELOG.md                      # Initial changelog
RELEASING.md                      # Maintainer guide (405 lines)
README.md                         # Added badges and installation section
```

## Architecture

### Release Pipeline

1. **create-release job** (ubuntu-latest)
   - Parse tag to extract version and detect pre-release
   - Install git-cliff and generate changelog
   - Create draft GitHub release with release notes
   - Upload changelog artifact for other jobs

2. **build-release job** (matrix: 5 platforms)
   - Install Rust toolchain for target
   - Build with `cargo build --release --package branchbox-cli`
   - Strip binary (Linux/macOS only)
   - Create archive directory with binary + docs
   - Package as tar.gz (Unix) or zip (Windows)
   - Generate SHA256 checksum
   - Upload artifact for publish job

3. **publish-release job** (ubuntu-latest)
   - Download all build artifacts
   - Consolidate checksums into single file
   - Upload all archives to GitHub release
   - Upload consolidated checksums.txt
   - Mark release as published (remove draft status)

### Version Bumping Workflow (Maintainers)

```bash
# Stable release
cargo release --workspace --execute
git push --follow-tags

# Pre-release
cargo release --workspace --pre-release beta --execute
git push --follow-tags
```

Tag push triggers the release workflow automatically.

## Testing Results

**Test iterations:** 5 test releases (v0.0.0-test.1 through v0.0.0-test.5)

**Issues found and fixed:**
1. Package ambiguity → Added `--package branchbox-cli`
2. Cargo.lock sync → Removed `--locked` flag
3. OpenSSL missing (ARM) → Added `vendored-openssl` and `vendored-libgit2` features
4. macOS checksum command → Split to use `shasum -a 256` on macOS
5. Bash syntax in upload loop → Added `shopt -s nullglob`
6. GitHub Actions conditionals → Changed `!matrix.use_cross` to `matrix.use_cross != true`

**Final validation (v0.0.0-test.5):**
- ✅ All 5 platform builds completed successfully
- ✅ All binaries packaged correctly
- ✅ SHA256 checksums generated for all artifacts
- ✅ All artifacts uploaded to GitHub release
- ✅ Release published successfully

## Technical Details

### Cross-Compilation

**ARM Linux builds** use the `cross` tool:
```yaml
- name: Install cross
  if: matrix.use_cross
  run: cargo install cross --locked

- name: Build
  run: |
    if [[ "${{ matrix.use_cross }}" == "true" ]]; then
      cross build --release --target ${{ matrix.target }} --package branchbox-cli
    else
      cargo build --release --target ${{ matrix.target }} --package branchbox-cli
    fi
```

**OpenSSL vendoring** required for ARM cross-compilation:
```toml
# In core/Cargo.toml
git2 = { version = "0.18", features = ["vendored-openssl", "vendored-libgit2"] }
```

### Checksum Generation

Platform-specific commands:
```bash
# Linux
sha256sum "$ASSET_PATH" > "$ASSET_PATH.sha256"

# macOS
shasum -a 256 "$ASSET_PATH" > "$ASSET_PATH.sha256"

# Windows PowerShell
$hash = (Get-FileHash "$ASSET_PATH" -Algorithm SHA256).Hash.ToLower()
"$hash  $filename" | Out-File -FilePath "$ASSET_PATH.sha256" -Encoding ASCII
```

All individual checksums are consolidated in the publish job:
```bash
find artifacts -name "*.sha256" -exec cat {} \; > checksums.txt
```

### Pre-Release Detection

Automatic detection based on version string:
```bash
if [[ "$VERSION" == *"-beta"* ]] || [[ "$VERSION" == *"-alpha"* ]] || [[ "$VERSION" == *"-rc"* ]]; then
  echo "is_prerelease=true" >> $GITHUB_OUTPUT
else
  echo "is_prerelease=false" >> $GITHUB_OUTPUT
fi
```

Passed to release creation:
```bash
gh release create "v$VERSION" \
  --title "v$VERSION" \
  --notes-file RELEASE_CHANGELOG.md \
  ${{ steps.version.outputs.is_prerelease == 'true' && '--prerelease' || '' }} \
  --draft
```

## Usage

### For Maintainers

**Creating a stable release:**
```bash
cargo release --workspace --dry-run  # Preview
cargo release --workspace --execute  # Execute
git push --follow-tags              # Trigger workflow
gh run watch                        # Monitor
```

**Creating a beta release:**
```bash
cargo release --workspace --pre-release beta --execute
git push --follow-tags
```

See `RELEASING.md` for full maintainer guide.

### For Users

**Download binaries** from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest)

**Verify checksums:**
```bash
# Linux/macOS
curl -fsSL https://github.com/branchbox/branchbox/releases/download/v0.2.0/checksums.txt -o checksums.txt
sha256sum -c checksums.txt --ignore-missing

# Windows PowerShell
Get-FileHash branchbox-0.2.0-windows-x86_64.zip -Algorithm SHA256
# Compare with checksums.txt
```

## Configuration

### cargo-release (Cargo.toml)

```toml
[workspace.metadata.release]
sign-commit = false
sign-tag = false
pre-release-commit-message = "chore(release): prepare {{version}}"
tag-message = "Release {{version}}"
tag-prefix = "v"
consolidate-commits = true
pre-release-hook = ["cargo", "test", "--all"]
publish = false  # No crates.io publishing yet
```

### git-cliff (cliff.toml)

```toml
[git]
conventional_commits = true
commit_parsers = [
  { message = "^feat", group = "Features" },
  { message = "^fix", group = "Bug Fixes" },
  { message = "^doc", group = "Documentation" },
  { message = "^perf", group = "Performance" },
  { message = "^refactor", group = "Refactoring" },
  { message = "^test", group = "Testing" },
  { message = "^chore\\(release\\): prepare", skip = true },
  { message = "^chore", group = "Miscellaneous" },
]
```

## Dependencies

**Required for release workflow:**
- GitHub Actions (existing infrastructure)
- `cross` (installed in workflow for ARM builds)
- `git-cliff` (installed in workflow for changelog)

**Required for maintainers:**
- `cargo-release`: `cargo install cargo-release --locked`
- `git-cliff`: `cargo install git-cliff --locked`
- GitHub CLI: `brew install gh` (or platform equivalent)

## Metrics

**Workflow performance:**
- Total time: ~15-20 minutes for all 5 platforms
- create-release job: ~2 minutes
- build-release job: ~8-12 minutes per platform (parallel)
- publish-release job: ~1 minute

**Binary sizes** (v0.0.0-test.5, stripped):
- Linux x86_64: ~15 MB
- macOS aarch64: ~12 MB
- Windows x64: ~18 MB

**Release artifacts:**
- 5 platform archives
- 5 individual checksum files
- 1 consolidated checksums.txt
- Auto-generated changelog in release notes

## Future Enhancements

The following were specified but not implemented (see backlog specs):

- **Distribution channels** (Milestone 5)
  - Homebrew tap for macOS
  - Linux install script (curl | sh)
  - Scoop manifest for Windows
  - Automated formula updates

- **Additional testing** (Milestone 6)
  - End-to-end installation validation
  - Multi-platform smoke tests

- **Phase 2+** (Long-term)
  - Binary signing (GPG, notarization, code signing)
  - crates.io publishing
  - Self-update command
  - Docker images
  - Additional package managers (AUR, Snap, Chocolatey)

## References

**Related specs:**
- `docs/features/backlog/homebrew-tap-distribution.md` - Homebrew packaging
- `docs/features/backlog/install-scripts.md` - Linux/Windows install scripts

**Documentation:**
- `RELEASING.md` - Maintainer release guide
- `CHANGELOG.md` - Version history
- `.github/workflows/release.yml` - Workflow source

**Tools:**
- [cargo-release](https://github.com/crate-ci/cargo-release)
- [git-cliff](https://github.com/orhun/git-cliff)
- [cross](https://github.com/cross-rs/cross)
- [GitHub CLI](https://cli.github.com/)

## Known Issues

1. **Repository URL in Cargo.toml** is incorrect
   - Current: `https://github.com/branchbox-branchbox`
   - Should be: `https://github.com/branchbox/branchbox`
   - Status: Tracked in PR feedback, fix required before first release

2. **Placeholder author metadata** in Cargo.toml
   - Current: `Your Name <you@example.com>`
   - Status: Needs update before first release

## Success Criteria (Met)

- ✅ Releases are fully automated (tag push triggers workflow)
- ✅ All major platforms have downloadable binaries (5 targets)
- ✅ Direct binary download method available
- ✅ README installation instructions present
- ✅ Maintainer release process documented
- ✅ Pre-release (beta) support implemented
- ✅ Changelog automatically generated
- ✅ Checksums provided for verification
- ✅ Workflow tested and validated (5 test iterations)
