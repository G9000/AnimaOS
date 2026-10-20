use super::delivery::{DeliveryChannel, DeliveryEvent, DeliveryResult, ReminderStatus};
use super::*;

fn fired() -> ScheduledPromptRecord {
    let mut record = test_automation("agent", "reminder");
    record.last_fired = Some(ScheduleLastFired {
        fired_at_ms: 71_000,
        scheduled_at_ms: Some(70_000),
        run_idempotency_key: "occurrence-1".into(),
        manual: false,
    });
    record.next_due_at_ms = 130_000;
    record.last_safe_outcome = Some(ScheduleSafeOutcome {
        status: ScheduleOutcomeStatus::Spoke,
        occurred_at_ms: 71_000,
        error_code: None,
    });
    record
}

fn event(result: DeliveryResult) -> DeliveryEvent {
    DeliveryEvent {
        occurrence_id: "occurrence-1".into(),
        outbound_id: Some("outbound-1".into()),
        scheduled_at_ms: Some(70_000),
        channel: DeliveryChannel::Telegram,
        attempted_at_ms: Some(72_000),
        completed_at_ms: Some(73_000),
        result,
        provider_message_ids: vec![],
    }
}

#[test]
fn reminder_status_successful_send_is_not_device_delivery() {
    let mut record = fired();
    let mut sent = event(DeliveryResult::Sent);
    sent.provider_message_ids.push("message-1".into());
    record.delivery_events.push(sent);
    let status = ReminderStatus::from_record(&record, 80_000);
    assert_eq!(status.status, "sent");
    assert_eq!(status.automation_id, "reminder");
    assert_eq!(status.scheduled_at_ms, Some(70_000));
    assert!(!status.receipt_verified);
    assert!(status.explanation.contains("receipt cannot be verified"));
}

#[test]
fn reminder_status_definite_send_failure_is_failed() {
    let mut record = fired();
    record.delivery_events.push(event(DeliveryResult::Failed));
    assert_eq!(
        ReminderStatus::from_record(&record, 80_000).status,
        "failed"
    );
}

#[test]
fn reminder_status_missing_or_ambiguous_evidence_is_unconfirmed() {
    let mut record = fired();
    assert_eq!(
        ReminderStatus::from_record(&record, 80_000).status,
        "delivery_unconfirmed"
    );
    record.delivery_events.push(event(DeliveryResult::Unknown));
    assert_eq!(
        ReminderStatus::from_record(&record, 80_000).status,
        "delivery_unconfirmed"
    );
    // Evidence from an older occurrence cannot confirm this occurrence.
    record.delivery_events[0].occurrence_id = "older".into();
    record.delivery_events[0].result = DeliveryResult::Sent;
    assert_eq!(
        ReminderStatus::from_record(&record, 80_000).status,
        "delivery_unconfirmed"
    );
}

#[test]
fn reminder_status_ran_or_spoke_without_send_receipt_is_unconfirmed() {
    let record = fired();
    let status = ReminderStatus::from_record(&record, 80_000);
    assert_eq!(status.status, "delivery_unconfirmed");
    assert!(!status.receipt_verified);
    assert!(status.explanation.contains("does not confirm"));
}

#[test]
fn reminder_status_is_scheduled_only_before_due_and_when_enabled() {
    let mut record = test_automation("agent", "reminder");
    assert_eq!(
        ReminderStatus::from_record(&record, 69_999).status,
        "scheduled"
    );
    assert_eq!(
        ReminderStatus::from_record(&record, 70_001).status,
        "delivery_unconfirmed"
    );
    record.enabled = false;
    assert_eq!(
        ReminderStatus::from_record(&record, 69_999).status,
        "delivery_unconfirmed"
    );
}

#[test]
fn reminder_delivery_journal_is_bounded_and_survives_snapshot_round_trip() {
    let mut record = fired();
    for index in 0..120 {
        let mut attempt = event(DeliveryResult::Unknown);
        attempt.attempted_at_ms = Some(72_000 + index);
        record.record_delivery_event(attempt);
    }
    assert_eq!(record.delivery_events.len(), 100);
    let json = serde_json::to_value(&record).unwrap();
    let restored: ScheduledPromptRecord = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(restored.delivery_events, record.delivery_events);
    let mut legacy = json;
    legacy.as_object_mut().unwrap().remove("deliveryEvents");
    legacy["lastFired"]
        .as_object_mut()
        .unwrap()
        .remove("scheduledAtMs");
    let old: ScheduledPromptRecord = serde_json::from_value(legacy).unwrap();
    assert!(old.delivery_events.is_empty());
    assert_eq!(
        ReminderStatus::from_record(&old, 80_000).status,
        "delivery_unconfirmed"
    );
    let mut edited_legacy = old;
    edited_legacy.trigger = ScheduleTrigger::Once { at_ms: 200_000 };
    assert_eq!(
        ReminderStatus::from_record(&edited_legacy, 80_000).scheduled_at_ms,
        None,
        "an edited trigger cannot supply an unknown old occurrence's original time"
    );
}

#[test]
fn reminder_telegram_results_require_complete_provider_send_acknowledgements() {
    use super::delivery::telegram_send_result;
    use crate::connectors::telegram::{TelegramSentMessage, TelegramTransportError};
    use crate::connectors::{TelegramChatKind, TelegramChatMetadata};
    let receipt = TelegramSentMessage {
        message_id: "101".into(),
        chat: TelegramChatMetadata {
            id: "42".into(),
            kind: TelegramChatKind::Private,
            title: None,
            username: None,
        },
    };
    assert_eq!(
        telegram_send_result(&Ok(vec![receipt.clone()]), "42", "hello").0,
        DeliveryResult::Sent
    );
    assert_eq!(
        telegram_send_result(&Ok(vec![]), "42", "hello").0,
        DeliveryResult::Unknown
    );
    assert_eq!(
        telegram_send_result(&Ok(vec![receipt]), "other", "hello").0,
        DeliveryResult::Unknown
    );
    for error in [
        TelegramTransportError::Timeout,
        TelegramTransportError::Transport,
        TelegramTransportError::InvalidResponse,
    ] {
        assert_eq!(
            telegram_send_result(&Err(error), "42", "hello").0,
            DeliveryResult::Unknown
        );
    }
    assert_eq!(
        telegram_send_result(
            &Err(TelegramTransportError::HttpStatus { status: 403 }),
            "42",
            "hello"
        )
        .0,
        DeliveryResult::Failed
    );
    assert_eq!(
        telegram_send_result(
            &Err(TelegramTransportError::HttpStatus { status: 403 }),
            "42",
            &"x".repeat(5000)
        )
        .0,
        DeliveryResult::Unknown,
        "an earlier chunk may already have been accepted"
    );
}
