#!/bin/bash
# ==============================================================================
# start-system.sh - Combined build and run script
# ==============================================================================
#
# This script:
# 1. Builds all Python plugins (Sensor, BME680, Dashboard) to WASM
# 2. Starts the Rust Host
#
# Usage: ./scripts/start-system.sh
# ==============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# step 1: build plugins
echo "🛠️ Step 1: Building WASM Plugins..."
bash "$SCRIPT_DIR/build-plugins.sh"

# step 2: run host
echo ""
echo "🚀 Step 2: Starting Wasi Host..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$ROOT_DIR/host"

# Use cargo run (debug) for faster startup, or --release for performance
# On Pi, --release is highly recommended for WASM execution speed.
if [[ "$*" == *"--release"* ]]; then
    cargo run --release
else
    cargo run
fi
