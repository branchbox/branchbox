---
work_feature: install-scripts
status: backlog
created: 2025-10-27
updated: 2025-10-27
---

# Cross-Platform Installation Scripts

## Overview

Create installation scripts and package manifests to enable easy installation of BranchBox on Linux and Windows. This includes a curl-able shell script for Linux and a Scoop manifest for Windows, both leveraging the automated GitHub Releases created by the release pipeline.

## Background

With the release automation infrastructure complete (see `docs/features/completed/release-automation.md`), BranchBox now publishes pre-built binaries for Linux (x86_64, ARM64) and Windows (x86_64). This spec covers making these binaries easily installable via platform-native mechanisms.

## Goals

### Linux Installation Script

1. **Create `install.sh` Script**
   - Curl-able one-liner: `curl -fsSL https://... | sh`
   - Automatic architecture detection (x86_64 vs ARM64)
   - Download latest or specific version from GitHub Releases
   - Verify SHA256 checksums
   - Install to `/usr/local/bin` (with sudo) or `~/.local/bin` (without sudo)
   - Provide clear output and error messages

2. **Host Installation Script**
   - Add `install.sh` to main repository
   - Make accessible via raw GitHub URL
   - Document usage in README

### Windows Installation (Scoop)

3. **Create Scoop Manifest**
   - Set up `branchbox/scoop-bucket` repository
   - Create `branchbox.json` manifest
   - Configure automatic version detection
   - Verify SHA256 checksums

4. **Automate Manifest Updates**
   - Add workflow step to update Scoop manifest on releases
   - Generate correct download URLs and checksums

### Testing & Validation

5. **End-to-End Testing**
   - Test Linux install script on multiple distros
   - Test Scoop installation on Windows 10/11
   - Validate checksum verification works
   - Test installation as root and non-root (Linux)
   - Test updates and uninstallation

6. **Documentation**
   - Update README with installation instructions
   - Document troubleshooting steps
   - Create video/GIF demos (optional)

## Technical Requirements

### Linux Install Script

**Location:** `install.sh` in repository root

**Script features:**
- Architecture detection (x86_64, aarch64)
- OS detection (Linux only for now, could extend to macOS)
- Version selection (latest or specific via `BRANCHBOX_VERSION` env var)
- Download from GitHub Releases
- Checksum verification
- Installation path selection (auto-detect sudo availability)
- Cleanup on failure
- Idempotent (can run multiple times)

