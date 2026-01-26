#!/bin/bash
# ==============================================================================
# update-configs.sh - Deploy configuration files (WASM nodes + Pi Zero native)
# ==============================================================================

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [ -f "$ENV_FILE" ]; then source "$ENV_FILE"; else echo "⚠️ No .env file!"; exit 1; fi
# -----------------------------

set -e

echo "⚙️  Syncing Configuration Files..."

# Hub (WASM)
echo "    -> Hub (${HUB_USER}@${HUB_IP})..."
ssh ${HUB_USER}@${HUB_IP} "mkdir -p ~/wasi-python-host/host/config"
scp config/hub.toml ${HUB_USER}@${HUB_IP}:~/wasi-python-host/host/config/host.toml

# Spoke 1 (Pi 4 - WASM)
echo "    -> Spoke 1 (${SPOKE_USER}@${SPOKE1_IP})..."
ssh ${SPOKE_USER}@${SPOKE1_IP} "mkdir -p ~/wasi-python-host/host/config"
scp config/spoke.toml ${SPOKE_USER}@${SPOKE1_IP}:~/wasi-python-host/host/config/host.toml

# Spoke 2 (Pi Zero - Native Python)
echo "    -> Spoke 2 Pi Zero (Native Service)..."
scp pizero-native/pizero_service.py ${SPOKE_USER}@${SPOKE2_IP}:~/wasi-python-host/pizero-native/

echo "🔄 Restarting Services..."
ssh ${HUB_USER}@${HUB_IP} "sudo systemctl restart wasi-host"
ssh ${SPOKE_USER}@${SPOKE1_IP} "sudo systemctl restart wasi-host"
ssh ${SPOKE_USER}@${SPOKE2_IP} "sudo systemctl restart pizero-native"

echo "✅ Configs updated and services restarted!"
