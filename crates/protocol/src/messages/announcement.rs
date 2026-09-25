//! Versioned announcement lifecycle control message.
//!
//! Type 11 is new Sonium-native control traffic.  It is intentionally not a
//! reinterpretation of any legacy message and never contains media bytes.

use serde::{Deserialize, Serialize};

pub const ANNOUNCEMENT_CONTROL_VERSION: u8 = 1;
pub const MAX_ANNOUNCEMENT_CONTROL_IDENTIFIER_BYTES: usize = 128;
pub const MAX_ANNOUNCEMENT_CONTROL_DURATION_MS: u32 = 120_000;
pub const MAX_ANNOUNCEMENT_CONTROL_PAYLOAD_BYTES: usize = 16 * 1024;
pub const MAX_ANNOUNCEMENT_SOURCE_URI_BYTES: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementLifecycle {
    Scheduled,
    Started,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementPriorityV1 {
    Music,
    Chime,
    Announcement,
    Emergency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementResumeV1 {
    ResumePrevious,
    DoNotResume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnouncementDuckingV1 {
    pub attenuation_db: f32,
    pub attack_ms: u32,
    pub release_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnouncementIntentMetadataV1 {
    pub source_uri: String,
    pub priority: AnnouncementPriorityV1,
    pub duck: AnnouncementDuckingV1,
    pub expires_at_ms: i64,
    pub resume: AnnouncementResumeV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnouncementControlV1 {
    pub version: u8,
    pub announcement_id: String,
    pub group_id: String,
    pub lifecycle: AnnouncementLifecycle,
    pub scheduled_at_ms: i64,
    pub max_duration_ms: u32,
    /// Present on server-to-client scheduling messages.  Missing metadata is
    /// accepted so v1 payloads emitted before announcement media scheduling
    /// remain decodable; older v1 readers ignore this additive JSON field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<AnnouncementIntentMetadataV1>,
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
        if let Some(intent) = &self.intent {
            if intent.source_uri.is_empty()
                || intent.source_uri.len() > MAX_ANNOUNCEMENT_SOURCE_URI_BYTES
                || intent
                    .source_uri
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
                || !intent.duck.attenuation_db.is_finite()
                || !(-60.0..=0.0).contains(&intent.duck.attenuation_db)
                || intent.duck.attack_ms > 5_000
                || intent.duck.release_ms > 5_000
                || intent.expires_at_ms <= self.scheduled_at_ms
            {
                return Err(sonium_common::SoniumError::Protocol(
                    "invalid announcement intent metadata".into(),
                ));
            }
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
