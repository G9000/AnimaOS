use super::*;
use crate::mcp::{transport::*, McpConnectionRecord, McpManager, ReviewedMcpTool};
use anima_core::{prepare_external_tool, ExternalToolResult};
use futures::future::BoxFuture;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex as StdMutex,
};

#[derive(Default)]
struct Wire {
    calls: StdMutex<Vec<(String, Value)>>,
    pending: StdMutex<Option<Value>>,
    outcome: StdMutex<String>,
    lose: AtomicBool,
    fail_context: AtomicBool,
    reject_write: StdMutex<Option<String>>,
    gate: StdMutex<
        Option<(
            bool,
            Arc<tokio::sync::Semaphore>,
            Arc<tokio::sync::Semaphore>,
        )>,
    >,
    generation: StdMutex<Option<String>>,
}
struct Fake {
    wire: Arc<Wire>,
    tools: Vec<anima_core::ExternalToolDescriptor>,
    binding: binding::MemoryProviderBinding,
}
impl McpTransportFactory for Fake {
    fn connect(
        &self,
        _: ValidatedMcpLaunch,
        _: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        let session = Self {
            wire: self.wire.clone(),
            tools: self.tools.clone(),
            binding: self.binding.clone(),
        };
        Box::pin(async move { Ok(Arc::new(session) as Arc<dyn McpTransportSession>) })
    }
}
impl McpTransportSession for Fake {
    fn discover(
        &self,
        _: TransportControl,
    ) -> BoxFuture<'static, Result<McpDiscovery, McpTransportError>> {
        let tools = self.tools.clone();
        Box::pin(async move {
            Ok(McpDiscovery {
                protocol_version: "2025-11-25".into(),
                tools,
            })
        })
    }
    fn call_once(
        &self,
        call: McpCall,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<ExternalToolResult, McpTransportError>> {
        self.wire
            .calls
            .lock()
            .unwrap()
            .push((call.remote_name.clone(), call.arguments.clone()));
        if call.remote_name == "mother_get_context"
            && self.wire.fail_context.swap(false, Ordering::SeqCst)
        {
            return Box::pin(async { Err(McpTransportError::Unavailable) });
        }
        let generation = self
            .wire
            .generation
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.binding.generation.clone());
        // The real v1 contract rejects local/invented source URIs; validate that seam.
        if matches!(
            call.remote_name.as_str(),
            "mother_propose_memory" | "mother_write_record"
        ) {
            for source in call.arguments["record"]["sources"].as_array().unwrap() {
                assert!(source["uri"].as_str().unwrap().starts_with("https://"));
            }
        }
        if matches!(
            call.remote_name.as_str(),
            "mother_propose_memory" | "mother_write_record"
        ) {
            if let Some(code) = self.wire.reject_write.lock().unwrap().clone() {
                return Box::pin(async move {
                    Ok(ExternalToolResult {
                        structured_data: None,
                        text_blocks: vec![json!({"error":{"code":code}}).to_string()],
                        is_error: true,
                        omitted_blocks: 0,
                        shortened: false,
                    })
                });
            }
        }
        let mut is_error = false;
        let result = match call.remote_name.as_str() {
            "mother_get_context" => {
                json!({"access":{"version":1,"subject":self.binding.subject,"mode":"companion"},"workspace":{"id":self.binding.workspace_id,"generation":generation}})
            }
            "mother_propose_memory" => {
                let args = &call.arguments;
                let proposal = json!({"id":"00000000-0000-4000-8000-000000000010","revision":1,"status":"pending","actor":"synthetic-client","workspaceId":args["workspaceId"],"generation":args["generation"],"source":args["source"],"basis":args["basis"],"record":args["record"],"targetId":args["id"],"expectedRevision":args["expectedRevision"],"conflict":false,"receipt":null});
                let receipt = json!({"operationId":args["operationId"],"generation":generation,"proposal":proposal});
                *self.wire.pending.lock().unwrap() = Some(receipt.clone());
                receipt
            }
            "mother_write_record" => {
                let mut record = call.arguments["record"].clone();
                record["id"] = json!("00000000-0000-4000-8000-000000000012");
                record["revision"] = json!(1);
                record["actor"] = json!("synthetic-client");
                record["createdAt"] = json!("2026-11-14T00:00:00Z");
                record["updatedAt"] = json!("2026-11-14T00:00:00Z");
                json!({"operationId":call.arguments["operationId"],"generation":generation,"committedAt":"2026-11-14T00:00:00Z","record":record})
            }
            "mother_get_receipt" => {
                self.wire
                    .pending
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| {
                        is_error = true;
                        json!({"error":{"code":"NOT_FOUND"}})
                    })
            }
            "mother_get_proposal" => {
                let mut p = self.wire.pending.lock().unwrap().as_ref().unwrap()["proposal"].clone();
                let outcome = self.wire.outcome.lock().unwrap().clone();
                if outcome == "conflict" {
                    p["conflict"] = json!(true);
                } else if matches!(outcome.as_str(), "saved" | "dismissed") {
                    p["status"] = json!(outcome);
                    p["revision"] = json!(2);
                    let op = "00000000-0000-4000-8000-000000000011";
                    let mut record = p["record"].clone();
                    record["id"] = json!("00000000-0000-4000-8000-000000000012");
                    record["revision"] = json!(1);
                    record["actor"] = json!("owner");
                    record["createdAt"] = json!("2026-11-14T00:00:00Z");
                    record["updatedAt"] = json!("2026-11-14T00:00:00Z");
                    record["content"] = json!("Owner edited synthetic fact");
                    p["receipt"] = json!({"proposalId":p["id"],"status":outcome,"operationId":op,"generation":generation,"receipt":if outcome=="saved"{json!({"operationId":op,"generation":generation,"committedAt":"2026-11-14T00:00:00Z","record":record})}else{Value::Null}});
                }
                p
            }
            _ => panic!("unexpected synthetic call"),
        };
        let lose = call.remote_name == "mother_propose_memory"
            && self.wire.lose.swap(false, Ordering::SeqCst);
        let gate = self
            .wire
            .gate
            .lock()
            .unwrap()
            .clone()
            .filter(|(write, _, _)| {
                if *write {
                    matches!(
                        call.remote_name.as_str(),
                        "mother_propose_memory" | "mother_write_record"
                    )
                } else {
                    call.remote_name == "mother_get_context"
                }
            });
        Box::pin(async move {
            if let Some((_, entered, release)) = gate {
                entered.add_permits(1);
                tokio::select! {_=control.cancel.cancelled()=>return Err(McpTransportError::OutcomeUnknown),permit=release.acquire()=>{permit.map_err(|_|McpTransportError::Unavailable)?.forget();}}
            }
            if lose {
                return Err(McpTransportError::OutcomeUnknown);
            }
            Ok(ExternalToolResult {
                structured_data: None,
                text_blocks: vec![result.to_string()],
                is_error,
                omitted_blocks: 0,
                shortened: false,
            })
        })
    }
    fn close(&self) -> BoxFuture<'static, Result<PhysicalClose, McpTransportError>> {
        Box::pin(async { Ok(PhysicalClose::for_test()) })
    }
}
struct Fixture {
    service: Arc<PublicationService>,
    state: SharedDaemonState,
    agent: String,
    selection: Selection,
    wire: Arc<Wire>,
    manager: McpManager,
    _dir: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("publications.sqlite");
        let owner =
            ProfileOwner::acquire_resources([crate::profile_owner::ProfileResource::Sqlite(
                path.clone(),
            )])
            .unwrap();
        let mut daemon = DaemonState::new();
        let agent = daemon
            .create_agent(crate::sessions::test_support::agent_config(
                "Synthetic publication owner",
            ))
            .unwrap()
            .state
            .id;
        daemon.profile_owner = Some(owner.clone());
        let connection = uuid::Uuid::new_v4().to_string();
        let binding:binding::MemoryProviderBinding=serde_json::from_value(json!({"version":1,"provider":"mother","connectionId":connection,"workspaceId":"00000000-0000-4000-8000-000000000001","generation":"00000000-0000-4000-8000-000000000002","subject":agent,"publication":"selected","personaSource":"local","accessMode":"companion"})).unwrap();
        let tools: Vec<_> = [
            "mother_get_context",
            "mother_propose_memory",
            "mother_get_receipt",
            "mother_get_proposal",
            "mother_write_record",
        ]
        .iter()
        .map(|name| {
            prepare_external_tool(
                name,
                "Synthetic publication tool",
                json!({"type":"object","additionalProperties":true}),
                None,
            )
            .unwrap()
        })
        .collect();
        let reviewed: Vec<_> = tools
            .iter()
            .enumerate()
            .map(|(i, t)| ReviewedMcpTool {
                alias: format!("mcp_synthetic_{i}"),
                descriptor: t.clone(),
            })
            .collect();
        let mut config = daemon.agents[&agent].config().clone();
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert(binding::SETTING.into(), binding.portable());
        config.tools = Some(
            reviewed
                .iter()
                .map(ReviewedMcpTool::tool_descriptor)
                .collect(),
        );
        daemon.restore_agent_config(&agent, config);
        daemon.approvals.set_policy(
            &agent,
            crate::approvals::ApprovalPolicy {
                exec: crate::approvals::PolicyAction::Allow,
                ..Default::default()
            },
        );
        daemon.mcp_registry.connections.insert(
            connection.clone(),
            McpConnectionRecord {
                id: connection.clone(),
                agent_id: agent.clone(),
                label: "Synthetic".into(),
                endpoint: "http://127.0.0.1:9999/mcp".into(),
                stdio: None,
                bearer_credential_ref: None,
                oauth_credential_ref: None,
                enabled: true,
                version: 1,
                reviewed_tools: reviewed,
            },
        );
        let state = Arc::new(RwLock::new(daemon));
        let wire = Arc::new(Wire::default());
        let manager = McpManager::new(
            Arc::downgrade(&state),
            Arc::new(Mutex::new(())),
            state.read().await.mcp_publication.clone(),
            Arc::new(Fake {
                wire: wire.clone(),
                tools,
                binding,
            }),
        );
        state.write().await.mcp_manager = Some(manager.clone());
        manager
            .refresh_agent_permissions(&*state.read().await, &agent)
            .unwrap();
        manager.connect(&agent, &connection, 1).await.unwrap();
        let service = PublicationService::open(&state, owner, &path)
            .await
            .unwrap();
        state.write().await.memory_publications = Some(service.clone());
        let memory = state.read().await.memory_handle();
        let source = memory
            .write()
            .await
            .add(anima_memory::NewMemory {
                agent_id: agent.clone(),
                agent_name: "Synthetic".into(),
                memory_type: MemoryType::Fact,
                content: "Synthetic selected fact".into(),
                importance: 0.5,
                tags: None,
                scope: Some(MemoryScope::Shared),
                room_id: None,
                world_id: None,
                session_id: None,
            })
            .unwrap();
        let selection = Selection {
            publication_id: uuid::Uuid::new_v4().to_string(),
            local_id: source.id,
            expected_content: source.content,
            title: "Synthetic title".into(),
            source: "Explicit synthetic selection".into(),
            basis: Basis::OwnerStatement,
            delivery: Delivery::Proposal,
            target: None,
        };
        Self {
            service,
            state,
            agent,
            selection,
            wire,
            manager,
            _dir: dir,
        }
    }
    async fn item(&self) -> Publication {
        self.service
            .get(&self.agent, &self.selection.publication_id)
            .await
            .unwrap()
            .unwrap()
    }
    async fn dispatch(&self) {
        let item = self.item().await;
        self.service.dispatch(item).await;
    }
    async fn due_now(&self) {
        let id = self.selection.publication_id.clone();
        self.service
            .db(move |s| {
                let mut item = s.get(&id)?.unwrap();
                if item.state == PublicationState::Unknown {
                    item = s.transition(
                        &id,
                        item.revision,
                        Transition::Unknown {
                            error: "retry".into(),
                            next_attempt_at_ms: 0,
                        },
                        now_ms(),
                    )?;
                }
                Ok(item)
            })
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn selected_publication_pending_then_owner_edited_saved_has_canonical_mapping() {
    let f = Fixture::new().await;
    let p = f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    assert_eq!(p.state, PublicationState::Queued);
    assert!(f.wire.calls.lock().unwrap().is_empty());
    f.dispatch().await;
    assert_eq!(f.item().await.state, PublicationState::SubmittedForReview);
    *f.wire.outcome.lock().unwrap() = "saved".into();
    f.dispatch().await;
    let saved = f.item().await;
    assert_eq!(saved.state, PublicationState::Saved);
    assert_eq!(summary(&saved)["canonical"]["ownerEdited"], true);
    let id = saved.admission.publication_id;
    assert!(f
        .service
        .db(move |s| s.mapping(&id))
        .await
        .unwrap()
        .is_some());
    f.manager.shutdown().await;
}
#[tokio::test]
async fn selected_publication_lost_response_reconciles_same_uuid_without_second_write() {
    let f = Fixture::new().await;
    f.wire.lose.store(true, Ordering::SeqCst);
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    f.dispatch().await;
    assert_eq!(f.item().await.state, PublicationState::Unknown);
    // Reopen the same owned database as after restart; no pending item is uploaded automatically.
    let path = f._dir.path().join("publications.sqlite");
    let owner = f.state.read().await.profile_owner().unwrap();
    let replacement = PublicationService::open(&f.state, owner, &path)
        .await
        .unwrap();
    f.state.write().await.memory_publications = Some(replacement.clone());
    f.due_now().await;
    let item = f.item().await;
    replacement.dispatch(item).await;
    assert_eq!(f.item().await.state, PublicationState::SubmittedForReview);
    let calls = f.wire.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(name, _)| name == "mother_propose_memory")
            .count(),
        1
    );
    assert!(calls.iter().any(|(name, args)| name == "mother_get_receipt"
        && args["operationId"] == f.selection.publication_id));
    drop(calls);
    f.manager.shutdown().await;
}
#[tokio::test]
async fn selected_publication_ask_and_changed_binding_block_before_transport() {
    let f = Fixture::new().await;
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    f.state
        .write()
        .await
        .approvals
        .set_policy(&f.agent, crate::approvals::ApprovalPolicy::default());
    f.dispatch().await;
    assert_eq!(
        f.item().await.last_error.as_deref(),
        Some("approval_required")
    );
    assert!(f.wire.calls.lock().unwrap().is_empty());
    let prior = f.item().await;
    assert!(f
        .service
        .control(
            &f.agent,
            &f.selection.publication_id,
            prior.revision,
            "resume"
        )
        .await
        .is_err());
    f.state.write().await.approvals.set_policy(
        &f.agent,
        crate::approvals::ApprovalPolicy {
            exec: crate::approvals::PolicyAction::Allow,
            ..Default::default()
        },
    );
    {
        let mut guard = f.state.write().await;
        let mut config = guard.agents[&f.agent].config().clone();
        config
            .settings
            .as_mut()
            .unwrap()
            .additional
            .remove(binding::SETTING);
        guard.restore_agent_config(&f.agent, config);
    }
    assert!(f
        .service
        .control(
            &f.agent,
            &f.selection.publication_id,
            prior.revision,
            "resume"
        )
        .await
        .is_err());
    // A lost admission response is recoverable even after authority changes.
    assert_eq!(
        f.service
            .admit(&f.agent, f.selection.clone())
            .await
            .unwrap(),
        prior
    );
    f.manager.shutdown().await;
}
#[tokio::test]
async fn selected_publication_generation_change_never_writes() {
    let f = Fixture::new().await;
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    *f.wire.generation.lock().unwrap() = Some(uuid::Uuid::new_v4().to_string());
    f.dispatch().await;
    assert_eq!(f.item().await.state, PublicationState::Blocked);
    assert!(f
        .wire
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(name, _)| name == "mother_get_context"));
    f.manager.shutdown().await;
}
#[tokio::test]
async fn selected_publication_dismissal_and_conflict_never_create_mappings() {
    for outcome in ["dismissed", "conflict"] {
        let f = Fixture::new().await;
        f.service
            .admit(&f.agent, f.selection.clone())
            .await
            .unwrap();
        f.dispatch().await;
        *f.wire.outcome.lock().unwrap() = outcome.into();
        f.dispatch().await;
        let p = f.item().await;
        assert_eq!(
            p.state,
            if outcome == "dismissed" {
                PublicationState::Dismissed
            } else {
                PublicationState::Conflict
            }
        );
        let id = p.admission.publication_id;
        assert!(f
            .service
            .db(move |s| s.mapping(&id))
            .await
            .unwrap()
            .is_none());
        f.manager.shutdown().await;
    }
}
#[tokio::test]
async fn selected_publication_stale_source_rejects_without_queue_or_auto_upload() {
    let f = Fixture::new().await;
    let mut selection = f.selection.clone();
    selection.expected_content = "stale text".into();
    assert!(matches!(
        f.service.admit(&f.agent, selection).await,
        Err(PublicationError::Conflict(_))
    ));
    assert_eq!(f.service.list(&f.agent).await.unwrap()["items"], json!([]));
    assert!(f.wire.calls.lock().unwrap().is_empty());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_source_deletion_cancels_never_sent_only() {
    let f = Fixture::new().await;
    let admitted = f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    let memory = f.state.read().await.memory_handle();
    memory.write().await.forget(&f.selection.local_id);
    f.dispatch().await;
    assert_eq!(f.item().await.state, PublicationState::Cancelled);
    assert!(f.wire.calls.lock().unwrap().is_empty());
    let retry = f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    assert_eq!(retry.admission, admitted.admission);
    assert_eq!(retry.state, PublicationState::Cancelled);
    let mut changed = f.selection.clone();
    changed.title = "Different caller payload".into();
    assert!(matches!(
        f.service.admit(&f.agent, changed).await,
        Err(PublicationError::Conflict(_))
    ));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_private_and_room_facts_cannot_be_shared() {
    let f = Fixture::new().await;
    let memory = f.state.read().await.memory_handle();
    let source = memory.read().await.get(&f.selection.local_id).unwrap();
    for scope in [MemoryScope::Private, MemoryScope::Room] {
        let mut altered = source.clone();
        altered.scope = scope;
        assert!(matches!(
            validate_source(&altered, &f.agent, &altered.content),
            Err(PublicationError::Invalid(_))
        ));
    }
    let mut altered = source;
    altered.agent_id = "other-owner".into();
    assert!(validate_source(&altered, &f.agent, &altered.content).is_err());
    assert!(f.wire.calls.lock().unwrap().is_empty());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_stale_control_revision_preserves_item() {
    let f = Fixture::new().await;
    let admitted = f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    assert!(matches!(
        f.service
            .control(
                &f.agent,
                &f.selection.publication_id,
                admitted.revision + 1,
                "cancel"
            )
            .await,
        Err(PublicationError::Conflict(_))
    ));
    assert_eq!(f.item().await, admitted);
    f.manager.shutdown().await;
}

#[test]
fn selected_publication_portable_revision_is_exact_safe_integer() {
    let id = uuid::Uuid::new_v4().to_string();
    for value in [json!(0), json!(-1), json!(1.5), json!(9007199254740992_u64)] {
        assert!(
            serde_json::from_value::<Target>(json!({"id":id,"expectedRevision":value})).is_err()
        );
    }
    assert_eq!(
        serde_json::from_value::<Target>(json!({"id":id,"expectedRevision":1.0}))
            .unwrap()
            .expected_revision,
        1
    );
}

#[tokio::test]
async fn selected_publication_normalizes_wire_but_not_caller_identity() {
    let mut f = Fixture::new().await;
    f.selection.title = "  Synthetic title  ".into();
    f.selection.source = "  Explicit synthetic selection  ".into();
    let p = f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    assert_eq!(p.admission.request["record"]["title"], "Synthetic title");
    assert_eq!(
        p.admission.request["source"],
        "Explicit synthetic selection"
    );
    let mut changed = f.selection.clone();
    changed.title = changed.title.trim().into();
    assert!(matches!(
        f.service.admit(&f.agent, changed).await,
        Err(PublicationError::Conflict(_))
    ));
    f.dispatch().await;
    assert_eq!(f.item().await.state, PublicationState::SubmittedForReview);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_explicit_direct_save_uses_canonical_receipt() {
    let mut f = Fixture::new().await;
    f.selection.delivery = Delivery::Direct;
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    f.dispatch().await;
    let saved = f.item().await;
    assert_eq!(saved.state, PublicationState::Saved);
    assert_eq!(summary(&saved)["canonical"]["ownerEdited"], false);
    assert!(f
        .wire
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(name, _)| name != "mother_propose_memory"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_transient_bootstrap_failure_persists_retry_deadline() {
    let f = Fixture::new().await;
    f.wire.fail_context.store(true, Ordering::SeqCst);
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    f.dispatch().await;
    let retry = f.item().await;
    assert_eq!(retry.state, PublicationState::Queued);
    assert!(retry.next_attempt_at_ms > now_ms());
    assert!(f
        .wire
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(name, _)| name == "mother_get_context"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_denied_policy_and_lost_profile_never_send() {
    let f = Fixture::new().await;
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    f.state.write().await.approvals.set_policy(
        &f.agent,
        crate::approvals::ApprovalPolicy {
            exec: crate::approvals::PolicyAction::Deny,
            ..Default::default()
        },
    );
    f.dispatch().await;
    assert_eq!(f.item().await.state, PublicationState::Blocked);
    assert!(f.wire.calls.lock().unwrap().is_empty());
    f.state.read().await.profile_owner().unwrap().mark_lost();
    assert!(f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .is_err());
    assert!(f.wire.calls.lock().unwrap().is_empty());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_failed_transition_latches_visible_storage_unavailable() {
    let f = Fixture::new().await;
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    f.dispatch().await;
    let pending = f.item().await;
    let id = pending.admission.publication_id.clone();
    let revision = pending.revision;
    let sending = f
        .service
        .db(move |store| store.claim(&id, revision, now_ms()))
        .await
        .unwrap();
    // Exceed a real store transition bound to inject a non-CAS persistence failure.
    f.service
        .change(
            &sending,
            Transition::Unknown {
                error: "x".repeat(1025),
                next_attempt_at_ms: now_ms(),
            },
        )
        .await;
    let view = f.service.list(&f.agent).await.unwrap();
    assert_eq!(view["available"], false);
    assert_eq!(view["reason"], "publication_storage_unavailable");
    assert_eq!(view["items"][0]["state"], "sending");
    let mut other = f.selection.clone();
    other.publication_id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        f.service.admit(&f.agent, other).await,
        Err(PublicationError::Unavailable(_))
    ));
    let count = f.wire.calls.lock().unwrap().len();
    f.dispatch().await;
    assert_eq!(f.wire.calls.lock().unwrap().len(), count);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_remote_record_sources_do_not_invent_https_provenance() {
    let f = Fixture::new().await;
    let p = f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    assert_eq!(p.admission.request["record"]["sources"], json!([]));
    assert_eq!(
        p.admission.source_attribution.as_deref(),
        Some(f.selection.source.as_str())
    );
    assert_eq!(p.admission.request["source"], f.selection.source);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_definitive_invalid_request_never_replays() {
    let f = Fixture::new().await;
    *f.wire.reject_write.lock().unwrap() = Some("INVALID_INPUT".into());
    f.service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    f.dispatch().await;
    assert_eq!(f.item().await.state, PublicationState::Conflict);
    assert_eq!(
        f.item().await.last_error.as_deref(),
        Some("remote_request_rejected:INVALID_INPUT")
    );
    let count = f.wire.calls.lock().unwrap().len();
    f.dispatch().await;
    assert_eq!(f.wire.calls.lock().unwrap().len(), count);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn selected_publication_context_response_revalidates_changed_binding_and_policy() {
    for change_binding in [true, false] {
        let f = Fixture::new().await;
        f.service
            .admit(&f.agent, f.selection.clone())
            .await
            .unwrap();
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        *f.wire.gate.lock().unwrap() = Some((false, entered.clone(), release.clone()));
        let item = f.item().await;
        let service = f.service.clone();
        let task = tokio::spawn(async move { service.dispatch(item).await });
        tokio::time::timeout(Duration::from_secs(3), entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        {
            let mut guard = f.state.write().await;
            if change_binding {
                let mut config = guard.agents[&f.agent].config().clone();
                config
                    .settings
                    .as_mut()
                    .unwrap()
                    .additional
                    .remove(binding::SETTING);
                guard.restore_agent_config(&f.agent, config);
            } else {
                guard.approvals.set_policy(
                    &f.agent,
                    crate::approvals::ApprovalPolicy {
                        exec: crate::approvals::PolicyAction::Deny,
                        ..Default::default()
                    },
                );
            }
        }
        release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        f.dispatch().await;
        assert_eq!(f.item().await.state, PublicationState::Blocked);
        assert!(f
            .wire
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(name, _)| name == "mother_get_context"));
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn selected_publication_shutdown_during_mutation_preserves_unknown_uuid() {
    let f = Fixture::new().await;
    let original = f
        .service
        .admit(&f.agent, f.selection.clone())
        .await
        .unwrap();
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    *f.wire.gate.lock().unwrap() = Some((true, entered.clone(), release));
    let item = f.item().await;
    let service = f.service.clone();
    let task = tokio::spawn(async move { service.dispatch(item).await });
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    f.service.shutdown().await;
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    let unknown = f.item().await;
    assert_eq!(unknown.state, PublicationState::Unknown);
    assert_eq!(unknown.admission, original.admission);
    let count = f.wire.calls.lock().unwrap().len();
    f.dispatch().await;
    assert_eq!(f.wire.calls.lock().unwrap().len(), count);
    assert_eq!(
        f.wire
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "mother_propose_memory")
            .count(),
        1
    );
    f.manager.shutdown().await;
}
