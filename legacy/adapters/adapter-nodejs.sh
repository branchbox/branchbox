#!/bin/bash
# Node.js Stack Adapter for git-worktree workflow

# Source logging utilities
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../utils/logging.sh" 2>/dev/null || true

# Adapter name
adapter_get_name() {
  echo "Node.js"
}

# Get service URL for Cloudflare tunnel ingress
adapter_get_service_url() {
  echo "http://localhost:3000"
}

# Detect Node.js project
# Looks for package.json
adapter_detect() {
  if [[ -f "package.json" ]]; then
    ADAPTER_CONFIDENCE=90

    # Increase confidence for specific frameworks
    if command -v jq >/dev/null 2>&1; then
      if jq -e '.dependencies.next' package.json >/dev/null 2>&1; then
        ADAPTER_CONFIDENCE=95
      elif jq -e '.dependencies.express' package.json >/dev/null 2>&1; then
        ADAPTER_CONFIDENCE=95
      elif jq -e '.dependencies.react' package.json >/dev/null 2>&1; then
        ADAPTER_CONFIDENCE=92
      fi
    fi

    return 0
  fi

  return 1
}

# Copy Node.js-specific secret files
# Node.js typically stores secrets in:
# - .env, .env.local, .env.production
# - Service account JSON files
# - Next.js: .env.production.local
adapter_copy_secrets() {
  local src="$1"
  local dest="$2"

  local copied=0

  # Copy .env files
  for env_file in .env .env.local .env.production .env.production.local .env.development.local; do
    if [ -f "$src/$env_file" ]; then
      cp "$src/$env_file" "$dest/"
      log_success "Copied $env_file" 2>/dev/null || echo "✓ Copied $env_file"
      copied=1
    fi
  done

  # Copy service account keys (common pattern)
  for key_file in "$src"/*service-account*.json "$src"/*credentials*.json; do
    if [ -f "$key_file" ]; then
      local key_name=$(basename "$key_file")
      cp "$key_file" "$dest/"
      log_success "Copied $key_name" 2>/dev/null || echo "✓ Copied $key_name"
      copied=1
    fi
  done

  # Copy .npmrc or .yarnrc if they exist (may contain auth tokens)
  for rc_file in .npmrc .yarnrc .yarnrc.yml; do
    if [ -f "$src/$rc_file" ]; then
      cp "$src/$rc_file" "$dest/"
      log_success "Copied $rc_file" 2>/dev/null || echo "✓ Copied $rc_file"
      copied=1
    fi
  done

  if [ "$copied" -eq 0 ]; then
    log_info "No Node.js secret files found to copy" 2>/dev/null || echo "ℹ No Node.js secret files found"
  fi

  return 0
}

# Setup Node.js database (runs inside devcontainer)
adapter_setup_database() {
  if [ ! -f "package.json" ]; then
    return 0
  fi

  # Detect package manager
  local pm="npm"
  if [ -f "yarn.lock" ]; then
    pm="yarn"
  elif [ -f "pnpm-lock.yaml" ]; then
    pm="pnpm"
  fi

  log_info "Installing dependencies with $pm..." 2>/dev/null || echo "Installing dependencies with $pm..."
  $pm install

  # Run database migrations if script exists
  if command -v jq >/dev/null 2>&1; then
    if jq -e '.scripts["db:migrate"]' package.json >/dev/null 2>&1; then
      log_info "Running database migrations..." 2>/dev/null || echo "Running database migrations..."
      $pm run db:migrate
    elif jq -e '.scripts.migrate' package.json >/dev/null 2>&1; then
      log_info "Running migrations..." 2>/dev/null || echo "Running migrations..."
      $pm run migrate
    fi

    # Seed database if script exists
    if jq -e '.scripts["db:seed"]' package.json >/dev/null 2>&1; then
      log_info "Seeding database..." 2>/dev/null || echo "Seeding database..."
      $pm run db:seed
    fi
  fi

  return 0
}

# Health check for Node.js
adapter_healthcheck() {
  if [ -f "package.json" ]; then
    # Check if main file can be required (basic syntax check)
    if command -v jq >/dev/null 2>&1; then
      local main=$(jq -r '.main // "index.js"' package.json)
      if [ -f "$main" ]; then
        timeout 5 node -e "require('./$main')" >/dev/null 2>&1
        return $?
      fi
    fi
  fi

  return 0
}

# Cleanup Node.js build artifacts
adapter_cleanup() {
  # Remove build directories
  for dir in dist build .next out; do
    if [ -d "$dir" ]; then
      rm -rf "$dir"
      log_success "Cleaned up $dir/ directory" 2>/dev/null || echo "✓ Cleaned up $dir/"
    fi
  done

  # Clear caches
  for cache_dir in .cache .parcel-cache .turbo; do
    if [ -d "$cache_dir" ]; then
      rm -rf "$cache_dir"
      log_success "Cleaned up $cache_dir/ cache" 2>/dev/null || echo "✓ Cleaned up $cache_dir/"
    fi
  done

  return 0
}
