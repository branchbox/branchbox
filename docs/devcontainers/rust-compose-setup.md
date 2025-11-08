# 🧰 Rust DevContainer Stability & Performance Guidelines

## Goal
Prevent system freezes and container crashes when running multiple Rust devcontainers concurrently by capping resource usage, optimizing Cargo builds, and sharing caches.

---

## 1. Compose Setup: Add Resource Limits and Shared Caches

The `.devcontainer/compose.yaml` file includes:

```yaml
# Shared Rust caches for all worktree containers
x-rust-caches: &rust_caches
  volumes:
    - rust-cargo-registry:/usr/local/cargo/registry
    - rust-cargo-git:/usr/local/cargo/git
    - rust-sccache-cache:/home/vscode/.cache/sccache

# Resource limits to prevent system freezes with multiple containers
x-rust-limits: &rust_limits
  cpus: '2'                # Hard cap per container
  mem_limit: 6g
  memswap_limit: 8g
  pids_limit: 512
```

**✅ Why:**
- Enforces CPU/RAM caps per container
- Shares cargo/git/sccache caches across all containers
- Prevents any single container from consuming all system resources

---

## 2. DevContainer Configuration

The `.devcontainer/devcontainer.json` references the Compose service:

```json
{
  "name": "Worktree Manager",
  "dockerComposeFile": "compose.yaml",
  "service": "rust-dev",
  "workspaceFolder": "/workspaces/${localWorkspaceFolderBasename}"
}
```

When using multiple worktrees, each devcontainer mounts its own folder but shares caches and respects the same resource limits.

---

## 3. Dockerfile Enhancements

The `.devcontainer/Dockerfile` includes:

```dockerfile
# Install clang and lld for faster linking
RUN apt-get update && apt-get -y install --no-install-recommends \
    clang lld \
    && rm -rf /var/lib/apt/lists/*

# Install sccache for shared compilation cache
RUN cargo install sccache

# Set up environment
ENV RUSTC_WRAPPER=/usr/local/cargo/bin/sccache
ENV SCCACHE_CACHE_SIZE=15G
```

**✅ Why:**
- `lld` provides faster linking than the default linker
- `sccache` caches compilation artifacts across containers
- Dramatically reduces rebuild times when switching between worktrees

---

## 4. Cargo Optimization

The `.cargo/config.toml` file includes:

```toml
[build]
# Limit parallel jobs to reduce memory usage per build
jobs = 2

# Use lld for faster linking
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[profile.dev]
# Reduce debug info to save memory and build time
debug = 1
```

**✅ Why:**
- Limits concurrent compilation jobs to reduce memory pressure
- Uses the faster lld linker
- Reduces debug info size while maintaining useful stack traces

---

## 5. Optional: Serialize Heavy Builds

To prevent simultaneous heavy compiles, use the `scripts/cargo-queued.sh` script:

```bash
#!/usr/bin/env bash
# Usage: ./scripts/cargo-queued.sh build

set -euo pipefail
LOCK="${CARGO_QUEUE_LOCK:-/tmp/cargo-build.lock}"
exec 9>"$LOCK"
flock -w 900 9
exec cargo "$@"
```

**Usage:**
```bash
./scripts/cargo-queued.sh build
./scripts/cargo-queued.sh test --all-features
```

This ensures only one heavy build runs at a time across all worktrees.

---

## 6. Docker Desktop (macOS/Windows)

**Recommended settings:**
- Allocate 70-75% of total RAM to Docker
- Ensure swap is enabled for better stability
- Leave at least 25-30% of host RAM free

**Example for a 16GB system:**
- Docker: 12GB RAM
- Host: 4GB free
- Swap: 4GB

---

## 7. Verification Checklist

After applying these changes:

1. **Start multiple containers:**
   ```bash
   # In main repo
   code .

   # In feature worktree 1
   cd ../feature-worktree-1
   code .

   # In feature worktree 2
   cd ../feature-worktree-2
   code .
   ```

2. **Monitor resource usage:**
   ```bash
   docker stats
   ```

3. **Run builds in parallel:**
   ```bash
   # In each container
   cargo build -j 2
   ```

**✅ Expected results:**
- Each container stays ≤2 CPUs and ≤6-8 GB RAM
- Host remains responsive
- Warm builds reuse caches (sccache shows cache hits)
- No file-lock errors in `target/`
- No container OOM kills or VS Code disconnects

---

## 8. Recommended Defaults

| Setting | Value | Purpose |
|---------|-------|---------|
| CPU cap | 2 | Fair resource split across containers |
| Memory cap | 6 GB | Avoid host OOM |
| Swap | 8 GB | Handle short memory spikes |
| Cargo jobs | 2 | Limit compilation parallelism |
| Linker | lld | Faster & lighter than default ld |
| Cache volumes | Shared | Reuse build artifacts across worktrees |

---

## 9. Acceptance Criteria

**Success indicators:**
1. ✅ Running 4 devcontainers in parallel no longer freezes the host
2. ✅ No container OOM kills or VS Code disconnects
3. ✅ Warm builds complete significantly faster (sccache hits)
4. ✅ No concurrent `target/` corruption or lock contention
5. ✅ System remains responsive during heavy compilation

**Troubleshooting:**
- If containers still freeze: reduce `cpus` to 1 and `mem_limit` to 4g
- If builds are too slow: increase `jobs` to 4 (but monitor memory)
- If cache misses persist: check sccache stats with `sccache --show-stats`

---

## 10. Optional Improvements

**For even better performance:**

1. **Dedicated builder service:**
   ```yaml
   services:
     builder:
       <<: *rust_limits
       cpus: '4'
       mem_limit: 12g
   ```
   Trigger all builds in this service with higher resource allocation.

2. **BuildKit cache mounts (CI):**
   ```dockerfile
   RUN --mount=type=cache,target=/usr/local/cargo/registry \
       --mount=type=cache,target=/usr/local/cargo/git \
       cargo build --release
   ```

3. **Per-machine tuning:**
   - 2 CPUs × 4 containers = 8 CPU system minimum
   - 6 GB × 4 containers = 24 GB RAM minimum
   - Adjust limits based on your hardware

4. **Monitoring:**
   ```bash
   # Watch cache efficiency
   sccache --show-stats

   # Monitor container resources
   docker stats --format "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}"
   ```

---

## Outcome

Developers can safely run multiple Rust devcontainers concurrently with:
- ✅ Controlled resource usage
- ✅ Stable VS Code sessions
- ✅ Faster rebuild times via shared caches
- ✅ No system freezes or OOM kills
- ✅ Predictable, reproducible build behavior

---

## Related Documentation

- [DevContainer Module](../features/in-progress/devcontainer-module.md)
- [Worktree Workflow](../docs/worktrees.md)
- [BranchBox CLI Reference](../docs/reference/cli.md)
