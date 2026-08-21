#!/usr/bin/env bash
# Run the light-factory TUI client locally. Extra args are passed through to
# the `light-factory` binary (e.g. ./scripts/dev-tui.sh --help).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

cd "$ROOT"
exec cargo run -p light-factory-tui -- "$@"
