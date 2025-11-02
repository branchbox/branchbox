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

## Manual Devcontainer Smoke Test

1. **Prepare sample** (example: Rust CLI):
   ```bash
   ./scripts/setup-sample-workspaces.sh --force rust-cli
   cd test/workspaces/local/rust-cli
   ```

2. **Initialize BranchBox**:
   ```bash
   BRANCHBOX_SKIP_HOST_VALIDATION=1 branchbox init --stack rust
   ```

3. **Start feature worktree**:
   ```bash
   branchbox feature start "devcontainer-smoke"
   ```

4. **Open both directories (`.` and `../rust-cli-devcontainer-smoke/`) in VS Code or Cursor** and verify the “Reopen in Container” prompt appears.

5. **Validate shared tooling** inside the feature container:
   ```bash
   gh auth status
   claude whoami
   ```

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

Repeat with other samples to cover additional stacks.

## Cleanup

Remove all local copies:

```bash
rm -rf test/workspaces/local
```

Regenerate an individual sample:

```bash
./scripts/setup-sample-workspaces.sh --force rust-cli
```

## Adding New Samples

1. Create a new directory under `templates/` (e.g. `templates/rails-blog`).
2. Add minimal source files and a `TEMPLATE.md` describing stack-specific test steps.
3. Update `setup-sample-workspaces.sh` to recognise the new template.
4. Submit both template and documentation in a single commit.
