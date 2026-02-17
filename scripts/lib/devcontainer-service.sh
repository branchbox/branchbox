#!/usr/bin/env bash

detect_compose_service() {
  local compose_file="$1"
  if [[ ! -f "$compose_file" ]]; then
    return
  fi

  awk '
    /^services:[[:space:]]*$/ { in_services = 1; next }
    in_services && /^[^[:space:]]/ { exit }
    in_services && /^[[:space:]]+[A-Za-z0-9._-]+:[[:space:]]*$/ {
      service = $1
      sub(/:$/, "", service)
      print service
      exit
    }
  ' "$compose_file"
}

read_devcontainer_service() {
  local workspace="$1"
  devcontainer read-configuration \
    --workspace-folder "$workspace" \
    --log-format json 2>/dev/null \
    | jq -r 'select(type == "object" and has("configuration")) | .configuration.service // empty' \
    | tail -n 1
}

resolve_devcontainer_service() {
  local devcontainer_json="$1"
  local compose_file="$2"
  local fallback="${3:-}"
  local service_name=""
  local workspace=""

  if [[ -f "$devcontainer_json" ]]; then
    workspace="$(cd "$(dirname "$devcontainer_json")/.." 2>/dev/null && pwd || true)"
  fi

  if [[ -n "$workspace" ]]; then
    service_name="$(read_devcontainer_service "$workspace" || true)"
  fi

  if [[ -z "$service_name" && -f "$devcontainer_json" ]]; then
    service_name="$(sed -e 's#//.*##' "$devcontainer_json" | jq -r '.service // empty' 2>/dev/null || true)"
  fi

  if [[ -z "$service_name" ]]; then
    service_name="$(detect_compose_service "$compose_file" || true)"
  fi

  if [[ -z "$service_name" && -n "$fallback" ]]; then
    service_name="$fallback"
  fi

  printf '%s\n' "$service_name"
}
