use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tracing::{debug, info, warn};

use sonium_common::{SampleFormat, SoniumError};

/// Audio output backed by the system default device via CPAL.
///
/// The CPAL stream lives on a dedicated OS thread to avoid `!Send` issues on
/// macOS (CoreAudio streams must be created and used on the same thread).
/// PCM samples are exchanged through a shared lock-free ring buffer.
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
}

pub struct Player {
    ring: Arc<Mutex<VecDeque<i16>>>,
    fmt: SampleFormat,
    health: Arc<HealthState>,
    /// Dropping this disconnects the audio thread's park channel, stopping it.
    _keepalive: SyncSender<()>,
}

impl Player {
    pub fn new(fmt: SampleFormat, device_name: Option<&str>) -> Result<Self, SoniumError> {
        let capacity = fmt.rate as usize * fmt.channels as usize * 2; // ~2 s headroom
        let ring = Arc::new(Mutex::new(VecDeque::<i16>::with_capacity(capacity)));
        let health = Arc::new(HealthState::default());

        // init_tx/rx: audio thread reports success or failure back to Player::new.
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(0);
        // keepalive: while Player holds park_tx, the audio thread keeps running.
        let (park_tx, park_rx) = mpsc::sync_channel::<()>(0);

        let ring_clone = ring.clone();
        let health_clone = health.clone();
        let device_owned = device_name.map(String::from);
        thread::Builder::new()
            .name("sonium-audio".into())
            .spawn(move || {
                audio_thread(
                    ring_clone,
                    health_clone,
                    fmt,
                    device_owned,
                    init_tx,
                    park_rx,
                )
            })
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
            _keepalive: park_tx,
        })
    }

    /// Push interleaved i16 PCM samples into the ring buffer.
    pub fn write(&mut self, samples: &[i16]) -> Result<(), SoniumError> {
        let mut ring = self.ring.lock().unwrap();
        // Guard against unbounded growth (> 4 s): drop the incoming chunk.
        let max = self.fmt.rate as usize * self.fmt.channels as usize * 4;
        if ring.len() + samples.len() > max {
            warn!(drop = samples.len(), "Ring buffer full — dropping chunk");
            self.health.overrun_count.fetch_add(1, Ordering::Relaxed);
            return Ok(());
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
    pub fn take_health(&self) -> (u32, u32) {
        let underruns = self.health.underrun_count.swap(0, Ordering::Relaxed);
        let overruns = self.health.overrun_count.swap(0, Ordering::Relaxed);
        (underruns, overruns)
    }
}

fn audio_thread(
    ring: Arc<Mutex<VecDeque<i16>>>,
    health: Arc<HealthState>,
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
    let stream = match try_open_stream(ring.clone(), health.clone(), fmt, device_name.as_deref()) {
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
                    match try_open_stream(ring.clone(), health.clone(), fmt, device_name.as_deref())
                    {
                        Ok(s) => {
                            info!("Audio device recovered — stream re-opened");
                            current_stream = Some(s);
                            backoff = Duration::from_millis(100);

                            // Flush stale samples that accumulated during downtime
                            if let Ok(mut r) = ring.lock() {
                                let stale = r.len();
                                r.clear();
                                if stale > 0 {
                                    info!(flushed = stale, "Cleared stale samples after recovery");
                                }
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
    ring: Arc<Mutex<VecDeque<i16>>>,
    health: Arc<HealthState>,
    fmt: SampleFormat,
    device_name: Option<&str>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = select_device(&host, device_name)?;

    info!(device = %device.name().unwrap_or_default(), "Audio device selected");

    let supported = device
        .default_output_config()
        .map_err(|e| format!("default_output_config: {e}"))?;

    let config = cpal::StreamConfig {
        channels: fmt.channels as cpal::ChannelCount,
        sample_rate: cpal::SampleRate(fmt.rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // ~2 ms fade at the stream's sample rate (per channel).
    let fade_samples = (fmt.rate as usize * fmt.channels as usize * 2) / 1000;
    let err_fn = |err| warn!("CPAL stream error: {err}");

    let stream = match supported.sample_format() {
        cpal::SampleFormat::I16 => {
            let mut fade = FadeState::new(fade_samples);
            let ring = ring.clone();
            let health = health.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut ring = ring.lock().unwrap();
                    for sample in data.iter_mut() {
                        if let Some(s) = ring.pop_front() {
                            *sample = fade.feed(s);
                        } else {
                            if fade.phase == FadePhase::Playing {
                                health.underrun_count.fetch_add(1, Ordering::Relaxed);
                            }
                            *sample = fade.drain();
                        }
                    }
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let mut fade = FadeState::new(fade_samples);
            let ring = ring.clone();
            let health = health.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let mut ring = ring.lock().unwrap();
                    for sample in data.iter_mut() {
                        let s16 = if let Some(s) = ring.pop_front() {
                            fade.feed(s)
                        } else {
                            if fade.phase == FadePhase::Playing {
                                health.underrun_count.fetch_add(1, Ordering::Relaxed);
                            }
                            fade.drain()
                        };
                        *sample = (s16 as i32 + 32768) as u16;
                    }
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::F32 => {
            let mut fade = FadeState::new(fade_samples);
            let ring = ring.clone();
            let health = health.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut ring = ring.lock().unwrap();
                    for sample in data.iter_mut() {
                        let s16 = if let Some(s) = ring.pop_front() {
                            fade.feed(s)
                        } else {
                            if fade.phase == FadePhase::Playing {
                                health.underrun_count.fetch_add(1, Ordering::Relaxed);
                            }
                            fade.drain()
                        };
                        *sample = s16 as f32 / 32768.0;
                    }
                },
                err_fn,
                None,
            )
        }
        fmt => return Err(format!("unsupported sample format: {fmt:?}")),
    }
    .map_err(|e| format!("build_output_stream: {e}"))?;

    stream.play().map_err(|e| format!("stream.play: {e}"))?;
    Ok(stream)
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
