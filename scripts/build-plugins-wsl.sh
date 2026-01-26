#!/bin/bash
set -e
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source $HOME/wasi-env/bin/activate
echo "🔨 Building Python WASM plugins..."

build_plugin() {
    local name=$1
    local world=$2
    echo -e "\n📊 Building $name plugin..."
    cd "$ROOT_DIR/plugins/$name"
    componentize-py -d ../../wit -w $world componentize app -o $name.wasm
    echo "✅ $name.wasm created"
}

build_plugin "dht22" "dht22-plugin"
build_plugin "pi4-monitor" "pi4-monitor-plugin"
build_plugin "revpi-monitor" "revpi-monitor-plugin"
build_plugin "pizero-monitor" "pizero-monitor-plugin"
build_plugin "bme680" "bme680-plugin"
build_plugin "dashboard" "dashboard-plugin"

echo -e "\n🎉 All plugins built successfully!"
