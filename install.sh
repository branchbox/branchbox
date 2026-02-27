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
INSTALL_DIR="${INSTALL_DIR:-}"
TMP_DIR=""

cleanup() {
  if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
    rm -rf "$TMP_DIR"
  fi
}

has_command() {
  command -v "$1" >/dev/null 2>&1
}

print_error() {
  echo -e "${RED}Error: $1${NC}" >&2
}

require_tools() {
  if ! has_command tar; then
    print_error "Required command not found: tar"
    exit 1
  fi

  if ! has_command mktemp; then
    print_error "Required command not found: mktemp"
    exit 1
  fi

  if ! has_command curl && ! has_command wget; then
    print_error "Required command not found: curl or wget"
    exit 1
  fi

  if ! has_command sha256sum && ! has_command shasum && ! has_command openssl; then
    print_error "Required command not found: sha256sum, shasum, or openssl"
    exit 1
  fi
}

download_to_file() {
  local url="$1"
  local output="$2"

  if has_command curl; then
    curl -fsSL "$url" -o "$output"
  elif has_command wget; then
    wget -qO "$output" "$url"
  else
    return 1
  fi
}

download_to_stdout() {
  local url="$1"

  if has_command curl; then
    curl -fsSL "$url"
  elif has_command wget; then
    wget -qO- "$url"
  else
    return 1
  fi
}

calculate_sha256() {
  local file="$1"

  if has_command sha256sum; then
    sha256sum "$file" | awk '{print $1}'
  elif has_command shasum; then
    shasum -a 256 "$file" | awk '{print $1}'
  elif has_command openssl; then
    openssl dgst -sha256 "$file" | awk '{print $2}'
  else
    return 1
  fi
}

default_install_dir() {
  local os="$1"

  if [ "$os" = "darwin" ] && [ -d "/opt/homebrew/bin" ]; then
    echo "/opt/homebrew/bin"
  else
    echo "/usr/local/bin"
  fi
}

normalize_version() {
  local version="$1"

  if [ -z "$version" ]; then
    echo ""
  elif [[ "$version" == v* ]]; then
    echo "$version"
  else
    echo "v$version"
  fi
}

# Detect architecture
detect_arch() {
  local arch
  arch=$(uname -m)

  case "$arch" in
    x86_64|amd64)
      echo "x86_64"
      ;;
    aarch64|arm64|arm64e)
      echo "aarch64"
      ;;
    *)
      print_error "Unsupported architecture: $arch"
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
    darwin)
      echo "darwin"
      ;;
    *)
      print_error "Unsupported OS: $os"
      echo -e "${YELLOW}Use Homebrew on macOS or Scoop on Windows.${NC}" >&2
      exit 1
      ;;
  esac
}

build_target() {
  local arch="$1"
  local os="$2"

  case "$os" in
    linux)
      echo "${arch}-unknown-linux-gnu"
      ;;
    darwin)
      echo "${arch}-apple-darwin"
      ;;
    *)
      return 1
      ;;
  esac
}

# Get latest version
get_latest_version() {
  local version
  version=$(
    download_to_stdout "https://api.github.com/repos/$REPO/releases/latest" \
      | sed -n -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' \
      | head -n1
  )

  if [ -z "$version" ]; then
    print_error "Could not fetch latest version"
    exit 1
  fi

  echo "$version"
}

install_binary() {
  local source_binary="$1"
  local target_dir="$2"

  if [ ! -f "$source_binary" ]; then
    print_error "Extracted binary not found: $source_binary"
    exit 1
  fi

  if mkdir -p "$target_dir" 2>/dev/null && [ -w "$target_dir" ]; then
    echo "Installing to $target_dir (no sudo required)"
    mv "$source_binary" "$target_dir/$BINARY_NAME"
    chmod +x "$target_dir/$BINARY_NAME"
    return
  fi

  if has_command sudo; then
    echo "Installing to $target_dir (requires sudo)"
    sudo mkdir -p "$target_dir"
    sudo mv "$source_binary" "$target_dir/$BINARY_NAME"
    sudo chmod +x "$target_dir/$BINARY_NAME"
    return
  fi

  target_dir="$HOME/.local/bin"
  mkdir -p "$target_dir"
  echo "Installing to $target_dir (user installation)"
  mv "$source_binary" "$target_dir/$BINARY_NAME"
  chmod +x "$target_dir/$BINARY_NAME"

  case ":$PATH:" in
    *":$target_dir:"*)
      ;;
    *)
      echo ""
      echo -e "${YELLOW}Warning: $target_dir is not in your PATH${NC}"
      echo "Add this to your ~/.bashrc or ~/.zshrc:"
      echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
      ;;
  esac
}

