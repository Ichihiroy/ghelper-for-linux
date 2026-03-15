use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ── Status ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ChargeStatus {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl ChargeStatus {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "Charging"                   => Self::Charging,
            "Discharging"                => Self::Discharging,
            "Full"                       => Self::Full,
            "Not charging" | "Not Charging" => Self::NotCharging,
            _                            => Self::Unknown,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Charging    => "Charging",
            Self::Discharging => "Discharging",
            Self::Full        => "Full",
            Self::NotCharging => "Not Charging",
            Self::Unknown     => "Unknown",
        }
    }

    pub fn is_plugged(&self) -> bool {
        matches!(self, Self::Charging | Self::Full | Self::NotCharging)
    }
}

// ── BatteryInfo ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BatteryInfo {
    pub capacity: u8,
    pub status: ChargeStatus,
    pub charge_limit: u8,
    pub voltage_v: Option<f32>,
    pub current_a: Option<f32>,
    pub energy_full_wh: Option<f32>,
    pub energy_now_wh: Option<f32>,
    pub energy_full_design_wh: Option<f32>,
    pub power_w: Option<f32>,
    pub cycle_count: Option<u32>,
    pub technology: Option<String>,
    pub manufacturer: Option<String>,
    pub model_name: Option<String>,
    /// Can we write to charge_control_end_threshold right now?
    pub can_write_limit: bool,
    /// Does the threshold sysfs file exist at all?
    pub has_limit_support: bool,
    /// Is the systemd service for persistent limit active?
    pub persistent_enabled: bool,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            capacity: 0,
            status: ChargeStatus::Unknown,
            charge_limit: 100,
            voltage_v: None,
            current_a: None,
            energy_full_wh: None,
            energy_now_wh: None,
            energy_full_design_wh: None,
            power_w: None,
            cycle_count: None,
            technology: None,
            manufacturer: None,
            model_name: None,
            can_write_limit: false,
            has_limit_support: false,
            persistent_enabled: false,
        }
    }
}

impl BatteryInfo {
    /// Battery health as a percentage of design capacity.
    pub fn health_percent(&self) -> Option<f32> {
        let full   = self.energy_full_wh?;
        let design = self.energy_full_design_wh?;
        if design <= 0.0 { return None; }
        Some((full / design * 100.0).clamp(0.0, 100.0))
    }

    /// Rough time-remaining estimate in hours.
    pub fn time_remaining_h(&self) -> Option<f32> {
        let power = self.power_w.filter(|&p| p > 0.1)?;
        match &self.status {
            ChargeStatus::Discharging => {
                let energy_now = self.energy_now_wh?;
                Some(energy_now / power)
            }
            ChargeStatus::Charging => {
                let energy_full = self.energy_full_wh?;
                let energy_now  = self.energy_now_wh?;
                let remaining   = (energy_full - energy_now).max(0.0);
                Some(remaining / power)
            }
            _ => None,
        }
    }
}

// ── BatteryManager ────────────────────────────────────────────────────────────

pub struct BatteryManager {
    pub battery_path: Option<PathBuf>,
    pub info: BatteryInfo,
}

impl BatteryManager {
    pub fn new() -> Self {
        let path = Self::find_battery_path();
        let info = path.as_deref().map(Self::read_info).unwrap_or_default();
        Self { battery_path: path, info }
    }

