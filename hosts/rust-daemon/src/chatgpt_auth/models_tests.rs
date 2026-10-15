use super::super::{
    AuthTransport, Device, InMemoryOAuthAppVault, OAuthVaultBackend, UnavailableTransport, ACCOUNT,
    SERVICE,
};
use super::*;
use async_trait::async_trait;
use serde_json::json;
use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    },
};
use tokio::sync::Semaphore;

fn credential(account: &str, expired: bool) -> Credential {
    Credential {
        access_token: format!("secret-token-{account}"),
        refresh_token: "secret-refresh".into(),
        account_id: account.into(),
        plan_type: None,
        expires_at_ms: if expired {
            0
        } else {
            super::super::now_ms() + 3_600_000
        },
    }
}
struct FakeModels {
    calls: AtomicUsize,
    active: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
    blocked: AtomicBool,
    answers: StdMutex<VecDeque<Result<Vec<String>, String>>>,
    diagnostics: StdMutex<VecDeque<ModelsDiagnostic>>,
}
impl FakeModels {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            blocked: AtomicBool::new(false),
            answers: StdMutex::new(VecDeque::new()),
            diagnostics: StdMutex::new(VecDeque::new()),
        })
    }
    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(1), self.entered.acquire())
            .await
            .expect("fake enters")
            .unwrap()
            .forget();
    }
    fn answer(&self, value: Result<Vec<String>, String>) {
        self.answers.lock().unwrap().push_back(value);
    }
    fn answer_diagnostic(&self, diagnostic: ModelsDiagnostic) {
        self.diagnostics.lock().unwrap().push_back(diagnostic);
    }
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl ModelsTransport for FakeModels {
    async fn fetch(&self, _: String, _: String) -> Result<Vec<String>, ModelsFetchFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_add(1, Ordering::SeqCst);
        let _active = Active(&self.active);
        self.entered.add_permits(1);
        if self.blocked.load(Ordering::SeqCst) {
            self.release.acquire().await.unwrap().forget();
        }
        if let Some(diagnostic) = self.diagnostics.lock().unwrap().pop_front() {
            return Err(ModelsFetchFailure::Synthetic(diagnostic));
        }
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(vec!["synthetic-model-a".into(), "synthetic-model-b".into()]))
            .map_err(|error| {
                ModelsFetchFailure::Synthetic(if error == TIMEOUT {
                    ModelsDiagnostic::new(
                        Stage::Request,
                        Dispatch::Attempted,
                        Reason::Timeout,
                        None,
                    )
                } else if error == INVALID {
                    ModelsDiagnostic::new(
                        Stage::Decode,
                        Dispatch::ResponseReceived,
                        Reason::InvalidCatalog,
                        Some(200),
                    )
                } else {
                    ModelsDiagnostic::new(
                        Stage::Request,
                        Dispatch::Attempted,
                        Reason::Transport,
                        None,
                    )
                })
            })
    }
}
fn service() -> (ChatGptAuth, Arc<FakeModels>) {
    let fake = FakeModels::new();
    let mut service = ChatGptAuth::with_dependencies(
        Arc::new(InMemoryOAuthAppVault::default()),
        Arc::new(UnavailableTransport),
    );
    service.models_transport = fake.clone();
    (service, fake)
}
fn assert_private(snapshot: &ChatGptModelsSnapshot) {
    let public = serde_json::to_string(snapshot).unwrap();
    for private in [
        "secret-token",
        "secret-refresh",
        "account-a",
        "account-b",
        "generation",
        "manifest",
    ] {
        assert!(
            !public.contains(private),
            "private auth context must not escape"
        );
    }
}

