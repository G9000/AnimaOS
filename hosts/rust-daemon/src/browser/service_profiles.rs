//! Owner-only dedicated sign-in and revocation. No credential fields enter HTTP.
use super::*;
use crate::browser::profiles::{
    BrowserProfileMutation, BrowserProfileView, CreateBrowserProfile, StartBrowserLogin,
};

pub(super) struct ProfileSetup {
    pub(super) view: BrowserProfileView,
    pub(super) task: Arc<BoundBrowserTask>,
    pub(super) _session: Box<dyn BrowserSession>,
}
// Pending starts and bound owners remain drain-visible after their HTTP caller
// disappears. A physical owner is retained until its handle proves closure.
struct ProfileStartWork {
    service: BrowserService,
    id: String,
    retained: bool,
}
impl ProfileStartWork {
    fn new(service: &BrowserService) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        service
            .inner
            .profile_work
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), None);
        Self {
            service: service.clone(),
            id,
            retained: false,
        }
    }
    fn bind(&self, task: Arc<BoundBrowserTask>) {
        self.service
            .inner
            .profile_work
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(self.id.clone(), Some(task));
    }
}
impl Drop for ProfileStartWork {
    fn drop(&mut self) {
        if self.retained {
            return;
        }
        let mut work = self
            .service
            .inner
            .profile_work
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let physical = work
            .get(&self.id)
            .and_then(Option::as_ref)
            .is_some_and(|task| task.handle.snapshot().physical_lease_held);
        if !physical {
            work.remove(&self.id);
        }
    }
}
impl BrowserService {
    pub(crate) fn set_work_admission_probe(&self, probe: Arc<dyn Fn() -> bool + Send + Sync>) {
        *self
            .inner
            .work_admission_probe
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(probe);
    }
    fn admit_profile_start(&self) -> Result<(), BrowserServiceError> {
        let probe = self
            .inner
            .work_admission_probe
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if self.inner.profile_admission_closed.load(Ordering::Acquire)
            || probe.is_some_and(|probe| !probe())
        {
            return Err(BrowserServiceError::Unavailable);
        }
        Ok(())
    }
    pub(crate) fn profile_in_flight_count(&self) -> usize {
        let mut work = self
            .inner
            .profile_work
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        work.retain(|_, task| {
            task.as_ref().is_none_or(|task| {
                let snapshot = task.handle.snapshot();
                snapshot.physical_lease_held || snapshot.close.is_none()
            })
        });
        work.len()
    }
    pub(super) async fn profile_workspace(
        &self,
        agent: &str,
    ) -> Result<PathBuf, BrowserServiceError> {
        let state = self.state()?;
        let guard = state.read().await;
        Self::owner(&guard, agent)?;
        guard
            .workspace
            .as_ref()
            .map(|w| w.root_path.clone())
            .ok_or(BrowserServiceError::Conflict)
    }
    pub(super) fn remember_profile(&self, view: &BrowserProfileView) {
        self.inner
            .profile_versions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(view.id.clone(), view.clone());
    }
    pub(crate) async fn create_profile(
        &self,
        agent: &str,
        input: CreateBrowserProfile,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        let workspace = self.profile_workspace(agent).await?;
        let policy = self
            .policy(agent, &input.policy_id)
            .filter(|p| p.template.authenticated)
            .ok_or(BrowserServiceError::Invalid)?;
        let _gate = self.inner.profile_mutations.lock().await;
        let store = self.inner.profile_store.clone();
        let agent = agent.to_owned();
        let policy = policy.clone();
        let view = tokio::task::spawn_blocking(move || {
            store.create(&workspace, &agent, &policy.id, &policy.template.origin)
        })
        .await
        .map_err(|_| BrowserServiceError::Unavailable)??;
        self.remember_profile(&view);
        Ok(view)
    }
    pub(crate) async fn list_profiles(
        &self,
        agent: &str,
    ) -> Result<Vec<BrowserProfileView>, BrowserServiceError> {
        let workspace = self.profile_workspace(agent).await?;
        let store = self.inner.profile_store.clone();
        let agent = agent.to_owned();
        let mut profiles = tokio::task::spawn_blocking(move || store.list(&workspace, &agent))
            .await
            .map_err(|_| BrowserServiceError::Unavailable)??;
        let setups = self
            .inner
            .profile_setups
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for profile in &mut profiles {
            profile.login_active = setups.contains_key(&profile.id);
        }
        Ok(profiles)
    }
    pub(super) async fn profile_for_policy(
        &self,
        workspace: &PathBuf,
        agent: &str,
        policy: &InstalledBrowserPolicy,
        id: Option<&str>,
    ) -> Result<Option<BrowserProfileView>, BrowserServiceError> {
        let _profile_gate = self.inner.profile_mutations.lock().await;
        if !policy.template.authenticated {
            if id.is_some() {
                return Err(BrowserServiceError::Invalid);
            }
            return Ok(None);
        }
        let id = id.ok_or(BrowserServiceError::Invalid)?.to_owned();
        let store = self.inner.profile_store.clone();
        let workspace = workspace.clone();
        let agent = agent.to_owned();
        let view = tokio::task::spawn_blocking(move || store.get(&workspace, &agent, &id))
            .await
            .map_err(|_| BrowserServiceError::Unavailable)??;
        if view.policy_id != policy.id
            || view.origin != policy.template.origin
            || !view.ready
            || view.revoked
        {
            return Err(BrowserServiceError::Conflict);
        }
        self.remember_profile(&view);
        Ok(Some(view))
    }
    pub(super) fn profile_current(&self, view: &Option<BrowserProfileView>) -> bool {
        view.as_ref().is_none_or(|view| {
            self.inner
                .profile_versions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&view.id)
                == Some(view)
        })
    }
    pub(super) async fn revalidate_profile_disk(
        &self,
        workspace: PathBuf,
        view: Option<BrowserProfileView>,
    ) -> Result<(), BrowserError> {
        if let Some(view) = view {
            let store = self.inner.profile_store.clone();
            let expected = view.clone();
            let observed = tokio::task::spawn_blocking(move || {
                store.get(&workspace, &expected.agent_id, &expected.id)
            })
            .await
            .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?
            .map_err(|_| browser_error(BrowserErrorCode::Stale))?;
            if observed != view || view.revoked || !view.ready {
                return Err(browser_error(BrowserErrorCode::Stale));
            }
        }
        Ok(())
    }
    pub(super) async fn retire_profile(&self, id: &str) -> bool {
        let runs = {
            let mut rows = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            rows.iter_mut()
                .filter(|(_, row)| row.profile.as_ref().is_some_and(|p| p.id == id))
                .map(|(run, row)| {
                    Self::close_record(row, Some("Browser profile changed"));
                    run.clone()
                })
                .collect::<Vec<_>>()
        };
        for run in &runs {
            self.retire_run(run).await;
        }
        let setup = self
            .inner
            .profile_setups
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
        let mut clean = true;
        if let Some(setup) = setup {
            clean = setup.task.handle.close().await == BrowserClose::Closed
                && !setup.task.handle.snapshot().physical_lease_held;
        }
        self.profile_in_flight_count();
        let rows = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
        clean
            && runs.iter().all(|run| {
                rows.get(run)
                    .is_none_or(|row| !row.view.physical_lease_held)
            })
    }
    pub(crate) async fn start_profile_login(
        &self,
        agent: &str,
        id: &str,
        input: StartBrowserLogin,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        self.admit_profile_start()?;
        let service = self.clone();
        let agent = agent.to_owned();
        let id = id.to_owned();
        tokio::spawn(async move { service.start_profile_login_owned(&agent, &id, input).await })
            .await
            .map_err(|_| BrowserServiceError::Unavailable)?
    }
    async fn start_profile_login_owned(
        &self,
        agent: &str,
        id: &str,
        input: StartBrowserLogin,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        self.admit_profile_start()?;
        let mut work = ProfileStartWork::new(self);
        let workspace = self.profile_workspace(agent).await?;
        let _gate = self.inner.profile_mutations.lock().await;
        self.admit_profile_start()?;
        let store = self.inner.profile_store.clone();
        let a = agent.to_owned();
        let i = id.to_owned();
        let w = workspace.clone();
        let old = tokio::task::spawn_blocking(move || store.get(&w, &a, &i))
            .await
            .map_err(|_| BrowserServiceError::Unavailable)??;
        if old.revision != input.expected_revision || old.revoked {
            return Err(BrowserServiceError::Conflict);
        }
        let policy = self
            .policy(agent, &old.policy_id)
            .filter(|p| {
                p.template.authenticated
                    && p.template.origin == old.origin
                    && p.template
                        .routes
                        .iter()
                        .any(|r| r.id == input.route_id && r.method == BrowserMethod::Get)
            })
            .ok_or(BrowserServiceError::Invalid)?;
        if !self.retire_profile(id).await {
            return Err(BrowserServiceError::Busy);
        }
        let store = self.inner.profile_store.clone();
        let w = workspace.clone();
        let old = old.clone();
        let view = tokio::task::spawn_blocking(move || store.begin_login(&w, &old))
            .await
            .map_err(|_| BrowserServiceError::Unavailable)??;
        self.remember_profile(&view);
        let store = self.inner.profile_store.clone();
        let v = view.clone();
        let login_route = input.route_id.clone();
        let profile =
            tokio::task::spawn_blocking(move || store.acquire(&workspace, &v, true, login_route))
                .await
                .map_err(|_| BrowserServiceError::Unavailable)??;
        let installation = self
            .inner
            .policies
            .installation
            .as_ref()
            .ok_or(BrowserServiceError::Unavailable)?;
        let factory = self
            .inner
            .factory
            .as_ref()
            .ok_or(BrowserServiceError::Unavailable)?;
        let mut scope = policy.template.clone();
        scope.id = format!("login-{}", uuid::Uuid::new_v4());
        scope.revision = view.revision;
        let expires_at_ms = now_millis() + u64::from(scope.limits.scope_ms);
        let deadline = Instant::now() + Duration::from_millis(u64::from(scope.limits.scope_ms));
        let task = Arc::new(
            factory
                .bind(PreparedBrowserTask {
                    scope: scope.clone(),
                    policy_id: policy.id.clone(),
                    policy_digest: scope_digest(&policy.template)
                        .map_err(|_| BrowserServiceError::Invalid)?,
                    installation_id: installation.id.clone(),
                    installation_digest: installation.digest.clone(),
                    expires_at_ms,
                    inputs: vec![],
                    outputs: profile_outputs(),
                    profile: Some(profile),
                })
                .map_err(|_| BrowserServiceError::Unavailable)?,
        );
        self.admit_profile_start()?;
        work.bind(task.clone());
        match task
            .port
            .open(
                scope,
                BrowserControl {
                    cancel: CancelSignal::new(),
                    deadline,
                },
            )
            .await
        {
            Ok(session) => {
                work.retained = true;
                self.inner
                    .profile_setups
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(
                        id.into(),
                        ProfileSetup {
                            view: view.clone(),
                            task,
                            _session: session,
                        },
                    );
                Ok(BrowserProfileView {
                    login_active: true,
                    ..view
                })
            }
            Err(_) => {
                task.handle.close().await;
                Err(BrowserServiceError::Unavailable)
            }
        }
    }
    pub(crate) async fn finish_profile_login(
        &self,
        agent: &str,
        id: &str,
        input: BrowserProfileMutation,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        let service = self.clone();
        let agent = agent.to_owned();
        let id = id.to_owned();
        tokio::spawn(async move {
            let workspace = service.profile_workspace(&agent).await?;
            let _gate = service.inner.profile_mutations.lock().await;
            let setup = service
                .inner
                .profile_setups
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id)
                .ok_or(BrowserServiceError::Conflict)?;
            if setup.view.agent_id != agent || setup.view.revision != input.expected_revision {
                service
                    .inner
                    .profile_setups
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(id, setup);
                return Err(BrowserServiceError::Conflict);
            }
            let marker = setup.task.handle.finish_profile_login().await;
            // Marker failure must not abandon the owned physical context.
            let close = setup.task.handle.close().await;
            let physical_lease_held = setup.task.handle.snapshot().physical_lease_held;
            service.profile_in_flight_count();
            if marker.is_err() || close != BrowserClose::Closed || physical_lease_held {
                return Err(BrowserServiceError::Unavailable);
            }
            let store = service.inner.profile_store.clone();
            let view = setup.view;
            let finished = tokio::task::spawn_blocking(move || store.finish(&workspace, &view))
                .await
                .map_err(|_| BrowserServiceError::Unavailable)??;
            service.remember_profile(&finished);
            Ok(finished)
        })
        .await
        .map_err(|_| BrowserServiceError::Unavailable)?
    }
    pub(crate) async fn revoke_profile(
        &self,
        agent: &str,
        id: &str,
        input: BrowserProfileMutation,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        let service = self.clone();
        let agent = agent.to_owned();
        let id = id.to_owned();
        tokio::spawn(async move {
            let workspace = service.profile_workspace(&agent).await?;
            let _gate = service.inner.profile_mutations.lock().await;
            let store = service.inner.profile_store.clone();
            let w = workspace.clone();
            let a = agent.clone();
            let i = id.clone();
            let view = tokio::task::spawn_blocking(move || store.get(&w, &a, &i))
                .await
                .map_err(|_| BrowserServiceError::Unavailable)??;
            if view.revision != input.expected_revision {
                return Err(BrowserServiceError::Conflict);
            }
            // Fence dispatch immediately, then durably revoke before waiting on
            // physical closure. A crash during close cannot restore authority.
            let mut fence = view.clone();
            fence.revoked = true;
            fence.ready = false;
            fence.revision = fence.revision.saturating_add(1);
            service.remember_profile(&fence);
            let store = service.inner.profile_store.clone();
            let w = workspace.clone();
            let revoked = tokio::task::spawn_blocking(move || store.revoke(&w, &view))
                .await
                .map_err(|_| BrowserServiceError::Unavailable)
                .and_then(|result| result);
            if let Ok(view) = &revoked {
                service.remember_profile(view);
            }
            let clean = service.retire_profile(&id).await;
            let store = service.inner.profile_store.clone();
            let purge_id = id.clone();
            tokio::task::spawn_blocking(move || store.purge_sealed(&workspace, &purge_id))
                .await
                .map_err(|_| BrowserServiceError::Unavailable)??;
            if !clean {
                return Err(BrowserServiceError::Unavailable);
            }
            revoked
        })
        .await
        .map_err(|_| BrowserServiceError::Unavailable)?
    }
}
fn profile_outputs() -> Vec<OutputSlot> {
    [
        BrowserArtifactKind::Csv,
        BrowserArtifactKind::Screenshot,
        BrowserArtifactKind::Csv,
        BrowserArtifactKind::Screenshot,
    ]
    .into_iter()
    .map(|kind| OutputSlot {
        id: format!("output-{}", uuid::Uuid::new_v4()),
        kind,
    })
    .collect()
}

