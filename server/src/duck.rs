//! Ducked announcements: the announcement is mixed over the lowered music.
//!
//! For every music stream the targets are listening to, a mixer subscribes to
//! that stream, decodes each chunk, applies a smooth ducking envelope, adds
//! the announcement audio and re-encodes it with the *same* codec, format and
//! timestamps. Sessions switch to the mixed stream without a CodecHeader, so
//! clients keep their decoder and jitter buffer: the music never stops, it
//! just dips under the announcement and comes back.

use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Duration;
use tracing::{info, warn};

use sonium_codec::{make_decoder, make_encoder, Decoder, Encoder};
use sonium_common::SampleFormat;
use sonium_control::media::{AnnouncementMode, MediaSession};
use sonium_control::state::StreamStatus;
use sonium_control::ServerState;
use sonium_protocol::{
    header::HEADER_SIZE,
    messages::{CodecHeader, Message, WireChunk},
    Timestamp,
};

use crate::broadcaster::{self, AudioFrame, Broadcaster, BroadcasterRegistry};
use crate::streamreader::Pacer;

/// Give up if the URL produces no audio at all for this long.
const FIRST_AUDIO_TIMEOUT: Duration = Duration::from_secs(20);
/// Chunks re-encoded before clients switch over (hides decoder warm-up).
const PRIME_CHUNKS: u32 = 3;
/// Keep the mixed streams registered this long after the clients were
/// released, so every session can finish its seamless switch back.
const LINGER: Duration = Duration::from_secs(5);

/// Ducking envelope parameters.
#[derive(Debug, Clone, Copy)]
pub struct DuckParams {
    pub duck_db: f32,
    pub attack_ms: u32,
    pub release_ms: u32,
}

/// A music stream that can be ducked, and the clients listening to it.
pub struct MixerPlan {
    base_id: String,
    base: Arc<Broadcaster>,
    header: Bytes,
    decoder: Box<dyn Decoder + Send>,
    encoder: Box<dyn Encoder + Send>,
    fmt: SampleFormat,
    clients: Vec<String>,
}

/// Split the targets into duckable groups (by music stream) and clients that
/// must fall back to replace mode (stream not playing, missing or not
/// re-encodable identically).
pub fn plan(
    client_ids: &[String],
    registry: &Arc<BroadcasterRegistry>,
    state: &ServerState,
) -> (Vec<MixerPlan>, Vec<String>) {
    let mut by_stream: HashMap<String, Vec<String>> = HashMap::new();
    let mut fallback = Vec::new();
    for cid in client_ids {
        match state.client_group_stream_id(cid) {
            Some(sid) => by_stream.entry(sid).or_default().push(cid.clone()),
            None => fallback.push(cid.clone()),
        }
    }

    let playing: Vec<String> = state
        .all_streams()
        .into_iter()
        .filter(|s| s.status == StreamStatus::Playing)
        .map(|s| s.id)
        .collect();

    let mut plans = Vec::new();
    for (base_id, clients) in by_stream {
        match mixer_for(&base_id, registry, &playing) {
            Some((base, header, decoder, encoder, fmt)) => plans.push(MixerPlan {
                base_id,
                base,
                header,
                decoder,
                encoder,
                fmt,
                clients,
            }),
            None => fallback.extend(clients),
        }
    }
    (plans, fallback)
}

#[allow(clippy::type_complexity)]
fn mixer_for(
    base_id: &str,
    registry: &Arc<BroadcasterRegistry>,
    playing: &[String],
) -> Option<(
    Arc<Broadcaster>,
    Bytes,
    Box<dyn Decoder + Send>,
    Box<dyn Encoder + Send>,
    SampleFormat,
)> {
    if !playing.iter().any(|p| p == base_id) {
        // Nothing audible to duck: replace mode gives the same result.
        return None;
    }
    let base = broadcaster::lookup(registry, base_id)?;
    let header = base.codec_header()?;
    let ch = CodecHeader::decode(header.get(HEADER_SIZE..)?).ok()?;
    let decoder = make_decoder(&ch.codec, &ch.header_data).ok()?;
    let fmt = decoder.sample_format();
    if !(1..=2).contains(&fmt.channels) {
        return None;
    }
    let encoder = make_encoder(&ch.codec, fmt).ok()?;
    let ours = Message::CodecHeader(CodecHeader::new(
        encoder.codec_name(),
        encoder.codec_header(),
    ))
    .encode();
    // The client must be able to decode our frames with its current decoder.
    if ours[HEADER_SIZE..] != header[HEADER_SIZE..] {
        return None;
    }
    Some((base, header, decoder, encoder, fmt))
}