#[tokio::test]
async fn m49_chatgpt_models_cache_disconnected_ready_empty_expiry_and_failures() {
    let (service, fake) = service();
    assert_eq!(service.models().await.unwrap().state, "disconnected");
    assert_eq!(
        service.refresh_models().await.unwrap().state,
        "disconnected"
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    service.save(&credential("account-a", false)).unwrap();
    let initial = service.models().await.unwrap();
    assert_eq!(initial.state, "unrequested");
    assert!(initial.connection_revision.is_some());
    assert!(initial.models.is_empty());
    let ready = service.refresh_models().await.unwrap();
    assert_eq!(ready.state, "ready");
    assert_eq!(ready.models.len(), 2);
    assert_eq!(ready.connection_revision, initial.connection_revision);
    assert_private(&ready);
    let cached = service.models().await.unwrap();
    assert_eq!(cached.catalog_revision, ready.catalog_revision);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    service.models_cache.lock().await.expires = Some(Instant::now() - Duration::from_secs(1));
    let expired = service.models().await.unwrap();
    assert_eq!(expired.state, "stale");
    assert_eq!(expired.catalog_revision, ready.catalog_revision);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    for error in [UNAVAILABLE, TIMEOUT, INVALID, "secret upstream detail"] {
        fake.answer(Err(error.into()));
        let stale = service.refresh_models().await.unwrap();
        assert_eq!(stale.state, "stale");
        assert_eq!(stale.catalog_revision, ready.catalog_revision);
        assert_eq!(stale.models.len(), 2);
        assert_eq!(
            stale.error,
            Some(if error == TIMEOUT {
                TIMEOUT
            } else if error == INVALID {
                INVALID
            } else {
                UNAVAILABLE
            })
        );
        assert_private(&stale);
    }
    fake.answer(Ok(vec![]));
    let empty = service.refresh_models().await.unwrap();
    assert_eq!(empty.state, "empty");
    assert!(empty.models.is_empty());
    assert!(empty.error.is_none());
    assert_ne!(empty.catalog_revision, ready.catalog_revision);
    let restored =
        ChatGptAuth::with_dependencies(service.vault.clone(), Arc::new(UnavailableTransport));
    assert_eq!(
        restored.models().await.unwrap().state,
        "unrequested",
        "new auth instance has no restored catalog or request"
    );
}

#[tokio::test]
async fn m49_chatgpt_models_replacement_aba_and_disconnect_fence_late_success_and_failure() {
    for failure in [false, true] {
        let (service, fake) = service();
        service.save(&credential("account-a", false)).unwrap();
        let old = service.refresh_models().await.unwrap();
        fake.blocked.store(true, Ordering::SeqCst);
        // Discard the ready request's entry witness before the blocked one.
        fake.entered.acquire().await.unwrap().forget();
        if failure {
            fake.answer(Err("secret late failure".into()));
        }
        let pending = {
            let service = service.clone();
            tokio::spawn(async move { service.refresh_models().await })
        };
        fake.entered().await;
        service.cancel(false).await.unwrap(); // A -> B -> A lifecycle, same credential identity.
        service.save(&credential("account-b", false)).unwrap();
        service.save(&credential("account-a", false)).unwrap();
        let current = service.models().await.unwrap();
        assert_eq!(current.state, "unrequested");
        assert_ne!(current.connection_revision, old.connection_revision);
        fake.release.add_permits(1);
        let late = pending.await.unwrap().unwrap();
        assert_eq!(late.state, "unrequested");
        assert!(late.models.is_empty());
        assert!(late.error.is_none());
        assert_eq!(late.connection_revision, current.connection_revision);
        assert_private(&late);
        fake.blocked.store(false, Ordering::SeqCst);
        let new = service.refresh_models().await.unwrap();
        assert_eq!(new.state, "ready");
        assert_ne!(new.catalog_revision, old.catalog_revision);
        service.cancel(true).await.unwrap();
        let disconnected = service.models().await.unwrap();
        assert_eq!(disconnected.state, "disconnected");
        assert!(disconnected.connection_revision.is_none());
        assert!(disconnected.models.is_empty());
    }
}

#[tokio::test]
async fn m49_chatgpt_models_same_account_new_manifest_never_reuses_rows_or_dispatch_stamp() {
    let (service, fake) = service();
    service.save(&credential("account-a", false)).unwrap();
    let old = service.refresh_models().await.unwrap();
    let context = service.usable_context(true).await.unwrap().unwrap();
    service.save(&credential("account-a", false)).unwrap(); // External principal replacement need not change lifecycle generation.
    assert!(service
        .begin_models(&context.stamp)
        .await
        .unwrap()
        .is_none());
    let current = service.models().await.unwrap();
    assert_eq!(current.state, "unrequested");
    assert_ne!(old.connection_revision, current.connection_revision);
    assert!(current.models.is_empty());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn m49_chatgpt_models_disconnect_discards_pending_results_without_resurrection() {
    for fail in [false, true] {
        let (service, fake) = service();
        service.save(&credential("account-a", false)).unwrap();
        fake.blocked.store(true, Ordering::SeqCst);
        if fail {
            fake.answer(Err("secret late error".into()));
        }
        let pending = {
            let service = service.clone();
            tokio::spawn(async move { service.refresh_models().await })
        };
        fake.entered().await;
        service.cancel(true).await.unwrap();
        assert_eq!(service.models().await.unwrap().state, "disconnected");
        fake.release.add_permits(1);
        let result = pending.await.unwrap().unwrap();
        assert_eq!(result.state, "disconnected");
        assert!(result.models.is_empty());
        assert!(result.error.is_none());
        assert!(result.catalog_revision.is_none());
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn m49_chatgpt_models_busy_drop_and_total_deadline_do_not_queue_or_retry() {
    let (service, fake) = service();
    service.save(&credential("account-a", false)).unwrap();
    fake.blocked.store(true, Ordering::SeqCst);
    let pending = {
        let service = service.clone();
        tokio::spawn(async move { service.refresh_models().await })
    };
    fake.entered().await;
    assert!(matches!(
        service.refresh_models().await,
        Err(ModelsError::Busy)
    ));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(100), service.models())
            .await
            .expect("GET holds no network lock")
            .unwrap()
            .state,
        "unrequested"
    );
    pending.abort();
    assert!(matches!(pending.await,Err(error) if error.is_cancelled()));
    assert_eq!(
        fake.active.load(Ordering::SeqCst),
        0,
        "dropping request drops local HTTP future"
    );
    let timed = service
        .refresh_models_until(Instant::now() + Duration::from_millis(40))
        .await
        .unwrap();
    assert_eq!(timed.state, "unavailable");
    assert_eq!(timed.error, Some(TIMEOUT));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    assert_eq!(fake.active.load(Ordering::SeqCst), 0);
    fake.blocked.store(false, Ordering::SeqCst);
    assert_eq!(service.refresh_models().await.unwrap().state, "ready");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
}

struct RefreshAuth {
    calls: AtomicUsize,
}
#[async_trait]
impl AuthTransport for RefreshAuth {
    async fn start(&self) -> Result<Device, String> {
        Err("no device authorization permitted".into())
    }
    async fn poll(&self, _: &Device) -> Result<Option<Credential>, String> {
        Err("no device poll permitted".into())
    }
    async fn refresh(&self, _: &Credential) -> Result<Credential, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(credential("account-a", false))
    }
}
#[tokio::test]
async fn m49_chatgpt_models_token_refresh_binds_the_written_manifest_and_preserves_tuple_wrapper() {
    let auth = Arc::new(RefreshAuth {
        calls: AtomicUsize::new(0),
    });
    let fake = FakeModels::new();
    let mut service =
        ChatGptAuth::with_dependencies(Arc::new(InMemoryOAuthAppVault::default()), auth.clone());
    service.models_transport = fake.clone();
    service.save(&credential("account-a", true)).unwrap();
    let old = service.models().await.unwrap();
    let ready = service.refresh_models().await.unwrap();
    assert_eq!(ready.state, "ready");
    assert_ne!(old.connection_revision, ready.connection_revision);
    assert_eq!(auth.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    let context = service.usable_context(true).await.unwrap().unwrap();
    assert!(service
        .begin_models(&context.stamp)
        .await
        .unwrap()
        .is_some());
    let (token, account) = service.usable_credential().await.unwrap();
    assert!(
        token == "secret-token-account-a" && account == "account-a",
        "inference retains its exact token/account tuple"
    );
    assert_eq!(auth.calls.load(Ordering::SeqCst), 1);
    assert_private(&ready);
}

struct HeldAuth {
    entered: Semaphore,
    calls: AtomicUsize,
}
#[async_trait]
impl AuthTransport for HeldAuth {
    async fn start(&self) -> Result<Device, String> {
        Err("no device start".into())
    }
    async fn poll(&self, _: &Device) -> Result<Option<Credential>, String> {
        Err("no poll".into())
    }
    async fn refresh(&self, _: &Credential) -> Result<Credential, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        std::future::pending().await
    }
}
#[tokio::test]
async fn m49_chatgpt_models_total_deadline_includes_auth_resolution_and_never_starts_catalog() {
    let auth = Arc::new(HeldAuth {
        entered: Semaphore::new(0),
        calls: AtomicUsize::new(0),
    });
    let fake = FakeModels::new();
    let mut service =
        ChatGptAuth::with_dependencies(Arc::new(InMemoryOAuthAppVault::default()), auth.clone());
    service.models_transport = fake.clone();
    service.save(&credential("account-a", true)).unwrap();
    let result = service
        .refresh_models_until(Instant::now() + Duration::from_millis(40))
        .await
        .unwrap();
    assert_eq!(result.state, "unavailable");
    assert_eq!(result.error, Some(TIMEOUT));
    assert_eq!(auth.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert!(
        service.refresh_lock.try_lock().is_ok(),
        "cancelled auth resolution releases existing serialization guard"
    );
}

fn assert_scope_free_timeout(snapshot: &ChatGptModelsSnapshot) {
    assert_eq!(snapshot.state, "unavailable");
    assert_eq!(snapshot.error, Some(TIMEOUT));
    assert!(snapshot.connection_revision.is_none());
    assert!(snapshot.catalog_revision.is_none());
    assert!(snapshot.fetched_at_ms.is_none());
    assert!(snapshot.expires_at_ms.is_none());
    assert!(
        snapshot.models.is_empty(),
        "unvalidated scope exposes no cached rows"
    );
    assert_private(snapshot);
}

#[tokio::test]
async fn m49_chatgpt_models_deadline_initial_lifecycle_and_cache_contention_are_bounded() {
    for hold_cache in [false, true] {
        let (service, fake) = service();
        service.save(&credential("account-a", false)).unwrap();
        let previous = service.refresh_models().await.unwrap();
        assert_eq!(previous.state, "ready");
        service.save(&credential("account-b", false)).unwrap();
        let lifecycle = if hold_cache {
            None
        } else {
            Some(service.lifecycle.lock().await)
        };
        let cache = if hold_cache {
            Some(service.models_cache.lock().await)
        } else {
            None
        };
        let deadline = Instant::now() + Duration::from_millis(100);
        // Keep the contested lock held through both deadlines. The larger bound
        // fails the old unbounded acquisition without hanging the test process.
        let outcome = tokio::time::timeout(
            Duration::from_secs(1),
            service.refresh_models_until(deadline),
        )
        .await;
        let snapshot = outcome
            .expect("refresh deadline must return while the initial lock remains held")
            .unwrap();
        assert!(Instant::now() >= deadline);
        assert_scope_free_timeout(&snapshot);
        assert!(
            service.models_refresh.try_lock().is_ok(),
            "deadline releases single-flight without cleanup awaiting the held lock"
        );
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            1,
            "initial contention starts no metadata request or retry"
        );
        drop(cache);
        drop(lifecycle);
        let current = tokio::time::timeout(Duration::from_secs(1), service.models())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.state, "unrequested");
        assert!(current.models.is_empty());
        fake.answer(Ok(vec!["synthetic-current-model".into()]));
        let ready = tokio::time::timeout(Duration::from_secs(1), service.refresh_models())
            .await
            .unwrap()
            .unwrap();
        tokio::task::yield_now().await;
        let settled = tokio::time::timeout(Duration::from_secs(1), service.models())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(settled.catalog_revision, ready.catalog_revision);
        assert_eq!(settled.models[0].id, "synthetic-current-model");
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            2,
            "only the later explicit refresh dispatches"
        );
    }
}

