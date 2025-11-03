#!/bin/bash
# Rails Stack Adapter for git-worktree workflow

# Source logging utilities
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../utils/logging.sh" 2>/dev/null || true

# Adapter name
adapter_get_name() {
  echo "Rails"
}

# Get service URL for Cloudflare tunnel ingress
adapter_get_service_url() {
  echo "http://rails-app:3000"
}

# Detect Rails project
# Looks for Gemfile with 'rails' dependency
adapter_detect() {
  if [[ -f "Gemfile" ]] && grep -q "gem ['\"]rails['\"]" Gemfile 2>/dev/null; then
    ADAPTER_CONFIDENCE=95
    return 0
  fi

  # Also check for rails gem without quotes
  if [[ -f "Gemfile" ]] && grep -q "gem rails" Gemfile 2>/dev/null; then
    ADAPTER_CONFIDENCE=95
    return 0
  fi

  return 1
}

# Copy Rails-specific secret files
# Rails stores secrets in:
# - config/master.key (Rails 5.2+)
# - config/credentials/*.key (environment-specific credentials)
adapter_copy_secrets() {
  local src="$1"
  local dest="$2"

  local copied=0

  # Copy master.key if it exists
  if [ -f "$src/config/master.key" ]; then
    mkdir -p "$dest/config"
    cp "$src/config/master.key" "$dest/config/"
    log_success "Copied config/master.key" 2>/dev/null || echo "✓ Copied config/master.key"
    copied=1
  fi

  # Copy environment-specific credential keys
  if [ -d "$src/config/credentials" ]; then
    mkdir -p "$dest/config/credentials"
    for key_file in "$src/config/credentials/"*.key; do
      if [ -f "$key_file" ]; then
        cp "$key_file" "$dest/config/credentials/"
        local key_name=$(basename "$key_file")
        log_success "Copied $key_name" 2>/dev/null || echo "✓ Copied $key_name"
        copied=1
      fi
    done
  fi

  # Also check for .env files (some Rails apps use dotenv)
  if [ -f "$src/.env" ]; then
    cp "$src/.env" "$dest/"
    log_success "Copied .env" 2>/dev/null || echo "✓ Copied .env"
    copied=1
  fi

  if [ -f "$src/.env.local" ]; then
    cp "$src/.env.local" "$dest/"
    log_success "Copied .env.local" 2>/dev/null || echo "✓ Copied .env.local"
    copied=1
  fi

  if [ "$copied" -eq 0 ]; then
    log_info "No Rails secret files found to copy" 2>/dev/null || echo "ℹ No Rails secret files found"
  fi

  return 0
}

# Setup Rails database (runs inside devcontainer)
adapter_setup_database() {
  if [ -f "bin/rails" ]; then
    log_info "Setting up Rails database..." 2>/dev/null || echo "Setting up Rails database..."
    bin/rails db:setup
    return $?
  fi

  return 0
}

# Health check for Rails
adapter_healthcheck() {
  if [ -f "bin/rails" ]; then
    # Check if Rails can boot
    timeout 10 bin/rails runner "puts 'OK'" >/dev/null 2>&1
    return $?
  fi

  return 0
}

# Cleanup Rails temporary files
adapter_cleanup() {
  # Remove temporary files
  if [ -d "tmp" ]; then
    rm -rf tmp/cache/* tmp/pids/* tmp/sockets/* 2>/dev/null || true
    log_success "Cleaned up Rails tmp/ directory" 2>/dev/null || echo "✓ Cleaned up tmp/"
  fi

  return 0
}
