//! Rolling percentiles for playout error and callback latency.
//!
//! Used to feed `playout_error_us_p50/p95/p99` and `callback_xrun_us_p99` in
//! `HealthReport`. Backed by a ring buffer of absolute-value microseconds.
//! On each call to [`PlayoutErrorTracker::percentiles`] a sorted copy is made;
//! this is O(N log N) per call but the typical N is ~1024 and the call rate is
//! ~1 Hz (only when the client builds a HealthReport), so the cost is negligible.

use std::collections::VecDeque;

const DEFAULT_CAPACITY: usize = 1024;

/// Records absolute deltas in microseconds and reports p50/p95/p99.
#[derive(Debug)]
pub struct PlayoutErrorTracker {
    samples: VecDeque<u32>,
    capacity: usize,
}

impl PlayoutErrorTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
        }
    }

    pub fn record(&mut self, signed_error_us: i64) {
        let abs_us = signed_error_us.unsigned_abs().min(u32::MAX as u64) as u32;
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(abs_us);
    }

    pub fn percentiles(&self) -> (u32, u32, u32) {
        if self.samples.is_empty() {
            return (0, 0, 0);
        }
        let mut sorted: Vec<u32> = self.samples.iter().copied().collect();
        sorted.sort_unstable();
        let last = sorted.len() - 1;
        let p50 = sorted[last * 50 / 100];
        let p95 = sorted[last * 95 / 100];
        let p99 = sorted[last * 99 / 100];
        (p50, p95, p99)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl Default for PlayoutErrorTracker {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tracker_returns_zero_percentiles() {
        let t = PlayoutErrorTracker::default();
        assert_eq!(t.percentiles(), (0, 0, 0));
    }

    #[test]
    fn records_absolute_values() {
        let mut t = PlayoutErrorTracker::default();
        t.record(-500);
        t.record(500);
        assert_eq!(t.len(), 2);
        let (p50, _, _) = t.percentiles();
        assert_eq!(p50, 500);
    }

    #[test]
    fn percentiles_match_sorted_position() {
        let mut t = PlayoutErrorTracker::new(100);
        for i in 1..=100 {
            t.record(i as i64 * 100);
        }
        let (p50, p95, p99) = t.percentiles();
        // last index = 99, p50 -> index 49 -> value 5000; p95 -> index 94 -> value 9500;
        // p99 -> index 98 -> value 9900.
        assert_eq!(p50, 5_000);
        assert_eq!(p95, 9_500);
        assert_eq!(p99, 9_900);
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let mut t = PlayoutErrorTracker::new(4);
        t.record(1);
        t.record(2);
        t.record(3);
        t.record(4);
        t.record(5);
        // 1 should be evicted; remaining values are 2,3,4,5.
        assert_eq!(t.len(), 4);
        // With 4 samples and floor-percentile math, p50 -> index 1 (value 3),
        // p95 and p99 -> index 2 (value 4). The point is just to verify
        // the oldest sample (1) was dropped.
        let (p50, _, _) = t.percentiles();
        assert_eq!(p50, 3);
    }

    #[test]
    fn clear_resets_tracker() {
        let mut t = PlayoutErrorTracker::default();
        t.record(100);
        t.record(200);
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.percentiles(), (0, 0, 0));
    }
}
