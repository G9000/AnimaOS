use super::*;
use crate::browser::profiles::{BrowserProfileView, CreateBrowserProfile, StartBrowserLogin};
use crate::browser::transport::{
    BrowserTransportHandle, BrowserTransportReply, BrowserTransportSnapshot, VerifiedOutput,
};
use anima_service_adapters::browser::{BrowserArtifact, BrowserObservation};
use std::sync::atomic::AtomicUsize;
use tokio::sync::Semaphore;

struct TestTransport {
    pause_open: AtomicBool,
    pause_close: AtomicBool,
    opens: AtomicUsize,
    closes: AtomicUsize,
    open_entered: Semaphore,
    open_release: Semaphore,
    close_entered: Semaphore,
    close_release: Semaphore,
    snapshot: StdMutex<BrowserTransportSnapshot>,
    profile: StdMutex<Option<Arc<crate::browser::profiles::ProfileSession>>>,
}

impl Default for TestTransport {
    fn default() -> Self {
        Self {
            pause_open: AtomicBool::new(false),
            pause_close: AtomicBool::new(false),
            opens: AtomicUsize::new(0),
            closes: AtomicUsize::new(0),
            open_entered: Semaphore::new(0),
            open_release: Semaphore::new(0),
            close_entered: Semaphore::new(0),
            close_release: Semaphore::new(0),
            snapshot: StdMutex::new(BrowserTransportSnapshot::default()),
            profile: StdMutex::new(None),
        }
    }
}

#[async_trait]
impl BrowserPort for TestTransport {
    async fn open(
        &self,
        _: BrowserScope,
        _: BrowserControl,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.snapshot.lock().unwrap().physical_lease_held = true;
        self.open_entered.add_permits(1);
        if self.pause_open.load(Ordering::SeqCst) {
            self.open_release.acquire().await.unwrap().forget();
        }
        Ok(Box::new(TestSession))
    }
}
struct TestSession;
#[async_trait]
impl BrowserSession for TestSession {
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
impl BrowserTransportHandle for TestTransport {
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
        Ok(())
    }
    async fn close(&self) -> BrowserClose {
        self.closes.fetch_add(1, Ordering::SeqCst);
        self.close_entered.add_permits(1);
        if self.pause_close.load(Ordering::SeqCst) {
            self.close_release.acquire().await.unwrap().forget();
        }
        if let Some(profile) = self.profile.lock().unwrap().take() {
            profile.release();
        }
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.close = Some(BrowserClose::Closed);
        snapshot.physical_lease_held = false;
        BrowserClose::Closed
    }
}
struct TestFactory(Arc<TestTransport>);
impl BrowserTransportFactory for TestFactory {
    fn bind(&self, task: PreparedBrowserTask) -> Result<BoundBrowserTask, BrowserError> {
        *self.0.profile.lock().unwrap() = task.profile;
        Ok(BoundBrowserTask {
            port: self.0.clone(),
            handle: self.0.clone(),
        })
    }
}
struct Fixture {
    base: crate::browser::tests::Fixture,
    custody: tempfile::TempDir,
    service: BrowserService,
    transport: Arc<TestTransport>,
}
impl Fixture {
    async fn new() -> Self {
        let base = crate::browser::tests::Fixture::with_tools_model(
            120_000,
            crate::agent_runs::test_support::ScriptedModel::new(vec![]),
            &["browser_navigate"],
        )
        .await;
        let custody = tempfile::tempdir().unwrap();
        let transport = Arc::new(TestTransport::default());
        let mut service = BrowserService::new_with_factory(
            &base.state,
            base.coordinator.control_plane_transactions(),
            crate::browser::tests::policies(120_000),
            Arc::new(TestFactory(transport.clone())),
        );
        Arc::get_mut(&mut service.inner).unwrap().profile_store =
            crate::browser::profiles::ProfileStore::for_test(custody.path().join("profiles"));
        Self {
            base,
            custody,
            service,
            transport,
        }
    }
    fn input() -> CreateBrowserDestination {
        CreateBrowserDestination {
            start_url: "https://example.com/search?q=one".into(),
            allowed_origins: vec!["https://cdn.example.com".into()],
            authenticated: true,
        }
    }
    async fn publish(
        &self,
        input: CreateBrowserDestination,
    ) -> Result<BrowserPolicyView, BrowserServiceError> {
        let (start, origins) = destination_urls(&input)?;
        let pins = origins
            .into_iter()
            .map(|origin| BrowserOrigin {
                origin,
                public_ipv4_pin: "93.184.216.34".into(),
            })
            .collect();
        self.service
            .publish_destination(&self.base.run.agent_id, &input, start, pins)
            .await
    }
    async fn profile(&self, policy: &BrowserPolicyView) -> BrowserProfileView {
        self.service
            .create_profile(
                &self.base.run.agent_id,
                CreateBrowserProfile {
                    policy_id: policy.id.clone(),
                },
            )
            .await
            .unwrap()
    }
    async fn ready_profile(&self, policy: &BrowserPolicyView) -> BrowserProfileView {
        let profile = self.profile(policy).await;
        let workspace = self
            .service
            .profile_workspace(&self.base.run.agent_id)
            .await
            .unwrap();
        std::fs::write(
            self.custody
                .path()
                .join("profiles")
                .join(format!("{}.sealed", profile.id)),
            b"synthetic sealed fixture",
        )
        .unwrap();
        self.service
            .inner
            .profile_store
            .finish(&workspace, &profile)
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
                        route_id: "start".into(),
                    },
                )
                .await
        })
    }
    fn revoke(
        &self,
        policy: &BrowserPolicyView,
    ) -> tokio::task::JoinHandle<Result<(), BrowserServiceError>> {
        let service = self.service.clone();
        let agent = self.base.run.agent_id.clone();
        let policy = policy.clone();
        tokio::spawn(async move {
            service
                .revoke_destination(&agent, &policy.id, policy.version)
                .await
        })
    }
}

