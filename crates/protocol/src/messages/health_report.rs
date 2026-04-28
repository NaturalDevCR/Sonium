use serde::{Deserialize, Serialize};
use sonium_common::error::Result;
use crate::wire::{WireRead, WireWrite};

/// Real-time health metrics from a client playback session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthReport {
    /// Number of times the playback buffer ran dry (underrun).
    pub underrun_count: u32,
    /// Number of samples dropped due to buffer overflow (overrun).
    pub overrun_count: u32,
    /// Number of chunks dropped because they arrived after their playout time.
    pub stale_drop_count: u32,
    /// Current depth of the jitter buffer in milliseconds.
    pub buffer_depth_ms: u32,
    /// Estimated network jitter in milliseconds.
    pub jitter_ms: u32,
    /// Measured end-to-end latency in milliseconds (offset from server clock).
    pub latency_ms: i32,
}

impl HealthReport {
    pub fn new(
        underrun_count: u32,
        overrun_count: u32,
        stale_drop_count: u32,
        buffer_depth_ms: u32,
        jitter_ms: u32,
        latency_ms: i32,
    ) -> Self {
        Self {
            underrun_count,
            overrun_count,
            stale_drop_count,
            buffer_depth_ms,
            jitter_ms,
            latency_ms,
        }
    }

    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        Ok(Self {
            underrun_count: r.read_u32()?,
            overrun_count: r.read_u32()?,
            stale_drop_count: r.read_u32()?,
            buffer_depth_ms: r.read_u32()?,
            jitter_ms: r.read_u32()?,
            latency_ms: r.read_i32()?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = WireWrite::with_capacity(24);
        w.write_u32(self.underrun_count);
        w.write_u32(self.overrun_count);
        w.write_u32(self.stale_drop_count);
        w.write_u32(self.buffer_depth_ms);
        w.write_u32(self.jitter_ms);
        w.write_i32(self.latency_ms);
        w.finish()
    }
}
