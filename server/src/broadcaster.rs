use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};

use crate::metrics;

/// Encoded audio frame ready to send on the wire.
#[derive(Clone, Debug)]
pub struct AudioFrame {
    pub wire_bytes: Bytes,
}

/// Fan-out hub for a single stream: encodes once, delivers to all sessions.
pub struct Broadcaster {
    pub stream_id:   String,
    pub buffer_ms:   u32,
    sender:          broadcast::Sender<AudioFrame>,
    codec_header:    Mutex<Option<Bytes>>,
}

impl Broadcaster {
    pub fn new(stream_id: impl Into<String>, buffer_ms: u32) -> Self {
        let (sender, _) = broadcast::channel(256);
        Self {
            stream_id:    stream_id.into(),
            buffer_ms,
            sender,
            codec_header: Mutex::new(None),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AudioFrame> {
        self.sender.subscribe()
    }

    pub fn publish(&self, wire_bytes: Bytes) {
        metrics::ENCODED_CHUNKS.with_label_values(&[&self.stream_id]).inc();
        let _ = self.sender.send(AudioFrame { wire_bytes });
    }

    pub fn set_codec_header(&self, bytes: Bytes) {
        *self.codec_header.lock() = Some(bytes);
    }

    pub fn codec_header(&self) -> Option<Bytes> {
        self.codec_header.lock().clone()
    }
}

/// Registry of all active stream broadcasters, keyed by stream ID.
pub type BroadcasterRegistry = RwLock<HashMap<String, Arc<Broadcaster>>>;

pub fn new_registry() -> Arc<BroadcasterRegistry> {
    Arc::new(RwLock::new(HashMap::new()))
}

pub fn register(registry: &Arc<BroadcasterRegistry>, bc: Arc<Broadcaster>) {
    registry.write().insert(bc.stream_id.clone(), bc);
}

pub fn lookup(registry: &Arc<BroadcasterRegistry>, stream_id: &str) -> Option<Arc<Broadcaster>> {
    registry.read().get(stream_id).cloned()
}
