//! USB/IP Server — Export USB devices over TCP.
//!
//! ## Architecture
//!
//! ```text
//! main()
//!   ├─ mDNS thread: publishes _usbip._tcp.local
//!   ├─ TCP accept loop (port 3240)
//!   │    └─ per-client task
//!   │         ├─ handle_devlist()   → OP_REQ_DEVLIST / OP_REP_DEVLIST
//!   │         ├─ handle_import()    → OP_REQ_IMPORT / OP_REP_IMPORT
//!   │         └─ handle_urb_loop()  → USBIP_CMD_SUBMIT / USBIP_RET_SUBMIT
//!   └─ hotplug monitor: libusb hotplug callbacks
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use usbip_core::protocol::*;
use usbip_core::urb::*;
use usbip_core::descriptor::*;
use usbip_core::error::*;
use usbip_core::*;

mod usb;
mod discovery;

use usb::UsbDeviceManager;
use discovery::MdnsAdvertiser;

/// Global server state.
pub struct Server {
    /// USB device manager (libusb context).
    pub usb: Arc<UsbDeviceManager>,
    /// Active exports: busid → (client_addr, device_info).
    pub exports: Mutex<HashMap<String, (SocketAddr, UsbIpDeviceEntry)>>,
    /// mDNS advertiser.
    pub mdns: Option<MdnsAdvertiser>,
    /// Server configuration.
    pub config: ServerConfig,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub allowed_vid_pid: Vec<(u16, u16)>,
    pub require_confirmation: bool,
    pub encryption_enabled: bool,
    pub tcp_nodelay: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: USBIP_PORT,
            allowed_vid_pid: Vec::new(),
            require_confirmation: true,
            encryption_enabled: false,
            tcp_nodelay: true,
        }
    }
}

impl Server {
    pub async fn new(config: ServerConfig) -> UsbIpResult<Self> {
        let usb = UsbDeviceManager::new()?;
        let mdns = MdnsAdvertiser::new(config.port).ok();
        Ok(Self {
            usb: Arc::new(usb),
            exports: Mutex::new(HashMap::new()),
            mdns,
            config,
        })
    }

    /// Run the server — listens forever.
    pub async fn run(&self) -> UsbIpResult<()> {
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        info!("USB/IP server listening on {}", addr);

        // Start mDNS advertising
        if let Some(ref mdns) = self.mdns {
            mdns.start()?;
            info!("mDNS advertising _usbip._tcp.local");
        }

        // Accept loop
        loop {
            let (stream, peer_addr) = listener.accept().await?;
            info!("Client connected from {}", peer_addr);

            let usb = self.usb.clone();
            let exports = self.exports.clone();
            let config = self.config.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_client(stream, peer_addr, usb, exports, config).await {
                    error!("Client {} error: {}", peer_addr, e);
                }
            });
        }
    }

    /// Get list of exportable devices.
    pub async fn exportable_devices(&self) -> Vec<UsbIpDeviceEntry> {
        self.usb.list_devices()
    }
}

/// Handle one TCP client connection.
async fn handle_client(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    usb: Arc<UsbDeviceManager>,
    exports: Arc<Mutex<HashMap<String, (SocketAddr, UsbIpDeviceEntry)>>>,
    config: ServerConfig,
) -> UsbIpResult<()> {
    if config.tcp_nodelay {
        stream.set_nodelay(true)?;
    }

    // Read header (8 bytes)
    let mut header_buf = [0u8; 8];
    stream.read_exact(&mut header_buf).await?;

    let header = UsbIpHeader::read_from_prefix(&header_buf)
        .ok_or_else(|| UsbIpError::Protocol("invalid header".into()))?
        .clone();

    debug!("Received command: 0x{:04x}", header.command.get());

    match header.command.get() {
        OP_REQ_DEVLIST => handle_devlist(&mut stream, &usb).await?,
        OP_REQ_IMPORT => handle_import(&mut stream, &usb, &exports, peer_addr).await?,
        _ => {
            warn!("Unknown command: 0x{:04x}", header.command.get());
        }
    }

    Ok(())
}

/// Handle OP_REQ_DEVLIST: return all exportable devices.
async fn handle_devlist(
    stream: &mut TcpStream,
    usb: &UsbDeviceManager,
) -> UsbIpResult<()> {
    let devices = usb.list_devices();
    let ndev = devices.len() as u32;

    debug!("Sending device list: {} devices", ndev);

    // Build reply: header + ndev + device entries
    let mut reply = Vec::with_capacity(8 + 4 + ndev as usize * UsbIpDeviceEntry::SIZE);

    // Header
    let header = UsbIpHeader::new(OP_REP_DEVLIST);
    reply.extend_from_slice(header.as_bytes());

    // ndev (4 bytes, big-endian)
    reply.extend_from_slice(&ndev.to_be_bytes());

    // Device entries
    for dev in &devices {
        reply.extend_from_slice(dev.as_bytes());
    }

    stream.write_all(&reply).await?;
    stream.flush().await?;

    Ok(())
}

