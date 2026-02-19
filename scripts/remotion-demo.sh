#!/usr/bin/env bash

set -euo pipefail

STACK="rust"
MODE="render"
OUTPUT=""
INSTALL_LINUX_DEPS=0
CHROME_MODE="${REMOTION_CHROME_MODE:-headless-shell}"
ALL_STACKS=0
FORMAT="landscape"
EXTRA_ARGS=()

usage() {
  cat <<'EOF'
Usage: scripts/remotion-demo.sh [options] [-- <extra remotion args>]

Options:
  --stack <rust|node|rails|generic>   Demo stack to visualize (default: rust)
  --format <landscape|square|vertical> Output aspect variant (default: landscape)
  --studio                            Launch Remotion Studio instead of rendering
  --render                            Render MP4 output (default)
  --output <path>                     Output file path for render mode
  --chrome-mode <mode>                headless-shell (default) or chrome-for-testing
  --all-stacks                        Render rust/node/rails/generic in one run
  --install-linux-deps                Install Linux browser deps (apt-based systems)
  -h, --help                          Show this help

Examples:
  scripts/remotion-demo.sh --stack rust
  scripts/remotion-demo.sh --studio --stack node
  scripts/remotion-demo.sh --all-stacks
  scripts/remotion-demo.sh --install-linux-deps --chrome-mode chrome-for-testing
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --stack)
      STACK="${2:-}"
      shift
      ;;
    --stack=*)
      STACK="${1#*=}"
      ;;
    --studio)
      MODE="studio"
      ;;
    --render)
      MODE="render"
      ;;
    --format)
      FORMAT="${2:-}"
      shift
      ;;
    --format=*)
      FORMAT="${1#*=}"
      ;;
    --output)
      OUTPUT="${2:-}"
      shift
      ;;
    --output=*)
      OUTPUT="${1#*=}"
      ;;
    --chrome-mode)
      CHROME_MODE="${2:-}"
      shift
      ;;
    --chrome-mode=*)
      CHROME_MODE="${1#*=}"
      ;;
    --install-linux-deps)
      INSTALL_LINUX_DEPS=1
      ;;
    --all-stacks)
      ALL_STACKS=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      while [[ $# -gt 0 ]]; do
        EXTRA_ARGS+=("$1")
        shift
      done
      break
      ;;
    *)
      EXTRA_ARGS+=("$1")
      ;;
  esac
  shift
done

case "$STACK" in
  rust|node|rails|generic) ;;
  *)
    echo "Unsupported stack: $STACK" >&2
    exit 1
    ;;
esac

case "$FORMAT" in
  landscape|square|vertical) ;;
  *)
    echo "Unsupported format: $FORMAT" >&2
    exit 1
    ;;
esac

case "$CHROME_MODE" in
  headless-shell|chrome-for-testing) ;;
  *)
    echo "Unsupported chrome mode: $CHROME_MODE" >&2
    exit 1
    ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEMO_DIR="$REPO_ROOT/demos/remotion"

if [[ ! -f "$DEMO_DIR/package.json" ]]; then
  echo "Missing Remotion demo package at $DEMO_DIR" >&2
  exit 1
fi

