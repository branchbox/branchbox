#!/bin/bash
# Cloudflare Tunnel Module
# Manages Cloudflare tunnel provisioning and DNS records
#
# This module handles:
# - Automatic tunnel provisioning via Cloudflare API
# - Manual tunnel configuration
# - DNS record management
# - Tunnel cleanup during teardown

# Module metadata
MODULE_NAME="tunnel"
MODULE_DESCRIPTION="Cloudflare tunnel provisioning and management"
MODULE_VERSION="1.0.0"
MODULE_ENABLED=false
MODULE_DEPENDENCIES=()

# Module configuration (set during module_init)
MODULE_TUNNEL_NAME=""
MODULE_TUNNEL_TOKEN=""
MODULE_FEATURE_URL=""
MODULE_SERVICE_URL=""
MODULE_CLOUDFLARE_API_KEY=""
MODULE_CLOUDFLARE_ACCOUNT_ID=""

# Implementation of module interface

module_detect() {
  # Tunnel module is optional and enabled if Cloudflare credentials are available
  if [ -n "${CLOUDFLARE_API_KEY:-}" ] && [ -n "${CLOUDFLARE_ACCOUNT_ID:-}" ]; then
    MODULE_ENABLED=true
    MODULE_CONFIDENCE=100
    return 0
  fi

  # Check if credentials exist in .env file
  if [ -n "${CONFIG_ENV:-}" ] && [ -f "${CONFIG_ENV}" ]; then
    if grep -q "^CLOUDFLARE_API_KEY=" "${CONFIG_ENV}" 2>/dev/null; then
      MODULE_ENABLED=true
      MODULE_CONFIDENCE=90
      return 0
    fi
  fi

  # Even without API credentials, module can be enabled for manual setup
  MODULE_ENABLED=true
  MODULE_CONFIDENCE=50
  return 0
}

module_init() {
  local main_dir="$1"
  local feature_dir="$2"
  local config="${3:-}"

  # Read Cloudflare credentials from environment or .env
  MODULE_CLOUDFLARE_API_KEY="${CLOUDFLARE_API_KEY:-}"
  MODULE_CLOUDFLARE_ACCOUNT_ID="${CLOUDFLARE_ACCOUNT_ID:-}"

  # Generate tunnel name and feature URL
  local work_feature=$(basename "$feature_dir")
  MODULE_TUNNEL_NAME="${BASE_PREFIX:-app}-${work_feature}"
  MODULE_FEATURE_URL="${BASE_PREFIX:-app}-${work_feature}.${BASE_DOMAIN:-localhost}"

  # Get service URL from adapter (or use default)
  if command -v adapter_get_service_url &> /dev/null; then
    MODULE_SERVICE_URL=$(adapter_get_service_url)
  else
    MODULE_SERVICE_URL="rails-app:3000"
  fi

  module_log_step "$MODULE_NAME" "Initialized tunnel: $MODULE_TUNNEL_NAME"
  module_log_step "$MODULE_NAME" "Feature URL: https://$MODULE_FEATURE_URL"

  return 0
}

module_setup() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Setting up Cloudflare tunnel..."

  # Try automatic provisioning first if API credentials available
  if [ -n "$MODULE_CLOUDFLARE_API_KEY" ] && [ -n "$MODULE_CLOUDFLARE_ACCOUNT_ID" ]; then
    # Check if tunnel already exists
    module_log_step "$MODULE_NAME" "Checking for existing tunnel..."
    local existing_tunnel_id=$(find_tunnel_by_name \
      "$MODULE_CLOUDFLARE_API_KEY" \
      "$MODULE_CLOUDFLARE_ACCOUNT_ID" \
      "$MODULE_TUNNEL_NAME")

    if [ -n "$existing_tunnel_id" ]; then
      module_log_warning "$MODULE_NAME" "Tunnel already exists: $existing_tunnel_id"
      module_log_step "$MODULE_NAME" "Cannot retrieve token for existing tunnel"

      # In non-interactive mode, skip manual setup
      if [ "${ACCEPT_DEFAULTS:-false}" = "true" ]; then
        module_log_step "$MODULE_NAME" "Run without --yes flag to configure manually, or delete tunnel: bin/feature-teardown"
      else
        module_log_step "$MODULE_NAME" "To use existing tunnel, paste its token below"
        module_log_step "$MODULE_NAME" "Or run: bin/feature-teardown $WORK_FEATURE --yes (to delete and recreate)"
        _manual_tunnel_setup
      fi
    else
      # Tunnel doesn't exist, create it
      module_log_step "$MODULE_NAME" "Creating new tunnel via API..."

      MODULE_TUNNEL_TOKEN=$(provision_tunnel_api \
        "$MODULE_CLOUDFLARE_API_KEY" \
        "$MODULE_CLOUDFLARE_ACCOUNT_ID" \
        "$MODULE_TUNNEL_NAME" \
        "$MODULE_FEATURE_URL" \
        "${BASE_DOMAIN:-localhost}" \
        "$MODULE_SERVICE_URL") || true

      if [ -n "$MODULE_TUNNEL_TOKEN" ]; then
        module_log_success "$MODULE_NAME" "Tunnel provisioned via API"
        module_log_success "$MODULE_NAME" "Tunnel name: $MODULE_TUNNEL_NAME"
        module_log_success "$MODULE_NAME" "Tunnel URL: https://$MODULE_FEATURE_URL"
      else
        module_log_warning "$MODULE_NAME" "API provisioning failed, falling back to manual setup"
        _manual_tunnel_setup
      fi
    fi
  else
    # No API credentials - manual setup only
    module_log_step "$MODULE_NAME" "No API credentials - manual setup required"
    _manual_tunnel_setup
  fi

  # Write tunnel configuration to .cloudflared.env
  if [ -n "$MODULE_TUNNEL_TOKEN" ]; then
    _write_tunnel_config "$feature_dir"
    module_log_success "$MODULE_NAME" "Tunnel configuration saved"
  else
    module_log_warning "$MODULE_NAME" "No tunnel token - you can add it later"
    module_log_step "$MODULE_NAME" "Edit: $feature_dir/.devcontainer/.cloudflared.env"
  fi

  return 0
}

