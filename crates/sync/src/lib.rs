//! # sonium-sync
//!
//! Clock synchronization and jitter buffering for multiroom audio.
//!
//! ## Key types
//!
//! - [`TimeProvider`] — NTP-like offset estimator (200-sample median filter).
//! - [`time_source::TimeSource`] — trait for pluggable clock backends (NTP today, PTPv2 tomorrow).
//! - [`SyncBuffer`] — jitter buffer that releases PCM chunks at their scheduled playout time.
//! - [`PcmChunk`] — a decoded audio chunk with its playout timestamp.

pub mod buffer;
pub mod time_provider;
pub mod time_source;

pub use buffer::{PcmChunk, SyncBuffer};
pub use time_provider::TimeProvider;
pub use time_source::{NtpTimeSource, TimeSource};
