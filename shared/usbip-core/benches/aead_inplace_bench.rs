//! Micro-benchmark: in-place AEAD decrypt vs allocating decrypt.
//!
//! Compares the latency of `decrypt_in_place` (zero-copy, mutates the
//! read buffer) against `decrypt` (allocates a fresh `Vec<u8>`) on a
//! representative 512-byte USB/IP packet.
//!
//! Run with: cargo bench -p usbip-core --bench aead_inplace_bench

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use usbip_core::crypto;

/// Derive a deterministic AES-256-GCM session key from a fixed shared secret.
fn fixed_key() -> ring::aead::LessSafeKey {
    let shared = [0x42u8; 32];
    crypto::derive_session_key(&shared).expect("key derivation must succeed")
}

/// Encrypt a 512-byte plaintext once; returns the wire-format ciphertext.
fn encrypt_512(key: &ring::aead::LessSafeKey) -> Vec<u8> {
    let plaintext = vec![0xABu8; 512];
    crypto::encrypt_message(key, &plaintext).expect("encryption must succeed")
}

fn bench_decrypt_allocating(c: &mut Criterion) {
    let key = fixed_key();
    let wire = encrypt_512(&key);

    c.bench_function("aead/decrypt_alloc_512", |b| {
        b.iter(|| {
            let pt = crypto::decrypt(black_box(&key), black_box(&wire)).unwrap();
            black_box(pt.len());
        })
    });
}

fn bench_decrypt_in_place(c: &mut Criterion) {
    let key = fixed_key();
    let wire = encrypt_512(&key);

    c.bench_function("aead/decrypt_in_place_512", |b| {
        b.iter(|| {
            let mut buf = wire.clone();
            let pt = crypto::decrypt_in_place(black_box(&key), &mut buf).unwrap();
            black_box(pt.len());
        })
    });
}

fn bench_decrypt_byte_equality(c: &mut Criterion) {
    let key = fixed_key();
    let wire = encrypt_512(&key);

    c.bench_function("aead/decrypt_byte_equality_check", |b| {
        b.iter(|| {
            // Allocating path
            let wire_copy = wire.clone();
            let pt_alloc = crypto::decrypt(&key, &wire_copy).unwrap();

            // In-place path
            let mut buf = wire.clone();
            let pt_inplace = crypto::decrypt_in_place(&key, &mut buf).unwrap();

            assert_eq!(pt_alloc, pt_inplace);
            black_box(pt_alloc.len());
        })
    });
}

criterion_group! {
    name = aead_benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1))
        .sample_size(50);
    targets =
        bench_decrypt_allocating,
        bench_decrypt_in_place,
        bench_decrypt_byte_equality,
}

criterion_main!(aead_benches);
