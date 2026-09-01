use sonium_client_lib::ducking::{DuckEnvelope, DuckGain};
use sonium_protocol::messages::{
    AnnouncementControlV1, AnnouncementDuckingV1, AnnouncementIntentMetadataV1,
    AnnouncementLifecycle, AnnouncementPriorityV1, AnnouncementResumeV1,
};

fn scheduled(
    id: &str,
    priority: AnnouncementPriorityV1,
    scheduled_at_ms: i64,
) -> AnnouncementControlV1 {
    AnnouncementControlV1 {
        version: 1,
        announcement_id: id.into(),
        group_id: "default".into(),
        lifecycle: AnnouncementLifecycle::Scheduled,
        scheduled_at_ms,
        max_duration_ms: 1_000,
        intent: Some(AnnouncementIntentMetadataV1 {
            source_uri: "https://media.example.test/a.ogg".into(),
            priority,
            duck: AnnouncementDuckingV1 {
                attenuation_db: -20.0,
                attack_ms: 100,
                release_ms: 200,
            },
            expires_at_ms: scheduled_at_ms + 10_000,
            resume: AnnouncementResumeV1::ResumePrevious,
        }),
    }
}

fn approx(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn attack_hold_and_release_publish_deterministic_gain_and_one_completion() {
    let gain = DuckGain::default();
    let mut envelope = DuckEnvelope::new(gain.clone());

    let receipt = envelope.handle_control(
        scheduled("doorbell", AnnouncementPriorityV1::Announcement, 1_000),
        900,
    );
    assert_eq!(receipt.len(), 1);
    assert_eq!(receipt[0].lifecycle, AnnouncementLifecycle::Scheduled);
    assert_eq!(receipt[0].intent, None);
    approx(gain.load(), 1.0);

    assert!(envelope.tick(999).is_empty());
    approx(gain.load(), 1.0);
    let started = envelope.tick(1_000);
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].lifecycle, AnnouncementLifecycle::Started);
    approx(gain.load(), 1.0);

    assert!(envelope.tick(1_050).is_empty());
    approx(gain.load(), 0.55);
    assert!(envelope.tick(1_100).is_empty());
    approx(gain.load(), 0.1);
    assert!(envelope.tick(1_999).is_empty());
    approx(gain.load(), 0.1);

    let completed = envelope.tick(2_000);
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].lifecycle, AnnouncementLifecycle::Completed);
    approx(gain.load(), 0.1);
    assert!(envelope.tick(2_100).is_empty());
    approx(gain.load(), 0.55);
    assert!(envelope.tick(2_200).is_empty());
    approx(gain.load(), 1.0);
    assert!(envelope.tick(2_300).is_empty());
    approx(gain.load(), 1.0);
}

#[test]
fn cancellation_during_attack_releases_from_current_gain_once() {
    let gain = DuckGain::default();
    let mut envelope = DuckEnvelope::new(gain.clone());
    let control = scheduled("cancelled", AnnouncementPriorityV1::Announcement, 1_000);
    envelope.handle_control(control.clone(), 900);
    envelope.tick(1_000);
    envelope.tick(1_050);
    approx(gain.load(), 0.55);

    let mut cancelled = control;
    cancelled.lifecycle = AnnouncementLifecycle::Cancelled;
    cancelled.intent = None;
    let ack = envelope.handle_control(cancelled.clone(), 1_050);
    assert_eq!(ack.len(), 1);
    assert_eq!(ack[0].lifecycle, AnnouncementLifecycle::Cancelled);
    assert!(envelope.handle_control(cancelled, 1_060).is_empty());

    envelope.tick(1_150);
    approx(gain.load(), 0.775);
    envelope.tick(1_250);
    approx(gain.load(), 1.0);
    assert!(envelope.tick(1_300).is_empty());
    approx(gain.load(), 1.0);
}