**Script template:**
```bash
#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REPO="branchbox/branchbox"
BINARY_NAME="branchbox"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect architecture
detect_arch() {
  local arch
  arch=$(uname -m)

  case "$arch" in
    x86_64|amd64)
      echo "x86_64"
      ;;
    aarch64|arm64)
      echo "aarch64"
      ;;
    *)
      echo "${RED}Error: Unsupported architecture: $arch${NC}" >&2
      exit 1
      ;;
  esac
}

# Detect OS
detect_os() {
  local os
  os=$(uname -s | tr '[:upper:]' '[:lower:]')

  case "$os" in
    linux)
      echo "linux"
      ;;
    *)
      echo "${RED}Error: Unsupported OS: $os${NC}" >&2
      echo "${YELLOW}Use Homebrew for macOS: brew install branchbox/tap/branchbox${NC}" >&2
      exit 1
      ;;
  esac
}

# Get latest version
get_latest_version() {
  local version
  version=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep -oP '"tag_name": "\K[^"]+')

  if [ -z "$version" ]; then
    echo "${RED}Error: Could not fetch latest version${NC}" >&2
    exit 1
  fi

  echo "$version"
}

# Main installation
main() {
  echo "${GREEN}BranchBox Installer${NC}"
  echo ""

  # Detect system
  local arch os version
  arch=$(detect_arch)
  os=$(detect_os)

  echo "Detected: $os-$arch"

  # Get version
  if [ -n "$BRANCHBOX_VERSION" ]; then
    version="$BRANCHBOX_VERSION"
    echo "Installing version: $version (from BRANCHBOX_VERSION)"
  else
    version=$(get_latest_version)
    echo "Installing latest version: $version"
  fi

  # Build download URL
  local version_number="${version#v}"  # Remove 'v' prefix
  local target="${arch}-unknown-${os}-gnu"
  local archive_name="branchbox-${version_number}-${target}.tar.gz"
  local download_url="https://github.com/$REPO/releases/download/$version/$archive_name"
  local checksum_url="https://github.com/$REPO/releases/download/$version/checksums.txt"

  echo "Downloading from: $download_url"

  # Create temp directory
  local tmp_dir
  tmp_dir=$(mktemp -d)
  trap "rm -rf '$tmp_dir'" EXIT

  # Download archive
  if ! curl -fsSL "$download_url" -o "$tmp_dir/$archive_name"; then
    echo "${RED}Error: Failed to download $archive_name${NC}" >&2
    exit 1
  fi

  # Download and verify checksum
  echo "Verifying checksum..."
  if ! curl -fsSL "$checksum_url" -o "$tmp_dir/checksums.txt"; then
    echo "${YELLOW}Warning: Could not download checksums, skipping verification${NC}" >&2
  else
    cd "$tmp_dir"
    if ! sha256sum -c checksums.txt --ignore-missing 2>/dev/null; then
      echo "${RED}Error: Checksum verification failed${NC}" >&2
      exit 1
    fi
    echo "${GREEN}Checksum verified${NC}"
  fi

  # Extract archive
  echo "Extracting..."
  tar xzf "$tmp_dir/$archive_name" -C "$tmp_dir"

  # Determine install location
  if [ -w "$INSTALL_DIR" ]; then
    # Can write without sudo
    echo "Installing to $INSTALL_DIR (no sudo required)"
    mv "$tmp_dir/branchbox-${version_number}-${target}/$BINARY_NAME" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
  elif command -v sudo >/dev/null 2>&1; then
    # Need sudo
    echo "Installing to $INSTALL_DIR (requires sudo)"
    sudo mv "$tmp_dir/branchbox-${version_number}-${target}/$BINARY_NAME" "$INSTALL_DIR/"
    sudo chmod +x "$INSTALL_DIR/$BINARY_NAME"
  else
    # Fallback to user bin
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
    echo "Installing to $INSTALL_DIR (user installation)"
    mv "$tmp_dir/branchbox-${version_number}-${target}/$BINARY_NAME" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"

    # Check if in PATH
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
      echo ""
      echo "${YELLOW}Warning: $INSTALL_DIR is not in your PATH${NC}"
      echo "Add this to your ~/.bashrc or ~/.zshrc:"
      echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
  fi

  echo ""
  echo "${GREEN}✓ BranchBox installed successfully!${NC}"
  echo ""
  echo "Verify installation:"
  echo "  $BINARY_NAME --version"
  echo ""
  echo "Get started:"
  echo "  $BINARY_NAME --help"
}

main
```

**Installation methods:**
```bash
# Latest version (interactive)
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | sh

# Specific version
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | BRANCHBOX_VERSION=v0.2.0 sh

# Custom install directory
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | INSTALL_DIR=/opt/bin sh

# Download and inspect first
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh -o install.sh
chmod +x install.sh
./install.sh
```

### Windows Scoop Manifest

**Repository:** `github.com/branchbox/scoop-bucket`

**Structure:**
```
scoop-bucket/
├── README.md
├── bucket/
│   └── branchbox.json
└── .github/
    └── workflows/
        └── excavator.yml (Scoop's auto-update bot)
```

