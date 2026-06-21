//! Virtual Host Controller Interface (VHCI) driver abstraction.
//!
//! This module abstracts the platform-specific mechanism for creating
//! virtual USB devices that the local OS sees as real hardware. It uses
//! a trait-based backend pattern: [`VhciBackend`] is the public-facing
//! trait, and [`detect_backend`] returns a boxed platform implementation.
//!
//! ## Platform Implementations
//!
//! | Platform | Backend             | Mechanism                                |
//! |----------|---------------------|------------------------------------------|
//! | Linux    | `LinuxVhciBackend`  | vhci-hcd kernel module (/sys/.../vhci)   |
//! | Windows  | `WindowsVhciBackend`| usbip-win2 kernel driver (vhci.sys)      |
//! | Android  | `LinuxVhciBackend`  | vhci-hcd.ko kernel module (root required)|
//! | macOS    | Not supported       |                                          |

use usbip_core::error::*;
use usbip_core::protocol::*;

// Mutex is only used by test code but cargo fix strips non-test imports.
#[allow(unused_imports)]
use std::sync::Mutex;

// Declare platform-specific backends as child modules.
#[cfg(target_os = "linux")]
mod vhci_linux;
#[cfg(windows)]
mod vhci_windows;

// ─── VhciBackend Trait ──────────────────────────────────────────

/// Public trait for platform-specific VHCI operations.
///
/// Every method maps to one logical operation in the USB/IP VHCI
/// protocol. The trait is `Send + Sync` so a `Box<dyn VhciBackend>` is
/// shareable across the client's forwarding tasks. Exposed publicly so
/// downstream consumers (tests, third-party clients) can implement their
/// own backends.
pub trait VhciBackend: Send + Sync {
    /// Create a virtual USB device from descriptor data.
    fn create_device(
        &self,
        entry: &UsbIpDeviceEntry,
        descriptors: &[u8],
    ) -> UsbIpResult<VhciDevice>;

    /// Complete a URB (USBIP_RET_SUBMIT received from server).
    fn complete_urb(
        &self,
        seqnum: u32,
        devid: u32,
        status: i32,
        actual_length: u32,
        data: &[u8],
    ) -> UsbIpResult<()>;

    /// Cancel an in-flight URB (USBIP_RET_UNLINK received).
    fn cancel_urb(&self, seqnum: u32, devid: u32) -> UsbIpResult<()>;

    /// Remove a virtual device by port number.
    fn remove_device(&self, port: u32) -> UsbIpResult<()>;
}

// ─── VhciDevice ─────────────────────────────────────────────────

/// Handle for a virtual USB device created via VHCI.
#[derive(Debug, Clone)]
pub struct VhciDevice {
    pub port: u32,
    pub devid: u32,
    pub busid: String,
    pub vid: u16,
    pub pid: u16,
}

// ─── Backend Factory ────────────────────────────────────────────

/// Detect the platform and return the appropriate backend.
pub(crate) fn detect_backend() -> UsbIpResult<Box<dyn VhciBackend>> {
    #[cfg(target_os = "linux")]
    {
        let inner = vhci_linux::LinuxVhciBackend::new()?;
        Ok(Box::new(inner))
    }

    #[cfg(windows)]
    {
        return Ok(Box::new(vhci_windows::WindowsVhciBackend));
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(UsbIpError::from(ErrorKind::NotSupported(
            "VHCI is only supported on Linux and Windows".into(),
        )))
    }
}

// ─── Mock Backend (testing) ─────────────────────────────────────

#[cfg(test)]
pub(crate) struct MockVhciBackend {
    /// Track created devices.
    devices: Mutex<Vec<VhciDevice>>,
    /// Track completed URBs.
    urbs: Mutex<Vec<(u32, u32, i32, u32, Vec<u8>)>>,
    /// Next port number to assign.
    next_port: Mutex<u32>,
}

#[cfg(test)]
impl MockVhciBackend {
    pub(crate) fn new() -> Self {
        Self {
            devices: Mutex::new(Vec::new()),
            urbs: Mutex::new(Vec::new()),
            next_port: Mutex::new(0),
        }
    }

