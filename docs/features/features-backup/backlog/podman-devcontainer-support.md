---
status: backlog
created: 2025-10-23
tags:
  - devcontainer
  - podman
  - docker-compat
---

# Podman-Compatible Devcontainer & Runtime Support

## Overview

Investigate what it would take for branchbox to run confidently on Podman-based
hosts (desktop and CI) while preserving the current developer experience. Today
we depend on Docker-specific tooling:

- Devcontainer builds install the Docker engine via
  `ghcr.io/devcontainers/features/docker-in-docker:2`.
- Runtime mounts assume Docker layout (e.g. `/var/lib/docker`).
- Integration tests start a Docker DinD service.
- The compose module shells out to the Docker CLI (`docker compose`).

Goal: produce a compatibility plan that either adds a Podman mode or documents
why the migration is impractical. Deliverables should include code changes,
feature toggles, and documentation updates.

## Key Questions

1. **CLI Compatibility** – Can we rely on the `podman-docker` shim or do we need
   native `podman`/`podman compose` calls? The compose module currently executes
   `docker compose …` with v2 flags.
2. **Devcontainer Feature** – Does a Podman equivalent to
   `docker-in-docker:2` exist, or do we need a custom feature that installs and
   configures Podman inside the container?
3. **Volume Layout** – What path mapping is required (`/var/lib/containers`
   instead of `/var/lib/docker`)? How does that interact with our host bind
   mount strategy and privilege requirements?
4. **CI Coverage** – Is there a hosted Podman DinD image we can substitute in
   GitHub Actions? How different are the APIs and lifecycle hooks?
5. **Rootless vs Privileged** – Podman encourages rootless operation. Do we gain
   enough isolation benefits to drop `--privileged`, or will we still need it
   for nested container builds?
6. **Tooling Impact** – How does Podman interact with Codex/Node/npm features or
   our future Docker-based modules (tunnels, databases)?

## Proposed Work Packages

### 1. Devcontainer Runtime

- Replace `docker-in-docker:2` with a Podman-focused feature (official or
  custom) that:
  - Installs Podman CLI/engine inside the container.
  - Exposes a socket path compatible with the CLI shim.
  - Configures `/var/lib/containers` persistence via named volume.
- Update `.devcontainer/compose.yaml` (and templates) to mount Podman data dirs
  and remove Docker-specific comments.
- Decide whether to keep the host `.codex` bind mount untouched (should be
  unaffected).

### 2. Compose Module Compatibility

- Detect Podman environments (env vars? presence of `/usr/bin/podman`?).
- Option A: require `podman-docker` so the existing `docker` invocations work.
- Option B: Introduce an abstraction in `core/src/modules/compose.rs` that
  chooses between Docker and Podman commands.
- Ensure label filtering (`--filter label=com.docker.compose.project`) has a
  Podman equivalent or provide alternate implementation.

### 3. CI Pipeline

- Swap the `docker:24-dind` service for a Podman alternative.
- Update CI steps to export `DOCKER_HOST` equivalent (likely
  `unix:///var/run/podman/podman.sock` or TCP).
- Validate integration tests that rely on `docker compose` still work (may need
  to install `podman-compose`).

### 4. Bootstrap Templates & Docs

- Mirror runtime changes into `core/src/bootstrap/templates/**` files.
- Document Podman prerequisites and limitations in README + ARCHITECTURE.
- Provide migration guidance in the new feature doc once results are known.

## Open Questions / Risks

- **Compose Parity:** Podman’s compose support is evolving. Are we comfortable
  pinning to `podman compose` or an external Python `podman-compose` package?
- **Performance & Stability:** Nested container builds (DinD) can be slower or
  require extra flags in Podman rootless mode.
- **Platform Coverage:** Do macOS and Windows users have a straightforward
  Podman install path that plays nicely with devcontainers?
- **Tooling Ecosystem:** Devcontainers spec primarily targets Docker; we may hit
  undocumented edge cases.

## Definition of Done

- Prototype devcontainer build using Podman completes without manual tweaks.
- Compose module passes integration tests when `docker` commands are routed to
  Podman (either via shim or native calls).
- GitHub Actions integration tests run successfully on a Podman service.
- Documentation reflects dual-runtime support or clearly states the chosen
  direction.
- Retro notes filed if we decide against Podman support, including blockers and
  fallback guidance.

## Next Steps

1. Research available Podman devcontainer features / craft custom install.
2. Spike: run a single devcontainer build with Podman support and capture logs.
3. Audit compose module command usage to estimate refactor size.
4. Document findings and recommendations back in this feature file.
