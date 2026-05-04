use serde::{Deserialize, Serialize};

/// Group-wide playout timeline synchronisation.
///
/// The server broadcasts this message periodically (every 500 ms) to all
/// clients in the same group.  It contains:
///
/// - `server_now_us` — the server's current timestamp in microseconds.
/// - `group_offset_us` — the agreed group offset; every client should aim
///   to have its local `TimeProvider` converge to this value.
/// - `rate_ppm` — parts-per-million playback-rate correction.  Positive
///   means "play slightly faster", negative means "slower".  Used to
///   compensate for DAC crystal drift between devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupSync {
    /// Server wall-clock time at the moment this message was built (µs).
    pub server_now_us: i64,
    /// Target group offset (server − local) in microseconds.
    /// Clients should nudge their `TimeProvider` toward this value.
    pub group_offset_us: i64,
    /// Playback-rate correction in parts-per-million.
    /// ±500 ppm is the safe audible limit for music.
    pub rate_ppm: i32,
}

impl GroupSync {
    pub fn new(server_now_us: i64, group_offset_us: i64, rate_ppm: i32) -> Self {
        Self {
            server_now_us,
            group_offset_us,
            rate_ppm,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20);
        buf.extend_from_slice(&self.server_now_us.to_le_bytes());
        buf.extend_from_slice(&self.group_offset_us.to_le_bytes());
        buf.extend_from_slice(&self.rate_ppm.to_le_bytes());
        buf
    }

    pub fn decode(payload: &[u8]) -> Result<Self, sonium_common::SoniumError> {
        if payload.len() < 20 {
            return Err(sonium_common::SoniumError::Protocol(format!(
                "GroupSync payload too short: {} bytes",
                payload.len()
            )));
        }
        let server_now_us =
            i64::from_le_bytes(payload[0..8].try_into().map_err(|_| {
                sonium_common::SoniumError::Protocol("invalid server_now_us".into())
            })?);
        let group_offset_us =
            i64::from_le_bytes(payload[8..16].try_into().map_err(|_| {
                sonium_common::SoniumError::Protocol("invalid group_offset_us".into())
            })?);
        let rate_ppm = i32::from_le_bytes(
            payload[16..20]
                .try_into()
                .map_err(|_| sonium_common::SoniumError::Protocol("invalid rate_ppm".into()))?,
        );
        Ok(Self {
            server_now_us,
            group_offset_us,
            rate_ppm,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let gs = GroupSync::new(1_234_567, -890, 120);
        let bytes = gs.encode();
        let decoded = GroupSync::decode(&bytes).expect("decode ok");
        assert_eq!(decoded.server_now_us, gs.server_now_us);
        assert_eq!(decoded.group_offset_us, gs.group_offset_us);
        assert_eq!(decoded.rate_ppm, gs.rate_ppm);
    }
}