    /// Snapshot of URBs recorded by `complete_urb`. Returns `(seqnum,
    /// devid, status, actual_length, data)` tuples in completion order.
    /// Cloned so tests can assert without holding the mutex.
    pub(crate) fn recorded_urbs(&self) -> Vec<(u32, u32, i32, u32, Vec<u8>)> {
        self.urbs.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl VhciBackend for MockVhciBackend {
    fn create_device(
        &self,
        entry: &UsbIpDeviceEntry,
        _descriptors: &[u8],
    ) -> UsbIpResult<VhciDevice> {
        let mut port_guard = self.next_port.lock().unwrap();
        let port = *port_guard;
        *port_guard += 1;

        let device = VhciDevice {
            port,
            devid: port,
            busid: entry.busid_str().to_string(),
            vid: entry.vid(),
            pid: entry.pid(),
        };

        self.devices.lock().unwrap().push(device.clone());
        Ok(device)
    }

    fn complete_urb(
        &self,
        seqnum: u32,
        devid: u32,
        status: i32,
        actual_length: u32,
        data: &[u8],
    ) -> UsbIpResult<()> {
        self.urbs.lock().unwrap().push((seqnum, devid, status, actual_length, data.to_vec()));
        Ok(())
    }

    fn cancel_urb(&self, _seqnum: u32, _devid: u32) -> UsbIpResult<()> {
        Ok(())
    }

    fn remove_device(&self, _port: u32) -> UsbIpResult<()> {
        Ok(())
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal `UsbIpDeviceEntry` for testing.
    fn make_dummy_entry(busid: &str, vid: u16, pid: u16) -> UsbIpDeviceEntry {
        use zerocopy::byteorder::BigEndian;
        use zerocopy::FromZeros;

        let mut entry = UsbIpDeviceEntry::new_zeroed();
        let busid_bytes = busid.as_bytes();
        let copy_len = busid_bytes.len().min(31);
        entry.busid[..copy_len].copy_from_slice(&busid_bytes[..copy_len]);
        entry.id_vendor = zerocopy::byteorder::U16::<BigEndian>::new(vid);
        entry.id_product = zerocopy::byteorder::U16::<BigEndian>::new(pid);
        entry
    }

    #[test]
    fn test_mock_create_device() {
        let backend = MockVhciBackend::new();
        let entry = make_dummy_entry("1-2", 0x1234, 0x5678);
        let descriptors = vec![0u8; 64];

        let device = backend.create_device(&entry, &descriptors).unwrap();
        assert_eq!(device.port, 0);
        assert_eq!(device.vid, 0x1234);
        assert_eq!(device.pid, 0x5678);
        assert_eq!(backend.devices.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_mock_complete_urb() {
        let backend = MockVhciBackend::new();
        let data = vec![1u8, 2, 3, 4];

        backend.complete_urb(42, 0, 0, 4, &data).unwrap();

        let urbs = backend.recorded_urbs();
        assert_eq!(urbs.len(), 1);
        assert_eq!(urbs[0].0, 42); // seqnum
        assert_eq!(urbs[0].1, 0); // devid
        assert_eq!(urbs[0].3, 4); // actual_length
        assert_eq!(urbs[0].4, data);
    }

    #[test]
    fn test_mock_sequential_ports() {
        let backend = MockVhciBackend::new();
        let entry = make_dummy_entry("1-2", 0x1234, 0x5678);
        let descriptors = vec![0u8; 64];

        let d0 = backend.create_device(&entry, &descriptors).unwrap();
        assert_eq!(d0.port, 0);

        let d1 = backend.create_device(&entry, &descriptors).unwrap();
        assert_eq!(d1.port, 1);

        let d2 = backend.create_device(&entry, &descriptors).unwrap();
        assert_eq!(d2.port, 2);

        assert_eq!(backend.devices.lock().unwrap().len(), 3);
    }
}
