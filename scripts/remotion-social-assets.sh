#!/usr/bin/env bash

set -euo pipefail

STACK="rust"
TARGET="both"
SOURCE=""
OUT_DIR=""
CARD_AT_SECONDS="8"
CHROME_MODE="${REMOTION_CHROME_MODE:-headless-shell}"

usage() {
  cat <<'EOF'
Usage: scripts/remotion-social-assets.sh [options]

Generate share-ready variants from a rendered full-reel demo and publish them to
docs and/or website media directories. Square and vertical videos are rendered
as native Remotion compositions (not blurred crops).

Options:
  --stack <rust|node|rails|generic>    Stack to package (default: rust)
  --source <path>                      Full-reel source MP4 (default: demos/remotion/out/docs-assets/branchbox-teaser-<stack>-final.mp4)
  --out-dir <path>                     Variant output directory (default: source directory)
  --target <docs|website|both>         Publish destination (default: both)
  --card-at <seconds>                  Timestamp for poster/share card capture (default: 8)
  --chrome-mode <mode>                 headless-shell (default) or chrome-for-testing
  -h, --help                           Show this help

Outputs:
  branchbox-teaser-<stack>-web-16x9.mp4
  branchbox-teaser-<stack>-social-square.mp4
  branchbox-teaser-<stack>-social-vertical.mp4
  branchbox-teaser-<stack>-poster.jpg
  branchbox-teaser-<stack>-social-card.jpg
  manifest-<stack>-social.json
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
    --source)
      SOURCE="${2:-}"
      shift
      ;;
    --source=*)
      SOURCE="${1#*=}"
      ;;
    --out-dir)
      OUT_DIR="${2:-}"
      shift
      ;;
    --out-dir=*)
      OUT_DIR="${1#*=}"
      ;;
    --target)
      TARGET="${2:-}"
      shift
      ;;
    --target=*)
      TARGET="${1#*=}"
      ;;
    --card-at)
      CARD_AT_SECONDS="${2:-}"
      shift
      ;;
    --card-at=*)
      CARD_AT_SECONDS="${1#*=}"
      ;;
    --chrome-mode)
      CHROME_MODE="${2:-}"
      shift
      ;;
    --chrome-mode=*)
      CHROME_MODE="${1#*=}"
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

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "ffmpeg is required but not found in PATH" >&2
  exit 1
fi

if ! command -v ffprobe >/dev/null 2>&1; then
  echo "ffprobe is required but not found in PATH" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RENDER_SCRIPT="$REPO_ROOT/scripts/remotion-demo.sh"

if [[ ! -x "$RENDER_SCRIPT" ]]; then
  echo "Missing executable render script at $RENDER_SCRIPT" >&2
  exit 1
fi

if [[ -z "$SOURCE" ]]; then
  SOURCE="$REPO_ROOT/demos/remotion/out/docs-assets/branchbox-teaser-${STACK}-final.mp4"
