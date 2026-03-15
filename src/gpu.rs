use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

// ── GpuMode ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum GpuMode {
    Integrated,
    Hybrid,
    Discrete,
    Unknown,
}

impl GpuMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Integrated => "Integrated",
            Self::Hybrid     => "Hybrid",
            Self::Discrete   => "Discrete",
            Self::Unknown    => "Unknown",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Integrated => "Intel iGPU only  ·  best battery life",
            Self::Hybrid     => "Intel + NVIDIA PRIME  ·  balanced",
            Self::Discrete   => "NVIDIA dGPU only  ·  max performance",
            Self::Unknown    => "",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "integrated"          => Self::Integrated,
            "hybrid"              => Self::Hybrid,
            "nvidia" | "discrete" => Self::Discrete,
            _                     => Self::Unknown,
        }
    }

    pub fn envycontrol_value(self) -> &'static str {
        match self {
            Self::Integrated => "integrated",
            Self::Hybrid     => "hybrid",
            Self::Discrete   => "nvidia",
            Self::Unknown    => "hybrid",
        }
    }

    pub fn variants() -> [Self; 3] {
        [Self::Integrated, Self::Hybrid, Self::Discrete]
    }

    pub fn index(self) -> usize {
        match self {
            Self::Integrated => 0,
            Self::Hybrid     => 1,
            Self::Discrete   => 2,
            Self::Unknown    => 0,
        }
    }
}

// ── PowerProfile ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum PowerProfile {
    Quiet,
    Balanced,
    Performance,
    Unknown,
}

impl PowerProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Quiet       => "Quiet",
            Self::Balanced    => "Balanced",
            Self::Performance => "Performance",
            Self::Unknown     => "Unknown",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Quiet       => "silent fan  ·  max battery savings",
            Self::Balanced    => "adaptive fan  ·  everyday use",
            Self::Performance => "turbo fan  ·  full CPU/GPU power",
            Self::Unknown     => "",
        }
    }

    pub fn sysfs_value(self) -> &'static str {
        match self {
            Self::Quiet       => "low-power",
            Self::Balanced    => "balanced",
            Self::Performance => "performance",
            Self::Unknown     => "balanced",
        }
    }

    pub fn from_sysfs(s: &str) -> Self {
        match s.trim() {
            "low-power" | "power-saver" => Self::Quiet,
            "balanced"                  => Self::Balanced,
            "performance"               => Self::Performance,
            _                           => Self::Unknown,
        }
    }

    pub fn variants() -> [Self; 3] {
        [Self::Quiet, Self::Balanced, Self::Performance]
    }

    pub fn index(self) -> usize {
        match self {
            Self::Quiet       => 0,
            Self::Balanced    => 1,
            Self::Performance => 2,
            Self::Unknown     => 1,
        }
    }
}

// ── GpuManager ────────────────────────────────────────────────────────────────

const PLATFORM_PROFILE: &str = "/sys/firmware/acpi/platform_profile";

pub struct GpuManager {
    /// Currently active GPU mode (per envycontrol --query)
    pub mode: GpuMode,
    /// User-selected (pending) GPU mode — may differ before apply
    pub pending_mode: GpuMode,
    /// Currently active power profile (from sysfs)
    pub power_profile: PowerProfile,
    /// User-selected (pending) power profile
    pub pending_profile: PowerProfile,
    pub envycontrol_available: bool,
    pub platform_profile_available: bool,
    /// A GPU mode change was staged; reboot required to take effect
    pub needs_reboot: bool,
    /// Integrated GPU name (e.g. "Intel UHD Graphics 630")
    pub igpu_name: Option<String>,
    /// Discrete GPU name (e.g. "NVIDIA GeForce RTX 4060")
    pub dgpu_name: Option<String>,
}

impl GpuManager {
    pub fn new() -> Self {
        let envycontrol_available = Command::new("envycontrol")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let platform_profile_available = Path::new(PLATFORM_PROFILE).exists();

        let mode = if envycontrol_available {
            read_envycontrol_mode()
        } else {
            GpuMode::Unknown
        };

        let power_profile = if platform_profile_available {
            fs::read_to_string(PLATFORM_PROFILE)
                .map(|s| PowerProfile::from_sysfs(&s))
                .unwrap_or(PowerProfile::Unknown)
        } else {
            PowerProfile::Unknown
        };

        let (igpu_name, dgpu_name) = read_gpu_names();

        Self {
            pending_mode: mode,
            mode,
            pending_profile: power_profile,
            power_profile,
            envycontrol_available,
            platform_profile_available,
            needs_reboot: false,
            igpu_name,
            dgpu_name,
        }
    }

    pub fn refresh(&mut self) {
        if self.envycontrol_available {
            self.mode = read_envycontrol_mode();
        }
        if self.platform_profile_available {
            if let Ok(s) = fs::read_to_string(PLATFORM_PROFILE) {
                self.power_profile = PowerProfile::from_sysfs(&s);
            }
        }
    }

