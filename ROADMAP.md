# USB/IP Passthrough — Development Roadmap

> Cross-platform USB/IP implementation for Android, Android TV, and Windows.

---

## Milestone 1: Foundation & Protocol Core (Q2 2026)

| Status | Task |
|--------|------|
| ☐ | Define crate graph in workspace `Cargo.toml` |
| ☐ | Implement `shared/usbip-core` — USB/IP protocol constants, headers, packet serialization |
| ☐ | Document USB/IP protocol spec references and wire format |
| ☐ | Add comprehensive unit tests for all packet types |
| ☐ | Set up CI (cargo check + test) passing green |

**Goal:** A verified protocol library with zero unsafe code.

---

## Milestone 2: Server & Client (Q3 2026)

| Status | Task |
|--------|------|
| ☐ | Implement `server/usbip-server` — TCP listener, virtual host controller, device export |
| ☐ | Implement `client/usbip-client` — TCP connector, USB device driver binding |
| ☐ | Integration tests: server ↔ client loopback |
| ☐ | CLI binaries with `--help` and config file support |
| ☐ | Error handling, reconnection, logging |

**Goal:** `usbipd` and `usbip` CLI tools working on Linux.

---

## Milestone 3: GUI Windows Client (Q3 2026)

| Status | Task |
|--------|------|
| ☐ | egui-based Windows app in `windows/` crate |
| ☐ | Connect to remote USB/IP server |
| ☐ | List, attach, detach devices from GUI |
| ☐ | MSVC release build in CI |
| ☐ | Windows installer (MSI/WiX) |

**Goal:** One-click USB/IP client for Windows.

---

## Milestone 4: Android Native Library (Q4 2026)

| Status | Task |
|--------|------|
| ☐ | `android/rust/usbip-android` — JNI cdylib bridge |
| ☐ | Gradle project with AGP 8.2 + Kotlin 1.9.20 |
| ☐ | Android service wrapper (foreground service, USB permission handling) |
| ☐ | Android TV UI (leanback-friendly) |
| ☐ | Release APK build in CI |

**Goal:** Sideloadable APK for Android TV USB gadget support.

---

## Milestone 5: Polish & Release (Q1 2027)

| Status | Task |
|--------|------|
| ☐ | Signed Windows binaries |
| ☐ | Android app on GitHub Releases with changelog |
| ☐ | Performance benchmarking & optimization |
| ☐ | Security audit (no unsafe, no network vulns) |
| ☐ | Documentation site / README with architecture diagram |

**Goal:** v1.0.0 — production-ready cross-platform USB/IP solution.

---

## How to Release

```bash
# Tag and push
git tag v0.1.0
git push origin v0.1.0

# CI builds:
#   usbip-windows-x86_64.exe   (Windows EXE, MSVC)
#   usbip-android-release.apk  (Android APK)
```

All builds are handled by `.github/workflows/release.yml`.
