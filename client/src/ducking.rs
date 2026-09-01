//! Bounded client-side duck envelope.
//!
//! The control/network task owns [`DuckEnvelope`] and advances it with server
//! timestamps.  It publishes the resulting scalar through [`DuckGain`].  The
//! CPAL callback only performs one atomic load and sample multiplication; it
//! never locks or evaluates envelope timing.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use sonium_protocol::messages::{
    AnnouncementControlV1, AnnouncementLifecycle, AnnouncementPriorityV1,
};

#[derive(Clone, Debug)]
pub struct DuckGain {
    bits: Arc<AtomicU32>,
}

impl Default for DuckGain {
    fn default() -> Self {
        Self {
            bits: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        }
    }
}

impl DuckGain {
    #[inline]
    pub fn load(&self) -> f32 {
        f32::from_bits(self.bits.load(Ordering::Relaxed))
    }

    #[inline]
    pub(crate) fn store(&self, gain: f32) {
        self.bits
            .store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy)]
struct Release {
    started_at_ms: i64,
    start_gain: f32,
}

#[derive(Debug, Clone)]
struct ActiveEnvelope {
    control: AnnouncementControlV1,
    priority: AnnouncementPriorityV1,
    target_gain: f32,
    attack_ms: u32,
    release_ms: u32,
    attack_start_gain: f32,
    started: bool,
    release: Option<Release>,
}

impl ActiveEnvelope {
    fn gain_before_release(&self, now_ms: i64) -> f32 {
        if now_ms <= self.control.scheduled_at_ms {
            return self.attack_start_gain;
        }
        if self.attack_ms == 0 {
            return self.target_gain;
        }
        let elapsed = now_ms.saturating_sub(self.control.scheduled_at_ms);
        let progress = (elapsed as f32 / self.attack_ms as f32).clamp(0.0, 1.0);
        lerp(self.attack_start_gain, self.target_gain, progress)
    }

    fn gain_during_release(&self, release: Release, now_ms: i64) -> f32 {
        if self.release_ms == 0 {
            return 1.0;
        }
        let elapsed = now_ms.saturating_sub(release.started_at_ms);
        let progress = (elapsed as f32 / self.release_ms as f32).clamp(0.0, 1.0);
        lerp(release.start_gain, 1.0, progress)
    }
}

/// One active envelope is the complete client-side capacity.  Queueing and
/// equal-priority ordering remain server-owned; an unexpected second control
/// is either a higher-priority preemption or a bounded failed acknowledgement.
pub struct DuckEnvelope {
    gain: DuckGain,
    active: Option<ActiveEnvelope>,
}

impl DuckEnvelope {
    pub fn new(gain: DuckGain) -> Self {
        Self { gain, active: None }
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active
            .as_ref()
            .map(|active| active.control.announcement_id.as_str())
    }

    pub fn handle_control(
        &mut self,
        control: AnnouncementControlV1,
        now_ms: i64,
    ) -> Vec<AnnouncementControlV1> {
        match control.lifecycle {
            AnnouncementLifecycle::Scheduled => self.handle_scheduled(control),
            AnnouncementLifecycle::Completed
            | AnnouncementLifecycle::Cancelled
            | AnnouncementLifecycle::Failed => self.handle_terminal(control, now_ms),
            AnnouncementLifecycle::Started => Vec::new(),
        }
    }