/// Start a ducked playback and return its session immediately.
pub fn start(
    url: String,
    plans: Vec<MixerPlan>,
    volume: Option<u8>,
    params: DuckParams,
    registry: &Arc<BroadcasterRegistry>,
    state: &Arc<ServerState>,
) -> MediaSession {
    let id = format!("media-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let session = MediaSession {
        id: id.clone(),
        url: url.clone(),
        client_ids: plans.iter().flat_map(|p| p.clients.clone()).collect(),
        volume,
        mode: AnnouncementMode::Duck,
        started_at: chrono::Utc::now(),
    };
    info!(
        media = %id,
        url = %url,
        clients = ?session.client_ids,
        streams = ?plans.iter().map(|p| p.base_id.as_str()).collect::<Vec<_>>(),
        duck_db = params.duck_db,
        "Starting ducked media playback"
    );

    let registry = registry.clone();
    let state = state.clone();
    let pending = session.clone();
    tokio::spawn(async move {
        run(url, pending, plans, params, registry, state).await;
    });
    session
}

async fn run(
    url: String,
    session: MediaSession,
    plans: Vec<MixerPlan>,
    params: DuckParams,
    registry: Arc<BroadcasterRegistry>,
    state: Arc<ServerState>,
) {
    let id = session.id.clone();
    let go = Arc::new(AtomicBool::new(false));
    let mut routes = Vec::new();
    let mut duck_bases = Vec::new();
    let mut stream_ids = Vec::new();
    let mut mixers = Vec::new();
    let mut primed = Vec::new();

    for (index, plan) in plans.into_iter().enumerate() {
        // Decode the announcement in the music's own rate/channel layout and
        // wait for real audio before touching any client.
        let (first, audio_rx) = match open_audio(&url, plan.fmt).await {
            Ok(v) => v,
            Err(e) => {
                warn!(media = %id, stream = %plan.base_id, "Announcement audio unavailable: {e}");
                continue;
            }
        };
        let stream_id = format!("{id}-{index}");
        let bc = Arc::new(Broadcaster::new(&stream_id, plan.base.buffer_ms));
        bc.set_codec_header(plan.header.clone());
        broadcaster::register(&registry, bc.clone());

        for cid in &plan.clients {
            routes.push((cid.clone(), stream_id.clone()));
        }
        duck_bases.push((stream_id.clone(), plan.base_id.clone()));
        stream_ids.push(stream_id.clone());

        let (primed_tx, primed_rx) = oneshot::channel();
        primed.push(primed_rx);
        let mixer = Mixer {
            base_rx: plan.base.subscribe(),
            decoder: plan.decoder,
            encoder: plan.encoder,
            fmt: plan.fmt,
            buffer_ms: plan.base.buffer_ms,
            params,
            out: bc,
            stream_id,
            state: state.clone(),
            go: go.clone(),
            primed: Some(primed_tx),
        };
        mixers.push(tokio::spawn(mixer.run(first, audio_rx)));
    }

    if mixers.is_empty() {
        warn!(media = %id, "Ducked media playback failed: no audio decoded from {url}");
        return;
    }

    for rx in primed {
        let _ = tokio::time::timeout(Duration::from_secs(2), rx).await;
    }
    state.begin_media_routed(session, routes, duck_bases);
    go.store(true, Ordering::Release);

    for mixer in mixers {
        let _ = mixer.await;
    }
    state.end_media(&id);
    info!(media = %id, "Ducked media playback finished");

    tokio::time::sleep(LINGER).await;
    state.forget_duck_streams(&stream_ids);
    for sid in &stream_ids {
        broadcaster::unregister(&registry, sid);
    }
}

