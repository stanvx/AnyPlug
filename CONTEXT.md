# anyplug

A cross-platform USB/IP bridge that exports a physical USB device on one machine and imports it on another as if it were locally attached. Built around the USB/IP kernel protocol (RFC-compliant, see PROTOCOL.md).

## Language

**Passthrough**:
True USB passthrough — the device's native descriptors and endpoints are forwarded byte-for-byte over USB/IP, so the OS on the importing side loads the device's real driver. Force feedback, vendor-specific reports, bulk-only storage protocols, and CDC-ACM all work because the device is *not* re-emulated.
_Avoid_: Emulation, redirection, HID proxying, virtualisation

**Server**:
The machine that has the USB device physically attached and exports it over USB/IP. Runs the libusb / WinUSB / Android USB Host backed device-export side.
_Avoid_: Host, exporter, source

**Client**:
The machine that imports an exported USB device and presents it to its local OS as if it were physically attached. On Linux this uses vhci-hcd; on Windows it uses WinUSB; on Android it uses the VHCI module (rooted) or a uinput fallback.
_Avoid_: Guest, importer, consumer, target

**Device class scope (v1.0)**:
HID (keyboards, mice, gamepads, FFB wheels), USB mass storage, USB-to-serial, printers, scanners, and any bulk-only device. Isochronous transfers (USB audio, webcams, full-speed FFB) are explicitly out of scope for v1.0.
_Avoid_: "Works with anything USB" (too broad — implies isoch support), "any USB device" (same)

**G920 debt**:
Code in the shared core that encodes assumptions specific to the Logitech G920 racing wheel (VID/PID constants, FFB command bytes, endpoint layouts). The G920 is the original reference device but the project is no longer G920-specific; any G920-shaped code in `shared/usbip-core/` is a bug to be removed, not a feature to be extended.
_Avoid_: "G920 support" (the project supports arbitrary HID, not a specific wheel), "Logitech quirks" (those are device-profile data, not core logic)

Status: resolved as of 2026-06. The `shared/usbip-core/src/g920.rs` module was deleted in an earlier commit; G920 constants, `is_g920` field, and helper functions in `windows/src/windows_usb.rs` were removed in the architecture deepening pass. No G920-specific code remains in generic infrastructure.

**Test rig**:
The end-to-end test harness built on Linux's configfs + dummy_hcd/udc gadget subsystem, booted in QEMU on cloud CI runners (GitHub Actions `ubuntu-latest`). A minimal Linux kernel with USB gadget drivers compiled in presents a software USB device (HID keyboard, mass storage, CDC-ACM) to the host, and the project's own server + client connect to it over loopback. CI proves the tool works with arbitrary USB devices across interrupt, bulk, and control transfer classes.
_Avoid_: Mock device (too narrow — implies a userspace stub), emulator (ambiguous with VM emulation)

**Reliability primitives**:
The three non-negotiable capabilities for v1.0: structured errors with correlation IDs and exportable logs, hot-plug detection (device attach/detach after server start), and auto-reconnect (survive network flaps and server restarts). Session persistence is explicitly deferred to a later phase.
_Avoid_: Resilience, fault tolerance, recovery (too vague — the project is specific about *which* failures are handled)

**Ecosystem integration**:
Packaging and distribution of existing binaries onto community platforms via their native mechanisms. No new code — the server and client already work; integration means making them discoverable and installable on the target platform (RetroPie scriptmodule, Lakka package, Steam Link / Moonlight companion setup).
_Avoid_: Feature development, new UI, protocol changes (none of these are needed for packaging)

**RetroPie / Lakka integration**:
The RetroPie or Lakka device runs `usbip-server` to export locally-attached controllers (gamepads, fight sticks, FFB wheels) and `usbip-client` to import remotely-attached devices. Delivered as a RetroPie scriptmodule or Lakka package — no new code, only distribution.
_Avoid_: "RetroPie support", "Lakka support" (ambiguous — implies new features)

**Steam Link / Moonlight companion**:
The streaming-client device (Raspberry Pi at the TV running Steam Link or Moonlight) runs `usbip-server` to export locally-attached controllers to the gaming PC. The gaming PC runs `usbip-client` to import them so the game sees them as locally attached. Same architecture, deployment-specific packaging.
_Avoid_: "Steam integration" (too broad — not a Steam plugin), "Moonlight plugin" (it's a companion service, not a Moonlight fork)

**Client daemon**:
The client running as a persistent background service that auto-starts on boot, auto-connects to configured servers/devices, and survives login/logout. On Linux: a systemd unit with a local control socket so `usbip-client --status` and `usbip-client --disconnect <device>` talk to the running process. On Windows: a Windows Service (already built). On Android: a foreground service (already built). The client daemon is a v1.0 requirement — without it, ecosystem integrations (RetroPie, Steam Link) collapse because the target device boots headless with no user session.
_Avoid_: Client service (ambiguous — "service" already means too many things), background mode (too vague)

**Embedded server recipe**:
A documented procedure for setting up `usbip-server` on a Raspberry Pi or comparable SBC (single-board computer) to create a dedicated USB device exporter. Flash a standard OS image (Raspberry Pi OS), install the binary, configure which devices to export, and enable the systemd unit. The result is a headless USB-over-network appliance — same outcome as VirtualHere CloudHub, achieved with commodity hardware and a stock OS rather than custom firmware. A Buildroot/Yocto firmware image is deferred post-v1.0.
_Avoid_: CloudHub clone (implies custom firmware — the recipe uses stock OS), embedded firmware (overstates the deliverable)

**Service mode**:
A headless runtime that survives reboots and requires no UI after initial setup. On Windows: a Windows Service. On Android: a foreground service with a wake lock. On Linux: a systemd unit. Applies to both server and client. The presence of a GUI is a *companion* to service mode, not a replacement for it.
_Avoid_: Daemon (Unix-specific connotation; the project is cross-platform), background app (ambiguous about lifecycle)
