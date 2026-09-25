use sonium_control::announcements::{
    AnnouncementCoordinator, AnnouncementIntent, AnnouncementLifecycle, AnnouncementLimits,
    AnnouncementPriority, AnnouncementSource, Ducking, ResumePolicy,
};

fn intent(key: &str, priority: AnnouncementPriority) -> AnnouncementIntent {
    AnnouncementIntent {
        version: 1,
        idempotency_key: key.into(),
        target_groups: vec!["default".into()],
        priority,
        source: AnnouncementSource::Uri("https://media.example.test/chime.ogg".into()),
        duck: Ducking {
            attenuation_db: -18.0,
            attack_ms: 25,
            release_ms: 100,
        },
        max_duration_ms: 1_000,
        expires_at_ms: 10_000,
        resume: ResumePolicy::ResumePrevious,
    }
}

#[test]
fn higher_priority_interrupts_lower_priority_and_equal_priority_queues() {
    let mut coordinator = AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default"]);
    let low = coordinator
        .admit(intent("low", AnnouncementPriority::Chime), 1)
        .unwrap();
    let equal = coordinator
        .admit(intent("equal", AnnouncementPriority::Chime), 2)
        .unwrap();
    let high = coordinator
        .admit(intent("high", AnnouncementPriority::Emergency), 3)
        .unwrap();

    assert_eq!(
        coordinator.record(&low.id).unwrap().lifecycle,
        AnnouncementLifecycle::Cancelled
    );
    assert_eq!(
        coordinator.record(&equal.id).unwrap().lifecycle,
        AnnouncementLifecycle::Scheduled
    );
    assert_eq!(
        coordinator.record(&high.id).unwrap().lifecycle,
        AnnouncementLifecycle::Scheduled
    );
    assert!(coordinator.record(&low.id).unwrap().groups[0].resume_emitted);
}

#[test]
fn same_idempotency_key_returns_the_original_announcement_only_for_same_intent() {
    let mut coordinator = AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default"]);
    let first = coordinator
        .admit(intent("retry", AnnouncementPriority::Chime), 1)
        .unwrap();
    let retry = coordinator
        .admit(intent("retry", AnnouncementPriority::Chime), 2)
        .unwrap();

    assert_eq!(retry.id, first.id);
    assert!(retry.duplicate);
    assert!(coordinator
        .admit(intent("retry", AnnouncementPriority::Emergency), 3)
        .is_err());
}

#[test]
fn expiry_cancels_and_resumes_each_group_exactly_once() {
    let mut coordinator = AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default"]);
    let active = coordinator
        .admit(intent("expires", AnnouncementPriority::Announcement), 1)
        .unwrap();

    let first = coordinator.expire(10_000);
    let second = coordinator.expire(10_001);

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].lifecycle, AnnouncementLifecycle::Cancelled);
    assert!(first[0].resume);
    assert!(second.is_empty());
    assert!(coordinator.record(&active.id).unwrap().groups[0].resume_emitted);
}

#[test]
fn cancelling_and_terminal_client_retries_resume_only_once() {
    let mut coordinator = AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default"]);
    let active = coordinator
        .admit(intent("cancel", AnnouncementPriority::Announcement), 1)
        .unwrap();
    coordinator
        .acknowledge(&active.id, "default", AnnouncementLifecycle::Started)
        .unwrap();

    let cancelled = coordinator.cancel(&active.id).unwrap();
    let retried_terminal = coordinator
        .acknowledge(&active.id, "default", AnnouncementLifecycle::Cancelled)
        .unwrap();

    assert_eq!(cancelled.len(), 1);
    assert!(cancelled[0].resume);
    assert!(retried_terminal.is_empty());
}

#[test]
fn cancelling_a_queued_announcement_never_requests_resume() {
    let mut coordinator = AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default"]);
    coordinator
        .admit(
            intent("active-for-queued", AnnouncementPriority::Announcement),
            1,
        )
        .unwrap();
    let queued = coordinator
        .admit(
            intent("queued-cancel", AnnouncementPriority::Announcement),
            2,
        )
        .unwrap();

    let cancelled = coordinator.cancel(&queued.id).unwrap();

    assert_eq!(cancelled.len(), 1);
    assert!(!cancelled[0].resume);
    assert!(!coordinator.record(&queued.id).unwrap().groups[0].resume_emitted);
}

