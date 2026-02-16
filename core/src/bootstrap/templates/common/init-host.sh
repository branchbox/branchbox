#!/usr/bin/env bash
set -euo pipefail

# initializeCommand runs in a non-login shell, so include common binary locations.
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:$HOME/.local/bin"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEVCONTAINER_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
TOKEN_FILE="${DEVCONTAINER_DIR}/.github-token.env"
SIGNING_KEY_FILE="${DEVCONTAINER_DIR}/.git-signing-key"
GIT_CONFIG_FILE="${DEVCONTAINER_DIR}/.gitconfig.env"

OP_GITHUB_REF="${OP_GITHUB_REF:-op://<vault>/<item>/token}"
OP_SIGNING_KEY_REF="${OP_SIGNING_KEY_REF:-op://<vault>/<item>/private key}"

# Ensure mount targets exist before docker compose evaluates the file mounts.
touch "${TOKEN_FILE}" "${SIGNING_KEY_FILE}" "${GIT_CONFIG_FILE}"

if ! command -v op >/dev/null 2>&1; then
  echo "BranchBox: 1Password CLI (op) not found; skipping credential refresh."
  exit 0
fi

read_op_secret() {
  local reference="$1"
  local label="$2"
  local attempt
  local op_output
  for attempt in 1 2 3; do
    if op_output="$(op read "${reference}" 2>&1)"; then
      printf '%s' "${op_output}"
      return 0
    fi
    if [[ "${attempt}" -lt 3 ]]; then
      sleep 1
      continue
    fi
    echo "BranchBox warning: unable to read ${label} from 1Password (${reference})." >&2
    echo "BranchBox warning: output from 1Password CLI:" >&2
    printf '%s\n' "${op_output}" | sed 's/^/  /' >&2
  done
  return 1
}

echo "BranchBox: refreshing GitHub credentials from 1Password..."

if github_token="$(read_op_secret "${OP_GITHUB_REF}" "GitHub token")"; then
  token_tmp="${TOKEN_FILE}.tmp"
  printf 'GITHUB_TOKEN=%s\n' "${github_token}" >"${token_tmp}"
  chmod 600 "${token_tmp}"
  mv "${token_tmp}" "${TOKEN_FILE}"
fi

if signing_key="$(read_op_secret "${OP_SIGNING_KEY_REF}" "signing key")"; then
  signing_tmp="${SIGNING_KEY_FILE}.tmp"
  printf '%s\n' "${signing_key}" >"${signing_tmp}"
  chmod 600 "${signing_tmp}"
  mv "${signing_tmp}" "${SIGNING_KEY_FILE}"
fi

git_user_name="$(git config --global user.name 2>/dev/null || true)"
git_user_email="$(git config --global user.email 2>/dev/null || true)"
git_user_name="${git_user_name//$'\r'/}"
git_user_name="${git_user_name//$'\n'/}"
git_user_email="${git_user_email//$'\r'/}"
git_user_email="${git_user_email//$'\n'/}"
if [[ -n "${git_user_name}" || -n "${git_user_email}" ]]; then
  echo "BranchBox: propagating host git identity into devcontainer."
fi
: >"${GIT_CONFIG_FILE}"
if [[ -n "${git_user_name}" ]]; then
  printf 'GIT_USER_NAME=%s\n' "${git_user_name}" >>"${GIT_CONFIG_FILE}"
fi
if [[ -n "${git_user_email}" ]]; then
  printf 'GIT_USER_EMAIL=%s\n' "${git_user_email}" >>"${GIT_CONFIG_FILE}"
fi

echo "BranchBox: host credential refresh complete."
