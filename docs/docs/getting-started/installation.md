---
sidebar_position: 1
---

# Installation Guide

This guide covers all installation methods for BranchBox across different platforms.

## Quick Install

### macOS (Homebrew)

```bash
brew install branchbox/tap/branchbox
```

### Linux / macOS (installer script)

Install with our automated script:

```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
```

**Options:**

```bash
# Install specific version
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | BRANCHBOX_VERSION=v0.1.0 bash

# Install to custom directory
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | INSTALL_DIR=$HOME/bin bash

# Download and inspect script first (recommended)
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh -o install.sh
less install.sh  # Review the script
chmod +x install.sh
./install.sh
```

**What the script does:**
- Detects your architecture (x86_64 or ARM64)
- Detects your OS (Linux or macOS)
- Downloads the latest release from GitHub
- Verifies SHA256 checksums for the specific downloaded archive
- Installs to `/usr/local/bin` (Linux), `/opt/homebrew/bin` (macOS when available), or `~/.local/bin` fallback
- Provides clear output and error messages

### Windows (Scoop)

*Coming soon - Scoop package will be available in a future release.*

```powershell
scoop bucket add branchbox https://github.com/branchbox/scoop-bucket
scoop install branchbox
```

## Download Binaries

Download pre-built binaries from [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest):

### Linux

```bash
# Download (replace VERSION and ARCH as needed)
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-x86_64-unknown-linux-gnu.tar.gz -o branchbox.tar.gz

# Verify checksum
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/checksums.txt -o checksums.txt
sha256sum -c checksums.txt --ignore-missing

# Extract
tar xzf branchbox.tar.gz

# Install (requires sudo)
sudo mv branchbox-VERSION-x86_64-unknown-linux-gnu/branchbox /usr/local/bin/
sudo chmod +x /usr/local/bin/branchbox

# Verify installation
branchbox --version
```

### macOS

```bash
# Download (Intel)
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-x86_64-apple-darwin.tar.gz -o branchbox.tar.gz

# Download (Apple Silicon)
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-aarch64-apple-darwin.tar.gz -o branchbox.tar.gz

# Verify checksum
curl -fsSL https://github.com/branchbox/branchbox/releases/download/vVERSION/checksums.txt -o checksums.txt
shasum -a 256 -c checksums.txt --ignore-missing

# Extract
tar xzf branchbox.tar.gz

# Install (requires sudo)
sudo mv branchbox-VERSION-*/branchbox /usr/local/bin/
sudo chmod +x /usr/local/bin/branchbox

# Verify installation
branchbox --version
```

### Windows

```powershell
# Download
Invoke-WebRequest -Uri "https://github.com/branchbox/branchbox/releases/download/vVERSION/branchbox-VERSION-x86_64-pc-windows-msvc.zip" -OutFile branchbox.zip

# Download checksums
Invoke-WebRequest -Uri "https://github.com/branchbox/branchbox/releases/download/vVERSION/checksums.txt" -OutFile checksums.txt

# Verify checksum
$hash = (Get-FileHash branchbox.zip -Algorithm SHA256).Hash.ToLower()
$expectedHash = (Get-Content checksums.txt | Select-String "branchbox-VERSION-x86_64-pc-windows-msvc.zip" | ForEach-Object { $_.Line.Split(' ')[0] })
if ($hash -eq $expectedHash) {
    Write-Host "✓ Checksum verification passed" -ForegroundColor Green
} else {
    Write-Host "✗ Checksum verification failed!" -ForegroundColor Red
    Write-Host "  Expected: $expectedHash"
    Write-Host "  Got:      $hash"
    exit 1
}

# Extract
Expand-Archive branchbox.zip

# Add to PATH or move to a directory in PATH
Move-Item branchbox\branchbox-VERSION-x86_64-pc-windows-msvc\branchbox.exe C:\Users\$env:USERNAME\bin\

# Verify installation
branchbox --version
```

## Build from Source

```bash
# Clone repository
git clone https://github.com/branchbox/branchbox.git
cd branchbox

# Build and install
cargo install --path cli --locked

# Verify installation
branchbox --version
```

## Mac App (Preview)

BranchBox for Mac is a native app for your features: it lists feature worktrees across your projects, starts features, reviews teardown choices, and opens your editor, terminal or coding agent. It drives the `branchbox` CLI you installed above, so install the CLI first.

**Requirements:** macOS 26 or later and `branchbox` 0.13.4 or later. The released 0.13.4 CLI uses legacy compatibility fallbacks; a development build can report the same version with newer capabilities. Project configuration and tunnel credential editing require CLI capabilities; host-tool diagnostics remain available in legacy mode.

The current app packaging uses ad hoc signing rather than Developer ID notarization. Obtain a preview build or build from source:

1. Download the `BranchBox-macOS-<sha>` artifact from a successful run of the [**macOS App** workflow](https://github.com/branchbox/branchbox/actions/workflows/macos-app.yml) on GitHub Actions (retained for 14 days), or build it yourself with `scripts/package-macos-app.sh --native --zip` using Xcode 26. GitHub may require you to sign in to download workflow artifacts.
2. Unzip it and move `BranchBox.app` to `/Applications`.
3. Open it once, then use **System Settings › Privacy & Security › Open Anyway** if Gatekeeper blocks this preview build. For a build you trust, you can alternatively clear the downloaded bundle's quarantine flag:
   ```bash
   xattr -dr com.apple.quarantine /Applications/BranchBox.app
   ```

The app finds the CLI on your login shell's `PATH` or standard install locations. Use **Settings › Tools › Locate…** to pick another one, and **Window › Diagnostics** to see which CLI it uses and whether Git and Docker are ready. Continue with the [Mac app user guide](../guides/mac-app.md), or see [`macos/README.md`](https://github.com/branchbox/branchbox/blob/main/macos/README.md) for development and packaging details.

## Verify Installation

After installation, verify that BranchBox is working:

```bash
# Check version
branchbox --version

# Display help
branchbox --help

# List available commands
branchbox feature --help
```

## Troubleshooting

### Linux Install Script

**Q: The script says my architecture is unsupported**

A: BranchBox currently supports x86_64 and aarch64 (ARM64) on Linux. For other architectures, try building from source.

**Q: Installation fails with permission denied**

A: Try installing to a user directory:
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | INSTALL_DIR=$HOME/.local/bin bash
```

Then add `~/.local/bin` to your PATH:
```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

**Q: Checksum verification fails**

A: This could indicate a corrupted download or a network issue. Try running the installer again. If the problem persists, download the binary manually from the [GitHub Releases](https://github.com/branchbox/branchbox/releases/latest) page.

**Q: I don't want to pipe curl to sh**

A: You can download and inspect the script first:
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh -o install.sh
less install.sh  # Inspect the script
chmod +x install.sh
./install.sh
```

**Q: The script cannot find the release**

A: Make sure you have an active internet connection. If a specific version doesn't exist, you'll see an error. Check available versions at [GitHub Releases](https://github.com/branchbox/branchbox/releases).

### Windows

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

## Next Steps

Once installed, check out the [GitHub README](https://github.com/branchbox/branchbox) for usage examples and getting started guides.
