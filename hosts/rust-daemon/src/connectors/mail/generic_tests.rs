use super::credentials::EmailCredentialStore;
use super::*;
use anima_service_adapters::email::{
    EmailAccount, EmailError, EmailInbox, EmailOutgoing, EmailSecrets, EmailTransport,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct UncertainWriteStore {
    inner: credentials::InMemoryEmailCredentialStore,
    uncertain: std::sync::atomic::AtomicBool,
    fail_all: std::sync::atomic::AtomicBool,
    last_id: std::sync::Mutex<Option<String>>,
}
#[async_trait]
impl EmailCredentialStore for UncertainWriteStore {
    async fn load(
        &self,
        id: &str,
    ) -> Result<Option<EmailSecrets>, super::super::credentials::CredentialStoreError> {
        self.inner.load(id).await
    }
    async fn put(
        &self,
        id: &str,
        secrets: EmailSecrets,
    ) -> Result<(), super::super::credentials::CredentialStoreError> {
        *self.last_id.lock().unwrap() = Some(id.to_owned());
        self.inner.put(id, secrets).await?;
        if self.uncertain.swap(false, Ordering::SeqCst) || self.fail_all.load(Ordering::SeqCst) {
            Err(super::super::credentials::CredentialStoreError::CredentialStateUncertain)
        } else {
            Ok(())
        }
    }
    async fn delete(
        &self,
        id: &str,
    ) -> Result<(), super::super::credentials::CredentialStoreError> {
        self.inner.delete(id).await
    }
}

#[derive(Default)]
pub(crate) struct EmailFake {
    pub(crate) verifies: AtomicUsize,
    pub(crate) sends: AtomicUsize,
    unknown: bool,
}
#[async_trait]
impl EmailTransport for EmailFake {
    async fn verify(
        &self,
        _: &EmailAccount,
        secrets: &EmailSecrets,
    ) -> Result<EmailInbox, EmailError> {
        assert_eq!(secrets.imap_password(), "imap-secret-sentinel");
        self.verifies.fetch_add(1, Ordering::SeqCst);
        Ok(EmailInbox {
            uid_validity: 7,
            uid_next: 42,
            messages: vec![],
        })
    }
    async fn inbox(
        &self,
        _: &EmailAccount,
        _: &EmailSecrets,
        _: Option<anima_service_adapters::email::EmailCursor>,
    ) -> Result<EmailInbox, EmailError> {
        Ok(EmailInbox {
            uid_validity: 7,
            uid_next: 42,
            messages: vec![],
        })
    }
    async fn send(
        &self,
        _: &EmailAccount,
        _: &EmailSecrets,
        _: &EmailOutgoing,
    ) -> Result<(), EmailError> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        if self.unknown {
            Err(EmailError::Unknown)
        } else {
            Ok(())
        }
    }
}
pub(crate) fn setup_json(enabled: bool) -> serde_json::Value {
    serde_json::json!({
        "address":"agent@example.com", "imap":{"host":"imap.example.com","port":993,"security":"tls","username":"agent@example.com"},
        "smtp":{"host":"smtp.example.com","port":587,"security":"starttls","username":"agent@example.com"},
        "imapPassword":"imap-secret-sentinel","smtpPassword":"smtp-secret-sentinel",
        "autoReply":{"enabled":enabled,"allowedSenders":["owner@example.com"],"authservId":"mx.example.com"}
    })
}
fn setup(enabled: bool) -> generic::GenericEmailSetup {
    serde_json::from_value(setup_json(enabled)).unwrap()
}
pub(crate) async fn fixture(
    unknown: bool,
) -> (
    SharedDaemonState,
    MailManager,
    Arc<EmailFake>,
    Arc<credentials::InMemoryEmailCredentialStore>,
    String,
) {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let agent = state
        .write()
        .await
        .create_agent(anima_core::AgentConfig {
            name: "generic-mail".into(),
            model: "deterministic".into(),
            provider: None,
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            system: None,
            tools: None,
            plugins: None,
            settings: None,
            fallbacks: vec![],
        })
        .unwrap()
        .state
        .id;
    let transport = Arc::new(EmailFake {
        unknown,
        ..Default::default()
    });
    let credentials = Arc::new(credentials::InMemoryEmailCredentialStore::default());
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(tokio::sync::Semaphore::new(2)));
    let manager = MailManager::new(
        &state,
        runs,
        Arc::new(super::super::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        Arc::new(client::MailClient::new()),
        crate::connectors::oauth_apps::OAuthAppService::with_backends(
            Arc::new(crate::connectors::oauth_apps::InMemoryOAuthAppVault::default()),
            Arc::new(EmptyEnvironment),
        ),
    )
    .with_email_backends(transport.clone(), credentials.clone());
    (state, manager, transport, credentials, agent)
}
struct EmptyEnvironment;
impl crate::connectors::oauth_apps::OAuthEnvironment for EmptyEnvironment {
    fn get(&self, _: &str) -> Option<String> {
        None
    }
}
fn draft_input() -> DraftInput {
    DraftInput {
        to: vec!["owner@example.com".into()],
        subject: "hello".into(),
        body: "plain text".into(),
    }
}

