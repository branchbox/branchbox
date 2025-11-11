#!/usr/bin/env bash
set -euo pipefail

# Package the BranchBox macOS SwiftUI app into an .app bundle and embed the Rust CLI.
# Requires: macOS with Xcode toolchain, Rust toolchain installed for macOS, and this repo checked out.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="BranchBoxApp"
APP_ID="dev.branchbox.app"
SWIFT_DIR="${ROOT_DIR}/macos"
BUILD_DIR="${ROOT_DIR}/macos/.build"
OUT_DIR="${ROOT_DIR}/macos/build"
APP_DIR="${OUT_DIR}/${APP_NAME}.app"

echo "[1/5] Building Swift app (release)"
pushd "${SWIFT_DIR}" >/dev/null
swift build -c release
SWIFT_BIN="${BUILD_DIR}/release/${APP_NAME}"
popd >/dev/null

echo "[2/5] Building Rust CLI (release)"
pushd "${ROOT_DIR}" >/dev/null
cargo build -p branchbox-cli --release
CLI_BIN="${ROOT_DIR}/target/release/branchbox"
popd >/dev/null

echo "[3/5] Creating app bundle layout"
rm -rf "${APP_DIR}"
mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${APP_DIR}/Contents/Resources/bin"

cat > "${APP_DIR}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${APP_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

echo "[4/5] Staging binaries"
cp "${SWIFT_BIN}" "${APP_DIR}/Contents/MacOS/${APP_NAME}"
cp "${CLI_BIN}" "${APP_DIR}/Contents/Resources/bin/branchbox"
chmod +x "${APP_DIR}/Contents/MacOS/${APP_NAME}" "${APP_DIR}/Contents/Resources/bin/branchbox"

echo "[5/5] Done: ${APP_DIR}"
echo "Open with: open \"${APP_DIR}\""

