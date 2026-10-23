use super::*;

#[test]
fn email_sessions_retain_external_origin_and_use_drafts_for_owner_sending() {
    let (kind, origin) =
        crate::sessions::kind_for_room("email:connector:sender", Some(RunSource::Email), false);
    assert_eq!(kind, crate::sessions::SessionKind::Email);
    assert_eq!(origin, crate::sessions::SessionOrigin::Email);
    assert!(!kind.capabilities(false).send);
}
use anima_service_adapters::email::{
    EmailAccount, EmailError, EmailOutgoing, EmailSecrets, EmailTransport,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct ReplyModel {
    calls: AtomicUsize,
}
#[async_trait]
impl anima_core::ModelAdapter for ReplyModel {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
        true
    }
    fn provider(&self) -> &str {
        "email-test"
    }
    async fn generate(
        &self,
        _: &anima_core::AgentConfig,
        request: &anima_core::ModelGenerateRequest,
    ) -> Result<anima_core::ModelGenerateResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(request
            .messages
            .iter()
            .any(|m| m.content.text.contains("External email")));
        Ok(anima_core::ModelGenerateResponse {
            content: Content {
                text: "A useful email reply".into(),
                ..Default::default()
            },
            tool_calls: None,
            usage: anima_core::TokenUsage::default(),
            stop_reason: anima_core::ModelStopReason::End,
        })
    }
}

struct FakeEmail {
    inbox: StdMutex<EmailInbox>,
    sent: StdMutex<Vec<EmailOutgoing>>,
    unknown: AtomicBool,
}
#[async_trait]
impl EmailTransport for FakeEmail {
    async fn verify(&self, _: &EmailAccount, _: &EmailSecrets) -> Result<EmailInbox, EmailError> {
        Ok(EmailInbox {
            uid_validity: 7,
            uid_next: 10,
            messages: vec![],
        })
    }
    async fn inbox(
        &self,
        _: &EmailAccount,
        _: &EmailSecrets,
        after: Option<EmailCursor>,
    ) -> Result<EmailInbox, EmailError> {
        let mut inbox = self.inbox.lock().unwrap().clone();
        if after.is_some_and(|cursor| cursor.uid_validity != inbox.uid_validity) {
            return Err(EmailError::CursorChanged);
        }
        if let Some(cursor) = after {
            inbox.messages.retain(|m| m.uid > cursor.last_uid);
        }
        Ok(inbox)
    }
    async fn send(
        &self,
        _: &EmailAccount,
        _: &EmailSecrets,
        message: &EmailOutgoing,
    ) -> Result<(), EmailError> {
        self.sent.lock().unwrap().push(message.clone());
        if self.unknown.load(Ordering::SeqCst) {
            Err(EmailError::Unknown)
        } else {
            Ok(())
        }
    }
}
struct NoEnvironment;
impl crate::connectors::oauth_apps::OAuthEnvironment for NoEnvironment {
    fn get(&self, _: &str) -> Option<String> {
        None
    }
}
fn setup() -> super::super::generic::GenericEmailSetup {
    serde_json::from_value(serde_json::json!({"address":"agent@example.com","imap":{"host":"imap.example.com","port":993,"security":"tls","username":"agent@example.com"},"smtp":{"host":"smtp.example.com","port":587,"security":"starttls","username":"agent@example.com"},"imapPassword":"imap-secret","smtpPassword":"smtp-secret","autoReply":{"enabled":true,"allowedSenders":["alice@example.com"],"authservId":"mx.example.com"}})).unwrap()
}
fn message(uid: u32) -> EmailMessage {
    EmailMessage {
        uid,
        from: "alice@example.com".into(),
        subject: "A question".into(),
        body: "Please explain the project".into(),
        received_at: "2026-10-20T00:00:00Z".into(),
        message_id: Some(format!("<message-{uid}@example.com>")),
        automated: false,
        authentication_results: vec!["mx.example.com; dmarc=pass header.from=example.com".into()],
    }
}
async fn fixture() -> (
    SharedDaemonState,
    MailManager,
    Arc<FakeEmail>,
    Arc<ReplyModel>,
    String,
    String,
) {
    let model = Arc::new(ReplyModel::default());
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(model.clone())));
    let agent = state
        .write()
        .await
        .create_agent(anima_core::AgentConfig {
            name: "email-agent".into(),
            model: "email-test".into(),
            provider: Some("email-test".into()),
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
    let email = Arc::new(FakeEmail {
        inbox: StdMutex::new(EmailInbox {
            uid_validity: 7,
            uid_next: 11,
            messages: vec![message(10)],
        }),
        sent: StdMutex::new(vec![]),
        unknown: AtomicBool::new(false),
    });
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(tokio::sync::Semaphore::new(2)));
    let manager = MailManager::new(
        &state,
        runs,
        Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        Arc::new(client::MailClient::new()),
        crate::connectors::oauth_apps::OAuthAppService::with_backends(
            Arc::new(crate::connectors::oauth_apps::InMemoryOAuthAppVault::default()),
            Arc::new(NoEnvironment),
        ),
    )
    .with_email_backends(
        email.clone(),
        Arc::new(super::super::credentials::InMemoryEmailCredentialStore::default()),
    );
    let id = manager.connect_email(&agent, setup()).await.unwrap().id;
    (state, manager, email, model, agent, id)
}

#[test]
fn baseline_covers_all_historical_uids_even_when_inbox_is_empty() {
    let baseline = EmailMailboxState::baseline(&EmailInbox {
        uid_validity: 7,
        uid_next: 101,
        messages: vec![],
    });
    assert_eq!(
        baseline.cursor,
        EmailCursor {
            uid_validity: 7,
            last_uid: 100
        }
    );
    assert!(baseline.receipts.is_empty());
    assert!(baseline.valid());
}

