# In-Place Parent Structure Reorganization

## Overview

The `branchbox init --use-parent-structure` flag enables in-place reorganization of a git repository into an optimal worktree-parent structure. This is the recommended approach for organizing BranchBox projects.

## The Problem

When you have an existing repository, the typical structure is:

```
/path/to/my-project/     (git repository)
  .git/
  src/
  README.md
```

When you want to use BranchBox for isolated feature development, you need a structure that allows multiple worktrees as siblings:

```
/path/to/my-project/     (container directory)
  main/                  (main branch worktree)
  feature-auth/          (feature worktree)
  feature-api/           (feature worktree)
```

## The Solution

The `--use-parent-structure` flag reorganizes your repository in-place without moving it to a different location:

```bash
cd /path/to/my-project
branchbox init --reorganize --use-parent-structure
```

### What Happens

1. **Current location**: `/path/to/my-project/` (git repository)
2. **After reorganization**:
   - `/path/to/my-project/` becomes the container directory
   - `/path/to/my-project/main/` contains your git repository
   - Future worktrees will be created as siblings

### Safe Reorganization Process

The tool uses a safe "rename dance" to avoid collisions:

1. Rename current directory to temporary name: `.branchbox-temp-{uuid}`
2. Create new parent container with original name
3. Move temporary directory into container as `main/`
4. If any step fails, automatically roll back changes

## Usage Examples

### Basic Usage

```bash
# Navigate to your existing repository
cd ~/my-rails-app

# Initialize with parent structure
branchbox init --reorganize --use-parent-structure

# Result:
#   ~/my-rails-app/main/  (your repository is now here)
```

### With Dry Run

Preview what will happen without making changes:

```bash
branchbox init --reorganize --use-parent-structure --dry-run
```

Output:
```
[DRY RUN] Would reorganize in-place:
  Current: /home/user/my-rails-app
  Result:  my-rails-app/main/

  Future worktrees will be siblings:
    my-rails-app/main/
    my-rails-app/feature-name/
```

### Non-Interactive Mode

Skip confirmation prompts (useful for scripts):

```bash
branchbox init --reorganize --use-parent-structure -y
```

### Verbose Output

See detailed progress during reorganization:

```bash
branchbox init --reorganize --use-parent-structure -v
```

Output:
```
Starting safe reorganization:
  Step 1: Rename current directory to temporary name
  Step 2: Create parent container directory
  Step 3: Move repository into container as 'main'

  → Renaming my-rails-app to temporary location...
  → Creating container directory...
  → Moving repository into container as 'main'...

✓ Reorganization complete
  Repository is now at: /home/user/my-rails-app/main/
  Container directory: /home/user/my-rails-app/
```

## Benefits

### 1. **Keeps Current Location**
- No need to move to `~/projects/` or another location
- Repository stays exactly where it is
- Only the internal structure changes

### 2. **Optimal Worktree Organization**
- All worktrees are siblings at the same level
- Easy to navigate between worktrees
- Clean directory structure

### 3. **Safe and Reversible**
- Automatic rollback on failure
- Git repository integrity preserved
- All commits and branches intact

### 4. **Future-Proof**
- Sets up ideal structure for BranchBox workflows
- Works seamlessly with `branchbox feature start`
- Supports unlimited parallel feature development

## After Reorganization

Once reorganized, you can create feature worktrees:

```bash
# From anywhere in the container
cd ~/my-rails-app

# Create a new feature worktree
branchbox feature start "user authentication"

# Result:
#   ~/my-rails-app/
#     main/                    (main branch)
#     user-authentication/     (feature worktree)
```

Each worktree gets:
- Isolated git branch
- Isolated Docker containers
- Isolated database (if configured)
- Unique Cloudflare tunnel URL (if configured)

## Comparison with Standard Reorganization

### Standard (`--reorganize`)
Moves repository to a different location (default: `~/projects/`)

```bash
branchbox init --reorganize

# Before: /current/location/my-app/
# After:  ~/projects/my-app/
```

### In-Place Parent Structure (`--reorganize --use-parent-structure`)
Reorganizes in current location with parent/child structure

```bash
branchbox init --reorganize --use-parent-structure

# Before: /current/location/my-app/
# After:  /current/location/my-app/main/
```

## Git State Preservation

The reorganization preserves all git state:

- ✅ All commits and commit history
- ✅ All branches (local and remote)
- ✅ All tags
- ✅ All stashes
- ✅ Current working tree changes
- ✅ Git configuration
- ✅ Remote URLs

## When to Use

### ✅ Use In-Place Parent Structure When:

- You want to keep your repository in its current location
- You have multiple repositories in a specific location
- You're working in a team environment with established paths
- You want the cleanest worktree organization

### ❌ Use Standard Reorganization When:

- Repository is in a temporary location (`/tmp/`, `~/Downloads/`)
- You want all projects in a central location (`~/projects/`)
- You're starting fresh with a new repository structure

## Troubleshooting

### "Cannot reorganize root directory"

You cannot reorganize a repository at the root of the filesystem. Move it to a subdirectory first.

### Permission Denied

Ensure you have write permissions to the parent directory. The tool needs to create directories and move files.

### Rollback Occurred

If reorganization fails, the tool automatically restores the original state. Check the error message and resolve the issue before retrying.

## See Also

- [Universal Init Workflow](../features/completed/universal-init.md)
- [Feature Workflow Guide](../README.md)
- [BranchBox CLI Reference](../cli/README.md)
