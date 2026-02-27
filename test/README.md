# Install Script Tests

Automated testing for `install.sh` using [bats-core](https://bats-core.readthedocs.io/) and [shellcheck](https://www.shellcheck.net/).

## Quick Start

```bash
# Install dependencies
brew install shellcheck bats-core  # macOS
# or
sudo apt-get install shellcheck && npm install -g bats  # Linux

# Run tests
bats test/install.bats             # All tests
shellcheck install.sh              # Linting
```

## Test Structure

```
test/
├── install.bats           # All tests (unit + integration)
├── helpers/
│   └── test_helper.bash   # Shared utilities
└── README.md              # This file
```

## What's Tested

### Unit Tests (individual functions)
- `detect_arch()` - Architecture detection (x86_64, aarch64)
- `detect_os()` - OS detection (Linux, macOS messages)
- `get_latest_version()` - GitHub API parsing
- Script structure (error handling, traps, cleanup)

### Integration Tests (end-to-end)
- Full installation with mocked downloads
- Custom version installation (`BRANCHBOX_VERSION`)
- Custom directory installation (`INSTALL_DIR`)
- Checksum verification (pass/fail)
- Download failure handling

## Running Tests

```bash
# All tests
bats test/install.bats

# Single test
bats test/install.bats -f "detect_arch"

# Verbose mode
bats test/install.bats --verbose-run

# With shellcheck
shellcheck install.sh && bats test/install.bats
```

## CI/CD

Tests run automatically via GitHub Actions on:
- Every push to `main` or `feature/*`
- Every pull request
- Manual workflow dispatch

See `.github/workflows/test-install-script.yml`

## Writing Tests

Example unit test:
```bash
@test "my_function: does something" {
  source install.sh
  run my_function "arg"
  [ "$status" -eq 0 ]
  [ "$output" = "expected" ]
}
```

Example integration test:
```bash
@test "integration: test scenario" {
  # Mock external dependencies
  curl() { echo '{"tag_name": "v1.0.0"}'; }
  export -f curl

  # Run install script
  run bash install.sh

  # Assert results
  [ "$status" -eq 0 ]
  [ -f "$INSTALL_DIR/branchbox" ]
}
```

## Manual Testing

Test on specific distribution:
```bash
docker run -it --rm \
  -v $(pwd)/install.sh:/install.sh:ro \
  ubuntu:22.04 \
  bash /install.sh
```

Test with environment variables:
```bash
BRANCHBOX_VERSION=v0.1.0 bash install.sh
INSTALL_DIR=$HOME/test-bin bash install.sh
```

## Dependencies

**Required for tests:**
- `bats-core` - Test framework
- `shellcheck` - Static analysis

**Required by install.sh:**
- `curl` or `wget` - Download files
- `tar` - Extract archives
- `sha256sum`, `shasum`, or `openssl` - Verify checksums

## Troubleshooting

**bats not found:**
```bash
npm install -g bats
# or
brew install bats-core
```

**shellcheck not found:**
```bash
sudo apt-get install shellcheck
# or
brew install shellcheck
```

## Resources

- [bats-core documentation](https://bats-core.readthedocs.io/)
- [ShellCheck wiki](https://github.com/koalaman/shellcheck/wiki)
- [GitHub Actions workflow](../.github/workflows/test-install-script.yml)