#[tokio::test]
async fn generic_setup_verifies_without_oauth_or_sending_and_persists_no_password() {
    let (state, manager, fake, credentials, agent) = fixture(false).await;
    assert!(manager.configured(Provider::Email).await.unwrap());
    let connector = manager.connect_email(&agent, setup(true)).await.unwrap();
    assert_eq!(fake.verifies.load(Ordering::SeqCst), 1);
    assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
    assert!(credentials.load(&connector.id).await.unwrap().is_some());
    let state = state.read().await;
    let record = &state.mail_records[&connector.id];
    assert_eq!(record.email.as_ref().unwrap().cursor.last_uid, 41);
    let json = serde_json::to_string(&state.control_plane_snapshot()).unwrap();
    assert!(!json.contains("secret-sentinel"));
    assert!(!json.contains("Password"));
}

#[tokio::test]
async fn generic_unknown_send_is_not_retried_or_silently_reconnected() {
    let (_state, manager, fake, _credentials, agent) = fixture(true).await;
    let connector = manager.connect_email(&agent, setup(false)).await.unwrap();
    let draft = manager
        .create_draft(&agent, &connector.id, Provider::Email, draft_input())
        .await
        .unwrap();
    let resolved = manager
        .resolve(&agent, &connector.id, Provider::Email, &draft.id, true)
        .await
        .unwrap();
    assert_eq!(resolved.state, DraftState::Unknown);
    assert_eq!(
        manager
            .resolve(&agent, &connector.id, Provider::Email, &draft.id, true)
            .await
            .unwrap_err(),
        MailError::Conflict
    );
    assert_eq!(
        manager
            .connect_email(&agent, setup(false))
            .await
            .unwrap_err(),
        MailError::Conflict
    );
    assert_eq!(fake.sends.load(Ordering::SeqCst), 1);
    manager
        .disconnect(&agent, &connector.id, Provider::Email)
        .await
        .unwrap();
    let replacement = manager.connect_email(&agent, setup(false)).await.unwrap();
    assert_ne!(connector.id, replacement.id);
}

#[tokio::test]
async fn generic_reconnect_rejects_old_pending_drafts_and_disconnect_cleans_vault() {
    let (_state, manager, _fake, credentials, agent) = fixture(false).await;
    let connector = manager.connect_email(&agent, setup(false)).await.unwrap();
    let draft = manager
        .create_draft(&agent, &connector.id, Provider::Email, draft_input())
        .await
        .unwrap();
    manager.connect_email(&agent, setup(false)).await.unwrap();
    assert_eq!(
        manager
            .drafts(&agent, &connector.id, Provider::Email)
            .await
            .unwrap()
            .iter()
            .find(|d| d.id == draft.id)
            .unwrap()
            .state,
        DraftState::Rejected
    );
    manager
        .disconnect(&agent, &connector.id, Provider::Email)
        .await
        .unwrap();
    assert!(credentials.load(&connector.id).await.unwrap().is_none());
}

