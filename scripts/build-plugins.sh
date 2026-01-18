#!/bin/bash
# ==============================================================================
# build-plugins.sh - Build Python plugins to WASM
# ==============================================================================
#
# This script compiles Python plugins to WASM components using componentize-py.
# Run from the repository root: ./scripts/build-plugins.sh
#
# PREREQUISITES:
#   pip install componentize-py
#
# ==============================================================================

set -e  # Exit on any error

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

echo "🔨 Building Python WASM plugins..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Build sensor (DHT22) plugin
echo ""
echo "📊 Building sensor plugin..."
cd "$ROOT_DIR/plugins/sensor"
componentize-py -d ../../wit -w sensor-plugin componentize app -o sensor.wasm
echo "✅ sensor.wasm created"

# Build BME680 plugin
echo ""
echo "🌍 Building BME680 plugin..."
cd "$ROOT_DIR/plugins/bme680"
componentize-py -d ../../wit -w bme680-plugin componentize app -o bme680.wasm
echo "✅ bme680.wasm created"

# Build dashboard plugin
echo ""
echo "🎨 Building dashboard plugin..."
cd "$ROOT_DIR/plugins/dashboard"
componentize-py -d ../../wit -w dashboard-plugin componentize app -o dashboard.wasm
echo "✅ dashboard.wasm created"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🎉 All plugins built successfully!"
echo ""
echo "Next: cd host && cargo run --release"
