//! USB device management via the platform-agnostic `UsbBackend` trait.
//!
//! `UsbDeviceManager` delegates all operations to a pluggable backend,
//! defaulting to `LibusbBackend` on all platforms.  On macOS the
//! `IokitBackend` is also available.
//!
//! The `LibusbBackend` module provides the standard libusb-based
//! implementation for enumeration, claiming, and URB submission.

use std::collections::HashMap;
use std::sync::Mutex;

use tracing::debug;

use usbip_core::error::{usb_error_to_urb_status, ErrorKind, UsbIpError, UsbIpResult};
use usbip_core::protocol::UsbIpDeviceEntry;
use usbip_core::urb::UsbIpCmdSubmit;

use crate::api::DeviceLister;
use crate::usb_backend::{parse_busid, LibusbBackend, UrbTransferResult, UsbBackend};

/// Result of a single URB execution, mapped to a wire-ready status.
///
/// Not a `Result` — even error outcomes produce a valid `UrbResult` so the
/// caller can always serialise a wire reply via [`usbip_core::reply::serialize_reply`].
#[derive(Debug, Clone)]
pub struct UrbResult {
    pub status: i32,
    pub actual_length: u32,
    pub data: Vec<u8>,
}

/// Manages USB devices for the server, delegating to a backend.
///
/// Backend is chosen at build time or runtime:
/// - Default: `LibusbBackend` (rusb) — works on Linux, macOS, Windows
/// - macOS: `IokitBackend` (native I/O Kit) — for full macOS integration
///
/// The backend is pluggable via the `UsbBackend` trait.
pub struct UsbDeviceManager {
    /// The active USB backend.
    backend: Box<dyn UsbBackend>,
    /// busid → (backend status tracking, if needed)
    handles: Mutex<HashMap<String, bool>>,
}

impl UsbDeviceManager {
    /// Create with the default backend (libusb/rusb).
    pub fn new() -> UsbIpResult<Self> {
        let backend = Box::new(LibusbBackend::new()?);
        Ok(Self { backend, handles: Mutex::new(HashMap::new()) })
    }

    /// Create with a specific backend (e.g., `IokitBackend`).
    pub fn with_backend(backend: Box<dyn UsbBackend>) -> Self {
        Self { backend, handles: Mutex::new(HashMap::new()) }
    }

    /// List all USB devices on the system.
    pub fn list_devices(&self) -> Vec<UsbIpDeviceEntry> {
        self.backend.list_devices()
    }

    /// List devices, filtered by an allowlist of (VID, PID) pairs.
    ///
    /// If `allowed` is empty, all devices are returned.
    /// Only devices whose VID:PID match an entry in `allowed` are included.
    pub fn list_exportable_devices(&self, allowed: &[(u16, u16)]) -> Vec<UsbIpDeviceEntry> {
        let all = self.list_devices();
        if allowed.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|d| allowed.iter().any(|(vid, pid)| d.vid() == *vid && d.pid() == *pid))
            .collect()
    }

    /// Get a device entry by busid.
    pub fn get_device_entry(&self, busid: &str) -> Option<UsbIpDeviceEntry> {
        let (busnum, devnum) = parse_busid(busid).ok()?;
        self.list_devices()
            .into_iter()
            .find(|d| d.busnum.get() == busnum as u32 && d.devnum.get() == devnum as u32)
    }

    /// Claim a device (detach kernel driver, claim interface).
    pub fn claim_device(&self, busid: &str) -> UsbIpResult<()> {
        self.backend.claim_device(busid)?;
        self.handles.lock().unwrap().insert(busid.to_string(), true);
        debug!("Claimed device: {}", busid);
        Ok(())
    }

    /// Get the full USB descriptor tree for a device.
    pub fn get_descriptor_tree(&self, busid: &str) -> UsbIpResult<Vec<u8>> {
        self.backend.get_descriptor_tree(busid)
    }

    /// Execute a URB (submit a USB transfer) on the physical device.
    pub fn execute_urb(
        &self,
        busid: &str,
        cmd: &UsbIpCmdSubmit,
        out_data: &[u8],
    ) -> UsbIpResult<UrbTransferResult> {
        let _handles = self.handles.lock().unwrap();
        if !_handles.contains_key(busid) {
            return Err(UsbIpError::from(ErrorKind::DeviceNotFound(busid.into())));
        }
        self.backend.execute_urb(busid, cmd, out_data)
    }

    /// Execute a URB and map any error to a negative URB status code, so the
    /// caller can always serialise a valid `USBIP_RET_SUBMIT` wire reply.
    pub fn submit_urb(&self, busid: &str, cmd: &UsbIpCmdSubmit, out_data: &[u8]) -> UrbResult {
        match self.execute_urb(busid, cmd, out_data) {
            Ok(transfer) => {
                UrbResult { status: transfer.status, actual_length: transfer.actual_length, data: transfer.data }
            },
            Err(e) => {
                let status = match e.kind() {
                    ErrorKind::Usb(ref code) => usb_error_to_urb_status(code),
                    ErrorKind::DeviceNotFound(_) => -19, // -ENODEV
                    ErrorKind::Timeout => -62,           // -ETIME
                    _ => -5,                             // -EIO
                };
                UrbResult { status, actual_length: 0, data: Vec::new() }
            },
        }
    }

    /// Release a claimed device.
    pub fn release_device(&self, busid: &str) -> UsbIpResult<()> {
        let mut _handles = self.handles.lock().unwrap();
        _handles.remove(busid);
        self.backend.release_device(busid)?;
        debug!("Released device: {}", busid);
        Ok(())
    }
}