**Manifest template** (`bucket/branchbox.json`):
```json
{
  "version": "0.2.0",
  "description": "Distributed development environment orchestrator",
  "homepage": "https://github.com/branchbox/branchbox",
  "license": "MIT",
  "architecture": {
    "64bit": {
      "url": "https://github.com/branchbox/branchbox/releases/download/v0.2.0/branchbox-0.2.0-x86_64-pc-windows-msvc.zip",
      "hash": "sha256:CHECKSUM_HERE",
      "extract_dir": "branchbox-0.2.0-x86_64-pc-windows-msvc"
    }
  },
  "bin": "branchbox.exe",
  "checkver": {
    "github": "https://github.com/branchbox/branchbox"
  },
  "autoupdate": {
    "architecture": {
      "64bit": {
        "url": "https://github.com/branchbox/branchbox/releases/download/v$version/branchbox-$version-x86_64-pc-windows-msvc.zip",
        "extract_dir": "branchbox-$version-x86_64-pc-windows-msvc"
      }
    },
    "hash": {
      "url": "$baseurl/checksums.txt",
      "regex": "(?<hash>[a-f0-9]{64})  branchbox-$version-x86_64-pc-windows-msvc.zip"
    }
  }
}
```

**Scoop autoupdate:**
Scoop provides an "excavator" bot that can automatically update manifests. Configure `.github/workflows/excavator.yml`:

```yaml
name: Excavator

on:
  workflow_dispatch:
  schedule:
    - cron: '0 */6 * * *'  # Every 6 hours

jobs:
  excavate:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - name: Excavate
        uses: ScoopInstaller/GithubActions@main
        env:
          GITH_EMAIL: ${{ secrets.GITH_EMAIL }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

**Manual update workflow (alternative):**

Add to main repo's `.github/workflows/release.yml`:

```yaml
update-scoop:
  name: Update Scoop Manifest
  needs: [create-release, build-release, publish-release]
  runs-on: ubuntu-latest

  steps:
    - name: Checkout scoop-bucket repo
      uses: actions/checkout@v4
      with:
        repository: branchbox/scoop-bucket
        token: ${{ secrets.SCOOP_BUCKET_TOKEN }}
        path: scoop-bucket

    - name: Download checksums
      run: |
        VERSION="${{ needs.create-release.outputs.version }}"
        curl -fsSL "https://github.com/branchbox/branchbox/releases/download/v$VERSION/checksums.txt" -o checksums.txt

    - name: Extract checksum
      id: checksum
      run: |
        CHECKSUM=$(grep "x86_64-pc-windows-msvc" checksums.txt | awk '{print $1}')
        echo "windows=$CHECKSUM" >> $GITHUB_OUTPUT

    - name: Update manifest
      run: |
        VERSION="${{ needs.create-release.outputs.version }}"
        cd scoop-bucket/bucket

        # Update using jq
        jq --arg version "$VERSION" \
           --arg url "https://github.com/branchbox/branchbox/releases/download/v$VERSION/branchbox-$VERSION-x86_64-pc-windows-msvc.zip" \
           --arg hash "sha256:${{ steps.checksum.outputs.windows }}" \
           --arg extract_dir "branchbox-$VERSION-x86_64-pc-windows-msvc" \
           '.version = $version | .architecture["64bit"].url = $url | .architecture["64bit"].hash = $hash | .architecture["64bit"].extract_dir = $extract_dir' \
           branchbox.json > branchbox.json.tmp

        mv branchbox.json.tmp branchbox.json

    - name: Commit and push
      run: |
        cd scoop-bucket
        git config user.name "github-actions[bot]"
        git config user.email "github-actions[bot]@users.noreply.github.com"
        git add bucket/branchbox.json
        git commit -m "branchbox: Update to version ${{ needs.create-release.outputs.version }}"
        git push
