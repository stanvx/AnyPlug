//! Encode and decode the `devices` TXT value used in mDNS advertisements.
//!
//! The wire format is a comma-separated list of `vid=0xVVVV,pid=0xPPPP,bus=B-B,n=NAME`
//! tuples, where `vid` and `pid` are lowercase hex, `bus` is a raw USB bus-id string
//! (e.g. `1-1`), and `name` is a human-readable device name (today the
//! `vid:pid` hex pair).
//!
//! This module is the **single seam** for TXT serialization — both the server
//! (advertise) and the client (browse) use it.

use crate::error::{ErrorKind, UsbIpResult};
use crate::protocol::UsbIpDeviceEntry;

/// A device entry decoded from the mDNS `devices` TXT key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub vid: u16,
    pub pid: u16,
    pub bus: String,
    pub name: String,
}

/// Encode a slice of device entries into the `devices` TXT value.
///
/// Format: comma-separated `vid=0xVVVV,pid=0xPPPP,bus=B-B,n=NAME` tuples.
/// Returns an empty string for an empty input.
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

/// Parse a `devices` TXT value back into a list of [`DiscoveredDevice`].
///
/// The wire format uses commas as both field separators (within a device
/// tuple) and tuple separators (between devices).  Each device consists
/// of exactly 4 comma-separated fields in order: `vid=0x...`,
/// `pid=0x...`, `bus=...`, `n=...`.  The parser groups every 4
/// consecutive comma-separated tokens into one device.
///
/// Returns an error if the token count is not a multiple of 4 or any
/// required field is missing or malformed.
pub fn decode_devices_txt(txt: &str) -> UsbIpResult<Vec<DiscoveredDevice>> {
    if txt.is_empty() {
        return Ok(Vec::new());
    }

    let tokens: Vec<&str> = txt.split(',').collect();
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    if !tokens.len().is_multiple_of(4) {
        return Err(ErrorKind::InvalidMessage(format!(
            "device TXT token count {} is not a multiple of 4",
            tokens.len()
        ))
        .into());
    }

    let mut devices = Vec::with_capacity(tokens.len() / 4);

    for chunk in tokens.chunks(4) {
        let vid = parse_vid_pid_field(chunk[0], "vid")?;
        let pid = parse_vid_pid_field(chunk[1], "pid")?;
        let bus = parse_kv_field(chunk[2], "bus")?;
        let name = parse_kv_field(chunk[3], "n")?;
        devices.push(DiscoveredDevice { vid, pid, bus, name });
    }

    Ok(devices)
}

/// Parse a `vid=0x...` or `pid=0x...` field, returning the u16 value.
fn parse_vid_pid_field(field: &str, expected_key: &str) -> UsbIpResult<u16> {
    let prefix = format!("{expected_key}=0x");
    let val = field.strip_prefix(&prefix).ok_or_else(|| {
        ErrorKind::InvalidMessage(format!("expected {expected_key}=0x..., got: {field}"))
    })?;
    u16::from_str_radix(val, 16).map_err(|_| {
        { ErrorKind::InvalidMessage(format!("invalid hex in {expected_key}: {field}")) }.into()
    })
}

