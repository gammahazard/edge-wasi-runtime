#!/bin/bash
# Test DHT22 sensor on Spoke 1 (Pi4)

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [ -f "$ENV_FILE" ]; then source "$ENV_FILE"; else echo "⚠️ No .env file!"; exit 1; fi
# -----------------------------

echo "🌡️ Testing DHT22 on Spoke 1..."

ssh ${SPOKE_USER}@${SPOKE1_IP} << 'EOF'
python3 << 'PYEOF'
import adafruit_dht
import board
import json

try:
    dht = adafruit_dht.DHT22(board.D4)
    temp = dht.temperature
    hum = dht.humidity
    print(f"SUCCESS: Temp={temp}C, Humidity={hum}%")
except Exception as e:
    print(f"ERROR: {e}")
PYEOF
EOF