```

## Implementation Tasks

### Phase 1: Linux Install Script (2-3 days)

- [ ] Create `install.sh` in repository root
- [ ] Implement architecture detection (x86_64, aarch64)
- [ ] Implement download and checksum verification
- [ ] Implement install path selection (with/without sudo)
- [ ] Add colored output and error handling
- [ ] Test on Ubuntu, Debian, Fedora, Arch Linux
- [ ] Test as root and non-root user
- [ ] Test with specific version and custom install dir

**Deliverables:**
- Functional `install.sh` script in repository
- Tested on multiple Linux distributions
- Documentation in README

### Phase 2: Windows Scoop Manifest (1-2 days)

- [ ] Create `branchbox/scoop-bucket` repository
- [ ] Create `bucket/branchbox.json` manifest
- [ ] Configure autoupdate with excavator bot
- [ ] Test installation with Scoop on Windows 10/11
- [ ] Test updates and uninstallation
- [ ] Optional: Add manual update workflow to release pipeline

**Deliverables:**
- Scoop bucket repository with manifest
- Tested installation on Windows
- Documentation in README

### Phase 3: Documentation & Validation (1-2 days)

- [ ] Update main repo README with all installation methods
- [ ] Create troubleshooting guide
- [ ] Test end-to-end: release → install → verify on all platforms
- [ ] Create installation demo GIFs (optional)
- [ ] Document manual fallback for users who can't use scripts

**Deliverables:**
- Comprehensive installation documentation
- Validated installation flow
- Troubleshooting guide

## User Experience

### Linux Installation

**One-line install:**
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | sh
```

**Output:**
```
BranchBox Installer

Detected: linux-x86_64
Installing latest version: v0.2.0
Downloading from: https://github.com/branchbox/branchbox/releases/download/v0.2.0/branchbox-0.2.0-x86_64-unknown-linux-gnu.tar.gz
Verifying checksum...
Checksum verified
Extracting...
Installing to /usr/local/bin (requires sudo)
[sudo] password for user:

✓ BranchBox installed successfully!

Verify installation:
  branchbox --version

Get started:
  branchbox --help
```

**Specific version:**
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | BRANCHBOX_VERSION=v0.1.0 sh
```

**Custom install directory:**
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | INSTALL_DIR=$HOME/bin sh
```

### Windows Installation (Scoop)

**Adding the bucket:**
```powershell
scoop bucket add branchbox https://github.com/branchbox/scoop-bucket
```

**Installing:**
```powershell
scoop install branchbox
```

**One-line install:**
```powershell
scoop bucket add branchbox https://github.com/branchbox/scoop-bucket; scoop install branchbox
```

**Updating:**
```powershell
scoop update branchbox
```

**Uninstalling:**
```powershell
scoop uninstall branchbox
```

## Documentation Updates

### Main Repo README

Update installation section:
```markdown
## Installation

### macOS

```bash
brew install branchbox/tap/branchbox
```

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | sh
```

Or download binaries from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest).

### Windows

#### Scoop

```powershell
scoop bucket add branchbox https://github.com/branchbox/scoop-bucket
scoop install branchbox
```

#### Direct Download

Download the Windows binary from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest).

### From Source

```bash
cargo install --git https://github.com/branchbox/branchbox --locked branchbox-cli
```

### Verifying Installation

```bash
branchbox --version
```
```

### Install Script Troubleshooting

Add to main repo `README.md` or create `docs/INSTALLATION.md`:

```markdown
## Installation Troubleshooting

### Linux Install Script

**Q: The script says my architecture is unsupported**
A: BranchBox currently supports x86_64 and aarch64 (ARM64) on Linux. For other architectures, try building from source.

**Q: Installation fails with permission denied**
A: Try installing to a user directory:
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | INSTALL_DIR=$HOME/.local/bin sh
```

Then add `~/.local/bin` to your PATH:
```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

**Q: Checksum verification fails**
A: This could indicate a corrupted download or a network issue. Try running the installer again.

**Q: I don't want to pipe curl to sh**
A: You can download and inspect the script first:
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh -o install.sh
less install.sh  # Inspect the script
chmod +x install.sh
./install.sh
```

### Windows Scoop

**Q: Scoop install fails**
A: Make sure you have Scoop installed first:
```powershell
Set-ExecutionPolicy RemoteSigned -Scope CurrentUser
irm get.scoop.sh | iex
```

**Q: branchbox.exe is not recognized**
A: Restart your terminal or run:
```powershell
scoop reset branchbox
```
```

## Testing Matrix

### Linux Distributions

Test on:
- [ ] Ubuntu 22.04 LTS (x86_64)
- [ ] Ubuntu 22.04 LTS (ARM64)
- [ ] Debian 12 (x86_64)
- [ ] Fedora 39 (x86_64)
- [ ] Arch Linux (x86_64)
- [ ] Alpine Linux (x86_64) - musl target (future)

