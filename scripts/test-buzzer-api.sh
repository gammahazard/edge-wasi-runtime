#!/bin/bash
# Test buzzer via Hub API (which should forward to Pi4)

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [ -f "$ENV_FILE" ]; then source "$ENV_FILE"; else echo "⚠️ No .env file!"; exit 1; fi
# -----------------------------

echo "Testing buzzer via Hub API..."

curl -s -X POST -H "Content-Type: application/json" \
  -d '{"node_id":"pi4-spoke","pattern":"single"}' \
  http://${HUB_IP}:3000/api/buzzer/control

echo ""
echo "If you heard a beep, the API works!"
echo "If not, check Hub logs for forwarding errors."
