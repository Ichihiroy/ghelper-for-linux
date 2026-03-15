use std::fs;
use std::path::Path;

// ── SystemInfo ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    pub cpu_model: Option<String>,
    pub cpu_temp_c: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub mem_total_mb: Option<u64>,
    pub mem_used_mb: Option<u64>,
    pub mem_available_mb: Option<u64>,
    pub thermal_zones: Vec<ThermalZone>,
    pub ac_connected: bool,
}

#[derive(Debug, Clone)]
pub struct ThermalZone {
    pub name: String,
    pub temp_c: f32,
}

impl SystemInfo {
    pub fn new() -> Self {
        let mut s = Self::default();
        s.refresh();
        s
    }

    pub fn refresh(&mut self) {
        self.cpu_model       = read_cpu_model();
        self.thermal_zones   = read_thermal_zones();
        self.cpu_temp_c      = guess_cpu_temp(&self.thermal_zones);
        self.gpu_temp_c      = guess_gpu_temp(&self.thermal_zones);
        self.ac_connected    = read_ac_online();
        read_meminfo(self);
    }

    pub fn mem_used_percent(&self) -> Option<f32> {
        let total = self.mem_total_mb? as f32;
        let avail = self.mem_available_mb? as f32;
        if total <= 0.0 { return None; }
        Some(((total - avail) / total * 100.0).clamp(0.0, 100.0))
    }
}

// ── Readers ───────────────────────────────────────────────────────────────────

fn read_cpu_model() -> Option<String> {
    let content = fs::read_to_string("/proc/cpuinfo").ok()?;
    for line in content.lines() {
        if line.starts_with("model name") {
            let model = line.splitn(2, ':').nth(1)?.trim().to_string();
            // Shorten for display
            let short = model
                .replace("(R)", "")
                .replace("(TM)", "")
                .replace("  ", " ");
            return Some(short.trim().to_string());
        }
    }
    None
}

fn read_thermal_zones() -> Vec<ThermalZone> {
    let base = Path::new("/sys/class/thermal");
    if !base.exists() { return vec![]; }

    let mut zones = Vec::new();

    // Read thermal_zone* entries
    let Ok(entries) = fs::read_dir(base) else { return zones };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if !name.starts_with("thermal_zone") { continue; }

        let temp_raw = fs::read_to_string(path.join("temp"))
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok());
        let zone_type = fs::read_to_string(path.join("type"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| name.clone());

        if let Some(t) = temp_raw {
            zones.push(ThermalZone {
                name: zone_type,
                temp_c: t as f32 / 1000.0,
            });
        }
    }

    // Also try hwmon sensors for additional coverage
    let hwmon_base = Path::new("/sys/class/hwmon");
    if let Ok(entries) = fs::read_dir(hwmon_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            let hwmon_name = fs::read_to_string(path.join("name"))
                .ok()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            // Read temp*_input files
            for i in 1..=8u8 {
                let temp_file = path.join(format!("temp{i}_input"));
                let label_file = path.join(format!("temp{i}_label"));
                if !temp_file.exists() { continue; }

                let temp_raw = fs::read_to_string(&temp_file)
                    .ok()
                    .and_then(|s| s.trim().parse::<i64>().ok());

                let label = fs::read_to_string(&label_file)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| format!("{hwmon_name}/{i}"));

                if let Some(t) = temp_raw {
                    // Avoid duplicates with thermal_zone (rough dedup by name)
                    if !zones.iter().any(|z| z.name == label) {
                        zones.push(ThermalZone {
                            name: label,
                            temp_c: t as f32 / 1000.0,
                        });
                    }
                }
            }
        }
    }

    zones.sort_by(|a, b| a.name.cmp(&b.name));
    zones
}

fn guess_cpu_temp(zones: &[ThermalZone]) -> Option<f32> {
    // Priority: x86_pkg_temp > coretemp > acpitz (often inaccurate) > first zone
    let priority_names = ["x86_pkg_temp", "Package id 0", "coretemp", "Tdie", "Tctl", "k10temp", "acpitz"];
    for name in &priority_names {
        if let Some(z) = zones.iter().find(|z| z.name.to_lowercase().contains(&name.to_lowercase())) {
            return Some(z.temp_c);
        }
    }
    zones.first().map(|z| z.temp_c)
}

fn guess_gpu_temp(zones: &[ThermalZone]) -> Option<f32> {
    let gpu_names = ["nouveau", "amdgpu", "nvidia", "gpu", "AMDGPU", "Tdie"];
    for name in &gpu_names {
        if let Some(z) = zones.iter().find(|z| z.name.to_lowercase().contains(&name.to_lowercase())) {
            return Some(z.temp_c);
        }
    }
    None
}

fn read_meminfo(info: &mut SystemInfo) {
    let Ok(content) = fs::read_to_string("/proc/meminfo") else { return };
    let mut total: Option<u64> = None;
    let mut avail: Option<u64> = None;

    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            total = parse_kb_line(line);
        } else if line.starts_with("MemAvailable:") {
            avail = parse_kb_line(line);
        }
        if total.is_some() && avail.is_some() { break; }
    }

    if let (Some(t), Some(a)) = (total, avail) {
        info.mem_total_mb     = Some(t / 1024);
        info.mem_available_mb = Some(a / 1024);
        info.mem_used_mb      = Some((t - a) / 1024);
    }
}

fn parse_kb_line(line: &str) -> Option<u64> {
    // "MemTotal:      16384 kB"
    line.split_whitespace().nth(1)?.parse().ok()
}

fn read_ac_online() -> bool {
    let base = Path::new("/sys/class/power_supply");
    let Ok(entries) = fs::read_dir(base) else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        let type_file = path.join("type");
        let online_file = path.join("online");
        if let Ok(t) = fs::read_to_string(&type_file) {
            if t.trim() == "Mains" {
                if let Ok(v) = fs::read_to_string(&online_file) {
                    return v.trim() == "1";
                }
            }
        }
    }
    false
}