#[tokio::test]
async fn m49_chatgpt_models_deadline_publication_lifecycle_and_cache_contention_discard_late_success(
) {
    for hold_cache in [false, true] {
        let (service, fake) = service();
        service.save(&credential("account-a", false)).unwrap();
        assert_eq!(service.refresh_models().await.unwrap().state, "ready");
        fake.entered.acquire().await.unwrap().forget();
        fake.blocked.store(true, Ordering::SeqCst);
        fake.answer(Ok(vec!["synthetic-obsolete-model".into()]));
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut refresh = Box::pin(service.refresh_models_until(deadline));
        tokio::select! {
            _ = fake.entered() => {},
            _ = &mut refresh => panic!("refresh must wait for the controlled metadata response"),
        }
        // Dispatch is complete. Block only publication, replace the principal,
        // and let the fake return a successful old-scope catalog before expiry.
        let lifecycle = if hold_cache {
            None
        } else {
            Some(service.lifecycle.lock().await)
        };
        let cache = if hold_cache {
            Some(service.models_cache.lock().await)
        } else {
            None
        };
        service.save(&credential("account-b", false)).unwrap();
        assert!(
            Instant::now() < deadline,
            "publication barrier must be installed before expiry"
        );
        fake.release.add_permits(1);
        let outcome = tokio::time::timeout(Duration::from_secs(2), &mut refresh).await;
        // Cancellation is synchronous if the regression reaches the outer bound;
        // no spawned refresh or asynchronous cleanup survives the test future.
        drop(refresh);
        let snapshot = outcome
            .expect("refresh deadline must return while publication remains locked")
            .unwrap();
        assert!(Instant::now() >= deadline);
        assert_scope_free_timeout(&snapshot);
        assert!(
            service.models_refresh.try_lock().is_ok(),
            "publication timeout releases single-flight while the lock is held"
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
        assert_eq!(fake.active.load(Ordering::SeqCst), 0);
        drop(cache);
        drop(lifecycle);
        let current = tokio::time::timeout(Duration::from_secs(1), service.models())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.state, "unrequested");
        assert!(
            current.models.is_empty(),
            "old success cannot reappear after releasing publication"
        );
        fake.blocked.store(false, Ordering::SeqCst);
        fake.answer(Ok(vec!["synthetic-current-model".into()]));
        let ready = tokio::time::timeout(Duration::from_secs(1), service.refresh_models())
            .await
            .unwrap()
            .unwrap();
        tokio::task::yield_now().await;
        let settled = tokio::time::timeout(Duration::from_secs(1), service.models())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(settled.state, "ready");
        assert_eq!(settled.catalog_revision, ready.catalog_revision);
        assert_eq!(settled.models[0].id, "synthetic-current-model");
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            3,
            "expired publication performs no retry or delayed overwrite"
        );
    }
}