module_teardown() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Cleaning up Cloudflare tunnel..."

  # Extract tunnel information from worktree .env files
  local tunnel_name=""
  local feature_url=""

  if [ -d "$feature_dir" ]; then
    # Try to get feature URL from worktree's .env file
    if [ -f "$feature_dir/.env" ]; then
      feature_url=$(grep "^APP_URL=" "$feature_dir/.env" 2>/dev/null | tail -1 | cut -d'=' -f2 | tr -d '"' | tr -d "'")
    fi

    # Fallback to .cloudflared.env if not found in .env
    if [ -z "$feature_url" ] && [ -f "$feature_dir/.devcontainer/.cloudflared.env" ]; then
      feature_url=$(grep "^DEV_HOSTNAME=" "$feature_dir/.devcontainer/.cloudflared.env" 2>/dev/null | cut -d'=' -f2 | tr -d '"' | tr -d "'")
    fi
  fi

  # Generate tunnel name from APP_URL pattern
  if [ -n "$feature_url" ]; then
    tunnel_name=$(echo "$feature_url" | cut -d'.' -f1)
  fi

  module_log_step "$MODULE_NAME" "Tunnel name: ${tunnel_name:-unknown}"
  module_log_step "$MODULE_NAME" "Feature URL: ${feature_url:-unknown}"

  # Delete tunnel via API if credentials available
  if [ -n "$MODULE_CLOUDFLARE_API_KEY" ] && [ -n "$MODULE_CLOUDFLARE_ACCOUNT_ID" ] && [ -n "$tunnel_name" ]; then
    module_log_step "$MODULE_NAME" "Looking up tunnel by name: $tunnel_name"

    local tunnel_id=$(find_tunnel_by_name "$MODULE_CLOUDFLARE_API_KEY" "$MODULE_CLOUDFLARE_ACCOUNT_ID" "$tunnel_name")

    if [ -n "$tunnel_id" ]; then
      module_log_step "$MODULE_NAME" "Found tunnel ID: $tunnel_id"

      if delete_tunnel "$MODULE_CLOUDFLARE_API_KEY" "$MODULE_CLOUDFLARE_ACCOUNT_ID" "$tunnel_id"; then
        module_log_success "$MODULE_NAME" "Tunnel deleted successfully"
      else
        module_log_warning "$MODULE_NAME" "Could not delete tunnel automatically"
        module_log_step "$MODULE_NAME" "You may need to delete it manually from Cloudflare Dashboard"
      fi
    else
      module_log_step "$MODULE_NAME" "Tunnel not found (may already be deleted)"
    fi
  else
    module_log_step "$MODULE_NAME" "Skipping tunnel deletion (no API credentials or tunnel name)"
  fi

  # Delete DNS record
  if [ -n "$MODULE_CLOUDFLARE_API_KEY" ] && [ -n "$feature_url" ]; then
    module_log_step "$MODULE_NAME" "Deleting DNS record for $feature_url..."

    if delete_dns_record "$MODULE_CLOUDFLARE_API_KEY" "$feature_url" "${BASE_DOMAIN:-localhost}"; then
      module_log_success "$MODULE_NAME" "DNS record deleted successfully"
    else
      module_log_step "$MODULE_NAME" "DNS record not found or already deleted"
    fi
  else
    module_log_step "$MODULE_NAME" "Skipping DNS cleanup (no API credentials)"
  fi

  return 0
}

