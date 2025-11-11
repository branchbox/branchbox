#!/usr/bin/env bash

set -euo pipefail

# BranchBox Teaser Demo
#
# Spins up a disposable sample repo, runs a compact BranchBox flow, and prints
# human-friendly steps you can record as a teaser. Avoids heavy Docker work by
# skipping compose/database modules while still exercising worktrees and
# devcontainer sync.
#
# Usage:
#   scripts/demo-teaser.sh [--stack rust|node|rails|generic] [--keep]
#                          [--bin /path/to/branchbox]
#
# Examples:
#   scripts/demo-teaser.sh --stack rust
#   BRANCHBOX_DEVCONTAINER_STRATEGY=copy scripts/demo-teaser.sh --stack node --keep

STACK="rust"
KEEP=0
BRANCHBOX_BIN=""

lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --stack)
      STACK="${2:-}"
      shift
      ;;
    --stack=*)
      STACK="${1#*=}"
      ;;
    --keep)
      KEEP=1
      ;;
    --bin)
      BRANCHBOX_BIN="${2:-}"
      shift
      ;;
    --bin=*)
      BRANCHBOX_BIN="${1#*=}"
      ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
  shift
done

case "$(lower "$STACK")" in
  rust|node|rails|generic) ;;
  *) echo "Unsupported stack: $STACK" >&2; exit 1 ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -z "$BRANCHBOX_BIN" ]]; then
  if command -v branchbox >/dev/null 2>&1; then
    BRANCHBOX_BIN="$(command -v branchbox)"
  elif [[ -x "$REPO_ROOT/target/debug/branchbox" ]]; then
    BRANCHBOX_BIN="$REPO_ROOT/target/debug/branchbox"
  else
    echo "Could not find branchbox in PATH or target/debug. Build or install it, or pass --bin." >&2
    exit 1
  fi
fi

TMPDIR_ROOT="${TMPDIR:-/tmp}"
DEMO_ROOT="$(mktemp -d "$TMPDIR_ROOT/branchbox-teaser-XXXXXX")"
SOURCE_DIR="$DEMO_ROOT/source"
WORK_DIR="$DEMO_ROOT/workspaces"
PROJECT_NAME="bbx-demo-$STACK"

cleanup() {
  if [[ "$KEEP" == "1" ]]; then
    echo "ℹ️  Keeping demo artifacts at $DEMO_ROOT"
  else
    rm -rf "$DEMO_ROOT"
  fi
}
trap cleanup EXIT

mkdir -p "$SOURCE_DIR/$PROJECT_NAME"
cd "$SOURCE_DIR/$PROJECT_NAME"

echo "==> Seeding $STACK sample repo"
git init -q

case "$STACK" in
  rust)
    mkdir -p src
    cat > Cargo.toml <<'EOF'
[package]
name = "bbx-demo"
version = "0.1.0"
edition = "2021"

[dependencies]
EOF
    cat > src/main.rs <<'EOF'
fn main() { println!("Hello from BranchBox demo"); }
EOF
    ;;
  node)
    cat > package.json <<'EOF'
{
  "name": "bbx-demo",
  "version": "1.0.0",
  "main": "index.js",
  "license": "MIT"
}
EOF
    echo 'console.log("Hello from BranchBox demo")' > index.js
    ;;
  rails)
    cat > Gemfile <<'EOF'
source "https://rubygems.org"
gem "rails", "~> 7.1.0"
EOF
    ;;
  generic)
    echo "# BranchBox Demo ($STACK)" > README.md
    ;;
esac

git add . >/dev/null
git commit -q -m "seed"

# Ensure a .devcontainer exists so sync is meaningful. Copy project templates.
if [[ -d "$REPO_ROOT/.devcontainer" ]]; then
  mkdir -p .devcontainer
  if command -v rsync >/dev/null 2>&1; then
    rsync -a --exclude '.env' "$REPO_ROOT/.devcontainer/" .devcontainer/
  else
    (cd "$REPO_ROOT/.devcontainer" && find . -type f ! -name '.env' -print0 | xargs -0 -I{} sh -c 'mkdir -p ".devcontainer/$(dirname "{}")"; cp -f "$REPO_ROOT/.devcontainer/{}" ".devcontainer/{}"')
  fi
fi

echo "==> branchbox init"
BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" init >/dev/null || true

echo "==> Start first feature (full, but skip heavy modules)"
BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature start \
  --title "Add OAuth Integration" \
  --skip-module compose \
  --skip-module database 

echo "==> Start second feature (minimal + default prompt)"
BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature new backlog-quick-fix --minimal --default-prompt >/dev/null

echo "==> List features"
BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature list

echo "==> Edit .devcontainer in main repo and sync"
if [[ -f .devcontainer/devcontainer.json ]]; then
  echo "// demo tweak" >> .devcontainer/devcontainer.json
fi
BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" devcontainer sync --dry-run

echo "==> Done. Demo root: $DEMO_ROOT"