#[test]
fn generic_automatic_settings_require_a_pinned_auth_server_and_sender_allowlist() {
    let mut input = setup(true);
    input.auto_reply.allowed_senders.clear();
    assert!(input.validate().is_err());
    let mut input = setup(true);
    input.auto_reply.authserv_id.clear();
    assert!(input.validate().is_err());
    let mut input = setup(false);
    input.imap_password = "bad\r\nsecret".into();
    assert!(input.validate().is_err());
}

#[tokio::test]
async fn uncertain_generic_vault_replacement_restores_previous_passwords() {
    let (_state, manager, fake, _credentials, agent) = fixture(false).await;
    let store = Arc::new(UncertainWriteStore::default());
    let manager = manager.with_email_backends(fake, store.clone());
    let connector = manager.connect_email(&agent, setup(false)).await.unwrap();
    store.uncertain.store(true, Ordering::SeqCst);
    let mut replacement = setup(false);
    replacement.smtp_password = "replacement-secret".into();
    assert_eq!(
        manager
            .connect_email(&agent, replacement)
            .await
            .unwrap_err(),
        MailError::Credential
    );
    assert_eq!(
        store
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .smtp_password(),
        "smtp-secret-sentinel"
    );
}

#[tokio::test]
async fn uncertain_generic_vault_compensation_disables_existing_connector() {
    let (_state, manager, fake, _credentials, agent) = fixture(false).await;
    let store = Arc::new(UncertainWriteStore::default());
    let manager = manager.with_email_backends(fake, store.clone());
    manager.connect_email(&agent, setup(false)).await.unwrap();
    store.fail_all.store(true, Ordering::SeqCst);
    assert_eq!(
        manager
            .connect_email(&agent, setup(false))
            .await
            .unwrap_err(),
        MailError::Credential
    );
    assert_eq!(
        manager
            .connector_for_agent(&agent, Provider::Email)
            .await
            .unwrap()
            .unwrap()
            .status,
        "reauthRequired"
    );
}

#[tokio::test]
async fn generic_setup_persistence_failure_removes_new_credentials() {
    let (state, manager, fake, _credentials, agent) = fixture(false).await;
    let mut unrelated_config = state.read().await.get_agent(&agent).unwrap().state.config;
    unrelated_config.name = "unrelated-agent".into();
    let unrelated = state
        .write()
        .await
        .create_agent(unrelated_config)
        .unwrap()
        .state
        .id;
    let credentials = Arc::new(UncertainWriteStore::default());
    let manager = manager.with_email_backends(fake.clone(), credentials.clone());
    let directory = tempfile::tempdir().unwrap();
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(directory.path().to_path_buf()),
    ));
    assert_eq!(
        manager
            .connect_email(&agent, setup(false))
            .await
            .unwrap_err(),
        MailError::Persistence
    );
    assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
    assert!(state.read().await.mail_records.is_empty());
    assert!(
        state
            .read()
            .await
            .get_agent(&unrelated)
            .unwrap()
            .state
            .config
            .tools
            .is_none(),
        "setup rollback must preserve unrelated agent configuration"
    );
    assert!(
        state
            .read()
            .await
            .get_agent(&agent)
            .unwrap()
            .state
            .config
            .tools
            .unwrap_or_default()
            .is_empty(),
        "a failed first connection must also roll back newly enabled mail tools"
    );
    let id = credentials.last_id.lock().unwrap().clone().unwrap();
    assert!(credentials.load(&id).await.unwrap().is_none());
}

#[tokio::test]
async fn mail_worker_shutdown_releases_coordinator_state() {
    let (state, manager, _fake, _credentials, _agent) = fixture(false).await;
    let weak = Arc::downgrade(&state);
    manager.start();
    manager.shutdown().await;
    drop(manager);
    drop(state);
    assert!(weak.upgrade().is_none());
}
