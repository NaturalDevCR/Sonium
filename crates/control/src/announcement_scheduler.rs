//! Deterministic server-side scheduling for bounded announcement intents.
//!
//! Admission/priority ownership stays in [`AnnouncementCoordinator`].  This
//! layer assigns server timestamps only to the active intent of each group,
//! tracks acknowledgement deadlines, and produces Sonium-native control
//! messages for connected clients.  It contains no timers of its own: callers
//! inject `now_ms`, which keeps timeout and recovery behaviour deterministic.

use std::collections::HashMap;

use sonium_protocol::messages::{
    AnnouncementControlV1, AnnouncementDuckingV1, AnnouncementIntentMetadataV1,
    AnnouncementLifecycle as WireLifecycle, AnnouncementPriorityV1, AnnouncementResumeV1,
};

use crate::announcements::{
    AnnouncementAdmission, AnnouncementCoordinator, AnnouncementError, AnnouncementIntent,
    AnnouncementLifecycle, AnnouncementLimits, AnnouncementPriority, AnnouncementRecord,
    AnnouncementSource, AnnouncementTransition, ResumePolicy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Lead time between dispatch and synchronized start.
    pub start_lead_ms: u32,
    /// Maximum time to wait for each expected client acknowledgement.
    pub ack_timeout_ms: u32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            start_lead_ms: 250,
            ack_timeout_ms: 2_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SchedulerEvents {
    pub transitions: Vec<AnnouncementTransition>,
    pub controls: Vec<AnnouncementControlV1>,
}

#[derive(Debug, Clone)]
pub struct SchedulerAdmission {
    pub admission: AnnouncementAdmission,
    pub events: SchedulerEvents,
}

#[derive(Debug, Clone)]
struct ScheduledSlot {
    announcement_id: String,
    scheduled_at_ms: i64,
    max_duration_ms: u32,
    ack_deadline_ms: i64,
    scheduled_acked: bool,
}

pub struct AnnouncementScheduler {
    coordinator: AnnouncementCoordinator,
    config: SchedulerConfig,
    slots: HashMap<String, ScheduledSlot>,
}

impl AnnouncementScheduler {
    pub fn new<I, S>(limits: AnnouncementLimits, groups: I, config: SchedulerConfig) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            coordinator: AnnouncementCoordinator::new(limits, groups),
            config,
            slots: HashMap::new(),
        }
    }

    pub fn add_group(&mut self, group_id: impl Into<String>) {
        self.coordinator.add_group(group_id);
    }

    pub fn remove_group(&mut self, group_id: &str, now_ms: i64) -> SchedulerEvents {
        let transitions = self.coordinator.remove_group(group_id);
        let mut events = SchedulerEvents::default();
        self.apply_transitions(transitions, now_ms, &mut events);
        self.slots.remove(group_id);
        events
    }

    pub fn records(&self) -> Vec<AnnouncementRecord> {
        self.coordinator.records()
    }

    pub fn record(&self, id: &str) -> Option<AnnouncementRecord> {
        self.coordinator.record(id)
    }

    /// Snapshot the current scheduling control for a newly connected session.
    /// Replaying it is safe: client ACKs and coordinator lifecycle changes are
    /// idempotent, while the original absolute timestamp is preserved.
    pub fn pending_control(&self, group_id: &str) -> Option<AnnouncementControlV1> {
        let slot = self.slots.get(group_id)?;
        let record = self.coordinator.record(&slot.announcement_id)?;
        Some(AnnouncementControlV1 {
            version: 1,
            announcement_id: slot.announcement_id.clone(),
            group_id: group_id.into(),
            lifecycle: WireLifecycle::Scheduled,
            scheduled_at_ms: slot.scheduled_at_ms,
            max_duration_ms: slot.max_duration_ms,
            intent: Some(intent_metadata(&record.intent)),
        })
    }

    pub fn admit(
        &mut self,
        intent: AnnouncementIntent,
        now_ms: i64,
    ) -> Result<SchedulerAdmission, AnnouncementError> {
        let mut admission = self.coordinator.admit(intent, now_ms)?;
        let transitions = std::mem::take(&mut admission.transitions);
        let mut events = SchedulerEvents::default();
        self.apply_transitions(transitions, now_ms, &mut events);
        admission.transitions = events.transitions.clone();
        Ok(SchedulerAdmission { admission, events })
    }

    pub fn acknowledge(
        &mut self,
        id: &str,
        group_id: &str,
        lifecycle: AnnouncementLifecycle,
        now_ms: i64,
    ) -> Result<SchedulerEvents, AnnouncementError> {
        let already_terminal = self.coordinator.record(id).is_some_and(|record| {
            record.groups.iter().any(|group| {
                group.group_id == group_id
                    && matches!(
                        group.lifecycle,
                        AnnouncementLifecycle::Completed
                            | AnnouncementLifecycle::Cancelled
                            | AnnouncementLifecycle::Failed
                    )
            })
        });
        if already_terminal {
            return Ok(SchedulerEvents::default());
        }

        if lifecycle == AnnouncementLifecycle::Scheduled {
            let slot = self
                .slots
                .get_mut(group_id)
                .filter(|slot| slot.announcement_id == id)
                .ok_or(AnnouncementError::InvalidLifecycle)?;
            if !slot.scheduled_acked {
                slot.scheduled_acked = true;
                slot.ack_deadline_ms = slot
                    .scheduled_at_ms
                    .saturating_add(i64::from(self.config.ack_timeout_ms));
            }
            return Ok(SchedulerEvents::default());
        }

        if lifecycle == AnnouncementLifecycle::Started {
            let slot = self
                .slots
                .get(group_id)
                .filter(|slot| slot.announcement_id == id)
                .ok_or(AnnouncementError::InvalidLifecycle)?;
            if now_ms < slot.scheduled_at_ms {
                return Err(AnnouncementError::InvalidLifecycle);
            }
        }

        let transitions = self.coordinator.acknowledge(id, group_id, lifecycle)?;
        if lifecycle == AnnouncementLifecycle::Started && !transitions.is_empty() {
            let slot = self
                .slots
                .get_mut(group_id)
                .expect("started acknowledgement belongs to scheduled slot");
            slot.scheduled_acked = true;
            slot.ack_deadline_ms = slot
                .scheduled_at_ms
                .saturating_add(i64::from(slot.max_duration_ms))
                .saturating_add(i64::from(self.config.ack_timeout_ms));
        }
        let mut events = SchedulerEvents::default();
        self.apply_transitions(transitions, now_ms, &mut events);
        Ok(events)
    }

    pub fn cancel(&mut self, id: &str, now_ms: i64) -> Result<SchedulerEvents, AnnouncementError> {
        let transitions = self.coordinator.cancel(id)?;
        let mut events = SchedulerEvents::default();
        self.apply_transitions(transitions, now_ms, &mut events);
        Ok(events)
    }

    /// Expire intents and fail operations whose expected acknowledgement did
    /// not arrive.  Expiry wins over an ACK timeout at the same observation.
    pub fn tick(&mut self, now_ms: i64) -> SchedulerEvents {
        let mut events = SchedulerEvents::default();
        let expired = self.coordinator.expire(now_ms);
        self.apply_transitions(expired, now_ms, &mut events);

        let timed_out: Vec<(String, String)> = self
            .slots
            .iter()
            .filter(|(_, slot)| slot.ack_deadline_ms <= now_ms)
            .map(|(group_id, slot)| (group_id.clone(), slot.announcement_id.clone()))
            .collect();
        for (group_id, id) in timed_out {
            if self
                .slots
                .get(&group_id)
                .is_none_or(|slot| slot.announcement_id != id)
            {
                continue;
            }
            if let Ok(transitions) =
                self.coordinator
                    .acknowledge(&id, &group_id, AnnouncementLifecycle::Failed)
            {
                self.apply_transitions(transitions, now_ms, &mut events);
            }
        }
        events
    }

    fn apply_transitions(
        &mut self,
        transitions: Vec<AnnouncementTransition>,
        now_ms: i64,
        events: &mut SchedulerEvents,
    ) {
        let mut affected_groups = Vec::new();
        for transition in transitions {
            if !affected_groups.contains(&transition.group_id) {
                affected_groups.push(transition.group_id.clone());
            }
            if matches!(
                transition.lifecycle,
                AnnouncementLifecycle::Completed
                    | AnnouncementLifecycle::Cancelled
                    | AnnouncementLifecycle::Failed
            ) {
                if let Some(slot) = self.slots.get(&transition.group_id) {
                    if slot.announcement_id == transition.announcement_id {
                        let slot = self
                            .slots
                            .remove(&transition.group_id)
                            .expect("checked scheduled slot");
                        events.controls.push(AnnouncementControlV1 {
                            version: 1,
                            announcement_id: transition.announcement_id.clone(),
                            group_id: transition.group_id.clone(),
                            lifecycle: wire_lifecycle(transition.lifecycle),
                            scheduled_at_ms: slot.scheduled_at_ms,
                            max_duration_ms: slot.max_duration_ms,
                            intent: None,
                        });
                    }
                }
            }
            events.transitions.push(transition);
        }

        for group_id in affected_groups {
            self.schedule_active(&group_id, now_ms, events);
        }
    }

    fn schedule_active(&mut self, group_id: &str, now_ms: i64, events: &mut SchedulerEvents) {
        if self.slots.contains_key(group_id) {
            return;
        }
        let Some(record) = self.coordinator.active_record(group_id) else {
            return;
        };
        let scheduled_at_ms = now_ms
            .saturating_add(i64::from(self.config.start_lead_ms))
            .min(record.intent.expires_at_ms.saturating_sub(1))
            .max(now_ms);
        let slot = ScheduledSlot {
            announcement_id: record.id.clone(),
            scheduled_at_ms,
            max_duration_ms: record.intent.max_duration_ms,
            ack_deadline_ms: now_ms.saturating_add(i64::from(self.config.ack_timeout_ms)),
            scheduled_acked: false,
        };
        self.slots.insert(group_id.into(), slot);

        if !events.transitions.iter().any(|transition| {
            transition.announcement_id == record.id
                && transition.group_id == group_id
                && transition.lifecycle == AnnouncementLifecycle::Scheduled
        }) {
            events.transitions.push(AnnouncementTransition {
                announcement_id: record.id.clone(),
                group_id: group_id.into(),
                lifecycle: AnnouncementLifecycle::Scheduled,
                resume: false,
            });
        }
        events.controls.push(AnnouncementControlV1 {
            version: 1,
            announcement_id: record.id,
            group_id: group_id.into(),
            lifecycle: WireLifecycle::Scheduled,
            scheduled_at_ms,
            max_duration_ms: record.intent.max_duration_ms,
            intent: Some(intent_metadata(&record.intent)),
        });
    }
}

