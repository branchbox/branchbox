#!/bin/bash
# Cloudflare API utilities for git-worktree workflow
# Handles tunnel provisioning, DNS records, and cleanup

# Source logging for output
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/logging.sh" 2>/dev/null || true

# Provision Cloudflare tunnel via API
# Usage: token=$(provision_tunnel_api "api_key" "account_id" "tunnel_name" "feature_url" "base_domain" "service_url")
# Returns: tunnel token on stdout, logs to stderr
# Exit code: 0 on success, 1 on failure
provision_tunnel_api() {
  local api_key="$1"
  local account_id="$2"
  local tunnel_name="$3"
  local feature_url="$4"
  local base_domain="$5"
  local service_url="${6:-http://localhost:3000}"  # Default for backward compatibility

  # Disable xtrace for API calls to prevent debug output from contaminating JSON
  local xtrace_was_set=false
  if [[ $- =~ x ]]; then
    xtrace_was_set=true
    set +x
  fi

  # Strip any whitespace/newlines from parameters (can happen with shell debug mode)
  tunnel_name=$(echo "$tunnel_name" | tr -d '\n\r' | xargs)
  feature_url=$(echo "$feature_url" | tr -d '\n\r' | xargs)
  base_domain=$(echo "$base_domain" | tr -d '\n\r' | xargs)
  service_url=$(echo "$service_url" | tr -d '\n\r' | xargs)

  # Check for jq availability
  local has_jq=false
  if command -v jq >/dev/null 2>&1; then
    has_jq=true
  else
    echo -e "${YELLOW}⚠ Warning: 'jq' not found. Using grep/cut for JSON parsing (less reliable).${NC}" >&2
    echo -e "${YELLOW}  Install jq for more robust JSON parsing: apt-get install jq${NC}" >&2
  fi

  # Log to stderr to avoid contaminating the return value
  echo -e "${CYAN}ℹ Attempting API provisioning...${NC}" >&2

  # Create tunnel via API
  local response=$(curl -s -X POST \
    "https://api.cloudflare.com/client/v4/accounts/$account_id/cfd_tunnel" \
    -H "Authorization: Bearer $api_key" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"$tunnel_name\",\"config_src\":\"cloudflare\"}")

  if echo "$response" | grep -q '"success":true'; then
    local tunnel_id
    local tunnel_token

    if [ "$has_jq" = true ]; then
      tunnel_id=$(echo "$response" | jq -r '.result.id')
      tunnel_token=$(echo "$response" | jq -r '.result.token')
    else
      tunnel_id=$(echo "$response" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
      tunnel_token=$(echo "$response" | grep -o '"token":"[^"]*"' | cut -d'"' -f4)
    fi

    # Validate we got both ID and token
    if [ -z "$tunnel_id" ] || [ -z "$tunnel_token" ]; then
      echo -e "${RED}✗ Failed to extract tunnel ID or token from API response${NC}" >&2
      return 1
    fi

    echo -e "${GREEN}✓ Tunnel created: $tunnel_id${NC}" >&2

    # Configure tunnel route (output to stderr)
    curl -s -X PUT \
      "https://api.cloudflare.com/client/v4/accounts/$account_id/cfd_tunnel/$tunnel_id/configurations" \
      -H "Authorization: Bearer $api_key" \
      -H "Content-Type: application/json" \
      -d "{\"config\":{\"ingress\":[{\"hostname\":\"$feature_url\",\"service\":\"$service_url\"},{\"service\":\"http_status:404\"}]}}" >&2

    # Get zone ID for the domain
    local zone_response=$(curl -s -X GET \
      "https://api.cloudflare.com/client/v4/zones?name=$base_domain" \
      -H "Authorization: Bearer $api_key" \
      -H "Content-Type: application/json")

    local zone_id
    if [ "$has_jq" = true ]; then
      zone_id=$(echo "$zone_response" | jq -r '.result[0].id')
    else
      zone_id=$(echo "$zone_response" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
    fi

    if [ -n "$zone_id" ]; then
      # Create DNS CNAME record pointing to tunnel
      # Important: Use full UUID with hyphens, not truncated
      local tunnel_hostname="${tunnel_id}.cfargotunnel.com"
      curl -s -X POST \
        "https://api.cloudflare.com/client/v4/zones/$zone_id/dns_records" \
        -H "Authorization: Bearer $api_key" \
        -H "Content-Type: application/json" \
        -d "{\"type\":\"CNAME\",\"name\":\"$feature_url\",\"content\":\"$tunnel_hostname\",\"proxied\":true}" >&2

      echo -e "${GREEN}✓ Created DNS record: $feature_url -> $tunnel_hostname${NC}" >&2
    else
      echo -e "${YELLOW}⚠ Warning: Could not find zone ID for $base_domain - DNS record not created${NC}" >&2
      echo -e "${YELLOW}  You may need to create the DNS CNAME manually${NC}" >&2
    fi

    echo "$tunnel_token"
    # Restore xtrace if it was enabled
    [ "$xtrace_was_set" = true ] && set -x
    return 0
  else
    echo -e "${RED}✗ Tunnel creation failed${NC}" >&2
    echo -e "${YELLOW}API Response: $(echo "$response" | head -c 200)${NC}" >&2
    # Restore xtrace if it was enabled
    [ "$xtrace_was_set" = true ] && set -x
    return 1
  fi
}

# Delete Cloudflare tunnel via API
# Usage: delete_tunnel "api_key" "account_id" "tunnel_id"
# Returns: 0 on success, 1 on failure
delete_tunnel() {
  local api_key="$1"
  local account_id="$2"
  local tunnel_id="$3"

  local response=$(curl -s -X DELETE \
    "https://api.cloudflare.com/client/v4/accounts/$account_id/cfd_tunnel/$tunnel_id" \
    -H "Authorization: Bearer $api_key" \
    -H "Content-Type: application/json")

  if echo "$response" | grep -q '"success":true'; then
    return 0
  else
    return 1
  fi
}

# Find tunnel ID by name
# Usage: tunnel_id=$(find_tunnel_by_name "api_key" "account_id" "tunnel_name")
# Returns: tunnel ID on stdout, empty if not found
find_tunnel_by_name() {
  local api_key="$1"
  local account_id="$2"
  local tunnel_name="$3"

  local response=$(curl -s -X GET \
    "https://api.cloudflare.com/client/v4/accounts/$account_id/cfd_tunnel?name=$tunnel_name" \
    -H "Authorization: Bearer $api_key" \
    -H "Content-Type: application/json")

  # Check for jq availability
  if command -v jq >/dev/null 2>&1; then
    # Filter for active tunnels only (deleted_at is null)
    echo "$response" | jq -r '.result[] | select(.deleted_at == null) | .id' | head -1
  else
    # Fallback: use python to filter for active tunnels
    echo "$response" | python3 -c 'import sys, json; data = json.load(sys.stdin); active = [t for t in data.get("result", []) if t.get("deleted_at") is None]; print(active[0]["id"] if active else "")' 2>/dev/null
  fi
}

# Delete DNS record
# Usage: delete_dns_record "api_key" "feature_url" "base_domain"
# Returns: 0 on success, 1 on failure
delete_dns_record() {
  local api_key="$1"
  local feature_url="$2"
  local base_domain="$3"

  # Get zone ID
  local zone_response=$(curl -s -X GET \
    "https://api.cloudflare.com/client/v4/zones?name=$base_domain" \
    -H "Authorization: Bearer $api_key" \
    -H "Content-Type: application/json")

  local zone_id
  if command -v jq >/dev/null 2>&1; then
    zone_id=$(echo "$zone_response" | jq -r '.result[0].id // empty')
  else
    zone_id=$(echo "$zone_response" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
  fi

  if [ -z "$zone_id" ]; then
    return 1
  fi

  # Find DNS record
  local dns_response=$(curl -s -X GET \
    "https://api.cloudflare.com/client/v4/zones/$zone_id/dns_records?name=$feature_url" \
    -H "Authorization: Bearer $api_key" \
    -H "Content-Type: application/json")

  local dns_record_id
  if command -v jq >/dev/null 2>&1; then
    dns_record_id=$(echo "$dns_response" | jq -r '.result[0].id // empty')
  else
    dns_record_id=$(echo "$dns_response" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
  fi

  if [ -z "$dns_record_id" ]; then
    return 1
  fi

  # Delete DNS record
  local delete_response=$(curl -s -X DELETE \
    "https://api.cloudflare.com/client/v4/zones/$zone_id/dns_records/$dns_record_id" \
    -H "Authorization: Bearer $api_key" \
    -H "Content-Type: application/json")

  if echo "$delete_response" | grep -q '"success":true'; then
    return 0
  else
    return 1
  fi
}
