# AnyPlug — Architecture

## System Overview

```
                         NETWORK (LAN / Wi-Fi Direct)
    ┌──────────────────────────────────────────────────────────┐
    │                    USB/IP over TCP (port 3240)            │
    │              AES-256-GCM (optional, configurable)         │
    │                                                          │
    │  ┌──────────────────────┐       ┌──────────────────────┐ │
    │  │   mDNS advertiser    │       │    mDNS browser      │ │
    │  │   _usbip._tcp.local  │       │   _usbip._tcp.local  │ │
    │  └──────────────────────┘       └──────────────────────┘ │
    └──────────────────────────────────────────────────────────┘

 ┌─────────────────────────┐           ┌─────────────────────────┐
 │     SERVER (export)     │           │     CLIENT (import)     │
 │                         │           │                         │
 │  ┌───────────────────┐  │           │  ┌───────────────────┐  │
 │  │  usbip-server     │  │           │  │  usbip-client     │  │
 │  │                   │  │           │  │                   │  │
 │  │  Device Manager ──┼──┼── TCP ───┼──│  VHCI Driver      │  │
 │  │       │           │  │           │  │       │           │  │
 │  │  URB Forwarder    │  │           │  │  URB Receiver     │  │
 │  │       │           │  │           │  │       │           │  │
 │  │  libusb / USB     │  │           │  │  Kernel vhci-hcd  │  │
 │  │  Host API         │  │           │  │  (virtual USB HC) │  │
 │  └───────┼───────────┘  │           │  └───────┼───────────┘  │
 │          │              │           │          │              │
 │  ┌───────┴───────────┐  │           │  ┌───────┴───────────┐  │
 │  │  Physical USB HC  │  │           │  │  OS USB Stack     │  │
 │  │  (xhci / ehci)    │  │           │  │  (usbcore)        │  │
 │  └───────┬───────────┘  │           │  └───────┬───────────┘  │
 │          │              │           │          │              │
 │     ┌────┴────┐         │           │     ┌────┴────┐         │
 │     │  USB    │         │           │     │  Driver │         │
 │     │  Device │         │           │     │  (HID/  │         │
 │     └─────────┘         │           │     │  FFB)   │         │
 └─────────────────────────┘           └─────────────────────────┘
```

## Thread Architecture

### Server Threads

```
main thread
  ├── mDNS advertisement thread (publishes _usbip._tcp.local)
  ├── TCP accept thread (accepts client connections)
  │     └── per-client connection thread
  │           ├── URB receive thread (reads USB/IP commands from client)
  │           ├── URB submit thread (forwards to physical USB device)
  │           └── URB reply thread (sends responses back to client)
  └── device monitor thread (libusb hotplug callbacks)
```

### Client Threads

```
main thread
  ├── mDNS discovery thread (browses _usbip._tcp.local)
  ├── TCP connect thread (connects to server)
  │     └── per-connection thread
  │           ├── URB send thread (USB/IP commands to server)
  │           ├── URB receive thread (responses from server)
  │           └── VHCI dispatch thread (submits URBs to kernel vhci-hcd)
  └── VHCI event thread (kernel URB completion callbacks)
```

## Data Flow: A Single URB

