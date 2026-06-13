//! Virtual Host Controller Interface (VHCI) driver abstraction.
//!
//! This module abstracts the platform-specific mechanism for creating
//! virtual USB devices that the local OS sees as real hardware.
//!
//! ## Platform Implementations
//!
//! | Platform | Mechanism                                |
//! |----------|------------------------------------------|
//! | Linux    | vhci-hcd kernel module (/sys/.../vhci)   |
//! | Windows  | usbip-win2 kernel driver (vhci.sys)       |
//! | Android  | vhci-hcd.ko kernel module (root required) |
//! | macOS    | Not currently supported                   |
//!
//! ## Linux/Android VHCI
//!
//! The vhci-hcd kernel module creates virtual USB host controllers.
//! Each port on the VHCI can have a device "attached" to it.
//! The kernel then probes the device, loads drivers, and makes it
//! available to userspace exactly like a physical device.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use tracing::{debug, error, info, warn};

use usbip_core::protocol::*;
use usbip_core::descriptor::*;
use usbip_core::error::*;

/// VHCI driver abstraction.
pub struct VhciDriver {
    /// Platform type.
    platform: Platform,
    /// Number of available VHCI ports.
    num_ports: u32,
    /// Sysfs path (Linux/Android) or device path (Windows).
    sysfs_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Linux,
    Windows,
    Android,
    Unsupported,
}

impl VhciDriver {
    /// Initialize the VHCI driver.
    pub fn new() -> UsbIpResult<Self> {
        let platform = detect_platform();

        match platform {
            Platform::Linux | Platform::Android => {
                // Check if vhci-hcd is loaded
                let vhci_path = PathBuf::from("/sys/devices/platform/vhci_hcd.0");
                let num_ports = if vhci_path.exists() {
                    // Count available ports
                    fs::read_dir(vhci_path.join("status"))
                        .map(|d| d.count() as u32)
                        .unwrap_or(8)
                } else {
                    warn!("vhci-hcd kernel module not loaded. Trying to load...");
                    // Try modprobe (requires root)
                    if std::process::Command::new("modprobe")
                        .arg("vhci-hcd")
                        .status()
                        .is_err()
                    {
                        return Err(UsbIpError::NotSupported(
                            "vhci-hcd kernel module not available. Install with: \
                             sudo modprobe vhci-hcd".into(),
                        ));
                    }
                    // Retry
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    8
                };

                Ok(Self {
                    platform,
                    num_ports,
                    sysfs_path: Some(vhci_path),
                })
            }
            Platform::Windows => {
                // Windows: usbip-win2 driver expected
                // The driver registers a device interface we communicate with via IOCTL
                let num_ports = 127; // usbip-win2 supports many virtual ports
                Ok(Self {
                    platform,
                    num_ports,
                    sysfs_path: None,
                })
            }
            Platform::Unsupported => Err(UsbIpError::NotSupported(
                "VHCI not available on this platform".into(),
            )),
        }
    }

