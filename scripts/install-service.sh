#!/bin/bash
# ==============================================================================
# install-service.sh - Install WASI Host as a Systemd Service
# ==============================================================================
# 
# Usage: ./install-service.sh
#
# This script creates and enables a systemd service for the WASI Host.
# It automatically detects the current user and installation path.
#
# ==============================================================================

set -e

USER=$(whoami)
HOME_DIR=$(eval echo ~$USER)
PROJECT_DIR="$HOME_DIR/wasi-python-host"
HOST_DIR="$PROJECT_DIR/host"
BINARY_PATH="$HOST_DIR/target/release/wasi-host"
SERVICE_FILE="/etc/systemd/system/wasi-host.service"

echo "🔧 Installing WASI Host service for user '$USER'..."
echo "📂 Project Directory: $PROJECT_DIR"
echo "🚀 Binary Path: $BINARY_PATH"

if [ ! -f "$BINARY_PATH" ]; then
    echo "❌ Error: Binary not found at $BINARY_PATH"
    echo "   Please run 'cargo build --release' in the host directory first."
    exit 1
fi

echo "📝 Creating service file at $SERVICE_FILE..."

# Create the service file content
# We use sudo tee to write to /etc/systemd/system
sudo tee $SERVICE_FILE > /dev/null <<EOF
[Unit]
Description=WASI Python Host
After=network.target

[Service]
Type=simple
User=$USER
WorkingDirectory=$HOST_DIR
ExecStart=$BINARY_PATH
Restart=always
RestartSec=5
Environment=RUST_LOG=info
Environment=RUST_BACKTRACE=1
StandardOutput=append:$PROJECT_DIR/wasi-host.log
StandardError=inherit

[Install]
WantedBy=multi-user.target
EOF

echo "✅ Service file created."

echo "🔄 Reloading systemd daemon..."
sudo systemctl daemon-reload

echo "▶️ Enabling and starting service..."
sudo systemctl enable wasi-host
sudo systemctl restart wasi-host

echo "🔍 Checking status..."
sudo systemctl status wasi-host --no-pager

echo ""
echo "🎉 Success! The WASI Host is now running as a system service."
echo "   View logs with: journalctl -u wasi-host -f"
echo "   Stop service: sudo systemctl stop wasi-host"
