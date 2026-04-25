use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;
use bytes::Bytes;

/// Encoded audio frame with playout timestamp, ready to send on the wire.
#[derive(Clone, Debug)]
pub struct AudioFrame {
    /// Fully serialized WireChunk message (header + payload).
    pub wire_bytes: Bytes,
}

/// Fan-out hub: encodes once, delivers to all connected sessions.
pub struct Broadcaster {
    sender:   broadcast::Sender<AudioFrame>,
    next_id:  AtomicU64,
    /// Codec+header bytes sent to new clients on connect.
    codec_header: parking_lot::Mutex<Option<Bytes>>,
    settings_msg: parking_lot::Mutex<Option<Bytes>>,
}

impl Broadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self {
            sender,
            next_id: AtomicU64::new(1),
            codec_header: parking_lot::Mutex::new(None),
            settings_msg: parking_lot::Mutex::new(None),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AudioFrame> {
        self.sender.subscribe()
    }

    /// Publish a new encoded frame to all active subscribers.
    pub fn publish(&self, wire_bytes: Bytes) {
        // Ignore send errors — no active listeners is OK
        let _ = self.sender.send(AudioFrame { wire_bytes });
    }

    pub fn next_client_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn set_codec_header(&self, bytes: Bytes) {
        *self.codec_header.lock() = Some(bytes);
    }

    pub fn codec_header(&self) -> Option<Bytes> {
        self.codec_header.lock().clone()
    }

    pub fn set_settings_msg(&self, bytes: Bytes) {
        *self.settings_msg.lock() = Some(bytes);
    }

    pub fn settings_msg(&self) -> Option<Bytes> {
        self.settings_msg.lock().clone()
    }

    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for Broadcaster {
    fn default() -> Self { Self::new() }
}
