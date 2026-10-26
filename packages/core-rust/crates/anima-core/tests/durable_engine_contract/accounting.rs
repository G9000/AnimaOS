use super::*;
use anima_core::execution_limits::*;

struct AccountingBudget {
    inner: InMemoryExecutionBudget,
    reject_settlement: bool,
}

#[async_trait]
impl ExecutionBudgetPort for AccountingBudget {
    async fn reserve(
        &self,
        scope: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        self.inner.reserve(scope, request).await
    }

    async fn allowance(&self, scope: &str) -> Result<ExecutionAllowance, String> {
        self.inner.allowance(scope).await
    }

    fn submit_settlement(
        &self,
        scope: &str,
        settlement: ReservationSettlement,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        if self.reject_settlement {
            Box::pin(async { Err("fixture accounting commit failed".into()) })
        } else {
            self.inner.submit_settlement(scope, settlement)
        }
    }
}

struct AccountedModel {
    tool: Option<ToolCall>,
    stop: Option<MutableBoundarySignal>,
}

#[async_trait]
impl ModelAdapter for AccountedModel {
    fn provider(&self) -> &str {
        "deterministic"
    }

    fn single_attempt_contract(&self) -> bool {
        true
    }

    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }

    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        if let Some(stop) = &self.stop {
            stop.set(EngineBoundaryAction::Cancel);
        }
        Ok(ModelGenerateResponse {
            content: Content {
                text: "accounted response".into(),
                ..Content::default()
            },
            tool_calls: self.tool.clone().map(|tool| vec![tool]),
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                ..TokenUsage::default()
            },
            stop_reason: if self.tool.is_some() {
                ModelStopReason::ToolCall
            } else {
                ModelStopReason::End
            },
        })
    }
}

async fn controlled_accounting_scenario(reject_settlement: bool, tool: bool, stop: bool) {
    let clock = Arc::new(ManualExecutionClock::new(1_000));
    let capability_manifest = manifest(RecoveryMode::KeyedIdempotent);
    let mut definition = definition();
    definition.resolved_capabilities = vec![ResolvedCapability {
        capability_id: capability_manifest.id.clone(),
        manifest_version: capability_manifest.version,
        schema_digest: capability_manifest.schema_digest().into(),
        override_config: None,
        approval_policy_revision: 1,
    }];
    let (owner, run, store) = create_engine_run(0xa000, &definition, clock.clone()).await;
    let scope = ExecutionScope::new(
        "controlled-accounting".into(),
        run.to_string(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let port = Arc::new(AccountingBudget {
        inner: InMemoryExecutionBudget::new(scope.clone(), clock.clone()).unwrap(),
        reject_settlement,
    });
    let execution = ExecutionContext {
        scope_id: scope.id.clone(),
        run_id: run.to_string(),
        port: port.clone(),
        clock: clock.clone(),
        cancel: anima_core::CancelSignal::new(),
        deadline_at_ms: scope.deadline_at_ms,
        failures: Arc::new(Mutex::new(None)),
        secondary_cancel: None,
    };
    let signal = MutableBoundarySignal::new();
    let model = Arc::new(AccountedModel {
        tool: tool.then(|| ToolCall {
            id: "must-wait-for-accounting".into(),
            name: capability_manifest.id.clone(),
            args: BTreeMap::from([("path".into(), DataValue::String("note.txt".into()))]),
        }),
        stop: stop.then(|| signal.clone()),
    });
    let capabilities = Arc::new(CountingCapabilities {
        manifest: capability_manifest,
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let live = Arc::new(RecordingLiveSink::default());
    let engine = DurableAgentEngine::new(
        store.clone(),
        model,
        Arc::new(StaticDefinitions(definition)),
        Arc::new(AllowPolicy),
        capabilities.clone(),
        clock,
        live.clone(),
        Arc::new(NeverCrash),
        DurableEngineConfig::default(),
    )
    .unwrap();

    let outcome = engine.run_controlled(owner, run, &signal, &execution).await;
    assert_eq!(capabilities.calls.load(Ordering::SeqCst), 0);
    if reject_settlement {
        assert_eq!(
            outcome,
            Err(EngineError::new(EngineErrorCode::Model)),
            "model success cannot cross a failed accounting acknowledgement"
        );
        assert!(execution
            .failure()
            .unwrap()
            .contains("accounting commit failed"));
        assert_eq!(
            store
                .load_run(owner, run)
                .await
                .unwrap()
                .unwrap()
                .run()
                .state(),
            RunState::Running
        );
        let (_, checkpoint) = store.load_checkpoint(owner, run).await.unwrap().unwrap();
        assert!(checkpoint.provider_transcript().is_empty());
        assert_eq!(checkpoint.usage().turns(), 0);
        assert!(store
            .load_completed_output(owner, run)
            .await
            .unwrap()
            .is_none());
        let events = store
            .replay_events(owner, run, StoreReadPage::first(256).unwrap())
            .await
            .unwrap();
        assert_eq!(
            events
                .events()
                .iter()
                .map(|event| event.kind())
                .collect::<Vec<_>>(),
            vec![RuntimeEventKind::RunStarted, RuntimeEventKind::ModelStarted]
        );
        assert!(live.0.lock().unwrap().iter().all(|event| !matches!(
            event,
            EngineLiveEvent::Semantic {
                kind: RuntimeEventKind::ModelCompleted
                    | RuntimeEventKind::RunCompleted
                    | RuntimeEventKind::InvocationDispatchPrepared,
                ..
            }
        )));
    } else {
        assert!(execution.failure().is_none());
        if stop {
            assert_eq!(outcome.unwrap(), EngineRunOutcome::Cancelled);
        } else {
            assert!(matches!(
                outcome.unwrap(),
                EngineRunOutcome::Completed { .. }
            ));
        }
    }
    let consumption = port
        .inner
        .snapshot(&scope.id)
        .unwrap()
        .consumption()
        .unwrap();
    assert_eq!(consumption.model_attempts, 1);
    if reject_settlement {
        assert_eq!(consumption.inflight_operations, 1);
        assert_eq!(consumption.known.total_tokens, 0);
        assert!(consumption.charged_total_tokens > 12);
    } else {
        assert_eq!(consumption.inflight_operations, 0);
        assert_eq!(consumption.known.total_tokens, 12);
        assert_eq!(consumption.charged_total_tokens, 12);
    }
}

#[tokio::test]
async fn controlled_model_accounting_failure_cannot_publish_completion() {
    controlled_accounting_scenario(true, false, false).await;
}

#[tokio::test]
async fn controlled_model_accounting_failure_cannot_publish_or_dispatch_a_tool() {
    controlled_accounting_scenario(true, true, false).await;
}

#[tokio::test]
async fn controlled_model_success_keeps_its_acknowledged_usage() {
    controlled_accounting_scenario(false, false, false).await;
}

#[tokio::test]
async fn controlled_model_stop_after_response_keeps_its_acknowledged_usage() {
    controlled_accounting_scenario(false, false, true).await;
}
