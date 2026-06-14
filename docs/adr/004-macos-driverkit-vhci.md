---
adr: 004
title: macOS Client via DriverKit Virtual USB Host Controller
status: proposed
date: 2026-06-14
---

# macOS Client via DriverKit Virtual USB Host Controller

## Context

macOS has no built-in Virtual Host Controller Interface (VHCI) equivalent to other
target platforms in this project. Each existing platform provides a kernel-level
mechanism that accepts USB/IP traffic and exposes imported devices to the OS
USB stack without custom kernel code from this project:

- **Linux:** `vhci-hcd` — a kernel module (drivers/usb/usbip/) that creates
  virtual root hub devices. The client (`usbip-core`) communicates with vhci-hcd
  via sysfs (`/sys/devices/platform/vhci_hcd.*`). This is the reference
  implementation for the USB/IP protocol.

- **Windows:** `usbip-win2` — a signed kernel driver (WDF KMDF) implementing a
  virtual USB host controller. The project's existing Windows client wraps it
  via `SetupAPI` and `DeviceIoControl` for device enumeration and URB submission.

- **Android (Linux-based):** The Linux `vhci-hcd` module is available on
  GKI-compatible kernels. On older TV/phone kernels the project bundles a
  standalone VHCI kernel module built against the target kernel headers.

macOS has no such facility. There is no VHCI driver in the macOS kernel,
no I/O Kit family for virtual USB host controllers, and no third-party
open-source equivalent (the `usbip` project on macOS was kext-based and
abandoned after macOS 10.15).

