#!/bin/bash
# ==============================================================================
# deploy-pizero-native.sh - Deploy the lightweight native service to Pi Zero
# ==============================================================================

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [ -f "$ENV_FILE" ]; then source "$ENV_FILE"; else echo "⚠️ No .env file!"; exit 1; fi
# -----------------------------

PIZERO_TARGET="${SPOKE_USER}@${SPOKE2_IP}"

echo "🚀 Deploying Pi Zero Native Service..."

# Stop old WASM-based service if running
echo "  1. Stopping old wasi-host service..."
ssh $PIZERO_TARGET "sudo systemctl stop wasi-host 2>/dev/null || true"
ssh $PIZERO_TARGET "sudo systemctl disable wasi-host 2>/dev/null || true"

# Create directory
echo "  2. Creating pizero-native directory..."
ssh $PIZERO_TARGET "mkdir -p ~/wasi-python-host/pizero-native"

# Install dependencies
echo "  3. Installing Python dependencies..."
ssh $PIZERO_TARGET "pip3 install --user smbus2 requests 2>/dev/null || sudo pip3 install smbus2 requests"

# Copy files
echo "  4. Copying service files..."
scp pizero-native/pizero_service.py $PIZERO_TARGET:~/wasi-python-host/pizero-native/
scp pizero-native/pizero-native.service $PIZERO_TARGET:~/wasi-python-host/pizero-native/

# Install and start service
echo "  5. Installing systemd service..."
ssh $PIZERO_TARGET << 'EOF'
sudo cp ~/wasi-python-host/pizero-native/pizero-native.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable pizero-native
sudo systemctl start pizero-native
sleep 2
sudo systemctl status pizero-native --no-pager
EOF

echo ""
echo "✅ Pi Zero Native Service deployed!"
echo "   View logs: ssh ${PIZERO_TARGET} 'tail -f ~/wasi-python-host/pizero-native.log'"
