# SBX and Firecracker Runtime Compatibility

This checklist captures execution-boundary behavior proven or exposed by the Agentify coding-demo run on 2026-08-30. BranchBox must provide the same developer-environment facade whether the outer boundary is Docker Sandboxes locally or a Hetzner Firecracker microVM in Agentify's execution plane.

## Compatibility checklist

- [x] **Devcontainer config discovery (SBX):** generate the runtime-only config as `.devcontainer/.devcontainer.json`, a basename recognized by the current Dev Containers CLI, and pass it explicitly to `devcontainer up`, `exec`, and readiness probes.
- [ ] **Devcontainer config discovery (Firecracker):** use the same explicit config selection and readiness probe. Add coverage for repositories whose source config is the top-level `.devcontainer.json`; the current SBX overlay intentionally rejects that layout instead of overwriting it.
- [x] **Git metadata facade (SBX):** when the primary Compose service binds `/workspaces/main/.git`, append an ignored Compose override that binds the actual BranchBox `repo_root/.git`. Translate a lifecycle hook's `/workspaces/main/.git/worktrees/<name>` pointer back to the canonical repository worktree metadata, validate it, and write a relative host-portable pointer.
- [ ] **Git metadata facade (Firecracker):** mount or project the authoritative repository metadata at the facade target without assuming that the repository is literally named `main`. Preserve Git metadata writes across container and microVM restarts, and run the same pointer repair after lifecycle hooks.
- [x] **Suspend and restart semantics (SBX):** give the primary Compose service `restart: unless-stopped`. Port proxies resolve the stable container name on the Compose network instead of capturing an ephemeral container IP.
- [ ] **Suspend and restart semantics (Firecracker):** reconcile the devcontainer, dependencies, tunnel connector, and port routes after microVM resume/reboot. Readiness must verify the current container identity and route, not only that the microVM process exists.
- [x] **Host-file projection (SBX):** copy only configured `${localEnv:HOME}/.ssh/*.pub` bind sources that are regular, non-symlink files into the sandbox home before Dev Containers starts. Remove only an exact empty public-key path left by a failed bind, and set mode `0644`. Private keys and arbitrary home-directory files are never copied by this facade.
- [ ] **Mount and identity projection (Firecracker):** express public identity separately from secret leases. Project allowlisted public files before devcontainer startup; deliver private credentials through Agentify's scoped secret mechanism, not host mounts. A broader file-projection policy is deliberately deferred until it has explicit per-resource classification and audit events.
- [ ] **DNS SRV and tunnel egress (both):** prove SRV lookup for `_v2-origintunneld._tcp.argotunnel.com`, TCP egress to the selected Cloudflare edge on port `7844`, connector registration, and an HTTP request through the assigned hostname.

## Cloudflared boundary decision

The nested Docker connector is not yet portable through Docker Sandboxes' transparent TCP proxy. A DoH relay inside the sandbox restores the Cloudflare SRV records, and direct hostname tests to `region1.v2.argotunnel.com:7844` and `region2.v2.argotunnel.com:7844` work. However, cloudflared's HTTP/2 TLS SNI is `h2.cftunnel.com`. The SBX proxy attempts to dial that certificate-only name, which has no A/AAAA record, instead of preserving cloudflared's already-selected edge IP. The connector then fails before registration.

BranchBox and the Agentify execution plane must choose and test one of these models:

1. Run cloudflared at the outer SBX/Firecracker boundary and route it to the devcontainer's stable published service address.
2. Keep cloudflared inside the devcontainer network, but ensure the microVM egress path preserves the original destination IP while using SNI only for TLS policy.

The Firecracker implementation must not inherit the SBX failure mode. Its acceptance test is the full chain: SRV answer → edge TCP/TLS on `7844` → active connector → public hostname → current devcontainer service → browser-visible response.

## Runtime-only files

BranchBox owns and ignores these generated files; linked application source remains unchanged:

- `.devcontainer/.devcontainer.json`
- `.devcontainer/.branchbox-sbx-compose.yaml`

They are regenerated on each SBX feature start. If the source devcontainer no longer needs an SBX Compose facade and no `runServices` override is configured, BranchBox removes both stale generated files without editing the source `devcontainer.json` or Compose files. Feature teardown removes them with the BranchBox worktree.
