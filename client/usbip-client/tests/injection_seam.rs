//! RED test for issue #28 M1: the VHCI injection seam must be reachable
//! from outside the crate.
//!
//! This is an integration test (in `tests/`) so it can ONLY see items that
//! are `pub` in the `usbip_client` crate. It fails to compile against the
//! current `main` because both `VhciBackend` (in `vhci/mod.rs`) and
//! `Client::new_with_vhci` (in `client.rs`) are `pub(crate)`. The compile
//! error is the RED — once M1 makes them `pub`, this test compiles and
//! the constructor succeeds.

use std::sync::Arc;

use usbip_client::vhci::VhciBackend;
use usbip_client::Client;
use usbip_client::ClientConfig;

/// Minimal mock that implements the VHCI backend trait. Used only to
/// prove the trait is reachable from outside the crate; the test never
/// actually drives a VHCI operation.
struct DummyVhci;

impl VhciBackend for DummyVhci {
    fn create_device(
        &self,
        _entry: &usbip_core::protocol::UsbIpDeviceEntry,
        _descriptors: &[u8],
    ) -> usbip_core::error::UsbIpResult<usbip_client::vhci::VhciDevice> {
        Err(usbip_core::error::UsbIpError::from(usbip_core::error::ErrorKind::NotSupported(
            "dummy".into(),
        )))
    }

    fn complete_urb(
        &self,
        _seqnum: u32,
        _devid: u32,
        _status: i32,
        _actual_length: u32,
        _data: &[u8],
    ) -> usbip_core::error::UsbIpResult<()> {
        Ok(())
    }

    fn cancel_urb(&self, _seqnum: u32, _devid: u32) -> usbip_core::error::UsbIpResult<()> {
        Ok(())
    }

    fn remove_device(&self, _port: u32) -> usbip_core::error::UsbIpResult<()> {
        Ok(())
    }
}

#[test]
fn test_injection_seam_is_public() {
    // Both the trait and the constructor must be reachable through the
    // crate's public API (`usbip_client::...`). The use statements above
    // already prove that for the trait; this body proves it for the
    // constructor. If either is still `pub(crate)`, this file fails to
    // compile and the RED is confirmed.
    let config = ClientConfig::default();
    let backend: Arc<dyn VhciBackend> = Arc::new(DummyVhci);

    let result = Client::new_with_vhci(config, backend);
    assert!(
        result.is_ok(),
        "new_with_vhci must succeed when given an injected backend, got: {:?}",
        result.err()
    );
}
