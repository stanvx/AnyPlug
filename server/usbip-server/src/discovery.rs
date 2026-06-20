/// mDNS service advertisement for USB/IP.
///
/// Publishes `_usbip._tcp.local` so clients can discover the server
/// without knowing its IP address. Each advertised service also carries
/// a `devices` TXT key listing the exportable USB devices, formatted as
/// comma-separated `vid=0xVVVV,pid=0xPPPP,bus=B-B,n=NAME` tuples.
///
/// The Android client (`ServerDiscovery.kt::parseDevices`) reads the
/// `devices` TXT key as its first strategy. Devices the server is not
/// willing to share are filtered out at the caller (via
/// `UsbDeviceManager::list_exportable_devices`) before reaching here, so
/// they never appear in the advertisement.
///
/// TXT single-value cap is 255 bytes; with ~30 bytes per device entry
/// that yields a practical limit of ~8 devices per server. Beyond that
/// the client will fall back to a manual connect — out of scope for v1.
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tracing::{error, info, warn};

use usbip_core::error::*;
use usbip_core::protocol::UsbIpDeviceEntry;

use crate::api::{DiscoveredServer, MdnsBrowser};

pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    service_name: String,
    port: u16,
    /// Devices to advertise in the `devices` TXT key. Already filtered
    /// by the caller's share policy (e.g. `allowed_vid_pid`).
    devices: Vec<UsbIpDeviceEntry>,
}

impl MdnsAdvertiser {
    pub fn new(port: u16, devices: Vec<UsbIpDeviceEntry>) -> UsbIpResult<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| ErrorKind::NotSupported(format!("mDNS init failed: {}", e)))?;

        let hostname = gethostname::gethostname().to_string_lossy().to_string();
        let service_name = format!("{}._usbip._tcp.local.", hostname);

        Ok(Self { daemon, service_name, port, devices })
    }

    pub fn start(&self) -> UsbIpResult<()> {
        let local_ip = get_local_ip().unwrap_or(IpAddr::from([127, 0, 0, 1]));

        // The mDNS daemon holds `&'static str` references for the
        // duration of the registration. We own the strings and leak
        // them once at boot — the daemon shutdown in `Drop` releases
        // the registration, the leaked bytes are reclaimed at process
        // exit. Bounded to one leak per `start()` call.
        let version: &'static str = Box::leak("1.1.1".to_owned().into_boxed_str());
        let platform: &'static str = Box::leak(std::env::consts::OS.to_owned().into_boxed_str());
        let devices_value: Option<&'static str> = if self.devices.is_empty() {
            None
        } else {
            let encoded = encode_devices_txt(&self.devices);
            Some(Box::leak(encoded.into_boxed_str()))
        };

        let mut properties: Vec<(&str, &str)> = vec![("version", version), ("platform", platform)];
        if let Some(d) = devices_value {
            properties.push(("devices", d));
        }

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

        info!(
            "mDNS advertised: {} on {}:{} ({} shared device(s))",
            self.service_name,
            local_ip,
            self.port,
            self.devices.len()
        );
        Ok(())
    }
}

/// Encode the device list as the `devices` TXT value.
///
/// Format: comma-separated `vid=0xVVVV,pid=0xPPPP,bus=B-B,n=NAME` tuples.
/// The `n=` field is the human-readable device name; today
/// `UsbIpDeviceEntry` carries no product string, so we fall back to a
/// stable `VVVV:PPPP` placeholder. A future change can plumb `iProduct`
/// from the descriptor tree.
pub fn encode_devices_txt(devices: &[UsbIpDeviceEntry]) -> String {
    devices
        .iter()
        .map(|d| {
            format!(
                "vid=0x{:04x},pid=0x{:04x},bus={},n={:04x}:{:04x}",
                d.vid(),
                d.pid(),
                d.busid_str(),
                d.vid(),
                d.pid()
            )
        })
        .collect::<Vec<_>>()
        .join(",")
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

/// mDNS browser for `_usbip._tcp.local`.
///
/// Used by the server's REST API (`POST /api/scan`) to discover remote
/// USB/IP servers on the LAN. Implements the `api::MdnsBrowser` trait
/// so tests can swap it out.
pub struct MdnsBrowserImpl {
    daemon: ServiceDaemon,
}

impl MdnsBrowserImpl {
    pub fn new() -> UsbIpResult<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| ErrorKind::NotSupported(format!("mDNS init failed: {}", e)))?;
        Ok(Self { daemon })
    }
}

impl MdnsBrowser for MdnsBrowserImpl {
    fn browse(&self, timeout_secs: u32) -> Vec<DiscoveredServer> {
        let receiver = match self.daemon.browse("_usbip._tcp.local.") {
            Ok(r) => r,
            Err(e) => {
                warn!("mDNS browse failed: {}", e);
                return Vec::new();
            },
        };

        let timeout = Duration::from_secs(timeout_secs as u64);
        let deadline = std::time::Instant::now() + timeout;
        let mut by_name: HashMap<String, DiscoveredServer> = HashMap::new();

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    let host = info
                        .get_addresses()
                        .iter()
                        .next()
                        .map(|a| a.to_string())
                        .unwrap_or_default();
                    let txt: HashMap<String, String> = info
                        .get_properties()
                        .iter()
                        .map(|p| (p.key().to_string(), p.val_str().to_string()))
                        .collect();
                    by_name.entry(info.get_fullname().to_string()).or_insert(DiscoveredServer {
                        host,
                        port: info.get_port(),
                        txt,
                    });
                },
                Ok(ServiceEvent::SearchStopped(_)) => break,
                Err(_) => break,
                _ => {},
            }
        }

        let _ = self.daemon.shutdown();
        by_name.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_backend::make_test_entry;

    #[test]
    fn encode_devices_txt_empty() {
        assert_eq!(encode_devices_txt(&[]), "");
    }

    #[test]
    fn encode_devices_txt_single() {
        let devices = vec![make_test_entry("1-1", 0x1234, 0x5678)];
        assert_eq!(encode_devices_txt(&devices), "vid=0x1234,pid=0x5678,bus=1-1,n=1234:5678");
    }

    #[test]
    fn encode_devices_txt_multi_preserves_order() {
        let devices =
            vec![make_test_entry("1-1", 0x046d, 0xc261), make_test_entry("1-2", 0x8087, 0x0024)];
        assert_eq!(
            encode_devices_txt(&devices),
            "vid=0x046d,pid=0xc261,bus=1-1,n=046d:c261,vid=0x8087,pid=0x0024,bus=1-2,n=8087:0024"
        );
    }
}
