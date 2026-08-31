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

## Security contract

- The assignment and every materialization must be owner-only regular files. Materializations must
  be siblings below the assignment's `materializations` directory and match their declared SHA-256.
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
- Teardown removes every recorded materialization. Any file that cannot be removed is reported as
  `lease-materialization` residue and prevents a residue-free teardown receipt.

Version 1 remains accepted for existing integrations and retains its original fixed provider
allowlist. New orchestrators should emit version 2 and place managed files below a run-specific
directory such as `/run/branchbox/managed/<run-id>/materializations`.
