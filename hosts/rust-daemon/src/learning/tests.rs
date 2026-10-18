use super::records::*;
use super::*;
use std::collections::HashSet;

fn record(source: &str, at: u64) -> LearningReviewRecord {
    LearningReviewRecord {
        id: format!("review-{source}"),
        agent_id: "owner".into(),
        source_run_id: source.into(),
        session_id: "chat:case".into(),
        trigger: LearningTrigger::Manual,
        policy_version: POLICY_VERSION,
        evidence_digest: "a".repeat(64),
        evidence_brief: "bounded supplied source".into(),
        evidence_anchors: vec!["reply".into()],
        state: LearningReviewState::Queued,
        reserved_at_ms: at,
        started_at_ms: None,
        finished_at_ms: None,
        model: "fixture".into(),
        provider: Some("test".into()),
        usage: None,
        usage_complete: false,
        error: None,
        proposals: vec![],
    }
}

#[test]
fn m38_shared_daily_reservations_are_not_refunded_by_cancel_failure_or_restart() {
    let mut registry = LearningRegistry::default();
    for (index, state) in [LearningReviewState::Cancelled, LearningReviewState::Failed]
        .into_iter()
        .enumerate()
    {
        let mut review = record(&index.to_string(), 86_400_001);
        review.finish(state, "fixture", "Fixture terminal outcome", 86_400_002);
        registry.reserve(review).unwrap();
    }
    assert!(registry.reserve(record("third", 86_400_003)).is_err());
    let mut restored = LearningRegistry::restored(
        registry.snapshot(),
        &HashSet::from(["owner".into()]),
        86_400_004,
    );
    assert!(restored.reserve(record("third", 86_400_005)).is_err());
    assert!(restored.reserve(record("tomorrow", 172_800_001)).is_ok());
}

#[test]
fn the_review_budget_rolls_over_24_hours_instead_of_a_utc_day() {
    const HOUR: u64 = 3_600_000;
    let mut registry = LearningRegistry::default();
    let late = 10 * WINDOW_MS - HOUR;
    assert_eq!(registry.availability(late), (DAILY_LIMIT, None));
    registry.reserve(record("first", late)).unwrap();
    registry.reserve(record("second", late + 1)).unwrap();
    // Two hours later is a new UTC day, but not a new budget.
    let after_midnight = late + 2 * HOUR;
    assert_eq!(
        registry.availability(after_midnight),
        (0, Some(late + WINDOW_MS))
    );
    let refused = registry
        .reserve(record("third", after_midnight))
        .unwrap_err();
    assert!(refused.message().contains("used up"));
    assert_eq!(
        registry.availability(late + WINDOW_MS),
        (1, None),
        "the oldest review leaves the window"
    );
    registry.reserve(record("third", late + WINDOW_MS)).unwrap();
    assert_eq!(
        registry.availability(late + WINDOW_MS),
        (0, Some(late + 1 + WINDOW_MS))
    );
    assert_eq!(
        registry
            .policy("owner", late + WINDOW_MS, LearningMode::Strict)
            .next_available_at_ms,
        Some(late + 1 + WINDOW_MS)
    );
}

#[test]
fn m38_manual_automatic_dedup_and_evidence_conflict_keep_one_attempt() {
    let mut registry = LearningRegistry::default();
    let original = registry.reserve(record("same", 1)).unwrap();
    let mut automatic = record("same", 2);
    automatic.trigger = LearningTrigger::Automatic;
    assert_eq!(registry.reserve(automatic.clone()).unwrap(), original);
    automatic.evidence_digest = "b".repeat(64);
    assert!(registry.reserve(automatic).is_err());
    assert_eq!(registry.snapshot().reviews.len(), 1);
}

#[test]
fn m38_recovery_interrupts_pending_without_replay_or_quota_refund() {
    let mut snapshot = LearningSnapshot::default();
    snapshot.reviews.push(record("queued", 1));
    let mut running = record("running", 1);
    running.state = LearningReviewState::Running;
    running.started_at_ms = Some(2);
    snapshot.reviews.push(running);
    snapshot.policies.push(LearningPolicy {
        agent_id: "deleted".into(),
        auto_enabled: true,
    });
    let restored = LearningRegistry::restored(snapshot, &HashSet::from(["owner".into()]), 3);
    assert!(!restored.enabled("owner"));
    assert!(!restored.enabled("deleted"));
    assert!(restored
        .snapshot()
        .reviews
        .iter()
        .all(|r| r.state == LearningReviewState::Interrupted && r.proposals.is_empty()));
    assert!(restored
        .snapshot()
        .reviews
        .iter()
        .all(|r| r.reserved_at_ms == 1));
}

#[test]
fn m38_capacity_refuses_new_work_without_dropping_keys_or_old_reservations() {
    let mut registry = LearningRegistry::default();
    for i in 0..RETAINED_LIMIT {
        let review = record(&i.to_string(), (i as u64 + 1) * 86_400_000);
        registry.reviews.insert(review.id.clone(), review);
    }
    assert!(registry.reserve(record("new", 2000 * 86_400_000)).is_err());
    let old = record("0", 86_400_000);
    assert_eq!(registry.reserve(old.clone()).unwrap(), old);
    assert_eq!(registry.snapshot().reviews.len(), RETAINED_LIMIT);
}

#[test]
fn m38_snapshot_validation_rejects_duplicate_attempts_unknown_anchors_and_hidden_advice() {
    let review = record("valid", 1);
    let mut snapshot = LearningSnapshot {
        reviews: vec![review.clone()],
        policies: vec![],
    };
    LearningRegistry::validate(&snapshot).unwrap();
    snapshot.reviews.push(review.clone());
    assert!(LearningRegistry::validate(&snapshot).is_err());
    snapshot.reviews = vec![review];
    snapshot.reviews[0].proposals.push(LearningProposal {
        id: "p1".into(),
        observation: "Observed source".into(),
        suggestion: "Inspect the source".into(),
        anchors: vec!["foreign".into()],
        decision: LearningDecision::Pending,
        decision_version: 0,
        skill_draft_id: None,
        memory_id: None,
    });
    assert!(LearningRegistry::validate(&snapshot).is_err());
    snapshot.reviews[0].proposals[0].anchors = vec!["reply".into()];
    snapshot.reviews[0].proposals[0].suggestion = "hidden\u{202e}text".into();
    assert!(LearningRegistry::validate(&snapshot).is_err());
}

#[test]
fn m38_utf8_evidence_excerpt_preserves_byte_bounds() {
    let text = "🦀".repeat(1000);
    let cut = super::evidence::bounded_text(&text, 1536);
    assert!(cut.len() <= 1536);
    assert!(text.starts_with(&cut));
    assert!(cut.is_char_boundary(cut.len()));
    assert!(!valid_advice_text("control\u{0008}text"));
    assert!(valid_advice_text("Ordinary\nformatted\tadvice"));
}
