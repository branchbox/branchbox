# Agentify In-Guest Devcontainer Provider

BranchBox's `in-guest` runtime is the inner developer-environment provider for an isolation boundary already owned by Agentify. It does not create a VM, use SSH, or expose the guest Docker daemon to the coding container. The trusted guest creates a BranchBox worktree, generates a runtime-only devcontainer facade, starts the untrusted devcontainer, and returns correlated runtime evidence.

## Command contract

Agentify starts a feature with an absolute, trusted-guest manifest path:

```console
branchbox feature start coding-demo \
  --repo /workspace/agentify \
  --runtime in-guest \
  --runtime-manifest /run/agentify-runtime/branchbox-in-guest.json \
  --json
```

The manifest may live at any absolute path. It and every materialization must be an owner-only, regular, non-symlink file. Materializations must be below the manifest's sibling `materializations/` directory; BranchBox never accepts a value, token, URL, or secret on the command line.

```json
{
  "version": "3",
  "run_id": "run_opaque",
  "lease_id": "assignment_lease_opaque",
  "outer_runtime_id": "firecracker_vm_opaque",
  "workspace": "/workspace",
  "repository": {
    "path": "/workspace/agentify",
    "revision": "0123456789abcdef0123456789abcdef01234567"
  },
  "task_branch": "feature/coding-demo",
  "workspace_consumer": { "uid": 1000, "gid": 1000 },
  "tunnel_placement": "outer",
  "published_ports": [{ "host": 3000, "runtime": 3000 }],
  "leases": [
    {
      "lease_id": "lease_project_environment",
      "scope": "project-environment",
      "consumer": "rails-app",
      "expires_at": "2026-08-30T20:00:00Z",
      "materializations": [
        {
          "source_path": "/run/agentify-runtime/materializations/project-environment.env",
          "target_path": "/run/branchbox/leases/project-env",
          "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }
      ]
    },
    {
      "lease_id": "lease_outer_tunnel",
      "scope": "platform-tunnel",
      "consumer": "outer-connector",
      "materializations": []
    }
  ]
}
```

Version 3 requires one explicit, non-root `workspace_consumer` identity. Before starting the Dev
Container, BranchBox grants that signed group write access to only the assigned worktree and its
repository's required Git metadata. Directories receive setgid so newly created paths retain the
same group, plus a POSIX default ACL so those paths also inherit group read/write regardless of the
consumer's umask; without it the unprivileged runtime could not reclaim its own worktree at
teardown once the consumer had created a directory. Files receive group read/write and only
preserve executable access when already executable. BranchBox never adds world access, follows a
symlink, or changes a path it does not own. It then resolves the primary container user's actual
UID and primary GID and fails closed unless both equal the signed identity. The in-guest filesystem
must support POSIX default ACLs; delegation fails closed when it does not.

The outer runtime must start the unprivileged BranchBox process with the signed consumer GID as a
supplementary group. This is a narrow delegation, not root authority: the process can group-share
its own task paths but cannot change ownership or access unrelated files. Omitting this integration
preserves the version 1 or version 2 behavior and performs no workspace permission delegation.

Because the worktree stays owned by the runtime UID, Git would refuse every command in the coding
container with `detected dubious ownership`. Version 3 therefore also gives the primary Compose
service exactly two Git ownership exceptions through the generated facade's overridden
`environment`: `safe.directory` for the effective task worktree folder and for the canonical Git
metadata target. They are exact platform-owned paths, never a wildcard, and BranchBox is their only
possible source because project and provider environments both reserve the `GIT_CONFIG_` prefix; a
repository's own `GIT_CONFIG_*` values are replaced rather than merged, so a repository cannot
redirect `core.hooksPath` or any other Git setting into the consumer's process tree.

Supported lease scopes are `model-identity`, `source-control-identity`, `project-environment`, and `platform-tunnel`. An outer platform-tunnel lease cannot have a devcontainer materialization. Host and runtime ports are non-zero and unique. Every signed published port targets the inspected primary devcontainer only; a repository dependency cannot claim it by advertising the same port. Agentify should publish its primary application port (3000) and keep PostgreSQL on the private Compose network. A future multi-service route must extend the signed assignment with an exact service/container identity.

Exactly one `project-environment` materialization may be supplied. Its consumer must equal the primary Compose service and its target must be `/run/branchbox/leases/project-env`. Unlike other materializations, it is not mounted into the coding container. BranchBox validates its digest and canonical dotenv structure, then attaches the original source path only to the primary service as a required Compose `env_file` with `format: raw`. Docker Compose 2.30.0 or later is therefore required.

The canonical project-environment file is UTF-8, at most 64 KiB, ends with LF, and contains 1–256 strictly sorted, unique `UPPERCASE_NAME=raw value` lines. Values are single physical lines of at most 16 KiB. Raw format preserves `$`, `#`, spaces, and quote characters without interpolation; actual CR, LF, NUL, and other control characters are rejected. Do not JSON-quote values: raw Compose format would preserve those quote characters as part of the value. Multiline credentials must remain file materializations rather than environment variables. Runtime/control names such as `OPENAI_API_KEY`, Docker/Compose/Devcontainer controls, dynamic-loader variables, Git execution/config overrides, and shell startup variables are rejected. BranchBox never parses the file with a shell or serializes its values into the facade, state, output, or residue evidence.

Successful JSON output includes the resolved worktree and this runtime identity:

