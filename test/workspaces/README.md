# Sample Workspaces

`test/workspaces/` hosts lightweight example projects that make it easy to exercise BranchBox end-to-end without touching your real repositories.

## Layout

- `templates/` – git-tracked source material for each sample project.
- `local/` – git-ignored working copies produced by the setup script. You can safely delete and recreate these whenever you need a fresh run.

## Getting Started

```bash
# From repo root
./scripts/setup-sample-workspaces.sh
```

The script copies each template into `test/workspaces/local/<sample>/`, bootstraps a git repository, and prints the next commands to run. By default it skips samples that already exist; pass `--force` to recreate them.

Current templates:

| Template    | Stack  | Purpose                                |
|-------------|--------|----------------------------------------|
| `rust-cli`  | `rust` | Minimal CLI for cargo workflow checks. |
| `node-api`  | `nodejs` | Minimal HTTP server for Node runtime checks. |

## Manual Devcontainer Smoke Test

1. **Prepare sample**:
   ```bash
   ./scripts/setup-sample-workspaces.sh --force <template>
   cd test/workspaces/local/<template>
   ```

2. **Initialize BranchBox** (stack is supplied by the script output):
   ```bash
   BRANCHBOX_SKIP_HOST_VALIDATION=1 branchbox init --stack <stack>
   ```

3. **Start feature worktree**:
   ```bash
   branchbox feature start "devcontainer-smoke"
   ```

4. **Open both directories (`.` and the generated feature worktree) in VS Code or Cursor** and verify the “Reopen in Container” prompt appears.

5. **Validate shared tooling** inside the feature container (`gh auth status`, `claude whoami`, `codex whoami`, etc.).

6. **Confirm devcontainer sync**:
   ```bash
   # Modify the main .devcontainer, then run
   branchbox devcontainer sync --dry-run
   branchbox devcontainer sync
   ```

7. **Tear down**:
   ```bash
   branchbox feature teardown devcontainer-smoke --complete-spec
   ```

Refer to each template’s `README.md` for stack-specific commands (e.g. `cargo run`, `npm install && npm start`).

## Cleanup

Remove all local copies:

```bash
rm -rf test/workspaces/local
```

Regenerate an individual sample:

```bash
./scripts/setup-sample-workspaces.sh --force <template>
```

## Adding New Samples

1. Create a new directory under `templates/` (e.g. `templates/rails-blog`).
2. Add minimal source files, a `template.json` (with at least `"stack"`) and an accompanying `README.md` describing stack-specific test steps.
3. Update `setup-sample-workspaces.sh` if additional metadata is required.
4. Submit both template and documentation in a single commit.
