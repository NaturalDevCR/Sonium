//! Versioned announcement lifecycle control message.
//!
//! Type 11 is new Sonium-native control traffic.  It is intentionally not a
//! reinterpretation of any legacy message and never contains media bytes.

use serde::{Deserialize, Serialize};

pub const ANNOUNCEMENT_CONTROL_VERSION: u8 = 1;
pub const MAX_ANNOUNCEMENT_CONTROL_IDENTIFIER_BYTES: usize = 128;
pub const MAX_ANNOUNCEMENT_CONTROL_DURATION_MS: u32 = 120_000;
pub const MAX_ANNOUNCEMENT_CONTROL_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementLifecycle {
    Scheduled,
    Started,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncementControlV1 {
    pub version: u8,
    pub announcement_id: String,
    pub group_id: String,
    pub lifecycle: AnnouncementLifecycle,
    pub scheduled_at_ms: i64,
    pub max_duration_ms: u32,
}

impl AnnouncementControlV1 {
    /// Validate the bounded v1 semantic contract before putting it on the wire.
    pub fn validate(&self) -> Result<(), sonium_common::SoniumError> {
        if self.version != ANNOUNCEMENT_CONTROL_VERSION {
            return Err(sonium_common::SoniumError::Protocol(format!(
                "unsupported announcement control version {}",
                self.version
            )));
        }
        if self.announcement_id.is_empty()
            || self.announcement_id.len() > MAX_ANNOUNCEMENT_CONTROL_IDENTIFIER_BYTES
            || self.group_id.is_empty()
            || self.group_id.len() > MAX_ANNOUNCEMENT_CONTROL_IDENTIFIER_BYTES
            || self
                .announcement_id
                .bytes()
                .chain(self.group_id.bytes())
                .any(|byte| byte.is_ascii_control())
        {
            return Err(sonium_common::SoniumError::Protocol(
                "invalid announcement control identifiers".into(),
            ));
        }
        if self.scheduled_at_ms < 0
            || self.max_duration_ms == 0
            || self.max_duration_ms > MAX_ANNOUNCEMENT_CONTROL_DURATION_MS
        {
            return Err(sonium_common::SoniumError::Protocol(
                "invalid announcement control timing".into(),
            ));
        }
        Ok(())
    }

    /// Fallible encoder for callers that need to handle invalid outbound data.
    pub fn try_encode(&self) -> Result<Vec<u8>, sonium_common::SoniumError> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|error| {
            sonium_common::SoniumError::Protocol(format!("invalid announcement control: {error}"))
        })?;
        if encoded.len() > MAX_ANNOUNCEMENT_CONTROL_PAYLOAD_BYTES {
            return Err(sonium_common::SoniumError::Protocol(
                "announcement control payload exceeds maximum size".into(),
            ));
        }
        Ok(encoded)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.try_encode()
            .expect("announcement control must satisfy the v1 contract")
    }

    pub fn decode(payload: &[u8]) -> Result<Self, sonium_common::SoniumError> {
        if payload.len() > MAX_ANNOUNCEMENT_CONTROL_PAYLOAD_BYTES {
            return Err(sonium_common::SoniumError::Protocol(
                "announcement control payload exceeds maximum size".into(),
            ));
        }
        let message: Self = serde_json::from_slice(payload).map_err(|error| {
            sonium_common::SoniumError::Protocol(format!("invalid announcement control: {error}"))
        })?;
        message.validate()?;
        Ok(message)
    }
}