    pub fn tick(&mut self, now_ms: i64) -> Vec<AnnouncementControlV1> {
        let Some(active) = self.active.as_mut() else {
            self.gain.store(1.0);
            return Vec::new();
        };
        let mut acknowledgements = Vec::new();

        if let Some(release) = active.release {
            let gain = active.gain_during_release(release, now_ms);
            self.gain.store(gain);
            if active.release_ms == 0
                || now_ms
                    >= release
                        .started_at_ms
                        .saturating_add(i64::from(active.release_ms))
            {
                self.gain.store(1.0);
                self.active = None;
            }
            return acknowledgements;
        }

        if now_ms < active.control.scheduled_at_ms {
            return acknowledgements;
        }
        if !active.started {
            active.started = true;
            active.attack_start_gain = self.gain.load();
            acknowledgements.push(ack(&active.control, AnnouncementLifecycle::Started));
        }

        let end_at_ms = active
            .control
            .scheduled_at_ms
            .saturating_add(i64::from(active.control.max_duration_ms));
        if now_ms >= end_at_ms {
            let start_gain = active.gain_before_release(end_at_ms);
            let release = Release {
                started_at_ms: end_at_ms,
                start_gain,
            };
            active.release = Some(release);
            self.gain.store(active.gain_during_release(release, now_ms));
            acknowledgements.push(ack(&active.control, AnnouncementLifecycle::Completed));
            if active.release_ms == 0
                || now_ms >= end_at_ms.saturating_add(i64::from(active.release_ms))
            {
                self.gain.store(1.0);
                self.active = None;
            }
            return acknowledgements;
        }

        self.gain.store(active.gain_before_release(now_ms));
        acknowledgements
    }

    fn handle_scheduled(&mut self, control: AnnouncementControlV1) -> Vec<AnnouncementControlV1> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.control.announcement_id == control.announcement_id)
        {
            return vec![ack(&control, AnnouncementLifecycle::Scheduled)];
        }

        let Some(metadata) = control.intent.as_ref() else {
            return vec![ack(&control, AnnouncementLifecycle::Failed)];
        };
        if let Some(active) = self.active.as_ref() {
            if active.release.is_none()
                && priority_rank(metadata.priority) <= priority_rank(active.priority)
            {
                return vec![ack(&control, AnnouncementLifecycle::Failed)];
            }
        }

        let mut acknowledgements = Vec::new();
        if let Some(active) = self.active.take() {
            if active.release.is_none() {
                acknowledgements.push(ack(&active.control, AnnouncementLifecycle::Cancelled));
            }
        }
        let target_gain = 10.0f32.powf(metadata.duck.attenuation_db / 20.0);
        self.active = Some(ActiveEnvelope {
            priority: metadata.priority,
            target_gain,
            attack_ms: metadata.duck.attack_ms,
            release_ms: metadata.duck.release_ms,
            attack_start_gain: self.gain.load(),
            started: false,
            release: None,
            control: control.clone(),
        });
        acknowledgements.push(ack(&control, AnnouncementLifecycle::Scheduled));
        acknowledgements
    }

    fn handle_terminal(
        &mut self,
        control: AnnouncementControlV1,
        now_ms: i64,
    ) -> Vec<AnnouncementControlV1> {
        let Some(active) = self.active.as_mut() else {
            return Vec::new();
        };
        if active.control.announcement_id != control.announcement_id || active.release.is_some() {
            return Vec::new();
        }

        // Release from the scalar the audio callback can actually have
        // observed.  A terminal control may arrive between envelope ticks;
        // recomputing the attack at `now_ms` would publish an instantaneous
        // jump before beginning the release.
        let current_gain = self.gain.load();
        self.gain.store(current_gain);
        active.release = Some(Release {
            started_at_ms: now_ms,
            start_gain: current_gain,
        });
        vec![ack(&active.control, control.lifecycle)]
    }
}

fn ack(control: &AnnouncementControlV1, lifecycle: AnnouncementLifecycle) -> AnnouncementControlV1 {
    let mut acknowledgement = control.clone();
    acknowledgement.lifecycle = lifecycle;
    acknowledgement.intent = None;
    acknowledgement
}

fn priority_rank(priority: AnnouncementPriorityV1) -> u8 {
    match priority {
        AnnouncementPriorityV1::Music => 0,
        AnnouncementPriorityV1::Chime => 1,
        AnnouncementPriorityV1::Announcement => 2,
        AnnouncementPriorityV1::Emergency => 3,
    }
}

fn lerp(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}
