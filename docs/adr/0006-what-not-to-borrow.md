# What NOT to borrow from adjacent USB-over-network designs

The architecture review across related projects (VirtualHere, Cemuhook, DSU
server, and the console-remote ecosystem) identified a set of tempting-but-wrong
patterns that this project must not adopt. They each solve a real problem well
enough to seem reusable, but they conflict with the project's core constraint:
**byte-for-byte passthrough of native USB descriptors and endpoints over
USB/IP.**

## Rejected patterns

### 1. UDP-only transports

Some implementations (notably Cemuhook protocol and certain cloud-game
companions) carry control + input over UDP to minimise latency. This is
rejected because:

- UDP has no reliability, ordering, or congestion control by default.
  USB/IP URBs carry arbitrary bulk data (mass storage) and control transfers;
  dropping or reordering a single packet corrupts the device session
  irrecoverably.
- The project's latency budget (~1.5-3.5 ms round-trip on Ethernet) already
  fits comfortably inside TCP's retransmission overhead; there is no latency
  case for UDP.
- Encryption (AES-256-GCM) is significantly easier to do correctly on a
  reliable, ordered byte stream than on a datagram protocol.

### 2. REST-only topology

Some designs expose device control exclusively through HTTP/REST endpoints,
treating the data plane as a side effect of API calls. This is rejected because:

- REST is designed for request/response, not for a sustained bidirectional
  byte stream. USB/IP is fundamentally a streaming protocol: the server and
  client exchange URBs continuously for the lifetime of a session.
- A REST wrapper adds a serialisation boundary between the wire protocol and
  the URB forwarding loop, which would force copying and re-framing of every
  message — defeating zero-copy where it matters.
- The project already uses REST (`/api/*`) as the *control* plane (status,
  scan, connect, config). It must not be promoted to the *data* plane.

### 3. MSG_RUMBLE opcodes

Some controller passthrough schemes define ad-hoc "rumble" or effect opcodes
layered on top of the transport to send force-feedback commands. This is
rejected because:

- Force feedback is already expressed natively in USB HID reports
  (output reports and feature reports). A custom rumble opcode is a second,
  competing encoding of data the HID stack already produces — two sources of
  truth for the same physical action.
- The whole point of USB/IP passthrough is that the importing OS loads the
  real vendor/class driver; the driver already issues HID output reports.
  Injecting MSG_RUMBLE lets a generic client bypass the driver, which is
  only useful when emulating a device (explicitly out of scope).
- Any custom opcode must be versioned, documented, and kept in sync with
  every client and server. Native HID needs none of that machinery.

### 4. Cemuhook DSU axis layout

Cemuhook's DSU ("DS4Windows-compatible") protocol defines its own 6-axis
layout (accel + gyro with a specific byte order and scale) that game clients
parse directly. The project must not build its own axis-layout protocol on
top of USB/IP. This is rejected because:

- DSU is a *replacement* for the native controller protocol — the DSU server
  converts the hardware's raw reports into DSU's fixed schema. That is the
  opposite of passthrough.
- A custom axis layout would require each consumer (game, app) to implement
  the schema, exactly duplicating what the OS HID/FFB stack already does for
  native devices.
- The narrowest correct way to support input is to let the importing OS load
  the real HID driver and speak standard HID reports. Any game that already
  supports the device directly will work with zero extra client code.

## Considered option

- **"Meet the ecosystem halfway with a compatibility layer that speaks DSU
  or UDP on the input path"** — rejected. It erodes the passthrough guarantee
  (devices would report through two different paths), adds a second network
  surface to secure, and grows a compatibility-maintenance surface that does
  not advance the core USB/IP design.

## Consequences

- The project keeps a single data plane (TCP + USB/IP) and a single encoding
  of device behaviour (native USB descriptors and HID reports).
- New protocol features must justify themselves against these anti-patterns.
  If a proposed feature looks like any of the four rejected patterns, it
  needs a stronger reason than "it's what adjacent products ship."
- The RetroPie / Lakka / Steam Link ecosystem packaging (see CONTEXT.md
  "Ecosystem integration") operates at the *existing* USB/IP boundary, not
  by adding a second protocol.
