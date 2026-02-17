#!/usr/bin/env bash
set -euo pipefail

warn() {
  echo "BranchBox warning: $*" >&2
}

HOME_DIR="${HOME:-/home/vscode}"
TOKEN_FILE="${HOME_DIR}/.github-token.env"
SIGNING_KEY_SRC="${HOME_DIR}/.git-signing-key"
SIGNING_KEY_DST="${HOME_DIR}/.ssh/branchbox-signing-key"
GITCONFIG_FILE="${HOME_DIR}/.gitconfig.env"
WORKSPACE_DIR="${WORKSPACE_FOLDER:-$(pwd)}"
SHARED_TOKEN_FILE="${HOME_DIR}/.github-token.env"
SHARED_SIGNING_KEY_FILE="${HOME_DIR}/.git-signing-key"
SHARED_GITCONFIG_FILE="${HOME_DIR}/.gitconfig.env"
LEGACY_SHARED_HOME="/home/vscode"

if [[ "${HOME_DIR}" != "${LEGACY_SHARED_HOME}" ]]; then
  if [[ ! -s "${SHARED_TOKEN_FILE}" && -s "${LEGACY_SHARED_HOME}/.github-token.env" ]]; then
    SHARED_TOKEN_FILE="${LEGACY_SHARED_HOME}/.github-token.env"
  fi
  if [[ ! -s "${SHARED_SIGNING_KEY_FILE}" && -s "${LEGACY_SHARED_HOME}/.git-signing-key" ]]; then
    SHARED_SIGNING_KEY_FILE="${LEGACY_SHARED_HOME}/.git-signing-key"
  fi
  if [[ ! -s "${SHARED_GITCONFIG_FILE}" && -s "${LEGACY_SHARED_HOME}/.gitconfig.env" ]]; then
    SHARED_GITCONFIG_FILE="${LEGACY_SHARED_HOME}/.gitconfig.env"
  fi
fi

if [[ ! -s "${TOKEN_FILE}" && -s "${SHARED_TOKEN_FILE}" ]]; then
  TOKEN_FILE="${SHARED_TOKEN_FILE}"
fi
if [[ ! -s "${SIGNING_KEY_SRC}" && -s "${SHARED_SIGNING_KEY_FILE}" ]]; then
  SIGNING_KEY_SRC="${SHARED_SIGNING_KEY_FILE}"
fi
if [[ ! -s "${GITCONFIG_FILE}" && -s "${SHARED_GITCONFIG_FILE}" ]]; then
  GITCONFIG_FILE="${SHARED_GITCONFIG_FILE}"
fi

REPO_ROOT="${WORKSPACE_DIR}"
if git_root="$(git -C "${WORKSPACE_DIR}" rev-parse --show-toplevel 2>/dev/null)"; then
  REPO_ROOT="${git_root}"
fi

if [[ -s "${TOKEN_FILE}" ]]; then
  github_token="$(grep '^GITHUB_TOKEN=' "${TOKEN_FILE}" | cut -d= -f2- || true)"
  github_token="${github_token//$'\r'/}"
  github_token="${github_token//$'\n'/}"
  if [[ -n "${github_token}" ]]; then
    export GH_TOKEN="${github_token}"
    credential_store_file="${HOME_DIR}/.git-credentials-branchbox"
    printf -v credential_helper 'store --file %q' "${credential_store_file}"
    git config --global credential.https://github.com.helper \
      "${credential_helper}"
    printf 'protocol=https\nhost=github.com\nusername=oauth2\npassword=%s\n\n' "${github_token}" \
      | git credential approve
    chmod 600 "${credential_store_file}" 2>/dev/null || true

    if ! grep -q "# BranchBox GH token" "${HOME_DIR}/.bashrc" 2>/dev/null; then
      cat >>"${HOME_DIR}/.bashrc" <<EOF

# BranchBox GH token
if [ -f "${TOKEN_FILE}" ]; then
  export GH_TOKEN="\$(grep '^GITHUB_TOKEN=' \"${TOKEN_FILE}\" | cut -d= -f2-)"
fi
EOF
    fi

    remote_url="$(git -C "${REPO_ROOT}" remote get-url origin 2>/dev/null || true)"
    if [[ "${remote_url}" == git@github.com:* ]]; then
      https_url="${remote_url/git@github.com:/https://github.com/}"
      git -C "${REPO_ROOT}" remote set-url origin "${https_url}"
      echo "BranchBox: switched origin remote to HTTPS (${https_url})."
    elif [[ "${remote_url}" == ssh://git@github.com/* ]]; then
      https_url="${remote_url/ssh:\/\/git@github.com\//https://github.com/}"
      git -C "${REPO_ROOT}" remote set-url origin "${https_url}"
      echo "BranchBox: switched origin remote to HTTPS (${https_url})."
    fi
  fi
fi

if [[ -s "${GITCONFIG_FILE}" ]]; then
  git_user_name="$(grep '^GIT_USER_NAME=' "${GITCONFIG_FILE}" | cut -d= -f2- || true)"
  git_user_email="$(grep '^GIT_USER_EMAIL=' "${GITCONFIG_FILE}" | cut -d= -f2- || true)"
  if [[ -n "${git_user_name}" ]]; then
    git config --global user.name "${git_user_name}"
  fi
  if [[ -n "${git_user_email}" ]]; then
    git config --global user.email "${git_user_email}"
  fi
fi

if [[ -s "${SIGNING_KEY_SRC}" ]] && head -n 1 "${SIGNING_KEY_SRC}" | grep -q '^-----BEGIN '; then
  mkdir -p "${HOME_DIR}/.ssh"
  chmod 700 "${HOME_DIR}/.ssh"
  cp "${SIGNING_KEY_SRC}" "${SIGNING_KEY_DST}"
  chmod 600 "${SIGNING_KEY_DST}"

  if ssh-keygen -y -f "${SIGNING_KEY_DST}" >"${SIGNING_KEY_DST}.pub" 2>/dev/null; then
    git config --global gpg.format ssh
    git config --global user.signingkey "${SIGNING_KEY_DST}"
    git config --global commit.gpgsign true
    git config --global tag.gpgsign true

    git_email="$(git config --global user.email 2>/dev/null || true)"
    if [[ -n "${git_email}" ]]; then
      allowed_signers_file="${HOME_DIR}/.ssh/allowed_signers"
      tmp_allowed_signers="${allowed_signers_file}.tmp"
      signer_key="$(cat "${SIGNING_KEY_DST}.pub")"
      if [[ -f "${allowed_signers_file}" && -s "${allowed_signers_file}" ]]; then
        awk -v email="${git_email}" '$1 != email' "${allowed_signers_file}" >"${tmp_allowed_signers}"
      else
        : >"${tmp_allowed_signers}"
      fi
      printf '%s %s\n' "${git_email}" "${signer_key}" >>"${tmp_allowed_signers}"
      mv "${tmp_allowed_signers}" "${allowed_signers_file}"
      chmod 600 "${allowed_signers_file}" 2>/dev/null || true
      git config --global gpg.ssh.allowedSignersFile "${allowed_signers_file}"
    fi
  else
    warn "Signing key at ${SIGNING_KEY_SRC} is not a valid SSH private key."
  fi
fi
