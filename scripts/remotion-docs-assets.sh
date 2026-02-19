#!/usr/bin/env bash

set -euo pipefail

STACK="rust"
PART="all"
TARGET="docs"
OUT_DIR=""
SKIP_RENDER=0
INSTALL_LINUX_DEPS=0
CHROME_MODE="${REMOTION_CHROME_MODE:-headless-shell}"
GENERATE_SOCIAL=1

usage() {
  cat <<'EOF'
Usage: scripts/remotion-docs-assets.sh [options]

Renders documentation-ready demo cuts from the Remotion composition, copies them
to docs and/or website static directories, and writes a manifest.

Options:
  --stack <rust|node|rails|generic>    Stack to render (default: rust)
  --part <name>                        Part to render:
                                        full-reel
                                        getting-started
                                        minimal-mode
                                        parallel-features
                                        devcontainer-sync
                                        all (default)
  --target <docs|website|both>         Publish destination (default: docs)
  --out-dir <path>                     Render output directory (default: demos/remotion/out/docs-assets)
  --skip-render                        Publish existing files only (skips social/mobile rendering too)
  --install-linux-deps                 Install Linux browser dependencies before first render (apt-based systems)
  --chrome-mode <mode>                 headless-shell (default) or chrome-for-testing
  --no-social                          Skip social/mobile asset generation
  -h, --help                           Show this help

Examples:
  scripts/remotion-docs-assets.sh
  scripts/remotion-docs-assets.sh --part getting-started
  scripts/remotion-docs-assets.sh --stack node --part parallel-features
  scripts/remotion-docs-assets.sh --target both
  scripts/remotion-docs-assets.sh --target both --install-linux-deps
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
    --part)
      PART="${2:-}"
      shift
      ;;
    --part=*)
      PART="${1#*=}"
      ;;
    --target)
      TARGET="${2:-}"
      shift
      ;;
    --target=*)
      TARGET="${1#*=}"
      ;;
    --out-dir)
      OUT_DIR="${2:-}"
      shift
      ;;
    --out-dir=*)
      OUT_DIR="${1#*=}"
      ;;
    --skip-render)
      SKIP_RENDER=1
      ;;
    --install-linux-deps)
      INSTALL_LINUX_DEPS=1
      ;;
    --chrome-mode)
      CHROME_MODE="${2:-}"
      shift
      ;;
    --chrome-mode=*)
      CHROME_MODE="${1#*=}"
      ;;
    --no-social)
      GENERATE_SOCIAL=0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
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

case "$PART" in
  full-reel|getting-started|minimal-mode|parallel-features|devcontainer-sync|all) ;;
  *)
    echo "Unsupported part: $PART" >&2
    exit 1
    ;;
esac

case "$TARGET" in
  docs|website|both) ;;
  *)
    echo "Unsupported target: $TARGET" >&2
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
RENDER_SCRIPT="$REPO_ROOT/scripts/remotion-demo.sh"

if [[ ! -x "$RENDER_SCRIPT" ]]; then
  echo "Missing executable render script at $RENDER_SCRIPT" >&2
  exit 1
fi

if [[ -z "$OUT_DIR" ]]; then
  OUT_DIR="$REPO_ROOT/demos/remotion/out/docs-assets"
