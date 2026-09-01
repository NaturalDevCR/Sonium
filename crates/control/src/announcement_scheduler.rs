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

const MAX_STARTED_SKEW_TOLERANCE_MS: u32 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Lead time between dispatch and synchronized start.
    pub start_lead_ms: u32,
    /// Maximum time to wait for each expected client acknowledgement.
    pub ack_timeout_ms: u32,
    /// Bounded allowance for a client's server-clock estimate to report
    /// `Started` just before the authoritative schedule timestamp.
    pub started_skew_tolerance_ms: u32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            start_lead_ms: 250,
            ack_timeout_ms: 2_000,
            started_skew_tolerance_ms: 50,
        }
    }
}

/// Identity of one admitted client session.  The generation prevents a late
/// ACK from an old connection from advancing the replacement session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnnouncementClient {
    pub client_id: String,
    pub generation: u64,
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
    expected_clients: HashMap<String, ClientAcknowledgements>,
}

#[derive(Debug, Clone)]
struct ClientAcknowledgements {
    generation: u64,
    scheduled: bool,
    started: bool,
    terminal: Option<AnnouncementLifecycle>,
}

impl ClientAcknowledgements {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            scheduled: false,
            started: false,
            terminal: None,
        }
    }
}

pub struct AnnouncementScheduler {
    coordinator: AnnouncementCoordinator,
    config: SchedulerConfig,
    slots: HashMap<String, ScheduledSlot>,
    group_clients: HashMap<String, HashMap<String, u64>>,
}

