//! Domain regressions use metadata-only transport counters; the separate
//! executor acceptance exercises the actual SDK/native stdio peer.
use super::*;
use crate::control_plane_store::{
    load_control_plane_snapshot, ControlPlaneStoreConfig, WorkspaceConfig,
};
use crate::mcp::credentials::{
    InMemoryMcpCredentialStore, McpBearerToken, McpCredentialError, McpCredentialStore,
};
use crate::mcp::transport::{
    McpTransportError, McpTransportFactory, McpTransportSession, TransportControl,
    ValidatedMcpLaunch,
};
use crate::mcp::{CreateMcpConnection, McpManager, McpServiceError, UpdateMcpConnection};
use crate::skills::{DraftApproval, SkillContent, SkillService};
use crate::state::DaemonState;
use futures::future::BoxFuture;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tempfile::TempDir;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout, Duration};

#[derive(Default)]
struct NoLaunch(AtomicUsize);
impl McpTransportFactory for NoLaunch {
    fn connect(
        &self,
        _: ValidatedMcpLaunch,
        _: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(McpTransportError::Unavailable) })
    }
}
#[derive(Default)]
struct BundleVault {
    inner: InMemoryMcpCredentialStore,
    deletes: std::sync::Mutex<Vec<(String, String)>>,
    fail_at: AtomicUsize,
    uncertain: AtomicUsize,
}
#[async_trait::async_trait]
impl McpCredentialStore for BundleVault {
    async fn load(
        &self,
        id: &str,
        endpoint: &str,
        reference: &str,
    ) -> Result<Option<McpBearerToken>, McpCredentialError> {
        self.inner.load(id, endpoint, reference).await
    }
    async fn put(
        &self,
        id: &str,
        endpoint: &str,
        reference: &str,
        token: McpBearerToken,
    ) -> Result<(), McpCredentialError> {
        self.inner.put(id, endpoint, reference, token).await
    }
    async fn delete(&self, id: &str, reference: &str) -> Result<(), McpCredentialError> {
        let call = {
            let mut calls = self.deletes.lock().unwrap();
            calls.push((id.into(), reference.into()));
            calls.len()
        };
        if self.fail_at.load(Ordering::SeqCst) == call {
            if self.uncertain.load(Ordering::SeqCst) != 0 {
                self.inner.delete(id, reference).await?;
                return Err(McpCredentialError::Uncertain);
            }
            return Err(McpCredentialError::Unavailable);
        }
        self.inner.delete(id, reference).await
    }
}
struct Fixture {
    folder: TempDir,
    state: crate::app::SharedDaemonState,
    agent: String,
    mcp: McpManager,
    bundles: BundleService,
    skills: SkillService,
    factory: Arc<NoLaunch>,
    vault: Arc<BundleVault>,
}
impl Fixture {
    async fn new() -> Self {
        let folder = TempDir::new().unwrap();
        let mut daemon = DaemonState::new();
        daemon.workspace = Some(WorkspaceConfig {
            root_path: folder.path().into(),
            company_name: "Synthetic bundle workspace".into(),
            mission: "Controlled metadata evidence".into(),
            values: vec![],
        });
        daemon.control_plane_store = Some(ControlPlaneStoreConfig::Json(
            folder.path().join("control.json"),
        ));
        let mut config = crate::sessions::test_support::agent_config("Bundle owner");
        config.tools = Some(vec![]);
        let agent = daemon.create_agent(config).unwrap().state.id;
        let publication = daemon.mcp_publication.clone();
        let state = Arc::new(RwLock::new(daemon));
        let factory = Arc::new(NoLaunch::default());
        let vault = Arc::new(BundleVault::default());
        let mcp = McpManager::with_credentials(
            Arc::downgrade(&state),
            Arc::new(Mutex::new(())),
            publication,
            factory.clone(),
            vault.clone(),
        );
        state.write().await.mcp_manager = Some(mcp.clone());
        let bundles = BundleService::new(state.clone(), mcp.clone());
        let skills = SkillService::new(state.clone(), mcp.control_plane_transactions());
        Self {
            folder,
            state,
            agent,
            mcp,
            bundles,
            skills,
            factory,
            vault,
        }
    }
    async fn review(
        &self,
        body: &str,
        endpoint: &str,
        target: Option<(&str, u64)>,
    ) -> BundleReview {
        self.bundles
            .review(
                &self.agent,
                ReviewRequest {
                    manifest_text: manifest(body, endpoint),
                    program_bindings: BTreeMap::new(),
                    bundle_id: target.map(|(id, _)| id.into()),
                    expected_bundle_version: target.map(|(_, v)| v),
                },
            )
            .await
            .unwrap()
    }
    async fn install(&self, review: &BundleReview) -> BundleView {
        self.bundles
            .install(
                &self.agent,
                InstallRequest {
                    review_id: review.id.clone(),
                    review_digest: review.review_digest.clone(),
                },
            )
            .await
            .unwrap()
    }
    async fn saved(&self) -> crate::control_plane_store::ControlPlaneSnapshot {
        load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(
            self.folder.path().join("control.json"),
        ))
        .await
        .unwrap()
        .unwrap()
    }
    fn no_launch(&self) {
        assert_eq!(self.factory.0.load(Ordering::SeqCst), 0);
    }
}
fn manifest(body: &str, endpoint: &str) -> String {
    json!({"schemaVersion":1,"id":"synthetic.violet","version":"1.0.0","name":"Synthetic violet bundle","description":"Controlled declarative metadata",
 "compatibility":{"host":"rust-daemon","minVersion":"0.0.0","maxVersionExclusive":"999.0.0","requiredCapabilities":["mcp.owner.v1","skills.import-draft.v1"]},
 "permissions":{"nativeToolNames":["read_file"],"externalTools":[{"component":"record","remoteName":"synthetic_record"}]},
 "attribution":[{"component":"synthetic","source":"locally authored test fixture","revision":"synthetic-v1","license":"MIT","notice":"Synthetic only"}],
 "components":[{"kind":"mcpHttp","key":"record","label":"Synthetic record","endpoint":endpoint},{"kind":"skill","key":"procedure","skillFile":format!("---\nname: Violet procedure\ndescription: Read controlled evidence\n---\n\n{body}\n")}] }).to_string()
}
fn skill(view: &BundleView) -> &SkillComponentView {
    view.components
        .iter()
        .find_map(|part| match part {
            BundleComponentView::Skill(part) => Some(part),
            _ => None,
        })
        .unwrap()
}
fn connection(view: &BundleView) -> &McpComponentView {
    view.components
        .iter()
        .find_map(|part| match part {
            BundleComponentView::Mcp(part) => Some(part),
            _ => None,
        })
        .unwrap()
}
fn change(view: &BundleView, enabled: bool) -> ChangeRequest {
    ChangeRequest {
        expected_version: view.summary.version,
        reviewed_component_state_digest: view.summary.reviewed_component_state_digest.clone(),
        enabled,
    }
}
fn removal(view: &BundleView) -> RemoveRequest {
    RemoveRequest {
        expected_version: view.summary.version,
        reviewed_component_state_digest: view.summary.reviewed_component_state_digest.clone(),
    }
}