    fn find_battery_path() -> Option<PathBuf> {
        for name in ["BAT0", "BAT1", "BATC", "BATT"] {
            let p = PathBuf::from("/sys/class/power_supply").join(name);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    pub fn refresh(&mut self) {
        if let Some(ref p) = self.battery_path.clone() {
            self.info = Self::read_info(p);
        }
    }

    fn read_info(path: &Path) -> BatteryInfo {
        let read_str = |name: &str| -> Option<String> {
            fs::read_to_string(path.join(name))
                .ok()
                .map(|s| s.trim().to_string())
        };
        let read_u8  = |n: &str| read_str(n).and_then(|s| s.parse().ok());
        let read_u32 = |n: &str| read_str(n).and_then(|s| s.parse().ok());
        let read_i64 = |n: &str| read_str(n).and_then(|s: String| s.parse::<i64>().ok());
        let read_f32_from_uw = |n: &str| read_i64(n).map(|v| v as f32 / 1_000_000.0);

        let threshold_path = path.join("charge_control_end_threshold");
        let has_limit_support = threshold_path.exists();
        let can_write_limit = has_limit_support && {
            fs::OpenOptions::new().write(true).open(&threshold_path).is_ok()
        };
        let charge_limit = if has_limit_support {
            read_u8("charge_control_end_threshold").unwrap_or(100)
        } else {
            100
        };

        let persistent_enabled = Path::new("/etc/systemd/system/battery-charge-limit.service").exists();

        let voltage_v  = read_f32_from_uw("voltage_now").filter(|&v| v > 0.0);
        let current_a  = read_f32_from_uw("current_now");
        let energy_now = read_f32_from_uw("energy_now");
        let energy_full= read_f32_from_uw("energy_full");
        let energy_design = read_f32_from_uw("energy_full_design");
        let power_w    = read_f32_from_uw("power_now")
            .filter(|&p| p >= 0.0)
            .or_else(|| voltage_v.zip(current_a).map(|(v, i)| (v * i.abs()).max(0.0)));

        BatteryInfo {
            capacity: read_u8("capacity").unwrap_or(0),
            status: read_str("status")
                .map(|s| ChargeStatus::from_str(&s))
                .unwrap_or(ChargeStatus::Unknown),
            charge_limit,
            voltage_v,
            current_a,
            energy_now_wh: energy_now,
            energy_full_wh: energy_full,
            energy_full_design_wh: energy_design,
            power_w,
            cycle_count: read_u32("cycle_count"),
            technology: read_str("technology"),
            manufacturer: read_str("manufacturer"),
            model_name: read_str("model_name"),
            can_write_limit,
            has_limit_support,
            persistent_enabled,
        }
    }

    // ── Write charge limit ────────────────────────────────────────────────────

    pub fn set_charge_limit(&self, limit: u8) -> Result<(), String> {
        let path = self.battery_path.as_deref()
            .ok_or_else(|| "No battery found".to_string())?;
        let threshold_path = path.join("charge_control_end_threshold");

        if !threshold_path.exists() {
            return Err(
                "Battery charge limit is not supported on this device.\n\
                 Ensure the asus-nb-wmi kernel module is loaded:\n\
                 sudo modprobe asus-nb-wmi".to_string(),
            );
        }

        let value = limit.to_string();

        // Attempt 1 — direct write (works after udev setup or when running as root)
        match fs::write(&threshold_path, &value) {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(e) => return Err(format!("Write error: {e}")),
        }

        // Attempt 2 — pkexec tee (shows native GNOME auth dialog)
        BatteryManager::write_privileged(&threshold_path.to_string_lossy(), &value)
    }

    fn write_privileged(path: &str, value: &str) -> Result<(), String> {
        let mut child = Command::new("pkexec")
            .args(["tee", path])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to launch pkexec: {e}\nRun Setup to configure permissions."))?;

        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(value.as_bytes())
                .map_err(|e| format!("Stdin write error: {e}"))?;
        }

        let out = child.wait_with_output()
            .map_err(|e| format!("pkexec wait failed: {e}"))?;

        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "Permission denied.\nRun Setup to configure passwordless battery control.\n{}",
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }

    // ── Persistent setup (udev + systemd) ────────────────────────────────────

    pub fn run_setup(&self, limit: u8) -> Result<(), String> {
        let bat_name = self.battery_path.as_deref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("BAT0");

        let script = format!(
            r#"#!/usr/bin/env bash
set -e

RULE_FILE="/etc/udev/rules.d/50-battery-charging-threshold.rules"
CONF_DIR="/etc/ghelper-linux"
CONF_FILE="$CONF_DIR/charge_limit"
SERVICE_FILE="/etc/systemd/system/battery-charge-limit.service"

# 1. udev rule — makes threshold file world-writable after each boot/resume
cat > "$RULE_FILE" << 'RULE_EOF'
SUBSYSTEM=="power_supply", KERNEL=="BAT[01CT]", ACTION=="add|change", \
    RUN+="/bin/chmod a+w /sys/class/power_supply/%k/charge_control_end_threshold"
RULE_EOF

# 2. Config directory + initial charge limit value
mkdir -p "$CONF_DIR"
echo "{limit}" > "$CONF_FILE"

# 3. Systemd service — re-applies limit after boot and resume
cat > "$SERVICE_FILE" << 'SVC_EOF'
[Unit]
Description=Set ASUS battery charge limit
After=multi-user.target
After=suspend.target
After=hibernate.target

[Service]
Type=oneshot
ExecStart=/usr/bin/bash -c "cat /etc/ghelper-linux/charge_limit | xargs -I{{}} bash -c 'echo {{}} | tee /sys/class/power_supply/{bat}/charge_control_end_threshold'"
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target suspend.target hibernate.target
SVC_EOF

systemctl daemon-reload
systemctl enable --now battery-charge-limit.service

# 4. Trigger udev immediately (makes file writable right now without reboot)
udevadm control --reload-rules
udevadm trigger --subsystem-match=power_supply

echo "GHelper Linux setup complete."
"#,
            limit = limit,
            bat   = bat_name,
        );

        let tmp = "/tmp/ghelper-linux-setup.sh";
        fs::write(tmp, &script).map_err(|e| format!("Failed to write setup script: {e}"))?;

        let out = Command::new("pkexec")
            .args(["bash", tmp])
            .output()
            .map_err(|e| format!("pkexec failed: {e}"))?;

        let _ = fs::remove_file(tmp);

        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "Setup failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }

    /// Update the persistent limit stored in /etc/ghelper-linux/charge_limit
    pub fn update_persistent_limit(&self, limit: u8) -> Result<(), String> {
        let conf = "/etc/ghelper-linux/charge_limit";
        let value = limit.to_string();

        match fs::write(conf, &value) {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(e) => return Err(e.to_string()),
        }

        Self::write_privileged(conf, &value)
    }

    pub fn bat_name(&self) -> &str {
        self.battery_path.as_deref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
    }
}
