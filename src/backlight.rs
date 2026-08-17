use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const LED_DIR: &str = "/sys/class/leds/asus::kbd_backlight";

pub struct KbdBacklight {
    pub available:    bool,
    pub max_level:    u8,
    /// Currently active brightness level (as read from sysfs)
    pub level:        u8,
    /// User-selected (pending) level — may differ before apply
    pub pending_level: u8,
    brightness_path:  PathBuf,
}

impl KbdBacklight {
    pub fn new() -> Self {
        let dir = Path::new(LED_DIR);
        let brightness_path = dir.join("brightness");
        let max_path = dir.join("max_brightness");

        let available = brightness_path.exists() && max_path.exists();

        let max_level = if available {
            fs::read_to_string(&max_path)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        } else {
            0
        };

        let level = read_level(&brightness_path);

        Self {
            available,
            max_level,
            level,
            pending_level: level,
            brightness_path,
        }
    }

    pub fn refresh(&mut self) {
        if self.available {
            self.level = read_level(&self.brightness_path);
        }
    }

    /// Write the pending brightness level to sysfs, falling back to pkexec on permission error.
    pub fn apply(&mut self) -> Result<(), String> {
        if !self.available {
            return Err("Keyboard backlight not found on this device.".to_string());
        }

        let value = self.pending_level.to_string();

        match fs::write(&self.brightness_path, &value) {
            Ok(_) => { self.level = self.pending_level; return Ok(()); }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(e) => return Err(format!("Write error: {e}")),
        }

        write_privileged(&self.brightness_path.to_string_lossy(), &value)?;
        self.level = self.pending_level;
        Ok(())
    }
}

fn read_level(brightness_path: &Path) -> u8 {
    fs::read_to_string(brightness_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn write_privileged(path: &str, value: &str) -> Result<(), String> {
    let mut child = Command::new("pkexec")
        .args(["tee", path])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch pkexec: {e}"))?;

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
            "Permission denied.\n{}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}
