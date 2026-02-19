#!/usr/bin/env bash

set -euo pipefail

"$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/remotion-docs-assets.sh" --part all "$@"
