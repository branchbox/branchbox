#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
OUTPUT_DIR="${BRANCHBOX_LOCAL_VM_IMAGE_OUTPUT:-$REPO_ROOT/target/local-vm-image}"
ROOTFS_SIZE_GIB="${BRANCHBOX_LOCAL_VM_ROOTFS_GIB:-12}"
ROOTFS_IMAGE="${BRANCHBOX_LOCAL_VM_ROOTFS_IMAGE:-branchbox-local-vm-rootfs:build}"
KERNEL_REQUIREMENTS="$SCRIPT_DIR/image/kernel-required.config"
BUILD_DIR=$(mktemp -d)
MOUNT_DIR=$(mktemp -d)
ROOTFS_CONTAINER=''

cleanup() {
  if mountpoint -q "$MOUNT_DIR"; then sudo umount "$MOUNT_DIR"; fi
  [[ -n "$ROOTFS_CONTAINER" ]] && docker rm -f "$ROOTFS_CONTAINER" >/dev/null 2>&1 || true
  rmdir "$MOUNT_DIR" 2>/dev/null || true
  find "$BUILD_DIR" -depth -delete 2>/dev/null || true
}
trap cleanup EXIT

for command in docker git gzip jq mkfs.ext4 mountpoint sha256sum stat sudo; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 1; }
done
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || {
  echo 'local-vm images must be built on x86_64 Linux' >&2
  exit 1
}

mkdir -p "$OUTPUT_DIR"
kernel_cache=()
rootfs_cache=()
if [[ "${BRANCHBOX_LOCAL_VM_GHA_CACHE:-}" == 1 ]]; then
  kernel_cache=(--cache-from type=gha,scope=branchbox-local-vm-kernel --cache-to type=gha,scope=branchbox-local-vm-kernel,mode=max)
  rootfs_cache=(--cache-from type=gha,scope=branchbox-local-vm-rootfs --cache-to type=gha,scope=branchbox-local-vm-rootfs,mode=max)
fi
docker buildx build "${kernel_cache[@]}" --file "$SCRIPT_DIR/image/kernel.Dockerfile" \
  --output "type=local,dest=$BUILD_DIR/kernel" "$SCRIPT_DIR/image"
docker buildx build "${rootfs_cache[@]}" --file "$SCRIPT_DIR/image/rootfs.Dockerfile" \
  --tag "$ROOTFS_IMAGE" --load "$SCRIPT_DIR/image"
ROOTFS_CONTAINER=$(docker create "$ROOTFS_IMAGE")
docker export "$ROOTFS_CONTAINER" --output "$BUILD_DIR/rootfs.tar"
gzip -n -9 -c "$BUILD_DIR/rootfs.tar" >"$OUTPUT_DIR/rootfs.tar.gz"

truncate -s "${ROOTFS_SIZE_GIB}G" "$OUTPUT_DIR/rootfs.ext4"
mkfs.ext4 -q -F -L branchbox-rootfs "$OUTPUT_DIR/rootfs.ext4"
sudo mount -o loop "$OUTPUT_DIR/rootfs.ext4" "$MOUNT_DIR"
sudo tar -xf "$BUILD_DIR/rootfs.tar" -C "$MOUNT_DIR"
sudo rm -f "$MOUNT_DIR/etc/resolv.conf"
sudo ln -s /run/systemd/resolve/stub-resolv.conf "$MOUNT_DIR/etc/resolv.conf"
sudo umount "$MOUNT_DIR"
cp "$BUILD_DIR/kernel/vmlinux" "$OUTPUT_DIR/vmlinux"
cp "$BUILD_DIR/kernel/kernel.config" "$OUTPUT_DIR/kernel.config"

[[ -f "$KERNEL_REQUIREMENTS" && ! -L "$KERNEL_REQUIREMENTS" ]] || {
  echo 'local-vm kernel requirement contract is unavailable' >&2
  exit 1
}
while IFS= read -r requirement; do
  grep -Fqx "$requirement" "$OUTPUT_DIR/kernel.config" || {
    echo "built kernel is missing $requirement" >&2
    exit 1
  }
done < "$KERNEL_REQUIREMENTS"
kernel_requirements_json=$(jq -Rsc 'split("\n") | map(select(length > 0))' "$KERNEL_REQUIREMENTS")

kernel_sha=$(sha256sum "$OUTPUT_DIR/vmlinux" | awk '{print $1}')
kernel_config_sha=$(sha256sum "$OUTPUT_DIR/kernel.config" | awk '{print $1}')
rootfs_sha=$(sha256sum "$OUTPUT_DIR/rootfs.ext4" | awk '{print $1}')
rootfs_archive_sha=$(sha256sum "$OUTPUT_DIR/rootfs.tar.gz" | awk '{print $1}')
rootfs_archive_size=$(stat -c '%s' "$OUTPUT_DIR/rootfs.tar.gz")
source_commit=$(git -C "$REPO_ROOT" rev-parse HEAD)
[[ "$source_commit" =~ ^[0-9a-f]{40}$ ]] || {
  echo 'local-vm guest base requires an exact lowercase Git source commit' >&2
  exit 1
}
jq -n \
  --arg format_version '2' \
  --arg schema_version 'branchbox.local-vm-guest-base/2' \
  --arg source_repository 'branchbox/branchbox' \
  --arg source_commit "$source_commit" \
  --arg target 'x86_64-unknown-linux-gnu' \
  --arg base_os 'ubuntu-24.04' \
  --arg firecracker_version 'v1.16.1' \
  --arg kernel_version '6.1.155' \
  --arg devcontainer_cli_version '0.80.3' \
  --arg kernel_sha256 "$kernel_sha" \
  --arg kernel_config_sha256 "$kernel_config_sha" \
  --argjson kernel_requirements "$kernel_requirements_json" \
  --arg rootfs_sha256 "$rootfs_sha" \
  --arg rootfs_archive_sha256 "$rootfs_archive_sha" \
  --argjson rootfs_archive_size_bytes "$rootfs_archive_size" \
  '{
    format_version: $format_version,
    schema_version: $schema_version,
    source_repository: $source_repository,
    source_commit: $source_commit,
    target: $target,
    base_os: $base_os,
    firecracker_version: $firecracker_version,
    kernel_version: $kernel_version,
    devcontainer_cli_version: $devcontainer_cli_version,
    kernel_sha256: $kernel_sha256,
    kernel_config: {
      name: "kernel.config",
      sha256: $kernel_config_sha256,
      required: $kernel_requirements
    },
    capabilities: {
      guest_to_host_vsock: true,
      legacy_ipv4_xtables: "1"
    },
    rootfs_sha256: $rootfs_sha256,
    rootfs_archive: {
      name: "rootfs.tar.gz",
      format: "docker-export-tar",
      compression: "gzip",
      sha256: $rootfs_archive_sha256,
      size_bytes: $rootfs_archive_size_bytes
    }
  }' \
  >"$OUTPUT_DIR/manifest.json"

printf 'Built BranchBox local-vm image in %s\n' "$OUTPUT_DIR"
cat "$OUTPUT_DIR/manifest.json"
