# ghelper-linux

**Still under development

A terminal UI (TUI) alternative to [GHelper](https://github.com/seerge/g-helper) for ASUS ROG / TUF laptops on Linux (Pop!_OS, Ubuntu, Arch, etc.).

**Primary feature: battery charge cap** — stop charging at 60 / 80 / 100% to extend long-term battery health.

```
  ghelper linux  ·  bat 85%  ·  cpu 45°c                          ac

▶ battery  │  battery status
  system   │  ████████████████████████░░░░░░  85%  Charging
  settings │
           │  voltage 12.1V  current 2.3A  power 27.8W  to full 1h 20m
           │
           │  charge limit
           │  ████████████████░░░░░░░░░░░░░░  80%  ←→ adjust  shift+←→ ±5
           │
           │  presets  [80%]  60%  100%     1 / 2 / 3
           │
           │  [a/↵] apply   [s] setup persist
           │
           │  details
           │  energy now 61.6Wh  full 72.5Wh  health 95%
           │  cycles 127  tech Li-ion  device BAT0  persistent active

  q quit   tab→sidebar   ←→ limit   a apply   s setup   r refresh
```

---

## Features

- **Battery charge cap** — set a charge limit (20–100%) to protect battery longevity
- **Persistent limit** — survives reboots via systemd service + udev rule (one-time setup)
- **Live stats** — voltage, current, power draw, time to full/empty, cycle count, health %
- **System info** — CPU & GPU temperatures, memory usage, all thermal zones
- **Keyboard-driven** — full navigation without a mouse
- **Lightweight** — ~800 KB static binary, no desktop environment required

---

## Requirements

- Linux (Pop!_OS, Ubuntu 22.04+, Arch, Fedora, etc.)
- ASUS ROG / TUF / VivoBook laptop
- Kernel module `asus-nb-wmi` loaded (usually automatic on ASUS hardware)
- Linux kernel **5.4+** (5.9+ recommended for full battery support)

Verify your kernel supports battery charge control:
```bash
ls /sys/class/power_supply/BAT*/charge_control_end_threshold
```
If the file exists, you're good to go.

---

## Installation

### Option 1 — Download binary (fastest)

Grab the latest release from the [Releases page](https://github.com/YOUR_USERNAME/ghelper-for-linux/releases):

```bash
# Download and extract
tar -xzf ghelper-linux-v0.1.0-x86_64-unknown-linux-musl.tar.gz

# Install system-wide
sudo cp ghelper-linux-*/ghelper-linux /usr/local/bin/ghelper
```

### Option 2 — Install via cargo

```bash
cargo install ghelper-linux
```

Then run with:
```bash
ghelper-linux
# or if you want a shorter alias:
alias ghelper='ghelper-linux'
```

### Option 3 — Build from source

```bash
git clone https://github.com/YOUR_USERNAME/ghelper-for-linux
cd ghelper-for-linux
cargo build --release
sudo cp target/release/ghelper-linux /usr/local/bin/ghelper
```

---

## First-time Setup

The battery charge limit file requires root to write. Run the one-time setup to configure:
- A **udev rule** — makes the sysfs file writable by your user after each boot
- A **systemd service** — re-applies your chosen limit after boot and resume from suspend

**Option A — inside the app:**
Launch `ghelper`, navigate to **battery** tab, press `Tab` to focus content, then press `s`.

**Option B — run directly:**
```bash
sudo bash /path/to/ghelper-for-linux/setup.sh 80
# Replace 80 with your desired charge limit %
```

After setup, the app can set the charge limit without any password prompt.

---

## Usage

```bash
ghelper        # if installed to /usr/local/bin
ghelper-linux  # if installed via cargo
```

### Keybindings

| Key | Action |
|-----|--------|
| `Tab` | Toggle sidebar ↔ content focus |
| `j` / `k` or `↑` / `↓` | Navigate sidebar |
| `↵` or `→` | Enter content from sidebar |
| `Esc` | Back to sidebar |
| `←` / `→` | Adjust charge limit (battery tab) |
| `Shift+←` / `Shift+→` | Adjust by ±5% |
| `1` / `2` / `3` | Preset to 60% / 80% / 100% |
| `a` or `↵` | Apply charge limit |
| `s` | Setup persistence (or update persistent limit) |
| `r` | Force refresh |
| `q` | Quit |

---

## How battery charge limiting works

The Linux kernel exposes a sysfs interface for ASUS laptops:

```
/sys/class/power_supply/BAT0/charge_control_end_threshold
```

Writing a value (e.g. `80`) stops the battery from charging above that percentage.
This is the same mechanism used by ASUS's own tools and GHelper on Windows.

The value resets on reboot unless persisted — which is what the setup script handles.

---

## Supported hardware

Any ASUS laptop supported by the `asus-nb-wmi` kernel module, including:

- ROG Zephyrus (G14, G15, G16, M16, Duo)
- ROG Flow (X13, X16, Z13)
- ROG Strix (G513, G533, G733, etc.)
- TUF Gaming (A15, A17, F15, F17)
- VivoBook / ProArt (models with asus-nb-wmi support)

If your model isn't listed but has `charge_control_end_threshold` in sysfs, it will work.

---

## Contributing

PRs welcome. Ideas for future features:
- Fan curve control
- Performance mode switching (via platform_profile)
- GPU mode switching (Eco / Standard / Ultimate)
- Keyboard backlight control

---

## License

MIT — see [LICENSE](LICENSE)

---

*Inspired by [GHelper](https://github.com/seerge/g-helper) for Windows by seerge.*