/// Spawn ffmpeg for `url` producing PCM in `fmt`; returns the first block
/// once available plus a channel with the rest (closed at end of file).
async fn open_audio(
    url: &str,
    fmt: SampleFormat,
) -> anyhow::Result<(Vec<i16>, mpsc::Receiver<Vec<i16>>)> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-protocol_whitelist",
            "http,https,tcp,tls,crypto",
            "-i",
            url,
            "-vn",
            "-ac",
            &fmt.channels.to_string(),
            "-ar",
            &fmt.rate.to_string(),
            "-f",
            "s16le",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot start ffmpeg (is it installed?): {e}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg has no stdout"))?;
    let mut stderr = child.stderr.take();

    let (tx, mut rx) = mpsc::channel::<Vec<i16>>(64);
    tokio::spawn(async move {
        // Owning `child` here keeps ffmpeg alive exactly as long as the reader.
        let _child = child;
        let mut buf = vec![0u8; 8192];
        let mut carry: Option<u8> = None;
        loop {
            let n = match stdout.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut bytes: Vec<u8> = Vec::with_capacity(n + 1);
            if let Some(b) = carry.take() {
                bytes.push(b);
            }
            bytes.extend_from_slice(&buf[..n]);
            if bytes.len() % 2 == 1 {
                carry = bytes.pop();
            }
            let pcm: Vec<i16> = bytes
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            if tx.send(pcm).await.is_err() {
                break; // mixer finished or stopped
            }
        }
    });

    match tokio::time::timeout(FIRST_AUDIO_TIMEOUT, rx.recv()).await {
        Ok(Some(first)) => Ok((first, rx)),
        Ok(None) => {
            let mut msg = Vec::new();
            if let Some(ref mut e) = stderr {
                let _ = AsyncReadExt::take(e, 4096).read_to_end(&mut msg).await;
            }
            let msg = String::from_utf8_lossy(&msg);
            anyhow::bail!("no audio decoded: {}", msg.trim())
        }
        Err(_) => anyhow::bail!("timed out waiting for audio"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Clients not switched yet: pass the music through unchanged.
    Waiting,
    /// Fading the music down.
    Attack,
    /// Music ducked, announcement playing.
    Play,
    /// Announcement over, fading the music back up.
    Release,
    Done,
}

/// Envelope + mixing state, kept separate from I/O so it can be unit tested.
struct DuckMix {
    channels: usize,
    duck_gain: f32,
    attack_step: f32,
    release_step: f32,
    gain: f32,
    phase: Phase,
    pending: VecDeque<i16>,
    audio_done: bool,
}

impl DuckMix {
    fn new(fmt: SampleFormat, params: DuckParams, first: Vec<i16>) -> Self {
        let duck_gain = 10f32.powf(params.duck_db / 20.0);
        let frames = |ms: u32| (u64::from(fmt.rate) * u64::from(ms) / 1000).max(1) as f32;
        Self {
            channels: usize::from(fmt.channels.max(1)),
            duck_gain,
            attack_step: (1.0 - duck_gain) / frames(params.attack_ms),
            release_step: (1.0 - duck_gain) / frames(params.release_ms),
            gain: 1.0,
            phase: Phase::Waiting,
            pending: first.into(),
            audio_done: false,
        }
    }

    fn start(&mut self) {
        if self.phase == Phase::Waiting {
            self.phase = Phase::Attack;
        }
    }

    fn push_audio(&mut self, block: Vec<i16>) {
        self.pending.extend(block);
    }

    fn finish_audio(&mut self) {
        self.audio_done = true;
    }

    /// Duck `music` in place and mix in announcement samples.
    fn process(&mut self, music: &mut [i16]) {
        for frame in music.chunks_mut(self.channels) {
            match self.phase {
                Phase::Waiting | Phase::Done => self.gain = 1.0,
                Phase::Attack => {
                    self.gain -= self.attack_step;
                    if self.gain <= self.duck_gain {
                        self.gain = self.duck_gain;
                        self.phase = Phase::Play;
                    }
                }
                Phase::Play => {
                    if self.audio_done && self.pending.is_empty() {
                        self.phase = Phase::Release;
                    }
                }
                Phase::Release => {
                    self.gain += self.release_step;
                    if self.gain >= 1.0 {
                        self.gain = 1.0;
                        self.phase = Phase::Done;
                    }
                }
            }
            let play = self.phase == Phase::Play;
            for sample in frame.iter_mut() {
                let voice = if play {
                    f32::from(self.pending.pop_front().unwrap_or(0))
                } else {
                    0.0
                };
                let mixed = f32::from(*sample) * self.gain + voice;
                *sample = mixed.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
            }
        }
    }

    fn done(&self) -> bool {
        self.phase == Phase::Done
    }
}

struct Mixer {
    base_rx: broadcast::Receiver<AudioFrame>,
    decoder: Box<dyn Decoder + Send>,
    encoder: Box<dyn Encoder + Send>,
    fmt: SampleFormat,
    buffer_ms: u32,
    params: DuckParams,
    out: Arc<Broadcaster>,
    stream_id: String,
    state: Arc<ServerState>,
    go: Arc<AtomicBool>,
    primed: Option<oneshot::Sender<()>>,
}

impl Mixer {
    async fn run(mut self, first: Vec<i16>, mut audio_rx: mpsc::Receiver<Vec<i16>>) {
        let mut mix = DuckMix::new(self.fmt, self.params, first);
        let mut enc_buf = Vec::new();
        let mut published = 0u32;
        // Self-clock used only if the music stream stops mid-announcement.
        let mut chunk_samples = self.fmt.frames_for_ms(20.0) * mix.channels;
        let mut pacer = Pacer::new(20, self.buffer_ms);
        let mut synced = false;

        loop {
            if self.go.load(Ordering::Acquire) {
                mix.start();
                if self.state.media_stream_client_count(&self.stream_id) == 0 {
                    info!(stream = %self.stream_id, "Ducked media stopped");
                    break;
                }
            }
            loop {
                match audio_rx.try_recv() {
                    Ok(block) => mix.push_audio(block),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        mix.finish_audio();
                        break;
                    }
                }
            }

            let (slot_ts, wait) = pacer.time_until_slot();
            let stall = Duration::from_millis(u64::from(self.buffer_ms / 2).max(60));
            let (ts, mut pcm) = tokio::select! {
                biased;
                frame = self.base_rx.recv() => match frame {
                    Ok(frame) => match self.decode(&frame.wire_bytes) {
                        Some((ts, pcm)) => {
                            if !synced || pcm.len() != chunk_samples {
                                chunk_samples = pcm.len().max(mix.channels);
                                let ms = (chunk_samples / mix.channels) as u64 * 1000
                                    / u64::from(self.fmt.rate.max(1));
                                pacer = Pacer::new(ms.max(1) as u32, self.buffer_ms);
                                synced = true;
                            }
                            pacer.advance(ts);
                            (ts, pcm)
                        }
                        None => continue,
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                // The music stopped: keep the announcement going on its own clock.
                _ = tokio::time::sleep(wait + stall), if synced => {
                    pacer.advance(slot_ts);
                    (slot_ts, vec![0i16; chunk_samples])
                }
            };

            mix.process(&mut pcm);
            enc_buf.clear();
            if let Err(e) = self.encoder.encode(&pcm, &mut enc_buf) {
                warn!(stream = %self.stream_id, "Ducked mix encode error: {e}");
                continue;
            }
            let chunk = WireChunk::new(Timestamp::from_micros(ts), enc_buf.clone());
            self.out
                .publish(Bytes::from(Message::WireChunk(chunk).encode()));

            published += 1;
            if published >= PRIME_CHUNKS {
                if let Some(tx) = self.primed.take() {
                    let _ = tx.send(());
                }
            }
            if mix.done() {
                break;
            }
        }
    }

    /// Timestamp and PCM of an encoded music chunk.
    fn decode(&mut self, wire: &[u8]) -> Option<(i64, Vec<i16>)> {
        let chunk = WireChunk::decode(wire.get(HEADER_SIZE..)?).ok()?;
        let mut pcm = Vec::new();
        if self.decoder.decode(&chunk.data, &mut pcm).is_err() || pcm.is_empty() {
            return None;
        }
        Some((chunk.timestamp.to_micros(), pcm))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DuckParams {
        DuckParams {
            duck_db: -20.0,
            attack_ms: 1,
            release_ms: 1,
        }
    }

    fn fmt() -> SampleFormat {
        SampleFormat::new(8_000, 16, 1)
    }

    #[test]
    fn music_passes_untouched_until_started() {
        let mut mix = DuckMix::new(fmt(), params(), vec![500; 4]);
        let mut music = vec![1000i16; 8];
        mix.process(&mut music);
        assert!(music.iter().all(|s| *s == 1000));
    }

    #[test]
    fn ducks_music_mixes_voice_and_recovers() {
        // 1 ms at 8 kHz = 8 frames of attack/release.
        let mut mix = DuckMix::new(fmt(), params(), vec![500; 16]);
        mix.finish_audio();
        mix.start();
        let mut music = vec![1000i16; 64];
        mix.process(&mut music);
        // Attack ramps down monotonically.
        assert!(music[0] < 1000 && music[0] > 100);
        // While ducked (-20 dB = 0.1) the voice is added on top.
        assert!(music.contains(&600));
        // After the voice ends the music fades back to full level.
        assert_eq!(*music.last().unwrap(), 1000);
        assert!(mix.done());
    }

    #[test]
    fn mix_clamps_instead_of_wrapping() {
        let loud = DuckParams {
            duck_db: 0.0,
            attack_ms: 1,
            release_ms: 1,
        };
        let mut mix = DuckMix::new(fmt(), loud, vec![i16::MAX; 32]);
        mix.start();
        let mut music = vec![i16::MAX; 32];
        mix.process(&mut music);
        assert!(music.iter().all(|s| *s == i16::MAX));
    }
}