```
TIME ──────────────────────────────────────────────────────────►

CLIENT APP                    SERVER APP                  PHYSICAL USB
    │                             │                             │
    │  Game calls                  │                             │
    │  DeviceIoControl()           │                             │
    │         │                    │                             │
    │    ┌────▼────┐               │                             │
    │    │ Windows │               │                             │
    │    │ Kernel  │               │                             │
    │    │ vhci-hcd│               │                             │
    │    └────┬────┘               │                             │
    │         │                    │                             │
    │    OP_REQ_SUBMIT             │                             │
    │    (USBIP_CMD_SUBMIT)        │                             │
    │─────────┬───────────────────►│                             │
    │         │              ┌─────▼──────┐                      │
    │         │              │ Parse URB  │                      │
    │         │              │ Validate   │                      │
    │         │              └─────┬──────┘                      │
    │         │                    │ libusb_submit_transfer()    │
    │         │                    ├─────────────────────────────►
    │         │                    │                              │
    │         │                    │         USB completion       │
    │         │                    │◄─────────────────────────────
    │         │              ┌─────▼──────┐                      │
    │         │              │ Build URB  │                      │
    │         │              │ reply      │                      │
    │         │              └─────┬──────┘                      │
    │         │                    │                             │
    │    OP_REP_SUBMIT            │                             │
    │    (USBIP_RET_SUBMIT)       │                             │
    │◄────────┬───────────────────┤                             │
    │         │                    │                             │
    │    ┌────▼────┐               │                             │
    │    │ Windows │               │                             │
    │    │ delivers│               │                             │
    │    │ to game │               │                             │
    │    └─────────┘               │                             │
```

## Platform-Specific Details

### Windows

```
┌───────────────────────────────────────┐
│          anyplug.exe          │
│                                       │
│  Server:                              │
│    libusb (via WinUSB/libusbK)        │
│    ⇩                                   │
│    Windows USB stack (usbhub.sys,     │
│    usbport.sys, xhci.sys)             │
│                                       │
│  Client:                              │
│    usbip-win2 kernel driver           │
│    ⇩                                   │
│    vhci.sys (virtual host controller) │
│    ⇩                                   │
│    Windows PnP stack auto-detects     │
│    imported device, loads driver      │
└───────────────────────────────────────┘
```

Windows Server uses libusb with WinUSB backend (`libusbK` driver). The device must be "freed" from Windows first (no driver attached) so libusb can claim it. This is done via `usbipd-win`'s `bind` command pattern — we detach the Windows driver and attach WinUSB.

Windows Client uses `usbip-win2` (vadimgrn) as the VHCI kernel driver, which creates a virtual USB host controller that Windows sees as real hardware. The client app communicates with this driver via IOCTLs.

### Android

```
┌───────────────────────────────────────┐
│       Android App (Kotlin)            │
│                                       │
│  Server:                              │
│    Android USB Host API               │
│    ⇩                                   │
│    UsbManager.openDevice()            │
│    UsbDeviceConnection.bulkTransfer()  │
│    UsbDeviceConnection.controlTransfer()│
│                                       │
│  Client:                              │
│    VHCI kernel module (root required) │
│    or userspace /dev/usbip-vhci       │
│    ⇩                                   │
│    Linux usbcore on Android kernel    │
└───────────────────────────────────────┘
```

Android server uses the USB Host API, available without root on Android 3.1+. The app claims the USB device and proxies every `bulkTransfer()` / `controlTransfer()` call over TCP.

Android client requires a VHCI kernel module. On rooted devices, we load `vhci-hcd.ko` and `usbip-core.ko`. On non-rooted Android TV devices, we use a userspace `/dev/usbip-vhci` character device (custom kernel driver required, typically via custom ROM or kernel module sideload).

### Android TV

Android TV is the same APK with a different UI module. The `tv/` module provides:
- Leanback theme (horizontal browse fragments)
- D-pad navigation (no touch required)
- Large text (10-foot UI)
- Simplified setup wizard
- Voice search integration

## Security Model

```
┌───────────────────────────────────────┐
│        Security Layers                │
│                                       │
│  1. Network isolation (optional)      │
│     └─ Bind to specific interface     │
│                                       │
│  2. AES-256-GCM tunnel (optional)     │
│     └─ Pre-shared key or QR code      │
│        pairing                        │
│                                       │
│  3. Device allowlisting               │
│     └─ Only export whitelisted VID:PID│
│                                       │
│  4. Connection confirmation           │
│     └─ UI prompt: "Allow AndroidTV    │
│        to access USB Device?"          │
└───────────────────────────────────────┘
```

## Client VHCI Backend Abstraction