struct ReadFailureVault {
    inner: InMemoryOAuthAppVault,
    fail: AtomicBool,
}
impl OAuthVaultBackend for ReadFailureVault {
    fn load(
        &self,
        s: &str,
        a: &str,
    ) -> Result<Option<zeroize::Zeroizing<String>>, crate::connectors::oauth_apps::OAuthVaultError>
    {
        if self.fail.load(Ordering::SeqCst) {
            return Err(crate::connectors::oauth_apps::OAuthVaultError::Unavailable);
        }
        self.inner.load(s, a)
    }
    fn put(
        &self,
        s: &str,
        a: &str,
        p: &str,
    ) -> Result<(), crate::connectors::oauth_apps::OAuthVaultError> {
        self.inner.put(s, a, p)
    }
    fn delete(
        &self,
        s: &str,
        a: &str,
    ) -> Result<(), crate::connectors::oauth_apps::OAuthVaultError> {
        self.inner.delete(s, a)
    }
}
#[tokio::test]
async fn m49_chatgpt_models_vault_failure_forgets_cached_authority_and_auth_failure_is_not_empty_success(
) {
    let vault = Arc::new(ReadFailureVault {
        inner: InMemoryOAuthAppVault::default(),
        fail: AtomicBool::new(false),
    });
    let fake = FakeModels::new();
    let mut service = ChatGptAuth::with_dependencies(vault.clone(), Arc::new(UnavailableTransport));
    service.models_transport = fake.clone();
    service.save(&credential("account-a", false)).unwrap();
    let ready = service.refresh_models().await.unwrap();
    vault.fail.store(true, Ordering::SeqCst);
    assert!(matches!(service.models().await, Err(ModelsError::Vault)));
    assert!(matches!(
        service.refresh_models().await,
        Err(ModelsError::Vault)
    ));
    vault.fail.store(false, Ordering::SeqCst);
    let recovered = service.models().await.unwrap();
    assert_eq!(recovered.state, "unrequested");
    assert!(recovered.models.is_empty());
    assert_ne!(recovered.connection_revision, ready.connection_revision);
    service.save(&credential("account-a", true)).unwrap();
    let unavailable = service.refresh_models().await.unwrap();
    assert_eq!(unavailable.state, "unavailable");
    assert_eq!(unavailable.error, Some(UNAVAILABLE));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_private(&unavailable);
}

fn routed_models(service: ChatGptAuth) -> axum::Router {
    let mut daemon = crate::state::DaemonState::with_model_adapter(
        crate::agent_runs::test_support::ScriptedModel::new(vec![]),
    );
    daemon.chatgpt_auth = service;
    crate::app::app_with_state(
        Arc::new(tokio::sync::RwLock::new(daemon)),
        crate::app::DaemonConfig::default(),
    )
}
async fn request_models(
    app: axum::Router,
    method: &str,
) -> (axum::http::StatusCode, serde_json::Value) {
    use tower::ServiceExt;
    let path = if method == "GET" {
        "/api/providers/chatgpt/models"
    } else {
        "/api/providers/chatgpt/models/refresh"
    };
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .header("content-type", "application/json")
        .body(axum::body::Body::from("{}"))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn m49_chatgpt_models_routes_get_zero_egress_post_once_busy_and_vault_static_envelopes() {
    use axum::http::StatusCode;
    let vault = Arc::new(ReadFailureVault {
        inner: InMemoryOAuthAppVault::default(),
        fail: AtomicBool::new(false),
    });
    let fake = FakeModels::new();
    let auth = Arc::new(RefreshAuth {
        calls: AtomicUsize::new(0),
    });
    let mut service = ChatGptAuth::with_dependencies(vault.clone(), auth.clone());
    service.models_transport = fake.clone();
    service.save(&credential("account-a", false)).unwrap();
    let app = routed_models(service);
    for _ in 0..2 {
        let (status, snapshot) = request_models(app.clone(), "GET").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(snapshot["state"], "unrequested");
        assert_eq!(snapshot.as_object().unwrap().len(), 7);
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let (status, ready) = request_models(app.clone(), "POST").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["models"],
        json!([{"id":"synthetic-model-a"},{"id":"synthetic-model-b"}])
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    let (status, cached) = request_models(app.clone(), "GET").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cached, ready);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    fake.entered.acquire().await.unwrap().forget();
    fake.blocked.store(true, Ordering::SeqCst);
    let pending = {
        let app = app.clone();
        tokio::spawn(async move { request_models(app, "POST").await })
    };
    fake.entered().await;
    let (status, error) = request_models(app.clone(), "POST").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        error,
        json!({"error":"Model metadata refresh is already in progress."})
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    let (status, cached) = request_models(app.clone(), "GET").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cached, ready);
    pending.abort();
    assert!(matches!(pending.await,Err(error) if error.is_cancelled()));
    assert_eq!(fake.active.load(Ordering::SeqCst), 0);
    vault.fail.store(true, Ordering::SeqCst);
    for method in ["GET", "POST"] {
        let (status, error) = request_models(app.clone(), method).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            error,
            json!({"error":"ChatGPT credential vault is unavailable"})
        );
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        auth.calls.load(Ordering::SeqCst),
        0,
        "cache reads and fresh credentials never refresh auth"
    );
}

