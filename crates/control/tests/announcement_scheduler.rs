use sonium_control::announcement_scheduler::{AnnouncementScheduler, SchedulerConfig};
use sonium_control::announcements::{
    AnnouncementIntent, AnnouncementLifecycle, AnnouncementLimits, AnnouncementPriority,
    AnnouncementSource, Ducking, ResumePolicy, ANNOUNCEMENT_INTENT_VERSION,
};
use sonium_protocol::messages::{
    AnnouncementLifecycle as WireLifecycle, AnnouncementPriorityV1, AnnouncementResumeV1,
};

fn intent(key: &str, priority: AnnouncementPriority, expires_at_ms: i64) -> AnnouncementIntent {
    AnnouncementIntent {
        version: ANNOUNCEMENT_INTENT_VERSION,
        idempotency_key: key.into(),
        target_groups: vec!["default".into()],
        priority,
        source: AnnouncementSource::Uri("https://media.example.test/doorbell.ogg".into()),
        duck: Ducking {
            attenuation_db: -18.0,
            attack_ms: 25,
            release_ms: 100,
        },
        max_duration_ms: 1_000,
        expires_at_ms,
        resume: ResumePolicy::ResumePrevious,
    }
}

fn scheduler() -> AnnouncementScheduler {
    AnnouncementScheduler::new(
        AnnouncementLimits::default(),
        ["default"],
        SchedulerConfig {
            start_lead_ms: 250,
            ack_timeout_ms: 100,
        },
    )
}

#[test]
fn active_intent_is_timestamped_with_bounded_wire_metadata() {
    let mut scheduler = scheduler();

    let scheduled = scheduler
        .admit(
            intent("doorbell", AnnouncementPriority::Announcement, 20_000),
            10_000,
        )
        .unwrap();

    assert_eq!(scheduled.events.controls.len(), 1);
    let control = &scheduled.events.controls[0];
    assert_eq!(control.announcement_id, scheduled.admission.id);
    assert_eq!(control.lifecycle, WireLifecycle::Scheduled);
    assert_eq!(control.scheduled_at_ms, 10_250);
    assert_eq!(control.max_duration_ms, 1_000);
    let metadata = control.intent.as_ref().unwrap();
    assert_eq!(
        metadata.source_uri,
        "https://media.example.test/doorbell.ogg"
    );
    assert_eq!(metadata.priority, AnnouncementPriorityV1::Announcement);
    assert_eq!(metadata.duck.attenuation_db, -18.0);
    assert_eq!(metadata.duck.attack_ms, 25);
    assert_eq!(metadata.duck.release_ms, 100);
    assert_eq!(metadata.expires_at_ms, 20_000);
    assert_eq!(metadata.resume, AnnouncementResumeV1::ResumePrevious);
}

#[test]
fn dropped_ack_fails_active_and_schedules_queued_intent_from_timeout_time() {
    let mut scheduler = scheduler();
    let first = scheduler
        .admit(
            intent("first", AnnouncementPriority::Announcement, 20_000),
            10_000,
        )
        .unwrap();
    let second = scheduler
        .admit(
            intent("second", AnnouncementPriority::Announcement, 20_000),
            10_010,
        )
        .unwrap();
    assert!(
        second.events.controls.is_empty(),
        "queued intent must not play early"
    );

    assert!(scheduler.tick(10_099).controls.is_empty());
    let timeout = scheduler.tick(10_100);

    assert_eq!(timeout.transitions.len(), 2);
    assert_eq!(timeout.transitions[0].announcement_id, first.admission.id);
    assert_eq!(
        timeout.transitions[0].lifecycle,
        AnnouncementLifecycle::Failed
    );
    assert!(timeout.transitions[0].resume);
    assert_eq!(timeout.transitions[1].announcement_id, second.admission.id);
    assert_eq!(
        timeout.transitions[1].lifecycle,
        AnnouncementLifecycle::Scheduled
    );
    assert_eq!(timeout.controls.len(), 2);
    assert_eq!(timeout.controls[0].lifecycle, WireLifecycle::Failed);
    assert_eq!(timeout.controls[1].lifecycle, WireLifecycle::Scheduled);
    assert_eq!(timeout.controls[1].scheduled_at_ms, 10_350);

    assert!(scheduler.tick(10_101).transitions.is_empty());
    let late_started = scheduler
        .acknowledge(
            &first.admission.id,
            "default",
            AnnouncementLifecycle::Started,
            10_101,
        )
        .unwrap();
    assert!(late_started.transitions.is_empty());
    assert!(late_started.controls.is_empty());
}

