//! Bounded delivery and receipt reconciliation for the durable selection journal.
use super::{
    digest, now_ms, reconcile, validate_source, Publication, PublicationError, PublicationService,
    PublicationState, Transition,
};
use serde_json::{json, Value};

fn authority_error(error: &str) -> bool {
    matches!(
        error,
        "approval_required"
            | "policy_denied"
            | "publication_authority_changed"
            | "profile_ownership_lost"
            | "connection_manually_disconnected"
            | "ACCESS_DENIED"
            | "UNAUTHORIZED"
            | "FORBIDDEN"
            | "GENERATION_MISMATCH"
            | "WORKSPACE_MISMATCH"
    )
}

impl PublicationService {
    pub(super) async fn change(&self, item: &Publication, transition: Transition) {
        let id = item.admission.publication_id.clone();
        let revision = item.revision;
        if matches!(
            self.db(move |s| s.transition(&id, revision, transition, now_ms()))
                .await,
            Err(PublicationError::Unavailable(_))
        ) {
            self.storage_failed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    pub(super) async fn dispatch(&self, item: Publication) {
        if !self.accepting() {
            return;
        }
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let Some(manager) = state.read().await.mcp_manager.clone() else {
            return;
        };
        let memory = state.read().await.memory_handle();
        let source_guard = if item.state == PublicationState::Queued {
            Some(memory.read().await)
        } else {
            None
        };
        if let Some(guard) = &source_guard {
            let source = guard.get(&item.admission.local_id);
            match source {
                None => {
                    self.change(
                        &item,
                        if item.attempts == 0 {
                            Transition::Cancelled
                        } else {
                            Transition::Blocked {
                                error: "selected_source_missing".into(),
                            }
                        },
                    )
                    .await;
                    return;
                }
                Some(source)
                    if validate_source(&source, &item.admission.agent_id, &source.content)
                        .is_err()
                        || item.admission.source_digest.as_ref()
                            != Some(&digest(&source.content)) =>
                {
                    self.change(
                        &item,
                        Transition::Blocked {
                            error: "selected_source_changed".into(),
                        },
                    )
                    .await;
                    return;
                }
                _ => {}
            }
        }
        if let Err(error) = manager
            .publication_preflight(&item, "mother_get_context", &json!({"limit":1}))
            .await
        {
            self.change(
                &item,
                Transition::Blocked {
                    error: error.into(),
                },
            )
            .await;
            return;
        }
        let id = item.admission.publication_id.clone();
        let revision = item.revision;
        let Ok(claim) = self.db(move |s| s.claim(&id, revision, now_ms())).await else {
            return;
        };
        drop(source_guard);
        let phase = claim.resume_state.unwrap_or(PublicationState::Unknown);
        let backoff = now_ms().saturating_add(
            (2_000_i64.saturating_mul(1_i64 << claim.attempts.min(8))).min(300_000),
        );
        let context = self
            .remote(&manager, &claim, "mother_get_context", json!({"limit":1}))
            .await;
        match context {
            Ok(value) if reconcile::authority(&claim.admission.binding, &value).is_ok() => {}
            Ok(_) => {
                self.change(
                    &claim,
                    Transition::Blocked {
                        error: "workspace_generation_or_access_changed".into(),
                    },
                )
                .await;
                return;
            }
            Err(error) => {
                let transition = if authority_error(&error) {
                    Transition::Blocked { error }
                } else if phase == PublicationState::Queued {
                    Transition::Retry {
                        error,
                        next_attempt_at_ms: backoff,
                    }
                } else {
                    Transition::Unknown {
                        error,
                        next_attempt_at_ms: backoff,
                    }
                };
                self.change(&claim, transition).await;
                return;
            }
        }
        if !self.accepting() {
            self.change(
                &claim,
                Transition::Unknown {
                    error: "dispatch_interrupted".into(),
                    next_attempt_at_ms: backoff,
                },
            )
            .await;
            return;
        }
        let (remote, args) = match phase {
            PublicationState::Queued => (
                claim.admission.remote_tool.as_str(),
                claim.admission.request.clone(),
            ),
            PublicationState::Unknown => (
                "mother_get_receipt",
                json!({"operationId":claim.admission.operation_id}),
            ),
            PublicationState::SubmittedForReview => match &claim.proposal_id {
                Some(id) => ("mother_get_proposal", json!({"id":id})),
                None => {
                    self.change(
                        &claim,
                        Transition::Blocked {
                            error: "proposal_identity_missing".into(),
                        },
                    )
                    .await;
                    return;
                }
            },
            _ => return,
        };
        let transition = match self.remote(&manager, &claim, remote, args).await {
            Ok(value) => {
                let result = if phase == PublicationState::SubmittedForReview {
                    reconcile::proposal(&claim, value, now_ms())
                } else {
                    reconcile::submission(&claim, value, now_ms())
                };
                result.unwrap_or_else(|_| {
                    if phase == PublicationState::SubmittedForReview {
                        Transition::Blocked {
                            error: "approval_unreadable".into(),
                        }
                    } else {
                        Transition::Unknown {
                            error: "invalid_remote_evidence".into(),
                            next_attempt_at_ms: backoff,
                        }
                    }
                })
            }
            Err(error) if error == "NOT_FOUND" && phase == PublicationState::Unknown => {
                Transition::ReconciledAbsent {
                    next_attempt_at_ms: backoff,
                }
            }
            Err(error)
                if phase == PublicationState::Queued
                    && matches!(
                        error.as_str(),
                        "CONFLICT"
                            | "INVALID_INPUT"
                            | "INVALID_REQUEST"
                            | "NOT_FOUND"
                            | "KIND_MISMATCH"
                            | "REFERENCE_NOT_FOUND"
                            | "OPERATION_REUSED"
                    ) =>
            {
                Transition::Conflict {
                    error: format!("remote_request_rejected:{error}"),
                    receipt: json!({"error":{"code":error}}),
                }
            }
            Err(error)
                if phase == PublicationState::SubmittedForReview
                    && matches!(
                        error.as_str(),
                        "NOT_FOUND" | "ACCESS_DENIED" | "FORBIDDEN" | "invalid_remote_evidence"
                    ) =>
            {
                Transition::Blocked {
                    error: "approval_unreadable".into(),
                }
            }
            Err(error) if authority_error(&error) => Transition::Blocked { error },
            Err(error) => Transition::Unknown {
                error,
                next_attempt_at_ms: backoff,
            },
        };
        self.change(&claim, transition).await;
    }
    async fn remote(
        &self,
        manager: &crate::mcp::McpManager,
        item: &Publication,
        remote: &str,
        args: Value,
    ) -> Result<Value, String> {
        self.owner
            .verify_owned()
            .await
            .map_err(|_| "profile_ownership_lost".to_owned())?;
        if !self.accepting() {
            return Err("dispatch_interrupted".into());
        }
        let content = manager
            .invoke_publication(item, remote, args, self.cancel.clone())
            .await
            .map_err(str::to_owned)?;
        match reconcile::envelope(&content.text) {
            Ok(value) => Ok(value),
            Err(reconcile::EvidenceError::Remote(code)) => Err(code),
            Err(_) => Err("invalid_remote_evidence".into()),
        }
    }
}
