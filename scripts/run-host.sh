#!/bin/bash
# ==============================================================================
# run-host.sh - Build plugins and run the Rust host
# ==============================================================================
#
# One-command setup: builds Python plugins, then runs the Rust host.
# Run from the repository root: ./scripts/run-host.sh
#
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# Build plugins first
"$SCRIPT_DIR/build-plugins.sh"

echo ""
echo "🚀 Starting Rust host..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cd "$ROOT_DIR/host"
cargo run --release
