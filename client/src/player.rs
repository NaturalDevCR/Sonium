use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tracing::{debug, info, warn};

use sonium_common::{SampleFormat, SoniumError};
use sonium_sync::time_provider::now_us;
use sonium_sync::{PcmChunk, SyncBuffer};

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

/// Audio output backed by the system default device via CPAL.
///
/// The CPAL stream lives on a dedicated OS thread to avoid `!Send` issues on
/// macOS (CoreAudio streams must be created and used on the same thread).
/// PCM samples are normally pulled from a server-timestamped playback timeline
/// directly inside the audio callback.  A legacy ring-buffer write path remains
/// for simple fallback use.
///
/// **Hotplug recovery:** when a device error occurs (e.g. USB DAC
/// disconnected), the audio thread automatically retries opening the stream
/// with exponential backoff (100 ms → 5 s).  `Player::write()` silently
/// drops samples while the device is unavailable — no panics, no hangs.
///
/// Dropping `Player` closes the keep-alive sender, which unblocks the audio
/// thread so the stream is stopped and the thread exits cleanly.
#[derive(Debug, Default)]
struct HealthState {
    underrun_count: AtomicU32,
    overrun_count: AtomicU32,
    callback_starvation_count: AtomicU32,
    audio_callback_xrun_count: AtomicU32,
    drift_drop_count: AtomicU64,
    drift_dup_count: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerHealth {
    pub underrun_count: u32,
    pub overrun_count: u32,
    pub callback_starvation_count: u32,
    pub audio_callback_xrun_count: u32,
    pub drift_drop_count: u64,
    pub drift_dup_count: u64,
}

pub struct Player {
    ring: Arc<Mutex<VecDeque<i16>>>,
    fmt: SampleFormat,
    health: Arc<HealthState>,
    max_samples: Arc<AtomicUsize>,
    playback: Option<PlaybackHandle>,
    latest_output_latency_us: Arc<AtomicI64>,
    /// Dropping this disconnects the audio thread's park channel, stopping it.
    _keepalive: SyncSender<()>,
}

#[derive(Clone)]
struct AudioState {
    ring: Arc<Mutex<VecDeque<i16>>>,
    health: Arc<HealthState>,
    playback: Option<PlaybackHandle>,
    latest_output_latency_us: Arc<AtomicI64>,
}

#[derive(Clone)]
pub struct PlaybackHandle {
    inner: Arc<Mutex<PlaybackTimeline>>,
    offset_us: Arc<AtomicI64>,
    latest_output_latency_us: Arc<AtomicI64>,
}

impl PlaybackHandle {
    pub fn new(fmt: SampleFormat, offset_us: Arc<AtomicI64>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PlaybackTimeline::new(fmt))),
            offset_us,
            latest_output_latency_us: Arc::new(AtomicI64::new(0)),
        }
    }

    pub fn push(&self, chunk: PcmChunk, arrival_us: i64) {
        self.inner.lock().unwrap().buffer.push(chunk, arrival_us);
    }

    pub fn take_drift_metrics(&self) -> (u64, u64) {
        let mut inner = self.inner.lock().unwrap();
        (
            inner.buffer.take_drift_drop_count(),
            inner.buffer.take_drift_dup_count(),
        )
    }

    pub fn set_target_buffer_ms(&self, buffer_ms: i32) {
        // Delegates to PlaybackTimeline so the resampler is also reset when
        // the target changes (its EMA should start fresh).
        self.inner
            .lock()
            .unwrap()
            .set_target_buffer_ms(buffer_ms);
    }

    pub fn buffer_depth_us(&self) -> i64 {
        self.inner.lock().unwrap().buffer.buffer_depth_us()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().buffer.is_empty()
    }

    pub fn jitter_us(&self) -> i64 {
        self.inner.lock().unwrap().buffer.jitter_us()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.buffer.clear();
        inner.current = None;
    }

    pub fn get_report(&self, now_server_us: i64) -> sonium_protocol::messages::HealthReport {
        self.inner.lock().unwrap().buffer.get_report(now_server_us)
    }

    fn fill_i16(
        &self,
        out: &mut [i16],
        info: &cpal::OutputCallbackInfo,
        health: &HealthState,
        fade: &mut FadeState,
    ) {
        let output_latency_us = output_latency_us(info).unwrap_or_else(|| {
            let frames = out.len() / self.inner.lock().unwrap().fmt.channels as usize;
            frames_to_us(frames, self.inner.lock().unwrap().fmt.rate)
        });
        self.latest_output_latency_us
            .store(output_latency_us, Ordering::Relaxed);
        let dac_server_us = now_us() + self.offset_us.load(Ordering::Relaxed) + output_latency_us;
        self.inner
            .lock()
            .unwrap()
            .fill_i16(out, dac_server_us, health, fade);
    }

    fn fill_f32(
        &self,
        out: &mut [f32],
        info: &cpal::OutputCallbackInfo,
        health: &HealthState,
        fade: &mut FadeStateF32,
    ) {
        let output_latency_us = output_latency_us(info).unwrap_or_else(|| {
            let frames = out.len() / self.inner.lock().unwrap().fmt.channels as usize;
            frames_to_us(frames, self.inner.lock().unwrap().fmt.rate)
        });
        self.latest_output_latency_us
            .store(output_latency_us, Ordering::Relaxed);
        let dac_server_us = now_us() + self.offset_us.load(Ordering::Relaxed) + output_latency_us;
        self.inner
            .lock()
            .unwrap()
            .fill_f32(out, dac_server_us, health, fade);
    }

    pub fn latest_output_latency_us(&self) -> i64 {
        self.latest_output_latency_us.load(Ordering::Relaxed)
    }
}