#[test]
fn terminal_between_ticks_releases_from_the_last_published_gain() {
    let gain = DuckGain::default();
    let mut envelope = DuckEnvelope::new(gain.clone());
    let control = scheduled("between-ticks", AnnouncementPriorityV1::Announcement, 1_000);
    envelope.handle_control(control.clone(), 900);
    envelope.tick(1_000);
    envelope.tick(1_050);
    approx(gain.load(), 0.55);

    let mut completed = control;
    completed.lifecycle = AnnouncementLifecycle::Completed;
    completed.intent = None;
    envelope.handle_control(completed, 1_075);

    approx(gain.load(), 0.55);
    envelope.tick(1_175);
    approx(gain.load(), 0.775);
}

#[test]
fn terminal_before_started_does_not_publish_an_unobserved_attack_gain() {
    let gain = DuckGain::default();
    let mut envelope = DuckEnvelope::new(gain.clone());
    let control = scheduled("never-started", AnnouncementPriorityV1::Announcement, 1_000);
    envelope.handle_control(control.clone(), 900);

    let mut failed = control;
    failed.lifecycle = AnnouncementLifecycle::Failed;
    failed.intent = None;
    let acknowledgements = envelope.handle_control(failed, 1_050);

    assert_eq!(acknowledgements.len(), 1);
    assert_eq!(acknowledgements[0].lifecycle, AnnouncementLifecycle::Failed);
    approx(gain.load(), 1.0);
    envelope.tick(1_150);
    approx(gain.load(), 1.0);
}

#[test]
fn higher_priority_preempts_and_lower_priority_is_failed_without_unbounded_queueing() {
    let gain = DuckGain::default();
    let mut envelope = DuckEnvelope::new(gain);
    envelope.handle_control(
        scheduled("chime", AnnouncementPriorityV1::Chime, 1_000),
        900,
    );

    let dropped = envelope.handle_control(
        scheduled("music", AnnouncementPriorityV1::Music, 1_010),
        905,
    );
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].announcement_id, "music");
    assert_eq!(dropped[0].lifecycle, AnnouncementLifecycle::Failed);
    assert_eq!(envelope.active_id(), Some("chime"));

    let preempted = envelope.handle_control(
        scheduled("emergency", AnnouncementPriorityV1::Emergency, 1_020),
        910,
    );
    assert_eq!(preempted.len(), 2);
    assert_eq!(preempted[0].announcement_id, "chime");
    assert_eq!(preempted[0].lifecycle, AnnouncementLifecycle::Cancelled);
    assert_eq!(preempted[1].announcement_id, "emergency");
    assert_eq!(preempted[1].lifecycle, AnnouncementLifecycle::Scheduled);
    assert_eq!(envelope.active_id(), Some("emergency"));

    let duplicate = envelope.handle_control(
        scheduled("emergency", AnnouncementPriorityV1::Emergency, 1_020),
        915,
    );
    assert_eq!(duplicate.len(), 1);
    assert_eq!(duplicate[0].lifecycle, AnnouncementLifecycle::Scheduled);
    assert_eq!(envelope.active_id(), Some("emergency"));
}

#[test]
fn server_ordered_cancel_then_schedule_does_not_acknowledge_cancel_twice() {
    let gain = DuckGain::default();
    let mut envelope = DuckEnvelope::new(gain);
    let chime = scheduled("chime", AnnouncementPriorityV1::Chime, 1_000);
    envelope.handle_control(chime.clone(), 900);

    let mut cancelled = chime;
    cancelled.lifecycle = AnnouncementLifecycle::Cancelled;
    cancelled.intent = None;
    let cancelled_ack = envelope.handle_control(cancelled, 910);
    assert_eq!(cancelled_ack.len(), 1);
    assert_eq!(cancelled_ack[0].lifecycle, AnnouncementLifecycle::Cancelled);

    let scheduled_ack = envelope.handle_control(
        scheduled("next-chime", AnnouncementPriorityV1::Chime, 1_020),
        910,
    );
    assert_eq!(scheduled_ack.len(), 1);
    assert_eq!(scheduled_ack[0].announcement_id, "next-chime");
    assert_eq!(scheduled_ack[0].lifecycle, AnnouncementLifecycle::Scheduled);
    assert_eq!(envelope.active_id(), Some("next-chime"));
}
