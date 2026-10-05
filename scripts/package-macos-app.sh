#!/usr/bin/env bash
set -euo pipefail

# Package the BranchBox Mac app as a sealed, signed macos/build/BranchBox.app (DESIGN §12.1).
# Requires macOS with Xcode or the Command Line Tools (swift, codesign, iconutil, plutil, lipo, ditto).
# The CLI is not built here: the app finds an installed `branchbox`, and --embed-cli bundles a prebuilt one.

usage() {
  cat <<'EOF'
Usage: scripts/package-macos-app.sh [options]

Builds the BranchBox product and assembles macos/build/BranchBox.app:
  Contents/MacOS/BranchBox, Contents/Info.plist (from macos/Packaging/Info.plist.template),
  Contents/Resources/AppIcon.icns (iconutil over macos/Packaging/AppIcon.iconset),
  and with --embed-cli, Contents/Helpers/branchbox.
Then signs it (hardened runtime) and checks it: codesign --verify --deep --strict, the architectures,
and CFBundleShortVersionString against the Cargo workspace version.

Options:
  --universal             build arm64 + x86_64 (default)
  --native                build for this Mac's architecture only
  --configuration CFG     release (default) or debug
  --embed-cli PATH        copy a prebuilt branchbox to Contents/Helpers/branchbox (signed first)
  --sign IDENTITY         codesign identity (default '-', ad hoc, no timestamp)
  --notarize              notarize and staple (needs a Developer ID identity and NOTARY_PROFILE)
  --zip                   also write BranchBox-<version>-<build>-<sha>.zip and .zip.sha256 to the output dir
  --out DIR               output directory (default macos/build)
  --scratch-path DIR      SwiftPM build directory (default: macos/.build)
  -j, --jobs N            parallel build jobs (default: SwiftPM's)
  -h, --help              show this help

Environment:
  BRANCHBOX_BUILD_NUMBER  CFBundleVersion override (default: git rev-list --count HEAD)
  BRANCHBOX_GIT_SHA       BranchBoxGitSHA override (default: git rev-parse --short HEAD)
  NOTARY_PROFILE          notarytool keychain profile, for --notarize

The last line of output is the bundle path (or the zip path with --zip).
EOF
}

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE_DIR="${ROOT_DIR}/macos"
PACKAGING_DIR="${PACKAGE_DIR}/Packaging"
PRODUCT="BranchBox"
BUNDLE_ID="dev.branchbox.app"
BUNDLE_NAME="BranchBox"

arch_mode="universal"
configuration="release"
embed_cli=""
identity="-"
notarize=0
make_zip=0
out_dir="${PACKAGE_DIR}/build"
scratch_path=""
jobs=""

die() { echo "error: $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --universal) arch_mode="universal" ;;
    --native) arch_mode="native" ;;
    --configuration)
      [[ $# -ge 2 ]] || { echo "--configuration needs release or debug" >&2; exit 2; }
      configuration="$2"; shift ;;
    --embed-cli)
      [[ $# -ge 2 ]] || { echo "--embed-cli needs a path" >&2; exit 2; }
      embed_cli="$2"; shift ;;
    --sign)
      [[ $# -ge 2 && -n "$2" ]] || { echo "--sign needs an identity ('-' for ad hoc)" >&2; exit 2; }
      identity="$2"; shift ;;
    --notarize) notarize=1 ;;
    --zip) make_zip=1 ;;
    --out)
      [[ $# -ge 2 ]] || { echo "--out needs a directory" >&2; exit 2; }
      out_dir="$2"; shift ;;
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

case "$configuration" in
  release|debug) ;;
  *) echo "--configuration must be release or debug, not '${configuration}'" >&2; exit 2 ;;
esac

# --notarize needs credentials this project does not have yet. Fail before building, naming the cause, rather
# than produce an unnotarized app that looks like a success.
if [[ "$notarize" == 1 ]]; then
  if [[ "$identity" == "-" || -z "${NOTARY_PROFILE:-}" ]]; then
    die "--notarize requires a Developer ID identity and NOTARY_PROFILE; see docs (pass --sign 'Developer ID Application: …' and set NOTARY_PROFILE to a 'xcrun notarytool store-credentials' profile)"
  fi
fi

if [[ -n "$embed_cli" ]]; then
  [[ -f "$embed_cli" && -x "$embed_cli" ]] || die "--embed-cli ${embed_cli} is not an executable file"
  embed_cli="$(cd "$(dirname "$embed_cli")" && pwd)/$(basename "$embed_cli")"
fi

for tool in swift codesign iconutil plutil lipo ditto shasum; do
  command -v "$tool" >/dev/null 2>&1 || die "${tool} not found; install Xcode or the Command Line Tools"
done

# --- Version metadata -------------------------------------------------------------------------------------

# [workspace.package] version, from cargo metadata when cargo is available, else straight from Cargo.toml.
workspace_version() {
  local version=""
  if command -v cargo >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    version="$(cd "$ROOT_DIR" && cargo metadata --no-deps --offline --format-version 1 2>/dev/null | python3 -c '
import json, sys
packages = json.load(sys.stdin)["packages"]
versions = [p["version"] for p in packages if p["name"] == "branchbox-cli"] or [p["version"] for p in packages]
print(versions[0] if versions else "")
' 2>/dev/null)" || version=""
  fi
  if [[ -z "$version" ]]; then
    version="$(awk '
      /^\[/ { in_section = ($0 == "[workspace.package]"); next }
      in_section && $1 == "version" { gsub(/[" ]/, "", $0); sub(/^version=/, "", $0); print; exit }
    ' "${ROOT_DIR}/Cargo.toml")"
  fi
  echo "$version"
}

VERSION="$(workspace_version)"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]] || die "could not read [workspace.package] version from ${ROOT_DIR}/Cargo.toml (got '${VERSION}')"

if git -C "$ROOT_DIR" rev-parse --git-dir >/dev/null 2>&1; then
  BUILD="${BRANCHBOX_BUILD_NUMBER:-$(git -C "$ROOT_DIR" rev-list --count HEAD)}"
  SHA="${BRANCHBOX_GIT_SHA:-$(git -C "$ROOT_DIR" rev-parse --short HEAD)}"
else
  [[ -n "${BRANCHBOX_BUILD_NUMBER:-}" && -n "${BRANCHBOX_GIT_SHA:-}" ]] \
    || echo "warning: ${ROOT_DIR} is not a git checkout; using build 0 and sha 'unknown' (set BRANCHBOX_BUILD_NUMBER and BRANCHBOX_GIT_SHA)" >&2
  BUILD="${BRANCHBOX_BUILD_NUMBER:-0}"
  SHA="${BRANCHBOX_GIT_SHA:-unknown}"
fi
[[ "$BUILD" =~ ^[0-9]+$ ]] || die "the build number must be an integer, not '${BUILD}'"

# --- Build ------------------------------------------------------------------------------------------------

swift_args=(--package-path "$PACKAGE_DIR" -c "$configuration" --product "$PRODUCT")
[[ "$arch_mode" == "universal" ]] && swift_args+=(--arch arm64 --arch x86_64)
[[ -n "$scratch_path" ]] && swift_args+=(--scratch-path "$scratch_path")
[[ -n "$jobs" ]] && swift_args+=(-j "$jobs")

echo "[1/6] Building ${PRODUCT} ${VERSION} (${BUILD}, ${SHA}): ${configuration}, ${arch_mode}" >&2
swift build "${swift_args[@]}" >&2
bin_dir="$(swift build "${swift_args[@]}" --show-bin-path)"
binary="${bin_dir}/${PRODUCT}"
[[ -x "$binary" ]] || die "the build produced no executable at ${binary}"

# --- Assemble ---------------------------------------------------------------------------------------------

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
APP_DIR="${out_dir}/${BUNDLE_NAME}.app"
CONTENTS="${APP_DIR}/Contents"

echo "[2/6] Assembling ${APP_DIR}" >&2
rm -rf "$APP_DIR"
mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Resources"
cp "$binary" "${CONTENTS}/MacOS/${PRODUCT}"
chmod 755 "${CONTENTS}/MacOS/${PRODUCT}"
# SwiftPM resource bundles of the app's targets, if any appear later.
for bundle in "${bin_dir}"/${PRODUCT}_*.bundle; do
  [[ -e "$bundle" ]] && cp -R "$bundle" "${CONTENTS}/Resources/"
done

# Render the plist template. Values are escaped for sed and for XML.
xml_escape() { local s="$1"; s="${s//&/&amp;}"; s="${s//</&lt;}"; s="${s//>/&gt;}"; printf '%s' "$s"; }
sed_escape() { printf '%s' "$1" | sed -e 's/[\\|&]/\\&/g'; }
render_plist() {
  local dest="$1" key value expr=()
  for pair in "BUNDLE_ID=${BUNDLE_ID}" "BUNDLE_NAME=${BUNDLE_NAME}" "VERSION=${VERSION}" "BUILD=${BUILD}" \
              "GIT_SHA=${SHA}" "YEAR=$(date +%Y)"; do
    key="${pair%%=*}"; value="$(sed_escape "$(xml_escape "${pair#*=}")")"
    expr+=(-e "s|@${key}@|${value}|g")
  done
  sed "${expr[@]}" "${PACKAGING_DIR}/Info.plist.template" > "$dest"
  ! grep -q '@[A-Z_]*@' "$dest" || die "unrendered tokens left in ${dest}: $(grep -o '@[A-Z_]*@' "$dest" | sort -u | tr '\n' ' ')"
  plutil -lint "$dest" >&2
}
render_plist "${CONTENTS}/Info.plist"
printf 'APPL????' > "${CONTENTS}/PkgInfo"

iconutil --convert icns --output "${CONTENTS}/Resources/AppIcon.icns" "${PACKAGING_DIR}/AppIcon.iconset"

if [[ -n "$embed_cli" ]]; then
  mkdir -p "${CONTENTS}/Helpers"
  cp "$embed_cli" "${CONTENTS}/Helpers/branchbox"
  chmod 755 "${CONTENTS}/Helpers/branchbox"
fi

# --- Sign -------------------------------------------------------------------------------------------------

sign_args=(--force --sign "$identity" --options runtime)
if [[ "$identity" == "-" ]]; then sign_args+=(--timestamp=none); else sign_args+=(--timestamp); fi

echo "[3/6] Signing with identity '${identity}'" >&2
# Nested code is signed first, so the app's seal covers the signed helper.
if [[ -n "$embed_cli" ]]; then
  codesign "${sign_args[@]}" --identifier dev.branchbox.cli "${CONTENTS}/Helpers/branchbox" >&2
fi
app_sign_args=("${sign_args[@]}" --identifier "$BUNDLE_ID")
[[ -f "${PACKAGING_DIR}/BranchBox.entitlements" ]] && app_sign_args+=(--entitlements "${PACKAGING_DIR}/BranchBox.entitlements")
codesign "${app_sign_args[@]}" "$APP_DIR" >&2

# --- Gates ------------------------------------------------------------------------------------------------

echo "[4/6] Verifying" >&2
codesign --verify --deep --strict --verbose=2 "$APP_DIR" >&2 || die "codesign --verify --deep --strict failed for ${APP_DIR}"

archs="$(lipo -archs "${CONTENTS}/MacOS/${PRODUCT}")"
if [[ "$arch_mode" == "universal" ]]; then
  [[ "$archs" == "x86_64 arm64" ]] || die "expected a universal binary (x86_64 arm64), got '${archs}'"
fi
echo "  architectures: ${archs}" >&2

plist_version="$(plutil -extract CFBundleShortVersionString raw -o - "${CONTENTS}/Info.plist")"
[[ "$plist_version" == "$VERSION" ]] || die "CFBundleShortVersionString is '${plist_version}', expected ${VERSION}"
plist_id="$(plutil -extract CFBundleIdentifier raw -o - "${CONTENTS}/Info.plist")"
[[ "$plist_id" == "$BUNDLE_ID" ]] || die "CFBundleIdentifier is '${plist_id}', expected ${BUNDLE_ID}"
[[ -s "${CONTENTS}/Resources/AppIcon.icns" ]] || die "Contents/Resources/AppIcon.icns is missing"
echo "  ${BUNDLE_ID} ${plist_version} (${BUILD}, ${SHA})" >&2

# --- Notarize ---------------------------------------------------------------------------------------------

# Guarded above: this runs only with a Developer ID identity and NOTARY_PROFILE, neither of which CI has yet.
# Untested until those exist.
if [[ "$notarize" == 1 ]]; then
  echo "[5/6] Notarizing with profile ${NOTARY_PROFILE}" >&2
  submit_zip="$(mktemp -d)/${BUNDLE_NAME}.zip"
  ditto -c -k --sequesterRsrc --keepParent "$APP_DIR" "$submit_zip"
  xcrun notarytool submit "$submit_zip" --keychain-profile "$NOTARY_PROFILE" --wait >&2
  rm -rf "$(dirname "$submit_zip")"
  xcrun stapler staple "$APP_DIR" >&2
  spctl -a -vv "$APP_DIR" >&2
else
  echo "[5/6] Not notarized (pass --notarize with a Developer ID identity)" >&2
fi

# --- Zip --------------------------------------------------------------------------------------------------

if [[ "$make_zip" == 1 ]]; then
  zip_path="${out_dir}/${BUNDLE_NAME}-${VERSION}-${BUILD}-${SHA}.zip"
  echo "[6/6] Zipping ${zip_path}" >&2
  rm -f "$zip_path" "${zip_path}.sha256"
  ditto -c -k --sequesterRsrc --keepParent "$APP_DIR" "$zip_path"
  (cd "$out_dir" && shasum -a 256 "$(basename "$zip_path")" > "$(basename "$zip_path").sha256")
  cat "${zip_path}.sha256" >&2
  echo "$zip_path"
else
  echo "[6/6] Done" >&2
  echo "$APP_DIR"
fi
