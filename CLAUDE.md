# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`ghelper-linux` is a Rust TUI application for ASUS ROG/TUF laptops on Linux. It manages battery charge limits and displays live system stats (CPU/GPU temps, memory). It interacts with the Linux sysfs interface (`/sys/class/power_supply/`, `/sys/class/thermal/`) and integrates with systemd and udev for persistence.

## Commands

```bash
# Build
cargo build --release          # Output: target/release/ghelper-linux

# Run locally
./target/release/ghelper-linux

# Check/lint
cargo check
cargo clippy

# Release (triggers CI via GitHub Actions)
git tag v0.1.0 && git push --tags
```

There are no automated tests in this project. Validation is manual.

## Architecture

```
main.rs → app.rs::run() (event loop + rendering)
              ├── battery.rs (BatteryManager)  → sysfs /sys/class/power_supply/
              ├── gpu.rs     (GpuManager)       → supergfxctl + /sys/firmware/acpi/platform_profile
              ├── system.rs  (SystemInfo)       → /proc/cpuinfo, /proc/meminfo, /sys/class/thermal/
              └── config.rs  (Config)           → ~/.config/ghelper-linux/config.json
```

### Key modules

**`src/app.rs`** — TUI state machine. `TuiApp` holds all state: `battery`, `gpu`, `system`, `config`, `active_tab` (0=battery 1=system 2=gpu 3=settings), `focus`, `desired_limit`, `status`. `run()` polls keyboard every 250ms and refreshes data on a configurable interval. Rendering uses `ratatui`. Key dispatch: `handle_key()` (global), `battery_key()`, `gpu_key()`, `settings_key()`.

**`src/battery.rs`** — All sysfs reads/writes. `BatteryManager` finds the battery device (BAT0/BAT1/BATC/BATT), reads capacity/status/voltage/energy/cycle count, and writes to `charge_control_end_threshold`. `set_charge_limit()` tries a direct write first, falls back to `pkexec tee` for privilege escalation. `run_setup()` embeds a shell script that creates a udev rule (makes sysfs file world-writable) and a systemd service (re-applies limit on boot/resume).

**`src/system.rs`** — Parses `/proc/cpuinfo`, `/proc/meminfo`, and all `/sys/class/thermal/thermal_zone*` entries. CPU/GPU temps are heuristically identified from zone type names.

**`src/gpu.rs`** — GPU mode switching and power profiles. `GpuManager` reads the current GPU mode via `supergfxctl -g` and switches with `supergfxctl -s <mode>` (Integrated/Hybrid/Discrete). Power profiles use `/sys/firmware/acpi/platform_profile` (low-power/balanced/performance) with a `pkexec tee` fallback for permission elevation. GPU mode changes take effect after logout (`needs_logout` flag). Both have a `mode`/`pending_mode` pattern — user selects pending, apply writes it.

**`src/config.rs`** — Loads/saves JSON to `~/.config/ghelper-linux/config.json`. Fields: `charge_limit` (default 80), `auto_apply_on_start`, `persistent_limit`, `refresh_secs` (default 5).

### Privilege model

- Writing `charge_control_end_threshold` requires root by default.
- After `setup.sh` runs (or the app's `s` key invokes it), a udev rule makes the file world-writable, removing the need for sudo on every write.
- The app tries direct write → falls back to `pkexec tee` → surfaces error in status bar.

### CI/CD

`.github/workflows/release.yml` builds static musl binaries for `x86_64` and `aarch64` on version tags and uploads them as GitHub Release assets.
