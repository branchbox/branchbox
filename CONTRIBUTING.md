# Contributing to BranchBox

Thank you for your interest in contributing to BranchBox! This document provides guidelines and instructions for contributing.

## Getting Started

### Prerequisites

- Rust 1.75+ ([Install Rust](https://rustup.rs/))
- Git
- Docker (for integration tests)
- VS Code or Cursor (recommended for devcontainer support)

### Development Setup

#### Using Devcontainer (Recommended)

1. Clone the repository:
   ```bash
   git clone https://github.com/branchbox-branchbox.git
   cd branchbox
   ```

2. Open in VS Code/Cursor and reopen in container when prompted

3. Inside the container:
   ```bash
   cd core
   cargo build
   cargo test
   ```

#### Local Development

1. Clone the repository
2. Install Rust toolchain:
   ```bash
   rustup update
   rustup component add rustfmt clippy
   ```

3. Build and test:
   ```bash
   cargo build
   cargo test
   ```

## Development Workflow

### Before Making Changes

1. Create a new branch:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Make sure tests pass:
   ```bash
   cargo test --all
   ```

### Making Changes

1. **Write tests first** (TDD approach)
2. **Implement the feature**
3. **Run quality checks**:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```

4. **Update documentation**:
   ```bash
   cargo doc --open
   ```

### Code Quality Standards

#### Formatting

All code must be formatted with `rustfmt`:

```bash
cargo fmt --all -- --check
```

#### Linting

Code must pass Clippy with no warnings:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

#### Testing

- **Unit tests**: Test individual functions and modules
- **Integration tests**: Test component interactions
- **Doc tests**: Ensure examples in documentation work
- **Coverage**: Maintain >90% code coverage

```bash
# Run all tests (uses cargo-nextest for parity with CI)
cargo nextest run --all-features --no-fail-fast

# Run doc tests
cargo test --doc --all-features

# Generate coverage locally (matches CI configuration)
cargo install cargo-llvm-cov
cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
```

#### Documentation

- All public APIs must have documentation comments
- Include examples in doc comments
- Run doc tests:
  ```bash
  cargo test --doc
  ```

### Commit Messages

Follow conventional commits format:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types**:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `test`: Adding or updating tests
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `chore`: Maintenance tasks

**Examples**:
```
feat(naming): add support for custom word filters

fix(validation): handle missing .env files gracefully

docs(bootstrap): add usage examples for Rails projects

test(adapters): add comprehensive Rails adapter tests
```

### Pull Request Process

1. **Update your branch**:
   ```bash
   git fetch origin
   git rebase origin/main
   ```

2. **Ensure all checks pass**:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all
   cargo doc --no-deps
   ```

3. **Push your changes**:
   ```bash
   git push origin feature/your-feature-name
   ```

4. **Create Pull Request**:
   - Provide clear description of changes
   - Reference any related issues
   - Include screenshots/examples if applicable
   - Ensure CI passes

5. **Address Review Comments**:
   - Make requested changes
   - Push updates
   - Re-request review

## Project Structure

```
branchbox/
├── core/               # Core Rust library
│   ├── src/
│   │   ├── naming.rs      # Feature naming utilities
│   │   ├── validation.rs  # Environment validation
│   │   ├── adapters/      # Stack adapters
│   │   ├── modules/       # Feature modules
│   │   └── bootstrap/     # Self-bootstrapping system
│   └── tests/         # Integration tests
├── agent/             # Local agent daemon (planned)
├── cli/               # CLI tool (planned)
├── .github/           # GitHub workflows
└── docs/              # Documentation
```

## Testing Guidelines

### Unit Tests

Located in the same file as the code, in `#[cfg(test)]` modules:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_work_feature() {
        let result = generate_work_feature("OAuth Integration");
        assert_eq!(result, "oauth-integration");
    }
}
```

### Integration Tests

Located in `tests/` directory:

```rust
// tests/integration_test.rs
use worktree_core::bootstrap::Bootstrap;

#[test]
fn test_bootstrap_rails_project() {
    // Test logic here
}
```

### Doc Tests

Include examples in documentation:

```rust
/// Generate WORK_FEATURE name
///
/// # Examples
///
/// ```
/// use worktree_core::naming::generate_work_feature;
///
/// let name = generate_work_feature("OAuth Integration");
/// assert_eq!(name, "oauth-integration");
/// ```
pub fn generate_work_feature(title: &str) -> String {
    // Implementation
}
```

## Style Guidelines

### Rust Idioms

- Use `Result<T>` and `?` operator for error handling
- Prefer `impl Trait` for return types when appropriate
- Use descriptive variable names
- Keep functions small and focused
- Document public APIs thoroughly

### Error Handling

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

### Module Organization

```rust
// lib.rs
pub mod naming;
pub mod validation;

pub use error::{Error, Result};
```

## CI/CD Pipeline

All pull requests must pass:

1. **Quality Checks**:
   - `cargo fmt --check`
   - `cargo clippy`
   - `cargo audit`

2. **Tests**:
   - Unit tests on Linux and macOS
   - Integration tests
   - Doc tests

3. **Coverage**:
   - Maintain >90% code coverage
   - Coverage report uploaded to Codecov

4. **Build**:
   - Build on Linux, macOS, Windows
   - Build with stable and beta Rust

## Getting Help

- **Documentation**: [docs/](docs/)
- **Issues**: [GitHub Issues](https://github.com/branchbox-branchbox/issues)
- **Discussions**: [GitHub Discussions](https://github.com/branchbox-branchbox/discussions)

## File-operation security patterns

BranchBox writes secret material (tokens, signing keys) to disk.  Every file
operation that touches secrets — or any path an attacker could replace with a
symlink — **must** follow these rules.  Failing to do so has historically caused
review loops where each "fix" re-introduced the same class of vulnerability.

### The cardinal rule: never check-then-act

A pattern like this is **always wrong** for security-sensitive paths:

```rust
// ❌ TOCTOU — attacker can swap path for a symlink between check and write
if path.is_symlink() { fs::remove_file(path)?; }
fs::write(path, contents)?;
fs::set_permissions(path, …)?;   // follows symlinks!
```

Between any two syscalls an attacker can re-create a symlink.  The *only*
defence is to perform the security-critical operation **atomically in a single
syscall** (or a single `open()` with the right flags).

### Rust: `safe_write` / `safe_read` (in `core/src/bootstrap/mod.rs`)

| Operation | Pattern |
|-----------|---------|
| **Write** | `safe_write(path, contents, mode)` — opens with `O_NOFOLLOW \| O_CREAT \| O_TRUNC` and sets permissions via `OpenOptionsExt::mode()` in the same `open()` call. |
| **Read**  | `safe_read(path)` — opens with `O_NOFOLLOW`; returns `Ok(None)` for symlinks or missing files. |
| **Restricted placeholder** | `create_restricted_file(path)` — opens with `O_NOFOLLOW` + `.mode(0o600)`. |

**Do not** use `std::fs::set_permissions` after writing.  Permissions must be
set atomically at file creation time via `.mode()`.

### Shell: atomic writes via `mktemp` + `mv`

```bash
# ✅ Correct — umask sets 0600 at creation, mv is atomic on same filesystem
atomic_write() {
    local target="$1" dir tmp
    dir="$(dirname "$target")"
    tmp="$(umask 077 && mktemp "$dir/.tmp.XXXXXX")"
    cat > "$tmp"
    mv -f "$tmp" "$target"
}
printf 'TOKEN=%q\n' "$TOKEN" | atomic_write "$SECRET_FILE"
```

```bash
# ❌ Wrong — TOCTOU between the check and the write
[ -L "$f" ] && rm -f "$f"
echo "$SECRET" > "$f"
chmod 600 "$f"

# ❌ Also wrong — path-based chmod after mktemp has a TOCTOU window
tmp="$(mktemp /tmp/.tmp.XXXXXX)"
chmod 600 "$tmp"   # attacker could swap tmp for symlink between mktemp and chmod
```

### Quick checklist for PRs that touch file I/O

1. **Does the code write to a path that could be attacker-controlled?**
   → Use `safe_write` (Rust) or `atomic_write` (shell).
2. **Does the code read a path that could be a symlink?**
   → Use `safe_read` (Rust) or open with `O_NOFOLLOW`.
3. **Does the code call `set_permissions` / `chmod` on a path (not an fd)?**
   → Replace with atomic mode-at-creation.  Path-based `chmod` follows symlinks.
4. **Does a "fix" introduce a new check-then-act pattern?**
   → Stop.  The fix is wrong.  Re-read this section.

### Why this section exists

PR #53 went through 13+ review rounds because each TOCTOU "fix" introduced a
new variant of the same race condition.  The root cause was applying band-aids
(check-then-act guards) instead of fixing the pattern (atomic operations).

**When a reviewer flags the same class of bug twice, stop patching and fix the
underlying pattern.**

## Code of Conduct

Be respectful, inclusive, and constructive. We follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct).

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
