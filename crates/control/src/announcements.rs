//! Bounded, idempotent announcement intent admission and arbitration.
//!
//! This is deliberately control-plane only: it stores metadata and lifecycle
//! transitions, never announcement audio.  The media scheduler consumes the
//! accepted intent in a later layer.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

pub const ANNOUNCEMENT_INTENT_VERSION: u8 = 1;
pub const MAX_ANNOUNCEMENT_SOURCE_BYTES: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementPriority {
    Music,
    Chime,
    Announcement,
    Emergency,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumePolicy {
    ResumePrevious,
    DoNotResume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "uri")]
pub enum AnnouncementSource {
    Uri(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ducking {
    pub attenuation_db: f32,
    pub attack_ms: u32,
    pub release_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnouncementIntent {
    /// Explicit wire/control contract version.  Version 1 is the only
    /// accepted version; incompatible versions are rejected rather than
    /// silently reinterpreted.
    pub version: u8,
    pub idempotency_key: String,
    pub target_groups: Vec<String>,
    pub priority: AnnouncementPriority,
    pub source: AnnouncementSource,
    pub duck: Ducking,
    pub max_duration_ms: u32,
    /// Absolute Unix timestamp. Required so control intents cannot outlive
    /// their caller's scheduling horizon.
    pub expires_at_ms: i64,
    pub resume: ResumePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementLifecycle {
    Scheduled,
    Started,
    Completed,
    Cancelled,
    Failed,
}

impl AnnouncementLifecycle {
    fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncementGroupState {
    pub group_id: String,
    pub lifecycle: AnnouncementLifecycle,
    /// A resume can be requested only once for each group, even if a client
    /// retries its terminal acknowledgement.
    pub resume_emitted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnouncementRecord {
    pub id: String,
    pub intent: AnnouncementIntent,
    pub lifecycle: AnnouncementLifecycle,
    pub groups: Vec<AnnouncementGroupState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncementTransition {
    pub announcement_id: String,
    pub group_id: String,
    pub lifecycle: AnnouncementLifecycle,
    pub resume: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncementAdmission {
    pub id: String,
    pub lifecycle: AnnouncementLifecycle,
    pub duplicate: bool,
}

#[derive(Debug, Clone)]
pub struct AnnouncementLimits {
    pub max_targets_per_intent: usize,
    pub max_queue_depth_per_group: usize,
    pub max_queued_duration_ms_per_group: u64,
    pub max_duration_ms: u32,
    pub max_expiry_ahead_ms: i64,
}

impl Default for AnnouncementLimits {
    fn default() -> Self {
        Self {
            max_targets_per_intent: 32,
            max_queue_depth_per_group: 16,
            max_queued_duration_ms_per_group: 10 * 60 * 1_000,
            max_duration_ms: 120_000,
            max_expiry_ahead_ms: 24 * 60 * 60 * 1_000,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AnnouncementError {
    #[error("unsupported announcement intent version {0}")]
    UnsupportedVersion(u8),
    #[error("idempotency key must contain 1..=128 bytes")]
    InvalidIdempotencyKey,
    #[error("target group list must contain 1..={0} groups")]
    InvalidTargetCount(usize),
    #[error("target group {0:?} is unknown or duplicated")]
    InvalidTargetGroup(String),
    #[error("announcement source must be a bounded http(s) or media URI")]
    InvalidSource,
    #[error("duck attenuation must be in -60..=0 dB and ramps must be <= 5000 ms")]
    InvalidDuck,
    #[error("maximum duration must be in 1..={0} ms")]
    InvalidDuration(u32),
    #[error("announcement expiry is invalid or too far in the future")]
    InvalidExpiry,
    #[error("announcement queue depth for group {0:?} is exhausted")]
    QueueDepthExceeded(String),
    #[error("announcement duration budget for group {0:?} is exhausted")]
    QueueDurationExceeded(String),
    #[error("idempotency key was reused with a different intent")]
    IdempotencyConflict,
    #[error("announcement {0:?} was not found")]
    NotFound(String),
    #[error("invalid lifecycle transition")]
    InvalidLifecycle,
}

#[derive(Debug, Default)]
struct GroupQueue {
    active: Option<String>,
    queued: VecDeque<String>,
    queued_duration_ms: u64,
}

/// In-memory admission state with strict per-group limits.
pub struct AnnouncementCoordinator {
    limits: AnnouncementLimits,
    known_groups: HashSet<String>,
    by_key: HashMap<String, String>,
    records: HashMap<String, AnnouncementRecord>,
    queues: HashMap<String, GroupQueue>,
}

impl AnnouncementCoordinator {
    pub fn new<I, S>(limits: AnnouncementLimits, groups: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let known_groups: HashSet<String> = groups.into_iter().map(Into::into).collect();
        let queues = known_groups
            .iter()
            .map(|group| (group.clone(), GroupQueue::default()))
            .collect();
        Self {
            limits,
            known_groups,
            by_key: HashMap::new(),
            records: HashMap::new(),
            queues,
        }
    }

    pub fn add_group(&mut self, group_id: impl Into<String>) {
        let group_id = group_id.into();
        self.known_groups.insert(group_id.clone());
        self.queues.entry(group_id).or_default();
    }

    pub fn remove_group(&mut self, group_id: &str) {
        self.known_groups.remove(group_id);
        self.queues.remove(group_id);
    }

    pub fn records(&self) -> Vec<AnnouncementRecord> {
        let mut records: Vec<_> = self.records.values().cloned().collect();
        records.sort_by(|a, b| a.id.cmp(&b.id));
        records
    }

    pub fn record(&self, id: &str) -> Option<AnnouncementRecord> {
        self.records.get(id).cloned()
    }

    pub fn admit(
        &mut self,
        intent: AnnouncementIntent,
        now_ms: i64,
    ) -> Result<AnnouncementAdmission, AnnouncementError> {
        if let Some(id) = self.by_key.get(&intent.idempotency_key) {
            let existing = self
                .records
                .get(id)
                .expect("idempotency index is consistent");
            if existing.intent == intent {
                return Ok(AnnouncementAdmission {
                    id: id.clone(),
                    lifecycle: existing.lifecycle,
                    duplicate: true,
                });
            }
            return Err(AnnouncementError::IdempotencyConflict);
        }
        self.validate(&intent, now_ms)?;

        // Admission is atomic: inspect every target queue before changing any.
        for group_id in &intent.target_groups {
            let queue = self.queues.get(group_id).expect("validated known group");
            let interrupts = queue
                .active
                .as_ref()
                .is_some_and(|active_id| self.records[active_id].intent.priority < intent.priority);
            if !interrupts {
                if queue.queued.len() >= self.limits.max_queue_depth_per_group {
                    return Err(AnnouncementError::QueueDepthExceeded(group_id.clone()));
                }
                if queue.queued_duration_ms + u64::from(intent.max_duration_ms)
                    > self.limits.max_queued_duration_ms_per_group
                {
                    return Err(AnnouncementError::QueueDurationExceeded(group_id.clone()));
                }
            }
        }

        let id = uuid::Uuid::new_v4().to_string();
        let record = AnnouncementRecord {
            id: id.clone(),
            intent: intent.clone(),
            lifecycle: AnnouncementLifecycle::Scheduled,
            groups: intent
                .target_groups
                .iter()
                .map(|group_id| AnnouncementGroupState {
                    group_id: group_id.clone(),
                    lifecycle: AnnouncementLifecycle::Scheduled,
                    resume_emitted: false,
                })
                .collect(),
        };
        self.by_key
            .insert(intent.idempotency_key.clone(), id.clone());
        self.records.insert(id.clone(), record);

        for group_id in &intent.target_groups {
            let active = self.queues[group_id].active.clone();
            let interrupts = active
                .as_ref()
                .is_some_and(|active_id| self.records[active_id].intent.priority < intent.priority);
            if let Some(active_id) = active.filter(|_| interrupts) {
                self.finish_group(&active_id, group_id, AnnouncementLifecycle::Cancelled);
                self.queues.get_mut(group_id).expect("known group").active = Some(id.clone());
            } else if self.queues[group_id].active.is_none() {
                self.queues.get_mut(group_id).expect("known group").active = Some(id.clone());
            } else {
                let queue = self.queues.get_mut(group_id).expect("known group");
                queue.queued.push_back(id.clone());
                queue.queued_duration_ms += u64::from(intent.max_duration_ms);
            }
        }
        Ok(AnnouncementAdmission {
            id,
            lifecycle: AnnouncementLifecycle::Scheduled,
            duplicate: false,
        })
    }

    pub fn acknowledge(
        &mut self,
        id: &str,
        group_id: &str,
        lifecycle: AnnouncementLifecycle,
    ) -> Result<Vec<AnnouncementTransition>, AnnouncementError> {
        if lifecycle == AnnouncementLifecycle::Scheduled {
            return Err(AnnouncementError::InvalidLifecycle);
        }
        let record = self
            .records
            .get(id)
            .ok_or_else(|| AnnouncementError::NotFound(id.into()))?;
        if !record.groups.iter().any(|group| group.group_id == group_id) {
            return Err(AnnouncementError::InvalidTargetGroup(group_id.into()));
        }
        if lifecycle == AnnouncementLifecycle::Started {
            let queue = self.queues.get(group_id).expect("record target is known");
            if queue.active.as_deref() != Some(id) {
                return Err(AnnouncementError::InvalidLifecycle);
            }
            let state = self.group_state_mut(id, group_id)?;
            if state.lifecycle == AnnouncementLifecycle::Scheduled {
                state.lifecycle = AnnouncementLifecycle::Started;
                self.refresh_lifecycle(id);
                return Ok(vec![AnnouncementTransition {
                    announcement_id: id.into(),
                    group_id: group_id.into(),
                    lifecycle,
                    resume: false,
                }]);
            }
            return Ok(vec![]);
        }
        if !lifecycle.terminal() {
            return Err(AnnouncementError::InvalidLifecycle);
        }
        if self.group_state(id, group_id)?.lifecycle.terminal() {
            return Ok(vec![]);
        }
        let transition = self.finish_group(id, group_id, lifecycle);
        self.advance_group(group_id);
        Ok(vec![transition])
    }

    pub fn cancel(&mut self, id: &str) -> Result<Vec<AnnouncementTransition>, AnnouncementError> {
        let targets: Vec<String> = self
            .records
            .get(id)
            .ok_or_else(|| AnnouncementError::NotFound(id.into()))?
            .groups
            .iter()
            .filter(|state| !state.lifecycle.terminal())
            .map(|state| state.group_id.clone())
            .collect();
        let mut transitions = Vec::new();
        for group_id in targets {
            if let Some(position) = self.queues[&group_id]
                .queued
                .iter()
                .position(|queued| queued == id)
            {
                self.queues
                    .get_mut(&group_id)
                    .unwrap()
                    .queued
                    .remove(position);
                self.queues.get_mut(&group_id).unwrap().queued_duration_ms = self.queues[&group_id]
                    .queued
                    .iter()
                    .map(|queued| u64::from(self.records[queued].intent.max_duration_ms))
                    .sum();
            }
            let transition = self.finish_group(id, &group_id, AnnouncementLifecycle::Cancelled);
            if self.queues[&group_id].active.is_none() {
                self.advance_group(&group_id);
            }
            transitions.push(transition);
        }
        Ok(transitions)
    }

    pub fn expire(&mut self, now_ms: i64) -> Vec<AnnouncementTransition> {
        let expired: Vec<String> = self
            .records
            .values()
            .filter(|record| {
                record.intent.expires_at_ms <= now_ms
                    && record
                        .groups
                        .iter()
                        .any(|group| !group.lifecycle.terminal())
            })
            .map(|record| record.id.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|id| self.cancel(&id).ok())
            .flatten()
            .collect()
    }

    fn validate(&self, intent: &AnnouncementIntent, now_ms: i64) -> Result<(), AnnouncementError> {
        if intent.version != ANNOUNCEMENT_INTENT_VERSION {
            return Err(AnnouncementError::UnsupportedVersion(intent.version));
        }
        if intent.idempotency_key.is_empty() || intent.idempotency_key.len() > 128 {
            return Err(AnnouncementError::InvalidIdempotencyKey);
        }
        if intent.target_groups.is_empty()
            || intent.target_groups.len() > self.limits.max_targets_per_intent
        {
            return Err(AnnouncementError::InvalidTargetCount(
                self.limits.max_targets_per_intent,
            ));
        }
        let mut targets = HashSet::new();
        for group in &intent.target_groups {
            if !self.known_groups.contains(group) || !targets.insert(group) {
                return Err(AnnouncementError::InvalidTargetGroup(group.clone()));
            }
        }
        let AnnouncementSource::Uri(uri) = &intent.source;
        if uri.is_empty()
            || uri.len() > MAX_ANNOUNCEMENT_SOURCE_BYTES
            || !(uri.starts_with("https://")
                || uri.starts_with("http://")
                || uri.starts_with("media://"))
        {
            return Err(AnnouncementError::InvalidSource);
        }
        if !(-60.0..=0.0).contains(&intent.duck.attenuation_db)
            || !intent.duck.attenuation_db.is_finite()
            || intent.duck.attack_ms > 5_000
            || intent.duck.release_ms > 5_000
        {
            return Err(AnnouncementError::InvalidDuck);
        }
        if intent.max_duration_ms == 0 || intent.max_duration_ms > self.limits.max_duration_ms {
            return Err(AnnouncementError::InvalidDuration(
                self.limits.max_duration_ms,
            ));
        }
        if intent.expires_at_ms <= now_ms
            || intent.expires_at_ms > now_ms + self.limits.max_expiry_ahead_ms
        {
            return Err(AnnouncementError::InvalidExpiry);
        }
        Ok(())
    }

    fn group_state(
        &self,
        id: &str,
        group_id: &str,
    ) -> Result<&AnnouncementGroupState, AnnouncementError> {
        self.records
            .get(id)
            .and_then(|record| {
                record
                    .groups
                    .iter()
                    .find(|group| group.group_id == group_id)
            })
            .ok_or_else(|| AnnouncementError::NotFound(id.into()))
    }

    fn group_state_mut(
        &mut self,
        id: &str,
        group_id: &str,
    ) -> Result<&mut AnnouncementGroupState, AnnouncementError> {
        self.records
            .get_mut(id)
            .and_then(|record| {
                record
                    .groups
                    .iter_mut()
                    .find(|group| group.group_id == group_id)
            })
            .ok_or_else(|| AnnouncementError::NotFound(id.into()))
    }

    fn finish_group(
        &mut self,
        id: &str,
        group_id: &str,
        lifecycle: AnnouncementLifecycle,
    ) -> AnnouncementTransition {
        let was_active = self.queues[group_id].active.as_deref() == Some(id);
        let resume = {
            let record = self.records.get_mut(id).expect("known announcement");
            let state = record
                .groups
                .iter_mut()
                .find(|state| state.group_id == group_id)
                .expect("known target");
            state.lifecycle = lifecycle;
            let resume = was_active
                && record.intent.resume == ResumePolicy::ResumePrevious
                && !state.resume_emitted;
            state.resume_emitted |= resume;
            resume
        };
        if was_active {
            self.queues.get_mut(group_id).unwrap().active = None;
        }
        self.refresh_lifecycle(id);
        AnnouncementTransition {
            announcement_id: id.into(),
            group_id: group_id.into(),
            lifecycle,
            resume,
        }
    }

    fn advance_group(&mut self, group_id: &str) {
        let next = self.queues.get_mut(group_id).unwrap().queued.pop_front();
        if let Some(id) = next {
            let duration = u64::from(self.records[&id].intent.max_duration_ms);
            let queue = self.queues.get_mut(group_id).unwrap();
            queue.queued_duration_ms = queue.queued_duration_ms.saturating_sub(duration);
            queue.active = Some(id);
        }
    }

    fn refresh_lifecycle(&mut self, id: &str) {
        let record = self.records.get_mut(id).expect("known announcement");
        record.lifecycle = if record
            .groups
            .iter()
            .any(|group| group.lifecycle == AnnouncementLifecycle::Started)
        {
            AnnouncementLifecycle::Started
        } else if record.groups.iter().all(|group| group.lifecycle.terminal()) {
            record
                .groups
                .iter()
                .find(|group| group.lifecycle == AnnouncementLifecycle::Failed)
                .map(|_| AnnouncementLifecycle::Failed)
                .or_else(|| {
                    record
                        .groups
                        .iter()
                        .find(|group| group.lifecycle == AnnouncementLifecycle::Cancelled)
                        .map(|_| AnnouncementLifecycle::Cancelled)
                })
                .unwrap_or(AnnouncementLifecycle::Completed)
        } else {
            AnnouncementLifecycle::Scheduled
        };
    }
}
