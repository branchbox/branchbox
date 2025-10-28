#!/bin/bash
# Test helper functions for bats tests

# Load bats support libraries
load_bats_support() {
  # Try multiple possible locations for bats libraries
  local bats_support_paths=(
    "/usr/lib/bats/bats-support/load.bash"
    "/usr/local/lib/bats/bats-support/load.bash"
    "$HOME/.bats/bats-support/load.bash"
    "test/helpers/bats-support/load.bash"
  )

  local bats_assert_paths=(
    "/usr/lib/bats/bats-assert/load.bash"
    "/usr/local/lib/bats/bats-assert/load.bash"
    "$HOME/.bats/bats-assert/load.bash"
    "test/helpers/bats-assert/load.bash"
  )

  for path in "${bats_support_paths[@]}"; do
    if [ -f "$path" ]; then
      load "$path"
      break
    fi
  done

  for path in "${bats_assert_paths[@]}"; do
    if [ -f "$path" ]; then
      load "$path"
      break
    fi
  done
}

# Source the install script functions without running main()
# We'll override main() to prevent automatic execution
source_install_script() {
  # Source the script but prevent main() from running
  # shellcheck disable=SC2154  # BATS_TEST_DIRNAME is provided by bats
  source "$BATS_TEST_DIRNAME/../../install.sh" &>/dev/null || true
}

# Mock curl for testing
mock_curl() {
  local response_file="$1"

  curl() {
    if [[ "$*" == *"releases/latest"* ]]; then
      cat "$response_file"
      return 0
    elif [[ "$*" == *"checksums.txt"* ]]; then
      echo "dummy-checksum  branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      return 0
    else
      # Simulate download failure by default for safety
      return 1
    fi
  }
  export -f curl
}

# Mock GitHub API response
create_mock_release_response() {
  local version="$1"
  local tmpfile
  tmpfile=$(mktemp)

  cat > "$tmpfile" <<EOF
{
  "tag_name": "$version",
  "name": "Release $version",
  "assets": [
    {
      "name": "branchbox-${version#v}-x86_64-unknown-linux-gnu.tar.gz",
      "browser_download_url": "https://github.com/branchbox/branchbox/releases/download/$version/branchbox-${version#v}-x86_64-unknown-linux-gnu.tar.gz"
    }
  ]
}
EOF

  echo "$tmpfile"
}

# Create a fake binary archive for testing
create_mock_binary_archive() {
  local version="$1"
  local arch="$2"
  local tmpdir
  tmpdir=$(mktemp -d)

  local version_number="${version#v}"
  local target="${arch}-unknown-linux-gnu"
  local archive_name="branchbox-${version_number}-${target}"

  mkdir -p "$tmpdir/$archive_name"
  echo '#!/bin/bash' > "$tmpdir/$archive_name/branchbox"
  echo 'echo "branchbox '$version'"' >> "$tmpdir/$archive_name/branchbox"
  chmod +x "$tmpdir/$archive_name/branchbox"

  tar czf "$tmpdir/$archive_name.tar.gz" -C "$tmpdir" "$archive_name"
  echo "$tmpdir/$archive_name.tar.gz"
}

# Setup function run before each test
test_setup() {
  # Create temp directory for test
  TEST_TEMP_DIR=$(mktemp -d)
  export TEST_TEMP_DIR

  # Save original functions
  ORIGINAL_CURL=$(command -v curl)
  export ORIGINAL_CURL
  export ORIGINAL_INSTALL_DIR="$INSTALL_DIR"
}

# Teardown function run after each test
test_teardown() {
  # Cleanup temp directory
  if [ -n "$TEST_TEMP_DIR" ] && [ -d "$TEST_TEMP_DIR" ]; then
    rm -rf "$TEST_TEMP_DIR"
  fi

  # Restore environment
  unset BRANCHBOX_VERSION
  unset INSTALL_DIR
  export INSTALL_DIR="$ORIGINAL_INSTALL_DIR"
}

# Check if running in CI
is_ci() {
  [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]
}

# Skip test if not in CI (for integration tests that need network)
skip_if_not_ci() {
  if ! is_ci; then
    skip "Integration test - run in CI only"
  fi
}