#[test]
fn destination_identity_is_canonical_and_does_not_include_dns_pins() {
    let mut first = Fixture::input();
    first.allowed_origins.push("https://example.com".into());
    let mut reordered = Fixture::input();
    reordered.allowed_origins = vec![
        "https://example.com".into(),
        "https://cdn.example.com".into(),
        "https://cdn.example.com".into(),
    ];
    let (start, origins) = destination_urls(&first).unwrap();
    let (second_start, second_origins) = destination_urls(&reordered).unwrap();
    assert_eq!(
        destination_id("owner", &start, &origins, true).unwrap(),
        destination_id("owner", &second_start, &second_origins, true).unwrap()
    );
    assert_ne!(
        destination_id("owner", &start, &origins, true).unwrap(),
        destination_id("other", &start, &origins, true).unwrap()
    );
}

#[tokio::test]
async fn destination_recreation_uses_fresh_versions_and_cannot_be_revoked_by_an_old_request() {
    let fixture = Fixture::new().await;
    let first = fixture.publish(Fixture::input()).await.unwrap();
    assert!(matches!(
        fixture.publish(Fixture::input()).await,
        Err(BrowserServiceError::Conflict)
    ));
    fixture.revoke(&first).await.unwrap().unwrap();
    let second = fixture.publish(Fixture::input()).await.unwrap();
    assert_eq!(second.id, first.id);
    assert!(second.version > first.version && second.version <= MAX_BROWSER_SAFE_INTEGER);
    assert_eq!(
        fixture
            .service
            .revoke_destination(&fixture.base.run.agent_id, &first.id, first.version)
            .await,
        Err(BrowserServiceError::Conflict)
    );
    assert!(fixture
        .service
        .policy(&fixture.base.run.agent_id, &second.id)
        .is_some());
}

#[tokio::test]
async fn destination_revocation_durably_removes_ready_profile_authority_and_sealed_state() {
    let fixture = Fixture::new().await;
    let policy = fixture.publish(Fixture::input()).await.unwrap();
    let ready = fixture.ready_profile(&policy).await;
    fixture.revoke(&policy).await.unwrap().unwrap();
    let profiles = fixture
        .service
        .list_profiles(&fixture.base.run.agent_id)
        .await
        .unwrap();
    let revoked = profiles.iter().find(|view| view.id == ready.id).unwrap();
    assert!(revoked.revoked && !revoked.ready);
    assert!(!fixture
        .custody
        .path()
        .join("profiles")
        .join(format!("{}.sealed", ready.id))
        .exists());
    let replacement = fixture.publish(Fixture::input()).await.unwrap();
    let workspace = fixture
        .service
        .profile_workspace(&fixture.base.run.agent_id)
        .await
        .unwrap();
    let installed = fixture
        .service
        .policy(&fixture.base.run.agent_id, &replacement.id)
        .unwrap();
    assert!(fixture
        .service
        .profile_for_policy(
            &workspace,
            &fixture.base.run.agent_id,
            &installed,
            Some(&ready.id)
        )
        .await
        .is_err());
}

