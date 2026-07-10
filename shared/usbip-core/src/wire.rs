//! Wire framing for USB/IP messages in the URB exchange phase.
//!
//! PROTOCOL.md / ADR-0001 fix "plain" framing to raw kernel USB/IP bytes:
//! an 8-byte `UsbIpHeader` followed by the command-specific payload, with
//! **no length prefix**. Message boundaries are self-describing — the
//! header's `command` field determines how many more bytes to read (and,
//! for `CMD_SUBMIT`/`RET_SUBMIT`, the payload's own length fields decide
//! whether a variable-length data section follows).
//!
//! This module is the single implementation of that framing, shared by the
//! server's URB loop and the client's URB loop, so the two ends can never
//! diverge again. It is transport-agnostic (`AsyncRead`/`AsyncWrite`) so it
//! works over a real `TcpStream` or an in-memory duplex in tests.
//!
//! Encrypted transport is a distinct framing (length-prefixed ciphertext,
//! see `crypto_stream::CryptoStream` in `usbip-server`) and is out of scope
//! here — callers needing an encrypted variant wrap this codec's messages
//! rather than replacing them.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zerocopy::FromBytes;

use crate::error::{ErrorKind, UsbIpError, UsbIpResult};
use crate::protocol::{UsbIpHeader, USBIP_CMD_SUBMIT, USBIP_RET_SUBMIT, USBIP_RET_UNLINK};
use crate::urb::{UsbIpCmdSubmit, UsbIpRetSubmit, UsbIpRetUnlink};

/// Read one full USB/IP message (header + command-specific payload) from
/// `stream`, byte-identical to what the Linux kernel client/server would
/// read — no length prefix, ever.
pub async fn read_message<S: AsyncRead + Unpin>(stream: &mut S) -> UsbIpResult<Vec<u8>> {
    let mut buf = vec![0u8; UsbIpHeader::SIZE];
    stream.read_exact(&mut buf).await?;
    let (header, _) = UsbIpHeader::read_from_prefix(&buf)
        .map_err(|_| UsbIpError::from(ErrorKind::InvalidMessage("truncated header".into())))?;

    match header.command.get() {
        USBIP_CMD_SUBMIT => {
            let mut rest = vec![0u8; UsbIpCmdSubmit::HEADER_SIZE];
            stream.read_exact(&mut rest).await?;
            let (cmd, _) = UsbIpCmdSubmit::read_from_prefix(&rest).map_err(|_| {
                UsbIpError::from(ErrorKind::InvalidMessage("truncated CMD_SUBMIT".into()))
            })?;
            buf.extend_from_slice(&rest);
            if !cmd.is_in() && cmd.data_len() > 0 {
                let mut data = vec![0u8; cmd.data_len() as usize];
                stream.read_exact(&mut data).await?;
                buf.extend_from_slice(&data);
            }
        },
        USBIP_RET_SUBMIT => {
            let mut rest = vec![0u8; UsbIpRetSubmit::HEADER_SIZE];
            stream.read_exact(&mut rest).await?;
            let (ret, _) = UsbIpRetSubmit::read_from_prefix(&rest).map_err(|_| {
                UsbIpError::from(ErrorKind::InvalidMessage("truncated RET_SUBMIT".into()))
            })?;
            buf.extend_from_slice(&rest);
            if ret.has_data() {
                let mut data = vec![0u8; ret.actual_len() as usize];
                stream.read_exact(&mut data).await?;
                buf.extend_from_slice(&data);
            }
        },
        USBIP_RET_UNLINK => {
            let mut rest = vec![0u8; UsbIpRetUnlink::SIZE];
            stream.read_exact(&mut rest).await?;
            buf.extend_from_slice(&rest);
        },
        _ => {
            // Unknown command: return the header alone. Callers decide
            // whether to log and continue or treat it as fatal.
        },
    }

    Ok(buf)
}

