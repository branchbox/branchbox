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
sudo install -m 0644 target/local-vm-image/{vmlinux,kernel.config,rootfs.ext4,manifest.json} \
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
8. The approved kernel config is digest-bound and includes built-in `CONFIG_VSOCKETS` and
   `CONFIG_VIRTIO_VSOCKETS`; a trusted guest process transfers a challenge to a host UDS through
   Firecracker's virtio-vsock mediator.
9. Concurrent VMs safely reuse guest CID `3` and the same probe port because every jailed VM owns a
   distinct UDS namespace; an existing endpoint is never unlinked or replaced.
10. The coding devcontainer cannot access `/dev/vsock`; the transfer endpoint remains an outer guest
   supervisor capability, not a container or Docker control channel.
11. Runtime JSON reports Firecracker, kernel SHA-256, and rootfs SHA-256 identity.
12. Teardown removes Firecracker processes, TAP interfaces, proxy and vsock listener processes,
   Unix sockets, keys, writable disks, and driver state.

The GitHub workflow `.github/workflows/local-vm-e2e.yml` runs this same harness on an x64 Ubuntu KVM
runner whenever local-vm implementation/image paths change and can also be dispatched manually.
After the lifecycle and residue checks pass, it publishes `rootfs.tar.gz` plus `manifest.json` as
`branchbox-agentify-guest-base-<source-commit>`. The manifest binds the archive digest and byte size to
the exact BranchBox commit. This is the only guest-base handoff Agentify may consume; it must remove
the local-VM SSH and sudo identity while installing its own managed runtime boundary.