#[test]
fn rejects_a_group_queue_that_exceeds_its_bounded_depth() {
    let mut coordinator = AnnouncementCoordinator::new(
        AnnouncementLimits {
            max_queue_depth_per_group: 1,
            ..AnnouncementLimits::default()
        },
        ["default"],
    );

    coordinator
        .admit(intent("active", AnnouncementPriority::Announcement), 1)
        .unwrap();
    coordinator
        .admit(intent("queued", AnnouncementPriority::Announcement), 2)
        .unwrap();

    let error = coordinator
        .admit(intent("overflow", AnnouncementPriority::Announcement), 3)
        .unwrap_err();

    assert!(error.to_string().contains("queue depth"));
}

#[test]
fn terminal_ack_for_a_queued_announcement_removes_it_without_advancing_the_active_one() {
    let mut coordinator = AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default"]);
    let active = coordinator
        .admit(intent("active", AnnouncementPriority::Announcement), 1)
        .unwrap();
    let queued = coordinator
        .admit(intent("queued", AnnouncementPriority::Announcement), 2)
        .unwrap();
    let next = coordinator
        .admit(intent("next", AnnouncementPriority::Announcement), 3)
        .unwrap();

    let terminal = coordinator
        .acknowledge(&queued.id, "default", AnnouncementLifecycle::Completed)
        .unwrap();
    coordinator
        .acknowledge(&active.id, "default", AnnouncementLifecycle::Completed)
        .unwrap();

    assert_eq!(terminal.len(), 1);
    assert_eq!(
        coordinator.record(&queued.id).unwrap().lifecycle,
        AnnouncementLifecycle::Completed
    );
    assert!(coordinator
        .acknowledge(&next.id, "default", AnnouncementLifecycle::Started)
        .is_ok());
}

#[test]
fn removing_a_group_terminalizes_its_announcements_and_leaves_no_stale_queue_references() {
    let mut coordinator =
        AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default", "removed"]);
    let mut target = intent("removed-group", AnnouncementPriority::Announcement);
    target.target_groups = vec!["removed".into()];
    let announcement = coordinator.admit(target, 1).unwrap();

    coordinator.remove_group("removed");

    assert_eq!(
        coordinator.record(&announcement.id).unwrap().lifecycle,
        AnnouncementLifecycle::Cancelled
    );
    assert!(coordinator.expire(10_000).is_empty());
    assert!(coordinator.cancel(&announcement.id).unwrap().is_empty());
}

#[test]
fn terminal_history_and_idempotency_index_are_globally_bounded() {
    let mut coordinator = AnnouncementCoordinator::new(
        AnnouncementLimits {
            max_retained_records: 2,
            ..AnnouncementLimits::default()
        },
        ["default"],
    );

    let first = coordinator
        .admit(
            intent("retained-first", AnnouncementPriority::Announcement),
            1,
        )
        .unwrap();
    coordinator
        .acknowledge(&first.id, "default", AnnouncementLifecycle::Completed)
        .unwrap();
    let second = coordinator
        .admit(
            intent("retained-second", AnnouncementPriority::Announcement),
            2,
        )
        .unwrap();
    coordinator
        .acknowledge(&second.id, "default", AnnouncementLifecycle::Completed)
        .unwrap();
    let third = coordinator
        .admit(
            intent("retained-third", AnnouncementPriority::Announcement),
            3,
        )
        .unwrap();

    assert_eq!(coordinator.records().len(), 2);
    assert!(coordinator.record(&first.id).is_none());
    assert_eq!(
        coordinator
            .admit(
                intent("retained-second", AnnouncementPriority::Announcement),
                4
            )
            .unwrap()
            .id,
        second.id
    );
    let reused = coordinator
        .admit(
            intent("retained-first", AnnouncementPriority::Announcement),
            4,
        )
        .unwrap();
    assert!(
        !reused.duplicate,
        "an evicted idempotency key is a new admission"
    );
    assert_ne!(reused.id, first.id);
    assert_ne!(reused.id, third.id);
}

#[test]
fn rejects_uri_without_an_authority_or_with_control_characters() {
    let mut coordinator = AnnouncementCoordinator::new(AnnouncementLimits::default(), ["default"]);
    let mut media = intent("media-uri", AnnouncementPriority::Announcement);
    media.source = AnnouncementSource::Uri("media://library/doorbell.ogg".into());
    assert!(coordinator.admit(media, 1).is_ok());

    let mut no_authority = intent("no-authority", AnnouncementPriority::Announcement);
    no_authority.source = AnnouncementSource::Uri("https://".into());
    assert!(coordinator.admit(no_authority, 1).is_err());

    let mut controls = intent("control-char", AnnouncementPriority::Announcement);
    controls.source = AnnouncementSource::Uri("https://media.example.test/a.ogg\n".into());
    assert!(coordinator.admit(controls, 1).is_err());
}