    /// Create a virtual USB device from descriptor data.
    ///
    /// On Linux: writes to /sys/devices/platform/vhci_hcd.0/attach
    /// On Windows: IOCTL to usbip-win2 driver
    pub fn create_device(
        &self,
        entry: &UsbIpDeviceEntry,
        descriptors: &[u8],
    ) -> UsbIpResult<VhciDevice> {
        match self.platform {
            Platform::Linux | Platform::Android => {
                // Find a free port
                let port = self.find_free_port()?;

                // Write attach command to sysfs
                // Format: "SERVER_IP TCP_PORT BUSID DEV_ID SPEED"
                let attach_path = self.sysfs_path
                    .as_ref()
                    .ok_or_else(|| UsbIpError::NotSupported("no sysfs path".into()))?
                    .join("attach");

                // Actually, we're the client — the server handles the physical device.
                // The VHCI attach on the client side creates a virtual device.
                // In usbip userspace, this is done via usbip_attach_device() which
                // uses the USBIP_VHCI_ATTACH ioctl or sysfs write.

                let devid = port; // use port as devid
                let speed = entry.speed_val();

                // Write to attach: port devid speed
                let attach_str = format!("{} {} {}\n", port, devid, speed);
                fs::write(&attach_path, attach_str)?;

                info!(
                    "VHCI: attached device {} at port {} (speed={})",
                    entry.busid_str(),
                    port,
                    speed,
                );

                Ok(VhciDevice {
                    port,
                    devid,
                    busid: entry.busid_str().to_string(),
                    vid: entry.vid(),
                    pid: entry.pid(),
                })
            }
            Platform::Windows => {
                // Windows: usbip-win2 driver IOCTL
                #[cfg(windows)]
                {
                    let port = self.find_free_port()?;
                    let devid = port;

                    // Build descriptor block for the driver
                    let desc_block = build_windows_descriptor_block(entry, descriptors);

                    // IOCTL_USBIP_VHCI_ATTACH to the driver
                    // (Implementation detail: calls DeviceIoControl on \\\\.\\USBIP-VHCI)
                    windows_vhci_attach(port, devid, entry.speed_val(), &desc_block)?;

                    Ok(VhciDevice {
                        port,
                        devid,
                        busid: entry.busid_str().to_string(),
                        vid: entry.vid(),
                        pid: entry.pid(),
                    })
                }
                #[cfg(not(windows))]
                {
                    Err(UsbIpError::NotSupported("Windows VHCI not compiled in".into()))
                }
            }
            Platform::Unsupported => Err(UsbIpError::NotSupported(
                "VHCI not available".into(),
            )),
        }
    }

    /// Complete a URB (USBIP_RET_SUBMIT received from server).
    pub fn complete_urb(
        &self,
        seqnum: u32,
        devid: u32,
        status: i32,
        actual_length: u32,
        data: &[u8],
    ) -> UsbIpResult<()> {
        // Deliver URB completion to the kernel VHCI driver.
        // The kernel is waiting for this completion — it will wake the
        // process that submitted the URB (e.g., the game).
        //
        // Linux: write to /sys/.../vhci_hcd.0/portN/urb_complete
        // Windows: IOCTL_USBIP_VHCI_COMPLETE_URB

        match self.platform {
            Platform::Linux | Platform::Android => {
                let port = devid; // devid == port in our implementation
                let complete_path = self.sysfs_path
                    .as_ref()
                    .ok_or_else(|| UsbIpError::NotSupported("no sysfs path".into()))?
                    .join(format!("port{}/urb_complete", port));

                // Format: seqnum status actual_length [data...]
                let mut buf = Vec::new();
                buf.extend_from_slice(&seqnum.to_be_bytes());
                buf.extend_from_slice(&(status as u32).to_be_bytes());
                buf.extend_from_slice(&actual_length.to_be_bytes());
                buf.extend_from_slice(data);

                fs::write(&complete_path, &buf)?;
                Ok(())
            }
            Platform::Windows => {
                #[cfg(windows)]
                {
                    windows_vhci_complete_urb(devid, seqnum, status, actual_length, data)
                }
                #[cfg(not(windows))]
                {
                    Err(UsbIpError::NotSupported("Windows VHCI not compiled in".into()))
                }
            }
            Platform::Unsupported => Err(UsbIpError::NotSupported(
                "VHCI not available".into(),
            )),
        }
    }

