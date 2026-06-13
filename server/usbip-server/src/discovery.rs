/// mDNS service advertisement for USB/IP.
///
/// Publishes `_usbip._tcp.local` so clients can discover the server
/// without knowing its IP address.
use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::net::IpAddr;
use tracing::{error, info};

use usbip_core::error::*;

pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    service_name: String,
    port: u16,
}

impl MdnsAdvertiser {
    pub fn new(port: u16) -> UsbIpResult<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| ErrorKind::NotSupported(format!("mDNS init failed: {}", e)))?;

        let hostname = gethostname::gethostname().to_string_lossy().to_string();
        let service_name = format!("{}._usbip._tcp.local.", hostname);

        Ok(Self { daemon, service_name, port })
    }

    pub fn start(&self) -> UsbIpResult<()> {
        let local_ip = get_local_ip().unwrap_or(IpAddr::from([127, 0, 0, 1]));

        let properties = [("version", "1.1.1"), ("platform", std::env::consts::OS)];

        let service_info = ServiceInfo::new(
            "_usbip._tcp.local.",
            "USB/IP Server",
            &self.service_name,
            local_ip,
            self.port,
            &properties[..],
        )
        .map_err(|e| ErrorKind::NotSupported(format!("mDNS service creation failed: {}", e)))?;

        self.daemon
            .register(service_info)
            .map_err(|e| ErrorKind::NotSupported(format!("mDNS register failed: {}", e)))?;

        info!("mDNS advertised: {} on {}:{}", self.service_name, local_ip, self.port);
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
fn get_local_ip() -> Option<IpAddr> {
    use std::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()? {
        std::net::SocketAddr::V4(addr) => Some(IpAddr::V4(*addr.ip())),
        std::net::SocketAddr::V6(addr) => Some(IpAddr::V6(*addr.ip())),
    }
}