#[cfg(test)]
mod profile_dispatch_tests {
    use super::*;
    #[tokio::test]
    async fn authenticated_profile_revision_is_in_the_approved_dispatch_and_revoke_fences_it() {
        let fixture = crate::browser::tests::Fixture::with_tools_model(
            120_000,
            crate::agent_runs::test_support::ScriptedModel::new(vec![]),
            &["browser_navigate"],
        )
        .await;
        let custody = tempfile::tempdir().unwrap();
        let mut policies = crate::browser::tests::policies(120_000);
        policies
            .policies
            .get_mut("controlled-operator-policy")
            .unwrap()
            .template
            .authenticated = true;
        let mut service = BrowserService::new(
            &fixture.state,
            fixture.coordinator.control_plane_transactions(),
            policies,
            Arc::new(UnavailableBrowserPort),
        );
        Arc::get_mut(&mut service.inner).unwrap().profile_store =
            crate::browser::profiles::ProfileStore::for_test(custody.path().join("profiles"));
        let created = service
            .create_profile(
                &fixture.run.agent_id,
                CreateBrowserProfile {
                    policy_id: "controlled-operator-policy".into(),
                },
            )
            .await
            .unwrap();
        let workspace = service
            .profile_workspace(&fixture.run.agent_id)
            .await
            .unwrap();
        let store = service.inner.profile_store.clone();
        let setup = store.begin_login(&workspace, &created).unwrap();
        // Only the real-worker test asserts encryption and successful sign-in. This
        // controlled metadata fixture isolates approval and revision invalidation.
        std::fs::write(
            custody
                .path()
                .join("profiles")
                .join(format!("{}.sealed", setup.id)),
            b"sealed-fixture",
        )
        .unwrap();
        let ready = store.finish(&workspace, &setup).unwrap();
        service
            .save(
                &fixture.run.agent_id,
                &fixture.run.run_id,
                SaveBrowserScope {
                    profile_id: Some(ready.id.clone()),
                    policy_id: ready.policy_id.clone(),
                    selected_input_ids: vec![],
                    enabled: true,
                    expected_version: None,
                },
            )
            .await
            .unwrap();
        let call = anima_core::ToolCall {
            id: "approved-auth-navigation".into(),
            name: "browser_navigate".into(),
            args: BTreeMap::new(),
        };
        let stamp = service
            .capture_dispatch(
                &fixture.run,
                &call,
                BrowserOperation::Navigate {
                    route_id: "page".into(),
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(stamp.profile.as_ref().unwrap().revision, ready.revision);
        assert_eq!(stamp.profile.as_ref().unwrap().digest, ready.digest);
        service.revalidate_dispatch(&stamp).await.unwrap();
        let prepared = service
            .prepare_selection(
                &fixture.run.agent_id,
                &fixture.run.session_id,
                BrowserSelection {
                    profile_id: Some(ready.id.clone()),
                    policy_id: ready.policy_id.clone(),
                    selected_input_ids: vec![],
                },
            )
            .await
            .unwrap();
        service
            .revoke_profile(
                &fixture.run.agent_id,
                &ready.id,
                BrowserProfileMutation {
                    expected_revision: ready.revision,
                },
            )
            .await
            .unwrap();
        assert!(service.revalidate_dispatch(&stamp).await.is_err());
        assert!(!service.prepared_current(
            &*fixture.state.read().await,
            &fixture.run.agent_id,
            &fixture.run.session_id,
            &prepared
        ));
        assert!(service
            .prepare_selection(
                &fixture.run.agent_id,
                &fixture.run.session_id,
                BrowserSelection {
                    profile_id: Some(ready.id),
                    policy_id: ready.policy_id,
                    selected_input_ids: vec![]
                }
            )
            .await
            .is_err());
    }
}

#[cfg(all(test, windows))]
mod native_profile_acceptance {
    use super::*;
    use crate::agent_runs::test_support::{ledger_run, ScriptedModel};
    use axum::{
        http::{HeaderMap, HeaderValue},
        response::Html,
        routing::get,
        Router,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CredentialCleanup {
        store: crate::browser::profiles::ProfileStore,
        id: String,
    }
    impl Drop for CredentialCleanup {
        fn drop(&mut self) {
            self.store.cleanup_test_key(&self.id);
        }
    }
    #[tokio::test]
    #[ignore = "explicit pinned worker and Windows OS-keyring acceptance; synthetic localhost site only"]
    async fn browser_authenticated_native_worker_restart_revoke_and_one_lease() {
        let config = std::env::var_os("ANIMAOS_RS_BROWSER_PROFILE_TEST_CONFIG")
            .map(PathBuf::from)
            .expect("explicit verified Node/worker/Chromium config required");
        let installed =
            crate::browser::config::load_startup_setup(&config).expect("pinned installation");
        let oracle = format!("PROTECTED-ORACLE-{}", uuid::Uuid::new_v4());
        let secret = format!("private-session-{}", uuid::Uuid::new_v4());
        let hits = Arc::new(AtomicUsize::new(0));
        let issued = secret.clone();
        let expected = secret.clone();
        let evidence = oracle.clone();
        let read_hits = hits.clone();
        let router = Router::new()
            .route(
                "/login",
                get(move || {
                    let issued = issued.clone();
                    async move {
                        let mut headers = HeaderMap::new();
                        headers.insert(
                            "set-cookie",
                            HeaderValue::from_str(&format!(
                                "session={issued}; Path=/; HttpOnly; SameSite=Strict"
                            ))
                            .unwrap(),
                        );
                        (headers, Html("<h1>Owner sign-in completed</h1>"))
                    }
                }),
            )
            .route(
                "/protected",
                get(move |headers: HeaderMap| {
                    let expected = expected.clone();
                    let evidence = evidence.clone();
                    let hits = read_hits.clone();
                    async move {
                        if headers.get("cookie").and_then(|value| value.to_str().ok())
                            == Some(format!("session={expected}").as_str())
                        {
                            hits.fetch_add(1, Ordering::SeqCst);
                            Html(format!("<button>{evidence}</button>"))
                        } else {
                            Html("<h1>Sign in required</h1>".into())
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let stop = tokio_util::sync::CancellationToken::new();
        let stop_server = stop.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(stop_server.cancelled_owned())
                .await
                .unwrap()
        });
        let fixture = crate::browser::tests::Fixture::with_tools_model(
            120_000,
            ScriptedModel::new(vec![]),
            &["browser_navigate"],
        )
        .await;
        let custody = tempfile::tempdir().unwrap();
        let custody_root = custody.path().join("profiles");
        let store =
            crate::browser::profiles::ProfileStore::with_system_keys_for_test(custody_root.clone());
        let configured = |installation: &InstalledBrowserPolicies| {
            let mut table = crate::browser::tests::policies(120_000);
            let mut policy = table.policies.remove("controlled-operator-policy").unwrap();
            policy.id = "authenticated-fixture".into();
            policy.template.authenticated = true;
            policy.template.origin = origin.clone();
            policy.template.routes = vec![
                anima_service_adapters::browser::BrowserRoute {
                    id: "login".into(),
                    method: BrowserMethod::Get,
                    path: "/login".into(),
                },
                anima_service_adapters::browser::BrowserRoute {
                    id: "protected".into(),
                    method: BrowserMethod::Get,
                    path: "/protected".into(),
                },
            ];
            InstalledBrowserPolicies::new(installation.installation.clone().unwrap(), vec![policy])
                .unwrap()
        };
        let mut service = BrowserService::new_with_factory(
            &fixture.state,
            fixture.coordinator.control_plane_transactions(),
            configured(&installed.policies),
            installed.factory,
        );
        Arc::get_mut(&mut service.inner).unwrap().profile_store = store.clone();
        let profile = service
            .create_profile(
                &fixture.run.agent_id,
                CreateBrowserProfile {
                    policy_id: "authenticated-fixture".into(),
                },
            )
            .await
            .expect("OS keyring must be available");
        let _credential_cleanup = CredentialCleanup {
            store: store.clone(),
            id: profile.id.clone(),
        };
        let signing = service
            .start_profile_login(
                &fixture.run.agent_id,
                &profile.id,
                StartBrowserLogin {
                    expected_revision: profile.revision,
                    route_id: "login".into(),
                },
            )
            .await
            .unwrap();
        let private = service
            .inner
            .profile_setups
            .lock()
            .unwrap()
            .get(&profile.id)
            .unwrap()
            .task
            .handle
            .private_witness()
            .unwrap();
        let ready = service
            .finish_profile_login(
                &fixture.run.agent_id,
                &profile.id,
                BrowserProfileMutation {
                    expected_revision: signing.revision,
                },
            )
            .await
            .unwrap();
        assert!(ready.ready);
        assert!(!private.private_root.exists());
        let sealed =
            std::fs::read_to_string(custody_root.join(format!("{}.sealed", profile.id))).unwrap();
        assert!(!sealed.contains(&secret));
        let selection = || SaveBrowserScope {
            profile_id: Some(ready.id.clone()),
            policy_id: ready.policy_id.clone(),
            selected_input_ids: vec![],
            enabled: true,
            expected_version: None,
        };
        let call = anima_core::ToolCall {
            id: "protected-read".into(),
            name: "browser_navigate".into(),
            args: BTreeMap::new(),
        };
        service
            .save(&fixture.run.agent_id, &fixture.run.run_id, selection())
            .await
            .unwrap();
        let approved = service
            .capture_dispatch(
                &fixture.run,
                &call,
                BrowserOperation::Navigate {
                    route_id: "protected".into(),
                },
                None,
            )
            .await
            .unwrap();
        let content = service.invoke(approved).await.unwrap();
        assert!(content.text.contains(&oracle));
        assert!(!content.text.contains(&secret));
        service.shutdown().await;
        // A fresh native runtime/factory and empty scope registry must restore only the
        // sealed dedicated custody, then obtain a new ordinary run's approval stamp.
        let installed = crate::browser::config::load_startup_setup(&config).unwrap();
        let mut restarted = BrowserService::new_with_factory(
            &fixture.state,
            fixture.coordinator.control_plane_transactions(),
            configured(&installed.policies),
            installed.factory,
        );
        Arc::get_mut(&mut restarted.inner).unwrap().profile_store = store;
        let fresh = ledger_run(
            &fixture.coordinator,
            &fixture.run.agent_id,
            "chat:authenticated-restart",
        )
        .await;
        fixture
            .state
            .read()
            .await
            .live
            .runs()
            .register(&fresh.run_id);
        restarted
            .save(&fresh.agent_id, &fresh.run_id, selection())
            .await
            .unwrap();
        let fresh_approval = restarted
            .capture_dispatch(
                &fresh,
                &call,
                BrowserOperation::Navigate {
                    route_id: "protected".into(),
                },
                None,
            )
            .await
            .unwrap();
        let content = restarted.invoke(fresh_approval).await.unwrap();
        assert!(content.text.contains(&oracle));
        let held = restarted.private_witness(&fresh.run_id).unwrap();
        let competing = ledger_run(
            &fixture.coordinator,
            &fixture.run.agent_id,
            "chat:authenticated-competing",
        )
        .await;
        fixture
            .state
            .read()
            .await
            .live
            .runs()
            .register(&competing.run_id);
        restarted
            .save(&competing.agent_id, &competing.run_id, selection())
            .await
            .unwrap();
        let competing_stamp = restarted
            .capture_dispatch(
                &competing,
                &call,
                BrowserOperation::Navigate {
                    route_id: "protected".into(),
                },
                None,
            )
            .await
            .unwrap();
        assert!(restarted.invoke(competing_stamp).await.is_err());
        let stale = restarted
            .capture_dispatch(
                &fresh,
                &call,
                BrowserOperation::Navigate {
                    route_id: "protected".into(),
                },
                None,
            )
            .await
            .unwrap();
        restarted
            .revoke_profile(
                &fresh.agent_id,
                &ready.id,
                BrowserProfileMutation {
                    expected_revision: ready.revision,
                },
            )
            .await
            .unwrap();
        assert!(restarted.revalidate_dispatch(&stale).await.is_err());
        assert!(!held.private_root.exists());
        assert!(!custody_root.join(format!("{}.sealed", ready.id)).exists());
        assert!(restarted
            .prepare_selection(
                &fresh.agent_id,
                &fresh.session_id,
                BrowserSelection {
                    profile_id: Some(ready.id),
                    policy_id: ready.policy_id,
                    selected_input_ids: vec![]
                }
            )
            .await
            .is_err());
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        restarted.shutdown().await;
        stop.cancel();
        server.await.unwrap();
    }
}

#[cfg(test)]
#[path = "service_profile_lifecycle_tests.rs"]
mod lifecycle_tests;
