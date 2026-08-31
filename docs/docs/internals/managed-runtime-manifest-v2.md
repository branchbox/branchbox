---
sidebar_position: 2
---

# Managed runtime manifest v2

BranchBox's `in-guest` runtime accepts an orchestrator-owned, versioned assignment. The manifest
describes identities and materializations without teaching BranchBox about a particular platform,
coding provider, source host, tunnel vendor, or secret manager.

Version 2 separates three identities:

- `consumer` is an arbitrary name used to bind related leases.
- `executable` is the exact provider entrypoint BranchBox may start for that consumer.
- `environment_name` is the exact environment target for one owner-only, digest-bound value file.

For example:

```json
{
  "version": "2",
  "leases": [
    {
      "lease_id": "lease_model",
      "scope": "model-identity",
      "consumer": "coding-agent",
      "executable": "provider-cli",
      "inherited_environment": ["MODEL_ACCESS_TOKEN"],
      "expires_at": "2099-01-01T00:00:00Z",
      "materializations": []
    },
    {
      "lease_id": "lease_delivery",
      "scope": "provider-environment",
      "consumer": "coding-agent",
      "expires_at": "2099-01-01T00:00:00Z",
      "materializations": [
        {
          "source_path": "/run/branchbox/managed/run-id/materializations/delivery-token",
          "environment_name": "SOURCE_DELIVERY_TOKEN",
          "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }
      ]
    }
  ]
}
```

The complete manifest also carries the run, outer-runtime, repository, branch, workspace, and
published-port fields required by version 1.

## Shared directory and tool endpoint leases

Version 2 can also expose two provider-neutral, per-run channels to the primary devcontainer. A
`shared-directory` lets an outer helper update a directory while the container sees a read-only
bind. A `tool-endpoint` exposes either an owner-only directory or an owner-only Unix socket for
requests. These live under the assignment's exact run directory and do not carry `sha256`, because
their contents or socket state may change during the run:

```json
{
  "version": "2",
  "run_id": "run-id",
  "leases": [
    {
      "lease_id": "shared_exchange",
      "scope": "shared-directory",
      "consumer": "primary-tool",
      "expires_at": "2099-01-01T00:00:00Z",
      "materializations": [
        {
          "source_path": "/run/branchbox/managed/run-id/shared/exchange",
          "target_path": "/run/branchbox/leases/shared/exchange"
        }
      ]
    },
    {
      "lease_id": "request_endpoint",
      "scope": "tool-endpoint",
      "consumer": "primary-tool",
      "expires_at": "2099-01-01T00:00:00Z",
      "materializations": [
        {
          "source_path": "/run/branchbox/managed/run-id/tool-endpoints/requests",
          "target_path": "/run/branchbox/leases/tool-endpoints/requests"
        }
      ]
    }
  ]
}
```

The assignment must be stored directly in `/run/branchbox/managed/run-id/`. The run directory and
every source-directory ancestor use mode `0700` and the assignment owner's UID. A shared-directory
leaf uses mode `0755` so arbitrary non-root container users can inspect its files through the exact
read-only bind; its private ancestors still prevent ambient host traversal. Tool-endpoint
directories remain `0700`, and socket permissions remain owner-only.

## Security contract

- The assignment and file materializations must be owner-only regular files. File materializations
  are siblings below the assignment's `materializations` directory and match their declared
  SHA-256.
- Shared-directory and tool-endpoint leases are live, exact-run bindings. BranchBox rejects sources
  outside the manifest's `run_id` directory, source ownership or type changes, overlapping sources,
  targets outside the scope-specific BranchBox lease namespace, and ambient supervisor-authority or
  secret paths.
- The generated configuration mounts only manifest-declared source/target pairs. Final Docker
  inspection must show every pair exactly once as a read-only bind on the primary container; a
  missing, writable, mistargeted, duplicate, or unsigned managed mount fails startup.
- The primary devcontainer is forced onto Docker's built-in seccomp profile, and final inspection
  rejects a missing or unconfined profile. This preserves normal Unix and Internet socket families
  while denying direct `AF_VSOCK` creation even when no `/dev/vsock` path is mounted.
- A provider starts only when the requested executable and inherited environment exactly match one
  live `model-identity` lease. Provider-environment values are selected only for that lease's exact
  consumer.
- Environment names use bounded uppercase syntax and reject shell, loader, Git-control, container
  supervisor, and BranchBox control variables. Value files are bounded, UTF-8, and control-free.
- Provider-environment files are never mounted into the devcontainer and their names may not persist
  in the container configuration. BranchBox reads them immediately before provider execution,
  recomputes the digest from the bytes it will use, and passes values only to that provider process
  tree.
- Provider execution starts from an empty process environment, adding only a fixed safe `PATH`, the
  exact signed inherited names, and the exact digest-bound provider-environment bindings.
- Teardown removes every recorded materialization, including non-empty shared directories and Unix
  sockets. Any source that cannot be removed is reported as `lease-materialization` residue and
  prevents a residue-free teardown receipt.

Version 1 remains accepted for existing integrations and retains its original fixed provider
allowlist. New orchestrators should emit version 2 and place managed files below a run-specific
directory such as `/run/branchbox/managed/<run-id>/`.
