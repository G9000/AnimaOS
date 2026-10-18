//! Wire as a child of the existing native-fixture `tests` module.
use super::*;
use crate::tool_rpc::BrokerDisposition;

#[tokio::test]
async fn fp06_reply_native_success_and_actual_handler_failure_are_distinct() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    let credentials = broker.credentials();
    let success = broker
        .request_reply(&frame(
            &credentials,
            1,
            "read_file",
            json!({"file_path":"alpha.json"}),
        ))
        .await;
    assert_eq!(success.disposition, BrokerDisposition::Success);
    let failed = broker
        .request_reply(&frame(
            &credentials,
            2,
            "read_file",
            json!({"file_path":"missing-native-file.json"}),
        ))
        .await;
    assert_eq!(failed.disposition, BrokerDisposition::NativeFailed);
    assert_eq!(fixture.receipts().await[2].status, ActionStatus::Failed);
    assert!(!failed.text.is_empty());
}

#[tokio::test]
async fn fp06_reply_policy_or_current_grant_refusal_has_no_handler_receipt() {
    for policy_denial in [false, true] {
        let fixture = Fixture::new(PolicyAction::Allow).await;
        let mut broker = fixture.capture().await.unwrap();
        if policy_denial {
            fixture.state.write().await.approvals.set_policy(
                &fixture.agent.id,
                ApprovalPolicy::default()
                    .with(RiskClass::Exec, PolicyAction::Allow)
                    .with(RiskClass::Write, PolicyAction::Deny),
            );
        } else {
            fixture.remove_grant("write_file").await;
        }
        let reply = broker
            .request_reply(&write_frame(&broker.credentials()))
            .await;
        assert_eq!(reply.disposition, BrokerDisposition::Refused);
        assert!(!fixture.output_exists());
        assert_eq!(fixture.receipts().await.len(), 1);
    }
}

#[tokio::test]
async fn fp06_reply_exact_sequence_replay_keeps_typed_result_without_effect() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    let request = write_frame(&broker.credentials());
    let first = broker.request_reply(&request).await;
    assert_eq!(first.disposition, BrokerDisposition::Success);
    std::fs::write(fixture.root.path().join(TARGET), b"owner newer data").unwrap();
    assert_eq!(broker.request_reply(&request).await, first);
    assert_eq!(
        std::fs::read(fixture.root.path().join(TARGET)).unwrap(),
        b"owner newer data"
    );
    assert_eq!(fixture.receipts().await.len(), 2);
}

#[tokio::test]
async fn fp06_reply_unacknowledged_native_outcome_is_review_required_not_native_failure() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    let credentials = broker.credentials();
    // Positive assertion makes the rejecting seam fail promptly before gates.
    assert_eq!(
        broker
            .request_reply(&frame(
                &credentials,
                1,
                "read_file",
                json!({"file_path":"alpha.json"})
            ))
            .await
            .disposition,
        BrokerDisposition::Success
    );
    let request = frame(
        &credentials,
        2,
        "write_file",
        json!({"file_path":TARGET,"content":"written through native dispatcher"}),
    );
    let intent = fixture.save_gate(false).await;
    let worker = tokio::spawn(async move { broker.request_reply(&request).await });
    entered(&intent).await;
    let outcome = fixture.save_gate(true).await;
    intent.release.add_permits(1);
    entered(&outcome).await;
    assert!(fixture.output_exists());
    outcome.release.add_permits(1);
    assert_eq!(
        worker.await.unwrap().disposition,
        BrokerDisposition::ReviewRequired
    );
    assert_eq!(fixture.receipts().await[2].status, ActionStatus::Started);
}