#[tokio::test]
async fn destination_revocation_joins_a_signin_opening_before_its_setup_is_visible() {
    let fixture = Fixture::new().await;
    let policy = fixture.publish(Fixture::input()).await.unwrap();
    let profile = fixture.profile(&policy).await;
    fixture.transport.pause_open.store(true, Ordering::SeqCst);
    let start = fixture.start(&profile);
    fixture
        .transport
        .open_entered
        .acquire()
        .await
        .unwrap()
        .forget();
    let revoke = fixture.revoke(&policy);
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(
        !revoke.is_finished(),
        "revocation returned while sign-in could still publish a new owner"
    );
    fixture.transport.open_release.add_permits(1);
    start.await.unwrap().unwrap();
    revoke.await.unwrap().unwrap();
    assert!(!fixture.transport.snapshot().physical_lease_held);
    assert!(fixture
        .service
        .inner
        .profile_setups
        .lock()
        .unwrap()
        .is_empty());
    assert!(fixture
        .service
        .list_profiles(&fixture.base.run.agent_id)
        .await
        .unwrap()
        .iter()
        .all(|view| view.revoked));
}

#[tokio::test]
async fn dropped_destination_revoke_caller_does_not_abandon_owned_cleanup() {
    let fixture = Fixture::new().await;
    let policy = fixture.publish(Fixture::input()).await.unwrap();
    let ready = fixture.ready_profile(&policy).await;
    let profile = fixture.profile(&policy).await;
    fixture.start(&profile).await.unwrap().unwrap();
    fixture.transport.pause_close.store(true, Ordering::SeqCst);
    let revoke = fixture.revoke(&policy);
    fixture
        .transport
        .close_entered
        .acquire()
        .await
        .unwrap()
        .forget();
    revoke.abort();
    fixture.transport.close_release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), async {
        let _gate = fixture.service.inner.profile_mutations.lock().await;
        assert!(!fixture.transport.snapshot().physical_lease_held);
        let profiles = fixture
            .service
            .list_profiles(&fixture.base.run.agent_id)
            .await
            .unwrap();
        assert!(profiles.iter().all(|view| view.revoked && !view.ready));
        assert!(!fixture
            .custody
            .path()
            .join("profiles")
            .join(format!("{}.sealed", ready.id))
            .exists());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn destination_reauthorization_after_restart_preserves_only_unrevoked_account_profiles() {
    let fixture = Fixture::new().await;
    let first = fixture.publish(Fixture::input()).await.unwrap();
    let ready = fixture.ready_profile(&first).await;
    // A fresh service has no reviewed website grants until the owner reauthorizes.
    let mut restarted = BrowserService::new_with_factory(
        &fixture.base.state,
        fixture.base.coordinator.control_plane_transactions(),
        crate::browser::tests::policies(120_000),
        Arc::new(TestFactory(fixture.transport.clone())),
    );
    Arc::get_mut(&mut restarted.inner).unwrap().profile_store =
        fixture.service.inner.profile_store.clone();
    let (start, origins) = destination_urls(&Fixture::input()).unwrap();
    let pins = origins
        .into_iter()
        .map(|origin| BrowserOrigin {
            origin,
            public_ipv4_pin: "93.184.216.35".into(),
        })
        .collect();
    let current = restarted
        .publish_destination(&fixture.base.run.agent_id, &Fixture::input(), start, pins)
        .await
        .unwrap();
    assert_eq!(current.id, first.id);
    let installed = restarted
        .policy(&fixture.base.run.agent_id, &current.id)
        .unwrap();
    let workspace = restarted
        .profile_workspace(&fixture.base.run.agent_id)
        .await
        .unwrap();
    assert!(restarted
        .profile_for_policy(
            &workspace,
            &fixture.base.run.agent_id,
            &installed,
            Some(&ready.id)
        )
        .await
        .is_ok());
    restarted
        .revoke_destination(&fixture.base.run.agent_id, &current.id, current.version)
        .await
        .unwrap();
    assert!(restarted
        .profile_for_policy(
            &workspace,
            &fixture.base.run.agent_id,
            &installed,
            Some(&ready.id)
        )
        .await
        .is_err());
}