#[test]
fn snapshot_rejects_receipts_outside_its_uid_watermark() {
    let mut state = EmailMailboxState::baseline(&EmailInbox {
        uid_validity: 7,
        uid_next: 2,
        messages: vec![],
    });
    state.receipts.insert(
        2,
        EmailReceipt {
            uid: 2,
            sender: "sender@example.com".into(),
            message_id: None,
            generation: "generation".into(),
            state: EmailReceiptState::Claimed,
            draft_id: None,
        },
    );
    assert!(!state.valid());
}

#[tokio::test]
async fn automatic_mail_runs_and_sends_once_to_the_admitted_sender_with_thread_headers() {
    let (state, manager, email, model, agent, id) = fixture().await;
    manager.poll_email_mailbox(&agent, &id).await.unwrap();
    manager.poll_email_mailbox(&agent, &id).await.unwrap();
    let sent = email.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, vec!["alice@example.com"]);
    assert_eq!(sent[0].body, "A useful email reply");
    assert_eq!(
        sent[0].in_reply_to.as_deref(),
        Some("<message-10@example.com>")
    );
    assert!(sent[0].automatic);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let state = state.read().await;
    assert_eq!(state.runs.status_counts().get("completed"), Some(&1));
    assert_eq!(
        state.mail_records[&id].messages.len(),
        1,
        "an empty incremental poll preserves the inbox cache"
    );
    assert_eq!(
        state.mail_records[&id]
            .email
            .as_ref()
            .unwrap()
            .cursor
            .last_uid,
        10
    );
    assert_eq!(
        state.mail_drafts.values().next().unwrap().state,
        DraftState::Sent
    );
}

#[tokio::test]
async fn historical_automated_self_and_unauthenticated_mail_never_runs() {
    let (_state, manager, email, model, agent, id) = fixture().await;
    let mut old = message(9);
    old.body = "Historical".into();
    let mut auto = message(10);
    auto.automated = true;
    let mut own = message(11);
    own.from = "agent@example.com".into();
    let mut forged = message(12);
    forged.authentication_results =
        vec!["attacker.example; dmarc=pass header.from=example.com".into()];
    *email.inbox.lock().unwrap() = EmailInbox {
        uid_validity: 7,
        uid_next: 13,
        messages: vec![old, auto, own, forged],
    };
    manager.poll_email_mailbox(&agent, &id).await.unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    assert!(email.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_send_pauses_automatic_email_and_cannot_repeat_the_send() {
    let (state, manager, email, model, agent, id) = fixture().await;
    email.unknown.store(true, Ordering::SeqCst);
    assert!(manager.poll_email_mailbox(&agent, &id).await.is_err());
    manager.poll_email_mailbox(&agent, &id).await.unwrap();
    assert_eq!(email.sent.lock().unwrap().len(), 1);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let state = state.read().await;
    assert!(
        !state.mail_records[&id]
            .connector
            .email
            .as_ref()
            .unwrap()
            .auto_reply
            .enabled
    );
    assert_eq!(
        state.mail_drafts.values().next().unwrap().state,
        DraftState::Unknown
    );
}

#[tokio::test]
async fn interrupted_claim_and_changed_uidvalidity_require_review_without_running() {
    let (state, manager, email, model, agent, id) = fixture().await;
    {
        let mut state = state.write().await;
        let record = state.mail_records.get_mut(&id).unwrap();
        let generation = record.auth_generation.clone();
        let mailbox = record.email.as_mut().unwrap();
        mailbox.cursor.last_uid = 10;
        mailbox.receipts.insert(
            10,
            EmailReceipt {
                uid: 10,
                sender: "alice@example.com".into(),
                message_id: None,
                generation,
                state: EmailReceiptState::Claimed,
                draft_id: None,
            },
        );
    }
    manager.poll_email_mailbox(&agent, &id).await.unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    assert!(state.read().await.mail_records[&id]
        .connector
        .error
        .as_ref()
        .unwrap()
        .contains("owner review"));
    manager
        .messages(&agent, &id, Provider::Email, true)
        .await
        .unwrap();
    manager.poll_email_mailbox(&agent, &id).await.unwrap();
    assert!(state.read().await.mail_records[&id]
        .connector
        .error
        .as_ref()
        .unwrap()
        .contains("owner review"));
    manager.connect_email(&agent, setup()).await.unwrap();
    email.inbox.lock().unwrap().uid_validity = 8;
    manager.poll_email_mailbox(&agent, &id).await.unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    assert!(email.sent.lock().unwrap().is_empty());
    assert!(
        !state.read().await.mail_records[&id]
            .connector
            .email
            .as_ref()
            .unwrap()
            .auto_reply
            .enabled
    );
}

#[tokio::test]
async fn stale_failure_cannot_disable_a_new_connection_and_shutdown_releases_ownership() {
    let (state, manager, _email, _model, agent, id) = fixture().await;
    let old = state.read().await.mail_records[&id].auth_generation.clone();
    manager.connect_email(&agent, setup()).await.unwrap();
    manager
        .pause_email(&agent, &id, &old, "stale failure")
        .await
        .unwrap();
    assert!(
        state.read().await.mail_records[&id]
            .connector
            .email
            .as_ref()
            .unwrap()
            .auto_reply
            .enabled
    );
    let weak = manager.weak();
    state.write().await.mail_manager = Some(weak.clone());
    manager.start();
    manager.shutdown().await;
    drop(manager);
    assert!(
        weak.upgrade().is_none(),
        "the state only retains a weak mail manager"
    );
}