/// Parse a `key=value` field, returning the value string.
fn parse_kv_field(field: &str, expected_key: &str) -> UsbIpResult<String> {
    let prefix = format!("{expected_key}=");
    let val = field.strip_prefix(&prefix).ok_or_else(|| {
        ErrorKind::InvalidMessage(format!("expected {expected_key}=..., got: {field}"))
    })?;
    Ok(val.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::U16BE;
    use crate::protocol::U32BE;

    /// Build a minimal `UsbIpDeviceEntry` for testing.
    fn make_test_entry(busid: &str, vid: u16, pid: u16) -> UsbIpDeviceEntry {
        let mut entry = UsbIpDeviceEntry {
            path: [0u8; 256],
            busid: [0u8; 32],
            busnum: U32BE::new(1),
            devnum: U32BE::new(1),
            speed: U32BE::new(3),
            id_vendor: U16BE::new(vid),
            id_product: U16BE::new(pid),
            bcd_device: U16BE::new(0x0100),
            b_device_class: 0,
            b_device_sub_class: 0,
            b_device_protocol: 0,
            b_configuration_value: 1,
            b_num_configurations: 1,
            b_num_interfaces: 1,
        };
        let busid_bytes = busid.as_bytes();
        let copy_len = busid_bytes.len().min(31);
        entry.busid[..copy_len].copy_from_slice(&busid_bytes[..copy_len]);
        entry
    }

    // -- encode tests (migrated from server discovery.rs) --

    #[test]
    fn encode_empty() {
        assert_eq!(encode_devices_txt(&[]), "");
    }

    #[test]
    fn encode_single() {
        let devices = vec![make_test_entry("1-1", 0x1234, 0x5678)];
        assert_eq!(encode_devices_txt(&devices), "vid=0x1234,pid=0x5678,bus=1-1,n=1234:5678");
    }

    #[test]
    fn encode_multi_preserves_order() {
        let devices =
            vec![make_test_entry("1-1", 0x046d, 0xc261), make_test_entry("1-2", 0x8087, 0x0024)];
        assert_eq!(
            encode_devices_txt(&devices),
            "vid=0x046d,pid=0xc261,bus=1-1,n=046d:c261,vid=0x8087,pid=0x0024,bus=1-2,n=8087:0024"
        );
    }

    // -- decode tests --

    #[test]
    fn decode_empty_string() {
        assert_eq!(decode_devices_txt("").unwrap(), vec![]);
    }

    #[test]
    fn decode_single() {
        let result = decode_devices_txt("vid=0x1234,pid=0x5678,bus=1-1,n=1234:5678").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            DiscoveredDevice {
                vid: 0x1234,
                pid: 0x5678,
                bus: "1-1".into(),
                name: "1234:5678".into()
            }
        );
    }

    #[test]
    fn decode_multi() {
        let txt =
            "vid=0x046d,pid=0xc261,bus=1-1,n=046d:c261,vid=0x8087,pid=0x0024,bus=1-2,n=8087:0024";
        let result = decode_devices_txt(txt).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].vid, 0x046d);
        assert_eq!(result[0].pid, 0xc261);
        assert_eq!(result[0].bus, "1-1");
        assert_eq!(result[1].vid, 0x8087);
        assert_eq!(result[1].pid, 0x0024);
        assert_eq!(result[1].bus, "1-2");
    }

    // -- round-trip tests --

    #[test]
    fn round_trip_single() {
        let devices = vec![make_test_entry("1-1", 0x1234, 0x5678)];
        let encoded = encode_devices_txt(&devices);
        let decoded = decode_devices_txt(&encoded).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].vid, 0x1234);
        assert_eq!(decoded[0].pid, 0x5678);
        assert_eq!(decoded[0].bus, "1-1");
        assert_eq!(decoded[0].name, "1234:5678");
    }

    #[test]
    fn round_trip_multi() {
        let devices = vec![
            make_test_entry("1-1", 0x046d, 0xc261),
            make_test_entry("1-2", 0x8087, 0x0024),
            make_test_entry("3-4", 0x0aaa, 0x0bbb),
        ];
        let encoded = encode_devices_txt(&devices);
        let decoded = decode_devices_txt(&encoded).unwrap();
        assert_eq!(decoded.len(), 3);
        for (i, dev) in devices.iter().enumerate() {
            assert_eq!(decoded[i].vid, dev.vid());
            assert_eq!(decoded[i].pid, dev.pid());
            assert_eq!(decoded[i].bus, dev.busid_str());
        }
    }

    // -- malformed input tests --

    #[test]
    fn decode_malformed_missing_field() {
        let result = decode_devices_txt("vid=0x1234,pid=0x5678");
        assert!(result.is_err());
    }

    #[test]
    fn decode_malformed_bad_hex() {
        let result = decode_devices_txt("vid=0xZZZZ,pid=0x5678,bus=1-1,n=bad");
        assert!(result.is_err());
    }

    #[test]
    fn decode_unknown_field_only() {
        // Segment has no vid/pid/bus/n fields -- should fail
        let result = decode_devices_txt("foo=bar");
        assert!(result.is_err());
    }

    #[test]
    fn default_discovered_device() {
        let d = DiscoveredDevice::default();
        assert_eq!(d.vid, 0);
        assert_eq!(d.pid, 0);
        assert_eq!(d.bus, "");
        assert_eq!(d.name, "");
    }
}
