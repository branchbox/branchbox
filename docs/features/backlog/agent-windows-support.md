---
status: backlog
priority: medium
tags:
  - agent
  - windows
owner: core-platform
---

# Agent Windows Support

## Problem

BranchBox’s new Rust agent currently compiles and runs only on Unix-like systems (macOS, Linux, devcontainers) because the IPC layer depends on Unix domain sockets (`tokio::net::UnixListener/UnixStream`). The official CI matrix includes Windows builds, and any future attempt to run the daemon on Windows fails with `E0432` (`tokio::net::unix` disabled). Contributors on Windows must fall back to `BRANCHBOX_CLI_DIRECT=1`, so the agent-based workflow has no coverage on that platform.

## Goals

1. **Cross-platform agent**: Ship a transport abstraction so the agent can run on Windows without feature-gating the entire crate.
2. **CLI interoperability**: Automatically detect and talk to the appropriate transport (Unix socket vs. TCP/Named Pipe) so users do not need to flip env vars.
3. **CI coverage**: Ensure `cargo build --all-features` and the Windows job in `.github/workflows/ci.yml` exercise the agent successfully.
4. **Documentation**: Clearly describe which transports are available per OS and how to override addresses/sockets.

## Proposed Approach

### 1. Transport abstraction

- Introduce a `Transport` trait (start/accept/handle) with implementations:
  - `UnixTransport` (existing UnixListener/UnixStream code)
  - `TcpTransport` (loopback `127.0.0.1:<port>` listening, works on every platform)
  - Optional: Windows Named Pipe transport (bonus if we want parity with future control-plane requirements)
- Wire the CLI client to detect (or read from config) whether to use the Unix socket or TCP port. A simple heuristic: check `BRANCHBOX_AGENT_SOCKET` first; if missing and `BRANCHBOX_AGENT_TCP_ADDR` is set, dial TCP.

### 2. Config & CLI flags

- Extend `AgentConfig` to include `tcp_bind_addr` (with a default on non-Unix targets) and a feature flag to prefer TCP even on macOS/Linux (handy for WSL or remote tunnels).
- CLI `AgentClient` learns to use TCP when `BRANCHBOX_AGENT_TCP_ADDR` (or the config file) is set.

### 3. Tests & tooling

- Update `scripts/manual-agent-e2e.sh` to detect the platform; on Windows, launch the agent with TCP transport and pass the env var to the CLI harness.
- Add a small integration test (maybe under `tests/`) that spins up the agent on TCP and performs a sample request using the CLI client to guard against regressions.
- Ensure the `build_extra` Windows job runs `cargo build` with the agent included (it already does) and ideally run a smoke test in the Windows matrix once the TCP transport works.

### 4. Documentation

- README / `docs/ARCHITECTURE.md`: Add a transport matrix (“Unix sockets on macOS/Linux; TCP on Windows”) plus instructions on overriding `BRANCHBOX_AGENT_SOCKET`/`...TCP_ADDR`.
- Add troubleshooting guidance (“On Windows, ensure the TCP port is not firewalled; refer to script X for manual e2e”).

## Open Questions

- Do we eventually need named pipes instead of TCP for security reasons (e.g., preventing other local processes from connecting)? If so, we should add a follow-up to evaluate Named Pipe support in `tokio`.
- Should the control plane also talk over TCP/outbound connections for Windows support, or do we assume only local CLI usage for now?
- How do we coordinate the config file between Unix socket and TCP settings without confusing existing users?

## Next Steps

1. Land transport abstraction with TCP fallback (agent + CLI updates).
2. Update the manual agent e2e script and documentation.
3. Add Windows smoke test and re-enable the `build_extra.windows` job to ensure the agent compiles/runs.
4. Track named-pipe or additional hardening if required.
