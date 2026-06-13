//! USB device management via libusb (rusb).
//!
//! Handles device enumeration, claiming, URB submission, and hotplug.

use rusb::{Context, Device, DeviceHandle, HotplugBuilder, UsbContext};
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{debug, warn};

use usbip_core::descriptor::*;
use usbip_core::error::*;
use usbip_core::protocol::*;
use usbip_core::urb::*;

/// Manages USB devices for the server.
pub struct UsbDeviceManager {
    context: Context,
    /// busid → (DeviceHandle, claimed)
    handles: Mutex<HashMap<String, (DeviceHandle<Context>, bool)>>,
}

impl UsbDeviceManager {
    pub fn new() -> UsbIpResult<Self> {
        let context = Context::new()?;
        Ok(Self { context, handles: Mutex::new(HashMap::new()) })
    }

    /// List all USB devices on the system.
    pub fn list_devices(&self) -> Vec<UsbIpDeviceEntry> {
        let mut devices = Vec::new();

        let dev_list = match self.context.devices() {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to enumerate USB devices: {}", e);
                return devices;
            },
        };

        for device in dev_list.iter() {
            let desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };

            let busnum = device.bus_number();
            let devnum = device.address();
            let speed = device.speed() as u32;

            // Build busid: "busnum-port.port.port"
            let busid = format!("{}-{}", busnum, devnum);
            // For real sysfs path, we'd read from the device's port chain
            let path = format!("/sys/bus/usb/devices/{}-{}", busnum, devnum);

            let mut entry = UsbIpDeviceEntry {
                path: [0u8; 256],
                busid: [0u8; 32],
                busnum: U32BE::new(busnum.into()),
                devnum: U32BE::new(devnum.into()),
                speed: U32BE::new(speed),
                id_vendor: U16BE::new(desc.vendor_id()),
                id_product: U16BE::new(desc.product_id()),
                bcd_device: U16BE::new(desc.device_version().0),
                b_device_class: desc.class_code(),
                b_device_sub_class: desc.sub_class_code(),
                b_device_protocol: desc.protocol_code(),
                b_configuration_value: 0,
                b_num_configurations: desc.num_configurations(),
                b_num_interfaces: 0, // filled below
            };

            // Copy strings into fixed arrays
            let path_bytes = path.as_bytes();
            let copy_len = path_bytes.len().min(255);
            entry.path[..copy_len].copy_from_slice(&path_bytes[..copy_len]);

            let busid_bytes = busid.as_bytes();
            let copy_len = busid_bytes.len().min(31);
            entry.busid[..copy_len].copy_from_slice(&busid_bytes[..copy_len]);

            // Count interfaces from config descriptor
            if let Ok(config) = device.config_descriptor(0) {
                entry.b_num_interfaces = config.num_interfaces();
                entry.b_configuration_value = config.number();
            }

