# BranchBox CLI

**Isolated development environments for every feature.**

BranchBox helps you manage git worktrees with devcontainer configurations, making it easy to work on multiple features in parallel without environment conflicts.

## Installation

```bash
cargo install --path .
```

## Usage

### Initialize a Project

Set up devcontainer configuration for your project:

```bash
# Auto-detect stack and generate files
branchbox init

# Or use the alias
branchbox bootstrap

# Force a specific stack
branchbox init --stack rails
branchbox init --stack nodejs
branchbox init --stack rust
branchbox init --stack generic

# Initialize a different directory
branchbox init --path /path/to/project
```

This creates:
- `.devcontainer/devcontainer.json`
- `.devcontainer/compose.yaml`
- `.devcontainer/Dockerfile`
- `.env.sample` (if it doesn't exist)

### Detect Project Configuration

See what stack, adapters, and modules would be used:

```bash
branchbox detect

# Example output:
# 📦 BranchBox Configuration
#
# Project: .
# Stack: Rails
# Adapter: rails
#
# Enabled modules: 4
#   ✓ compose
#   ✓ database
#   ✓ tunnel
#   ✓ specs
```

### Feature Name Utilities

BranchBox provides tools for generating and validating feature names:

```bash
# Generate a feature name from a title
branchbox name generate "OAuth Integration Feature"
# Output: oauth-integration

branchbox name generate "Add Support for Multiple Databases"
# Output: add-support-multiple-databases

# Validate a feature name
branchbox name validate oauth-integration
# Output: ✓ Valid feature name: oauth-integration

branchbox name validate "OAuth Integration"
# Output: ✗ Invalid feature name: OAuth Integration
#   Feature names must be DNS-safe (lowercase a-z, 0-9, hyphens only)
```

## Command Structure

```
branchbox
├── init (alias: bootstrap)    # Initialize project with devcontainer
├── detect                     # Show project configuration
└── name
    ├── generate               # Generate feature name from title
    └── validate               # Validate feature name
```

## Environment Variables

- `RUST_LOG` - Set logging level (e.g., `RUST_LOG=debug branchbox detect`)

## Examples

### Complete Workflow

```bash
# 1. Initialize a new Rails project
cd my-rails-app
branchbox init

# 2. Verify configuration
branchbox detect

# 3. Generate a feature name
branchbox name generate "Add Payment Integration"
# Output: add-payment-integration

# 4. Validate the name
branchbox name validate add-payment-integration
# Output: ✓ Valid feature name: add-payment-integration
```

### Quick Commands

```bash
# Initialize with specific stack
branchbox init --stack nodejs

# Generate name (just the name, no prefix)
branchbox name generate "My Feature"

# Check what would be configured
branchbox detect
```

## Why "BranchBox"?

- **Branch**: Every feature gets its own git worktree branch
- **Box**: Isolated devcontainer environment for each branch
- **Result**: Work on multiple features in parallel without conflicts!

## Development

Run without installing:

```bash
cargo run -- init
cargo run -- detect
cargo run -- name generate "My Feature"
```

Run tests:

```bash
cargo test
```

## Future Commands

Coming soon:

```bash
branchbox feature create <name>    # Create new feature worktree
branchbox feature list             # List all feature worktrees
branchbox feature remove <name>    # Remove feature worktree
```
