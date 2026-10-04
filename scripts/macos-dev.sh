#!/usr/bin/env bash
set -euo pipefail

# Build the BranchBox Mac app (debug) and wrap it as macos/build/dev/BranchBox Dev.app, a real but
# development-only bundle (id dev.branchbox.app.dev, ad-hoc signed, hardened runtime). Unlike
# `swift run BranchBox`, the bundle gets notifications, and with --open the Finder/launchd
# environment (no terminal PATH), which is how the released app finds the CLI. It keeps its own
# settings suite, projects and logs ("BranchBox Dev"), so it never touches an installed app's state.

usage() {
  cat <<'EOF'
Usage: scripts/macos-dev.sh [options]

Builds the debug app and wraps it as macos/build/dev/BranchBox Dev.app, then:
  (default)       runs the app's binary in the foreground; logs stream to this terminal
  --open          launches the bundle through LaunchServices (`open`), like Finder does
  --build-only    builds the bundle, prints its path and exits without launching

Options:
  --background    with --open: launch without activating the app (`open -g`)
  --env K=V       set an environment variable for the app (repeatable); passed to
                  `open --env` with --open, exported in the foreground run
  --preview NAME  shorthand for --env BRANCHBOX_BACKEND=preview --env BRANCHBOX_PREVIEW_SCENARIO=NAME
                  (DEBUG preview backend, e.g. showcase, contract, legacy0134, cliMissing)
  --scratch-path DIR   SwiftPM build directory (default: macos/.build)
  -j, --jobs N    parallel build jobs (default: SwiftPM's)
  -h, --help      show this help

The bundle path is printed on the last line of a --build-only run.
Examples:
  scripts/macos-dev.sh --build-only
  scripts/macos-dev.sh --open --background --preview showcase
  scripts/macos-dev.sh --env BRANCHBOX_CLI_PATH="$PWD/target/debug/branchbox" --env BRANCHBOX_APP_LOG_STDERR=1
EOF
}

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE_DIR="${ROOT_DIR}/macos"
APP_DIR="${PACKAGE_DIR}/build/dev/BranchBox Dev.app"
BUNDLE_ID="dev.branchbox.app.dev"
BUNDLE_NAME="BranchBox Dev"
PRODUCT="BranchBox"

mode="foreground"
background=0
scratch_path=""
jobs=""
env_pairs=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --open) mode="open" ;;
    --build-only) mode="build-only" ;;
    --background) background=1 ;;
    --env)
      [[ $# -ge 2 && "$2" == *=* ]] || { echo "--env needs KEY=VALUE" >&2; exit 2; }
      env_pairs+=("$2"); shift ;;
    --env=*)
      pair="${1#--env=}"
      [[ "$pair" == *=* ]] || { echo "--env needs KEY=VALUE" >&2; exit 2; }
      env_pairs+=("$pair") ;;
    --preview)
      [[ $# -ge 2 ]] || { echo "--preview needs a scenario name" >&2; exit 2; }
      env_pairs+=("BRANCHBOX_BACKEND=preview" "BRANCHBOX_PREVIEW_SCENARIO=$2"); shift ;;
    --scratch-path)
      [[ $# -ge 2 ]] || { echo "--scratch-path needs a directory" >&2; exit 2; }
      scratch_path="$2"; shift ;;
    -j|--jobs)
      [[ $# -ge 2 ]] || { echo "$1 needs a number" >&2; exit 2; }
      jobs="$2"; shift ;;
    *) echo "Unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

if [[ "$background" == 1 && "$mode" != "open" ]]; then
  echo "--background only applies with --open" >&2
  exit 2
fi

swift_args=(--package-path "${PACKAGE_DIR}" --product "${PRODUCT}")
[[ -n "$scratch_path" ]] && swift_args+=(--scratch-path "$scratch_path")
[[ -n "$jobs" ]] && swift_args+=(-j "$jobs")

echo "Building ${PRODUCT} (debug)…" >&2
swift build "${swift_args[@]}" >&2
bin_dir="$(swift build "${swift_args[@]}" --show-bin-path)"
binary="${bin_dir}/${PRODUCT}"
[[ -x "$binary" ]] || { echo "Build produced no executable at ${binary}" >&2; exit 1; }

echo "Wrapping ${APP_DIR}…" >&2
rm -rf "${APP_DIR}"
mkdir -p "${APP_DIR}/Contents/MacOS" "${APP_DIR}/Contents/Resources"
cp "$binary" "${APP_DIR}/Contents/MacOS/${PRODUCT}"
# SwiftPM resource bundles of the app's targets, if any (Bundle.module looks next to the executable).
for bundle in "${bin_dir}"/BranchBox_*.bundle; do
  [[ -e "$bundle" ]] && cp -R "$bundle" "${APP_DIR}/Contents/Resources/"
done

# Info.plist from the release template (macos/Packaging/Info.plist.template), with the dev identity. The
# version is the Cargo workspace's; the build number and SHA come from git when this is a checkout.
TEMPLATE="${PACKAGE_DIR}/Packaging/Info.plist.template"
version="$(awk '
  /^\[/ { in_section = ($0 == "[workspace.package]"); next }
  in_section && $1 == "version" { gsub(/[" ]/, "", $0); sub(/^version=/, "", $0); print; exit }
' "${ROOT_DIR}/Cargo.toml")"
build="$(git -C "$ROOT_DIR" rev-list --count HEAD 2>/dev/null || echo 0)"
sha="$(git -C "$ROOT_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)"
sed -e "s|@BUNDLE_ID@|${BUNDLE_ID}|g" -e "s|@BUNDLE_NAME@|${BUNDLE_NAME}|g" -e "s|@VERSION@|${version:-0.0.0}|g" \
    -e "s|@BUILD@|${build}|g" -e "s|@GIT_SHA@|${sha}|g" -e "s|@YEAR@|$(date +%Y)|g" \
    "$TEMPLATE" > "${APP_DIR}/Contents/Info.plist"
if grep -q '@[A-Z_]*@' "${APP_DIR}/Contents/Info.plist"; then
  echo "Unrendered placeholders left in ${APP_DIR}/Contents/Info.plist" >&2
  exit 1
fi
plutil -lint -s "${APP_DIR}/Contents/Info.plist" >&2
if [[ -d "${PACKAGE_DIR}/Packaging/AppIcon.iconset" ]]; then
  iconutil --convert icns --output "${APP_DIR}/Contents/Resources/AppIcon.icns" "${PACKAGE_DIR}/Packaging/AppIcon.iconset"
fi

codesign --force --sign - --options runtime --timestamp=none --identifier "${BUNDLE_ID}" "${APP_DIR}" >&2

case "$mode" in
  build-only)
    echo "${APP_DIR}"
    ;;
  open)
    open_args=()
    [[ "$background" == 1 ]] && open_args+=(-g)
    for pair in ${env_pairs[@]+"${env_pairs[@]}"}; do open_args+=(--env "$pair"); done
    echo "Opening ${APP_DIR}" >&2
    open ${open_args[@]+"${open_args[@]}"} "${APP_DIR}"
    echo "${APP_DIR}"
    ;;
  foreground)
    for pair in ${env_pairs[@]+"${env_pairs[@]}"}; do export "${pair?}"; done
    echo "Running ${APP_DIR} in the foreground (Ctrl-C quits)" >&2
    exec "${APP_DIR}/Contents/MacOS/${PRODUCT}"
    ;;
esac
