#!/usr/bin/env bash
set -euo pipefail

# Regenerate macos/Packaging/AppIcon.iconset from the logo exports in assets/icons. The PNGs it writes are
# committed; packaging only runs `iconutil` over them, so this runs once, when the logo changes.
#
# Sources: assets/icons/png/logo-darkmode-<N>x<N>.png for every size that exists (16 to 512), each slot scaled
# from the closest export at or above its size. The 1024 px slot (icon_512x512@2x) is rendered from
# assets/icons/logo-darkmode.svg with rsvg-convert when it is installed (`brew install librsvg`), else
# upscaled from the 512 px export with sips. The square logo is used for every slot so the @1x and @2x
# images match; the circle variants carry a dark disc and are meant for avatars.

usage() {
  cat <<'EOF'
Usage: macos/Packaging/make-iconset.sh [--out DIR] [--check]

Writes AppIcon.iconset (16, 32, 128, 256 and 512 pt at @1x and @2x) next to this script.
  --out DIR   write the iconset to DIR instead
  --check     also build an .icns with iconutil into a temp dir to prove the set is complete
  -h, --help  show this help
EOF
}

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PNG_DIR="${ROOT_DIR}/assets/icons/png"
SVG="${ROOT_DIR}/assets/icons/logo-darkmode.svg"
OUT="${SCRIPT_DIR}/AppIcon.iconset"
check=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --out) [[ $# -ge 2 ]] || { echo "--out needs a directory" >&2; exit 2; }; OUT="$2"; shift ;;
    --check) check=1 ;;
    *) echo "Unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

command -v sips >/dev/null 2>&1 || { echo "sips not found; run this on macOS" >&2; exit 1; }

available=()
for size in 16 32 48 64 128 256 512; do
  [[ -f "${PNG_DIR}/logo-darkmode-${size}x${size}.png" ]] && available+=("$size")
done
[[ ${#available[@]} -gt 0 ]] || { echo "No logo-darkmode-<N>x<N>.png exports in ${PNG_DIR}" >&2; exit 1; }
largest="${available[${#available[@]}-1]}"

# The export to scale from for a slot of `px` pixels: the smallest one at least that large, else the largest.
source_for() {
  local px="$1" size
  for size in "${available[@]}"; do
    if (( size >= px )); then echo "${PNG_DIR}/logo-darkmode-${size}x${size}.png"; return; fi
  done
  echo "${PNG_DIR}/logo-darkmode-${largest}x${largest}.png"
}

write_slot() {
  local name="$1" px="$2" dest="${OUT}/$1"
  if (( px > largest )) && command -v rsvg-convert >/dev/null 2>&1 && [[ -f "$SVG" ]]; then
    rsvg-convert --width "$px" --height "$px" --keep-aspect-ratio --format png --output "$dest" "$SVG"
    echo "  ${name} (${px} px) from $(basename "$SVG") via rsvg-convert"
  else
    local src
    src="$(source_for "$px")"
    sips --resampleHeightWidth "$px" "$px" "$src" --out "$dest" >/dev/null
    echo "  ${name} (${px} px) from $(basename "$src")"
  fi
}

rm -rf "$OUT"
mkdir -p "$OUT"
echo "Writing ${OUT}"
for pt in 16 32 128 256 512; do
  write_slot "icon_${pt}x${pt}.png" "$pt"
  write_slot "icon_${pt}x${pt}@2x.png" "$((pt * 2))"
done

if [[ "$check" == 1 ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  iconutil --convert icns --output "${tmp}/AppIcon.icns" "$OUT"
  echo "iconutil OK: $(stat -f %z "${tmp}/AppIcon.icns") bytes"
fi