#[tokio::test]
async fn m65_bundle_remove_cleans_only_owned_current_and_predecessor_slots_after_retirement() {
    let f = Fixture::new().await;
    let review = f
        .review("Synthetic procedure", "https://m65.invalid/first", None)
        .await;
    let installed = f.install(&review).await;
    let id = installed.summary.id.clone();
    let old_id = connection(&installed).connection_id.clone();
    let old = f
        .mcp
        .set_bearer(
            &f.agent,
            &old_id,
            connection(&installed).connection_version,
            McpBearerToken::parse("m65-bundle-first".into()).unwrap(),
        )
        .await
        .unwrap()
        .record;
    let review = f
        .review(
            "Synthetic procedure",
            "https://m65.invalid/second",
            Some((&id, installed.summary.version)),
        )
        .await;
    let updated = f
        .bundles
        .update(
            &f.agent,
            &id,
            UpdateRequest {
                expected_version: installed.summary.version,
                review_id: review.id,
                review_digest: review.review_digest,
            },
        )
        .await
        .unwrap();
    let new_id = connection(&updated).connection_id.clone();
    assert_ne!(new_id, old_id);
    let current = f
        .mcp
        .set_bearer(
            &f.agent,
            &new_id,
            connection(&updated).connection_version,
            McpBearerToken::parse("m65-bundle-second".into()).unwrap(),
        )
        .await
        .unwrap()
        .record;
    let neighbor = f
        .mcp
        .create(
            &f.agent,
            CreateMcpConnection {
                label: "Unmanaged neighbor".into(),
                endpoint: "https://m65.invalid/neighbor".into(),
            },
        )
        .await
        .unwrap();
    let neighbor = f
        .mcp
        .set_bearer(
            &f.agent,
            &neighbor.record.id,
            neighbor.record.version,
            McpBearerToken::parse("m65-neighbor".into()).unwrap(),
        )
        .await
        .unwrap()
        .record;
    let refreshed = f.bundles.get(&f.agent, &id).await.unwrap();
    let disabled = f
        .bundles
        .set_enabled(&f.agent, &id, change(&refreshed, false))
        .await
        .unwrap();
    assert!(
        f.vault.deletes.lock().unwrap().is_empty(),
        "disable/update preserve retained credentials"
    );
    for row in [&old, &current, &neighbor] {
        assert!(f
            .vault
            .load(
                &row.id,
                &row.endpoint,
                row.bearer_credential_ref.as_ref().unwrap()
            )
            .await
            .unwrap()
            .is_some());
    }
    f.bundles
        .remove(&f.agent, &id, removal(&disabled))
        .await
        .unwrap();
    timeout(Duration::from_secs(1), f.mcp.shutdown())
        .await
        .unwrap();
    for row in [&old, &current] {
        assert!(f
            .vault
            .load(
                &row.id,
                &row.endpoint,
                row.bearer_credential_ref.as_ref().unwrap()
            )
            .await
            .unwrap()
            .is_none());
    }
    assert_eq!(
        f.vault
            .load(
                &neighbor.id,
                &neighbor.endpoint,
                neighbor.bearer_credential_ref.as_ref().unwrap()
            )
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "m65-neighbor"
    );
    let mut expected = vec![
        (old.id, old.bearer_credential_ref.unwrap()),
        (current.id, current.bearer_credential_ref.unwrap()),
    ];
    expected.sort();
    let mut actual = f.vault.deletes.lock().unwrap().clone();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(
        f.state.read().await.mcp_registry.snapshot(),
        vec![neighbor.clone()]
    );
    assert_eq!(f.saved().await.mcp_connections, vec![neighbor]);
    f.no_launch();
}

