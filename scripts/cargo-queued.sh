#!/usr/bin/env bash
# Serialize heavy Cargo builds to prevent resource contention
# Usage: ./scripts/cargo-queued.sh build
#        ./scripts/cargo-queued.sh test --all-features

set -euo pipefail

LOCK="${CARGO_QUEUE_LOCK:-/tmp/cargo-build.lock}"

# Open lock file descriptor
exec 9>"$LOCK"

# Acquire lock (wait up to 15 minutes)
if flock -w 900 9; then
    # Run cargo with all provided arguments
    exec cargo "$@"
else
    echo "ERROR: Failed to acquire build lock after 15 minutes" >&2
    exit 1
fi
