# Worktree CLI

Command-line tool for git worktree and devcontainer orchestration.

## Installation

```bash
cargo install --path .
```

## Usage

### Bootstrap a Project

Generate devcontainer configuration for your project:

```bash
# Auto-detect stack and generate files
worktree bootstrap

# Force a specific stack
worktree bootstrap --stack rails
worktree bootstrap --stack nodejs
worktree bootstrap --stack rust
worktree bootstrap --stack generic

# Bootstrap a different directory
worktree bootstrap --path /path/to/project
```

This creates:
- `.devcontainer/devcontainer.json`
- `.devcontainer/compose.yaml`
- `.devcontainer/Dockerfile`
- `.env.sample` (if it doesn't exist)

### Detect Project Configuration

See what stack, adapters, and modules would be used:

```bash
worktree detect

# Example output:
# Project: .
# Stack: Rails
# Adapter: rails
# Enabled modules: 4
#   - compose
#   - database
#   - tunnel
#   - specs
```

### Generate Feature Names

Create DNS-safe feature names from titles:

```bash
worktree generate-name "OAuth Integration Feature"
# Output: oauth-integration

worktree generate-name "Add Support for Multiple Databases"
# Output: add-support-multiple-databases
```

### Validate Feature Names

Check if a feature name is valid:

```bash
worktree validate-name oauth-integration
# Output: ✓ Valid feature name: oauth-integration

worktree validate-name "OAuth Integration"
# Output: ✗ Invalid feature name: OAuth Integration
#   Feature names must be DNS-safe (lowercase a-z, 0-9, hyphens only)
```

## Environment Variables

- `RUST_LOG` - Set logging level (e.g., `RUST_LOG=debug worktree detect`)

## Examples

### Complete Workflow

```bash
# 1. Bootstrap a new Rails project
cd my-rails-app
worktree bootstrap

# 2. Verify configuration
worktree detect

# 3. Generate a feature name
worktree generate-name "Add Payment Integration"
# Output: add-payment-integration

# 4. Validate the name
worktree validate-name add-payment-integration
# Output: ✓ Valid feature name: add-payment-integration
```

## Development

Run without installing:

```bash
cargo run -- bootstrap
cargo run -- detect
cargo run -- generate-name "My Feature"
```

Run tests:

```bash
cargo test
```
