//! Controlled transport fixtures exercise service lifecycle ownership, not login.
use super::*;
use crate::browser::transport::{
    BrowserTransportHandle, BrowserTransportReply, BrowserTransportSnapshot, VerifiedOutput,
};
use anima_service_adapters::browser::{BrowserArtifact, BrowserObservation};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Semaphore;

struct OwnerTransport {
    opens: AtomicUsize,
    close_attempts: AtomicUsize,
    fail_finish: AtomicBool,
    paused: AtomicBool,
    quarantine: AtomicBool,
    entered: Semaphore,
    release: Semaphore,
    snapshot: StdMutex<BrowserTransportSnapshot>,
    profile: StdMutex<Option<Arc<crate::browser::profiles::ProfileSession>>>,
    on_bind: StdMutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
impl Default for OwnerTransport {
    fn default() -> Self {
        Self {
            opens: AtomicUsize::new(0),
            close_attempts: AtomicUsize::new(0),
            fail_finish: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            quarantine: AtomicBool::new(false),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            snapshot: StdMutex::new(BrowserTransportSnapshot::default()),
            profile: StdMutex::new(None),
            on_bind: StdMutex::new(None),
        }
    }
}
#[async_trait]
impl BrowserPort for OwnerTransport {
    async fn open(
        &self,
        _: BrowserScope,
        _: BrowserControl,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.snapshot.lock().unwrap().physical_lease_held = true;
        self.entered.add_permits(1);
        if self.paused.load(Ordering::SeqCst) {
            self.release.acquire().await.unwrap().forget();
        }
        Ok(Box::new(OwnerSession))
    }
}
struct OwnerSession;
#[async_trait]
impl BrowserSession for OwnerSession {
    async fn execute(
        &self,
        _: BrowserOperation,
        _: BrowserControl,
    ) -> Result<BrowserObservation, BrowserError> {
        Err(browser_error(BrowserErrorCode::Denied))
    }
    async fn close(&self) -> BrowserClose {
        BrowserClose::Closed
    }
}
#[async_trait]
impl BrowserTransportHandle for OwnerTransport {
    fn snapshot(&self) -> BrowserTransportSnapshot {
        self.snapshot.lock().unwrap().clone()
    }
    async fn execute_admitted(
        &self,
        _: BrowserOperation,
        _: Option<BrowserDocumentWitness>,
        _: BrowserControl,
    ) -> Result<BrowserTransportReply, BrowserError> {
        Err(browser_error(BrowserErrorCode::Denied))
    }
    async fn read_output(&self, _: &BrowserArtifact) -> Result<VerifiedOutput, BrowserError> {
        Err(browser_error(BrowserErrorCode::Denied))
    }
    async fn finish_profile_login(&self) -> Result<(), BrowserError> {
        if self.fail_finish.load(Ordering::SeqCst) {
            Err(browser_error(BrowserErrorCode::Denied))
        } else {
            Ok(())
        }
    }
    async fn close(&self) -> BrowserClose {
        self.close_attempts.fetch_add(1, Ordering::SeqCst);
        let quarantined = self.quarantine.load(Ordering::SeqCst);
        let closed = if quarantined {
            BrowserClose::Quarantined
        } else {
            BrowserClose::Closed
        };
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.close = Some(closed);
        snapshot.physical_lease_held = quarantined;
        if !quarantined {
            if let Some(profile) = self.profile.lock().unwrap().take() {
                profile.release();
            }
        }
        closed
    }
}
struct OwnerFactory(Arc<OwnerTransport>);
impl BrowserTransportFactory for OwnerFactory {
    fn bind(&self, task: PreparedBrowserTask) -> Result<BoundBrowserTask, BrowserError> {
        *self.0.profile.lock().unwrap() = task.profile;
        if let Some(callback) = self.0.on_bind.lock().unwrap().take() {
            callback();
        }
        Ok(BoundBrowserTask {
            port: self.0.clone(),
            handle: self.0.clone(),
        })
    }
}
struct LifecycleFixture {
    base: crate::browser::tests::Fixture,
    custody: tempfile::TempDir,
    service: BrowserService,
    transport: Arc<OwnerTransport>,
    _router: axum::Router,
}
impl LifecycleFixture {
    async fn new() -> Self {
        let base = crate::browser::tests::Fixture::with_tools_model(
            120_000,
            crate::agent_runs::test_support::ScriptedModel::new(vec![]),
            &["browser_navigate"],
        )
        .await;
        let custody = tempfile::tempdir().unwrap();
        let transport = Arc::new(OwnerTransport::default());
        let mut policies = crate::browser::tests::policies(120_000);
        policies
            .policies
            .get_mut("controlled-operator-policy")
            .unwrap()
            .template
            .authenticated = true;
        let mut service = BrowserService::new_with_factory(
            &base.state,
            base.coordinator.control_plane_transactions(),
            policies,
            Arc::new(OwnerFactory(transport.clone())),
        );
        Arc::get_mut(&mut service.inner).unwrap().profile_store =
            crate::browser::profiles::ProfileStore::for_test(custody.path().join("profiles"));
        base.state.write().await.browser_service = Some(service.clone());
        // Exercise the production router installation of the lifecycle probe.
        let _router = crate::routes::router_with_runs(
            base.state.clone(),
            crate::app::DaemonConfig::default(),
            |_| base.coordinator.clone(),
        );
        Self {
            base,
            custody,
            service,
            transport,
            _router,
        }
    }
    async fn create(&self) -> BrowserProfileView {
        self.service
            .create_profile(
                &self.base.run.agent_id,
                CreateBrowserProfile {
                    policy_id: "controlled-operator-policy".into(),
                },
            )
            .await
            .unwrap()
    }
    fn start(
        &self,
        profile: &BrowserProfileView,
    ) -> tokio::task::JoinHandle<Result<BrowserProfileView, BrowserServiceError>> {
        let service = self.service.clone();
        let agent = self.base.run.agent_id.clone();
        let profile = profile.clone();
        tokio::spawn(async move {
            service
                .start_profile_login(
                    &agent,
                    &profile.id,
                    StartBrowserLogin {
                        expected_revision: profile.revision,
                        route_id: "page".into(),
                    },
                )
                .await
        })
    }
    fn drain(&self) -> crate::system::DrainController {
        use crate::connectors::{credentials::InMemoryCredentialStore, runtime::ConnectorManager};
        let state = self.base.state.clone();
        let coordinator = self.base.coordinator.clone();
        let connectors = ConnectorManager::new(
            state.clone(),
            coordinator.clone(),
            Arc::new(InMemoryCredentialStore::default()),
            Arc::new(crate::app::DeterministicTelegramTransport),
        );
        let scheduler =
            crate::schedules::SchedulerService::new(state.clone(), coordinator.clone(), connectors);
        let jobs = crate::jobs::JobService::new(state.clone(), coordinator.clone());
        crate::system::DrainController::new(state, coordinator, jobs, scheduler)
    }
}

#[tokio::test]
async fn browser_profile_lifecycle_shutdown_fences_a_start_queued_behind_its_snapshot() {
    let fixture = LifecycleFixture::new().await;
    let profile = fixture.create().await;
    let gate = fixture.service.inner.profile_mutations.lock().await;
    let service = fixture.service.clone();
    let shutdown = tokio::spawn(async move { service.shutdown().await });
    tokio::task::yield_now().await;
    let start = fixture.start(&profile);
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    drop(gate);
    shutdown.await.unwrap();
    let result = start.await.unwrap();
    assert!(
        result.is_err(),
        "queued sign-in opened after shutdown returned"
    );
    assert_eq!(fixture.transport.opens.load(Ordering::SeqCst), 0);
    assert!(
        fixture.start(&profile).await.unwrap().is_err(),
        "shutdown admission must remain closed"
    );
}

#[tokio::test]
async fn browser_profile_lifecycle_drain_refuses_start_cancel_reopens_and_cleanup_is_allowed() {
    let fixture = LifecycleFixture::new().await;
    let profile = fixture.create().await;
    fixture.base.coordinator.begin_drain();
    let result = fixture.start(&profile).await.unwrap();
    assert!(
        result.is_err(),
        "owner sign-in bypassed the coordinator drain"
    );
    assert_eq!(fixture.transport.opens.load(Ordering::SeqCst), 0);
    fixture.base.coordinator.cancel_drain();
    let signing = fixture.start(&profile).await.unwrap().unwrap();
    // This is metadata-only cleanup evidence; encrypted cookie custody has its own acceptance.
    std::fs::write(
        fixture
            .custody
            .path()
            .join("profiles")
            .join(format!("{}.sealed", profile.id)),
        b"sealed-fixture",
    )
    .unwrap();
    fixture.base.coordinator.begin_drain();
    let ready = fixture
        .service
        .finish_profile_login(
            &fixture.base.run.agent_id,
            &profile.id,
            BrowserProfileMutation {
                expected_revision: signing.revision,
            },
        )
        .await
        .unwrap();
    fixture
        .service
        .revoke_profile(
            &fixture.base.run.agent_id,
            &profile.id,
            BrowserProfileMutation {
                expected_revision: ready.revision,
            },
        )
        .await
        .unwrap();
    fixture.service.shutdown().await;
}

#[tokio::test]
async fn browser_profile_lifecycle_rechecks_drain_immediately_before_physical_open() {
    let fixture = LifecycleFixture::new().await;
    let profile = fixture.create().await;
    let coordinator = fixture.base.coordinator.clone();
    *fixture.transport.on_bind.lock().unwrap() = Some(Arc::new(move || coordinator.begin_drain()));
    assert!(
        fixture.start(&profile).await.unwrap().is_err(),
        "sign-in opened after drain began during preparation"
    );
    assert_eq!(fixture.transport.opens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn browser_profile_lifecycle_drain_counts_pending_active_and_quarantined_owner_work() {
    let fixture = LifecycleFixture::new().await;
    let profile = fixture.create().await;
    let drain = fixture.drain();
    let gate = fixture.service.inner.profile_mutations.lock().await;
    fixture.transport.paused.store(true, Ordering::SeqCst);
    let start = fixture.start(&profile);
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    let pending = serde_json::to_value(drain.in_flight().await).unwrap();
    assert_eq!(
        pending.get("browserProfileLogins"),
        Some(&serde_json::json!(1)),
        "queued owner sign-in missing from drain"
    );
    drop(gate);
    fixture.transport.entered.acquire().await.unwrap().forget();
    let opening = serde_json::to_value(drain.in_flight().await).unwrap();
    assert_eq!(opening["browserProfileLogins"], 1);
    fixture.transport.release.add_permits(1);
    let signing = start.await.unwrap().unwrap();
    let active = serde_json::to_value(drain.begin().await).unwrap();
    assert_eq!(active["browserProfileLogins"], 1);
    fixture.transport.quarantine.store(true, Ordering::SeqCst);
    assert!(fixture
        .service
        .revoke_profile(
            &fixture.base.run.agent_id,
            &profile.id,
            BrowserProfileMutation {
                expected_revision: signing.revision
            }
        )
        .await
        .is_err());
    let quarantined = serde_json::to_value(drain.in_flight().await).unwrap();
    assert_eq!(
        quarantined["browserProfileLogins"], 1,
        "removed setup must retain uncertain physical work"
    );
    fixture.transport.quarantine.store(false, Ordering::SeqCst);
    BrowserTransportHandle::close(fixture.transport.as_ref()).await;
    let closed = serde_json::to_value(drain.in_flight().await).unwrap();
    assert_eq!(closed["browserProfileLogins"], 0);
}

#[tokio::test]
async fn browser_profile_marker_cleanup_failure_attempts_close_and_settles_physical_work() {
    let fixture = LifecycleFixture::new().await;
    let profile = fixture.create().await;
    let signing = fixture.start(&profile).await.unwrap().unwrap();
    fixture.transport.fail_finish.store(true, Ordering::SeqCst);
    assert_eq!(fixture.service.profile_in_flight_count(), 1);
    let result = fixture
        .service
        .finish_profile_login(
            &fixture.base.run.agent_id,
            &profile.id,
            BrowserProfileMutation {
                expected_revision: signing.revision,
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(
        fixture.transport.close_attempts.load(Ordering::SeqCst),
        1,
        "a failed finish marker must still attempt physical close"
    );
    assert!(!fixture.transport.snapshot().physical_lease_held);
    assert_eq!(fixture.service.profile_in_flight_count(), 0);
}

#[tokio::test]
async fn browser_profile_marker_cleanup_shutdown_retries_retained_owner_absent_from_setup_table() {
    let fixture = LifecycleFixture::new().await;
    let profile = fixture.create().await;
    let signing = fixture.start(&profile).await.unwrap().unwrap();
    fixture.transport.fail_finish.store(true, Ordering::SeqCst);
    fixture.transport.quarantine.store(true, Ordering::SeqCst);
    assert!(fixture
        .service
        .finish_profile_login(
            &fixture.base.run.agent_id,
            &profile.id,
            BrowserProfileMutation {
                expected_revision: signing.revision
            },
        )
        .await
        .is_err());
    assert!(fixture
        .service
        .inner
        .profile_setups
        .lock()
        .unwrap()
        .is_empty());
    assert_eq!(fixture.service.profile_in_flight_count(), 1);
    let attempted_before_shutdown = fixture.transport.close_attempts.load(Ordering::SeqCst);
    fixture.transport.quarantine.store(false, Ordering::SeqCst);
    fixture.service.shutdown().await;
    assert!(
        fixture.transport.close_attempts.load(Ordering::SeqCst) > attempted_before_shutdown,
        "shutdown skipped the retained physical owner removed from the setup table"
    );
    assert!(!fixture.transport.snapshot().physical_lease_held);
    assert_eq!(fixture.service.profile_in_flight_count(), 0);
}