fn intent_metadata(intent: &AnnouncementIntent) -> AnnouncementIntentMetadataV1 {
    let AnnouncementSource::Uri(source_uri) = &intent.source;
    AnnouncementIntentMetadataV1 {
        source_uri: source_uri.clone(),
        priority: match intent.priority {
            AnnouncementPriority::Music => AnnouncementPriorityV1::Music,
            AnnouncementPriority::Chime => AnnouncementPriorityV1::Chime,
            AnnouncementPriority::Announcement => AnnouncementPriorityV1::Announcement,
            AnnouncementPriority::Emergency => AnnouncementPriorityV1::Emergency,
        },
        duck: AnnouncementDuckingV1 {
            attenuation_db: intent.duck.attenuation_db,
            attack_ms: intent.duck.attack_ms,
            release_ms: intent.duck.release_ms,
        },
        expires_at_ms: intent.expires_at_ms,
        resume: match intent.resume {
            ResumePolicy::ResumePrevious => AnnouncementResumeV1::ResumePrevious,
            ResumePolicy::DoNotResume => AnnouncementResumeV1::DoNotResume,
        },
    }
}

fn wire_lifecycle(lifecycle: AnnouncementLifecycle) -> WireLifecycle {
    match lifecycle {
        AnnouncementLifecycle::Scheduled => WireLifecycle::Scheduled,
        AnnouncementLifecycle::Started => WireLifecycle::Started,
        AnnouncementLifecycle::Completed => WireLifecycle::Completed,
        AnnouncementLifecycle::Cancelled => WireLifecycle::Cancelled,
        AnnouncementLifecycle::Failed => WireLifecycle::Failed,
    }
}