### Windows Versions

Test on:
- [ ] Windows 11 (x64)
- [ ] Windows 10 (x64)
- [ ] Windows Server 2022 (optional)

### Installation Scenarios

Test:
- [ ] Fresh installation (no existing branchbox)
- [ ] Upgrade installation (existing branchbox)
- [ ] Installation with sudo
- [ ] Installation without sudo (user directory)
- [ ] Installation with custom INSTALL_DIR
- [ ] Installation with specific version
- [ ] Checksum verification (valid and invalid)
- [ ] Network failure handling
- [ ] Uninstallation

## Dependencies

**Required:**
- curl (Linux install script)
- tar (Linux install script)
- sha256sum (Linux install script)
- sudo (Linux, optional)
- Scoop (Windows)
- jq (for automated Scoop manifest updates)

**For Testing:**
- VMs or containers for each Linux distro
- Windows VM or machine for Scoop testing

## Risks & Mitigations

**Risk: Users don't trust piping curl to sh**
- Mitigation: Provide alternative installation methods
- Mitigation: Host script in main repo for transparency
- Mitigation: Document how to inspect script before running

**Risk: Architecture detection fails**
- Mitigation: Thorough testing on various systems
- Mitigation: Clear error messages
- Mitigation: Manual fallback instructions

**Risk: Checksum URL changes**
- Mitigation: Test with actual releases
- Mitigation: Fallback to skip verification with warning

**Risk: Scoop bucket not maintained**
- Mitigation: Set up excavator bot for auto-updates
- Mitigation: Monitor bucket for issues
- Mitigation: Document manual update process

**Risk: Windows antivirus blocks binary**
- Mitigation: Code signing (future enhancement)
- Mitigation: Document how to whitelist
- Mitigation: Submit to Windows Defender for analysis

## Success Criteria

### Linux
- [ ] Install script successfully installs on all tested distros
- [ ] Checksum verification works correctly
- [ ] Installation with and without sudo works
- [ ] Script provides clear output and errors
- [ ] Uninstallation instructions are clear

### Windows
- [ ] Scoop manifest successfully installs branchbox
- [ ] Binary runs without Windows Defender warnings
- [ ] Updates work via `scoop update`
- [ ] Uninstallation via `scoop uninstall` works

### Documentation
- [ ] README has complete installation instructions
- [ ] Troubleshooting guide covers common issues
- [ ] Installation verified on clean systems

## Future Enhancements

### Additional Package Managers

**Linux:**
- **Snap package** - Universal Linux package
- **AppImage** - Self-contained executable
- **AUR (Arch User Repository)** - For Arch Linux users
- **Flatpak** - Container-based distribution

**Windows:**
- **Chocolatey package** - Alternative to Scoop
- **winget manifest** - Windows Package Manager
- **MSI installer** - Traditional Windows installer

### Improvements

- **Progress bars** for downloads
- **Automatic PATH detection** and setup
- **Shell completion installation** (once implemented)
- **Configuration file creation** (once config is added)
- **Telemetry opt-in** during installation (optional)
- **Update notifications** ("new version available")

### Security

- **Code signing** for Windows binary
- **GPG signatures** for Linux binaries
- **HTTPS verification** in install script
- **Reproducible builds** for supply chain security

## References

**Installation Script Examples:**
- [Rust installer (rustup)](https://sh.rustup.rs)
- [Deno installer](https://deno.land/install.sh)
- [Volta installer](https://get.volta.sh)

**Scoop Documentation:**
- [Creating a Bucket](https://github.com/ScoopInstaller/Scoop/wiki/Buckets)
- [App Manifests](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)
- [Excavator (auto-update)](https://github.com/ScoopInstaller/GithubActions)

**Related Specs:**
- `docs/features/completed/release-automation.md` - Release infrastructure
- `docs/features/backlog/homebrew-tap-distribution.md` - macOS Homebrew

**Workflow:**
- `.github/workflows/release.yml` - Main repo release workflow
