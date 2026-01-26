#!/bin/bash
# ==============================================================================
# update-all-nodes.sh - Master Cluster Update Script
# ==============================================================================
#
# Usage: ./scripts/update-all-nodes.sh
#
# Description:
#   1. Syncs Host Source Code to Hub (RevPi)
#   2. Builds Release Binary on Hub (with hardware features)
#   3. Downloads new binary to local machine
#   4. Pushes binary to Hub and Pi4 Spoke (NOT Pi Zero)
#   5. Deploys Pi Zero Native Service separately (no WASM)
#   6. Restarts services on all nodes
#
# ==============================================================================

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"

if [ -f "$ENV_FILE" ]; then
    source "$ENV_FILE"
else
    echo "⚠️  No .env file found at $ENV_FILE"
    echo "   Create one with: HUB_USER, SPOKE_USER, HUB_IP, SPOKE1_IP, SPOKE2_IP"
    exit 1
fi
# -----------------------------

set -e  # Exit on error

echo "🚀 Starting Master Cluster Update..."

# ==============================================================================
# 0. BUILD PLUGINS (LOCAL)
# ==============================================================================
echo "🐍 [1/8] Building WASM Plugins locally..."
./scripts/build-plugins-wsl.sh

# ==============================================================================
# 1. UPDATE SOURCE & CONFIGS (SYNC)
# ==============================================================================
echo "📦 [2/8] Syncing source code to Hub..."
ssh ${HUB_USER}@${HUB_IP} "mkdir -p ~/wasi-python-host/host/src ~/wasi-python-host/host/config"
scp host/src/*.rs ${HUB_USER}@${HUB_IP}:~/wasi-python-host/host/src/

echo "⚙️ [3/8] Syncing CONFIG files to Nodes..."
# Hub Config
scp config/hub.toml ${HUB_USER}@${HUB_IP}:~/wasi-python-host/host/config/host.toml
# Spoke 1 (Pi 4) Config
ssh ${SPOKE_USER}@${SPOKE1_IP} "mkdir -p ~/wasi-python-host/host/config"
scp config/spoke.toml ${SPOKE_USER}@${SPOKE1_IP}:~/wasi-host-update-config.toml

echo "🔨 [4/8] Building Release Binary on Hub (this may take a while)..."
# Touch source files to force cargo to see them as modified
ssh ${HUB_USER}@${HUB_IP} "touch ~/wasi-python-host/host/src/*.rs && source ~/.cargo/env && cd ~/wasi-python-host/host && cargo build --release --features hardware"

# ==============================================================================
# 2. REDISTRIBUTE BINARY & PLUGINS (Hub + Pi4 ONLY)
# ==============================================================================
echo "⬇️  [5/8] Downloading new binary from Hub..."
scp ${HUB_USER}@${HUB_IP}:~/wasi-python-host/host/target/release/wasi-host ./wasi-host-latest

echo "⬆️  [6/8] Uploading binary to Pi4 Spoke..."
scp ./wasi-host-latest ${SPOKE_USER}@${SPOKE1_IP}:~/wasi-host-update

echo "🧩 [7/8] Syncing Plugins to Hub and Pi4..."
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
# NOTE: Pi Zero uses native Python, NOT WASM plugins

# ==============================================================================
# 3. APPLY & RESTART WASM NODES (Hub + Pi4)
# ==============================================================================
echo "🔄 Restarting WASM Services..."

echo "    -> Hub..."
ssh ${HUB_USER}@${HUB_IP} "sudo systemctl stop wasi-host && sudo cp ~/wasi-python-host/host/target/release/wasi-host /usr/local/bin/ && sudo systemctl start wasi-host"

echo "    -> Spoke 1 (Pi 4)..."
ssh ${SPOKE_USER}@${SPOKE1_IP} "sudo systemctl stop wasi-host && sudo mv ~/wasi-host-update /usr/local/bin/wasi-host && sudo chmod +x /usr/local/bin/wasi-host && mv ~/wasi-host-update-config.toml ~/wasi-python-host/host/config/host.toml && sudo sed -i 's|ExecStart=.*|ExecStart=/usr/local/bin/wasi-host|g' /etc/systemd/system/wasi-host.service && sudo systemctl daemon-reload && sudo systemctl start wasi-host"

# ==============================================================================
# 4. DEPLOY PI ZERO NATIVE SERVICE
# ==============================================================================
echo "🐍 [8/8] Deploying Pi Zero Native Service..."
PIZERO_TARGET="${SPOKE_USER}@${SPOKE2_IP}"

# Stop old WASM service if running
ssh $PIZERO_TARGET "sudo systemctl stop wasi-host 2>/dev/null || true"
ssh $PIZERO_TARGET "sudo systemctl disable wasi-host 2>/dev/null || true"

# Create directory and install deps
ssh $PIZERO_TARGET "mkdir -p ~/wasi-python-host/pizero-native"
ssh $PIZERO_TARGET "pip3 install --user smbus2 requests 2>/dev/null || true"

# Copy files
scp pizero-native/pizero_service.py $PIZERO_TARGET:~/wasi-python-host/pizero-native/
scp pizero-native/pizero-native.service $PIZERO_TARGET:~/wasi-python-host/pizero-native/

# Substitute actual username in service file (template uses 'pi' as placeholder)
ssh $PIZERO_TARGET "sed -i 's|/home/pi/|/home/${SPOKE_USER}/|g' ~/wasi-python-host/pizero-native/pizero-native.service"
ssh $PIZERO_TARGET "sed -i 's|User=pi|User=${SPOKE_USER}|g' ~/wasi-python-host/pizero-native/pizero-native.service"

# Install and start service
ssh $PIZERO_TARGET "sudo cp ~/wasi-python-host/pizero-native/pizero-native.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl enable pizero-native && sudo systemctl restart pizero-native"

echo ""
echo "✅ Cluster Update Complete!"
echo "   - Hub: WASM Runtime ✓"
echo "   - Pi4: WASM Runtime ✓"
echo "   - Pi Zero: Native Python Service ✓ (BME680 + Network Monitor)"
