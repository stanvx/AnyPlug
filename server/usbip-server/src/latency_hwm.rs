//! Per-thread high-water-mark for URB latency.
//!
//! Tracks a thread-local peak and conditionally CAS-updates a shared
//! global peak (`Arc<AtomicU64>`). Broadcasts a `LatencySample` only
//! when a new global peak is established, eliminating nearly all
//! cross-thread CAS and broadcast operations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::api::LatencySample;

/// Per-thread high-water-mark tracker for URB round-trip latency.
///
/// Each URB loop owns one instance. On every completed URB the loop
/// calls [`observe`](Self::observe) with the elapsed microseconds.
/// The struct keeps a thread-local peak; only when that peak exceeds
/// the shared global peak does it attempt a CAS. On CAS success it
/// broadcasts a single [`LatencySample`] to the WebSocket channel.
pub(crate) struct HighWaterMark {
    local_peak_us: u64,
    global_peak: Arc<AtomicU64>,
    busid: String,
    latency_tx: broadcast::Sender<LatencySample>,
}

impl HighWaterMark {
    pub(crate) fn new(
        busid: String,
        global_peak: Arc<AtomicU64>,
        latency_tx: broadcast::Sender<LatencySample>,
    ) -> Self {
        Self { local_peak_us: 0, global_peak, busid, latency_tx }
    }

    /// Record an observed URB round-trip latency in microseconds.
    ///
    /// If this exceeds the thread-local peak, update it. If the new
    /// local peak exceeds the global peak, attempt a CAS. On CAS
    /// success, broadcast a [`LatencySample`]. On CAS failure
    /// (another thread won), snap the local peak to the new global
    /// value.
    pub(crate) fn observe(&mut self, elapsed_us: u64, seqnum: u32) {
        if elapsed_us <= self.local_peak_us {
            return;
        }
        self.local_peak_us = elapsed_us;

        let current_global = self.global_peak.load(Ordering::Relaxed);
        if elapsed_us <= current_global {
            return;
        }

        match self.global_peak.compare_exchange_weak(
            current_global,
            elapsed_us,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                let sample = LatencySample {
                    latency_us: elapsed_us,
                    device: self.busid.clone(),
                    seqnum: seqnum as u64,
                };
                let _ = self.latency_tx.send(sample);
            },
            Err(actual) => {
                // Another thread won — sync local to global.
                self.local_peak_us = actual;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_peak_tracking() {
        let global = Arc::new(AtomicU64::new(0));
        let (tx, _rx) = broadcast::channel(16);
        let mut hwm = HighWaterMark::new("1-1".into(), Arc::clone(&global), tx);

        hwm.observe(100, 1);
        assert_eq!(hwm.local_peak_us, 100);

        // Below peak -- no update.
        hwm.observe(50, 2);
        assert_eq!(hwm.local_peak_us, 100);

        // Above local peak -- local updates and CAS fires.
        hwm.observe(200, 3);
        assert_eq!(hwm.local_peak_us, 200);
        assert_eq!(global.load(Ordering::Relaxed), 200);
    }

    #[test]
    fn cas_broadcasts_once_on_new_peak() {
        let global = Arc::new(AtomicU64::new(0));
        let (tx, mut rx) = broadcast::channel(16);
        let mut hwm = HighWaterMark::new("2-2".into(), Arc::clone(&global), tx);

        // First observation sets global peak and broadcasts.
        hwm.observe(500, 10);
        assert_eq!(rx.try_recv().unwrap().latency_us, 500);

        // Same value -- local peak matches, no broadcast.
        hwm.observe(500, 11);
        assert!(rx.try_recv().is_err());

        // Lower -- no broadcast.
        hwm.observe(300, 12);
        assert!(rx.try_recv().is_err());

        // Higher -- broadcasts once.
        hwm.observe(600, 13);
        let sample = rx.try_recv().unwrap();
        assert_eq!(sample.latency_us, 600);
        assert_eq!(sample.device, "2-2");
        assert_eq!(sample.seqnum, 13);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn below_global_peak_no_cas() {
        let global = Arc::new(AtomicU64::new(1000));
        let (tx, mut rx) = broadcast::channel(16);
        let mut hwm = HighWaterMark::new("3-3".into(), Arc::clone(&global), tx);

        // Below global -- no broadcast.
        hwm.observe(500, 1);
        assert!(rx.try_recv().is_err());
        assert_eq!(global.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn concurrent_threads_only_new_peak_broadcasts() {
        let global = Arc::new(AtomicU64::new(0));
        let (tx, mut rx) = broadcast::channel(1024);
        let mut handles = vec![];

        for thread_id in 0..4u32 {
            let g = Arc::clone(&global);
            let t = tx.clone();
            handles.push(std::thread::spawn(move || {
                let mut hwm = HighWaterMark::new(format!("t{thread_id}"), g, t);
                for i in 0..1000u32 {
                    let latency = 100 + thread_id * 10;
                    hwm.observe(latency as u64, i);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        // With 4 threads at distinct latencies (100, 110, 120, 130),
        // at most 4 broadcasts fire (one per new peak value).
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert!(count <= 4, "expected at most 4 broadcasts, got {count}");
        assert!(count >= 1, "expected at least 1 broadcast");
    }
}