impl DeviceLister for UsbDeviceManager {
    fn list_devices(&self) -> Vec<UsbIpDeviceEntry> {
        self.list_devices()
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_backend::make_test_entry;

    #[test]
    fn test_usb_error_to_urb_status_mapping() {
        use usbip_core::error::UsbErrorCode;
        assert_eq!(usbip_core::error::usb_error_to_urb_status(&UsbErrorCode::Io), -5);
        assert_eq!(usbip_core::error::usb_error_to_urb_status(&UsbErrorCode::Timeout), -62);
        assert_eq!(usbip_core::error::usb_error_to_urb_status(&UsbErrorCode::NoDevice), -19);
        assert_eq!(usbip_core::error::usb_error_to_urb_status(&UsbErrorCode::NotSupported), -95);
    }

    /// The `with_backend` constructor accepts any `Box<dyn UsbBackend>`.
    #[test]
    fn test_with_backend_accepts_fake_backend() {
        use crate::usb_backend::FakeBackend;
        let fake = FakeBackend::new(vec![]);
        let mgr = UsbDeviceManager::with_backend(Box::new(fake));
        assert!(mgr.list_devices().is_empty());
    }

    /// Allowlisting with empty list returns all devices.
    #[test]
    fn test_list_exportable_devices_empty_allowlist() {
        use crate::usb_backend::FakeBackend;
        let entry = make_test_entry("1-1", 0x046d, 0xc261);
        let fake = FakeBackend::new(vec![entry]);
        let mgr = UsbDeviceManager::with_backend(Box::new(fake));

        let devs = mgr.list_exportable_devices(&[]);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].vid(), 0x046d);
    }

    /// Allowlisting filters out non-matching devices.
    #[test]
    fn test_list_exportable_devices_filters() {
        use crate::usb_backend::FakeBackend;
        let dev1 = make_test_entry("1-1", 0x046d, 0xc261);
        let dev2 = make_test_entry("1-2", 0x8087, 0x0024);
        let fake = FakeBackend::new(vec![dev1, dev2]);
        let mgr = UsbDeviceManager::with_backend(Box::new(fake));

        // Allow only a specific device (046d:c261)
        let devs = mgr.list_exportable_devices(&[(0x046d, 0xc261)]);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].vid(), 0x046d);
        assert_eq!(devs[0].pid(), 0xc261);
    }

    /// Allowlisting with multiple VID:PID pairs works.
    #[test]
    fn test_list_exportable_devices_multiple_allowed() {
        use crate::usb_backend::FakeBackend;
        let dev1 = make_test_entry("1-1", 0x046d, 0xc261);
        let dev2 = make_test_entry("1-2", 0x8087, 0x0024);
        let dev3 = make_test_entry("1-3", 0x1234, 0x5678);
        let fake = FakeBackend::new(vec![dev1, dev2, dev3]);
        let mgr = UsbDeviceManager::with_backend(Box::new(fake));

        let devs = mgr.list_exportable_devices(&[(0x046d, 0xc261), (0x8087, 0x0024)]);
        assert_eq!(devs.len(), 2);
    }

    // ── submit_urb ──────────────────────────────────────────────────────
    //
    // These cross the same seam production uses: `UsbDeviceManager` backed
    // by a fake `UsbBackend`, with the manager's own claim/lookup guard
    // exercised exactly as it is at runtime.

    fn make_cmd() -> UsbIpCmdSubmit {
        use usbip_core::protocol::U32BE;
        UsbIpCmdSubmit {
            seqnum: U32BE::new(1),
            devid: U32BE::new(1),
            direction: U32BE::new(1),
            ep: U32BE::new(0x81),
            transfer_flags: U32BE::new(0),
            transfer_buffer_length: U32BE::new(64),
            start_frame: U32BE::new(0),
            number_of_packets: U32BE::new(0),
            interval: U32BE::new(0),
            setup: [0u8; 8],
        }
    }

    /// A device that was never claimed maps to -ENODEV, same as the real
    /// backend's `execute_urb` guard in `UsbDeviceManager::execute_urb`.
    #[test]
    fn test_submit_urb_unclaimed_device_maps_to_enodev() {
        use crate::usb_backend::FakeBackend;
        let entry = make_test_entry("1-1", 0x046d, 0xc261);
        let fake = FakeBackend::new(vec![entry]);
        let mgr = UsbDeviceManager::with_backend(Box::new(fake));

        let result = mgr.submit_urb("1-1", &make_cmd(), &[]);
        assert_eq!(result.status, -19);
        assert_eq!(result.actual_length, 0);
        assert!(result.data.is_empty());
    }

    /// A claimed device delegates to the backend and maps the transfer
    /// result straight through.
    #[test]
    fn test_submit_urb_claimed_device_succeeds() {
        use crate::usb_backend::FakeBackend;
        let entry = make_test_entry("1-1", 0x046d, 0xc261);
        let fake = FakeBackend::new(vec![entry]);
        let mgr = UsbDeviceManager::with_backend(Box::new(fake));
        mgr.claim_device("1-1").unwrap();

        let result = mgr.submit_urb("1-1", &make_cmd(), &[]);
        assert_eq!(result.status, 0);
        assert_eq!(result.actual_length, 0);
        assert!(result.data.is_empty());
    }
}
