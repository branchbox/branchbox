---
sidebar_position: 2
---

# How It Works

A high-level overview of what happens when you run BranchBox commands.

## The Core Idea: Git Worktrees

BranchBox uses [Git worktrees](https://git-scm.com/docs/git-worktree) — a built-in Git feature that lets you have multiple working directories from the same repository.

```
your-project/           # Main worktree (main branch)
├── .git/
├── src/
└── ...

../add-oauth/           # Feature worktree (feature/add-oauth branch)
├── src/                # Same repo, different branch, different folder
└── ...

../fix-bug/             # Another feature worktree
├── src/
└── ...
```

Each worktree is a full working copy. You can run different branches simultaneously without switching.

## What `branchbox feature start` Does

When you run:

```bash
branchbox feature start "Add OAuth"
```

BranchBox:

1. **Creates a git worktree** at `../add-oauth/`
2. **Creates a branch** `feature/add-oauth` from your current HEAD
3. **Detects your stack** (Rails, Node.js, Rust, or Generic)
4. **Runs modules** based on what it detects:
   - **Devcontainer**: Syncs `.devcontainer/` to the new worktree
   - **Compose**: Sets `COMPOSE_PROJECT_NAME` for Docker isolation
   - **Database**: Initializes an isolated database
   - **Tunnel**: Provisions a Cloudflare tunnel (if available)
   - **Specs**: Moves feature spec from `backlog/` to `in-progress/`
5. **Copies your `.env`** with updated values (`APP_URL`, `COMPOSE_PROJECT_NAME`)
6. **Prepares the selected workspace runtime** (the existing container workflow by default)
   - With `sbx`, creates a project-scoped sandbox, publishes `forwardPorts`, and runs
     `devcontainer up` inside the sandbox's Docker daemon
   - Bridges each published VM port to the matching nested Compose service; occupied host ports
     are replaced with available ports and the resolved mappings are recorded in the registry
7. **Registers the feature** in `.branchbox/registry.json`

## Runtime Providers

Runtime providers describe the outer execution boundary for a workspace. This is separate from a
devcontainer, which describes the tools and services inside the developer environment.

```bash
# Existing behavior; this remains the default and requires no Docker Sandboxes account
branchbox feature start add-oauth --runtime container

# Experimental Docker Sandboxes microVM boundary
branchbox feature start add-oauth --runtime sbx

# Account-free Firecracker microVM boundary (x86_64 Linux/KVM)
branchbox feature start add-oauth --runtime local-vm
```

The `sbx` provider detects the Docker Sandboxes CLI and authentication before changing the
repository. If SBX is unavailable, only the explicitly selected SBX start fails; normal BranchBox
workflows remain account-free. The `local-vm` provider is also account-free, but intentionally
requires an x86_64 Linux host with writable `/dev/kvm`, passwordless `sudo` for narrowly scoped
jailer/TAP lifecycle commands, Firecracker and jailer, and approved guest artifacts.

SBX startup failures are sanitized at the runtime boundary shared by text, JSON, agent, CI, and
telemetry callers. BranchBox drops rendered Compose configuration, redacts environment-shaped
assignments and values, and returns a bounded actionable tail plus the exit status. Detailed output
stays in the sandbox-local devcontainer logs. Rotate any credential that may have appeared in a log
created by an older BranchBox version.

Tunnel inputs are also a startup preflight. If the project's Compose configuration requires
`.devcontainer/.cloudflared.env`, BranchBox writes the feature-specific file before creating the SBX
sandbox. Without configured Cloudflare credentials, startup stops before the sandbox or
devcontainer build begins and reports the supported credential setup options.

Commands can be routed through the runtime recorded for an active feature. This is also how an
SBX-backed coding agent is kept inside the sandbox boundary:

```bash
branchbox feature exec add-oauth -- codex

# Capture stdout, stderr, and the exit code for automation
branchbox feature exec add-oauth --json -- codex --version
```

Before each SBX exec, BranchBox resumes the sandbox, reruns idempotent `devcontainer up`, and
refreshes port proxies from the current container ID. The command then runs as the configured
devcontainer user and working directory through that user's login shell, restoring PATH changes
from Mise, asdf, nvm, and similar toolchain managers. `feature list` reports a stopped inner
devcontainer as `degraded` instead of claiming it is fully active.

For diagnostic retries, retain a failed sandbox and its nested Docker cache explicitly:

```bash
branchbox feature start add-oauth --runtime sbx --keep-runtime-on-failure
branchbox feature start add-oauth --runtime sbx --reuse-runtime
branchbox feature teardown add-oauth --force
```

The registry distinguishes `failed_retained`, `degraded`, and `orphaned` runtimes. Default failure
cleanup remains unchanged, while teardown and prune include retained/orphaned entries.

When `BRANCHBOX_DEFAULT_AGENT_CMD` is configured, the automatic agent launch uses the same runtime
route. Container-backed features retain the existing local execution behavior.

Projects can set a default in `.branchbox/config.json`:

```json
{
  "runtime": {
    "provider": "sbx",
    "sbx": {
      "run_services": ["app"]
    }
  }
}
```

`runtime.sbx.run_services` is an explicit SBX-only opt-out for unrelated host-integrated Compose
sidecars. BranchBox generates an ignored runtime config containing devcontainers' `runServices`;
Compose still starts dependencies declared by the selected primary service, such as Postgres and
Redis. If a service requests `/dev/net/tun`, SBX preflight either confirms it is excluded or stops
before sandbox creation with an actionable error. BranchBox never silently rewrites the source
Compose file.

### Account-free Firecracker local VM

`local-vm` implements the same `RuntimeProvider` lifecycle without Docker SBX or a hosted account:

```text
BranchBox -> branchbox-local-vm -> jailer -> Firecracker VM
                                          -> guest Docker Engine
                                          -> devcontainer / Compose stack
```

Phase 1 supports x86_64 Linux/KVM. Install matching Firecracker and jailer binaries (the CI and
image manifest pin v1.16.1), then build the versioned guest artifacts:

```bash
scripts/local-vm/build-image.sh
sudo install -d -m 0755 /var/lib/branchbox/local-vm/images/current
sudo install -m 0644 target/local-vm-image/{vmlinux,kernel.config,rootfs.ext4,manifest.json} \
  /var/lib/branchbox/local-vm/images/current/
branchbox feature start add-oauth --runtime local-vm
```

The build recipe pins the kernel, Firecracker guest configuration, Ubuntu base, and devcontainer
CLI. `manifest.json` records kernel/rootfs SHA-256 digests plus the kernel-config digest; startup
fails closed unless the config matches BranchBox's complete fixed requirement list. The portable
manifest advertises versioned virtio-vsock and legacy IPv4 xtables capabilities without embedding a
consumer's provider, domain, or firewall-chain policy. After the lifecycle proof passes, CI publishes
`vmlinux`, `kernel.config`, `rootfs.tar.gz`, and `manifest.json` together. Immediately before upload,
it re-verifies the kernel, kernel-config, and rootfs-archive digests plus the complete fixed kernel
requirement list. The runtime attaches one Firecracker vsock device for trusted
guest-supervisor-to-host transfers. Its UDS is never passed into the coding
devcontainer, which also cannot access `/dev/vsock` or either Docker control plane. The registry/JSON
API reports the kernel/rootfs digests with the Firecracker version.

Each feature gets a disposable writable rootfs, one-time SSH key, TAP subnet, NAT policy, and host
port proxies. The project directory is synchronized to the same absolute guest path before runtime
commands and back afterward, preserving Git worktree changes without mounting host storage. Guest
egress may reach the public network, but guest-initiated access to host, RFC1918, link-local, and
metadata networks is rejected. The host Docker socket and shared `~/.codex`, `~/.claude`, and
`~/.gh` directories are never mounted.

Callers can explicitly inject scoped, disposable files by setting
`BRANCHBOX_LOCAL_VM_INJECT_DIR`; they appear at `/run/branchbox/credentials` with owner-only
permissions and are removed with the VM. This is an injection seam, not an ambient credential
binding. `BRANCHBOX_LOCAL_VM_STATE_DIR`, `BRANCHBOX_LOCAL_VM_IMAGE_DIR`,
`BRANCHBOX_LOCAL_VM_JAIL_DIR`, `BRANCHBOX_LOCAL_VM_VCPUS`, and
`BRANCHBOX_LOCAL_VM_MEMORY_MIB` provide host-policy overrides. Image builders can set
`BRANCHBOX_LOCAL_VM_ROOTFS_IMAGE` to keep the temporary Docker build tag in a caller-owned
namespace. A root-owned supervisor must set `BRANCHBOX_LOCAL_VM_JAIL_UID` and
`BRANCHBOX_LOCAL_VM_JAIL_GID` to a dedicated non-root execution identity; root is never accepted as
the Firecracker process identity.

Run `scripts/manual-local-vm-e2e.sh` on a KVM host to prove single-container and concurrent
app/Postgres/Redis workspaces, captured and interactive execution, port publication, network/socket
isolation, the versioned legacy IPv4 xtables rules in a disposable trusted network namespace,
immutable artifact reporting, and orphan-free teardown. This does not grant `NET_ADMIN` to a coding
devcontainer; it proves only that a trusted outer guest process can apply policy supplied by its
runtime owner.

## Stack Detection

BranchBox automatically detects your project type:

| Stack | Detection |
|-------|-----------|
| **Rails** | `Gemfile` with `rails` gem |
| **Node.js** | `package.json` present |
| **Rust** | `Cargo.toml` present |
| **Generic** | Fallback for everything else |

Each stack has an **adapter** that knows how to:
- Copy the right secrets (`.env`, `credentials.yml`, etc.)
- Set the correct service URL
- Run stack-specific setup

## Module System

Modules are composable features that run during the worktree lifecycle:

| Module | Purpose | When It Runs |
|--------|---------|--------------|
| **Devcontainer** | Sync `.devcontainer/` config | Always (if `.devcontainer/` exists) |
| **Compose** | Isolate Docker Compose project | Always (if `compose.yaml` exists) |
| **Database** | Isolate database | If `database.yml` or similar detected |
| **Tunnel** | Provision Cloudflare tunnel | Always (manual fallback if no cloudflared) |
| **Specs** | Feature spec lifecycle | If `docs/features/` exists |

Modules run in dependency order. You can skip any module with `--skip-module`.

## The Registry

BranchBox tracks features in `.branchbox/registry.json`:

```json
{
  "features": [
    {
      "work_feature": "add-oauth",
      "branch_name": "feature/add-oauth",
      "worktree_path": "/path/to/add-oauth",
      "status": "Active",
      "start_mode": "full",
      "runtime": {
        "provider": "container"
      },
      "created_at": "2025-01-07T10:30:00Z",
      "updated_at": "2025-01-07T10:30:00Z"
    }
  ]
}
```

This is how `branchbox feature list` knows what's running.

## What `branchbox feature teardown` Does

When you run:

```bash
branchbox feature teardown add-oauth
```

BranchBox:

1. **Runs module teardown** (in reverse order)
   - Specs: Optionally moves spec to `completed/`
   - Tunnel: Removes tunnel configuration
   - Compose: Discovers the worktree's actual devcontainer Compose project, then removes its containers, networks, and volumes
   - Devcontainer: (No-op)

:::note[Database Persistence]
The database module does **not** automatically delete databases on teardown. Feature databases persist to prevent accidental data loss. To clean them up, manually drop the database or use your database admin tools.
:::
2. **Removes the git worktree** at `../add-oauth/`
3. **Optionally deletes the branch** (prompts if unmerged)
4. **Updates the registry** to mark feature as `Removed`

## Isolation Boundaries

| Resource | How It's Isolated |
|----------|-------------------|
| **Git branch** | Each feature = own branch |
| **Working directory** | Each feature = own folder (`../feature-name/`) |
| **Docker network** | `COMPOSE_PROJECT_NAME` creates separate network |
| **Docker containers** | Compose project name prefixes all containers |
| **Ports** | Each Compose project gets its own port mappings |
| **Environment** | `.env` copied and customized per feature |
| **Database** | Database module creates feature-specific DB |

During teardown, BranchBox matches the exact `devcontainer.local_folder` Docker label before acting
on a devcontainer CLI project. It also restores the BranchBox-managed project name from
`.devcontainer/.branchbox.env`. Teardown verifies that no containers, networks, or volumes with
either owned project label remain before removing the worktree.

---

**Deep Dive:** For architecture details (SQLite schemas, gRPC protocols, module implementation), see [Internals](internals/architecture.md).
