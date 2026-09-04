# anyplug

A cross-platform USB/IP bridge that exports a physical USB device on one machine and imports it on another as if it were locally attached. Built around the USB/IP kernel protocol (RFC-compliant, see PROTOCOL.md).

## Language

**Passthrough**:
Byte-for-byte forwarding of native USB descriptors and endpoints over USB/IP.
The importing OS loads the real driver, so FFB, vendor reports, bulk storage,
and CDC-ACM all work — the device is not re-emulated.
_Avoid_: Emulation, redirection, HID proxying, virtualisation

**Server**:
Machine with the physically attached USB device, exporting it over USB/IP
via libusb / WinUSB / Android USB Host.
_Avoid_: Host, exporter, source

**Client**:
Machine that imports an exported USB device and presents it as locally attached.
Linux: vhci-hcd. Windows: WinUSB. Android: VHCI module (rooted) or uinput fallback.
_Avoid_: Guest, importer, consumer, target

**Device class scope (v1.0)**:
HID, mass storage, USB-to-serial, printers, scanners, bulk-only devices.
Isochronous (audio, webcams) is out of scope.
_Avoid_: "Works with anything USB" (implies isoch support)

**Test rig**:
QEMU-based E2E harness on cloud CI runners. Linux configfs + dummy_hcd/udc
presents software USB devices (HID, mass storage, CDC-ACM) over loopback
to the project's own server + client.
_Avoid_: Mock device (too narrow), emulator (ambiguous with VM)

**Reliability primitives**:
Three v1.0 requirements: structured errors with correlation IDs, hot-plug
detection, auto-reconnect. Session persistence is deferred.
_Avoid_: Resilience, fault tolerance, recovery (too vague)

**Ecosystem integration**:
Packaging existing binaries for community platforms via native mechanisms
(RetroPie scriptmodule, Lakka package, Steam Link/Moonlight companion).
No new code.
_Avoid_: Feature development, new UI, protocol changes

**RetroPie / Lakka integration**:
The RetroPie/Lakka device runs `usbip-server` to export controllers and
`usbip-client` to import remote devices. Distribution-only — no new code.
_Avoid_: "RetroPie support", "Lakka support" (implies new features)

**Steam Link / Moonlight companion**:
Streaming-client device (Pi at the TV) runs `usbip-server` to export
controllers to the gaming PC. The PC runs `usbip-client` to import them.
Same architecture, deployment-specific packaging.
_Avoid_: "Steam integration" (not a plugin), "Moonlight plugin" (companion service, not a fork)

**Client daemon**:
Persistent background service — auto-starts on boot, auto-connects,
survives login/logout. Linux: systemd unit with control socket.
Windows: Windows Service. Android: foreground service. Required for
headless ecosystem integrations.
_Avoid_: Client service (ambiguous), background mode (too vague)

**Embedded server recipe**:
Documented procedure for setting up `usbip-server` on a Raspberry Pi/SBC
as a headless USB-over-network appliance. Stock OS, no custom firmware.
Buildroot/Yocto image deferred post-v1.0.
_Avoid_: CloudHub clone (implies custom firmware), embedded firmware (overstates it)

**Service mode**:
Headless runtime that survives reboots with no UI after setup.
Windows Service / Android foreground service / systemd unit.
GUI is a companion, not a replacement.
_Avoid_: Daemon (Unix-specific), background app (ambiguous)

**Wire port** (default 3240):
TCP port carrying raw USB/IP protocol packets — URBs, descriptors,
isochronous data. The core passthrough data path between server and client.
_Avoid_: USB port, data port, main port

**API port** (default 3241):
TCP port for the REST API and WebSocket event stream (latency telemetry,
connection state). Only active when `--api-port` is set.
_Avoid_: Admin port, management port, HTTP port

**mDNS port** (default 5353):
UDP port for `_usbip._tcp.local` service advertisements. Link-local only —
does not cross subnets or VLANs.
_Avoid_: Discovery port, broadcast port

## Module ownership

This section names ONE owner per architectural concern. These claims are
authoritative; every other doc (CLAUDE.md, ARCHITECTURE.md, PROTOCOL.md,
ROADMAP.md, README.md, docs/*.md) must agree with them. When a doc disagrees,
the doc is wrong — fix the doc, not this section.

- **ConfigOwner**: `Server::app_state` (server/usbip-server/src/server.rs) —
  the one place that reads and writes the merged config (`api::load_config` /
  `AppState.config`). CLI flags in `main.rs` seed field values;
  `app_state` reconciles them with the persisted file.
- **MetricsOwner**: `server/usbip-server/src/metrics.rs` —
  `build_metrics_router` and the `ENCRYPTION_ENABLED` gauge are live (wired
  from `main.rs`) and are the ONLY metrics present. The former
  `DEVICES_EXPORTED` / `CLIENTS_CONNECTED` / `URB_SUBMIT_TOTAL` /
  `URB_BYTES_TOTAL` statics were DELETED (issue #31): they were never wired.
  If live export/connected/URB counters are ever needed again, add them at
  the single site that owns each event, not as dead statics.
- **VhciBackendOwner**: `Client` (client/usbip-client/src/client.rs) — the
  one place that constructs a `VhciBackend` (`detect_backend`),
  and the injection seam `Client::new_with_vhci`.
- **HotplugOwner**: DEFERRED (issue #29). The poll-based `HotplugMonitor`
  was DELETED (Path B); no v1.0 hotplug detection exists. ADR-0005 remains
  the v1.1 blueprint (libusb/callback-based). Any future monitor is owned by
  `Server` and must follow ADR-0005.
- **DiscoveryTxtOwner**: `usbip-core::discovery_txt` —
  `shared/usbip-core/src/discovery_txt.rs` owns the `device` TXT wire format
  (encode + decode: vid/pid/bus/name tuples, e.g.
  `vid=0x046d,pid=0xc261,bus=1-1,n=046d:c261`). The server
  (`discovery.rs`) advertises and delegates encode to it; the client
  (`usbip-client::discovery`) decodes via it (issue #30). One seam.
- **AndroidStateOwner**: single Kotlin class managing Android connection /
  runtime state — pending issue #32 (nominating the class is part of that work).
- **AndroidComposerOwner**: single Kotlin class composing the Compose UI
  tree (phone and TV) — pending issue #32.
- **AndroidControllerOwner**: single Kotlin class owning UI event dispatch
  and state mutation — pending issue #32.
