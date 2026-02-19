#!/usr/bin/env bash
# Build script for GitHub Pages
# Combines landing page + Docusaurus docs into single deployment

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$ROOT_DIR/build"
DEMO_STACK="rust"
SKIP_DEMO_ASSETS=0
INSTALL_DEMO_LINUX_DEPS=0

usage() {
  cat <<'EOF'
Usage: scripts/build-site.sh [options]

Options:
  --demo-stack <rust|node|rails|generic>  Stack used for rendered website/docs demo assets (default: rust)
  --skip-demo-assets                       Skip remotion demo rendering/publish step
  --install-demo-linux-deps                Install Linux browser dependencies before rendering demos
  -h, --help                               Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --demo-stack)
      DEMO_STACK="${2:-}"
      shift
      ;;
    --demo-stack=*)
      DEMO_STACK="${1#*=}"
      ;;
    --skip-demo-assets)
      SKIP_DEMO_ASSETS=1
      ;;
    --install-demo-linux-deps)
      INSTALL_DEMO_LINUX_DEPS=1
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

case "$DEMO_STACK" in
  rust|node|rails|generic) ;;
  *)
    echo "Unsupported demo stack: $DEMO_STACK" >&2
    exit 1
    ;;
esac

echo "Building BranchBox site..."

if [[ "$SKIP_DEMO_ASSETS" == "0" ]]; then
  echo "Rendering and publishing demo assets (stack: $DEMO_STACK)..."
  REMOTION_ARGS=(--stack "$DEMO_STACK" --target both)
  if [[ "$INSTALL_DEMO_LINUX_DEPS" == "1" ]]; then
    REMOTION_ARGS+=(--install-linux-deps)
  fi
  "$ROOT_DIR/scripts/remotion-docs-all.sh" "${REMOTION_ARGS[@]}"
else
  echo "Skipping demo asset render (--skip-demo-assets)"
fi

# Clean build directory
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# 1. Copy landing page to root (including dotfiles for complete static output)
echo "Copying landing page..."
cp -r "$ROOT_DIR/website/." "$BUILD_DIR/"

# 2. Build Docusaurus docs
echo "Building documentation..."
cd "$ROOT_DIR/docs"
npm ci
npm run build

# 3. Copy docs to /docs/ subpath (including dotfiles)
echo "Copying docs to /docs/..."
mkdir -p "$BUILD_DIR/docs"
cp -r "$ROOT_DIR/docs/build/." "$BUILD_DIR/docs/"

# 4. Add .nojekyll for GitHub Pages
touch "$BUILD_DIR/.nojekyll"

echo ""
echo "Build complete."
echo "Output: $BUILD_DIR/"
echo "Landing page: $BUILD_DIR/index.html"
echo "Documentation: $BUILD_DIR/docs/"