if [[ -n "$OUTPUT" && "$OUTPUT" != /* ]]; then
  OUTPUT="$REPO_ROOT/$OUTPUT"
fi

if [[ "$ALL_STACKS" == "1" && "$MODE" == "studio" ]]; then
  echo "--all-stacks is only supported with render mode." >&2
  exit 1
fi

install_linux_deps() {
  if [[ "$(uname -s)" != "Linux" ]]; then
    echo "Skipping Linux dependency install (current OS: $(uname -s))."
    return 0
  fi

  if ! command -v apt-get >/dev/null 2>&1; then
    echo "Could not find apt-get; install Remotion browser dependencies manually." >&2
    return 1
  fi

  local asound_pkg="libasound2"
  if ! apt-cache show "$asound_pkg" >/dev/null 2>&1 && apt-cache show "libasound2t64" >/dev/null 2>&1; then
    asound_pkg="libasound2t64"
  fi

  local packages=(
    libnss3
    libdbus-1-3
    libatk1.0-0
    "$asound_pkg"
    libxrandr2
    libxkbcommon0
    libxfixes3
    libxcomposite1
    libxdamage1
    libgbm1
    libcups2
    libcairo2
    libpango-1.0-0
    libatk-bridge2.0-0
  )

  if [[ "$EUID" -eq 0 ]]; then
    apt-get update
    apt-get install -y "${packages[@]}"
  else
    sudo apt-get update
    sudo apt-get install -y "${packages[@]}"
  fi
}

if [[ "$INSTALL_LINUX_DEPS" == "1" ]]; then
  install_linux_deps
fi

cd "$DEMO_DIR"

if [[ ! -d node_modules ]]; then
  if [[ -f package-lock.json ]]; then
    npm ci --include=dev
  else
    npm install --include=dev
  fi
fi

npx remotion browser ensure --chrome-mode="$CHROME_MODE"

if [[ "$MODE" == "studio" ]]; then
  PROPS_JSON="$(printf '{"stack":"%s"}' "$STACK")"
  if [[ "${#EXTRA_ARGS[@]}" -gt 0 ]]; then
    exec npx remotion studio src/index.ts --props="$PROPS_JSON" "${EXTRA_ARGS[@]}"
  fi
  exec npx remotion studio src/index.ts --props="$PROPS_JSON"
fi

render_stack() {
  local stack_name="$1"
  local output_path="$2"
  local props_json
  local composition_id

  props_json="$(printf '{"stack":"%s"}' "$stack_name")"
  composition_id="BranchBoxTeaser"
  if [[ "$FORMAT" == "square" ]]; then
    composition_id="BranchBoxTeaserSquare"
    props_json="$(printf '{"stack":"%s","format":"square"}' "$stack_name")"
  elif [[ "$FORMAT" == "vertical" ]]; then
    composition_id="BranchBoxTeaserVertical"
    props_json="$(printf '{"stack":"%s","format":"vertical"}' "$stack_name")"
  fi
  mkdir -p "$(dirname "$output_path")"

  if [[ "${#EXTRA_ARGS[@]}" -gt 0 ]]; then
    npx remotion render src/index.ts "$composition_id" "$output_path" \
      --props="$props_json" \
      --chrome-mode="$CHROME_MODE" \
      --codec=h264 \
      --overwrite \
      "${EXTRA_ARGS[@]}"
    return
  fi

  npx remotion render src/index.ts "$composition_id" "$output_path" \
    --props="$props_json" \
    --chrome-mode="$CHROME_MODE" \
    --codec=h264 \
    --overwrite
}

if [[ "$ALL_STACKS" == "1" ]]; then
  for stack_name in rust node rails generic; do
    output_path=""
    if [[ -z "$OUTPUT" ]]; then
      if [[ "$FORMAT" == "landscape" ]]; then
        output_path="$DEMO_DIR/out/branchbox-teaser-${stack_name}-final.mp4"
      else
        output_path="$DEMO_DIR/out/branchbox-teaser-${stack_name}-${FORMAT}.mp4"
      fi
    elif [[ "$OUTPUT" == *.mp4 ]]; then
      output_path="${OUTPUT%.mp4}-${stack_name}.mp4"
    else
      if [[ "$FORMAT" == "landscape" ]]; then
        output_path="${OUTPUT%/}/branchbox-teaser-${stack_name}-final.mp4"
      else
        output_path="${OUTPUT%/}/branchbox-teaser-${stack_name}-${FORMAT}.mp4"
      fi
    fi

    echo "=== Rendering ${stack_name} (${FORMAT}) -> ${output_path} ==="
    render_stack "$stack_name" "$output_path"
  done
  exit 0
fi

if [[ -z "$OUTPUT" ]]; then
  if [[ "$FORMAT" == "landscape" ]]; then
    OUTPUT="$DEMO_DIR/out/branchbox-teaser-${STACK}.mp4"
  else
    OUTPUT="$DEMO_DIR/out/branchbox-teaser-${STACK}-${FORMAT}.mp4"
  fi
fi

render_stack "$STACK" "$OUTPUT"
