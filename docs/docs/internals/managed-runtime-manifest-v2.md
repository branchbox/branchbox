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

## Preloaded service images

A version 2 assignment can opt into build-free startup by binding Compose service names to exact
preloaded image references:

```json
{
  "version": "2",
  "published_ports": [{"host": 3000, "runtime": 3000}],
  "service_images": {
    "app": "registry.example/team/app@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "database": "registry.example/team/database@sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
  },
  "port_proxy_image": "registry.example/runtime/tcp-proxy@sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
}
```

When `service_images` is present and non-empty, it must bind every runnable service declared by the
repository Compose files. Services that BranchBox disables as outer tunnel connectors are excluded.
Unknown, disabled, missing, duplicate map entries, indirect Compose includes, interpolated,
URL-shaped, tag-only, and non-SHA-256 image values are rejected before startup. A valid value is a
literal lowercase `registry/repository@sha256:<64 lowercase hexadecimal characters>` reference; an
optional tag and registry port may precede the digest. A local image content digest in the exact
form `sha256:<64 lowercase hexadecimal characters>` is also accepted.

When preloaded service mode publishes one or more ports, `port_proxy_image` is required and follows
the same immutable-reference rules. BranchBox inspects that exact image locally before startup and
passes it as an argument to the out-of-Compose loopback proxy launch with Docker `--pull=never`.
This closes the only image path outside the generated Compose facade. A version 2 assignment may
also set `port_proxy_image` independently of `service_images`; version 1 and version 2 assignments
without preloaded service images retain the legacy proxy image when this field is absent.

BranchBox writes the assigned `image`, resets the repository `build`, and sets `pull_policy: never`
for each bound service. It also removes devcontainer `build` and `features`, disables remote-user UID
image rewriting, and verifies every assigned reference with a local Docker image inspection before
running the Dev Containers CLI. A missing image therefore fails the assignment instead of building
or pulling during the task. Omitting `service_images` preserves existing Compose startup behavior;
omitting both preloaded-image fields preserves the complete version 1 and existing version 2
startup behavior.

## Shared directory, tool endpoint, and request-spool leases

