//! mDNS service discovery for USB/IP.
//!
//! Browses `_usbip._tcp.local` to find servers on the local network.
//! Also provides helpers to decode the `devices` TXT key carried in
//! advertisements, delegating to `usbip_core::discovery_txt`.

use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::net::SocketAddr;
use std::time::Duration;
use tracing::info;

use usbip_core::discovery_txt::decode_devices_txt;
pub use usbip_core::discovery_txt::DiscoveredDevice;
use usbip_core::error::*;

pub struct MdnsBrowser {
    daemon: ServiceDaemon,
}

impl MdnsBrowser {
    pub fn new() -> UsbIpResult<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| ErrorKind::NotSupported(format!("mDNS init failed: {}", e)))?;

        Ok(Self { daemon })
    }

    /// Browse for USB/IP servers. Returns list of SocketAddr after a short scan.
    pub fn browse(&self) -> UsbIpResult<Vec<SocketAddr>> {
        let service_type = "_usbip._tcp.local.";
        let receiver = self
            .daemon
            .browse(service_type)
            .map_err(|e| ErrorKind::NotSupported(format!("mDNS browse failed: {}", e)))?;

        let mut servers = Vec::new();

        // Wait for responses (2 second scan)
        let timeout = Duration::from_secs(2);
        let start = std::time::Instant::now();

        loop {
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }

            match receiver.recv_timeout(remaining) {
                Ok(event) => {
                    if let ServiceEvent::ServiceResolved(info) = event {
                        let addr = info.get_addresses();
                        for a in addr.iter() {
                            let port = info.get_port();
                            let sock = SocketAddr::new(*a, port);
                            if !servers.contains(&sock) {
                                info!("mDNS discovered USB/IP server at {}", sock);
                                servers.push(sock);
                            }
                        }
                    }
                },
                Err(_) => break, // timeout
            }
        }

        Ok(servers)
    }
}

/// Decode the `devices` TXT value from a discovered server into a list of
/// [`DiscoveredDevice`] entries.
///
/// This is a thin wrapper around [`usbip_core::discovery_txt::decode_devices_txt`].
pub fn decode_devices_from_txt(txt: &str) -> UsbIpResult<Vec<DiscoveredDevice>> {
    decode_devices_txt(txt)
}

/// Decode the `devices` TXT value from a server's TXT properties map.
///
/// Looks up the `"devices"` key; returns an empty list if the key is
/// absent or empty.
pub fn decode_devices_from_properties(
    properties: &std::collections::HashMap<String, String>,
) -> UsbIpResult<Vec<DiscoveredDevice>> {
    match properties.get("devices") {
        Some(txt) if !txt.is_empty() => decode_devices_txt(txt),
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_devices_from_txt_single() {
        let result = decode_devices_from_txt("vid=0x1234,pid=0x5678,bus=1-1,n=1234:5678").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].vid, 0x1234);
        assert_eq!(result[0].pid, 0x5678);
        assert_eq!(result[0].bus, "1-1");
        assert_eq!(result[0].name, "1234:5678");
    }

    #[test]
    fn decode_devices_from_txt_empty() {
        let result = decode_devices_from_txt("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn decode_devices_from_txt_malformed() {
        assert!(decode_devices_from_txt("vid=0x1234").is_err());
    }

    #[test]
    fn decode_devices_from_properties_with_devices() {
        let mut props = std::collections::HashMap::new();
        props
            .insert("devices".to_string(), "vid=0x046d,pid=0xc261,bus=1-1,n=046d:c261".to_string());
        let result = decode_devices_from_properties(&props).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].vid, 0x046d);
    }

    #[test]
    fn decode_devices_from_properties_no_devices_key() {
        let props = std::collections::HashMap::new();
        let result = decode_devices_from_properties(&props).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn decode_devices_from_properties_empty_devices() {
        let mut props = std::collections::HashMap::new();
        props.insert("devices".to_string(), String::new());
        let result = decode_devices_from_properties(&props).unwrap();
        assert!(result.is_empty());
    }
}
