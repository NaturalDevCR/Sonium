//! One-shot media playback: announcements, TTS and `play_media`.
//!
//! The control API validates a request and hands it to the audio server
//! through a [`PlayMediaRequest`] channel (see
//! [`ServerState::set_media_backend`][crate::ServerState::set_media_backend]).
//! The server decodes the URL into a temporary stream and temporarily
//! overrides the stream of each target client. When the media ends (or is
//! stopped) the override is released and every client returns to its group's
//! stream. Overrides are runtime-only and never persisted.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// An active one-shot media playback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSession {
    /// Unique ID; also the ID of the temporary stream carrying the audio.
    pub id: String,
    /// Source URL being played.
    pub url: String,
    /// Clients currently playing this media.
    pub client_ids: Vec<String>,
    /// Temporary volume (0–100) applied to the target clients, if any.
    pub volume: Option<u8>,
    /// When playback was requested.
    pub started_at: DateTime<Utc>,
}

/// Request sent from the control API to the audio server.
pub struct PlayMediaRequest {
    pub url: String,
    pub client_ids: Vec<String>,
    pub volume: Option<u8>,
    pub respond_to: oneshot::Sender<Result<MediaSession, String>>,
}

/// Validate a media URL: only remote HTTP(S) resources are accepted so the
/// API cannot be used to read local files or other ffmpeg protocols.
pub fn validate_media_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("url is required".into());
    }
    if url.len() > 4096 {
        return Err("url is too long".into());
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("url must not contain whitespace or control characters".into());
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("only http:// and https:// URLs are supported".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_urls() {
        assert!(validate_media_url("http://ha.local:8123/api/tts_proxy/abc.mp3").is_ok());
        assert!(validate_media_url("HTTPS://example.com/chime.wav").is_ok());
    }

    #[test]
    fn rejects_other_schemes_and_junk() {
        assert!(validate_media_url("").is_err());
        assert!(validate_media_url("file:///etc/passwd").is_err());
        assert!(validate_media_url("/etc/passwd").is_err());
        assert!(validate_media_url("-i http://x").is_err());
        assert!(validate_media_url("concat:http://a|http://b").is_err());
        assert!(validate_media_url("http://a b").is_err());
    }
}