```json
{
  "provider": "in-guest",
  "runtime_id": "firecracker_vm_opaque",
  "published_ports": [{ "host": 3000, "runtime": 3000 }],
  "container_id": "docker-container-id",
  "workspace_folder": "/workspaces/coding-demo",
  "container_user": "vscode",
  "config_path": "/workspace/coding-demo/.devcontainer/.devcontainer.json",
  "in_guest": {
    "run_id": "run_opaque",
    "assignment_lease_id": "assignment_lease_opaque",
    "outer_runtime_id": "firecracker_vm_opaque",
    "repository_revision": "0123456789abcdef0123456789abcdef01234567",
    "task_branch": "feature/coding-demo",
    "tunnel_placement": "outer",
    "project_docker": "disabled",
    "leases": [],
    "state_path": "/workspace/agentify/.branchbox/runtime/in-guest/run_opaque.json"
  }
}
```

## Trusted-guest boundary

Before checkout, BranchBox validates the exact revision and rejects repository `.gitattributes` filter drivers. Worktree creation disables Git hooks, fsmonitor, and ambient global attributes. Immediately after checkout, before BranchBox env, adapter, spec, stash, or module behavior, it generates `.devcontainer/.devcontainer.json` and `.devcontainer/.branchbox-sbx-compose.yaml`. The source devcontainer and Compose files are not edited.

The generated facade:

- removes host-side `initializeCommand`, ambient `remoteEnv`, custom workspace/host mounts, devcontainer port publication/forwarding, env-file/runArg mounts, host IPC/PID, privileged mode, extra capabilities, and unsafe build authority;
- removes outside/in/from-Docker feature aliases and daemon-bearing run arguments, then inspects the Dev Containers merged configuration before startup to prove no alias, local Docker/containerd/Podman/BuildKit socket, or remote daemon endpoint was reintroduced by feature resolution;
- preserves only non-secret literal connectivity settings such as `DB_HOST=postgres`, `PORT=3000`, and `RAILS_ENV=development`; secrets and interpolated values require a typed project-environment materialization;
- replaces repository Compose env files and volumes, synthesizes only the canonical task worktree at a normalized effective folder below platform-owned `/workspaces` plus the authoritative Git metadata facade, rejects `extends`, external volumes, privileged namespaces, dangerous build options, secondary-service host paths, and service secrets/configs;
- clears repository `ports` and `expose` on every service, pins Dev Containers startup to the primary service and its validated Compose dependency closure, and leaves signed BranchBox loopback proxies targeting that inspected primary container as the only published-port path;
- disables cloudflared, Tailscale, ngrok, `/dev/net/tun`, and tunnel-named services with Compose `!override`, and removes dependency edges to them. The Agentify outer boundary owns the only platform connector;
- gives the primary coding service a private, bounded 1 GiB `/dev/shm`; repository `--shm-size` overrides are removed and host IPC remains forbidden;
- mounts non-environment manifest-approved individual files read-only; the typed project environment is available only as the primary service's raw env file. It never mounts a Docker/containerd/Podman/BuildKit socket, daemon state directory, or Agentify supervisor directory.
- for version 3, group-shares only the assigned worktree and required Git metadata with the signed
  Dev Container UID/GID; the coding container receives neither the outer runtime's UID nor access
  to sibling runtime files, browser evidence, leases, or supervisor state.

After startup, Docker inspection fails closed if the primary container is privileged, shares a host namespace, receives host devices or elevated capabilities, disables confinement, contains a supervisor socket/directory mount, persists a remote daemon endpoint, or persists `OPENAI_API_KEY`. A readiness exec must also succeed. A repository primary command or container-side lifecycle hook that still assumes stripped SSH/1Password files produces an explicit startup failure; BranchBox does not report the environment ready.

Project Docker is deliberately `disabled`. Projects that require Docker must later use a task-scoped rootless/nested daemon that cannot see supervisor containers, volumes, assignment state, or credential bundles.

## Coding-provider execution

Ordinary `feature exec` removes `OPENAI_API_KEY` from Dev Containers CLI subprocesses. Codex receives it only through a fixed executable and name-only inheritance lane:

```console
branchbox feature exec-provider coding-demo \
  --repo /workspace/agentify \
  --provider codex \
  --inherit-env OPENAI_API_KEY \
  -- <codex arguments>
```

BranchBox executes `docker exec --interactive --user <container-user> --workdir <workspace> --env OPENAI_API_KEY <container-id> codex <arguments>`. No executable override or additional inherited environment name is accepted, and the value is not serialized or placed in an argument.

## Teardown contract

```console
branchbox feature teardown coding-demo \
  --repo /workspace/agentify \
  --force \
  --json
```

`runtime_teardown` reports `provider`, `runtime_id`, `verified`, `residue_free`, and typed `residue`. Before startup, BranchBox records lexical/canonical workspace paths, deterministic Compose candidates, proxy names, and assignment identity. Even if `devcontainer up` fails after creating only dependency services, cleanup discovers ownership through exact `devcontainer.local_folder` and Compose project/working-directory/config-file labels. BranchBox removes the exact containers, project networks and volumes, loopback port proxies, individual materialization files, provider state, failed worktree, and failed task branch. A later teardown can recover owner-only provider state without registry metadata and bypasses all repository tunnel/database/Compose/spec modules and adapters. Provider state is retained when residue remains so cleanup can be retried. The generated facade disappears with the worktree. Image/build cache retention is currently outside residue accounting and is a documented execution-plane policy decision.

## Agentify canary prerequisites

The primary container must remain alive without host identity, and repository setup must treat local
1Password loading as optional when the platform supplies a typed project environment. `DB_HOST` is
preserved by the safe literal policy; account/admin seed values and framework encryption keys can
arrive through the signed assignment's typed project-environment env file. Model/provider identity
such as `OPENAI_API_KEY` remains reserved for the fixed provider execution lane and cannot be
persisted through project environment.