Version 2 can also expose provider-neutral, per-run channels to the primary devcontainer. A
`shared-directory` lets an outer helper update finalized evidence while the container sees a
read-only bind. A standalone `tool-endpoint` can expose an owner-only directory or socket as a
read-only compatibility bind. A linked socket endpoint is different: it remains wholly outside the
coding container and is paired with a `tool-request` spool. The spool is a run-and-lease-derived
Docker volume, not a bind to the private endpoint.

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
      "lease_id": "delivery_endpoint",
      "scope": "tool-endpoint",
      "consumer": "coding-agent",
      "expires_at": "2099-01-01T00:00:00Z",
      "materializations": [
        {
          "source_path": "/run/branchbox/managed/run-id/tool-endpoints/delivery.sock",
          "target_path": "/run/branchbox/leases/tool-endpoints/delivery.sock"
        }
      ]
    },
    {
      "lease_id": "delivery_requests",
      "scope": "tool-request",
      "consumer": "coding-agent",
      "consumer_uid": 1000,
      "endpoint_lease_id": "delivery_endpoint",
      "request_spool_target": "/run/branchbox/leases/tool-requests/delivery",
      "expires_at": "2099-01-01T00:00:00Z",
      "materializations": [
        {
          "source_path": "/run/branchbox/managed/run-id/materializations/delivery-capability",
          "target_path": "/run/branchbox/leases/tool-requests/delivery/.capability",
          "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
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

Directly binding that socket does not translate UID 10001 in the guest to UID 1000 in a typical
container. Docker Compose has no portable per-bind idmapped-mount contract, and changing the socket
owner, group, ACL, or mode would weaken the trusted boundary. A linked `tool-request` therefore
suppresses the endpoint bind. Only its exact named request volume may be writable.

BranchBox initializes the volume with these paths:

- `.binding.json`: root-owned mode `0444`, containing the non-secret run, lease, consumer, request
  and response paths, filename convention, and quotas from the signed assignment.
- `.capability`: root-owned mode `0444`, containing a random endpoint-only capability. This token
  authorizes requests to the linked broker endpoint; it is not the broker's source-host, cloud,
  model, or other underlying credential.
- `requests/`: mode `0700` and owned by `consumer_uid`. The consumer writes a temporary file, uses
  mode `0600`, then atomically renames it to `<request_id>.json`.
- `responses/`: root-owned mode `0755`. The trusted dispatcher atomically creates a correlated
  `<request_id>.json` owned by `consumer_uid` at mode `0400`, so the consumer can read but cannot
  replace it.
- `.processing/`: root-owned mode `0700` and invisible to the consumer. BranchBox atomically moves
  one finalized request here before reading it, then re-checks its type, owner, mode, size, and link
  count. A request is therefore consumed once and cannot be path-swapped after validation.

The request envelope has only these top-level fields:

```json
{
  "version": "1",
  "run_id": "run-id",
  "lease_id": "delivery_requests",
  "consumer": "coding-agent",
  "request_id": "artifact-delivery",
  "capability": "endpoint-only-random-value",
  "payload": {}
}
```

The payload is opaque to BranchBox. The trusted endpoint must return the same version, run, lease,
consumer, and request IDs plus an opaque `payload`. BranchBox first consumes the request into the
root-only staging directory, strips the capability, claims the request in an owner-only replay
ledger, and relays one newline-delimited JSON frame followed by write-side EOF. It validates the
correlated response and writes the response file. A relay failure leaves the claim in place:
automatic replay is deliberately denied because the external side effect may already have occurred.
An authenticated correlated response is a successful transport even when its opaque payload reports
a tool-level decline (for example, an optional upload declined in favor of a durable artifact link).
Only transport, authentication, framing, or correlation failures fail dispatch.

Run the dispatcher concurrently with the coding provider:

```bash
branchbox feature dispatch-tool coding-demo \
  --lease delivery_requests \
  --request-id artifact-delivery \
  --wait-seconds 300 \
  --json
```

Only absence of the atomic final request is retryable. An exhausted wait returns exit status `75`
and JSON with `status: "not-pending"` and `retryable: true`. Success returns status `dispatched`.
Malformed paths, modes, symlinks, quotas, bindings, capabilities, replay claims, relay failures,
timeouts, and response mismatches are terminal and never share the retryable exit status.

## Security contract

- The assignment and file materializations must be owner-only regular files. File materializations
  are siblings below the assignment's `materializations` directory and match their declared
  SHA-256.
- Shared-directory and tool-endpoint leases are live, exact-run bindings. BranchBox rejects sources
  outside the manifest's `run_id` directory, source ownership or type changes, overlapping sources,
  targets outside the scope-specific BranchBox lease namespace, and ambient supervisor-authority or
  secret paths.
- The generated configuration mounts only manifest-declared source/target pairs. Final Docker
  inspection must show each bind-capable lease exactly once as a read-only bind on the primary
  container; a missing, writable, mistargeted, duplicate, or unsigned managed mount fails startup.
- A linked tool endpoint is never mounted. Final inspection permits one exact writable named volume
  per signed `tool-request` lease and rejects writable binds, read-only request volumes, wrong volume
  names, or any other writable entry under the managed namespace. Docker's `Mounts[].Name` is the
  signed volume identity; its engine-private physical `Source` path is never treated as a consumer
  mount or exposed to the container.
- Request spools enforce a non-root consumer UID, one same-consumer live socket endpoint, immutable
  binding/capability files, regular non-symlink request files, mode `0600`, at most 16 pending files,
  256 KiB per request, and 1 MiB total. BranchBox resolves the actual provider user from
  `remoteUser`/`containerUser` or Docker's inspected `Config.User`, runs `id -u` in the container,
  and rejects root or any mismatch with the signed `consumer_uid` before spool initialization and
  again before dispatch. Docker exec and Unix relay operations are time-bounded.
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
  sockets, every exact request volume, and the replay ledger. Any source that cannot be removed is
  reported as residue and prevents a residue-free teardown receipt.

Version 1 remains accepted for existing integrations and retains its original fixed provider
allowlist. New orchestrators should emit version 2 and place managed files below a run-specific
directory such as `/run/branchbox/managed/<run-id>/`.
