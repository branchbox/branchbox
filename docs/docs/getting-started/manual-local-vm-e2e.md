# Manual local-vm Firecracker E2E

Run this harness on an x86_64 Linux host with KVM. It is the release proof for the account-free
`local-vm` provider.

## Prerequisites

- readable/writable `/dev/kvm`;
- Firecracker and jailer v1.16.1 on `PATH`;
- passwordless `sudo` for jailer, loop mounts, TAP, and iptables lifecycle;
- Docker, Rust, `jq`, `rsync`, `socat`, `e2fsprogs`, OpenSSH, and standard Linux networking tools;
- built and installed BranchBox guest artifacts.

```bash
scripts/local-vm/build-image.sh
sudo install -d -m 0755 /var/lib/branchbox/local-vm/images/current
sudo install -m 0644 target/local-vm-image/{vmlinux,rootfs.ext4,manifest.json} \
  /var/lib/branchbox/local-vm/images/current/
scripts/manual-local-vm-e2e.sh
```

## What it proves

1. A fresh single-container devcontainer starts inside Firecracker.
2. A configured coding-agent stub runs through `RuntimeProvider::exec_interactive` and its file
   changes synchronize back to the host worktree.
3. Captured `feature exec --json` returns the guest command's actual output and status.
4. Two feature workspaces run concurrently without VM, TAP, Docker network, database, or host-port
   collisions.
5. Each multi-service workspace starts app, Postgres, and Redis in its own guest Docker daemon, and
   the app can reach both guest-local dependencies.
6. Published application ports reach the correct nested devcontainer through guest and host proxy
   layers.
7. The devcontainer cannot see a Docker socket and cannot initiate a connection to the host gateway.
8. Runtime JSON reports Firecracker, kernel SHA-256, and rootfs SHA-256 identity.
9. Teardown removes Firecracker processes, TAP interfaces, proxy processes, keys, writable disks,
   and driver state.

The GitHub workflow `.github/workflows/local-vm-e2e.yml` runs this same harness on an x64 Ubuntu KVM
runner whenever local-vm implementation/image paths change and can also be dispatched manually.