module_validate() {
  local main_dir="$1"
  local feature_dir="$2"

  # Validate tunnel configuration file exists
  if [ ! -f "$feature_dir/.devcontainer/.cloudflared.env" ]; then
    module_log_warning "$MODULE_NAME" "Tunnel configuration file not found"
    module_log_step "$MODULE_NAME" "Expected: $feature_dir/.devcontainer/.cloudflared.env"
    module_log_step "$MODULE_NAME" "This is OK if tunnel setup hasn't been completed yet"
    return 0
  fi

  # Validate tunnel token is present
  if ! grep -q "^TUNNEL_TOKEN=" "$feature_dir/.devcontainer/.cloudflared.env" 2>/dev/null; then
    module_log_warning "$MODULE_NAME" "TUNNEL_TOKEN not found in .cloudflared.env"
    module_log_step "$MODULE_NAME" "Tunnel may not be configured correctly"
    return 0
  fi

  module_log_success "$MODULE_NAME" "Tunnel configuration looks valid"
  return 0
}

module_get_name() {
  echo "$MODULE_NAME"
}

module_get_description() {
  echo "$MODULE_DESCRIPTION"
}

module_get_version() {
  echo "$MODULE_VERSION"
}

module_health_check() {
  local feature_dir="$1"

  # Check if tunnel is reachable
  if [ -n "$MODULE_FEATURE_URL" ]; then
    if curl -s -o /dev/null -w "%{http_code}" "https://$MODULE_FEATURE_URL" | grep -q "200\|301\|302"; then
      return 0
    fi
  fi

  return 1
}

# Module-specific helper functions

# _manual_tunnel_setup() - Guide user through manual tunnel setup
_manual_tunnel_setup() {
  # Skip manual setup if running in non-interactive mode
  if [ "${ACCEPT_DEFAULTS:-false}" = "true" ]; then
    module_log_warning "$MODULE_NAME" "Skipping manual setup (--yes flag set)"
    module_log_step "$MODULE_NAME" "Tunnel token can be added later by editing .cloudflared.env"
    return 0
  fi

  echo -e "\n${CYAN}${BOLD}Manual Cloudflare Tunnel Setup Required${NC}\n"

  echo -e "1. Open Cloudflare Dashboard:"
  echo -e "   ${BLUE}https://dash.cloudflare.com${NC}"
  echo ""
  echo -e "2. Navigate to: ${BOLD}Zero Trust > Access > Tunnels${NC}"
  echo ""
  echo -e "3. Click ${BOLD}Create a tunnel${NC}"
  echo ""
  echo -e "4. Configure tunnel:"
  echo -e "   - Name: ${BOLD}$MODULE_TUNNEL_NAME${NC}"
  echo -e "   - Connector: ${BOLD}Cloudflared${NC}"
  echo ""
  echo -e "5. In ${BOLD}Route tunnel${NC} section:"
  echo -e "   - Subdomain: ${BOLD}$(echo "$MODULE_FEATURE_URL" | cut -d'.' -f1)${NC}"
  echo -e "   - Domain: ${BOLD}$(echo "$MODULE_FEATURE_URL" | cut -d'.' -f2-)${NC}"
  echo -e "   - Service Type: ${BOLD}HTTP${NC}"
  echo -e "   - URL: ${BOLD}$MODULE_SERVICE_URL${NC}"
  echo ""
  echo -e "6. Copy the tunnel token (starts with 'eyJ...')"
  echo ""

  read -p "Paste tunnel token: " -r MODULE_TUNNEL_TOKEN

  if [ -z "$MODULE_TUNNEL_TOKEN" ]; then
    module_log_warning "$MODULE_NAME" "No tunnel token provided"
  else
    module_log_success "$MODULE_NAME" "Tunnel token captured"
  fi
}

# _write_tunnel_config() - Write tunnel configuration to .cloudflared.env
#
# Args:
#   $1 - Feature worktree directory
_write_tunnel_config() {
  local feature_dir="$1"

  cat > "$feature_dir/.devcontainer/.cloudflared.env" <<EOF
# Cloudflare Tunnel Configuration for $(basename "$feature_dir")
# Generated on $(date +%Y-%m-%d)
TUNNEL_TOKEN=$MODULE_TUNNEL_TOKEN
DEV_HOSTNAME=$MODULE_FEATURE_URL
EOF
}