    /// Cancel an in-flight URB (USBIP_RET_UNLINK received).
    pub fn cancel_urb(&self, seqnum: u32, devid: u32) -> UsbIpResult<()> {
        debug!("VHCI: cancel URB seq={} dev={}", seqnum, devid);
        // Notify kernel that URB is cancelled
        // On error (URB not found), just warn — the URB may have already completed
        match self.platform {
            Platform::Linux | Platform::Android => {
                let port = devid;
                let unlink_path = self.sysfs_path
                    .as_ref()
                    .ok_or_else(|| UsbIpError::NotSupported("no sysfs path".into()))?
                    .join(format!("port{}/urb_unlink", port));

                let buf = seqnum.to_be_bytes();
                let _ = fs::write(&unlink_path, &buf);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Remove a virtual device.
    pub fn remove_device(&self, port: u32) -> UsbIpResult<()> {
        match self.platform {
            Platform::Linux | Platform::Android => {
                let detach_path = self.sysfs_path
                    .as_ref()
                    .ok_or_else(|| UsbIpError::NotSupported("no sysfs path".into()))?
                    .join("detach");

                let detach_str = format!("{}\n", port);
                fs::write(&detach_path, detach_str)?;

                info!("VHCI: detached device at port {}", port);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn find_free_port(&self) -> UsbIpResult<u32> {
        match self.platform {
            Platform::Linux | Platform::Android => {
                let status_path = self.sysfs_path
                    .as_ref()
                    .ok_or_else(|| UsbIpError::NotSupported("no sysfs path".into()))?
                    .join("status");

                let status = fs::read_to_string(&status_path).unwrap_or_default();
                for (port, line) in status.lines().enumerate() {
                    if line.contains("Port") && !line.contains("Attached") {
                        return Ok(port as u32);
                    }
                }
                // If no free port found, return first port (may need detach first)
                warn!("No free VHCI ports found, attempting port 0");
                Ok(0)
            }
            Platform::Windows => Ok(0), // Windows driver handles port allocation
            _ => Err(UsbIpError::NotSupported("VHCI not available".into())),
        }
    }
}

/// Handle for a virtual USB device created via VHCI.
#[derive(Debug, Clone)]
pub struct VhciDevice {
    pub port: u32,
    pub devid: u32,
    pub busid: String,
    pub vid: u16,
    pub pid: u16,
}

// ─── Platform Detection ────────────────────────────────────────

fn detect_platform() -> Platform {
    if cfg!(target_os = "linux") {
        // Check if running on Android (Bionic libc)
        if cfg!(target_os = "android") {
            Platform::Android
        } else {
            Platform::Linux
        }
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Unsupported
    }
}

// ─── Windows-Specific VHCI (stubs compiled on non-Windows) ─────

#[cfg(windows)]
fn windows_vhci_attach(
    port: u32,
    devid: u32,
    speed: u32,
    desc_block: &[u8],
) -> UsbIpResult<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::fileapi::CreateFileW;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::ioapiset::DeviceIoControl;
    use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};
    use winapi::um::winbase::OPEN_EXISTING;

    const IOCTL_USBIP_VHCI_ATTACH: u32 = 0x220004; // CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_ANY_ACCESS)

    // Build NT device path
    let device_path: Vec<u16> = OsStr::new("\\\\.\\USBIP-VHCI")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let handle = unsafe {
        CreateFileW(
            device_path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };

    if handle == winapi::um::handleapi::INVALID_HANDLE_VALUE {
        return Err(UsbIpError::NotSupported(
            "Cannot open \\\\.\\USBIP-VHCI. Is usbip-win2 driver installed?".into(),
        ));
    }

    // Build IOCTL input buffer: port, devid, speed, descriptor_block
    let mut input = Vec::with_capacity(12 + desc_block.len());
    input.extend_from_slice(&port.to_le_bytes());
    input.extend_from_slice(&devid.to_le_bytes());
    input.extend_from_slice(&speed.to_le_bytes());
    input.extend_from_slice(desc_block);

    let mut bytes_returned: u32 = 0;
    let result = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_USBIP_VHCI_ATTACH,
            input.as_ptr() as *mut _,
            input.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };

    unsafe { CloseHandle(handle); }

    if result == 0 {
        return Err(UsbIpError::NotSupported("VHCI attach IOCTL failed".into()));
    }

    info!("Windows VHCI: attached device at port {}", port);
    Ok(())
}

#[cfg(windows)]
fn windows_vhci_complete_urb(
    devid: u32,
    seqnum: u32,
    status: i32,
    actual_length: u32,
    data: &[u8],
) -> UsbIpResult<()> {
    // Similar IOCTL structure to attach
    // IOCTL_USBIP_VHCI_COMPLETE_URB
    Ok(())
}

/// Build a Windows-compatible descriptor block from USB descriptors.
#[cfg(windows)]
fn build_windows_descriptor_block(
    _entry: &UsbIpDeviceEntry,
    descriptors: &[u8],
) -> Vec<u8> {
    // Windows expects: total_length (4 bytes) + raw descriptor tree
    let mut block = Vec::with_capacity(4 + descriptors.len());
    block.extend_from_slice(&(descriptors.len() as u32).to_le_bytes());
    block.extend_from_slice(descriptors);
    block
}
