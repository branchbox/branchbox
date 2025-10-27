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
      echo -e "${RED}Error: Unsupported architecture: $arch${NC}" >&2
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
      echo -e "${RED}Error: Unsupported OS: $os${NC}" >&2
      echo -e "${YELLOW}Use Homebrew for macOS: brew install branchbox/tap/branchbox${NC}" >&2
      exit 1
      ;;
  esac
}

# Get latest version
get_latest_version() {
  local version
  version=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep -oP '"tag_name": "\K[^"]+')

  if [ -z "$version" ]; then
    echo -e "${RED}Error: Could not fetch latest version${NC}" >&2
    exit 1
  fi

  echo "$version"
}

# Main installation
main() {
  echo -e "${GREEN}BranchBox Installer${NC}"
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
  trap 'rm -rf "$tmp_dir"' EXIT

  # Download archive
  if ! curl -fsSL "$download_url" -o "$tmp_dir/$archive_name"; then
    echo -e "${RED}Error: Failed to download $archive_name${NC}" >&2
    exit 1
  fi

  # Download and verify checksum
  echo "Verifying checksum..."
  if ! curl -fsSL "$checksum_url" -o "$tmp_dir/checksums.txt"; then
    echo -e "${YELLOW}Warning: Could not download checksums, skipping verification${NC}" >&2
  else
    cd "$tmp_dir"
    if ! sha256sum -c checksums.txt --ignore-missing 2>/dev/null; then
      echo -e "${RED}Error: Checksum verification failed${NC}" >&2
      exit 1
    fi
    echo -e "${GREEN}Checksum verified${NC}"
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
      echo -e "${YELLOW}Warning: $INSTALL_DIR is not in your PATH${NC}"
      echo "Add this to your ~/.bashrc or ~/.zshrc:"
      echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
  fi

  echo ""
  echo -e "${GREEN}✓ BranchBox installed successfully!${NC}"
  echo ""
  echo "Verify installation:"
  echo "  $BINARY_NAME --version"
  echo ""
  echo "Get started:"
  echo "  $BINARY_NAME --help"
}

main
