//! Benchmarks for URB forwarding — comparing copy-based vs zero-copy paths.
//!
//! Run with: cargo bench -p usbip-server

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use usbip_core::protocol::{UsbIpHeader, U32BE, URB_DIR_IN};
use usbip_core::reply::{serialize_reply, serialize_reply_into};
use usbip_core::urb::{UsbIpCmdSubmit, UsbIpRetSubmit};

/// Shared serializer (allocating variant).
fn build_via_shared(cmd: &UsbIpCmdSubmit, data: &[u8], status: i32) -> Vec<u8> {
    serialize_reply(cmd, status, data.len() as u32, data)
}

/// Shared serializer (append-to-buffer variant) — benchmarks the batcher path.
fn build_via_shared_into(buf: &mut Vec<u8>, cmd: &UsbIpCmdSubmit, data: &[u8], status: i32) {
    buf.clear();
    serialize_reply_into(buf, cmd, status, data.len() as u32, data);
}

fn make_cmd(seqnum: u32, ep: u32, len: u32) -> UsbIpCmdSubmit {
    UsbIpCmdSubmit {
        seqnum: U32BE::new(seqnum),
        devid: U32BE::new(1),
        direction: U32BE::new(1),
        ep: U32BE::new(ep),
        transfer_flags: U32BE::new(URB_DIR_IN),
        transfer_buffer_length: U32BE::new(len),
        start_frame: U32BE::new(0),
        number_of_packets: U32BE::new(0),
        interval: U32BE::new(0),
        setup: [0u8; 8],
    }
}

fn bench_forward_copy_small(c: &mut Criterion) {
    let cmd = make_cmd(1, 0x81, 64);
    let data = vec![0xABu8; 64];

    c.bench_function("forward/copy_small_64b", |b| {
        b.iter(|| {
            let reply = build_via_shared(&cmd, &data, 0);
            black_box(reply.len());
        })
    });
}

fn bench_forward_copy_large(c: &mut Criterion) {
    let cmd = make_cmd(1, 0x02, 16384);
    let data = vec![0xABu8; 16384];

    c.bench_function("forward/copy_large_16k", |b| {
        b.iter(|| {
            let reply = build_via_shared(&cmd, &data, 0);
            black_box(reply.len());
        })
    });
}

fn bench_forward_append_small(c: &mut Criterion) {
    let cmd = make_cmd(1, 0x81, 64);
    let data = vec![0xABu8; 64];
    let total_size = UsbIpHeader::SIZE + UsbIpRetSubmit::HEADER_SIZE + data.len();
    let mut buf = Vec::with_capacity(total_size + 1024);

    c.bench_function("forward/append_small_64b", |b| {
        b.iter(|| {
            build_via_shared_into(&mut buf, &cmd, &data, 0);
            black_box(buf.len());
        })
    });
}

fn bench_forward_append_large(c: &mut Criterion) {
    let cmd = make_cmd(1, 0x02, 16384);
    let data = vec![0xABu8; 16384];
    let total_size = UsbIpHeader::SIZE + UsbIpRetSubmit::HEADER_SIZE + data.len();
    let mut buf = Vec::with_capacity(total_size + 1024);

    c.bench_function("forward/append_large_16k", |b| {
        b.iter(|| {
            build_via_shared_into(&mut buf, &cmd, &data, 0);
            black_box(buf.len());
        })
    });
}

fn bench_forward_append_reuse(c: &mut Criterion) {
    let data = vec![0xABu8; 64];
    let total_size = UsbIpHeader::SIZE + UsbIpRetSubmit::HEADER_SIZE + data.len();
    let mut buf = Vec::with_capacity(total_size + 1024);

    c.bench_function("forward/append_reuse_buffer", |b| {
        b.iter(|| {
            for i in 0..100 {
                let cmd = make_cmd(i, 0x81, 64);
                build_via_shared_into(&mut buf, &cmd, &data, 0);
                black_box(buf.len());
            }
        })
    });
}

fn bench_forward_copy_reuse(c: &mut Criterion) {
    let data = vec![0xABu8; 64];

    c.bench_function("forward/copy_reuse_alloc", |b| {
        b.iter(|| {
            for i in 0..100 {
                let cmd = make_cmd(i, 0x81, 64);
                let reply = build_via_shared(&cmd, &data, 0);
                black_box(reply.len());
            }
        })
    });
}

criterion_group! {
    name = forward_benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1))
        .sample_size(50);
    targets =
        bench_forward_copy_small,
        bench_forward_copy_large,
        bench_forward_append_small,
        bench_forward_append_large,
        bench_forward_append_reuse,
        bench_forward_copy_reuse,
}

criterion_main!(forward_benches);