#[test]
fn higher_priority_interrupts_once_and_late_terminal_ack_is_idempotent() {
    let mut scheduler = scheduler();
    let low = scheduler
        .admit(intent("chime", AnnouncementPriority::Chime, 20_000), 10_000)
        .unwrap();

    let high = scheduler
        .admit(
            intent("emergency", AnnouncementPriority::Emergency, 20_000),
            10_020,
        )
        .unwrap();

    assert_eq!(high.events.controls.len(), 2);
    assert_eq!(high.events.controls[0].announcement_id, low.admission.id);
    assert_eq!(high.events.controls[0].lifecycle, WireLifecycle::Cancelled);
    assert_eq!(high.events.controls[1].announcement_id, high.admission.id);
    assert_eq!(high.events.controls[1].lifecycle, WireLifecycle::Scheduled);
    assert_eq!(high.events.controls[1].scheduled_at_ms, 10_270);
    assert!(high.events.transitions[0].resume);

    let late = scheduler
        .acknowledge(
            &low.admission.id,
            "default",
            AnnouncementLifecycle::Cancelled,
            10_030,
        )
        .unwrap();
    assert!(late.transitions.is_empty());
    assert!(late.controls.is_empty());
}

#[test]
fn explicit_cancel_and_expiry_restore_active_program_exactly_once() {
    let mut scheduler = scheduler();
    let cancelled = scheduler
        .admit(
            intent("cancelled", AnnouncementPriority::Announcement, 20_000),
            10_000,
        )
        .unwrap();
    let first_cancel = scheduler.cancel(&cancelled.admission.id, 10_010).unwrap();
    assert_eq!(first_cancel.transitions.len(), 1);
    assert!(first_cancel.transitions[0].resume);
    assert_eq!(first_cancel.controls[0].lifecycle, WireLifecycle::Cancelled);
    assert!(scheduler
        .cancel(&cancelled.admission.id, 10_020)
        .unwrap()
        .transitions
        .is_empty());

    let expiring = scheduler
        .admit(
            intent("offline", AnnouncementPriority::Announcement, 11_000),
            10_100,
        )
        .unwrap();
    let expired = scheduler.tick(11_000);
    assert_eq!(expired.transitions.len(), 1);
    assert_eq!(
        expired.transitions[0].announcement_id,
        expiring.admission.id
    );
    assert_eq!(
        expired.transitions[0].lifecycle,
        AnnouncementLifecycle::Cancelled
    );
    assert!(expired.transitions[0].resume);
    assert_eq!(expired.controls[0].lifecycle, WireLifecycle::Cancelled);
    assert!(scheduler.tick(11_001).transitions.is_empty());
}

#[test]
fn scheduled_started_and_completed_acks_advance_deadlines_deterministically() {
    let mut scheduler = scheduler();
    let admission = scheduler
        .admit(
            intent("acked", AnnouncementPriority::Announcement, 20_000),
            10_000,
        )
        .unwrap();
    let id = admission.admission.id;

    assert!(scheduler
        .acknowledge(&id, "default", AnnouncementLifecycle::Scheduled, 10_010)
        .unwrap()
        .transitions
        .is_empty());
    assert!(scheduler.tick(10_249).transitions.is_empty());
    let started = scheduler
        .acknowledge(&id, "default", AnnouncementLifecycle::Started, 10_250)
        .unwrap();
    assert_eq!(
        started.transitions[0].lifecycle,
        AnnouncementLifecycle::Started
    );
    assert!(scheduler.tick(11_349).transitions.is_empty());

    let completed = scheduler
        .acknowledge(&id, "default", AnnouncementLifecycle::Completed, 11_250)
        .unwrap();
    assert_eq!(
        completed.transitions[0].lifecycle,
        AnnouncementLifecycle::Completed
    );
    assert!(completed.transitions[0].resume);
    assert_eq!(completed.controls[0].lifecycle, WireLifecycle::Completed);
    assert!(scheduler.tick(11_350).transitions.is_empty());
}

#[test]
fn server_state_fans_controls_to_media_sessions_and_ticks_without_rest_polling() {
    use std::sync::Arc;

    use sonium_control::{EventBus, ServerState};

    let state = ServerState::new(Arc::new(EventBus::new()), None, vec![], vec![]);
    let mut controls = state.subscribe_announcement_controls();
    let admitted = state
        .admit_announcement(
            intent("state-offline", AnnouncementPriority::Announcement, 20_000),
            10_000,
        )
        .unwrap();

    let scheduled = controls.try_recv().unwrap();
    assert_eq!(scheduled.announcement_id, admitted.id);
    assert_eq!(scheduled.lifecycle, WireLifecycle::Scheduled);
    assert_eq!(scheduled.scheduled_at_ms, 10_250);

    state.expire_announcements(12_000);
    let failed = controls.try_recv().unwrap();
    assert_eq!(failed.announcement_id, admitted.id);
    assert_eq!(failed.lifecycle, WireLifecycle::Failed);
}

#[test]
fn pending_control_can_be_replayed_to_a_client_that_connects_during_schedule() {
    let mut scheduler = scheduler();
    let admission = scheduler
        .admit(
            intent("reconnect", AnnouncementPriority::Announcement, 20_000),
            10_000,
        )
        .unwrap();

    let pending = scheduler.pending_control("default").unwrap();
    assert_eq!(pending.announcement_id, admission.admission.id);
    assert_eq!(pending.lifecycle, WireLifecycle::Scheduled);
    assert_eq!(pending.scheduled_at_ms, 10_250);

    scheduler.tick(10_100);
    assert_eq!(scheduler.pending_control("default"), None);
}
