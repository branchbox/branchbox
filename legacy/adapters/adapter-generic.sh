#!/bin/bash
# Generic Stack Adapter for git-worktree workflow
# Fallback adapter for projects that don't match specific stacks

# Source logging utilities
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../utils/logging.sh" 2>/dev/null || true

# Adapter name
adapter_get_name() {
  echo "Generic"
}

# Get service URL for Cloudflare tunnel ingress
adapter_get_service_url() {
  echo "http://localhost:3000"
}

# Detect generic project (always returns true as fallback)
adapter_detect() {
  ADAPTER_CONFIDENCE=10
  return 0
}

# Copy common secret files
# Generic adapter copies standard secret file patterns:
# - .env files
# - Files with "secret", "credential", "key" in the name
adapter_copy_secrets() {
  local src="$1"
  local dest="$2"

  local copied=0

  # Copy common .env files
  for env_file in .env .env.local .env.production .env.development; do
    if [ -f "$src/$env_file" ]; then
      cp "$src/$env_file" "$dest/"
      log_success "Copied $env_file" 2>/dev/null || echo "✓ Copied $env_file"
      copied=1
    fi
  done

  # Copy files with "secret", "credential", or "key" in the name (at root level only)
  for pattern in "*secret*" "*credential*" "*key*" "*.pem" "*.crt"; do
    for file in "$src"/$pattern; do
      if [ -f "$file" ]; then
        local filename=$(basename "$file")
        # Skip common non-secret files
        if [[ ! "$filename" =~ ^(package-lock\.json|yarn\.lock|Gemfile\.lock)$ ]]; then
          cp "$file" "$dest/"
          log_success "Copied $filename" 2>/dev/null || echo "✓ Copied $filename"
          copied=1
        fi
      fi
    done
  done

  if [ "$copied" -eq 0 ]; then
    log_info "No secret files found to copy" 2>/dev/null || echo "ℹ No secret files found"
    log_info "Manual configuration may be required" 2>/dev/null || echo "ℹ Manual configuration may be required"
  fi

  return 0
}

# Setup database (generic - no specific commands)
adapter_setup_database() {
  log_info "No stack-specific database setup (generic adapter)" 2>/dev/null || echo "ℹ No stack-specific database setup"
  log_info "Run your database setup commands manually if needed" 2>/dev/null || echo "ℹ Run your database setup commands manually if needed"
  return 0
}

# Health check (generic - always passes)
adapter_healthcheck() {
  # No specific health check for generic projects
  return 0
}

# Cleanup (generic - minimal cleanup)
adapter_cleanup() {
  # Remove common temporary directories
  for dir in tmp temp .tmp; do
    if [ -d "$dir" ]; then
      rm -rf "$dir"/* 2>/dev/null || true
      log_success "Cleaned up $dir/ directory" 2>/dev/null || echo "✓ Cleaned up $dir/"
    fi
  done

  return 0
}