#[tokio::test]
async fn m65_bundle_remove_vault_failure_retains_removing_intent_and_all_slot_references() {
    for uncertain in [false, true] {
        let f = Fixture::new().await;
        let review = f
            .review("Synthetic procedure", "https://m65.invalid/owned", None)
            .await;
        let installed = f.install(&review).await;
        let id = installed.summary.id.clone();
        let connection = connection(&installed);
        let owned = f
            .mcp
            .set_bearer(
                &f.agent,
                &connection.connection_id,
                connection.connection_version,
                McpBearerToken::parse("m65-bundle-owned".into()).unwrap(),
            )
            .await
            .unwrap()
            .record;
        let review = f
            .review(
                "Synthetic procedure",
                "https://m65.invalid/replacement",
                Some((&id, installed.summary.version)),
            )
            .await;
        let replacement = f
            .bundles
            .update(
                &f.agent,
                &id,
                UpdateRequest {
                    expected_version: installed.summary.version,
                    review_id: review.id,
                    review_digest: review.review_digest,
                },
            )
            .await
            .unwrap();
        let replacement_connection = replacement
            .components
            .iter()
            .find_map(|part| match part {
                BundleComponentView::Mcp(part) => Some(part),
                _ => None,
            })
            .unwrap();
        let current = f
            .mcp
            .set_bearer(
                &f.agent,
                &replacement_connection.connection_id,
                replacement_connection.connection_version,
                McpBearerToken::parse("m65-bundle-current".into()).unwrap(),
            )
            .await
            .unwrap()
            .record;
        let neighbor = f
            .mcp
            .create(
                &f.agent,
                CreateMcpConnection {
                    label: "Unmanaged neighbor".into(),
                    endpoint: "https://m65.invalid/neighbor".into(),
                },
            )
            .await
            .unwrap();
        let neighbor = f
            .mcp
            .set_bearer(
                &f.agent,
                &neighbor.record.id,
                neighbor.record.version,
                McpBearerToken::parse("m65-neighbor".into()).unwrap(),
            )
            .await
            .unwrap()
            .record;
        f.vault.fail_at.store(2, Ordering::SeqCst);
        f.vault
            .uncertain
            .store(usize::from(uncertain), Ordering::SeqCst);
        let refreshed = f.bundles.get(&f.agent, &id).await.unwrap();
        let result = f.bundles.remove(&f.agent, &id, removal(&refreshed)).await;
        timeout(Duration::from_secs(1), f.mcp.shutdown())
            .await
            .unwrap();
        assert!(matches!(result, Err(BundleError::Unavailable)));
        let state = f.state.read().await;
        let bundle = state
            .community_bundles
            .iter()
            .find(|row| row.id == id)
            .unwrap();
        assert_eq!(bundle.state, "removing");
        assert_eq!(bundle.settlement.as_ref().unwrap().kind, "remove");
        assert_eq!(
            state.mcp_registry.connections[&owned.id].bearer_credential_ref,
            owned.bearer_credential_ref
        );
        assert_eq!(
            state.mcp_registry.connections[&current.id].bearer_credential_ref,
            current.bearer_credential_ref
        );
        assert_eq!(state.mcp_registry.connections[&neighbor.id], neighbor);
        let rows = state.mcp_registry.snapshot();
        drop(state);
        let disk = f.saved().await;
        assert_eq!(disk.mcp_connections, rows);
        let mut restored = DaemonState::new();
        restored.restore_control_plane_snapshot(disk).unwrap();
        assert_eq!(restored.mcp_registry.snapshot(), rows);
        assert!(restored.mcp_publication.read().unwrap().live.is_empty());
        let calls = f.vault.deletes.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        let mut observed = calls.clone();
        observed.sort();
        let mut expected = vec![
            (
                owned.id.clone(),
                owned.bearer_credential_ref.clone().unwrap(),
            ),
            (
                current.id.clone(),
                current.bearer_credential_ref.clone().unwrap(),
            ),
        ];
        expected.sort();
        assert_eq!(observed, expected);
        for row in [&owned, &current] {
            assert_eq!(
                f.vault
                    .load(
                        &row.id,
                        &row.endpoint,
                        row.bearer_credential_ref.as_ref().unwrap()
                    )
                    .await
                    .unwrap()
                    .is_none(),
                row.id == calls[0].0 || uncertain,
                "known prior cleanup stays visible without losing any owned reference"
            );
        }
        assert_eq!(
            f.vault
                .load(
                    &neighbor.id,
                    &neighbor.endpoint,
                    neighbor.bearer_credential_ref.as_ref().unwrap()
                )
                .await
                .unwrap()
                .unwrap()
                .expose(),
            "m65-neighbor"
        );
        f.no_launch();
    }
}

