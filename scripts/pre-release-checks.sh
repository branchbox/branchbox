#!/usr/bin/env bash
#
# pre-release-checks.sh - Run all quality checks before a release
#
# Usage:
#   ./scripts/pre-release-checks.sh [--quick]
#
# Options:
#   --quick    Skip long-running tests, only run fmt and clippy
#
# Exit codes:
#   0 - All checks passed
#   1 - One or more checks failed
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Parse arguments
QUICK_MODE=false
for arg in "$@"; do
    case $arg in
        --quick)
            QUICK_MODE=true
            shift
            ;;
    esac
done

# Track failures
FAILED=0

print_header() {
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_failure() {
    echo -e "${RED}✗ $1${NC}"
    FAILED=1
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

print_info() {
    echo -e "  $1"
}

# Change to repo root
cd "$(git rev-parse --show-toplevel)"

print_header "Pre-Release Quality Checks"

# Check git status
print_header "Git Status"
if [[ -n "$(git status --porcelain)" ]]; then
    print_warning "Working directory has uncommitted changes"
    git status --short
else
    print_success "Working directory is clean"
fi

# Check branch
CURRENT_BRANCH=$(git branch --show-current)
if [[ "$CURRENT_BRANCH" != "main" ]]; then
    print_warning "Not on main branch (currently on: $CURRENT_BRANCH)"
else
    print_success "On main branch"
fi

# Format check
print_header "Checking Code Formatting"
if cargo fmt --all -- --check 2>/dev/null; then
    print_success "Code formatting OK"
else
    print_failure "Code formatting issues found. Run: cargo fmt --all"
fi

# Clippy
print_header "Running Clippy"
if cargo clippy --all-targets --all-features -- -D warnings 2>&1; then
    print_success "Clippy passed"
else
    print_failure "Clippy found issues"
fi

if [[ "$QUICK_MODE" == "true" ]]; then
    print_warning "Quick mode: skipping tests and doc build"
else
    # Tests
    print_header "Running Tests"
    if cargo test --all-features 2>&1; then
        print_success "All tests passed"
    else
        print_failure "Some tests failed"
    fi

    # Doc build
    print_header "Building Documentation"
    if cargo doc --no-deps --all-features 2>&1; then
        print_success "Rust docs built successfully"
    else
        print_failure "Rust doc build failed"
    fi

    # Docusaurus build
    print_header "Building Docusaurus Site"
    if [[ -d "docs" ]] && [[ -f "docs/package.json" ]]; then
        cd docs
        if npm install --silent 2>/dev/null && npm run build 2>&1; then
            print_success "Docusaurus site built successfully"
        else
            print_failure "Docusaurus build failed"
        fi
        cd ..
    else
        print_warning "Docusaurus site not found, skipping"
    fi
fi

# Check for unreleased changes
print_header "Checking CHANGELOG.md"
if grep -q "^## \[Unreleased\]" CHANGELOG.md; then
    # Check if there's content under Unreleased
    UNRELEASED_CONTENT=$(awk '/^## \[Unreleased\]/{found=1; next} /^## \[/{found=0} found && /^### /{print}' CHANGELOG.md)
    if [[ -n "$UNRELEASED_CONTENT" ]]; then
        print_success "Found unreleased changes in CHANGELOG.md"
        echo "$UNRELEASED_CONTENT" | while read -r line; do
            print_info "$line"
        done
    else
        print_warning "No unreleased changes found in CHANGELOG.md"
    fi
else
    print_failure "CHANGELOG.md missing [Unreleased] section"
fi

# Get current version
print_header "Version Information"
CURRENT_VERSION=$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
print_info "Current version: $CURRENT_VERSION"

LATEST_TAG=$(git describe --tags --abbrev=0 2>/dev/null || echo "none")
print_info "Latest tag: $LATEST_TAG"

# Summary
print_header "Summary"
if [[ $FAILED -eq 0 ]]; then
    print_success "All checks passed! Ready for release."
    echo ""
    echo "Next steps:"
    echo "  1. Update CHANGELOG.md: Change [Unreleased] to [X.Y.Z] - $(date +%Y-%m-%d)"
    echo "  2. Update RELEASING.md version history table"
    echo "  3. Commit: git add CHANGELOG.md RELEASING.md && git commit -m 'chore(release): prepare vX.Y.Z'"
    echo "  4. Dry-run: cargo release --workspace <minor|patch|major> --dry-run"
    echo "  5. Release: cargo release --workspace <minor|patch|major> --execute --no-confirm"
    echo ""
    exit 0
else
    print_failure "Some checks failed. Please fix issues before releasing."
    exit 1
fi