# Main installation
main() {
  echo -e "${GREEN}BranchBox Installer${NC}"
  echo ""

  require_tools

  # Detect system
  local arch os target version version_number archive_name download_url checksum_url
  arch=$(detect_arch)
  os=$(detect_os)
  target=$(build_target "$arch" "$os")

  echo "Detected: $os-$arch"

  # Get version
  if [ -n "${BRANCHBOX_VERSION:-}" ]; then
    version=$(normalize_version "$BRANCHBOX_VERSION")
    echo "Installing version: $version (from BRANCHBOX_VERSION)"
  else
    version=$(get_latest_version)
    echo "Installing latest version: $version"
  fi

  # Build download URL
  version_number="${version#v}" # Remove 'v' prefix
  archive_name="branchbox-${version_number}-${target}.tar.gz"
  download_url="https://github.com/$REPO/releases/download/$version/$archive_name"
  checksum_url="https://github.com/$REPO/releases/download/$version/checksums.txt"

  echo "Downloading from: $download_url"

  # Create temp directory
  TMP_DIR=$(mktemp -d)

  # Download archive
  if ! download_to_file "$download_url" "$TMP_DIR/$archive_name"; then
    print_error "Failed to download $archive_name"
    exit 1
  fi

  # Download and verify checksum
  echo "Verifying checksum..."
  if ! download_to_file "$checksum_url" "$TMP_DIR/checksums.txt"; then
    print_error "Failed to download checksum file. Aborting installation."
    exit 1
  fi

  local expected_hash actual_hash
  expected_hash=$(
    awk -v archive_name="$archive_name" '
      {
        candidate=$2
        sub(/^\*/, "", candidate)
        if (candidate == archive_name) {
          print $1
          found=1
          exit
        }
      }
      END {
        if (!found) {
          exit 1
        }
      }
    ' "$TMP_DIR/checksums.txt"
  ) || {
    print_error "Could not find checksum entry for $archive_name"
    exit 1
  }

  if ! [[ "$expected_hash" =~ ^[[:xdigit:]]{64}$ ]]; then
    print_error "Invalid checksum entry for $archive_name"
    exit 1
  fi

  actual_hash=$(calculate_sha256 "$TMP_DIR/$archive_name" | tr '[:upper:]' '[:lower:]')
  expected_hash=$(echo "$expected_hash" | tr '[:upper:]' '[:lower:]')

  if [ "$actual_hash" != "$expected_hash" ]; then
    print_error "Checksum verification failed for $archive_name"
    exit 1
  fi
  echo -e "${GREEN}Checksum verified${NC}"

  # Extract archive
  echo "Extracting..."
  tar xzf "$TMP_DIR/$archive_name" -C "$TMP_DIR"

  # Determine install location
  local install_dir
  install_dir="$INSTALL_DIR"
  if [ -z "$install_dir" ]; then
    install_dir=$(default_install_dir "$os")
  fi

  install_binary "$TMP_DIR/branchbox-${version_number}-${target}/$BINARY_NAME" "$install_dir"

  echo ""
  echo -e "${GREEN}✓ BranchBox installed successfully!${NC}"
  echo ""
  echo "Verify installation:"
  echo "  $BINARY_NAME --version"
  echo ""
  echo "Get started:"
  echo "  $BINARY_NAME --help"
}

# Run main only when script is executed, not sourced.
if (return 0 2>/dev/null); then
  :
else
  trap cleanup EXIT
  main "$@"
fi