#[tokio::test]
async fn m45_unchanged_background_scan_keeps_review_installable_without_new_authority() {
    let f = Fixture::new().await;
    f.skills
        .save(
            "neighbor",
            SkillContent {
                name: "Unmanaged neighbor".into(),
                description: "Separately approved owner content".into(),
                body: "Keep approved unmanaged bytes.".into(),
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    // Establish the scanner's ordinary steady state before issuing the review.
    f.skills.scan().await.unwrap();
    let neighbor = f.state.read().await.skills.get("neighbor").unwrap().clone();
    let neighbor_bytes = std::fs::read(f.folder.path().join("skills/neighbor/SKILL.md")).unwrap();
    let grants = f.state.read().await.agents[&f.agent].config().tools.clone();
    let review = f
        .review(
            "New content waits for separate adoption.",
            "http://127.0.0.1:9432/mcp",
            None,
        )
        .await;
    assert!(
        !f.skills.scan().await.unwrap(),
        "the background scan found no semantic change"
    );
    assert_eq!(f.state.read().await.skills.get("neighbor"), Some(&neighbor));
    assert!(anima_core::primitives::now_millis() < review.expires_at_ms);
    let installed = f
        .bundles
        .install(
            &f.agent,
            InstallRequest {
                review_id: review.id,
                review_digest: review.review_digest,
            },
        )
        .await
        .expect("an unchanged background scan must keep an exact in-TTL review installable");
    assert_eq!(f.state.read().await.skills.get("neighbor"), Some(&neighbor));
    assert_eq!(f.state.read().await.agents[&f.agent].config().tools, grants);
    assert_eq!(
        std::fs::read(f.folder.path().join("skills/neighbor/SKILL.md")).unwrap(),
        neighbor_bytes
    );
    assert_eq!(
        f.skills.load_slug("neighbor").await.unwrap().body,
        "Keep approved unmanaged bytes."
    );
    assert_eq!(skill(&installed).status, "pending");
    assert_eq!(skill(&installed).draft_status.as_deref(), Some("pending"));
    assert!(skill(&installed).approved_hash.is_none());
    assert!(!connection(&installed).enabled);
    assert!(connection(&installed).reviewed_alias_names.is_empty());
    assert!(installed.tool_names.is_empty());
    assert!(!f
        .folder
        .path()
        .join(format!("skills/{}/SKILL.md", skill(&installed).slug))
        .exists());
    let saved = f.saved().await;
    assert_eq!(saved.skills, vec![neighbor]);
    assert_eq!(saved.skill_drafts.len(), 1);
    assert!(saved.skill_drafts[0].is_pending());
    // A real semantic neighbor mutation still fences an otherwise fresh
    // receipt, without creating its new component identities or authority.
    let stale = f
        .review(
            "Rejected after real semantic change.",
            "http://127.0.0.1:9433/mcp",
            None,
        )
        .await;
    f.skills.set_enabled("neighbor", false).await.unwrap();
    assert!(anima_core::primitives::now_millis() < stale.expires_at_ms);
    assert_eq!(
        f.bundles
            .install(
                &f.agent,
                InstallRequest {
                    review_id: stale.id,
                    review_digest: stale.review_digest,
                }
            )
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    let after = f.saved().await;
    assert_eq!(after.community_bundles.len(), 1);
    assert_eq!(after.community_bundles[0].id, installed.summary.id);
    assert_eq!(after.mcp_connections, saved.mcp_connections);
    assert_eq!(after.skill_drafts, saved.skill_drafts);
    assert_eq!(after.skills.len(), 1);
    assert!(!after.skills[0].enabled);
    assert_eq!(f.state.read().await.agents[&f.agent].config().tools, grants);
    assert_eq!(
        std::fs::read(f.folder.path().join("skills/neighbor/SKILL.md")).unwrap(),
        neighbor_bytes
    );
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_review_install_exact_adoption_disable_and_remove_keep_separate_authority() {
    let f = Fixture::new().await;
    let review = f
        .review(
            "Quote returned evidence.",
            "http://127.0.0.1:9432/mcp",
            None,
        )
        .await;
    {
        let state = f.state.read().await;
        assert!(state.community_bundles.is_empty());
        assert!(state.mcp_registry.connections.is_empty());
        assert!(state.skills.snapshot().drafts.is_empty());
        assert!(state.agents[&f.agent]
            .config()
            .tools
            .as_ref()
            .unwrap()
            .is_empty());
    }
    assert!(!f.folder.path().join("skills").exists());
    f.no_launch();
    let installed = f.install(&review).await;
    assert!(installed.summary.enabled);
    assert!(!connection(&installed).enabled);
    assert_eq!(skill(&installed).status, "pending");
    assert!(installed.tool_names.is_empty());
    let member = skill(&installed).clone();
    let draft = member.draft_id.clone().unwrap();
    assert!(f
        .skills
        .approve_draft(
            &draft,
            DraftApproval {
                body: Some("Owner side-channel edit".into()),
                hash: None
            }
        )
        .await
        .is_err());
    assert!(!f.folder.path().join("skills").exists());
    f.skills
        .approve_draft(&draft, DraftApproval::default())
        .await
        .unwrap();
    assert_eq!(
        f.skills.load_slug(&member.slug).await.unwrap().body,
        "Quote returned evidence.\n"
    );
    let bytes = std::fs::read(
        f.folder
            .path()
            .join(format!("skills/{}/SKILL.md", member.slug)),
    )
    .unwrap();
    assert_eq!(crate::skills::skill_hash(&bytes), member.canonical_hash);
    f.skills
        .save(
            "neighbor",
            SkillContent {
                name: "Neighbor".into(),
                description: "Independent owner content".into(),
                body: "Keep neighboring file".into(),
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    let ready = f
        .bundles
        .get(&f.agent, &installed.summary.id)
        .await
        .unwrap();
    let disabled = f
        .bundles
        .set_enabled(&f.agent, &ready.summary.id, change(&ready, false))
        .await
        .unwrap();
    assert_eq!(disabled.summary.state, "disabled");
    assert!(f.skills.load_slug(&member.slug).await.is_err());
    assert!(f
        .skills
        .index()
        .await
        .iter()
        .all(|row| row.slug != member.slug));
    assert!(f.skills.set_enabled(&member.slug, true).await.is_err());
    assert_eq!(
        std::fs::read(
            f.folder
                .path()
                .join(format!("skills/{}/SKILL.md", member.slug))
        )
        .unwrap(),
        bytes
    );
    let enabled = f
        .bundles
        .set_enabled(&f.agent, &disabled.summary.id, change(&disabled, true))
        .await
        .unwrap();
    assert!(f.skills.load_slug(&member.slug).await.is_ok());
    f.no_launch();
    let removed = f
        .bundles
        .remove(&f.agent, &enabled.summary.id, removal(&enabled))
        .await
        .unwrap();
    assert!(removed.removed);
    assert_eq!(removed.trash_paths.len(), 1);
    assert!(removed.tool_names.is_empty());
    assert_eq!(
        std::fs::read(
            f.folder
                .path()
                .join(&removed.trash_paths[0])
                .join("SKILL.md")
        )
        .unwrap(),
        bytes
    );
    assert!(f.folder.path().join("skills/neighbor/SKILL.md").exists());
    assert_eq!(
        f.skills.load_slug("neighbor").await.unwrap().body,
        "Keep neighboring file"
    );
    let saved = f.saved().await;
    assert!(saved.community_bundles.is_empty());
    assert!(saved.mcp_connections.is_empty());
    assert!(saved
        .skill_drafts
        .iter()
        .all(|draft| draft.bundle_source.is_none()));
    assert_eq!(saved.skills.len(), 1);
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_precommit_failure_and_dropped_caller_preserve_single_ordered_candidate() {
    let f = Fixture::new().await;
    let review = f
        .review("Bounded metadata.", "http://127.0.0.1:9432/mcp", None)
        .await;
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let service = f.bundles.clone();
    let agent = f.agent.clone();
    let input = InstallRequest {
        review_id: review.id.clone(),
        review_digest: review.review_digest.clone(),
    };
    let task = tokio::spawn(async move { service.install(&agent, input).await });
    timeout(Duration::from_secs(2), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    {
        let state = f.state.read().await;
        assert!(state.community_bundles.is_empty());
        assert!(state.skills.snapshot().drafts.is_empty());
        assert!(state.mcp_registry.connections.is_empty());
    }
    gate.release.add_permits(1);
    assert_eq!(task.await.unwrap().unwrap_err(), BundleError::Unavailable);
    assert_eq!(f.state.read().await.community_reviews.len(), 1);
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let service = f.bundles.clone();
    let agent = f.agent.clone();
    let input = InstallRequest {
        review_id: review.id.clone(),
        review_digest: review.review_digest.clone(),
    };
    let task = tokio::spawn(async move { service.install(&agent, input).await });
    timeout(Duration::from_secs(2), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let prepared_during_save = f.state.write().await.control_plane_persist_request();
    task.abort();
    gate.release.add_permits(1);
    prepared_during_save.save().await.unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if f.state.read().await.community_bundles.len() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let saved = f.saved().await;
    assert_eq!(saved.community_bundles.len(), 1);
    assert_eq!(saved.mcp_connections.len(), 1);
    assert_eq!(saved.skill_drafts.len(), 1);
    assert!(saved.skills.is_empty());
    assert!(!f.folder.path().join("skills").exists());
    assert_eq!(
        f.bundles
            .install(
                &f.agent,
                InstallRequest {
                    review_id: review.id,
                    review_digest: review.review_digest
                }
            )
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_uncertain_install_live_unavailable_restores_actual_candidate_without_adoption() {
    let f = Fixture::new().await;
    let review = f
        .review("No automatic adoption.", "http://127.0.0.1:9432/mcp", None)
        .await;
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    let service = f.bundles.clone();
    let agent = f.agent.clone();
    let input = InstallRequest {
        review_id: review.id.clone(),
        review_digest: review.review_digest.clone(),
    };
    let task = tokio::spawn(async move { service.install(&agent, input).await });
    timeout(Duration::from_secs(2), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    gate.release.add_permits(1);
    assert_eq!(task.await.unwrap().unwrap_err(), BundleError::Unavailable);
    let observed = f.bundles.get(&f.agent, &review.bundle_id).await.unwrap();
    assert_eq!(observed.summary.state, "unavailable");
    assert!(observed.summary.settlement.is_some());
    assert_eq!(observed.components.len(), 2);
    assert!(f
        .skills
        .approve_draft(
            skill(&observed).draft_id.as_ref().unwrap(),
            DraftApproval::default()
        )
        .await
        .is_err());
    assert!(!f.folder.path().join("skills").exists());
    assert!(observed.tool_names.is_empty());
    f.no_launch();
    // No follow-up save masks the snapshot actually replaced before the
    // uncertain acknowledgement. Restore validates that observed candidate.
    let persisted = f.saved().await;
    assert_eq!(persisted.community_bundles[0].state, "installed");
    assert!(persisted.community_bundles[0].settlement.is_none());
    assert!(persisted.skills.is_empty());
    assert_eq!(persisted.skill_drafts.len(), 1);
    assert!(persisted.skill_drafts[0].is_pending());
    assert!(persisted
        .mcp_connections
        .iter()
        .all(|row| !row.enabled && row.reviewed_tools.is_empty()));
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(persisted).unwrap();
    assert_eq!(restored.community_bundles[0].state, "installed");
    assert!(restored.skills.records().is_empty());
    assert!(restored.skills.pending_drafts()[0].is_pending());
    assert!(restored.mcp_publication.read().unwrap().live.is_empty());
    assert!(restored.agents[&f.agent]
        .config()
        .tools
        .as_ref()
        .unwrap()
        .is_empty());
    assert!(restored.community_reviews.is_empty());
    assert!(!f.folder.path().join("skills").exists());
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_update_preserves_unchanged_ids_and_prunes_removed_predecessor_under_same_cas() {
    let f = Fixture::new().await;
    let original = f
        .install(
            &f.review("Original.", "http://127.0.0.1:9432/mcp", None)
                .await,
        )
        .await;
    let review = f
        .review(
            "Original.",
            "http://127.0.0.1:9432/mcp",
            Some((&original.summary.id, original.summary.version)),
        )
        .await;
    let kept = f
        .bundles
        .update(
            &f.agent,
            &original.summary.id,
            UpdateRequest {
                expected_version: original.summary.version,
                review_id: review.id,
                review_digest: review.review_digest,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        connection(&kept).connection_id,
        connection(&original).connection_id
    );
    assert_eq!(skill(&kept).draft_id, skill(&original).draft_id);
    let review = f
        .review(
            "Replacement.",
            "http://127.0.0.1:9433/mcp",
            Some((&kept.summary.id, kept.summary.version)),
        )
        .await;
    let replacement = f
        .bundles
        .update(
            &f.agent,
            &kept.summary.id,
            UpdateRequest {
                expected_version: kept.summary.version,
                review_id: review.id,
                review_digest: review.review_digest,
            },
        )
        .await
        .unwrap();
    assert_ne!(
        connection(&replacement).connection_id,
        connection(&kept).connection_id
    );
    assert_ne!(skill(&replacement).draft_id, skill(&kept).draft_id);
    assert_eq!(replacement.predecessors.len(), 1);
    assert!(!replacement.predecessors[0].enabled);
    let predecessor = &replacement.predecessors[0];
    f.mcp
        .delete(
            &f.agent,
            &predecessor.connection_id,
            predecessor.connection_version,
        )
        .await
        .unwrap();
    let pruned = f.bundles.get(&f.agent, &kept.summary.id).await.unwrap();
    assert!(pruned.predecessors.is_empty());
    assert!(pruned.summary.version > replacement.summary.version);
    assert_eq!(f.saved().await.mcp_connections.len(), 1);
    assert_eq!(
        f.bundles
            .set_enabled(&f.agent, &pruned.summary.id, change(&replacement, false))
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    let saved = f.saved().await;
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(saved).unwrap();
    f.skills
        .approve_draft(
            skill(&pruned).draft_id.as_ref().unwrap(),
            DraftApproval::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        f.skills.load_slug(&skill(&pruned).slug).await.unwrap().body,
        "Replacement.\n"
    );
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_disabled_unchanged_update_keeps_saved_mcp_enable_preference_without_connect() {
    let f = Fixture::new().await;
    let original = f
        .install(
            &f.review("Preserved procedure.", "http://127.0.0.1:9432/mcp", None)
                .await,
        )
        .await;
    let connection_id = connection(&original).connection_id.clone();
    f.mcp
        .update(
            &f.agent,
            &connection_id,
            UpdateMcpConnection {
                expected_version: connection(&original).connection_version,
                label: None,
                endpoint: None,
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    let before = f.bundles.get(&f.agent, &original.summary.id).await.unwrap();
    assert!(connection(&before).enabled);
    let disabled = f
        .bundles
        .set_enabled(&f.agent, &before.summary.id, change(&before, false))
        .await
        .unwrap();
    assert!(!connection(&disabled).enabled);
    let review = f
        .review(
            "Preserved procedure.",
            "http://127.0.0.1:9432/mcp",
            Some((&disabled.summary.id, disabled.summary.version)),
        )
        .await;
    let updated = f
        .bundles
        .update(
            &f.agent,
            &disabled.summary.id,
            UpdateRequest {
                expected_version: disabled.summary.version,
                review_id: review.id,
                review_digest: review.review_digest,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.summary.state, "disabled");
    assert_eq!(connection(&updated).connection_id, connection_id);
    assert!(!connection(&updated).enabled);
    let enabled = f
        .bundles
        .set_enabled(&f.agent, &updated.summary.id, change(&updated, true))
        .await
        .unwrap();
    assert!(connection(&enabled).enabled);
    assert_eq!(connection(&enabled).connection_id, connection_id);
    assert_eq!(
        connection(&enabled).reviewed_alias_names,
        connection(&before).reviewed_alias_names
    );
    assert_eq!(
        connection(&enabled).granted_alias_names,
        connection(&before).granted_alias_names
    );
    assert_eq!(enabled.tool_names, before.tool_names);
    assert_eq!(connection(&enabled).runtime_state, "disconnected");
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(f.saved().await)
        .unwrap();
    assert!(restored.mcp_registry.connections[&connection_id].enabled);
    assert!(restored.mcp_publication.read().unwrap().live.is_empty());
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_uncertain_explicit_managed_adoption_restores_exact_approved_candidate_without_replay()
{
    let f = Fixture::new().await;
    let installed = f
        .install(
            &f.review(
                "Explicitly adopted exact procedure.",
                "http://127.0.0.1:9432/mcp",
                None,
            )
            .await,
        )
        .await;
    let member = skill(&installed).clone();
    let draft_id = member.draft_id.clone().unwrap();
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    let skills = f.skills.clone();
    let approving = draft_id.clone();
    let task = tokio::spawn(async move {
        skills
            .approve_draft(&approving, DraftApproval::default())
            .await
    });
    timeout(Duration::from_secs(2), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    gate.release.add_permits(1);
    assert!(matches!(
        task.await.unwrap(),
        Err(crate::skills::SkillError::Unavailable(_))
    ));
    let live = f
        .bundles
        .get(&f.agent, &installed.summary.id)
        .await
        .unwrap();
    assert_eq!(live.summary.state, "unavailable");
    assert!(f.skills.load_slug(&member.slug).await.is_err());
    let bytes = std::fs::read(
        f.folder
            .path()
            .join(format!("skills/{}/SKILL.md", member.slug)),
    )
    .unwrap();
    assert_eq!(crate::skills::skill_hash(&bytes), member.canonical_hash);
    let persisted = f.saved().await;
    assert_eq!(persisted.community_bundles[0].state, "installed");
    assert_eq!(persisted.skills.len(), 1);
    assert_eq!(persisted.skills[0].approved_hash, member.canonical_hash);
    let decided = persisted
        .skill_drafts
        .iter()
        .find(|draft| draft.id == draft_id)
        .unwrap()
        .clone();
    assert_eq!(decided.status, crate::skills::DraftStatus::Approved);
    assert!(persisted
        .mcp_connections
        .iter()
        .all(|row| !row.enabled && row.reviewed_tools.is_empty()));
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(persisted).unwrap();
    assert_eq!(restored.skills.draft(&draft_id), Some(&decided));
    assert_eq!(
        restored.skills.get(&member.slug).unwrap().approved_hash,
        member.canonical_hash
    );
    assert!(restored.skills.pending_drafts().is_empty());
    assert!(restored.community_reviews.is_empty());
    assert!(restored.mcp_publication.read().unwrap().live.is_empty());
    assert!(restored.agents[&f.agent]
        .config()
        .tools
        .as_ref()
        .unwrap()
        .is_empty());
    assert_eq!(
        std::fs::read(
            f.folder
                .path()
                .join(format!("skills/{}/SKILL.md", member.slug))
        )
        .unwrap(),
        bytes
    );
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_uncertain_predecessor_delete_fences_only_its_bundle_and_restores_pruned_candidate() {
    let f = Fixture::new().await;
    let original = f
        .install(
            &f.review("Original target.", "http://127.0.0.1:9432/mcp", None)
                .await,
        )
        .await;
    let neighbor = f
        .install(
            &f.review(
                "Approved same-owner neighbor.",
                "http://127.0.0.1:9434/mcp",
                None,
            )
            .await,
        )
        .await;
    let neighbor_member = skill(&neighbor).clone();
    f.skills
        .approve_draft(
            neighbor_member.draft_id.as_ref().unwrap(),
            DraftApproval::default(),
        )
        .await
        .unwrap();
    let neighbor_before = f
        .state
        .read()
        .await
        .community_bundles
        .iter()
        .find(|bundle| bundle.id == neighbor.summary.id)
        .unwrap()
        .clone();
    let review = f
        .review(
            "Replacement target.",
            "http://127.0.0.1:9433/mcp",
            Some((&original.summary.id, original.summary.version)),
        )
        .await;
    let replacement = f
        .bundles
        .update(
            &f.agent,
            &original.summary.id,
            UpdateRequest {
                expected_version: original.summary.version,
                review_id: review.id,
                review_digest: review.review_digest,
            },
        )
        .await
        .unwrap();
    let predecessor = replacement.predecessors[0].clone();
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    let manager = f.mcp.clone();
    let agent = f.agent.clone();
    let deleted = predecessor.connection_id.clone();
    let expected = predecessor.connection_version;
    let task = tokio::spawn(async move { manager.delete(&agent, &deleted, expected).await });
    timeout(Duration::from_secs(2), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    gate.release.add_permits(1);
    assert_eq!(
        task.await.unwrap().unwrap_err(),
        McpServiceError::Unavailable
    );
    let live = f.bundles.get(&f.agent, &original.summary.id).await.unwrap();
    assert_eq!(live.summary.state, "unavailable");
    assert!(live.predecessors.is_empty());
    assert!(f
        .skills
        .approve_draft(
            skill(&live).draft_id.as_ref().unwrap(),
            DraftApproval::default()
        )
        .await
        .is_err());
    assert_eq!(
        f.state
            .read()
            .await
            .community_bundles
            .iter()
            .find(|bundle| bundle.id == neighbor.summary.id),
        Some(&neighbor_before)
    );
    assert_eq!(
        f.skills
            .load_slug(&neighbor_member.slug)
            .await
            .unwrap()
            .body,
        "Approved same-owner neighbor.\n"
    );
    let persisted = f.saved().await;
    assert!(!persisted
        .mcp_connections
        .iter()
        .any(|row| row.id == predecessor.connection_id));
    let target = persisted
        .community_bundles
        .iter()
        .find(|bundle| bundle.id == original.summary.id)
        .unwrap();
    assert_eq!(target.state, "installed");
    assert!(target.predecessors.is_empty());
    assert!(persisted
        .skill_drafts
        .iter()
        .find(|draft| Some(&draft.id) == skill(&live).draft_id.as_ref())
        .unwrap()
        .is_pending());
    assert_eq!(
        persisted
            .community_bundles
            .iter()
            .find(|bundle| bundle.id == neighbor.summary.id),
        Some(&neighbor_before)
    );
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(persisted).unwrap();
    assert_eq!(
        restored
            .skills
            .get(&neighbor_member.slug)
            .unwrap()
            .approved_hash,
        neighbor_member.canonical_hash
    );
    assert!(restored.community_reviews.is_empty());
    assert!(restored.mcp_publication.read().unwrap().live.is_empty());
    assert!(restored.agents[&f.agent]
        .config()
        .tools
        .as_ref()
        .unwrap()
        .is_empty());
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_uncertain_final_remove_retains_live_intent_but_restores_actual_absence_and_trash() {
    let f = Fixture::new().await;
    let installed = f
        .install(
            &f.review("Exact removal bytes.", "http://127.0.0.1:9432/mcp", None)
                .await,
        )
        .await;
    let member = skill(&installed).clone();
    f.skills
        .approve_draft(member.draft_id.as_ref().unwrap(), DraftApproval::default())
        .await
        .unwrap();
    f.skills
        .save(
            "neighbor",
            SkillContent {
                name: "Neighbor".into(),
                description: "Independent approved neighbor".into(),
                body: "Keep unrelated approved bytes.".into(),
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    let bytes = std::fs::read(
        f.folder
            .path()
            .join(format!("skills/{}/SKILL.md", member.slug)),
    )
    .unwrap();
    let view = f
        .bundles
        .get(&f.agent, &installed.summary.id)
        .await
        .unwrap();
    let intent_gate = f
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let service = f.bundles.clone();
    let agent = f.agent.clone();
    let id = view.summary.id.clone();
    let request = removal(&view);
    let task = tokio::spawn(async move { service.remove(&agent, &id, request).await });
    timeout(Duration::from_secs(2), intent_gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    // Existing one-shot gates select the later final erase while the first
    // successful intention save is paused. No new production seam is needed.
    let final_gate = f
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    intent_gate.release.add_permits(1);
    timeout(Duration::from_secs(2), final_gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let intention = f
        .state
        .read()
        .await
        .community_bundles
        .iter()
        .find(|bundle| bundle.id == installed.summary.id)
        .unwrap()
        .clone();
    let destination = intention.settlement.as_ref().unwrap().trash[&member.slug].clone();
    let trash_path = crate::skills::disk::trash_relative_path(&destination);
    assert_eq!(
        std::fs::read(f.folder.path().join(&trash_path).join("SKILL.md")).unwrap(),
        bytes
    );
    assert!(!f
        .folder
        .path()
        .join(format!("skills/{}/SKILL.md", member.slug))
        .exists());
    final_gate.release.add_permits(1);
    assert_eq!(task.await.unwrap().unwrap_err(), BundleError::Unavailable);
    let live = f
        .bundles
        .get(&f.agent, &installed.summary.id)
        .await
        .unwrap();
    assert_eq!(live.summary.state, "removing");
    assert_eq!(
        live.summary.settlement.as_ref().unwrap().phase,
        "reconciliation_required"
    );
    assert!(f.skills.load_slug(&member.slug).await.is_err());
    let persisted = f.saved().await;
    assert!(persisted.community_bundles.is_empty());
    assert!(persisted.mcp_connections.is_empty());
    assert!(persisted.skill_drafts.is_empty());
    assert_eq!(persisted.skills.len(), 1);
    assert_eq!(persisted.skills[0].slug, "neighbor");
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(persisted).unwrap();
    assert!(restored.community_bundles.is_empty());
    assert!(restored.community_reviews.is_empty());
    assert!(restored.skills.get(&member.slug).is_none());
    assert!(restored.skills.get("neighbor").unwrap().enabled);
    assert!(restored.mcp_publication.read().unwrap().live.is_empty());
    assert!(restored.agents[&f.agent]
        .config()
        .tools
        .as_ref()
        .unwrap()
        .is_empty());
    assert_eq!(
        std::fs::read(f.folder.path().join(trash_path).join("SKILL.md")).unwrap(),
        bytes
    );
    assert_eq!(
        f.skills.load_slug("neighbor").await.unwrap().body,
        "Keep unrelated approved bytes."
    );
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_stale_receipts_caps_foreign_owner_and_changed_file_conflict_without_eviction() {
    let f = Fixture::new().await;
    let review = f.review("Caps.", "http://127.0.0.1:9432/mcp", None).await;
    let other = f
        .state
        .write()
        .await
        .create_agent(crate::sessions::test_support::agent_config("Other owner"))
        .unwrap()
        .state
        .id;
    assert_eq!(
        f.bundles
            .install(
                &other,
                InstallRequest {
                    review_id: review.id.clone(),
                    review_digest: review.review_digest.clone()
                }
            )
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    f.state
        .write()
        .await
        .community_reviews
        .get_mut(&review.id)
        .unwrap()
        .view
        .expires_at_ms = 0;
    assert_eq!(
        f.bundles
            .install(
                &f.agent,
                InstallRequest {
                    review_id: review.id,
                    review_digest: review.review_digest
                }
            )
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    for index in 0..4 {
        f.mcp
            .create(
                &f.agent,
                CreateMcpConnection {
                    label: format!("Neighbor {index}"),
                    endpoint: format!("http://127.0.0.1:{}/mcp", 9400 + index),
                },
            )
            .await
            .unwrap();
    }
    let review = f
        .review("Full MCP quota.", "http://127.0.0.1:9432/mcp", None)
        .await;
    assert_eq!(
        f.bundles
            .install(
                &f.agent,
                InstallRequest {
                    review_id: review.id,
                    review_digest: review.review_digest
                }
            )
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    assert_eq!(f.state.read().await.mcp_registry.connections.len(), 4);
    assert!(f.state.read().await.community_bundles.is_empty());
    f.no_launch();
    f.mcp.shutdown().await;
    let f = Fixture::new().await;
    let installed = f
        .install(
            &f.review("Owned file.", "http://127.0.0.1:9432/mcp", None)
                .await,
        )
        .await;
    let member = skill(&installed);
    f.skills
        .approve_draft(member.draft_id.as_ref().unwrap(), DraftApproval::default())
        .await
        .unwrap();
    let view = f
        .bundles
        .get(&f.agent, &installed.summary.id)
        .await
        .unwrap();
    std::fs::write(
        f.folder
            .path()
            .join(format!("skills/{}/SKILL.md", member.slug)),
        "Owner external edit",
    )
    .unwrap();
    assert_eq!(
        f.bundles
            .remove(&f.agent, &view.summary.id, removal(&view))
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    assert_eq!(
        f.bundles
            .set_enabled(&f.agent, &view.summary.id, change(&view, false))
            .await
            .unwrap_err(),
        BundleError::Conflict
    );
    assert!(f.state.read().await.community_bundles[0]
        .settlement
        .is_none());
    assert!(!f.folder.path().join(".anima-trash").exists());
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_restore_is_disconnected_and_interrupted_cleanup_is_unavailable_without_replay() {
    let f = Fixture::new().await;
    let installed = f
        .install(
            &f.review("Persist only metadata.", "http://127.0.0.1:9432/mcp", None)
                .await,
        )
        .await;
    f.mcp
        .update(
            &f.agent,
            &connection(&installed).connection_id,
            UpdateMcpConnection {
                expected_version: connection(&installed).connection_version,
                label: None,
                endpoint: None,
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    let snapshot = f.saved().await;
    assert_eq!(
        snapshot.version,
        crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
    );
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(snapshot.clone())
        .unwrap();
    assert!(restored.mcp_publication.read().unwrap().live.is_empty());
    assert!(restored.community_reviews.is_empty());
    assert!(restored.skills.records().is_empty());
    let mut interrupted = snapshot;
    interrupted.community_bundles[0].state = "removing".into();
    interrupted.community_bundles[0].enabled = false;
    interrupted.community_bundles[0].settlement = Some(Settlement {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "remove".into(),
        phase: "cleanup_pending".into(),
        trash: BTreeMap::new(),
    });
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(interrupted)
        .unwrap();
    assert_eq!(restored.community_bundles[0].state, "removing");
    assert!(!restored.community_bundles[0].available());
    assert!(!f.folder.path().join("skills").exists());
    f.no_launch();
    f.mcp.shutdown().await;
}

#[tokio::test]
async fn m45_version_eighteen_backup_is_independent_and_exact() {
    use crate::control_plane_store::{
        pre_upgrade_backup_path, write_pre_upgrade_backup, PRE_COMMUNITY_BUNDLES_BACKUP_SUFFIX,
    };
    let folder = TempDir::new().unwrap();
    let path = folder.path().join("control.json");
    let original="{\"version\":18,\"agents\":[],\"swarms\":[],\"mcpConnections\":[],\"skills\":[],\"skillDrafts\":[]}";
    std::fs::write(&path, original).unwrap();
    let older = pre_upgrade_backup_path(&path, 17);
    std::fs::write(&older, "Preserved M44 backup").unwrap();
    write_pre_upgrade_backup(&ControlPlaneStoreConfig::Json(path.clone()), 18)
        .await
        .unwrap();
    let backup = pre_upgrade_backup_path(&path, 18);
    assert!(backup
        .to_string_lossy()
        .ends_with(PRE_COMMUNITY_BUNDLES_BACKUP_SUFFIX));
    assert_ne!(backup, older);
    assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
    assert_eq!(
        std::fs::read_to_string(older).unwrap(),
        "Preserved M44 backup"
    );
}
