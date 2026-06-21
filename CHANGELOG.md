# Changelog

All notable changes to AnyPlug are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-06-21

This release is the first to ship the **Web Console**, the **Server HTTP API**,
and **AES-256-GCM wire encryption** end-to-end, alongside major Android
polish (Material 3 Expressive, layered LAN discovery, mDNS) and the
`VhciBackend` client refactor. 39 commits since v0.3.0 are summarised below.

### Added

- **Web Console: scan, config, latency, and health UI** (`01a1a6d`, issue #22) —
  first cut of the browser-based console. Operators can scan the local bus for
  exported devices, view/edit server configuration, watch per-client latency
  frames in real time, and see server health at a glance. Backed by the new
  Server API.
- **Server: HTTP API** (`5aada51`, issue #26) — implemented the REST surface
  the Web Console talks to: device attach/detach, status, configuration
  endpoints. Includes OpenAPI documentation (`030c6e2`, issue #25) and a
  port-model write-up that clarifies the TCP vs. in-process transport split.
- **Server: real `/api/connect` and `/api/disconnect`** (`c553535`) — wired
  the connect/disconnect endpoints to the `RemoteImporter` so the API is no
  longer a stub. The Web Console's Connect/Disconnect buttons now perform
  actual imports.
- **Server: `/api/status` exposes `server_id`, `server_name`, and `port`**
  (`93c6419`) — clients (and the Web Console) can now identify and address
  a specific server across reboots, not just any server on a given host.
- **Server: `--fake-devices` CLI flag for local development** (`a3201eb`) —
  the server can now advertise synthetic devices without requiring physical
  USB hardware, unblocking UI development, demos, and CI smoke tests.
- **Server: advertise shared devices via mDNS** (`f8a1e9f`) — the server
  publishes its available devices over multicast DNS so clients on the same
  LAN can discover exports without manually entering a host. Returns
  `NODEV` on unknown busid rather than a confusing generic error.
- **Server: `--bind-iface` flag for interface-specific binding** (`155f5bb`,
  merged via `c92540d`) — operators can pin the server socket to a single
  network interface (e.g. the LAN, not the VPN tunnel), useful on
  multi-homed hosts.
- **Server: `--encrypt` wires AES-256-GCM into `handle_client`**
  (`a857f1e`, issue #34) — opt-in end-to-end authenticated encryption on the
  USB/IP tunnel. The server now wraps the client stream in an AES-256-GCM
  AEAD tunnel when the flag is set, giving confidentiality + integrity
  without changing the on-wire USB/IP framing for peers that don't opt in.
- **Crypto: in-place AEAD decrypt primitive** (`a9ba735`, issue #34) — added
  an authenticated in-place decrypt path for inbound frames, avoiding an
  allocation per packet on the hot path. Foundational building block for
  the `--encrypt` tunnel.
- **Android: layered LAN discovery** (`44af813`, `5ed68ef`) — REST-probe
  discovery layered on top of mDNS, plus a last-known-server cache so the
  app reconnects to the same server after a restart even if mDNS is
  unavailable. Initial wiring commit `84f0466` added the mDNS client and
  a manual Connect button on Android.
- **Android: per-device picker on discovered servers** (`5cbca6d`) — the
  discovery screen now lists individual shared devices per server, not
  just servers, so users can pick which export to attach to.
- **Android: `restart()` primitive on `ServerDiscovery`** (`4bab026`) — the
  discovery subsystem can be cleanly torn down and re-initialised, used
  by the new Refresh button (`8574af0`).
- **Android (TV): per-client bandwidth UI and Stop/Discover wiring**
  (`1c92261`) — the TV client now shows per-client bandwidth and the Stop
  and Discover buttons are properly wired to the underlying state machine.
- **Android: M3 Expressive theming, TV D-pad focus, edge-to-edge**
  (`23aea2a`) — applied the Material 3 Expressive design language to the
  phone app, fixed D-pad focus traversal for the TV form factor, and
  enabled edge-to-edge layout on supported Android versions.
- **Build (Android): OkHttp and DataStore dependencies** (`a98079e`) — added
  OkHttp for HTTP/REST discovery and DataStore for the last-known-server
  cache. Also added the `NEARBY_WIFI_DEVICES` runtime permission and
  cleartext-traffic opt-in needed for LAN discovery.
- **Client: `VhciBackend` promotion and injection** (`ae5bfb9`, issue #28) —
  the USB/IP VHCI integration is now a first-class `VhciBackend` trait,
  injected into `Client` at construction. The client no longer knows about
  any specific VHCI implementation, enabling alternative transports and
  cleaner testing. Cleanup follow-up in `2061de0` collapses `VhciDriver`
  into the public seam and extracts a shared encode helper.

### Changed

- **macOS IOKit FFI lints allowed in `iokit_backend`** (`2766a5e`) — the
  server's macOS IOKit bindings are now allowlisted for the FFI lints they
  trigger, so `cargo clippy -D warnings` stays clean on macOS without
  weakening the global lint policy.
- **Repository housekeeping: CLAUDE.md, release.yml, BUILDING.md**
  (`4d53206`) — refreshed the root `CLAUDE.md`, the GitHub Actions release
  workflow, and the `BUILDING.md` guide to match the current build/test
  surface (TV app, iokit lints, layered discovery).
- **CI: fix `gradlew` path in workflow; drop unused Rust test import**
  (`029fb50`) — CI no longer breaks on the wrong Gradle path, and an
  unused import in a Rust test module was removed.

### Fixed

- **Client: stop dropping 18 bytes in `tcp_connect_and_import`** (`07580bd`)
  — high-severity (H1) data-loss bug on the import path. Remote devices
  now attach with the full configuration descriptor intact, fixing a class
  of "device imported but unusable" failures.
- **Server: `ApiConfig` test fixtures** (`0a5389c`) — added the
  `server_id` and `server_name` fields to test fixtures so the fixtures
  match the production shape (previously the mismatch masked a missing
  field in API responses).
- **Android: missing `Alignment` import in `MainScreen`** (`54dbc87`) —
  compile error fix; the screen that uses Compose `Alignment` had lost
  the import in an earlier refactor.
- **Android: stray closing brace in `ServerDiscovery`** (`2cc5f75`) —
  syntax fix in the discovery composable that prevented the file from
  compiling.
- **Server: `ServerSocket` leak on rapid stop/start; state propagation;
  duplicate UI stop buttons** (`e3811d8`) — the server no longer leaks
  sockets when restarted in quick succession, state changes propagate
  to the UI reliably, and the TV client no longer renders two competing
  Stop buttons.
- **Android: error handling in USB service and UI; errors surfaced via
  Toast** (`b92b9e2`) — the Android USB service now catches and reports
  errors through a single error channel; the UI surfaces them via
  Toast, replacing silently-failed imports.
- **Android: shared USB utils, robust string matching, de-duplicated
  code** (`83fca1e`) — extracted the shared USB descriptor parsing into
  a single utility, replaced brittle substring matching with proper
  descriptor-field comparison, and removed the duplicated logic that
  had drifted between code paths.
- **Crypto: drop redundant `mut`s flagged by the CI toolchain**
  (`23ba5a7`) — minor cleanup on the encryption path; behavior unchanged.
- **Server: extract `record` into inherent `impl RealImporter`** (`e769b95`)
  — code-organisation fix that lets clippy and the toolchain reason about
  the importer state correctly. No behavior change.
- **Server: use `ErrorKind::InvalidMessage`; drop `pub` on trait impl
  method** (`d1ff085`) — more precise error variant for malformed wire
  frames; tightened visibility on an over-exposed trait impl method.
- **Test: remove unused imports in `ws_events_forwards_latency_frames`
  test** (`990584a`) — clippy cleanup.

### Documentation

- **Architecture: VHCI backend abstraction + `vhci-tdd` worked example**
  (`217c8ba`) — `ARCHITECTURE.md` now documents the `VhciBackend` seam
  with a worked end-to-end TDD example showing how to drive a
  backend-agnostic test through the new trait. Pairs with the
  `VhciBackend` promotion above.
- **Skills: vertical-slice example, Rust patterns, verify-on-RED hook**
  (`ed5f861`) — added a vertical-slice worked example and Rust-specific
  TDD patterns to the in-repo skills, plus a hook that runs the verify
  gate on RED (test failing) so contributors can't ship a "passing"
  test that never actually ran.
- **Port model docs + OpenAPI update** (`030c6e2`, issue #25) —
  documented the TCP vs. in-process port model that the new Server API
  depends on, and updated the OpenAPI spec to match the implemented
  endpoints.

[0.5.0]: https://github.com/stanvx/AnyPlug/releases/tag/v0.5.0
