# USB Passthrough — Cross-Platform USB/IP App

**Pass any USB device over the network between Android, Android TV, and Windows with sub-millisecond latency.**

Cross-platform USB/IP bridge. Pass any USB device over the network — keyboards, gamepads, racing wheels, flash drives, USB-to-serial adapters, and more.

---

## What This Is

A fast, service-mode USB/IP bridge. Plug a USB device into one machine, use it on another as if it were physically connected. Built on the USB/IP kernel protocol (RFC-compliant, see [PROTOCOL.md](PROTOCOL.md)).

### Real-World Use Case

```
┌─────────────────────┐          ┌──────────────────────┐
│  Android TV (Client) │          │  Android Phone (Server)│
│  Xbox Cloud / PC Game  │  ◄─────► │  USB Device plugged in │
│  "sees" remote USB     │  Wi-Fi   │  USB Host Mode         │
└─────────────────────┘          └──────────────────────┘
```

The device's native drivers, force feedback, and all features — work exactly as if locally connected.

---

## Supported Platforms

| Platform         | Server (export) | Client (import) | Service Mode |
|------------------|:---------------:|:---------------:|:------------:|
| **Windows 10/11**| ✅              | ✅              | ✅ Windows Service |
| **Android 9+**   | ✅ USB Host     | ✅ VHCI module  | ✅ Foreground Service |
| **Android TV 9+**| ⚠️ Limited      | ✅              | ✅ Foreground Service |
| **Linux**        | ✅ (usbip-host) | ✅ (vhci-hcd)   | ✅ systemd |

---

## Quick Start

### Windows → Windows (fastest setup)

```powershell
# Install (requires admin)
winget install USB-Passthrough

# Server (machine with USB device — keyboard, gamepad, flash drive, etc.)
usb-passthrough serve --device "My Keyboard"

# Client (gaming machine)
usb-passthrough connect --server 192.168.1.100 --device "My Keyboard"
```

### Android Phone → Android TV

```bash
# Phone: Install APK, plug in USB device via USB-C hub
# TV: Install Android TV APK
# Both: Open app, devices auto-discover via mDNS
# Tap USB device on TV → connected
```

---

## Key Features

- **True USB passthrough**, not HID emulation — force feedback, gamepad rumble, pedals, all work
- **USB/IP protocol** — same protocol the Linux kernel uses, battle-tested since 2008
- **mDNS discovery** — no IP addresses needed, devices find each other automatically
- **AES-256-GCM encryption** — optional, for traversing untrusted networks
- **Sub-1ms per-URB latency** on wired Ethernet, 2-5ms on Wi-Fi 6
- **Service mode** — runs headless, survives reboots, no UI needed after setup
- **Android TV optimized** — D-pad navigable, 10-foot UI, remote-friendly
- **Auto-reconnect** — survives network flaps, device unplug/re-plug cycles

---

## Architecture Overview

See [ARCHITECTURE.md](ARCHITECTURE.md) for full details.

```
┌────────────────────────────────────────────┐
│                 Applications                │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐ │
│  │ Android  │  │ Android  │  │ Windows  │ │
│  │  (phone) │  │   (TV)   │  │   (PC)   │ │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘ │
│       │              │              │       │
│  ┌────┴──────────────┴──────────────┴────┐  │
│  │        Rust USB/IP Core (shared)      │  │
│  │   Protocol · URB · Descriptors · mDNS │  │
│  └────────────────┬──────────────────────┘  │
│                   │                         │
│  ┌────────────────┴──────────────────────┐  │
│  │         Platform USB Stack            │  │
│  │  libusb / WinUSB / Android USB Host   │  │
│  └───────────────────────────────────────┘  │
└────────────────────────────────────────────┘
```

---

## Latency Budget

```
HID URB round-trip on gigabit Ethernet:

  App → Kernel (ioctl):        ~10 µs
  Kernel → TCP send:           ~20 µs
  Network traversal (LAN):     ~200 µs
  TCP receive → Kernel:        ~20 µs
  Kernel → USB controller:     ~50 µs
  Hardware response:           ~100 µs
  Return path:                 ~300 µs
  ─────────────────────────────────────
  Total RTT:                   ~700 µs
```

Force feedback at 250 Hz update rate requires <4ms latency. We have 5x headroom on Ethernet, 2x on good Wi-Fi.

---

## Building From Source

```bash
# Prerequisites: Rust 1.78+, Android SDK 34+, JDK 17+

# Clone
git clone https://github.com/stanvx/usb-passthrough
cd usb-passthrough

# Build all Rust crates
cargo build --release

# Build Android (from android/ directory)
cd android
./gradlew assembleRelease

# Build Windows installer
cd windows/installer
makensis installer.nsi
```

See [docs/BUILDING.md](docs/BUILDING.md) for detailed platform-specific instructions.

---

## Documentation Index

| Document | What's Inside |
|----------|---------------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Full system design, data flow, thread model |
| [PROTOCOL.md](PROTOCOL.md) | USB/IP wire protocol reference |
| [docs/SETUP.md](docs/SETUP.md) | Step-by-step setup per platform |
| [docs/G920-SPECIFIC.md](docs/G920-SPECIFIC.md) | Reference device example — G920 quirks, force feedback, known issues |
| [docs/ANDROID-TV.md](docs/ANDROID-TV.md) | TV-specific setup, sideloading, remote navigation |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Tuning, buffer sizes, Wi-Fi vs Ethernet |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Common issues and fixes |
| [docs/BUILDING.md](docs/BUILDING.md) | Compile from source |

---

## License

MIT — see [LICENSE](LICENSE).

## Status

**Alpha.** Core protocol works. Android TV client needs VHCI kernel module on rooted devices. See [ROADMAP.md](ROADMAP.md) for what's coming.
