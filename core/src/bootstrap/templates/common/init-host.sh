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
DEFAULT_OP_GITHUB_REF='op://<vault>/<item>/token'
DEFAULT_OP_SIGNING_KEY_REF='op://<vault>/<item>/private key'

# Ensure mount targets exist before docker compose evaluates the file mounts.
touch "${TOKEN_FILE}" "${SIGNING_KEY_FILE}" "${GIT_CONFIG_FILE}"
chmod 600 "${TOKEN_FILE}" "${SIGNING_KEY_FILE}" "${GIT_CONFIG_FILE}"

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

should_skip_op_refresh() {
  case "${BRANCHBOX_SKIP_OP_REFRESH:-0}" in
    1|true|TRUE|yes|YES|on|ON)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

op_refresh_enabled=1
if should_skip_op_refresh; then
  op_refresh_enabled=0
  echo "BranchBox: skipping 1Password refresh (BRANCHBOX_SKIP_OP_REFRESH=${BRANCHBOX_SKIP_OP_REFRESH})."
elif ! command -v op >/dev/null 2>&1; then
  op_refresh_enabled=0
  echo "BranchBox: 1Password CLI (op) not found; skipping credential refresh."
fi

if [[ "${op_refresh_enabled}" == "1" ]]; then
  echo "BranchBox: refreshing GitHub credentials from 1Password..."

  if [[ "${OP_GITHUB_REF}" != "${DEFAULT_OP_GITHUB_REF}" ]]; then
    if github_token="$(read_op_secret "${OP_GITHUB_REF}" "GitHub token")"; then
      github_token="${github_token//$'\r'/}"
      github_token="${github_token//$'\n'/}"
      if [[ -n "${github_token}" ]]; then
        token_tmp="${TOKEN_FILE}.tmp"
        (umask 077; printf 'GITHUB_TOKEN=%s\n' "${github_token}" >"${token_tmp}")
        chmod 600 "${token_tmp}"
        mv "${token_tmp}" "${TOKEN_FILE}"
      else
        echo "BranchBox warning: GitHub token from ${OP_GITHUB_REF} was empty; keeping existing token file." >&2
      fi
    fi
  else
    echo "BranchBox: OP_GITHUB_REF not set; keeping existing token file."
  fi

  if [[ "${OP_SIGNING_KEY_REF}" != "${DEFAULT_OP_SIGNING_KEY_REF}" ]]; then
    if signing_key="$(read_op_secret "${OP_SIGNING_KEY_REF}" "signing key")"; then
      if [[ -n "${signing_key//[$'\t\r\n ']/}" ]]; then
        signing_tmp="${SIGNING_KEY_FILE}.tmp"
        (umask 077; printf '%s\n' "${signing_key}" >"${signing_tmp}")
        chmod 600 "${signing_tmp}"
        mv "${signing_tmp}" "${SIGNING_KEY_FILE}"
      else
        echo "BranchBox warning: signing key from ${OP_SIGNING_KEY_REF} was empty; keeping existing key file." >&2
      fi
    fi
  else
    echo "BranchBox: OP_SIGNING_KEY_REF not set; keeping existing signing key file."
  fi
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
git_config_tmp="${GIT_CONFIG_FILE}.tmp"
(
  umask 077
  {
    if [[ -n "${git_user_name}" ]]; then
      printf 'GIT_USER_NAME=%s\n' "${git_user_name}"
    fi
    if [[ -n "${git_user_email}" ]]; then
      printf 'GIT_USER_EMAIL=%s\n' "${git_user_email}"
    fi
  } >"${git_config_tmp}"
)
chmod 600 "${git_config_tmp}"
mv "${git_config_tmp}" "${GIT_CONFIG_FILE}"

echo "BranchBox: host credential refresh complete."
