#!/bin/bash
# ==============================================================================
# update-host.sh - Build and deploy ONLY the Rust Host binary (Hub + Pi4)
# ==============================================================================
# NOTE: Pi Zero uses native Python service, not WASM host

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [ -f "$ENV_FILE" ]; then source "$ENV_FILE"; else echo "⚠️ No .env file!"; exit 1; fi
# -----------------------------

set -e

echo "🔨 Building Host on Hub (RevPi)..."
ssh ${HUB_USER}@${HUB_IP} "touch ~/wasi-python-host/host/src/*.rs && source ~/.cargo/env && cd ~/wasi-python-host/host && cargo build --release --features hardware"

echo "⬇️  Downloading binary from Hub..."
scp ${HUB_USER}@${HUB_IP}:~/wasi-python-host/host/target/release/wasi-host ./wasi-host-latest

echo "⬆️  Uploading to Pi4 Spoke..."
scp ./wasi-host-latest ${SPOKE_USER}@${SPOKE1_IP}:~/wasi-host-update
# NOTE: Pi Zero uses native Python, not WASM host

echo "🔄 Restarting WASM Services (Hub + Pi4)..."
ssh ${HUB_USER}@${HUB_IP} "sudo systemctl stop wasi-host && sudo cp ~/wasi-python-host/host/target/release/wasi-host /usr/local/bin/ && sudo systemctl start wasi-host"
ssh ${SPOKE_USER}@${SPOKE1_IP} "sudo systemctl stop wasi-host && sudo mv ~/wasi-host-update /usr/local/bin/wasi-host && sudo chmod +x /usr/local/bin/wasi-host && sudo systemctl start wasi-host"

echo "✅ Host updated on Hub and Pi4!"
echo "   (Pi Zero uses native Python service - use deploy-pizero-native.sh for updates)"