elif [[ "$OUT_DIR" != /* ]]; then
  OUT_DIR="$REPO_ROOT/$OUT_DIR"
fi

mkdir -p "$OUT_DIR"

SOCIAL_SCRIPT="$REPO_ROOT/scripts/remotion-social-assets.sh"
if [[ "$GENERATE_SOCIAL" == "1" && ! -x "$SOCIAL_SCRIPT" ]]; then
  echo "Missing executable social asset script at $SOCIAL_SCRIPT" >&2
  exit 1
fi

part_frames() {
  case "$1" in
    full-reel) echo "0-1529" ;;
    getting-started) echo "120-449" ;;
    minimal-mode) echo "450-779" ;;
    parallel-features) echo "780-1109" ;;
    devcontainer-sync) echo "1110-1439" ;;
    *)
      return 1
      ;;
  esac
}

part_filename() {
  case "$1" in
    full-reel) echo "branchbox-teaser-${STACK}-final.mp4" ;;
    getting-started) echo "branchbox-docs-getting-started-${STACK}.mp4" ;;
    minimal-mode) echo "branchbox-docs-minimal-mode-${STACK}.mp4" ;;
    parallel-features) echo "branchbox-docs-parallel-features-${STACK}.mp4" ;;
    devcontainer-sync) echo "branchbox-docs-devcontainer-sync-${STACK}.mp4" ;;
    *)
      return 1
      ;;
  esac
}

PARTS=()
if [[ "$PART" == "all" ]]; then
  PARTS=(full-reel getting-started minimal-mode parallel-features devcontainer-sync)
else
  PARTS=("$PART")
fi

render_part() {
  local part_name="$1"
  local frames
  local file_name
  local out_file
  local render_cmd=()

  frames="$(part_frames "$part_name")"
  file_name="$(part_filename "$part_name")"
  out_file="$OUT_DIR/$file_name"

  if [[ "$SKIP_RENDER" == "0" ]]; then
    echo "Rendering $part_name ($frames) -> $out_file"
    render_cmd=("$RENDER_SCRIPT" --stack "$STACK" --output "$out_file" --chrome-mode "$CHROME_MODE")
    if [[ "$INSTALL_LINUX_DEPS" == "1" ]]; then
      render_cmd+=(--install-linux-deps)
      INSTALL_LINUX_DEPS=0
    fi
    "${render_cmd[@]}" -- --frames="$frames"
  fi

  if [[ ! -f "$out_file" ]]; then
    echo "Expected rendered file not found: $out_file" >&2
    exit 1
  fi
}

for part_name in "${PARTS[@]}"; do
  render_part "$part_name"
done

if [[ "$GENERATE_SOCIAL" == "1" ]]; then
  if [[ "$SKIP_RENDER" == "1" ]]; then
    echo "Skipping social asset render because --skip-render was provided (equivalent to --no-social)."
  elif [[ "$PART" == "all" || "$PART" == "full-reel" ]]; then
    FULL_REEL_FILE="$OUT_DIR/$(part_filename full-reel)"
    if [[ -f "$FULL_REEL_FILE" ]]; then
      "$SOCIAL_SCRIPT" \
        --stack "$STACK" \
        --source "$FULL_REEL_FILE" \
        --out-dir "$OUT_DIR" \
        --chrome-mode "$CHROME_MODE" \
        --target "$TARGET"
    else
      echo "Skipping social assets; full reel not found at $FULL_REEL_FILE"
    fi
  fi
fi

copy_assets() {
  local destination_root="$1"
  local media_dir="$destination_root/media/demos"
  mkdir -p "$media_dir"

  for part_name in "${PARTS[@]}"; do
    local file_name
    file_name="$(part_filename "$part_name")"
    cp -f "$OUT_DIR/$file_name" "$media_dir/$file_name"
  done
}

sha256_file() {
  local file="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
    return
  fi
  sha256sum "$file" | awk '{print $1}'
}

manifest_for_target() {
  local destination_root="$1"
  local media_dir="$destination_root/media/demos"
  local manifest_name="manifest-${STACK}.json"
  local timestamp
  local idx=0

  if [[ "$PART" != "all" ]]; then
    manifest_name="manifest-${STACK}-${PART}.json"
  fi

  local manifest="$media_dir/$manifest_name"
  timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  mkdir -p "$media_dir"

  {
    echo "{"
    echo "  \"generated_at\": \"$timestamp\","
    echo "  \"stack\": \"$STACK\","
    echo "  \"assets\": ["

    for part_name in "${PARTS[@]}"; do
      local file_name source_file sha size duration comma
      file_name="$(part_filename "$part_name")"
      source_file="$media_dir/$file_name"
      sha="$(sha256_file "$source_file")"
      size="$(wc -c < "$source_file" | tr -d ' ')"
      duration="null"

      if command -v ffprobe >/dev/null 2>&1; then
        duration="$(ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$source_file" 2>/dev/null || true)"
        if [[ -z "$duration" ]]; then
          duration="null"
        fi
      fi

      comma=","
      if [[ "$idx" -eq "$((${#PARTS[@]} - 1))" ]]; then
        comma=""
      fi

      if [[ "$duration" == "null" ]]; then
        echo "    {\"part\":\"$part_name\",\"file\":\"/media/demos/$file_name\",\"sha256\":\"$sha\",\"size_bytes\":$size,\"duration_seconds\":null}$comma"
      else
        echo "    {\"part\":\"$part_name\",\"file\":\"/media/demos/$file_name\",\"sha256\":\"$sha\",\"size_bytes\":$size,\"duration_seconds\":$duration}$comma"
      fi

      idx=$((idx + 1))
    done

    echo "  ]"
    echo "}"
  } > "$manifest"
}

if [[ "$TARGET" == "docs" || "$TARGET" == "both" ]]; then
  copy_assets "$REPO_ROOT/docs/static"
  manifest_for_target "$REPO_ROOT/docs/static"
  echo "Published demo assets to $REPO_ROOT/docs/static/media/demos"
fi

if [[ "$TARGET" == "website" || "$TARGET" == "both" ]]; then
  copy_assets "$REPO_ROOT/website"
  manifest_for_target "$REPO_ROOT/website"
  echo "Published demo assets to $REPO_ROOT/website/media/demos"
fi

echo "Done."
