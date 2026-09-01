//! Versioned announcement lifecycle control message.
//!
//! Type 11 is new Sonium-native control traffic.  It is intentionally not a
//! reinterpretation of any legacy message and never contains media bytes.

use serde::{Deserialize, Serialize};

pub const ANNOUNCEMENT_CONTROL_VERSION: u8 = 1;

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
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("announcement control is serializable")
    }

    pub fn decode(payload: &[u8]) -> Result<Self, sonium_common::SoniumError> {
        let message: Self = serde_json::from_slice(payload).map_err(|error| {
            sonium_common::SoniumError::Protocol(format!("invalid announcement control: {error}"))
        })?;
        if message.version != ANNOUNCEMENT_CONTROL_VERSION {
            return Err(sonium_common::SoniumError::Protocol(format!(
                "unsupported announcement control version {}",
                message.version
            )));
        }
        if message.announcement_id.is_empty()
            || message.announcement_id.len() > 128
            || message.group_id.is_empty()
            || message.group_id.len() > 128
        {
            return Err(sonium_common::SoniumError::Protocol(
                "invalid announcement control identifiers".into(),
            ));
        }
        Ok(message)
    }
}
