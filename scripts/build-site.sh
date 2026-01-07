#!/bin/bash
# Build script for GitHub Pages
# Combines landing page + Docusaurus docs into single deployment

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$ROOT_DIR/build"

echo "🏗️  Building BranchBox site..."

# Clean build directory
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# 1. Copy landing page to root
echo "📄 Copying landing page..."
cp -r "$ROOT_DIR/website/"* "$BUILD_DIR/"

# 2. Build Docusaurus docs
echo "📚 Building documentation..."
cd "$ROOT_DIR/docs"
npm ci
npm run build

# 3. Copy docs to /docs/ subpath
echo "📁 Copying docs to /docs/..."
mkdir -p "$BUILD_DIR/docs"
cp -r "$ROOT_DIR/docs/build/"* "$BUILD_DIR/docs/"

# 4. Add .nojekyll for GitHub Pages
touch "$BUILD_DIR/.nojekyll"

echo ""
echo "✅ Build complete!"
echo "   Output: $BUILD_DIR/"
echo ""
echo "   Landing page: $BUILD_DIR/index.html"
echo "   Documentation: $BUILD_DIR/docs/"
