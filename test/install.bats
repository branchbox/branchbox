#!/usr/bin/env bats
# Tests for install.sh - combines unit and integration tests

setup() {
  # Create isolated test directory
  TEST_TEMP_DIR="$(mktemp -d)"
  export TEST_TEMP_DIR
  export INSTALL_DIR="$TEST_TEMP_DIR/bin"
  mkdir -p "$INSTALL_DIR"
}

teardown() {
  # Cleanup
  rm -rf "$TEST_TEMP_DIR"
  unset BRANCHBOX_VERSION INSTALL_DIR
}

# === Unit Tests ===

@test "detect_arch: x86_64" {
  uname() { echo "x86_64"; }
  export -f uname
  source "$BATS_TEST_DIRNAME/../install.sh"
  run detect_arch
  [ "$status" -eq 0 ]
  [ "$output" = "x86_64" ]
}

@test "detect_arch: aarch64" {
  uname() { echo "aarch64"; }
  export -f uname
  source "$BATS_TEST_DIRNAME/../install.sh"
  run detect_arch
  [ "$status" -eq 0 ]
  [ "$output" = "aarch64" ]
}

@test "detect_arch: unsupported fails" {
  uname() { echo "riscv64"; }
  export -f uname
  source "$BATS_TEST_DIRNAME/../install.sh"
  run detect_arch
  [ "$status" -eq 1 ]
  [[ "$output" =~ "Unsupported architecture" ]]
}

@test "detect_os: linux" {
  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "Linux"
  }
  export -f uname
  source "$BATS_TEST_DIRNAME/../install.sh"
  run detect_os
  [ "$status" -eq 0 ]
  [ "$output" = "linux" ]
}

@test "detect_os: macOS" {
  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "Darwin"
  }
  export -f uname
  source "$BATS_TEST_DIRNAME/../install.sh"
  run detect_os
  [ "$status" -eq 0 ]
  [ "$output" = "darwin" ]
}

@test "detect_os: unsupported fails with guidance" {
  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "FreeBSD"
  }
  export -f uname
  source "$BATS_TEST_DIRNAME/../install.sh"
  run detect_os
  [ "$status" -eq 1 ]
  [[ "$output" =~ "Unsupported OS" ]]
}

@test "get_latest_version: parses API response" {
  curl() { echo '{"tag_name": "v0.2.5"}'; }
  export -f curl
  source "$BATS_TEST_DIRNAME/../install.sh"
  run get_latest_version
  [ "$status" -eq 0 ]
  [ "$output" = "v0.2.5" ]
}

@test "get_latest_version: fails on empty response" {
  curl() { echo "{}"; }
  export -f curl
  source "$BATS_TEST_DIRNAME/../install.sh"
  run get_latest_version
  [ "$status" -eq 1 ]
}

@test "normalize_version: adds v prefix when missing" {
  source "$BATS_TEST_DIRNAME/../install.sh"
  run normalize_version "0.2.5"
  [ "$status" -eq 0 ]
  [ "$output" = "v0.2.5" ]
}

@test "normalize_version: keeps existing v prefix" {
  source "$BATS_TEST_DIRNAME/../install.sh"
  run normalize_version "v0.2.5"
  [ "$status" -eq 0 ]
  [ "$output" = "v0.2.5" ]
}

@test "script has error handling" {
  run grep "^set -e" "$BATS_TEST_DIRNAME/../install.sh"
  [ "$status" -eq 0 ]
}

@test "script has cleanup trap" {
  run grep -q "trap.*EXIT" "$BATS_TEST_DIRNAME/../install.sh"
  [ "$status" -eq 0 ]
}

# === Integration Tests ===

