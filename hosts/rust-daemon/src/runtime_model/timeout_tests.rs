//! M47 real host-adapter deadline evidence against joined local HTTP only.
//! No external provider, environment credential or auth/vault lookup occurs.
use super::*;
use anima_core::{CancelSignal, ModelStreamFrame, ModelStreamSink};
use axum::{
    body::{Body, Bytes},
    extract::Json,
    response::IntoResponse,
    routing::post,
    Router,
};
use futures::StreamExt;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::Semaphore;

struct Fixture {
    adapter: RuntimeModelAdapter,
    attempts: Arc<AtomicUsize>,
    entered: Arc<Semaphore>,
    release: CancelSignal,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<Result<(), std::io::Error>>>,
}
impl Fixture {
    async fn new() -> Self {
        let attempts = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Semaphore::new(0));
        let release = CancelSignal::new();
        let app=Router::new().route("/v1/chat/completions",post({let attempts=attempts.clone();let entered=entered.clone();let release=release.clone();move |Json(body):Json<Value>|{let attempts=attempts.clone();let entered=entered.clone();let release=release.clone();async move {
            attempts.fetch_add(1,Ordering::SeqCst);entered.add_permits(1);
            if body["stream"]==true {
                let head=futures::stream::once(async{Ok::<_,Infallible>(Bytes::from_static(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n"))});
                let tail=futures::stream::once(async move{release.cancelled().await;Ok::<_,Infallible>(Bytes::from_static(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))});
                axum::http::Response::builder().header("content-type","text/event-stream").body(Body::from_stream(head.chain(tail))).unwrap()
            } else {
                release.cancelled().await;
                Json(json!({"choices":[{"message":{"content":"controlled complete"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})).into_response()
            }
        }}}));
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        let adapter = RuntimeModelAdapter::with_config(ProviderAdapterConfig {
            providers: BTreeMap::from([(
                "vllm".into(),
                ProviderCredential {
                    api_key: None,
                    base_url: format!("{origin}/v1"),
                },
            )]),
            ..ProviderAdapterConfig::default()
        });
        Self {
            adapter,
            attempts,
            entered,
            release,
            stop: Some(stop),
            task: Some(task),
        }
    }
    async fn finish(mut self) {
        self.release.cancel();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        tokio::time::timeout(Duration::from_secs(2), self.task.take().unwrap())
            .await
            .expect("controlled server joins")
            .unwrap()
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.release.cancel();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
fn config(timeout: Option<u64>) -> AgentConfig {
    let mut config = crate::agent_runs::test_support::companion_config("M47 controlled deadline");
    config.provider = Some("vllm".into());
    config.tools = None;
    let settings = config.settings.as_mut().unwrap();
    settings.timeout_ms = timeout;
    settings.max_retries = Some(0);
    config
}
fn request() -> ModelGenerateRequest {
    ModelGenerateRequest {
        system: "Controlled local deadline test".into(),
        messages: vec![],
        temperature: None,
        max_tokens: Some(16),
    }
}
#[derive(Default)]
struct Frames(Mutex<Vec<ModelStreamFrame>>);
#[async_trait]
impl ModelStreamSink for Frames {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        self.0.lock().unwrap().push(frame);
        Ok(())
    }
}

#[tokio::test]
async fn m47_coding_runtime_model_generate_timeout_bounds_one_http_attempt() {
    let fixture = Fixture::new().await;
    let config = config(Some(100));
    let request = request();
    let result = tokio::time::timeout(
        Duration::from_millis(750),
        fixture.adapter.generate(&config, &request),
    )
    .await;
    let attempts = fixture.attempts.load(Ordering::SeqCst);
    fixture.finish().await;
    assert_eq!(attempts, 1, "one real request and no automatic retry");
    assert_eq!(
        result
            .expect("configured generate timeout must bound the actual pending HTTP call")
            .unwrap_err(),
        "Model call timed out"
    );
}

#[tokio::test]
async fn m47_coding_runtime_model_partial_stream_timeout_has_no_false_final_or_retry() {
    let fixture = Fixture::new().await;
    let config = config(Some(100));
    let request = request();
    let frames = Frames::default();
    let result = tokio::time::timeout(
        Duration::from_millis(750),
        fixture.adapter.stream(&config, &request, &frames),
    )
    .await;
    let attempts = fixture.attempts.load(Ordering::SeqCst);
    let before = frames.0.lock().unwrap().clone();
    fixture.finish().await;
    assert_eq!(attempts, 1);
    assert_eq!(before, vec![ModelStreamFrame::TextDelta("partial".into())]);
    assert_eq!(
        *frames.0.lock().unwrap(),
        before,
        "released fixture cannot publish a late Final after timeout"
    );
    assert_eq!(
        result
            .expect("configured stream timeout must bound the actual pending body")
            .unwrap_err(),
        "Model call timed out"
    );
}

#[tokio::test]
async fn m47_coding_runtime_model_absent_timeout_preserves_generate_and_stream() {
    for streaming in [false, true] {
        let fixture = Fixture::new().await;
        fixture.release.cancel();
        let config = config(None);
        let request = request();
        let frames = Frames::default();
        if streaming {
            let result = fixture.adapter.stream(&config, &request, &frames).await;
            let attempts = fixture.attempts.load(Ordering::SeqCst);
            fixture.finish().await;
            result.unwrap();
            assert_eq!(attempts, 1);
            assert!(
                matches!(frames.0.lock().unwrap().last(),Some(ModelStreamFrame::Final(response))if response.content.text=="partialdone")
            );
        } else {
            let result = fixture.adapter.generate(&config, &request).await;
            let attempts = fixture.attempts.load(Ordering::SeqCst);
            fixture.finish().await;
            assert_eq!(result.unwrap().content.text, "controlled complete");
            assert_eq!(attempts, 1);
        }
    }
}

#[tokio::test]
async fn m47_coding_runtime_model_dropped_stream_remains_cancellable_before_deadline() {
    let fixture = Fixture::new().await;
    let config = config(Some(5_000));
    let request = request();
    let frames = Frames::default();
    let mut call = Box::pin(fixture.adapter.stream(&config, &request, &frames));
    let entered=tokio::time::timeout(Duration::from_millis(750),async{tokio::select!{permit=fixture.entered.acquire()=>{permit.unwrap().forget();true},_=&mut call=>false}}).await;
    drop(call);
    let attempts = fixture.attempts.load(Ordering::SeqCst);
    fixture.finish().await;
    assert!(matches!(entered, Ok(true)));
    assert_eq!(attempts, 1);
    assert!(
        !frames
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|frame| matches!(frame, ModelStreamFrame::Final(_))),
        "dropping for outer Stop/cancellation cannot manufacture completion"
    );
}