    /// Switch GPU mode via envycontrol. Takes effect after reboot.
    pub fn apply_gpu_mode(&mut self) -> Result<(), String> {
        if !self.envycontrol_available {
            return Err("envycontrol not found — install it from github.com/bayasdev/envycontrol".to_string());
        }

        let mode_str = self.pending_mode.envycontrol_value();

        let out = Command::new("pkexec")
            .args(["envycontrol", "--switch", mode_str])
            .output()
            .map_err(|e| format!("failed to run envycontrol: {e}"))?;

        if out.status.success() {
            self.mode = self.pending_mode;
            self.needs_reboot = true;
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let msg = if !stderr.trim().is_empty() { stderr } else { stdout };
            Err(format!("envycontrol: {}", msg.trim()))
        }
    }

    /// Apply power profile via powerprofilesctl (D-Bus), with sysfs fallback.
    pub fn apply_power_profile(&mut self) -> Result<(), String> {
        if !self.platform_profile_available {
            return Err("platform_profile not available on this kernel".to_string());
        }

        // Attempt 1 — powerprofilesctl (power-profiles-daemon, no root needed)
        let ppd_value = match self.pending_profile {
            PowerProfile::Quiet       => "power-saver",
            PowerProfile::Balanced    => "balanced",
            PowerProfile::Performance => "performance",
            PowerProfile::Unknown     => "balanced",
        };
        let out = Command::new("powerprofilesctl")
            .args(["set", ppd_value])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                self.power_profile = self.pending_profile;
                return Ok(());
            }
            // powerprofilesctl exists but failed — surface the error
            let err = String::from_utf8_lossy(&o.stderr);
            let err = err.trim();
            if !err.is_empty() {
                return Err(format!("powerprofilesctl: {err}"));
            }
        }

        // Attempt 2 — direct sysfs write (works if running as root or with CAP_SYS_ADMIN)
        let sysfs_value = self.pending_profile.sysfs_value();
        match fs::write(PLATFORM_PROFILE, sysfs_value) {
            Ok(_) => { self.power_profile = self.pending_profile; return Ok(()); }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(e) => return Err(format!("sysfs write error: {e}")),
        }

        Err("Failed to set power profile.\nInstall power-profiles-daemon: sudo apt install power-profiles-daemon".to_string())
    }
}

fn read_gpu_names() -> (Option<String>, Option<String>) {
    let out = Command::new("lspci")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let mut igpu: Option<String> = None;
    let mut dgpu: Option<String> = None;

    for line in out.lines() {
        let lower = line.to_lowercase();
        if !lower.contains("vga") && !lower.contains("3d controller") && !lower.contains("display controller") {
            continue;
        }

        // Format: "01:00.0 VGA compatible controller: NVIDIA Corporation GeForce ..."
        // Skip the PCI address (up to first space), then find the ": " separating class from name.
        let after_addr = match line.splitn(2, ' ').nth(1) {
            Some(s) => s,
            None => continue,
        };
        let name = match after_addr.splitn(2, ": ").nth(1) {
            Some(s) => s,
            None => continue,
        };
        // Strip trailing "(rev XX)"
        let name = match name.rfind(" (rev ") {
            Some(pos) => name[..pos].trim(),
            None      => name.trim(),
        };

        let lower_name = name.to_lowercase();
        if lower_name.contains("intel") {
            igpu = Some(shorten_gpu_name(name));
        } else if lower_name.contains("nvidia") {
            dgpu = Some(shorten_gpu_name(name));
        } else if lower_name.contains("amd") || lower_name.contains("advanced micro") {
            // AMD iGPU (Vega/Radeon Graphics) vs dGPU (Radeon RX / Pro)
            if lower_name.contains(" rx ") || lower_name.contains("radeon pro") {
                dgpu = Some(shorten_gpu_name(name));
            } else {
                igpu = Some(shorten_gpu_name(name));
            }
        }
    }

    (igpu, dgpu)
}

/// Trim verbose vendor prefixes for compact display.
fn shorten_gpu_name(name: &str) -> String {
    let name = name
        .replace("Intel Corporation ", "Intel ")
        .replace("NVIDIA Corporation ", "NVIDIA ")
        .replace("Advanced Micro Devices, Inc. [AMD/ATI] ", "AMD ")
        .replace("Advanced Micro Devices, Inc. ", "AMD ");
    // Extract bracketed model name if present, e.g. "Intel TigerLake [UHD Graphics 630]" → "Intel UHD Graphics 630"
    if let (Some(start), Some(end)) = (name.find('['), name.rfind(']')) {
        if end > start {
            let vendor = name.split_whitespace().next().unwrap_or("");
            let model  = name[start + 1..end].trim();
            return format!("{vendor} {model}");
        }
    }
    name
}

fn read_envycontrol_mode() -> GpuMode {
    Command::new("envycontrol")
        .arg("--query")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| GpuMode::from_str(&s))
        .unwrap_or(GpuMode::Unknown)
}
