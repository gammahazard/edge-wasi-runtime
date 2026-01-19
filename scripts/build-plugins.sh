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

# Build DHT22 plugin
echo ""
echo "📊 Building DHT22 plugin..."
cd "$ROOT_DIR/plugins/dht22"
componentize-py -d ../../wit -w dht22-plugin componentize app -o dht22.wasm
echo "✅ dht22.wasm created"

# Build Pi Monitor plugin
echo ""
echo "🖥️ Building Pi Monitor plugin..."
cd "$ROOT_DIR/plugins/pi-monitor"
componentize-py -d ../../wit -w pi-monitor-plugin componentize app -o pi-monitor.wasm
echo "✅ pi-monitor.wasm created"

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

# Build OLED plugin
echo ""
echo "📺 Building OLED plugin..."
cd "$ROOT_DIR/plugins/oled"
componentize-py -d ../../wit -w oled-plugin componentize app -o oled.wasm
echo "✅ oled.wasm created"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🎉 All plugins built successfully!"
echo ""
echo "Next: cd host && cargo run --release"
