use super::*;
use anima_core::execution_limits::{
    ExecutionContext, ExecutionLimits, ExecutionScope, InMemoryExecutionBudget, ModelCallContext,
    SystemBudgetClock,
};
use anima_core::{CancelSignal, ExecutionClock};

fn controlled(attempts: u64, retries: u64) -> (ModelCallContext, Arc<InMemoryExecutionBudget>) {
    let clock: Arc<dyn ExecutionClock> = Arc::new(SystemBudgetClock);
    let scope = ExecutionScope::new(
        "native-scope".into(),
        "native-run".into(),
        ExecutionLimits {
            max_model_attempts: attempts,
            max_retries: retries,
            ..ExecutionLimits::default()
        },
        clock.now_ms(),
    )
    .unwrap();
    let port = Arc::new(InMemoryExecutionBudget::new(scope.clone(), clock.clone()).unwrap());
    (
        ModelCallContext::new(
            ExecutionContext {
                scope_id: scope.id,
                run_id: scope.root_run_id,
                port: port.clone(),
                clock,
                cancel: CancelSignal::new(),
                deadline_at_ms: scope.deadline_at_ms,
                failures: Arc::new(Mutex::new(None)),
                secondary_cancel: None,
            },
            "same-logical-call".into(),
        ),
        port,
    )
}

#[tokio::test]
async fn execution_limits_native_transport_retry_consumes_another_shared_attempt() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let app = Router::new().route("/v1/chat/completions", post(move || {
        let seen = seen.clone();
        async move {
            let n = seen.fetch_add(1, Ordering::SeqCst);
            if n == 0 { (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"retry"}))) }
            else { (axum::http::StatusCode::OK, Json(json!({"choices":[{"message":{"content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}))) }
        }
    }));
    let base = spawn_server(app).await;
    let adapter = adapter_with(&[("openai", Some("test-key"), &format!("{base}/v1"))]);
    let (context, port) = controlled(2, 1);
    let sink = FrameSink(Mutex::new(Vec::new()));
    adapter
        .stream_controlled(&agent_config("openai", false), &request(), &sink, &context)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let usage = port
        .snapshot("native-scope")
        .unwrap()
        .consumption()
        .unwrap();
    assert_eq!(
        (
            usage.model_attempts,
            usage.retries,
            usage.inflight_operations
        ),
        (2, 1, 0)
    );
    assert_eq!(usage.known.total_tokens, 7);
}

#[tokio::test]
async fn execution_limits_native_transport_cannot_retry_the_last_slot() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"retry"})),
                )
            }
        }),
    );
    let base = spawn_server(app).await;
    let adapter = adapter_with(&[("openai", Some("test-key"), &format!("{base}/v1"))]);
    let (context, port) = controlled(1, 0);
    assert!(adapter
        .stream_controlled(
            &agent_config("openai", false),
            &request(),
            &FrameSink(Mutex::new(Vec::new())),
            &context
        )
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        port.snapshot("native-scope")
            .unwrap()
            .consumption()
            .unwrap()
            .model_attempts,
        1
    );
}
