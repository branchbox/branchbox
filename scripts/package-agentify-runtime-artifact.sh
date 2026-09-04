#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: package-agentify-runtime-artifact.sh BINARY OUTPUT_DIR SOURCE_COMMIT" >&2
}

[[ "$#" == 3 ]] || { usage; exit 2; }

readonly binary_argument="$1"
readonly output_dir="$2"
readonly source_commit="$3"
readonly target="x86_64-unknown-linux-gnu"
readonly artifact_base="branchbox-agentify-runtime-${source_commit}-${target}"
readonly script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly repository_root="$(cd "${script_dir}/.." && pwd)"

[[ "${source_commit}" =~ ^[0-9a-f]{40}$ ]] || {
  echo "SOURCE_COMMIT must be an exact lowercase 40-character Git commit." >&2
  exit 1
}
[[ -f "${binary_argument}" && ! -L "${binary_argument}" && -x "${binary_argument}" ]] || {
  echo "Branchbox input must be a regular executable file, not a symlink." >&2
  exit 1
}
readonly binary_source="$(cd "$(dirname "${binary_argument}")" && pwd)/$(basename "${binary_argument}")"
[[ "$(git -C "${repository_root}" rev-parse HEAD)" == "${source_commit}" ]] || {
  echo "SOURCE_COMMIT does not match the checked-out Branchbox commit." >&2
  exit 1
}
[[ "$(od -An -tx1 -N6 "${binary_source}" | tr -d ' \n')" == 7f454c460201 ]] || {
  echo "Branchbox input is not a 64-bit little-endian ELF executable." >&2
  exit 1
}
[[ "$(od -An -tx1 -j18 -N2 "${binary_source}" | tr -d ' \n')" == 3e00 ]] || {
  echo "Branchbox input is not an x86_64 executable." >&2
  exit 1
}
for command in gzip jq readelf sed sha256sum sort stat tar; do
  command -v "${command}" >/dev/null || {
    echo "Required packaging command is unavailable: ${command}" >&2
    exit 1
  }
done

mkdir -p "${output_dir}"
[[ -d "${output_dir}" && ! -L "${output_dir}" ]] || {
  echo "OUTPUT_DIR must be a real directory, not a symlink." >&2
  exit 1
}
readonly archive_path="${output_dir}/${artifact_base}.tar.gz"
readonly archive_checksum_path="${archive_path}.sha256"
[[ ! -e "${archive_path}" && ! -e "${archive_checksum_path}" ]] || {
  echo "Refusing to replace an existing Agentify runtime artifact." >&2
  exit 1
}

staging_dir="$(mktemp -d)"
trap 'rm -rf -- "${staging_dir}"' EXIT
install -m 0755 "${binary_source}" "${staging_dir}/branchbox"

readonly binary_sha256="$(sha256sum "${staging_dir}/branchbox" | awk '{print $1}')"
readonly binary_size="$(stat -c '%s' "${staging_dir}/branchbox")"
readonly cargo_lock_sha256="$(sha256sum "${repository_root}/Cargo.lock" | awk '{print $1}')"
readonly minimum_glibc_version="$(
  readelf --version-info "${staging_dir}/branchbox" |
    grep -oE 'GLIBC_[0-9]+\.[0-9]+' |
    sort -Vu |
    tail -1 |
    sed 's/^GLIBC_//' || true
)"
[[ "${minimum_glibc_version}" =~ ^[0-9]+\.[0-9]+$ ]] || {
  echo "Unable to determine the Branchbox glibc runtime requirement." >&2
  exit 1
}

jq -n \
  --arg schema_version "branchbox.agentify-runtime-artifact/1" \
  --arg source_repository "branchbox/branchbox" \
  --arg source_commit "${source_commit}" \
  --arg target "${target}" \
  --arg cargo_lock_sha256 "${cargo_lock_sha256}" \
  --arg minimum_glibc_version "${minimum_glibc_version}" \
  --arg binary_name "branchbox" \
  --arg binary_sha256 "${binary_sha256}" \
  --argjson binary_size_bytes "${binary_size}" \
  '{
    schema_version: $schema_version,
    source_repository: $source_repository,
    source_commit: $source_commit,
    target: $target,
    cargo_lock_sha256: $cargo_lock_sha256,
    runtime_abi: {
      family: "glibc",
      minimum_version: $minimum_glibc_version
    },
    binary: {
      name: $binary_name,
      sha256: $binary_sha256,
      size_bytes: $binary_size_bytes
    }
  }' > "${staging_dir}/branchbox.manifest.json"
chmod 0444 "${staging_dir}/branchbox.manifest.json"

tar --sort=name --mtime='UTC 2020-01-01' --owner=0 --group=0 --numeric-owner \
  -C "${staging_dir}" -cf - branchbox branchbox.manifest.json |
  gzip -n -9 > "${archive_path}"
printf '%s  %s\n' "$(sha256sum "${archive_path}" | awk '{print $1}')" \
  "$(basename "${archive_path}")" > "${archive_checksum_path}"

echo "Packaged ${archive_path}"
echo "Branchbox SHA-256: ${binary_sha256}"
echo "Minimum glibc: ${minimum_glibc_version}"