            devices.push(entry);
        }

        devices
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
        let (busnum, devnum) = parse_busid(busid)?;

        let device = self.find_device(busnum, devnum)?;
        let mut handle = device.open()?;

        // Detach kernel driver if active (Linux/Mac)
        let desc = device.device_descriptor()?;
        let config = device.config_descriptor(0)?;

        for iface_idx in 0..config.num_interfaces() {
            let iface_num = config
                .interfaces()
                .nth(iface_idx as usize)
                .and_then(|i| i.descriptors().next())
                .map(|d| d.interface_number());

            if let Some(num) = iface_num {
                // Try to detach kernel driver (ignore errors on platforms without this)
                let _ = handle.detach_kernel_driver(num);
                handle.claim_interface(num)?;
            }
        }

        self.handles.lock().unwrap().insert(busid.to_string(), (handle, true));

        debug!("Claimed device: {}", busid);
        Ok(())
    }

    /// Get the full USB descriptor tree for a device.
    pub fn get_descriptor_tree(&self, busid: &str) -> UsbIpResult<Vec<u8>> {
        let (busnum, devnum) = parse_busid(busid)?;

        let device = self.find_device(busnum, devnum)?;
        let desc = device.device_descriptor()?;

        let mut tree = Vec::new();

        // Device descriptor (18 bytes, LE)
        tree.extend_from_slice(&desc_to_bytes(&desc));

        // Config descriptor + all interfaces + endpoints
        for config_idx in 0..desc.num_configurations() {
            let config = device.config_descriptor(config_idx)?;

            // Config descriptor (9 bytes, LE)
            let desc_bytes = [
                config.descriptor().length(),
                config.descriptor().descriptor_type(),
                (config.descriptor().w_total_length() & 0xFF) as u8,
                ((config.descriptor().w_total_length() >> 8) & 0xFF) as u8,
                config.descriptor().b_num_interfaces(),
                config.descriptor().b_configuration_value(),
                config.descriptor().i_configuration(),
                config.descriptor().bm_attributes(),
                config.descriptor().b_max_power(),
            ];
            tree.extend_from_slice(&desc_bytes);

            for iface in config.interfaces() {
                for iface_desc in iface.descriptors() {
                    // Interface descriptor (9 bytes, LE)
                    let iface_bytes = [
                        iface_desc.length(),
                        iface_desc.descriptor_type(),
                        iface_desc.interface_number(),
                        iface_desc.alternate_setting(),
                        iface_desc.num_endpoints(),
                        iface_desc.class_code(),
                        iface_desc.sub_class_code(),
                        iface_desc.protocol_code(),
                        iface_desc.interface_string_index(),
                    ];
                    tree.extend_from_slice(&iface_bytes);

                    // HID descriptor (if class is HID)
                    if iface_desc.class_code() == 0x03 {
                        if let Ok(extra) = iface_desc.extra() {
                            tree.extend_from_slice(extra);
                        }
                    }

                    // Endpoint descriptors
                    for ep_desc in iface_desc.endpoint_descriptors() {
                        let ep_bytes = [
                            ep_desc.length(),
                            ep_desc.descriptor_type(),
                            ep_desc.address(),
                            ep_desc.transfer_type() as u8,
                            (ep_desc.max_packet_size() & 0xFF) as u8,
                            ((ep_desc.max_packet_size() >> 8) & 0xFF) as u8,
                            ep_desc.interval(),
                        ];
                        tree.extend_from_slice(&ep_bytes);
                    }
                }
            }
        }

        Ok(tree)
    }

    /// Execute a URB (submit a USB transfer) on the physical device.
    pub fn execute_urb(
        &self,
        busid: &str,
        cmd: &UsbIpCmdSubmit,
        out_data: &[u8],
    ) -> UsbIpResult<(i32, u32, Vec<u8>)> {
        let handles = self.handles.lock().unwrap();
        let (handle, _claimed) =
            handles.get(busid).ok_or_else(|| UsbIpError::DeviceNotFound(busid.into()))?;

        let ep_addr = cmd.ep_num() as u8;
        let timeout = std::time::Duration::from_millis(5000); // 5s timeout
        let is_in = cmd.is_in();
        let is_control = cmd.is_control();

        if is_control {
            // Control transfer
            let setup_packet = cmd.setup;
            let bm_request_type = setup_packet[0];
            let b_request = setup_packet[1];
            let w_value = u16::from_le_bytes([setup_packet[2], setup_packet[3]]);
            let w_index = u16::from_le_bytes([setup_packet[4], setup_packet[5]]);
            let w_length = u16::from_le_bytes([setup_packet[6], setup_packet[7]]);

            if is_in {
                let mut buf = vec![0u8; w_length as usize];
                let len = handle.read_control(
                    bm_request_type,
                    b_request,
                    w_value,
                    w_index,
                    &mut buf,
                    timeout,
                )?;
                buf.truncate(len);
                Ok((0, len as u32, buf))
            } else {
                let len = handle.write_control(
                    bm_request_type,
                    b_request,
                    w_value,
                    w_index,
                    out_data,
                    timeout,
                )?;
                Ok((0, len as u32, Vec::new()))
            }
        } else if is_in {
            // Bulk/Interrupt IN
            let max_size = cmd.data_len().max(512) as usize;
            let mut buf = vec![0u8; max_size];
            let len = handle.read_bulk(ep_addr, &mut buf, timeout)?;
            buf.truncate(len);
            Ok((0, len as u32, buf))
        } else {
            // Bulk/Interrupt OUT
            let len = handle.write_bulk(ep_addr, out_data, timeout)?;
            Ok((0, len as u32, Vec::new()))
        }
    }

    /// Release a claimed device.
    pub fn release_device(&self, busid: &str) -> UsbIpResult<()> {
        let mut handles = self.handles.lock().unwrap();
        if let Some((handle, _)) = handles.remove(busid) {
            // Release interfaces
            if let Ok(device) = handle.device() {
                if let Ok(config) = device.active_config_descriptor() {
                    for iface in config.interfaces() {
                        for desc in iface.descriptors() {
                            let num = desc.interface_number();
                            let _ = handle.release_interface(num);
                            let _ = handle.attach_kernel_driver(num);
                        }
                    }
                }
            }
        }
        debug!("Released device: {}", busid);
        Ok(())
    }

    fn find_device(&self, busnum: u8, devnum: u8) -> UsbIpResult<Device<Context>> {
        let devices = self.context.devices()?;
        for device in devices.iter() {
            if device.bus_number() == busnum && device.address() == devnum {
                return Ok(device);
            }
        }
        Err(UsbIpError::DeviceNotFound(format!("bus {} dev {}", busnum, devnum)))
    }
}

fn desc_to_bytes(desc: &rusb::DeviceDescriptor) -> Vec<u8> {
    vec![
        desc.length(),
        desc.descriptor_type(),
        (desc.usb_version().0 & 0xFF) as u8,
        ((desc.usb_version().0 >> 8) & 0xFF) as u8,
        desc.class_code(),
        desc.sub_class_code(),
        desc.protocol_code(),
        desc.max_packet_size_0(),
        (desc.vendor_id() & 0xFF) as u8,
        ((desc.vendor_id() >> 8) & 0xFF) as u8,
        (desc.product_id() & 0xFF) as u8,
        ((desc.product_id() >> 8) & 0xFF) as u8,
        (desc.device_version().0 & 0xFF) as u8,
        ((desc.device_version().0 >> 8) & 0xFF) as u8,
        desc.manufacturer_string_index(),
        desc.product_string_index(),
        desc.serial_number_string_index(),
        desc.num_configurations(),
    ]
}

/// Parse a busid string ("busnum-devnum") into its numeric components.
fn parse_busid(busid: &str) -> UsbIpResult<(u8, u8)> {
    let parts: Vec<&str> = busid.split('-').collect();
    if parts.len() < 2 {
        return Err(UsbIpError::DeviceNotFound(busid.into()));
    }
    let busnum: u8 = parts[0].parse().map_err(|_| UsbIpError::DeviceNotFound(busid.into()))?;
    let devnum: u8 = parts[1].parse().map_err(|_| UsbIpError::DeviceNotFound(busid.into()))?;
    Ok((busnum, devnum))
}