impl AnnouncementScheduler {
    pub fn new<I, S>(limits: AnnouncementLimits, groups: I, config: SchedulerConfig) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let config = SchedulerConfig {
            started_skew_tolerance_ms: config
                .started_skew_tolerance_ms
                .min(MAX_STARTED_SKEW_TOLERANCE_MS),
            ..config
        };
        let groups: Vec<String> = groups.into_iter().map(Into::into).collect();
        let group_clients = groups
            .iter()
            .map(|group_id| (group_id.clone(), HashMap::new()))
            .collect();
        Self {
            coordinator: AnnouncementCoordinator::new(limits, groups),
            config,
            slots: HashMap::new(),
            group_clients,
        }
    }

    pub fn add_group(&mut self, group_id: impl Into<String>) {
        let group_id = group_id.into();
        self.coordinator.add_group(group_id.clone());
        self.group_clients.entry(group_id).or_default();
    }

    pub fn remove_group(&mut self, group_id: &str, now_ms: i64) -> SchedulerEvents {
        let transitions = self.coordinator.remove_group(group_id);
        let mut events = SchedulerEvents::default();
        self.apply_transitions(transitions, now_ms, &mut events);
        self.slots.remove(group_id);
        self.group_clients.remove(group_id);
        events
    }

    /// Replace the known membership snapshot for a group.  Disconnected
    /// clients remain in this set; reconnects replace only their generation.
    /// If membership changes during an active schedule, the ACK set is
    /// reconciled without carrying acknowledgements across generations.
    pub fn set_group_clients<I>(
        &mut self,
        group_id: &str,
        clients: I,
        now_ms: i64,
    ) -> SchedulerEvents
    where
        I: IntoIterator<Item = AnnouncementClient>,
    {
        let clients: HashMap<String, u64> = clients
            .into_iter()
            .map(|client| (client.client_id, client.generation))
            .collect();
        if self.group_clients.get(group_id) == Some(&clients) {
            return SchedulerEvents::default();
        }
        self.group_clients
            .insert(group_id.to_owned(), clients.clone());

        let mut events = SchedulerEvents::default();
        let Some(slot) = self.slots.get_mut(group_id) else {
            return events;
        };
        let previous = std::mem::take(&mut slot.expected_clients);
        slot.expected_clients = clients
            .into_iter()
            .map(|(client_id, generation)| {
                let acknowledgements = previous
                    .get(&client_id)
                    .filter(|ack| ack.generation == generation)
                    .cloned()
                    .unwrap_or_else(|| ClientAcknowledgements::new(generation));
                (client_id, acknowledgements)
            })
            .collect();
        slot.ack_deadline_ms = slot
            .ack_deadline_ms
            .max(now_ms.saturating_add(i64::from(self.config.ack_timeout_ms)));

        if slot.expected_clients.is_empty() {
            let id = slot.announcement_id.clone();
            if let Ok(transitions) =
                self.coordinator
                    .acknowledge(&id, group_id, AnnouncementLifecycle::Cancelled)
            {
                self.apply_transitions(transitions, now_ms, &mut events);
            }
        } else {
            self.advance_client_acknowledgements(group_id, now_ms, &mut events);
        }
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
            for acknowledgements in slot.expected_clients.values_mut() {
                acknowledgements.scheduled = true;
            }
            slot.ack_deadline_ms = slot
                .scheduled_at_ms
                .saturating_add(i64::from(self.config.ack_timeout_ms));
            return Ok(SchedulerEvents::default());
        }

        if lifecycle == AnnouncementLifecycle::Started {
            let slot = self
                .slots
                .get(group_id)
                .filter(|slot| slot.announcement_id == id)
                .ok_or(AnnouncementError::InvalidLifecycle)?;
            if now_ms
                < slot
                    .scheduled_at_ms
                    .saturating_sub(i64::from(self.config.started_skew_tolerance_ms))
            {
                return Err(AnnouncementError::InvalidLifecycle);
            }
        }

        let transitions = self.coordinator.acknowledge(id, group_id, lifecycle)?;
        if lifecycle == AnnouncementLifecycle::Started && !transitions.is_empty() {
            let slot = self
                .slots
                .get_mut(group_id)
                .expect("started acknowledgement belongs to scheduled slot");
            slot.ack_deadline_ms = slot
                .scheduled_at_ms
                .saturating_add(i64::from(slot.max_duration_ms))
                .saturating_add(i64::from(self.config.ack_timeout_ms));
        }
        let mut events = SchedulerEvents::default();
        self.apply_transitions(transitions, now_ms, &mut events);
        Ok(events)
    }

    /// Record one ACK from the authenticated client session.  Unknown clients
    /// and stale generations are idempotent no-ops: they can never mutate a
    /// different group's lifecycle and do not turn a delayed frame into a
    /// connection-fatal protocol error.
    pub fn acknowledge_client(
        &mut self,
        id: &str,
        group_id: &str,
        client: &AnnouncementClient,
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

        let slot = self
            .slots
            .get_mut(group_id)
            .filter(|slot| slot.announcement_id == id)
            .ok_or(AnnouncementError::InvalidLifecycle)?;
        let Some(acknowledgements) = slot.expected_clients.get_mut(&client.client_id) else {
            return Ok(SchedulerEvents::default());
        };
        if acknowledgements.generation != client.generation {
            return Ok(SchedulerEvents::default());
        }

        if lifecycle == AnnouncementLifecycle::Started
            && now_ms
                < slot
                    .scheduled_at_ms
                    .saturating_sub(i64::from(self.config.started_skew_tolerance_ms))
        {
            return Err(AnnouncementError::InvalidLifecycle);
        }

        match lifecycle {
            AnnouncementLifecycle::Scheduled => acknowledgements.scheduled = true,
            AnnouncementLifecycle::Started => {
                acknowledgements.scheduled = true;
                acknowledgements.started = true;
            }
            AnnouncementLifecycle::Completed
            | AnnouncementLifecycle::Cancelled
            | AnnouncementLifecycle::Failed => {
                acknowledgements.scheduled = true;
                if acknowledgements.terminal.is_none() {
                    acknowledgements.terminal = Some(lifecycle);
                }
            }
        }

        let mut events = SchedulerEvents::default();
        self.advance_client_acknowledgements(group_id, now_ms, &mut events);
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

    fn advance_client_acknowledgements(
        &mut self,
        group_id: &str,
        now_ms: i64,
        events: &mut SchedulerEvents,
    ) {
        let Some(slot) = self.slots.get(group_id) else {
            return;
        };
        if slot.expected_clients.is_empty() {
            return;
        }

        let all_scheduled = slot.expected_clients.values().all(|ack| ack.scheduled);
        let all_started = slot.expected_clients.values().all(|ack| ack.started);
        let all_terminal = slot
            .expected_clients
            .values()
            .all(|ack| ack.terminal.is_some());
        let id = slot.announcement_id.clone();
        let scheduled_at_ms = slot.scheduled_at_ms;
        let max_duration_ms = slot.max_duration_ms;

        if all_terminal {
            let terminal = slot
                .expected_clients
                .values()
                .filter_map(|ack| ack.terminal)
                .fold(
                    AnnouncementLifecycle::Completed,
                    |aggregate, lifecycle| match (aggregate, lifecycle) {
                        (AnnouncementLifecycle::Failed, _) | (_, AnnouncementLifecycle::Failed) => {
                            AnnouncementLifecycle::Failed
                        }
                        (AnnouncementLifecycle::Cancelled, _)
                        | (_, AnnouncementLifecycle::Cancelled) => AnnouncementLifecycle::Cancelled,
                        _ => AnnouncementLifecycle::Completed,
                    },
                );
            if let Ok(transitions) = self.coordinator.acknowledge(&id, group_id, terminal) {
                self.apply_transitions(transitions, now_ms, events);
            }
            return;
        }

        if all_scheduled {
            if let Some(slot) = self.slots.get_mut(group_id) {
                slot.ack_deadline_ms = slot
                    .ack_deadline_ms
                    .max(scheduled_at_ms.saturating_add(i64::from(self.config.ack_timeout_ms)));
            }
        }

        let group_is_scheduled = self.coordinator.record(&id).is_some_and(|record| {
            record.groups.iter().any(|group| {
                group.group_id == group_id && group.lifecycle == AnnouncementLifecycle::Scheduled
            })
        });
        if all_started && group_is_scheduled {
            if let Some(slot) = self.slots.get_mut(group_id) {
                slot.ack_deadline_ms = scheduled_at_ms
                    .saturating_add(i64::from(max_duration_ms))
                    .saturating_add(i64::from(self.config.ack_timeout_ms));
            }
            if let Ok(transitions) =
                self.coordinator
                    .acknowledge(&id, group_id, AnnouncementLifecycle::Started)
            {
                self.apply_transitions(transitions, now_ms, events);
            }
        }
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
            expected_clients: self
                .group_clients
                .get(group_id)
                .into_iter()
                .flat_map(|clients| clients.iter())
                .map(|(client_id, generation)| {
                    (client_id.clone(), ClientAcknowledgements::new(*generation))
                })
                .collect(),
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