struct SwitchVault {
    inner: InMemoryOAuthAppVault,
    next: StdMutex<Option<String>>,
    armed: AtomicBool,
}
impl OAuthVaultBackend for SwitchVault {
    fn load(
        &self,
        s: &str,
        a: &str,
    ) -> Result<Option<zeroize::Zeroizing<String>>, crate::connectors::oauth_apps::OAuthVaultError>
    {
        let result = self.inner.load(s, a)?;
        if a == ACCOUNT && self.armed.swap(false, Ordering::SeqCst) {
            let next = self.next.lock().unwrap().take().unwrap();
            self.inner.put(s, a, &next)?;
        }
        Ok(result)
    }
    fn put(
        &self,
        s: &str,
        a: &str,
        p: &str,
    ) -> Result<(), crate::connectors::oauth_apps::OAuthVaultError> {
        self.inner.put(s, a, p)
    }
    fn delete(
        &self,
        s: &str,
        a: &str,
    ) -> Result<(), crate::connectors::oauth_apps::OAuthVaultError> {
        self.inner.delete(s, a)
    }
}
#[tokio::test]
async fn m49_chatgpt_models_load_pairs_credentials_with_the_manifest_actually_read() {
    let vault = Arc::new(SwitchVault {
        inner: InMemoryOAuthAppVault::default(),
        next: StdMutex::new(None),
        armed: AtomicBool::new(false),
    });
    let service = ChatGptAuth::with_dependencies(vault.clone(), Arc::new(UnavailableTransport));
    service.save(&credential("account-a", false)).unwrap();
    let a = service.manifest().unwrap().unwrap();
    let chunks = (0..a.chunks)
        .map(|i| {
            vault
                .inner
                .load(SERVICE, &a.key(i))
                .unwrap()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    service.save(&credential("account-b", false)).unwrap();
    let b = vault
        .inner
        .load(SERVICE, ACCOUNT)
        .unwrap()
        .unwrap()
        .to_string();
    for (index, chunk) in chunks.iter().enumerate() {
        vault.inner.put(SERVICE, &a.key(index), chunk).unwrap();
    }
    vault
        .inner
        .put(SERVICE, ACCOUNT, &serde_json::to_string(&a).unwrap())
        .unwrap();
    *vault.next.lock().unwrap() = Some(b);
    vault.armed.store(true, Ordering::SeqCst);
    let (manifest, loaded) = service.load_with_manifest().unwrap().unwrap();
    assert!(
        manifest.generation == a.generation && loaded.account_id == "account-a",
        "immutable chunks retain their original manifest association"
    );
    let context = AuthorizedModels::new(0, manifest, loaded);
    assert!(
        service
            .begin_models(&context.stamp)
            .await
            .unwrap()
            .is_none(),
        "current B cannot authorize captured A for dispatch"
    );
}

#[tokio::test]
async fn m49_chatgpt_models_revalidation_rejects_manifest_replaced_during_its_read() {
    let vault = Arc::new(SwitchVault {
        inner: InMemoryOAuthAppVault::default(),
        next: StdMutex::new(None),
        armed: AtomicBool::new(false),
    });
    let fake = FakeModels::new();
    let mut service = ChatGptAuth::with_dependencies(vault.clone(), Arc::new(UnavailableTransport));
    service.models_transport = fake.clone();
    service.save(&credential("account-a", false)).unwrap();
    let context = service.usable_context(true).await.unwrap().unwrap();
    let a = service.manifest().unwrap().unwrap();
    let chunks = (0..a.chunks)
        .map(|i| {
            vault
                .inner
                .load(SERVICE, &a.key(i))
                .unwrap()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    service.save(&credential("account-b", false)).unwrap();
    let b = vault
        .inner
        .load(SERVICE, ACCOUNT)
        .unwrap()
        .unwrap()
        .to_string();
    for (index, chunk) in chunks.iter().enumerate() {
        vault.inner.put(SERVICE, &a.key(index), chunk).unwrap();
    }
    let a_json = serde_json::to_string(&a).unwrap();
    for publication in [false, true] {
        vault.inner.put(SERVICE, ACCOUNT, &a_json).unwrap();
        let operation = service.begin_models(&context.stamp).await.unwrap().unwrap();
        *vault.next.lock().unwrap() = Some(b.clone());
        vault.armed.store(true, Ordering::SeqCst);
        let result = if publication {
            service
                .finish_models(
                    &context.stamp,
                    Some(&operation),
                    Ok(vec!["synthetic-stale-model".into()]),
                )
                .await
                .map(|_| ())
        } else {
            service.begin_models(&context.stamp).await.map(|_| ())
        };
        assert!(
            matches!(result, Err(ModelsError::Vault)),
            "an unstable manifest read cannot authorize dispatch or publication"
        );
        let current = service.models().await.unwrap();
        assert_eq!(current.state, "unrequested");
        assert!(current.models.is_empty());
        assert!(current.catalog_revision.is_none());
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn m49_chatgpt_models_older_operation_cannot_erase_newer_same_stamp_result() {
    let (service, _) = service();
    service.save(&credential("account-a", false)).unwrap();
    let context = service.usable_context(true).await.unwrap().unwrap();
    let old = service.begin_models(&context.stamp).await.unwrap().unwrap();
    let current = service.begin_models(&context.stamp).await.unwrap().unwrap();
    let ready = service
        .finish_models(
            &context.stamp,
            Some(&current),
            Ok(vec!["synthetic-current-model".into()]),
        )
        .await
        .unwrap();
    for result in [
        Err(UNAVAILABLE),
        Ok(vec!["synthetic-obsolete-model".into()]),
    ] {
        let late = service
            .finish_models(&context.stamp, Some(&old), result)
            .await
            .unwrap();
        assert_eq!(late.state, "ready");
        assert_eq!(late.catalog_revision, ready.catalog_revision);
        assert_eq!(late.models.len(), 1);
        assert_eq!(late.models[0].id, "synthetic-current-model");
        assert!(late.error.is_none());
    }
}

fn actual_metadata_service(flag: Option<OsString>) -> Result<ChatGptAuth, &'static str> {
    if flag.as_deref() != Some(OsStr::new("1")) {
        return Err("M49 actual metadata is held; exact root opt-in is required");
    }
    Ok(ChatGptAuth::new())
}
#[test]
fn m49_chatgpt_models_actual_driver_refuses_missing_or_inexact_opt_in_before_auth() {
    for flag in [
        None,
        Some(OsString::from("0")),
        Some(OsString::from("true")),
        Some(OsString::from("1 ")),
    ] {
        assert!(actual_metadata_service(flag).is_err());
    }
}
#[tokio::test]
#[ignore = "HELD: root separately releases one fixed metadata GET after gates; no generation"]
async fn m49_chatgpt_models_real_metadata_refresh() {
    let service = actual_metadata_service(std::env::var_os("ANIMAOS_M49_RUN_REAL_METADATA"))
        .unwrap_or_else(|error| panic!("{error}"));
    let result = tokio::time::timeout(Duration::from_secs(15), service.refresh_models()).await;
    match result {
        Ok(Ok(snapshot)) => {
            use sha2::{Digest, Sha256};
            let ids = snapshot
                .models
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>();
            let bytes = serde_json::to_vec(&ids).unwrap();
            println!(
                "M49_METADATA_DIAGNOSTIC {}",
                json!({"state":snapshot.state,"count":ids.len(),"digest":format!("{:x}",Sha256::digest(&bytes)),"error":snapshot.error})
            );
            assert!(matches!(snapshot.state,"ready"|"empty"),"M49 metadata did not return a current catalog; no fallback or generation is authorized");
        }
        Ok(Err(error)) => {
            println!(
                "M49_METADATA_DIAGNOSTIC {}",
                json!({"state":"unavailable","count":0,"digest":null,"error":if error==ModelsError::Busy {"busy"} else {"vault_unavailable"}})
            );
            panic!("M49 metadata failed; no retry is authorized");
        }
        Err(_) => {
            println!(
                "M49_METADATA_DIAGNOSTIC {}",
                json!({"state":"unavailable","count":0,"digest":null,"error":"root_deadline"})
            );
            panic!("M49 metadata deadline expired; no retry is authorized");
        }
    }
}

fn m50_diagnostic_report(outcome: &ModelsOutcome) -> serde_json::Value {
    let (state, count, error) = match &outcome.result {
        Ok(snapshot) => (snapshot.state, snapshot.models.len(), snapshot.error),
        Err(ModelsError::Busy) => (
            "unavailable",
            0,
            Some("Model metadata refresh is already in progress."),
        ),
        Err(ModelsError::Vault) => ("unavailable", 0, Some(VAULT_ERROR)),
    };
    let diagnostic = outcome.diagnostic;
    json!({"state":state,"count":count,"stage":diagnostic.stage.as_str(),"catalogDispatch":diagnostic.catalog_dispatch.as_str(),"reason":diagnostic.reason.as_str(),"httpStatus":diagnostic.http_status,"error":error})
}
fn assert_m50_private(outcome: &ModelsOutcome) {
    let output = format!(
        "{}|{:?}",
        m50_diagnostic_report(outcome),
        outcome.diagnostic
    );
    for marker in [
        "m50-private",
        "secret-token",
        "secret-refresh",
        "account-a",
        "account-b",
        "manifest",
        "generation",
        "http://",
        "https://",
    ] {
        assert!(
            !output.contains(marker),
            "closed diagnostic must omit every sensitive marker"
        );
    }
    assert_eq!(m50_diagnostic_report(outcome).as_object().unwrap().len(), 7);
    if let Ok(snapshot) = &outcome.result {
        assert_private(snapshot);
        assert_eq!(
            serde_json::to_value(snapshot)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            7,
            "public snapshot never grows diagnostics"
        );
    }
}
struct SecretAuth;
#[async_trait]
impl AuthTransport for SecretAuth {
    async fn start(&self) -> Result<Device, String> {
        panic!("M50 must not start device authorization")
    }
    async fn poll(&self, _: &Device) -> Result<Option<Credential>, String> {
        panic!("M50 must not poll device authorization")
    }
    async fn refresh(&self, _: &Credential) -> Result<Credential, String> {
        Err("m50-private-auth-error".into())
    }
}
#[tokio::test]
async fn m50_catalog_diagnostics_authorization_and_control_fail_before_catalog() {
    let (service, fake) = service();
    let outcome = service.refresh_models_diagnostic().await;
    assert_eq!(
        outcome.diagnostic,
        ModelsDiagnostic::before(Stage::Authorization, Reason::Disconnected)
    );
    assert_m50_private(&outcome);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let held = service.models_refresh.lock().await;
    let busy = service.refresh_models_diagnostic().await;
    assert!(matches!(busy.result, Err(ModelsError::Busy)));
    assert_eq!(
        busy.diagnostic,
        ModelsDiagnostic::before(Stage::Control, Reason::Busy)
    );
    assert_m50_private(&busy);
    drop(held);
    let mut service = ChatGptAuth::with_dependencies(
        Arc::new(InMemoryOAuthAppVault::default()),
        Arc::new(SecretAuth),
    );
    service.models_transport = fake.clone();
    service.save(&credential("account-a", true)).unwrap();
    let outcome = service.refresh_models_diagnostic().await;
    assert_eq!(
        outcome.diagnostic,
        ModelsDiagnostic::before(Stage::Authorization, Reason::Credential)
    );
    assert_eq!(outcome.result.as_ref().unwrap().error, Some(UNAVAILABLE));
    assert_m50_private(&outcome);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let vault = Arc::new(ReadFailureVault {
        inner: InMemoryOAuthAppVault::default(),
        fail: AtomicBool::new(true),
    });
    let mut service = ChatGptAuth::with_dependencies(vault, Arc::new(UnavailableTransport));
    service.models_transport = fake.clone();
    let outcome = service.refresh_models_diagnostic().await;
    assert!(matches!(outcome.result, Err(ModelsError::Vault)));
    assert_eq!(
        outcome.diagnostic,
        ModelsDiagnostic::before(Stage::Authorization, Reason::Vault)
    );
    assert_m50_private(&outcome);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn m50_catalog_diagnostics_typed_fetch_mapping_preserves_static_snapshots() {
    let failure = match anima_model_adapters::ChatGptModelsAdapter::new_detailed(
        String::new(),
        "m50-private-account".into(),
    ) {
        Err(failure) => failure,
        Ok(_) => panic!("empty credential must fail locally"),
    };
    assert_eq!(
        ModelsFetchFailure::Core(failure).diagnostic(),
        ModelsDiagnostic::before(Stage::Client, Reason::Credential)
    );
    for diagnostic in [
        ModelsDiagnostic::new(
            Stage::HttpStatus,
            Dispatch::ResponseReceived,
            Reason::HttpStatus,
            Some(403),
        ),
        ModelsDiagnostic::new(Stage::Request, Dispatch::Attempted, Reason::Connect, None),
        ModelsDiagnostic::new(
            Stage::Body,
            Dispatch::ResponseReceived,
            Reason::DeclaredLimit,
            Some(200),
        ),
        ModelsDiagnostic::new(
            Stage::Body,
            Dispatch::ResponseReceived,
            Reason::Timeout,
            Some(200),
        ),
        ModelsDiagnostic::new(
            Stage::Decode,
            Dispatch::ResponseReceived,
            Reason::InvalidCatalog,
            Some(200),
        ),
    ] {
        let (service, fake) = service();
        service.save(&credential("account-a", false)).unwrap();
        fake.answer_diagnostic(diagnostic);
        let outcome = service.refresh_models_diagnostic().await;
        assert_eq!(outcome.diagnostic, diagnostic);
        let snapshot = outcome.result.as_ref().unwrap();
        assert_eq!(snapshot.state, "unavailable");
        assert_eq!(snapshot.error, Some(diagnostic.public_message()));
        assert!(snapshot.models.is_empty());
        assert_m50_private(&outcome);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn m50_catalog_diagnostics_scope_aba_and_overlapping_calls_keep_their_own_evidence() {
    for failure in [false, true] {
        let (first, first_fake) = service();
        first.save(&credential("account-a", false)).unwrap();
        first_fake.blocked.store(true, Ordering::SeqCst);
        if failure {
            first_fake.answer_diagnostic(ModelsDiagnostic::new(
                Stage::HttpStatus,
                Dispatch::ResponseReceived,
                Reason::HttpStatus,
                Some(403),
            ));
        }
        let mut pending = Box::pin(first.refresh_models_diagnostic());
        tokio::select! {_ = first_fake.entered()=>{},_ = &mut pending=>panic!("first diagnostic must wait for its controlled response")};
        let (second, second_fake) = service();
        second.save(&credential("account-b", false)).unwrap();
        second_fake.answer_diagnostic(ModelsDiagnostic::new(
            Stage::HttpStatus,
            Dispatch::ResponseReceived,
            Reason::HttpStatus,
            Some(429),
        ));
        let independent = second.refresh_models_diagnostic().await;
        assert_eq!(independent.diagnostic.http_status, Some(429));
        assert_m50_private(&independent);
        let busy = first.refresh_models_diagnostic().await;
        assert_eq!(
            busy.diagnostic,
            ModelsDiagnostic::before(Stage::Control, Reason::Busy)
        );
        assert_m50_private(&busy);
        first.cancel(false).await.unwrap();
        first.save(&credential("account-b", false)).unwrap();
        first.save(&credential("account-a", false)).unwrap();
        first_fake.release.add_permits(1);
        let late = tokio::time::timeout(Duration::from_secs(1), &mut pending)
            .await
            .unwrap();
        drop(pending);
        assert_eq!(
            late.diagnostic,
            ModelsDiagnostic::new(
                Stage::Scope,
                Dispatch::ResponseReceived,
                Reason::ScopeChanged,
                if failure { Some(403) } else { None }
            )
        );
        assert_eq!(late.result.as_ref().unwrap().state, "unrequested");
        assert!(late.result.as_ref().unwrap().models.is_empty());
        assert_m50_private(&late);
        assert_eq!(independent.diagnostic.stage, Stage::HttpStatus);
        assert_eq!(independent.diagnostic.http_status, Some(429));
        assert_eq!(first_fake.calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_fake.calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn m50_catalog_diagnostics_outer_deadline_retains_unknown_dispatch_certainty() {
    for before_dispatch in [true, false] {
        let (service, fake) = service();
        service.save(&credential("account-a", false)).unwrap();
        fake.blocked.store(true, Ordering::SeqCst);
        let held = if before_dispatch {
            Some(service.lifecycle.lock().await)
        } else {
            None
        };
        let outcome = tokio::time::timeout(
            Duration::from_secs(1),
            service.refresh_models_diagnostic_until(Instant::now() + Duration::from_millis(100)),
        )
        .await
        .expect("outer deadline is finite despite lock or transport contention");
        assert_eq!(outcome.diagnostic, ModelsDiagnostic::deadline());
        assert_scope_free_timeout(outcome.result.as_ref().unwrap());
        assert_m50_private(&outcome);
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            usize::from(!before_dispatch)
        );
        assert_eq!(fake.active.load(Ordering::SeqCst), 0);
        assert!(service.models_refresh.try_lock().is_ok());
        drop(held);
    }
}
fn m50_actual_metadata_service(flag: Option<OsString>) -> Result<ChatGptAuth, &'static str> {
    if flag.as_deref() != Some(OsStr::new("1")) {
        return Err("M50 actual metadata is held; exact root opt-in is required");
    }
    Ok(ChatGptAuth::new())
}
#[test]
fn m50_catalog_diagnostics_actual_driver_refuses_missing_or_inexact_opt_in_before_auth() {
    for flag in [
        None,
        Some(OsString::from("")),
        Some(OsString::from("0")),
        Some(OsString::from("true")),
        Some(OsString::from("1 ")),
        Some(OsString::from("01")),
    ] {
        assert!(m50_actual_metadata_service(flag).is_err());
    }
}
#[tokio::test]
#[ignore = "HELD: separate M50 root release required for one fixed metadata diagnostic; no generation"]
async fn m50_catalog_diagnostics_real_metadata_refresh() {
    let outcome =
        match m50_actual_metadata_service(std::env::var_os("ANIMAOS_M50_RUN_REAL_METADATA")) {
            Ok(service) => match tokio::time::timeout(
                Duration::from_secs(15),
                service.refresh_models_diagnostic(),
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => ModelsOutcome::timeout(),
            },
            Err(_) => {
                let mut snapshot = ModelsCache::default().snapshot;
                snapshot.state = "unavailable";
                snapshot.error = Some(UNAVAILABLE);
                ModelsOutcome::new(
                    Ok(snapshot),
                    ModelsDiagnostic::before(Stage::Control, Reason::None),
                )
            }
        };
    println!(
        "M50_METADATA_DIAGNOSTIC {}",
        m50_diagnostic_report(&outcome)
    );
    assert!(
        matches!(&outcome.result,Ok(snapshot) if matches!(snapshot.state,"ready"|"empty") && outcome.diagnostic.stage==Stage::Complete),
        "M50 metadata did not return a current catalog; no retry or generation is authorized"
    );
}

fn m51_actual_metadata_service(flag: Option<OsString>) -> Result<ChatGptAuth, &'static str> {
    if flag.as_deref() != Some(OsStr::new("1")) {
        return Err("M51 actual metadata is held; exact root opt-in is required");
    }
    Ok(ChatGptAuth::new())
}
#[test]
fn m51_catalog_budget_actual_driver_refuses_missing_or_inexact_opt_in_before_auth() {
    for flag in [
        None,
        Some(OsString::from("")),
        Some(OsString::from("0")),
        Some(OsString::from("true")),
        Some(OsString::from("1 ")),
        Some(OsString::from("01")),
    ] {
        assert!(m51_actual_metadata_service(flag).is_err());
    }
}
#[tokio::test]
#[ignore = "HELD: separate M51 root release required for one fixed metadata diagnostic; no generation"]
async fn m51_catalog_budget_real_metadata_refresh() {
    let outcome =
        match m51_actual_metadata_service(std::env::var_os("ANIMAOS_M51_RUN_REAL_METADATA")) {
            Ok(service) => match tokio::time::timeout(
                Duration::from_secs(15),
                service.refresh_models_diagnostic(),
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => ModelsOutcome::timeout(),
            },
            Err(_) => {
                let mut snapshot = ModelsCache::default().snapshot;
                snapshot.state = "unavailable";
                snapshot.error = Some(UNAVAILABLE);
                ModelsOutcome::new(
                    Ok(snapshot),
                    ModelsDiagnostic::before(Stage::Control, Reason::None),
                )
            }
        };
    println!(
        "M51_METADATA_DIAGNOSTIC {}",
        m50_diagnostic_report(&outcome)
    );
    assert!(
        matches!(&outcome.result,Ok(snapshot) if matches!(snapshot.state,"ready"|"empty") && outcome.diagnostic.stage==Stage::Complete),
        "M51 metadata did not return a current catalog; no retry or generation is authorized"
    );
}