/// Write one or more concatenated raw USB/IP messages to `stream`.
///
/// `msg` may hold a single message or several back-to-back (e.g. a batch of
/// `RET_SUBMIT` replies) — plain framing has no boundary marker between
/// them, so concatenation on the wire is indistinguishable from separate
/// writes. Batching is purely an internal write-buffering optimization; it
/// never changes the bytes a reader sees.
pub async fn write_message<S: AsyncWrite + Unpin>(stream: &mut S, msg: &[u8]) -> UsbIpResult<()> {
    stream.write_all(msg).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::U32BE;
    use crate::reply::serialize_reply;
    use crate::urb::UsbIpCmdSubmit;
    use tokio::io::duplex;

    const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    /// Build a raw CMD_SUBMIT message the way a real peer puts it on the
    /// wire: header + `UsbIpCmdSubmit::HEADER_SIZE` bytes (48) + data.
    ///
    /// NOTE: `size_of::<UsbIpCmdSubmit>()` is only 44 (9 x u32 + 8-byte
    /// setup) — `HEADER_SIZE` carries 4 bytes of wire padding the struct
    /// doesn't model. Callers building test fixtures by hand must pad to
    /// `HEADER_SIZE`, or a reader expecting the padded frame blocks forever
    /// waiting for bytes that were never sent.
    fn make_cmd_submit(seqnum: u32, data: &[u8]) -> Vec<u8> {
        let header = UsbIpHeader::new(USBIP_CMD_SUBMIT);
        let cmd = UsbIpCmdSubmit {
            seqnum: U32BE::new(seqnum),
            devid: U32BE::new(1),
            direction: U32BE::new(0), // OUT
            ep: U32BE::new(1),
            transfer_flags: U32BE::new(0),
            transfer_buffer_length: U32BE::new(data.len() as u32),
            start_frame: U32BE::new(0),
            number_of_packets: U32BE::new(0),
            interval: U32BE::new(0),
            setup: [0u8; 8],
        };
        use zerocopy::IntoBytes;
        let cmd_bytes = cmd.as_bytes();
        assert_eq!(
            cmd_bytes.len() + 4,
            UsbIpCmdSubmit::HEADER_SIZE,
            "pad amount changed — update this fixture"
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(cmd_bytes);
        buf.extend_from_slice(&[0u8; 4]); // wire padding to HEADER_SIZE
        buf.extend_from_slice(data);
        buf
    }

    #[tokio::test]
    async fn plain_framing_has_no_length_prefix() {
        // A raw CMD_SUBMIT with an OUT data payload, written with no
        // wrapper, must round-trip through read_message byte-for-byte.
        let msg = make_cmd_submit(42, b"hello");
        let (mut a, mut b) = duplex(4096);
        write_message(&mut a, &msg).await.unwrap();
        let got = tokio::time::timeout(TEST_TIMEOUT, read_message(&mut b)).await.unwrap().unwrap();
        assert_eq!(got, msg);
    }

    #[tokio::test]
    async fn server_writer_client_reader_interop() {
        // Simulates the server's URB loop (batched RET_SUBMIT writer) feeding
        // the client's URB loop (single-message reader) over one stream —
        // the exact cross-loop path that was broken before this codec.
        // `serialize_reply` copies `cmd.direction` verbatim into the
        // reply's direction field, and `UsbIpRetSubmit::has_data()` only
        // reports data present when that field carries `URB_DIR_IN` — so
        // the fixture command must be an IN transfer for the replies below
        // to carry their "abc"/"xy" payloads on the wire.
        let cmd = {
            use crate::protocol::URB_DIR_IN;
            use zerocopy::FromBytes;
            let mut raw = make_cmd_submit(7, b"");
            raw[UsbIpHeader::SIZE + 8..UsbIpHeader::SIZE + 12]
                .copy_from_slice(&URB_DIR_IN.to_be_bytes());
            let (c, _) = UsbIpCmdSubmit::read_from_prefix(
                &raw[UsbIpHeader::SIZE..UsbIpHeader::SIZE + UsbIpCmdSubmit::HEADER_SIZE],
            )
            .unwrap();
            c
        };
        let reply1 = serialize_reply(&cmd, 0, 3, b"abc");
        let reply2 = serialize_reply(&cmd, 0, 2, b"xy");
        let mut batch = Vec::new();
        batch.extend_from_slice(&reply1);
        batch.extend_from_slice(&reply2);

        let (mut server_side, mut client_side) = duplex(8192);
        write_message(&mut server_side, &batch).await.unwrap();

        let got1 = tokio::time::timeout(TEST_TIMEOUT, read_message(&mut client_side))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got1, reply1);
        let got2 = tokio::time::timeout(TEST_TIMEOUT, read_message(&mut client_side))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got2, reply2);
    }
}