@test "integration: full install with mocked download" {
  command -v tar >/dev/null || skip "tar not available"
  command -v sha256sum >/dev/null || skip "sha256sum not available"

  # Create mock binary
  export MOCK_DIR="$TEST_TEMP_DIR/mock"
  mkdir -p "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu"
  echo '#!/bin/bash' > "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu/branchbox"
  echo 'echo "branchbox v0.1.0"' >> "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu/branchbox"
  chmod +x "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu/branchbox"

  cd "$MOCK_DIR"
  tar czf branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz branchbox-0.1.0-x86_64-unknown-linux-gnu
  sha256sum branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz > checksums.txt

  # Mock curl
  curl() {
    # Parse curl arguments properly
    local url output
    while [[ $# -gt 0 ]]; do
      case $1 in
        -o)
          output="$2"
          shift 2
          ;;
        -*)
          shift
          ;;
        *)
          url="$1"
          shift
          ;;
      esac
    done

    if [[ "$url" == *"releases/latest"* ]]; then
      echo '{"tag_name": "v0.1.0"}'
    elif [[ "$url" == *".tar.gz"* ]]; then
      cp "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz" "$output"
    elif [[ "$url" == *"checksums.txt"* ]]; then
      cp "$MOCK_DIR/checksums.txt" "$output"
    fi
  }
  export -f curl

  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "Linux"
  }
  export -f uname

  run bash "$BATS_TEST_DIRNAME/../install.sh"
  [ "$status" -eq 0 ]
  [[ "$output" =~ "installed successfully" ]]
  [ -x "$INSTALL_DIR/branchbox" ]
}

@test "integration: custom version" {
  command -v tar >/dev/null || skip "tar not available"
  export BRANCHBOX_VERSION="v0.2.0"

  # Similar mock setup...
  export MOCK_DIR="$TEST_TEMP_DIR/mock"
  mkdir -p "$MOCK_DIR/branchbox-0.2.0-x86_64-unknown-linux-gnu"
  echo '#!/bin/bash' > "$MOCK_DIR/branchbox-0.2.0-x86_64-unknown-linux-gnu/branchbox"
  chmod +x "$MOCK_DIR/branchbox-0.2.0-x86_64-unknown-linux-gnu/branchbox"
  cd "$MOCK_DIR"
  tar czf branchbox-0.2.0-x86_64-unknown-linux-gnu.tar.gz branchbox-0.2.0-x86_64-unknown-linux-gnu
  sha256sum branchbox-0.2.0-x86_64-unknown-linux-gnu.tar.gz > checksums.txt

  curl() {
    local url output
    while [[ $# -gt 0 ]]; do
      case $1 in
        -o) output="$2"; shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
      esac
    done
    if [[ "$url" == *".tar.gz"* ]]; then
      cp "$MOCK_DIR/branchbox-0.2.0-x86_64-unknown-linux-gnu.tar.gz" "$output"
    elif [[ "$url" == *"checksums.txt"* ]]; then
      cp "$MOCK_DIR/checksums.txt" "$output"
    fi
  }
  export -f curl

  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "Linux"
  }
  export -f uname

  run bash "$BATS_TEST_DIRNAME/../install.sh"
  [ "$status" -eq 0 ]
  [[ "$output" =~ "v0.2.0" ]]
}

@test "integration: checksum verification failure" {
  command -v tar >/dev/null || skip "tar not available"

  export MOCK_DIR="$TEST_TEMP_DIR/mock"
  mkdir -p "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu"
  echo '#!/bin/bash' > "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu/branchbox"
  chmod +x "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu/branchbox"
  cd "$MOCK_DIR"
  tar czf branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz branchbox-0.1.0-x86_64-unknown-linux-gnu

  curl() {
    local url output
    while [[ $# -gt 0 ]]; do
      case $1 in
        -o) output="$2"; shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
      esac
    done
    if [[ "$url" == *"releases/latest"* ]]; then
      echo '{"tag_name": "v0.1.0"}'
    elif [[ "$url" == *".tar.gz"* ]]; then
      cp "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz" "$output"
    elif [[ "$url" == *"checksums.txt"* ]]; then
      echo "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz" > "$output"
    fi
  }
  export -f curl

  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "Linux"
  }
  export -f uname

  run bash "$BATS_TEST_DIRNAME/../install.sh"
  [ "$status" -eq 1 ]
  [[ "$output" =~ "Checksum verification failed" ]]
}

