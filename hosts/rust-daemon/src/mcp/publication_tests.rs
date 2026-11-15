use super::*;
use crate::memory_provider::outbox::PublicationState;

#[test]
fn queued_publication_cannot_poll_a_different_operation() {
    assert!(!operation_allowed(
        PublicationState::Queued,
        None,
        "mother_get_receipt",
        false
    ));
    assert!(!operation_allowed(
        PublicationState::Sending,
        Some(PublicationState::Unknown),
        "mother_propose_record",
        true
    ));
    assert!(operation_allowed(
        PublicationState::Sending,
        Some(PublicationState::Unknown),
        "mother_get_receipt",
        false
    ));
    assert!(operation_allowed(
        PublicationState::Sending,
        Some(PublicationState::Queued),
        "mother_propose_record",
        true
    ));
    assert!(!operation_allowed(
        PublicationState::Saved,
        None,
        "mother_get_context",
        false
    ));
}