/// Handle OP_REQ_IMPORT: client wants to import a specific device.
async fn handle_import(
    stream: &mut TcpStream,
    usb: &UsbDeviceManager,
    exports: &Mutex<HashMap<String, (SocketAddr, UsbIpDeviceEntry)>>,
    peer_addr: SocketAddr,
) -> UsbIpResult<()> {
    // Read busid (32 bytes)
    let mut busid_buf = [0u8; 32];
    stream.read_exact(&mut busid_buf).await?;

    let busid = String::from_utf8_lossy(
        &busid_buf[..busid_buf.iter().position(|&b| b == 0).unwrap_or(32)]
    ).to_string();

    info!("Client {} wants to import device: {}", peer_addr, busid);

    // Check if device exists
    let device_entry = usb.get_device_entry(&busid)
        .ok_or_else(|| UsbIpError::DeviceNotFound(busid.clone()))?;

    // Check if already exported
    {
        let mut exports = exports.lock().await;
        if exports.contains_key(&busid) {
            // Send busy error
            let header = UsbIpHeader::with_status(OP_REP_IMPORT, STATUS_ST_DEV_BUSY);
            stream.write_all(header.as_bytes()).await?;
            return Ok(());
        }
        exports.insert(busid.clone(), (peer_addr, device_entry.clone()));
    }

    // Claim the device for USB/IP
    usb.claim_device(&busid)?;

    // Send OP_REP_IMPORT success with device entry + descriptor tree
    let descriptors = usb.get_descriptor_tree(&busid)?;

    let mut reply = Vec::new();
    let header = UsbIpHeader::new(OP_REP_IMPORT);
    reply.extend_from_slice(header.as_bytes());
    reply.extend_from_slice(device_entry.as_bytes());
    reply.extend_from_slice(&descriptors);

    stream.write_all(&reply).await?;

    // Enter URB forwarding loop
    handle_urb_loop(stream, usb, exports, busid, peer_addr).await
}

/// Main URB forwarding loop after device import.
async fn handle_urb_loop(
    stream: &mut TcpStream,
    usb: &UsbDeviceManager,
    exports: &Mutex<HashMap<String, (SocketAddr, UsbIpDeviceEntry)>>,
    busid: String,
    peer_addr: SocketAddr,
) -> UsbIpResult<()> {
    let mut header_buf = [0u8; 8];
    let mut seqnum: u32 = 0;

    loop {
        // Read header
        if stream.read_exact(&mut header_buf).await.is_err() {
            break; // client disconnected
        }

        let header = match UsbIpHeader::read_from_prefix(&header_buf) {
            Some(h) => h.clone(),
            None => break,
        };

        match header.command.get() {
            USBIP_CMD_SUBMIT => {
                // Read CMD_SUBMIT struct
                let mut cmd_buf = vec![0u8; UsbIpCmdSubmit::HEADER_SIZE];
                stream.read_exact(&mut cmd_buf).await?;

                let cmd = UsbIpCmdSubmit::read_from_prefix(&cmd_buf)
                    .ok_or_else(|| UsbIpError::Protocol("invalid CMD_SUBMIT".into()))?
                    .clone();

                let data_len = cmd.data_len() as usize;

                // Read data if OUT transfer
                let mut data = vec![0u8; data_len];
                if !cmd.is_in() && data_len > 0 {
                    stream.read_exact(&mut data).await?;
                }

                // Execute URB on physical device
                match usb.execute_urb(&busid, &cmd, &data) {
                    Ok((status, actual_len, in_data)) => {
                        // Build RET_SUBMIT
                        let ret = UsbIpRetSubmit {
                            seqnum:            cmd.seqnum,
                            devid:             cmd.devid,
                            direction:         cmd.direction,
                            ep:                cmd.ep,
                            status:            U32BE::new(status as u32),
                            actual_length:     U32BE::new(actual_len),
                            start_frame:       cmd.start_frame,
                            number_of_packets: cmd.number_of_packets,
                            error_count:       U32BE::new(0),
                            setup:             cmd.setup,
                        };

                        let mut reply = Vec::new();
                        let ret_header = UsbIpHeader::new(USBIP_RET_SUBMIT);
                        reply.extend_from_slice(ret_header.as_bytes());
                        reply.extend_from_slice(ret.as_bytes());
                        if !in_data.is_empty() {
                            reply.extend_from_slice(&in_data);
                        }

                        stream.write_all(&reply).await?;
                    }
                    Err(e) => {
                        warn!("URB error on {}: {}", busid, e);
                        let ret = UsbIpRetSubmit {
                            seqnum:            cmd.seqnum,
                            devid:             cmd.devid,
                            direction:         cmd.direction,
                            ep:                cmd.ep,
                            status:            U32BE::new(rusb_to_urb_status(
                                &e.downcast_ref::<rusb::Error>()
                                    .cloned()
                                    .unwrap_or(rusb::Error::Other),
                            ) as u32),
                            actual_length:     U32BE::new(0),
                            start_frame:       cmd.start_frame,
                            number_of_packets: cmd.number_of_packets,
                            error_count:       U32BE::new(1),
                            setup:             cmd.setup,
                        };

                        let mut reply = Vec::new();
                        let ret_header = UsbIpHeader::new(USBIP_RET_SUBMIT);
                        reply.extend_from_slice(ret_header.as_bytes());
                        reply.extend_from_slice(ret.as_bytes());

                        stream.write_all(&reply).await?;
                    }
                }

                seqnum = seqnum.wrapping_add(1);
            }
            _ => {
                debug!("Unknown command in URB loop: 0x{:04x}", header.command.get());
            }
        }
    }

    // Cleanup
    usb.release_device(&busid)?;
    exports.lock().await.remove(&busid);
    info!("Client {} disconnected, released {}", peer_addr, busid);

    Ok(())
}
