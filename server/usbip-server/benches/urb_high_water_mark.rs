//! Benchmarks for URB latency high-water-mark CAS reduction.
//!
//! Compares naive (every-sample CAS + broadcast) vs. HWM
//! (only-on-new-peak) approaches. Reports CAS count reduction.
//!
//! Run with: cargo bench -p usbip-server -- urb_high_water_mark

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::sync::broadcast;

use usbip_server::api::LatencySample;

const URBS_PER_THREAD: usize = 10_000;
const NUM_THREADS: usize = 4;

/// Naive approach: every sample performs a load + conditional CAS on
/// the shared atomic and broadcasts on CAS success. The counter
/// tracks how many times the shared atomic was touched.
fn naive_bench(latencies: &[u64]) -> u64 {
    let global = Arc::new(AtomicU64::new(0));
    let (tx, _rx) = broadcast::channel::<LatencySample>(1024);
    let atomic_ops = Arc::new(AtomicU64::new(0));

    std::thread::scope(|s| {
        for thread_id in 0..NUM_THREADS {
            let g = Arc::clone(&global);
            let t = tx.clone();
            let ops = Arc::clone(&atomic_ops);
            s.spawn(move || {
                let base = thread_id as u64 * 1000;
                for (i, &lat) in latencies.iter().enumerate() {
                    let sample_us = base + lat;
                    // Naive: every URB touches the shared atomic.
                    let current = g.load(Ordering::Relaxed);
                    ops.fetch_add(1, Ordering::Relaxed);
                    if sample_us > current {
                        if g.compare_exchange_weak(
                            current,
                            sample_us,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        )
                        .is_ok()
                        {
                            let _ = t.send(LatencySample {
                                latency_us: sample_us,
                                device: format!("bench-{thread_id}"),
                                seqnum: (i * NUM_THREADS + thread_id) as u64,
                            });
                        }
                    }
                }
            });
        }
    });

    Arc::try_unwrap(atomic_ops).unwrap().into_inner()
}

/// HWM approach: thread-local peak filters out most URBs. Only
/// attempts CAS when local peak exceeds the global peak.
fn hwm_bench(latencies: &[u64]) -> u64 {
    let global = Arc::new(AtomicU64::new(0));
    let (tx, _rx) = broadcast::channel::<LatencySample>(1024);
    let atomic_ops = Arc::new(AtomicU64::new(0));

    std::thread::scope(|s| {
        for thread_id in 0..NUM_THREADS {
            let g = Arc::clone(&global);
            let t = tx.clone();
            let ops = Arc::clone(&atomic_ops);
            s.spawn(move || {
                let base = thread_id as u64 * 1000;
                let mut local_peak: u64 = 0;
                for (i, &lat) in latencies.iter().enumerate() {
                    let sample_us = base + lat;
                    // HWM: thread-local check -- no atomic access.
                    if sample_us <= local_peak {
                        continue;
                    }
                    local_peak = sample_us;

                    // Only reach the shared atomic when local peak
                    // is a new contender.
                    let current_global = g.load(Ordering::Relaxed);
                    ops.fetch_add(1, Ordering::Relaxed);
                    if sample_us <= current_global {
                        continue;
                    }

                    if g.compare_exchange_weak(
                        current_global,
                        sample_us,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                    {
                        let _ = t.send(LatencySample {
                            latency_us: sample_us,
                            device: format!("bench-{thread_id}"),
                            seqnum: (i * NUM_THREADS + thread_id) as u64,
                        });
                    } else {
                        local_peak = current_global;
                    }
                }
            });
        }
    });

    Arc::try_unwrap(atomic_ops).unwrap().into_inner()
}

/// Generate synthetic URB latencies that oscillate around a baseline,
/// simulating steady-state USB traffic with occasional spikes.
fn synthetic_latencies() -> Vec<u64> {
    let mut lats = Vec::with_capacity(URBS_PER_THREAD);
    for i in 0..URBS_PER_THREAD {
        let jitter = ((i * 7 + 13) % 30) as u64;
        let spike = if i % 500 == 0 { 500 } else { 0 };
        lats.push(100 + jitter + spike);
    }
    lats
}

fn bench_hwm_cas_reduction(c: &mut Criterion) {
    let latencies = synthetic_latencies();
    let total_urbs = (URBS_PER_THREAD * NUM_THREADS) as u64;

    let mut group = c.benchmark_group("latency_hwm");
    group
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1))
        .sample_size(10);

    group.bench_function("naive_cas_attempts", |b| {
        b.iter(|| {
            let count = naive_bench(black_box(&latencies));
            black_box(count);
        })
    });

    group.bench_function("hwm_cas_attempts", |b| {
        b.iter(|| {
            let count = hwm_bench(black_box(&latencies));
            black_box(count);
        })
    });

    group.finish();

    // Post-bench assertion: HWM touches the shared atomic far less.
    let naive = naive_bench(&latencies);
    let hwm = hwm_bench(&latencies);
    let reduction = 100.0 * (1.0 - (hwm as f64 / naive as f64));
    eprintln!("\nCAS reduction: naive={naive} hwm={hwm} ({reduction:.1}% reduction)");
    assert!(hwm < naive, "HWM ({hwm}) should have fewer CAS ops than naive ({naive})");
    assert!(
        hwm <= total_urbs / 2,
        "HWM ({hwm}) should be well below half of total URBs ({total_urbs})"
    );
}

criterion_group! {
    name = hwm_benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1))
        .sample_size(10);
    targets = bench_hwm_cas_reduction,
}

criterion_main!(hwm_benches);