The client crate abstracts the platform VHCI surface behind a single trait so
the rest of the client logic (TCP connect, URB dispatch, reconnection) is
identical across Linux, Windows, and (eventually) macOS DriverKit.

- `vhci::VhciBackend` (`client/usbip-client/src/vhci/mod.rs`) is `pub` and
  re-exported at the crate root as `usbip_client::VhciBackend`. It exposes
  `create_device`, `remove_device`, `submit_urb`, `complete_urb`, and
  `cancel_urb` — the same set of operations the macOS DriverKit `IOUserClient`
  interface needs (see ADR-004).
- `Client::new(config)` calls `vhci::detect_backend()` internally to select
  the platform backend (`VhciLinux` or `VhciWindows`) at runtime. No
  compile-time platform gate.
- `Client::new_with_vhci(config, vhci: Arc<dyn VhciBackend>)` is the test
  injection point. It accepts any `Arc<dyn VhciBackend>` and bypasses
  `detect_backend()`, so integration tests can run on any host (CI on Linux
  runners can exercise Windows/macOS codepaths with a mock backend).
- The canonical wire-encode helper for `USBIP_RET_SUBMIT` messages is
  `usbip_core::reply::serialize_ret_submit(ret, data) -> Vec<u8>` in
  `shared/usbip-core/src/reply.rs`. All callers (server, batcher, test
  fixtures) go through this single function.

**Test surface:**

- `client/usbip-client/tests/injection_seam.rs` — integration test proving
  `VhciBackend` and `new_with_vhci` are reachable through the crate's
  public API. Will not compile if either regresses to `pub(crate)`.
- Inline `#[cfg(test)] mod tests` in `client.rs` and `vhci/mod.rs` cover
  backend selection, mock backend observation (`MockVhciBackend::recorded_urbs()`),
  and lifecycle behaviour.

This is the precondition for ADR-004 (macOS DriverKit VHCI): the macOS
backend is a third `impl VhciBackend`, slotted in without touching
client-level code.

## Configuration File

```toml
# ~/.config/anyplug/config.toml
# or /sdcard/Android/data/com.anyplug/files/config.toml

[server]
bind_address = "0.0.0.0"
port = 3240
allowed_devices = ["1d6b:0104"]  # Linux Foundation HID keyboard (example — any VID:PID works)
require_confirmation = true

[client]
discovery = "mdns"              # "mdns" | "manual"
connection_timeout_secs = 10
reconnect_attempts = 3
reconnect_delay_ms = 1000

[encryption]
enabled = true
# psk = "auto"                  # "auto" (QR code) | "file:/path" | hex string
key_derivation = "hkdf-sha256"

[performance]
urb_pool_size = 64              # pre-allocated URB buffers
tcp_nodelay = true              # disable Nagle's algorithm
recv_buffer_size = 262144       # 256 KB socket recv buffer
send_buffer_size = 262144       # 256 KB socket send buffer
max_transfer_size = 65536       # max USB transfer size

[android]
foreground_service = true
wakelock = true                 # keep CPU awake during transfers
battery_optimization_bypass = true
```

## Dependency Map

```
usbip-core (Rust, no_std capable)
├── byteorder
├── crc32fast
├── zerocopy (safe transmutes)
└── thiserror

usbip-server (Rust)
├── usbip-core
├── libusb (via rusb)
├── mdns-sd
├── tokio (async runtime)
├── tracing (structured logging)
└── ring (AES-GCM)

usbip-client (Rust)
├── usbip-core
├── mdns-sd
├── tokio
├── tracing
└── ring (AES-GCM)

usbip-android (Rust JNI)
├── usbip-core
├── jni crate
└── android_logger

Android App (Kotlin)
├── Jetpack Compose
├── Android USB Host API
├── JNI → usbip-android
├── JmDNS (mDNS for Android)
└── DataStore (preferences)

Windows App (Rust)
├── usbip-server (library)
├── usbip-client (library)
├── egui + eframe
├── windows-service
├── winapi (SetupAPI, IOCTL)
└── tray-icon
```