struct PlaybackTimeline {
    buffer: SyncBuffer,
    current: Option<PcmChunk>,
    fmt: SampleFormat,
    /// Adaptive resampler for f32 output; initialised lazily on first f32 callback.
    resampler: Option<AdaptiveResampler>,
}

impl PlaybackTimeline {
    fn new(fmt: SampleFormat) -> Self {
        Self {
            buffer: SyncBuffer::new(fmt),
            current: None,
            fmt,
            resampler: None,
        }
    }

    fn set_target_buffer_ms(&mut self, ms: i32) {
        self.buffer.set_target_buffer_ms(ms);
        // Tear down and rebuild the resampler when the target changes so its
        // internal state (EMA ratio) starts from a clean baseline.
        self.resampler = None;
    }

    fn fill_i16(
        &mut self,
        out: &mut [i16],
        dac_server_us: i64,
        health: &HealthState,
        fade: &mut FadeState,
    ) {
        let channels = self.fmt.channels as usize;
        let mut sample_pos = 0usize;
        let mut produced_frames = 0usize;

        while sample_pos < out.len() {
            let frame_server_us = dac_server_us + frames_to_us(produced_frames, self.fmt.rate);

            if self.current.is_none() {
                self.current = self.buffer.pop_due_exact(frame_server_us);
            }

            let Some(chunk) = self.current.as_mut() else {
                let remaining_frames = (out.len() - sample_pos) / channels;
                let silence_frames = self
                    .buffer
                    .next_playout_us()
                    .and_then(|next| {
                        if next > frame_server_us {
                            Some(us_to_frames(next - frame_server_us, self.fmt.rate))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(remaining_frames)
                    .clamp(1, remaining_frames);
                if fade.phase == FadePhase::Playing {
                    health.underrun_count.fetch_add(1, Ordering::Relaxed);
                }
                let silence_samples = silence_frames * channels;
                for sample in &mut out[sample_pos..sample_pos + silence_samples] {
                    *sample = fade.drain();
                }
                sample_pos += silence_samples;
                produced_frames += silence_frames;
                continue;
            };

            // Legacy drop/dup logic remains for i16 path
            let age_us = frame_server_us - chunk.current_playout_us();
            if age_us > 2000 && chunk.remaining_samples() > channels {
                chunk.read_pos += channels;
                health.drift_drop_count.fetch_add(1, Ordering::Relaxed);
            } else if age_us < -2000 && chunk.read_pos >= channels {
                let prev_pos = chunk.read_pos - channels;
                for i in 0..channels {
                    if sample_pos < out.len() {
                        out[sample_pos] = fade.feed(chunk.samples[prev_pos + i]);
                        sample_pos += 1;
                    }
                }
                produced_frames += 1;
                health.drift_dup_count.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            let remaining_output_frames = (out.len() - sample_pos) / channels;
            let chunk_frames = chunk.remaining_samples() / channels;
            let frames = remaining_output_frames.min(chunk_frames);
            let samples = frames * channels;
            for src in &chunk.samples[chunk.read_pos..chunk.read_pos + samples] {
                out[sample_pos] = fade.feed(*src);
                sample_pos += 1;
            }
            chunk.read_pos += samples;
            produced_frames += frames;

            if chunk.is_exhausted() {
                self.current = None;
            }
        }
    }

    fn fill_f32(
        &mut self,
        out: &mut [f32],
        dac_server_us: i64,
        health: &HealthState,
        fade: &mut FadeStateF32,
    ) {
        let channels = self.fmt.channels as usize;
        let mut sample_pos = 0usize;
        let mut produced_frames = 0usize;

        // Initialise resampler lazily on first f32 callback.
        if self.resampler.is_none() {
            self.resampler = Some(AdaptiveResampler::new(channels, self.fmt.rate));
        }
        let resampler = self.resampler.as_mut().unwrap();

        // Update drift-correction ratio once per callback from buffer-depth feedback.
        resampler.update_ratio(
            self.buffer.buffer_depth_us(),
            self.buffer.target_buffer_us(),
        );

        while sample_pos < out.len() {
            // ── 1. Drain resampler output ring first ──────────────────────
            if let Some(s) = resampler.pop_output() {
                out[sample_pos] = fade.feed(s);
                sample_pos += 1;
                if sample_pos.is_multiple_of(channels) {
                    produced_frames += 1;
                }
                continue;
            }

            // ── 2. Need more input — resolve current chunk ────────────────
            let frame_server_us = dac_server_us + frames_to_us(produced_frames, self.fmt.rate);

            if self.current.is_none() {
                self.current = self.buffer.pop_due_exact(frame_server_us);
            }

            let Some(chunk) = self.current.as_mut() else {
                // Underrun: fill remaining output with silence.
                let remaining_frames = (out.len() - sample_pos) / channels;
                let silence_frames = self
                    .buffer
                    .next_playout_us()
                    .and_then(|next| {
                        if next > frame_server_us {
                            Some(us_to_frames(next - frame_server_us, self.fmt.rate))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(remaining_frames)
                    .clamp(1, remaining_frames);
                if fade.phase == FadePhase::Playing {
                    health.underrun_count.fetch_add(1, Ordering::Relaxed);
                }
                let silence_samples = silence_frames * channels;
                for s in &mut out[sample_pos..sample_pos + silence_samples] {
                    *s = fade.drain();
                }
                sample_pos += silence_samples;
                produced_frames += silence_frames;
                continue;
            };

            // ── 3. Gross drift guard (>2 ms) — coarse drop / dup ─────────
            // The resampler only handles fine corrections (≤500 ppm).  Large
            // errors are resolved here first so the resampler never operates
            // far from unity ratio.
            let age_us = frame_server_us - chunk.current_playout_us();
            if age_us > 2_000 && chunk.remaining_samples() > channels {
                // Running ahead: drop one frame.
                chunk.read_pos += channels;
                health.drift_drop_count.fetch_add(1, Ordering::Relaxed);
                if chunk.is_exhausted() {
                    self.current = None;
                }
                continue;
            }
            if age_us < -2_000 && chunk.read_pos >= channels {
                // Running behind: duplicate the previous frame.
                let prev = chunk.read_pos - channels;
                resampler.push_frame(&chunk.samples[prev..prev + channels]);
                health.drift_dup_count.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            // ── 4. Normal path: feed one frame to the resampler ──────────
            if chunk.remaining_samples() >= channels {
                let start = chunk.read_pos;
                resampler.push_frame(&chunk.samples[start..start + channels]);
                chunk.read_pos += channels;
            }
            if chunk.is_exhausted() {
                self.current = None;
            }
        }
    }
}

// ── Adaptive Resampling ───────────────────────────────────────────────────────
//
// Design principles:
//
// * Uses SincFixedIn with lightweight quality parameters suited for real-time
//   drift correction, not offline high-quality resampling:
//     sinc_len=64, oversampling_factor=16  (~10× faster than the 256/128 config)
//     WindowFunction::Hann                 (fastest of the provided windows)
//     SincInterpolationType::Linear        (cheapest interpolation)
//   At 48 kHz stereo this stays well under 1 ms CPU per 10 ms block.
//
// * Feedback signal: buffer_depth_us − target_buffer_us.
//   Positive error (buffer too full) → ratio > 1 → consume input faster.
//   Negative error (buffer too empty) → ratio < 1 → slow down.
//   Max correction: ±500 ppm.  Imperceptible below ~1000 ppm for music.
//
// * EMA smoothing (τ ≈ 200 callbacks ≈ 2 s at 10 ms/callback) prevents
//   audible pitch wobble from short-term jitter spikes.
//
// * Gross errors (> 2 ms) are handled by the caller with drop/dup before
//   frames reach this resampler, so it only ever needs fine corrections.

struct AdaptiveResampler {
    resampler: SincFixedIn<f32>,
    /// Deinterleaved input accumulator: [channels][chunk_size]
    input_bufs: Vec<Vec<f32>>,
    /// How many frames are currently filled in input_bufs
    input_filled: usize,
    /// Deinterleaved output buffers allocated by rubato
    output_bufs: Vec<Vec<f32>>,
    /// Interleaved f32 output samples ready to consume
    output_pending: VecDeque<f32>,
    channels: usize,
    /// Input frames per rubato call (10 ms worth)
    chunk_size: usize,
    /// EMA-smoothed resampling ratio
    rate_ema: f64,
}

impl AdaptiveResampler {
    /// Create a new resampler for `channels` channels at `sample_rate` Hz.
    fn new(channels: usize, sample_rate: u32) -> Self {
        // 10 ms processing blocks — balances latency against rubato overhead.
        let chunk_size = (sample_rate / 100) as usize;

        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 16,
            window: WindowFunction::Hann,
        };
        // max_resample_ratio_relative is a multiplier ≥ 1.0.
        // 1.01 → ratio may deviate ±1% from the base; our ±500 ppm limit is
        // well inside that envelope.
        let resampler =
            SincFixedIn::new(1.0, 1.01, params, chunk_size, channels)
                .expect("rubato SincFixedIn init failed");
        let input_bufs = vec![vec![0.0f32; chunk_size]; channels];
        let output_bufs = resampler.output_buffer_allocate(true);

        Self {
            resampler,
            input_bufs,
            input_filled: 0,
            output_bufs,
            output_pending: VecDeque::with_capacity(chunk_size * channels * 4),
            channels,
            chunk_size,
            rate_ema: 1.0,
        }
    }

    /// Update the resampling ratio from buffer-depth feedback.
    ///
    /// Call once per audio callback, before pulling samples.
    ///
    /// * `depth_us`  — current SyncBuffer depth in microseconds.
    /// * `target_us` — target buffer depth in microseconds.
    fn update_ratio(&mut self, depth_us: i64, target_us: i64) {
        let error_us = depth_us - target_us;
        // Proportional controller: clamp to ±500 ppm.
        // A 10 ms error at full correction converges in ~10 s — slow enough
        // to be inaudible, fast enough to track typical LAN jitter.
        let p = (error_us as f64 / 10_000.0).clamp(-0.005, 0.005);
        let target_ratio = 1.0 + p;
        // EMA τ ≈ 200 callbacks to avoid audible pitch wobble.
        self.rate_ema = self.rate_ema * 0.995 + target_ratio * 0.005;
        // `true` = gradual (ramp) transition to avoid discontinuities.
        let _ = self.resampler.set_resample_ratio(self.rate_ema, true);
    }

    /// Push one interleaved frame (`channels` i16 samples) into the resampler.
    ///
    /// When a full processing block (`chunk_size` frames) is ready, rubato
    /// processes it and the resulting samples become available via `pop_output`.
    fn push_frame(&mut self, frame: &[i16]) {
        debug_assert_eq!(frame.len(), self.channels, "frame must be exactly one frame");
        for (chan, &s) in frame.iter().enumerate() {
            self.input_bufs[chan][self.input_filled] = s as f32 / 32768.0;
        }
        self.input_filled += 1;
        if self.input_filled == self.chunk_size {
            self.input_filled = 0;
            if self
                .resampler
                .process_into_buffer(&self.input_bufs, &mut self.output_bufs, None)
                .is_ok()
            {
                let produced = self.output_bufs[0].len();
                for f in 0..produced {
                    for c in 0..self.channels {
                        self.output_pending.push_back(self.output_bufs[c][f]);
                    }
                }
            }
        }
    }

    /// Pull the next f32 output sample (interleaved), or `None` if not yet available.
    fn pop_output(&mut self) -> Option<f32> {
        self.output_pending.pop_front()
    }
}

impl Player {
    pub fn new(
        fmt: SampleFormat,
        device_name: Option<&str>,
        playback: Option<PlaybackHandle>,
    ) -> Result<Self, SoniumError> {
        let capacity = fmt.rate as usize * fmt.channels as usize * 2; // ~2 s headroom
        let ring = Arc::new(Mutex::new(VecDeque::<i16>::with_capacity(capacity)));
        let health = Arc::new(HealthState::default());
        let max_samples = Arc::new(AtomicUsize::new(capacity));
        let latest_output_latency_us = playback
            .as_ref()
            .map(|p| p.latest_output_latency_us.clone())
            .unwrap_or_else(|| Arc::new(AtomicI64::new(0)));

        // init_tx/rx: audio thread reports success or failure back to Player::new.
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(0);
        // keepalive: while Player holds park_tx, the audio thread keeps running.
        let (park_tx, park_rx) = mpsc::sync_channel::<()>(0);

        let device_owned = device_name.map(String::from);
        let audio_state = AudioState {
            ring: ring.clone(),
            health: health.clone(),
            playback: playback.clone(),
            latest_output_latency_us: latest_output_latency_us.clone(),
        };
        thread::Builder::new()
            .name("sonium-audio".into())
            .spawn(move || audio_thread(audio_state, fmt, device_owned, init_tx, park_rx))
            .map_err(|e| SoniumError::Audio(format!("spawn: {e}")))?;

        init_rx
            .recv()
            .map_err(|_| SoniumError::Audio("audio thread died before init".into()))?
            .map_err(SoniumError::Audio)?;

        info!(
            rate = fmt.rate,
            channels = fmt.channels,
            "Audio output opened"
        );
        Ok(Self {
            ring,
            fmt,
            health,
            max_samples,
            playback,
            latest_output_latency_us,
            _keepalive: park_tx,
        })
    }

    pub fn set_buffer_limit_ms(&mut self, ms: i32) {
        let ms = ms.clamp(80, 10_000) as usize;
        let samples = self.fmt.rate as usize * self.fmt.channels as usize * ms / 1000;
        self.max_samples.store(samples.max(1), Ordering::Relaxed);
    }

    pub fn buffered_us(&self) -> i64 {
        if self.playback.is_some() {
            return self.latest_output_latency_us.load(Ordering::Relaxed);
        }
        let ring = self.ring.lock().unwrap();
        let frames = ring.len() / self.fmt.channels as usize;
        (frames as f64 / self.fmt.rate as f64 * 1_000_000.0) as i64
    }

    /// Push interleaved i16 PCM samples into the ring buffer.
    pub fn write(&mut self, samples: &[i16]) -> Result<(), SoniumError> {
        let mut ring = self.ring.lock().unwrap();
        // Keep the newest scheduled audio if the output callback falls behind.
        let max = self.max_samples.load(Ordering::Relaxed);
        if ring.len() + samples.len() > max {
            let overflow = ring.len() + samples.len() - max;
            for _ in 0..overflow {
                let _ = ring.pop_front();
            }
            warn!(
                drop = overflow,
                "Ring buffer full — dropping oldest samples"
            );
            self.health.overrun_count.fetch_add(1, Ordering::Relaxed);
        }
        ring.extend(samples.iter().copied());
        debug!(buffered = ring.len(), "Player::write");
        Ok(())
    }

    #[allow(dead_code)]
    pub fn sample_format(&self) -> SampleFormat {
        self.fmt
    }

    /// Return and reset health metrics.
    pub fn take_health(&self) -> PlayerHealth {
        PlayerHealth {
            underrun_count: self.health.underrun_count.swap(0, Ordering::Relaxed),
            overrun_count: self.health.overrun_count.swap(0, Ordering::Relaxed),
            callback_starvation_count: self
                .health
                .callback_starvation_count
                .swap(0, Ordering::Relaxed),
            audio_callback_xrun_count: self
                .health
                .audio_callback_xrun_count
                .swap(0, Ordering::Relaxed),
            drift_drop_count: self.health.drift_drop_count.swap(0, Ordering::Relaxed),
            drift_dup_count: self.health.drift_dup_count.swap(0, Ordering::Relaxed),
        }
    }
}

fn audio_thread(
    audio_state: AudioState,
    fmt: SampleFormat,
    device_name: Option<String>,
    init_tx: SyncSender<Result<(), String>>,
    park_rx: Receiver<()>,
) {
    if let Err(e) =
        thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max)
    {
        warn!("Failed to set audio thread priority: {e:?}");
    } else {
        info!("Audio thread priority elevated to Max");
    }

    // ── Initial open ──────────────────────────────────────────────────────
    let stream = match try_open_stream(audio_state.clone(), fmt, device_name.as_deref()) {
        Ok(s) => {
            let _ = init_tx.send(Ok(()));
            s
        }
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    // ── Main loop: park + hotplug recovery ────────────────────────────────
    //
    // We hold `stream` alive.  If the Player is dropped, park_rx.recv()
    // returns Err and we exit.  If we detect the stream died (device
    // disconnected), we enter a retry loop.
    //
    // We use recv_timeout to periodically check for device errors and
    // attempt recovery.
    let mut current_stream = Some(stream);
    let mut backoff = Duration::from_millis(100);
    let max_backoff = Duration::from_secs(5);

    loop {
        match park_rx.recv_timeout(Duration::from_secs(1)) {
            // Player dropped — exit
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("Audio thread: Player dropped — exiting");
                break;
            }
            // Timeout — check health or attempt recovery
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if current_stream.is_none() {
                    // Device was lost — attempt to re-open
                    match try_open_stream(audio_state.clone(), fmt, device_name.as_deref()) {
                        Ok(s) => {
                            info!("Audio device recovered — stream re-opened");
                            current_stream = Some(s);
                            backoff = Duration::from_millis(100);

                            // Flush stale samples that accumulated during downtime
                            if let Ok(mut r) = audio_state.ring.lock() {
                                let stale = r.len();
                                r.clear();
                                if stale > 0 {
                                    info!(flushed = stale, "Cleared stale samples after recovery");
                                }
                            }
                            if let Some(playback) = audio_state.playback.as_ref() {
                                playback.clear();
                            }
                        }
                        Err(e) => {
                            debug!(backoff_ms = backoff.as_millis(), "Device retry failed: {e}");
                            thread::sleep(backoff);
                            backoff = (backoff * 2).min(max_backoff);
                        }
                    }
                }
            }
            // Unexpected message (shouldn't happen but handle gracefully)
            Ok(()) => {}
        }
    }
}

/// Select an output device by name (case-insensitive substring match).
/// Falls back to the default device if no match is found or no name is given.
fn select_device(host: &cpal::Host, requested: Option<&str>) -> Result<cpal::Device, String> {
    if let Some(name) = requested {
        let needle = name.to_lowercase();
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if let Ok(dev_name) = dev.name() {
                    if dev_name.to_lowercase().contains(&needle) {
                        info!(device = %dev_name, requested = %name, "Matched audio device");
                        return Ok(dev);
                    }
                }
            }
        }
        warn!(requested = %name, "No device matched — falling back to default");
    }

    host.default_output_device()
        .ok_or_else(|| "no default audio output device".to_owned())
}

fn try_open_stream(
    audio_state: AudioState,
    fmt: SampleFormat,
    device_name: Option<&str>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = select_device(&host, device_name)?;

    info!(device = %device.name().unwrap_or_default(), "Audio device selected");

    let supported = device
        .default_output_config()
        .map_err(|e| format!("default_output_config: {e}"))?;

    // Keep the local backend cadence short while the network jitter buffer
    // remains the latency authority.
    let fixed_frames = (fmt.rate / 25).max(256);
    let fixed_config = cpal::StreamConfig {
        channels: fmt.channels as cpal::ChannelCount,
        sample_rate: cpal::SampleRate(fmt.rate),
        buffer_size: cpal::BufferSize::Fixed(fixed_frames),
    };
    let default_config = cpal::StreamConfig {
        channels: fmt.channels as cpal::ChannelCount,
        sample_rate: cpal::SampleRate(fmt.rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // ~2 ms fade at the stream's sample rate (per channel).
    let fade_samples = (fmt.rate as usize * fmt.channels as usize * 2) / 1000;
    let stream = build_stream_for_format(
        &device,
        supported.sample_format(),
        &fixed_config,
        audio_state.clone(),
        fade_samples,
    )
    .or_else(|e| {
        warn!(
            buffer_frames = fixed_frames,
            "Fixed audio callback buffer unavailable ({e}); falling back to device default"
        );
        build_stream_for_format(
            &device,
            supported.sample_format(),
            &default_config,
            audio_state.clone(),
            fade_samples,
        )
    })
    .map_err(|e| format!("build_output_stream: {e}"))?;

    stream.play().map_err(|e| format!("stream.play: {e}"))?;
    Ok(stream)
}

fn build_stream_for_format(
    device: &cpal::Device,
    sample_format: cpal::SampleFormat,
    config: &cpal::StreamConfig,
    audio_state: AudioState,
    fade_samples: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    match sample_format {
        cpal::SampleFormat::I16 => {
            let mut fade = FadeState::new(fade_samples);
            let mut timing = CallbackTiming::new(config.channels, config.sample_rate.0);
            let data_health = audio_state.health.clone();
            let err_health = audio_state.health.clone();
            let ring = audio_state.ring.clone();
            let playback = audio_state.playback.clone();
            let latest_output_latency_us = audio_state.latest_output_latency_us.clone();
            device.build_output_stream(
                config,
                move |data: &mut [i16], info: &cpal::OutputCallbackInfo| {
                    timing.observe(data.len(), &data_health);
                    if let Some(playback) = playback.as_ref() {
                        let (drops, dups) = playback.take_drift_metrics();
                        data_health
                            .drift_drop_count
                            .fetch_add(drops, Ordering::Relaxed);
                        data_health
                            .drift_dup_count
                            .fetch_add(dups, Ordering::Relaxed);
                        playback.fill_i16(data, info, &data_health, &mut fade);
                        return;
                    }
                    if let Some(latency_us) = output_latency_us(info) {
                        latest_output_latency_us.store(latency_us, Ordering::Relaxed);
                    }
                    let mut ring = ring.lock().unwrap();
                    for sample in data.iter_mut() {
                        if let Some(s) = ring.pop_front() {
                            *sample = fade.feed(s);
                        } else {
                            if fade.phase == FadePhase::Playing {
                                data_health.underrun_count.fetch_add(1, Ordering::Relaxed);
                            }
                            *sample = fade.drain();
                        }
                    }
                },
                move |err| {
                    err_health
                        .audio_callback_xrun_count
                        .fetch_add(1, Ordering::Relaxed);
                    warn!("CPAL stream error: {err}");
                },
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let mut fade = FadeState::new(fade_samples);
            let mut timing = CallbackTiming::new(config.channels, config.sample_rate.0);
            let data_health = audio_state.health.clone();
            let err_health = audio_state.health.clone();
            let ring = audio_state.ring.clone();
            let playback = audio_state.playback.clone();
            let latest_output_latency_us = audio_state.latest_output_latency_us.clone();
            let mut scratch = Vec::<i16>::new();
            device.build_output_stream(
                config,
                move |data: &mut [u16], info: &cpal::OutputCallbackInfo| {
                    timing.observe(data.len(), &data_health);
                    if let Some(playback) = playback.as_ref() {
                        let (drops, dups) = playback.take_drift_metrics();
                        data_health
                            .drift_drop_count
                            .fetch_add(drops, Ordering::Relaxed);
                        data_health
                            .drift_dup_count
                            .fetch_add(dups, Ordering::Relaxed);
                        scratch.resize(data.len(), 0);
                        playback.fill_i16(&mut scratch, info, &data_health, &mut fade);
                        for (dst, src) in data.iter_mut().zip(scratch.iter()) {
                            *dst = (*src as i32 + 32768) as u16;
                        }
                        return;
                    }
                    if let Some(latency_us) = output_latency_us(info) {
                        latest_output_latency_us.store(latency_us, Ordering::Relaxed);
                    }
                    let mut ring = ring.lock().unwrap();
                    for sample in data.iter_mut() {
                        let s16 = if let Some(s) = ring.pop_front() {
                            fade.feed(s)
                        } else {
                            if fade.phase == FadePhase::Playing {
                                data_health.underrun_count.fetch_add(1, Ordering::Relaxed);
                            }
                            fade.drain()
                        };
                        *sample = (s16 as i32 + 32768) as u16;
                    }
                },
                move |err| {
                    err_health
                        .audio_callback_xrun_count
                        .fetch_add(1, Ordering::Relaxed);
                    warn!("CPAL stream error: {err}");
                },
                None,
            )
        }
        cpal::SampleFormat::F32 => {
            let mut fade = FadeStateF32::new(fade_samples);
            let mut timing = CallbackTiming::new(config.channels, config.sample_rate.0);
            let data_health = audio_state.health.clone();
            let err_health = audio_state.health.clone();
            let ring = audio_state.ring.clone();
            let playback = audio_state.playback.clone();
            let latest_output_latency_us = audio_state.latest_output_latency_us.clone();
            device.build_output_stream(
                config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    timing.observe(data.len(), &data_health);
                    if let Some(playback) = playback.as_ref() {
                        let (drops, dups) = playback.take_drift_metrics();
                        data_health
                            .drift_drop_count
                            .fetch_add(drops, Ordering::Relaxed);
                        data_health
                            .drift_dup_count
                            .fetch_add(dups, Ordering::Relaxed);
                        playback.fill_f32(data, info, &data_health, &mut fade);
                        return;
                    }
                    if let Some(latency_us) = output_latency_us(info) {
                        latest_output_latency_us.store(latency_us, Ordering::Relaxed);
                    }
                    let mut ring = ring.lock().unwrap();
                    for sample in data.iter_mut() {
                        if let Some(s) = ring.pop_front() {
                            *sample = fade.feed(s as f32 / 32768.0);
                        } else {
                            if fade.phase == FadePhase::Playing {
                                data_health.underrun_count.fetch_add(1, Ordering::Relaxed);
                            }
                            *sample = fade.drain();
                        }
                    }
                },
                move |err| {
                    err_health
                        .audio_callback_xrun_count
                        .fetch_add(1, Ordering::Relaxed);
                    warn!("CPAL stream error: {err}");
                },
                None,
            )
        }
        _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
    }
}

fn output_latency_us(info: &cpal::OutputCallbackInfo) -> Option<i64> {
    let ts = info.timestamp();
    ts.playback
        .duration_since(&ts.callback)
        .map(|duration| duration.as_micros().min(i64::MAX as u128) as i64)
}

fn frames_to_us(frames: usize, sample_rate: u32) -> i64 {
    ((frames as u128) * 1_000_000u128 / u128::from(sample_rate.max(1))).min(i64::MAX as u128) as i64
}

fn us_to_frames(us: i64, sample_rate: u32) -> usize {
    if us <= 0 {
        return 0;
    }
    ((us as u128) * u128::from(sample_rate.max(1)) / 1_000_000u128).min(usize::MAX as u128) as usize
}

struct CallbackTiming {
    channels: usize,
    sample_rate: u32,
    last_started: Option<Instant>,
}

impl CallbackTiming {
    fn new(channels: cpal::ChannelCount, sample_rate: u32) -> Self {
        Self {
            channels: usize::from(channels).max(1),
            sample_rate: sample_rate.max(1),
            last_started: None,
        }
    }

    fn observe(&mut self, sample_count: usize, health: &HealthState) {
        let now = Instant::now();
        if let Some(last_started) = self.last_started {
            let elapsed = now.saturating_duration_since(last_started);
            let expected = self.expected_duration(sample_count);
            let threshold = expected
                .checked_mul(3)
                .unwrap_or_else(|| Duration::from_secs(1))
                .max(Duration::from_millis(50));

            if elapsed > threshold {
                health
                    .callback_starvation_count
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        self.last_started = Some(now);
    }

    fn expected_duration(&self, sample_count: usize) -> Duration {
        let frames = sample_count / self.channels;
        let micros = ((frames as u128) * 1_000_000u128 / u128::from(self.sample_rate)).max(1);
        Duration::from_micros(micros.min(u128::from(u64::MAX)) as u64)
    }
}

// ── Crossfade state machine ──────────────────────────────────────────────────

/// Smooth crossfade for underrun recovery.
///
/// States:
/// - **Playing** — data flows, `feed()` returns samples directly.
/// - **FadingOut** — buffer ran dry; `drain()` ramps the last known sample
///   value linearly to zero over `fade_len` samples.
/// - **Silent** — fully faded out; `drain()` returns 0.
/// - **FadingIn** — new data arrived after silence; `feed()` ramps from 0
///   back to full amplitude over `fade_len` samples.
struct FadeState {
    phase: FadePhase,
    fade_len: usize,
    fade_pos: usize,
    /// Last sample value, used as the starting point for fade-out.
    last_val: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadePhase {
    Playing,
    FadingOut,
    Silent,
    FadingIn,
}

impl FadeState {
    fn new(fade_len: usize) -> Self {
        Self {
            phase: FadePhase::Playing,
            fade_len: fade_len.max(1),
            fade_pos: 0,
            last_val: 0,
        }
    }

    /// Called when a real sample is available from the ring buffer.
    #[inline]
    fn feed(&mut self, sample: i16) -> i16 {
        match self.phase {
            FadePhase::Silent | FadePhase::FadingOut => {
                // Data arrived after underrun — fade back in.
                self.phase = FadePhase::FadingIn;
                self.fade_pos = 0;
                self.apply_fade_in(sample)
            }
            FadePhase::FadingIn => self.apply_fade_in(sample),
            FadePhase::Playing => {
                self.last_val = sample;
                sample
            }
        }
    }

    /// Called when the ring buffer is empty (underrun).
    #[inline]
    fn drain(&mut self) -> i16 {
        match self.phase {
            FadePhase::Playing => {
                // Start fading out from the last known value.
                self.phase = FadePhase::FadingOut;
                self.fade_pos = 0;
                self.apply_fade_out()
            }
            FadePhase::FadingOut => self.apply_fade_out(),
            FadePhase::FadingIn | FadePhase::Silent => {
                self.phase = FadePhase::Silent;
                0
            }
        }
    }

    #[inline]
    fn apply_fade_out(&mut self) -> i16 {
        if self.fade_pos >= self.fade_len {
            self.phase = FadePhase::Silent;
            self.last_val = 0;
            return 0;
        }
        let gain = (self.fade_len - self.fade_pos) as f32 / self.fade_len as f32;
        self.fade_pos += 1;
        (self.last_val as f32 * gain) as i16
    }

    #[inline]
    fn apply_fade_in(&mut self, sample: i16) -> i16 {
        if self.fade_pos >= self.fade_len {
            self.phase = FadePhase::Playing;
            self.last_val = sample;
            return sample;
        }
        let gain = self.fade_pos as f32 / self.fade_len as f32;
        self.fade_pos += 1;
        self.last_val = sample;
        (sample as f32 * gain) as i16
    }
}

struct FadeStateF32 {
    phase: FadePhase,
    fade_len: usize,
    fade_pos: usize,
    last_val: f32,
}

impl FadeStateF32 {
    fn new(fade_len: usize) -> Self {
        Self {
            phase: FadePhase::Playing,
            fade_len: fade_len.max(1),
            fade_pos: 0,
            last_val: 0.0,
        }
    }

    #[inline]
    fn feed(&mut self, sample: f32) -> f32 {
        match self.phase {
            FadePhase::Silent | FadePhase::FadingOut => {
                self.phase = FadePhase::FadingIn;
                self.fade_pos = 0;
                self.apply_fade_in(sample)
            }
            FadePhase::FadingIn => self.apply_fade_in(sample),
            FadePhase::Playing => {
                self.last_val = sample;
                sample
            }
        }
    }

    #[inline]
    fn drain(&mut self) -> f32 {
        match self.phase {
            FadePhase::Playing => {
                self.phase = FadePhase::FadingOut;
                self.fade_pos = 0;
                self.apply_fade_out()
            }
            FadePhase::FadingOut => self.apply_fade_out(),
            FadePhase::FadingIn | FadePhase::Silent => {
                self.phase = FadePhase::Silent;
                0.0
            }
        }
    }

    #[inline]
    fn apply_fade_out(&mut self) -> f32 {
        if self.fade_pos >= self.fade_len {
            self.phase = FadePhase::Silent;
            self.last_val = 0.0;
            return 0.0;
        }
        let gain = (self.fade_len - self.fade_pos) as f32 / self.fade_len as f32;
        self.fade_pos += 1;
        self.last_val * gain
    }

    #[inline]
    fn apply_fade_in(&mut self, sample: f32) -> f32 {
        if self.fade_pos >= self.fade_len {
            self.phase = FadePhase::Playing;
            self.last_val = sample;
            return sample;
        }
        let gain = self.fade_pos as f32 / self.fade_len as f32;
        self.fade_pos += 1;
        self.last_val = sample;
        sample * gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_1khz() -> SampleFormat {
        SampleFormat::new(1_000, 16, 1)
    }

    #[test]
    fn callback_timeline_waits_until_chunk_is_due() {
        let fmt = mono_1khz();
        let mut timeline = PlaybackTimeline::new(fmt);
        let health = HealthState::default();
        let mut fade = FadeState::new(1);

        timeline
            .buffer
            .push(PcmChunk::new(10_000, vec![1, 2, 3, 4, 5], fmt), 0);

        let mut early = [99i16; 5];
        timeline.fill_i16(&mut early, 0, &health, &mut fade);

        assert_eq!(early, [0, 0, 0, 0, 0]);
        assert_eq!(timeline.buffer.buffer_depth_us(), 5_000);
        assert_eq!(health.underrun_count.load(Ordering::Relaxed), 1);

        let mut due = [0i16; 5];
        let mut fade = FadeState::new(1);
        timeline.fill_i16(&mut due, 10_000, &health, &mut fade);

        assert_eq!(due, [1, 2, 3, 4, 5]);
        assert!(timeline.current.is_none());
    }

    #[test]
    fn callback_timeline_inserts_partial_silence_before_due_chunk() {
        let fmt = mono_1khz();
        let mut timeline = PlaybackTimeline::new(fmt);
        let health = HealthState::default();
        let mut fade = FadeState::new(1);

        timeline
            .buffer
            .push(PcmChunk::new(3_000, vec![7, 8, 9], fmt), 0);

        let mut out = [0i16; 6];
        timeline.fill_i16(&mut out, 0, &health, &mut fade);

        assert_eq!(out[0..3], [0, 0, 0]);
        assert_ne!(out[3..6], [0, 0, 0]);
        assert!(timeline.current.is_none());
    }
}