To support macOS as a client platform (per ADR-0004's v1.0 scope), the
project must build a custom virtual USB host controller driver. The only
modern, supported mechanism on macOS is DriverKit, introduced in macOS 11
(Big Sur) as the replacement for kernel extensions (kexts) on Apple Silicon.

## Decision

Use DriverKit to implement a virtual USB host controller interface as a
DriverKit extension (dext), paired with a user-space client daemon that
receives USB/IP traffic over TCP and forwards URBs into the dext via an
IOUserClient IPC channel.

The daemon and dext communicate over a custom IOUserClient interface
defined in the dext's IOKit personality. The daemon opens a service
connection to the dext, submits URBs as async calls, and receives
completion callbacks. The dext presents imported devices to the macOS
USB stack as `IOUSBHostDevice` objects in the IORegistry, making them
visible to user-space applications via `IOKit` and `USBDevice` APIs.

## Architecture

### Components

```
macOS Client Machine

  USB/IP TCP:3240 (from server)
         |
  +------v--------+
  |  Client       |  Swift daemon
  |  Daemon       |  - USB/IP protocol parsing
  |  (user-space) |  - AES-256-GCM / X25519 crypto
  |               |  - Compression
  |               |  - mDNS / REST discovery
  +------+--------+
         | IOUserClient IPC
         | (dext service connection)
  +------v--------+
  |  Virtual USB  |  DriverKit dext (C++)
  |  Host Ctrlr   |  - IOUSBHostDevice creation
  |  (dext)       |  - URB submission / completion
  |               |  - IORegistry device mapping
  +------+--------+
         |
  +------v--------+
  | macOS USB     |  IOKit / IOUSBFamily
  | Stack         |  - Device enumeration
  |               |  - Class drivers (HID, mass storage, etc.)
  +---------------+
```

### Client Daemon

The daemon is written in Swift using `swift-async-algorithms` and
`Network.framework` for async TCP I/O. Reasons for Swift over Rust
(which the rest of the project uses):

- DriverKit dexts implement a `IOService` C++ subclass. The daemon side
  of the IOUserClient channel is also C++/ObjC++ under IOKit.
- Swift interop with C/C++ headers is mature on Apple platforms and
  avoids the FFI gymnastics of calling IOKit from Rust.
- The daemon can vend a `XPCService` for launchd integration (auto-launch
  on boot, privilege separation).

A Rust daemon via `core-foundation` and `io-kit-rs` bindings was
considered but rejected (see Alternatives).

The daemon's responsibilities:

1. **Connection management:** TCP connect to the remote server, maintain
   keepalive, handle reconnection with exponential backoff (consistent
   with ADR-0003's auto-reconnect design).
2. **USB/IP protocol:** `OP_REQ_DEVLIST`, `OP_REQ_IMPORT`,
   `OP_REQ_UNLNK` / `OP_REQ_LNK` for URB submission, receive
   `OP_REP_DEVLIST` and URB completion messages. The Rust
   `usbip-core` crate is compiled as a static library and linked
   into the Swift daemon via a C FFI bridge (generated headers from
   `cbindgen`).
3. **Crypto/compression:** AES-256-GCM tunnel and X25519 key exchange
   (reusing the `crypto.rs` implementation via the same C FFI bridge).
4. **Device attach/detach:** Open/close the IOUserClient connection for
   each imported device. On device list change from server (hotplug),
   reconcile with locally-attached virtual devices.
5. **URB forwarding:** Serialise USB/IP URB submissions into
   IOUserClient async calls. Receive completion callbacks and
   forward `OP_REP_UNLNK` responses back to the server.

### DriverKit Dext (Virtual USB Host Controller)

The dext is a C++ `IOService` subclass that registers a custom
`IOUserClass`. It creates a virtual root hub and dynamically creates
`IOUSBHostDevice` objects (DriverKit's `IOUSBHostDevice` class) when
the daemon signals a device attach.

Key dext responsibilities:

1. **IOUserClient interface:** Exposes external methods for the daemon:
   - `AttachDevice(busid: OSSymbol, descriptor: OSData)` — create a new
     `IOUSBHostDevice` with the given device descriptor.
   - `DetachDevice(busid: OSSymbol)` — tear down a virtual device.
   - `SubmitURB(busid: OSSymbol, urb_data: OSData)` — submit a URB to
     the USB stack via `IOUSBHostDevice::CreateIOUSBHostInterface` and
     the pipe interfaces.
   - `CompleteURB(busid: OSSymbol, urb_data: OSData)` — called by
     daemon when a response arrives from the remote server for an
     in-flight URB. Matches by URB seqnum.
2. **IOUSBHostDevice lifecycle:** For each imported device, the dext
   allocates an `IOUSBHostDevice` in the IORegistry under the virtual
   root hub. The device descriptor, configuration descriptors, and
   endpoint descriptors are set from the USB/IP device descriptor data
   received during `OP_REQ_IMPORT`.
3. **URB dispatch:** URBs from the USB stack (class drivers) arrive at
   the `IOUSBHostInterface` pipe objects. The dext captures them and
   forwards to the daemon via an async IOUserClient completion callback.
   When the daemon returns the URB result (from the remote server), the
   dext completes the URB in the USB stack.
4. **Isochronous handling:** Initially unsupported. The dext rejects
   isochronous pipe requests with `kIOReturnUnsupported` (consistent
   with ADR-0002's v1.0 device class scope).

### IOUserClient IPC Interface

The IPC protocol between daemon and dext uses DriverKit's async method
calls (`AsyncExternalMethod`). Each call carries a shared memory buffer
(`IOSharedDataQueue` or `IOMemoryDescriptor`) for URB payload transfer
to avoid excessive data copying for bulk transfers.

```
Daemon                          Dext
  |                                |
  |--- AttachDevice(busid, desc)->|
  |                                |--- alloc IOUSBHostDevice
  |                                |--- set descriptors
  |                                |--- publish in IORegistry
  |<-- kIOReturnSuccess ----------|
  |                                |
  |--- SubmitURB(busid, urb) ---->|
  |                                |--- queue URB, return immediately
  |<-- kIOReturnSuccess ----------|
  |                                |
  |  [UWB received from server]   |
  |--- CompleteURB(busid, urb) -->|
  |                                |--- complete URB in USB stack
  |                                |--- wake waiting class driver
  |                                |
  |--- DetachDevice(busid) ------>|
  |                                |--- tear down IOUSBHostDevice
  |<-- kIOReturnSuccess ----------|
```

The dext exposes the following `IOUserClass` external methods:

| Method ID | Name | Parameters | Direction |
|-----------|------|------------|-----------|
| 0 | `AttachDevice` | `OSString* busid, OSData* descriptors` | daemon -> dext |
| 1 | `DetachDevice` | `OSString* busid` | daemon -> dext |
| 2 | `SubmitURB` | `OSString* busid, OSData* urb_data` | daemon -> dext |
| 3 | `URBCompletion` | `OSData* urb_data` | dext -> daemon (async callback) |
| 4 | `DeviceAttached` | `OSString* busid` | dext -> daemon (async callback) |
| 5 | `DeviceDetached` | `OSString* busid` | dext -> daemon (async callback) |

### Device Visibility

Imported devices appear in the IORegistry under the virtual root hub
service. The macOS USB stack enumerates them through standard
`IOUSBHostDevice` interfaces, which means:

- **Any class driver** (HID, mass storage, CDC/ACM for serial, printer,
  scanner) can bind to the virtual device without modification — the
  device reports standard USB descriptors and the class driver sees a
  normal USB device.
- **`system_profiler SPUSBDataType`** lists imported devices alongside
  physical ones.
- **`IOKit` APIs** (IOUSBDevice, IOUSBInterface) work transparently.
- **`IORegistryEntry`** shows the virtual root hub and attached devices
  with the project's bundle identifier for diagnostics.

## Build Requirements

Building the macOS client requires Apple tooling not available in the
project's CI (no macOS runners in current GitHub Actions configuration):

- **Xcode 14+** (macOS 13 Ventura or later) for `xcodebuild` and
  DriverKit SDK.
- **DriverKit entitlement** in the dext's `Info.plist`:
  `com.apple.developer.driverkit.transport.usb`.
- **Apple Developer Program membership** ($99 USD/year) for signing and
  notarization. DriverKit dexts must be signed with a Developer ID
  certificate and notarized by Apple before they load on end-user
  machines. Development signing works with a free Apple ID for local
  testing.
- **Code signing** with hardened runtime entitlements. Both the daemon
  and dext must be signed. The daemon requires
  `com.apple.security.cs.disable-library-validation` and
  `com.apple.security.device.usb` entitlements.
- **Approval prompt:** On first load, the user must approve the
  DriverKit extension in System Settings > Privacy & Security. This is
  a one-time step per version.

Build output:

```
client/macos/
  AnyPlugDaemon.app      # User-space daemon (Swift)
  AnyPlugVHCI.dext       # DriverKit extension (C++)
```

The daemon bundle should embed the dext within its `Contents/Library/`
directory for single-bundle distribution, installed via
`systemextensionsctl`.

## Limitations

- **No isochronous support:** Audio devices and webcams are not
  supported in v1.0, consistent with ADR-0002. The dext rejects
  isochronous endpoint requests.
- **Kernel panic risk during development:** DriverKit runs in a
  sandboxed user-space process (`dextd` supervisor), which provides
  better stability than kexts — a dext crash kills only the dext
  process, not the kernel. However, bugs in `IOUSBHostDevice`
  descriptor setup or pipe management can cause kernel panics
  in the `IOUSBFamily` kernel extension, especially on Apple
  Silicon where the USB stack is tightly integrated with the
  XHCI controller.
- **Apple Silicon only for production:** Intel Macs can run DriverKit
  dexts, but the project's primary target is Apple Silicon (M1+).
  The dext is universal binary (x86_64 + arm64) but isochronous and
  performance tuning are only validated on Apple Silicon.
- **One-time approval friction:** Users must approve the system
  extension via System Settings on first launch (requires admin
  password). This is a platform constraint of DriverKit.
- **Startup latency:** Loading a DriverKit dext via
  `systemextensionsctl` takes several seconds the first time.
  Subsequent loads are faster because the service is cached.
- **No IPv6:** Consistent with the v1.0 scope (ADR-0004), the daemon
  does not support IPv6 connections.

## Alternatives Considered

### Kernel Extension (kext) — Rejected

A traditional I/O Kit kernel extension implementing a virtual USB host
controller in the kernel would avoid the IPC overhead of DriverKit and
provide direct access to the kernel USB stack. However, Apple has
deprecated kexts in macOS 11 and they do not load on Apple Silicon
machines at all (SIP blocks non-Apple kexts). A kext-based approach
would limit the client to Intel Macs running macOS 10.15 or earlier,
contradicting the project's v1.0 goal of modern macOS support.

### IOUserSCSI Class Driver — Rejected

DriverKit provides `IOUserSCSI` for virtual SCSI devices. This is too
narrow — it only allows exposing block devices via a SCSI command set,
not arbitrary USB devices. The project's v1.0 scope includes HID,
serial, printers, scanners, and bulk-only devices that are not SCSI
and would require protocol emulation rather than passthrough.

### Pure User-Space (USB Device Emulation) — Rejected

A user-space approach using the `IOUSBDeviceInterface` API to create
virtual devices is not possible on macOS. The IOKit USB family does
not expose interfaces to create or inject virtual USB devices from
user space. `IOUSBDeviceInterface` is for communicating with existing
USB devices, not creating new ones. Apple's `IOUSBHostFamily` is a
kernel-private API.

The closest user-space alternative — `VirtualUSBDevice` via the
`Endpoint Security` framework — does not exist. The `Endpoint Security`
framework controls device access policy, not device injection.

### Rust-Based Daemon — Rejected (for now)

Building the daemon in Rust using `core-foundation` and `io-kit-rs`
crates would allow code reuse with the existing Rust codebase. However:

- The `io-kit-rs` crate does not support IOUserClient async external
  methods in its current form.
- Swift provides first-class async/await integration with IOKit via
  `IOService` notifications and `IOConnectCallAsyncMethod`.
- The FFI bridge for USB/IP protocol parsing (via the Rust `usbip-core`
  crate) is maintained; only the daemon orchestration layer is Swift.

This decision can be revisited if `io-kit-rs` gains DriverKit IPC
support and the project accumulates more macOS-specific Rust code.

## Consequences

- macOS becomes a supported client platform for v1.0 (per ADR-0004),
  closing a competitive gap with VirtualHere.
- The project gains a new codebase component (`client/macos/`) with
  Swift and C++ code, separate from the rest of the Rust workspace.
- The DriverKit dext requires Apple Developer Program membership for
  distribution, adding a recurring cost for maintainers. CI builds
  cannot sign or notarize the dext without Apple-issued credentials.
- The dual-bundle distribution (daemon + dext) requires a packaging
  step not present for the Linux or Windows clients.
- Users on macOS 11-13 will see a system extension approval prompt
  on first launch. macOS 14+ may improve this flow.
- The FFI bridge from Swift to Rust (`usbip-core`) creates a build
  dependency on `cbindgen` and the Rust toolchain for macOS builds.
- Testing is limited to manual testing on physical Mac hardware; no
  CI coverage until macOS runners are added to the GitHub Actions
  configuration.
