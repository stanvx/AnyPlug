---
name: tdd
description: Test-driven development for this Rust workspace — write tests first (RED), implement (GREEN), refactor. Use when adding features, fixing bugs, or touching any crate in the workspace.
---

## TDD Workflow

Follow this order for every change:

1. **RED**: Write a failing test in the target crate's `#[cfg(test)]` module
2. **GREEN**: Write the minimal implementation to pass
3. **REFACTOR**: Clean up while keeping tests green
4. **VERIFY**: Run the full workspace test suite

## Test Patterns by Crate

### `usbip-core` (shared/usbip-core/)

Tests live inline in `#[cfg(test)] mod tests {}` at the bottom of each source file. No separate `tests/` directory.

**Wire-format struct tests** (protocol.rs, urb.rs):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_cmd_submit() {
        let cmd = UsbIpCmdSubmit {
            transfer_flags: 0,
            transfer_buffer_length: 64,
            // ... fill all fields
        };
        let bytes = cmd.as_bytes();
        let parsed = UsbIpCmdSubmit::read_from_prefix(bytes).unwrap();
        assert_eq!(cmd.transfer_flags, parsed.transfer_flags);
    }
}
```

zerocopy 0.8 traits: `IntoBytes` (was `AsBytes`), `FromZeros` (was `FromZeroes`), `FromBytes` (unchanged). Derive macros: `#[derive(IntoBytes, FromBytes, FromZeros)]`.

**Crypto tests** (crypto.rs):
- X25519 test vectors (RFC 7748)
- AES-256-GCM encrypt/decrypt round-trip
- HKDF-SHA256 known-answer tests
- Error cases: wrong key, truncated ciphertext, corrupted tag

**Descriptor parsing** (descriptor.rs):
- Parse known-good descriptor byte arrays
- Truncated/malformed input → error
- Nested descriptors (HID, hub)

### `usbip-server` — Mocking libusb

The server crate depends on `rusb` (libusb bindings). Tests that touch USB hardware need mocking:

- Use `#[cfg(not(target_os = "macos"))]` guards for tests that need actual USB hardware (libusb on macOS requires kernel extension).
- For unit tests of `server.rs` protocol handling, construct `UsbIpHeader` + `UsbIpCmdSubmit` byte arrays and test the parse/respond logic without real USB.
- `discovery.rs` mDNS tests: verify TXT record format strings, don't require a running mDNS daemon.

### `usbip-client` — Mocking VHCI

- VHCI tests (`vhci.rs`): model the VHCI device lifecycle (create → submit URB → complete → cancel → delete) without a real kernel driver. Use trait objects or conditional compilation.
- Reconnect logic (`client.rs`): test backoff timing, retry count exhaustion, and connection state transitions.

### `windows` — Platform-gated

- Tests in `windows_usb.rs` are `#[cfg(windows)]` only. On non-Windows, they compile but don't run.
- Add `#[cfg(not(windows))]` unit tests for the non-SetupAPI logic (parsing, state machines) so CI on Linux can exercise them.

## Run Commands

```bash
# Run all workspace tests
cargo test --release --workspace

# Run a single crate
cargo test --release -p usbip-core

# Run a specific test by name
cargo test --release -p usbip-core -- roundtrip_cmd_submit

# Run with output (println! in tests)
cargo test --release -p usbip-core -- --nocapture
```

## Target: 80% coverage

Use `cargo tarpaulin` for coverage (install with `cargo install cargo-tarpaulin`):

```bash
cargo tarpaulin --workspace --out Html --output-dir target/tarpaulin
```

Focus coverage efforts on:
1. Protocol serialization/deserialization (highest risk — wire-format bugs)
2. Error mapping (`rusb_to_urb_status` and friends)
3. URB buffer pool allocation/deallocation
4. Crypto key exchange (already partially covered)

## Rust-Specific Patterns

### Why inline `#[cfg(test)] mod tests` (not `tests/*.rs`)

Integration tests in `tests/*.rs` can only see the crate's **public** API. When the unit under test is `pub(crate)` — common for trait abstractions meant to stay inside the workspace — the mock cannot be defined alongside it, and the test must live in the same module that has access to the type.

Rule: if the type you need to mock or exercise is `pub(crate)`, write the test as an inline `#[cfg(test)] mod tests {}` at the bottom of the file that **uses** the mock, not in `tests/`.

### Injection shape: `Arc<dyn Trait>` vs generic `Client<V: Trait>`

When a struct needs a swappable backend, the two shapes look interchangeable but have very different call-site costs:

- `Client<V: VhciBackend>` — type parameter ripples to every constructor, every handler, every test signature. Three CLI handlers all become `Handler<V>`. Compounding.
- `Client { backend: Arc<dyn VhciBackend + Send + Sync> }` — backend is a runtime object. Handlers stay concrete. Mocking swaps the `Arc` at construction.

Default to `Arc<dyn Trait>` whenever the trait object is held inside a struct. Reserve generics for genuine type-level constraints (e.g. `From`/`Into` bounds, no-std contexts, static dispatch through hot loops). The test seam is preserved; the call sites stay simple.

### `#[allow(dead_code)]` on platform trait methods

When extracting a `Trait` with default implementations, every method is part of the platform contract even if a single caller stops using it. Clippy's `dead_code` lint will flag the unused default. The right fix is `#[allow(dead_code)]` on the method — **not** deleting the method or wiring up a fake caller. Platform traits are consumed by multiple backends; removing a method to silence clippy breaks the contract for the next backend.

### Worked Example: VHCI Backend Injection Seam (issue #28)

The `usbip-client` crate's VHCI abstraction is the canonical example of this
section's patterns in practice:

- **Trait + Arc object**: `vhci::VhciBackend` is a `pub trait`; `Client` holds
  `Arc<dyn VhciBackend>`. See `client/usbip-client/src/vhci/mod.rs:39` and
  `client/usbip-client/src/client.rs:93` (`new_with_vhci`).
- **Public API for testing**: both the trait and the injection constructor are
  re-exported at the crate root, so `tests/injection_seam.rs` (an integration
  test in `tests/`, not `src/`) can construct a `DummyVhci` and prove the
  seam is reachable. If either regresses to `pub(crate)`, the integration
  test fails to compile.
- **MOCK pattern**: `MockVhciBackend` (in the `#[cfg(test)] mod tests` of
  `vhci/mod.rs`) records URBs into a `Mutex<Vec<RecordedUrb>>` and exposes
  them via a `recorded_urbs() -> Vec<RecordedUrb>` accessor. Tests use the
  accessor; the field stays private. `pub(crate)` field access is the
  common mistake — use a typed accessor instead.
- **RED for structural refactors**: when promoting a seam from `pub(crate)`
  to `pub`, the RED test is "the integration test in `tests/` fails to
  compile." A passing integration test is the GREEN. No runtime assertion
  is needed — the type system *is* the test. This is faster than writing
  a behaviour assertion, and it locks the public surface against future
  regressions.
