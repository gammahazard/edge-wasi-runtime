#!/bin/bash
# Test buzzer relay on Pi4 (GPIO 17)
# Relay is normally OFF, triggered by pulling LOW

# --- LOAD CONFIG FROM .env ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [ -f "$ENV_FILE" ]; then source "$ENV_FILE"; else echo "⚠️ No .env file!"; exit 1; fi
# -----------------------------

echo "🔊 Testing buzzer relay on Spoke 1 GPIO 17..."

ssh ${SPOKE_USER}@${SPOKE1_IP} << 'EOF'
python3 << 'PYEOF'
import RPi.GPIO as GPIO
import time

PIN = 17

GPIO.setmode(GPIO.BCM)
GPIO.setwarnings(False)
GPIO.setup(PIN, GPIO.OUT)

print(f"Testing GPIO {PIN}...")

# Relay OFF (default) - HIGH
GPIO.output(PIN, GPIO.HIGH)
print("  Relay OFF (HIGH)")
time.sleep(0.5)

# Relay ON - LOW (should trigger buzzer)
print("  Relay ON (LOW) - BUZZING!")
GPIO.output(PIN, GPIO.LOW)
time.sleep(0.5)

# Relay OFF
GPIO.output(PIN, GPIO.HIGH)
print("  Relay OFF (HIGH)")

print("Done! Did you hear the beep?")
GPIO.cleanup()
PYEOF
EOF