elif [[ "$SOURCE" != /* ]]; then
  SOURCE="$REPO_ROOT/$SOURCE"
fi

if [[ ! -f "$SOURCE" ]]; then
  echo "Source full reel not found: $SOURCE" >&2
  exit 1
fi

if [[ -z "$OUT_DIR" ]]; then
  OUT_DIR="$(dirname "$SOURCE")"
elif [[ "$OUT_DIR" != /* ]]; then
  OUT_DIR="$REPO_ROOT/$OUT_DIR"
fi
mkdir -p "$OUT_DIR"

WEB_16X9="$OUT_DIR/branchbox-teaser-${STACK}-web-16x9.mp4"
SQUARE_1X1="$OUT_DIR/branchbox-teaser-${STACK}-social-square.mp4"
VERTICAL_9X16="$OUT_DIR/branchbox-teaser-${STACK}-social-vertical.mp4"
POSTER_16X9="$OUT_DIR/branchbox-teaser-${STACK}-poster.jpg"
CARD_1200X630="$OUT_DIR/branchbox-teaser-${STACK}-social-card.jpg"

echo "Packaging social variants from $SOURCE"

# Keep a named web export so website integrations do not depend on a generic filename.
cp -f "$SOURCE" "$WEB_16X9"

echo "Rendering native square variant..."
"$RENDER_SCRIPT" \
  --stack "$STACK" \
  --format square \
  --chrome-mode "$CHROME_MODE" \
  --output "$SQUARE_1X1"

echo "Rendering native vertical variant..."
"$RENDER_SCRIPT" \
  --stack "$STACK" \
  --format vertical \
  --chrome-mode "$CHROME_MODE" \
  --output "$VERTICAL_9X16"

ffmpeg -y -ss "$CARD_AT_SECONDS" -i "$SOURCE" \
  -frames:v 1 \
  -update 1 \
  -filter_complex "[0:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2:color=0x060915[v]" \
  -map "[v]" \
  -q:v 2 \
  "$POSTER_16X9"

ffmpeg -y -ss "$CARD_AT_SECONDS" -i "$SOURCE" \
  -frames:v 1 \
  -update 1 \
  -filter_complex "[0:v]scale=1200:630:force_original_aspect_ratio=increase,crop=1200:630[v]" \
  -map "[v]" \
  -q:v 2 \
  "$CARD_1200X630"

copy_assets() {
  local destination_root="$1"
  local media_dir="$destination_root/media/demos"
  mkdir -p "$media_dir"

  cp -f "$WEB_16X9" "$media_dir/$(basename "$WEB_16X9")"
  cp -f "$SQUARE_1X1" "$media_dir/$(basename "$SQUARE_1X1")"
  cp -f "$VERTICAL_9X16" "$media_dir/$(basename "$VERTICAL_9X16")"
  cp -f "$POSTER_16X9" "$media_dir/$(basename "$POSTER_16X9")"
  cp -f "$CARD_1200X630" "$media_dir/$(basename "$CARD_1200X630")"
}

duration_or_null() {
  local file="$1"
  local duration
  duration="$(ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$file" 2>/dev/null || true)"
  if [[ -z "$duration" ]]; then
    echo "null"
  else
    echo "$duration"
  fi
}

sha256_file() {
  local file="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
    return
  fi
  sha256sum "$file" | awk '{print $1}'
}

write_manifest() {
  local destination_root="$1"
  local media_dir="$destination_root/media/demos"
  local manifest="$media_dir/manifest-${STACK}-social.json"
  local timestamp

  timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  mkdir -p "$media_dir"

  local web_file square_file vertical_file poster_file card_file
  web_file="$media_dir/$(basename "$WEB_16X9")"
  square_file="$media_dir/$(basename "$SQUARE_1X1")"
  vertical_file="$media_dir/$(basename "$VERTICAL_9X16")"
  poster_file="$media_dir/$(basename "$POSTER_16X9")"
  card_file="$media_dir/$(basename "$CARD_1200X630")"

  {
    echo "{"
    echo "  \"generated_at\": \"$timestamp\","
    echo "  \"stack\": \"$STACK\","
    echo "  \"variants\": ["
    echo "    {\"variant\":\"web-16x9\",\"file\":\"/media/demos/$(basename "$WEB_16X9")\",\"sha256\":\"$(sha256_file "$web_file")\",\"size_bytes\":$(wc -c < "$web_file" | tr -d ' '),\"duration_seconds\":$(duration_or_null "$web_file")},"
    echo "    {\"variant\":\"social-square-1x1\",\"file\":\"/media/demos/$(basename "$SQUARE_1X1")\",\"sha256\":\"$(sha256_file "$square_file")\",\"size_bytes\":$(wc -c < "$square_file" | tr -d ' '),\"duration_seconds\":$(duration_or_null "$square_file")},"
    echo "    {\"variant\":\"social-vertical-9x16\",\"file\":\"/media/demos/$(basename "$VERTICAL_9X16")\",\"sha256\":\"$(sha256_file "$vertical_file")\",\"size_bytes\":$(wc -c < "$vertical_file" | tr -d ' '),\"duration_seconds\":$(duration_or_null "$vertical_file")},"
    echo "    {\"variant\":\"poster-16x9\",\"file\":\"/media/demos/$(basename "$POSTER_16X9")\",\"sha256\":\"$(sha256_file "$poster_file")\",\"size_bytes\":$(wc -c < "$poster_file" | tr -d ' '),\"duration_seconds\":null},"
    echo "    {\"variant\":\"social-card-1200x630\",\"file\":\"/media/demos/$(basename "$CARD_1200X630")\",\"sha256\":\"$(sha256_file "$card_file")\",\"size_bytes\":$(wc -c < "$card_file" | tr -d ' '),\"duration_seconds\":null}"
    echo "  ]"
    echo "}"
  } > "$manifest"
}

if [[ "$TARGET" == "docs" || "$TARGET" == "both" ]]; then
  copy_assets "$REPO_ROOT/docs/static"
  write_manifest "$REPO_ROOT/docs/static"
  echo "Published social assets to $REPO_ROOT/docs/static/media/demos"
fi

if [[ "$TARGET" == "website" || "$TARGET" == "both" ]]; then
  copy_assets "$REPO_ROOT/website"
  write_manifest "$REPO_ROOT/website"
  echo "Published social assets to $REPO_ROOT/website/media/demos"
fi

echo "Done."
