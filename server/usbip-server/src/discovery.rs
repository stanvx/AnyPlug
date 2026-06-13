/// mDNS service advertisement for USB/IP.
///
/// Publishes `_usbip._tcp.local` so clients can discover the server
/// without knowing its IP address.

use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::net::Ipv4Addr;
use tracing::{debug, error, info};

use usbip_core::error::*;

pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    service_name: String,
    port: u16,
}

impl MdnsAdvertiser {
    pub fn new(port: u16) -> UsbIpResult<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| UsbIpError::NotSupported(format!("mDNS init failed: {}", e)))?;

        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "usbip-server".to_string());

        let service_name = format!("{}._usbip._tcp.local.", hostname);

        Ok(Self {
            daemon,
            service_name,
            port,
        })
    }

    pub fn start(&self) -> UsbIpResult<()> {
        // Get local IP (non-loopback)
        let local_ip = get_local_ip().unwrap_or(Ipv4Addr::new(127, 0, 0, 1));

        let properties = [
            ("version", "1.1.1"),
            ("platform", std::env::consts::OS),
        ];

        let service_info = ServiceInfo::new(
            "_usbip._tcp.local.",
            "USB/IP Server",
            &self.service_name,
            &local_ip,
            self.port,
            &properties[..],
        )
        .map_err(|e| UsbIpError::NotSupported(format!("mDNS service creation failed: {}", e)))?;

        self.daemon
            .register(service_info)
            .map_err(|e| UsbIpError::NotSupported(format!("mDNS register failed: {}", e)))?;

        info!(
            "mDNS advertised: {} on {}:{}",
            self.service_name, local_ip, self.port
        );
        Ok(())
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        if let Err(e) = self.daemon.unregister(&self.service_name) {
            error!("mDNS unregister error: {}", e);
        }
        let _ = self.daemon.shutdown();
    }
}

/// Get the first non-loopback IPv4 address.
fn get_local_ip() -> Option<Ipv4Addr> {
    use std::net::UdpSocket;

    // Connect a UDP socket to a dummy address to discover the local IP
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()? {
        std::net::SocketAddr::V4(addr) => Some(*addr.ip()),
        _ => None,
    }
}
