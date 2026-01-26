#!/bin/bash
# ==============================================================================
# update-plugins.sh - Build and deploy ONLY the WASM plugins (Hub + Pi4 only)
# ==============================================================================
# NOTE: Pi Zero uses native Python service, not WASM plugins

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [ -f "$ENV_FILE" ]; then source "$ENV_FILE"; else echo "⚠️ No .env file!"; exit 1; fi
# -----------------------------

set -e

echo "🐍 Building Plugins locally..."
./scripts/build-plugins-wsl.sh

echo "🧩 Pushing Plugins to WASM Nodes (Hub + Pi4)..."
push_plugins() {
    local target=$1
    echo "    -> Pushing to $target..."
    ssh $target "mkdir -p ~/wasi-python-host/plugins/dashboard ~/wasi-python-host/plugins/pi4-monitor ~/wasi-python-host/plugins/revpi-monitor ~/wasi-python-host/plugins/dht22 ~/wasi-python-host/plugins/bme680"
    scp plugins/dashboard/dashboard.wasm $target:~/wasi-python-host/plugins/dashboard/
    scp plugins/pi4-monitor/pi4-monitor.wasm $target:~/wasi-python-host/plugins/pi4-monitor/
    scp plugins/revpi-monitor/revpi-monitor.wasm $target:~/wasi-python-host/plugins/revpi-monitor/
    scp plugins/dht22/dht22.wasm $target:~/wasi-python-host/plugins/dht22/
    scp plugins/bme680/bme680.wasm $target:~/wasi-python-host/plugins/bme680/
}

push_plugins "${HUB_USER}@${HUB_IP}"
push_plugins "${SPOKE_USER}@${SPOKE1_IP}"
# NOTE: Pi Zero uses native Python, not WASM - use deploy-pizero-native.sh for it

echo "✅ Plugins updated! (Hot reload will pick them up within 2 seconds)"