@test "integration: checksum file missing archive entry fails" {
  command -v tar >/dev/null || skip "tar not available"

  export MOCK_DIR="$TEST_TEMP_DIR/mock"
  mkdir -p "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu"
  echo '#!/bin/bash' > "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu/branchbox"
  chmod +x "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu/branchbox"
  cd "$MOCK_DIR"
  tar czf branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz branchbox-0.1.0-x86_64-unknown-linux-gnu

  curl() {
    local url output
    while [[ $# -gt 0 ]]; do
      case $1 in
        -o) output="$2"; shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
      esac
    done
    if [[ "$url" == *"releases/latest"* ]]; then
      echo '{"tag_name": "v0.1.0"}'
    elif [[ "$url" == *".tar.gz"* ]]; then
      cp "$MOCK_DIR/branchbox-0.1.0-x86_64-unknown-linux-gnu.tar.gz" "$output"
    elif [[ "$url" == *"checksums.txt"* ]]; then
      echo "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  some-other-file.tar.gz" > "$output"
    fi
  }
  export -f curl

  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "Linux"
  }
  export -f uname

  run bash "$BATS_TEST_DIRNAME/../install.sh"
  [ "$status" -eq 1 ]
  [[ "$output" =~ "Could not find checksum entry" ]]
}

@test "integration: download failure" {
  curl() {
    [[ "$*" == *"releases/latest"* ]] && echo '{"tag_name": "v0.1.0"}' || return 1
  }
  export -f curl

  uname() {
    [ "$1" = "-m" ] && echo "x86_64" || echo "Linux"
  }
  export -f uname

  run bash "$BATS_TEST_DIRNAME/../install.sh"
  [ "$status" -eq 1 ]
  [[ "$output" =~ "Failed to download" ]]
}

@test "integration: piped execution installs successfully" {
  command -v tar >/dev/null || skip "tar not available"
  command -v sha256sum >/dev/null || skip "sha256sum not available"

  export MOCK_DIR="$TEST_TEMP_DIR/mock"
  mkdir -p "$MOCK_DIR/branchbox-0.3.0-x86_64-unknown-linux-gnu"
  echo '#!/bin/bash' > "$MOCK_DIR/branchbox-0.3.0-x86_64-unknown-linux-gnu/branchbox"
  echo 'echo "branchbox v0.3.0"' >> "$MOCK_DIR/branchbox-0.3.0-x86_64-unknown-linux-gnu/branchbox"
  chmod +x "$MOCK_DIR/branchbox-0.3.0-x86_64-unknown-linux-gnu/branchbox"
  cd "$MOCK_DIR"
  tar czf branchbox-0.3.0-x86_64-unknown-linux-gnu.tar.gz branchbox-0.3.0-x86_64-unknown-linux-gnu
  sha256sum branchbox-0.3.0-x86_64-unknown-linux-gnu.tar.gz > checksums.txt

  run bash -c '
    set -e
    INSTALL_DIR="$1"
    MOCK_DIR="$2"
    INSTALL_SCRIPT="$3"

    curl() {
      local url output
      while [[ $# -gt 0 ]]; do
        case $1 in
          -o)
            output="$2"
            shift 2
            ;;
          -*)
            shift
            ;;
          *)
            url="$1"
            shift
            ;;
        esac
      done

      if [[ "$url" == *"releases/latest"* ]]; then
        echo "{\"tag_name\": \"v0.3.0\"}"
      elif [[ "$url" == *".tar.gz"* ]]; then
        cp "$MOCK_DIR/branchbox-0.3.0-x86_64-unknown-linux-gnu.tar.gz" "$output"
      elif [[ "$url" == *"checksums.txt"* ]]; then
        cp "$MOCK_DIR/checksums.txt" "$output"
      fi
    }

    uname() {
      [ "$1" = "-m" ] && echo "x86_64" || echo "Linux"
    }

    export -f curl uname
    cat "$INSTALL_SCRIPT" | INSTALL_DIR="$INSTALL_DIR" bash
  ' _ "$INSTALL_DIR" "$MOCK_DIR" "$BATS_TEST_DIRNAME/../install.sh"

  [ "$status" -eq 0 ]
  [[ "$output" =~ "installed successfully" ]]
  [ -x "$INSTALL_DIR/branchbox" ]
}
