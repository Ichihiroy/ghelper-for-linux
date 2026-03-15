#!/usr/bin/env bash
# GHelper Linux — system setup
# Run with: sudo bash setup.sh [charge_limit]
# Default charge limit: 80%
set -e

LIMIT="${1:-80}"
BAT=""

# Find battery device
for name in BAT0 BAT1 BATC BATT; do
    if [ -d "/sys/class/power_supply/$name" ]; then
        BAT="$name"
        break
    fi
done

if [ -z "$BAT" ]; then
    echo "ERROR: No battery device found in /sys/class/power_supply/"
    exit 1
fi

THRESHOLD="/sys/class/power_supply/$BAT/charge_control_end_threshold"
if [ ! -f "$THRESHOLD" ]; then
    echo "ERROR: $THRESHOLD not found."
    echo "Make sure the asus-nb-wmi kernel module is loaded:"
    echo "  sudo modprobe asus-nb-wmi"
    exit 1
fi

echo "Battery device: $BAT"
echo "Charge limit:   $LIMIT%"
echo ""

# 1. udev rule — makes the sysfs file world-writable after each boot/resume
RULE_FILE="/etc/udev/rules.d/50-battery-charging-threshold.rules"
echo "Creating udev rule: $RULE_FILE"
cat > "$RULE_FILE" << 'RULE_EOF'
SUBSYSTEM=="power_supply", KERNEL=="BAT[01CT]", ACTION=="add|change", \
    RUN+="/bin/chmod a+w /sys/class/power_supply/%k/charge_control_end_threshold"
RULE_EOF

# 2. Config directory
CONF_DIR="/etc/ghelper-linux"
echo "Creating config directory: $CONF_DIR"
mkdir -p "$CONF_DIR"
echo "$LIMIT" > "$CONF_DIR/charge_limit"

# 3. Systemd service — applies limit on boot and after resume
SERVICE_FILE="/etc/systemd/system/battery-charge-limit.service"
echo "Creating systemd service: $SERVICE_FILE"
cat > "$SERVICE_FILE" << SVC_EOF
[Unit]
Description=Set ASUS battery charge limit (GHelper Linux)
After=multi-user.target

[Service]
Type=oneshot
ExecStart=/usr/bin/bash -c "echo \$(cat /etc/ghelper-linux/charge_limit) > /sys/class/power_supply/${BAT}/charge_control_end_threshold"
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target suspend.target hibernate.target
SVC_EOF

echo "Enabling service..."
systemctl daemon-reload
systemctl enable --now battery-charge-limit.service

# 4. Apply right now + trigger udev
echo "$LIMIT" > "$THRESHOLD" || true
udevadm control --reload-rules
udevadm trigger --subsystem-match=power_supply

echo ""
echo "✓ Setup complete!"
echo "  Charge limit set to ${LIMIT}% and will persist across reboots."
echo "  The app can now write the limit without sudo."
