//! `TimeSource` — abstraction over clock backends.
//!
//! The default implementation is [`NtpTimeSource`], which wraps
//! [`TimeProvider`] and uses the NTP-like software sync algorithm.
//!
//! A future [`PtpTimeSource`] will drive the same interface using
//! IEEE 1588v2 hardware timestamps from `/dev/ptp0`, giving nanosecond-level
//! accuracy without changing any other part of the pipeline.
//!
//! ## Using a custom `TimeSource`
//!
//! ```rust,ignore
//! use sonium_sync::time_source::{TimeSource, NtpTimeSource};
//!
//! let source: Box<dyn TimeSource> = Box::new(NtpTimeSource::new());
//! let server_now = source.now_server_us();
//! ```

use crate::TimeProvider;

/// A pluggable clock source that translates local time to server time.
///
/// Implementations must be `Send + Sync` so they can be shared across the
/// Tokio runtime and the audio playback thread.
pub trait TimeSource: Send + Sync {
    /// Current server time in microseconds since the UNIX epoch.
    ///
    /// Returns the best estimate of what the server's clock reads *right now*,
    /// derived from however this source tracks the offset.
    fn now_server_us(&self) -> i64;

    /// Whether this source currently has a reliable lock on the clock.
    ///
    /// Returns `false` before the first sync exchange completes, and after
    /// [`STALE_TIMEOUT`][crate::time_provider::TimeProvider::is_stale] seconds
    /// without a successful update.
    fn is_locked(&self) -> bool;

    /// A short human-readable name for logging and the web UI.
    fn name(&self) -> &'static str;
}

// ── NtpTimeSource ──────────────────────────────────────────────────────────

/// Software NTP-like clock source backed by [`TimeProvider`].
///
/// This is the default.  It achieves ~1 ms accuracy on a quiet LAN.
pub struct NtpTimeSource {
    provider: TimeProvider,
}

impl NtpTimeSource {
    /// Create a new source with a zeroed offset (not yet synced).
    pub fn new() -> Self {
        Self {
            provider: TimeProvider::new(),
        }
    }

    /// Access the underlying provider to feed RTT measurements into it.
    pub fn provider(&self) -> &TimeProvider {
        &self.provider
    }
}

impl Default for NtpTimeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeSource for NtpTimeSource {
    fn now_server_us(&self) -> i64 {
        use crate::time_provider::now_us;
        self.provider.to_server_time(now_us())
    }

    fn is_locked(&self) -> bool {
        !self.provider.is_stale()
    }

    fn name(&self) -> &'static str {
        "ntp-like"
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntp_source_unlocked_before_sync() {
        let src = NtpTimeSource::new();
        assert!(!src.is_locked());
    }

    #[test]
    fn ntp_source_locked_after_update() {
        let src = NtpTimeSource::new();
        src.provider().update(0, 10_000, 5_000);
        assert!(src.is_locked());
    }

    #[test]
    fn ntp_source_now_is_positive() {
        let src = NtpTimeSource::new();
        assert!(src.now_server_us() > 0);
    }

    #[test]
    fn ntp_source_name() {
        assert_eq!(NtpTimeSource::new().name(), "ntp-like");
    }

    #[test]
    fn trait_object_works() {
        let src: Box<dyn TimeSource> = Box::new(NtpTimeSource::new());
        assert!(src.now_server_us() > 0);
    }
}
