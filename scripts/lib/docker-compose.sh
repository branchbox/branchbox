#!/usr/bin/env bash

configure_compose_command() {
  if docker compose version >/dev/null 2>&1; then
    COMPOSE_CMD=(docker compose)
    return 0
  fi

  if command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_CMD=(docker-compose)
    if declare -f log >/dev/null 2>&1; then
      log "docker compose plugin unavailable; falling back to docker-compose"
    else
      printf '==> %s\n' "docker compose plugin unavailable; falling back to docker-compose"
    fi
    return 0
  fi

  local err_message="Neither docker compose nor docker-compose is available."
  if declare -f fatal >/dev/null 2>&1; then
    fatal "$err_message"
  else
    printf 'ERROR: %s\n' "$err_message" >&2
    return 1
  fi
}
