use super::*;
use crate::connectors::{OutboundDeliveryState, TelegramOutboundRecord};
use crate::sessions::test_support::within;

struct BlockedDelivery {
    inner: FakeTransport,
    entered: Semaphore,
    release: Semaphore,
}
impl Default for BlockedDelivery {
    fn default() -> Self {
        Self {
            inner: FakeTransport::default(),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }
}
#[async_trait]
impl TelegramTransport for BlockedDelivery {
    async fn get_me(
        &self,
        token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        self.inner.get_me(token).await
    }
    async fn get_updates(
        &self,
        _: &TelegramBotToken,
        _: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        std::future::pending().await
    }
    async fn send_message(
        &self,
        token: &TelegramBotToken,
        chat: &str,
        text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        self.inner.send_message(token, chat, text).await
    }
}
async fn setup() -> (ConnectorManager, Arc<BlockedDelivery>, String) {
    let state = state_with_agent();
    let agent = state.read().await.list_agents()[0].state.id.clone();
    let transport = Arc::new(BlockedDelivery::default());
    let manager = manager(
        state.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let connector = manager
        .create(
            agent.clone(),
            TelegramBotToken::parse("42:delivery-claim").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    let mut state = state.write().await;
    state
        .connectors
        .get_mut(&connector.id)
        .unwrap()
        .approved_chat = Some(TelegramChatMetadata {
        id: "202".into(),
        kind: TelegramChatKind::Private,
        title: None,
        username: None,
    });
    for (id, text, time) in [("first", "first reply", 1), ("second", "second reply", 2)] {
        state.outbound.insert(
            id.into(),
            TelegramOutboundRecord {
                id: id.into(),
                connector_id: connector.id.clone(),
                agent_id: agent.clone(),
                room_id: connector.room_id.clone(),
                assistant_message_id: id.into(),
                text: text.into(),
                created_at_ms: time,
                delivered_at_ms: None,
                attempts: 0,
                delivery_state: OutboundDeliveryState::Pending,
                message_pruned: false,
            },
        );
    }
    drop(state);
    (manager, transport, connector.id)
}
fn send(
    manager: &ConnectorManager,
    id: &str,
) -> tokio::task::JoinHandle<Result<bool, super::super::ConnectorManagerError>> {
    let manager = manager.clone();
    let id = id.to_owned();
    tokio::spawn(async move { manager.deliver_pending_once(id).await })
}
#[tokio::test]
async fn blocked_delivery_releases_transaction_and_duplicate_claim_preserves_order() {
    let (manager, transport, id) = setup().await;
    let first = send(&manager, &id);
    within("send entered", transport.entered.acquire())
        .await
        .unwrap()
        .forget();
    let transaction = within(
        "unrelated transaction",
        manager.runs.control_plane_transaction(),
    )
    .await;
    let persist = {
        let mut state = manager.state.write().await;
        state.outbound.get_mut("second").unwrap().text = "second updated".into();
        state.control_plane_persist_request()
    };
    within("unrelated persistence", persist.save())
        .await
        .unwrap();
    drop(transaction);
    assert!(
        !within("duplicate claim", manager.deliver_pending_once(id.clone()))
            .await
            .unwrap()
    );
    transport.release.add_permits(1);
    assert!(within("first settles", first).await.unwrap().unwrap());
    let second = send(&manager, &id);
    within("second entered", transport.entered.acquire())
        .await
        .unwrap()
        .forget();
    transport.release.add_permits(1);
    assert!(within("second settles", second).await.unwrap().unwrap());
    assert_eq!(
        *transport.inner.sent.lock().unwrap(),
        vec![
            ("202".into(), "first reply".into()),
            ("202".into(), "second updated".into())
        ]
    );
    manager.shutdown().await;
}
#[tokio::test]
async fn deletion_during_delivery_does_not_resurrect_outbound_or_status() {
    let (manager, transport, id) = setup().await;
    let delivering = send(&manager, &id);
    within("send entered", transport.entered.acquire())
        .await
        .unwrap()
        .forget();
    within("delete while send blocked", manager.delete(id.clone()))
        .await
        .unwrap();
    transport.release.add_permits(1);
    assert!(!within("stale settlement", delivering)
        .await
        .unwrap()
        .unwrap());
    assert!(manager.state.read().await.outbound.is_empty());
    assert_eq!(manager.status(&id).await, None);
    manager.shutdown().await;
}
#[tokio::test]
async fn shutdown_signal_keeps_owned_delivery_settlement_alive() {
    let (manager, transport, id) = setup().await;
    let delivering = send(&manager, &id);
    within("send entered", transport.entered.acquire())
        .await
        .unwrap()
        .forget();
    within("signal producers", manager.request_shutdown()).await;
    let mut shutdown = Box::pin(manager.shutdown());
    std::future::poll_fn(|cx| {
        assert!(
            std::future::Future::poll(shutdown.as_mut(), cx).is_pending(),
            "shutdown must drain the owned claim"
        );
        std::task::Poll::Ready(())
    })
    .await;
    transport.release.add_permits(1);
    assert!(within("owned settlement", delivering)
        .await
        .unwrap()
        .unwrap());
    assert_eq!(
        manager.state.read().await.outbound["first"].delivery_state,
        OutboundDeliveryState::Delivered
    );
    within("shutdown drain", shutdown).await;
}
#[tokio::test]
async fn replacement_generation_rejects_stale_delivery_settlement() {
    let (manager, transport, id) = setup().await;
    let delivering = send(&manager, &id);
    within("send entered", transport.entered.acquire())
        .await
        .unwrap()
        .forget();
    within(
        "replace while send blocked",
        manager.replace_token(
            id.clone(),
            TelegramBotToken::parse("42:replacement").unwrap(),
        ),
    )
    .await
    .unwrap();
    manager.stop_worker(&id).await.unwrap();
    transport.release.add_permits(1);
    assert!(!within("stale settlement", delivering)
        .await
        .unwrap()
        .unwrap());
    let state = manager.state.read().await;
    assert_eq!(state.outbound["first"].attempts, 0);
    assert_eq!(state.connectors[&id].bot.id, "42");
    drop(state);
    manager.shutdown().await;
}

#[tokio::test]
async fn delivery_claim_survives_lifecycle_save_rollback_without_overwriting_restoration() {
    for replacing in [false, true] {
        let (manager, transport, id) = setup().await;
        let before = manager.state.read().await.outbound.clone();
        let delivering = send(&manager, &id);
        within("send entered", transport.entered.acquire())
            .await
            .unwrap()
            .forget();
        let gate = manager
            .state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        let updating = {
            let manager = manager.clone();
            let id = id.clone();
            tokio::spawn(async move {
                if replacing {
                    manager
                        .replace_token(id, TelegramBotToken::parse("42:tentative").unwrap())
                        .await
                        .map(|_| ())
                } else {
                    manager.delete(id).await
                }
            })
        };
        within("lifecycle save", gate.entered.acquire())
            .await
            .unwrap()
            .forget();
        gate.release.add_permits(1);
        assert_eq!(
            within("lifecycle rollback", updating)
                .await
                .unwrap()
                .unwrap_err(),
            super::super::ConnectorManagerError::Persistence
        );
        manager.stop_worker(&id).await.unwrap();
        transport.release.add_permits(1);
        assert!(!within("fenced old settlement", delivering)
            .await
            .unwrap()
            .unwrap());
        assert_eq!(manager.state.read().await.outbound, before);
        assert!(manager.state.read().await.connectors[&id].is_active());
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn delivery_failure_retries_first_row_before_later_outbound() {
    let (manager, transport, id) = setup().await;
    transport
        .inner
        .remaining_send_failures
        .store(1, Ordering::SeqCst);
    for succeeds in [false, true, true] {
        let delivering = send(&manager, &id);
        within("ordered send", transport.entered.acquire())
            .await
            .unwrap()
            .forget();
        transport.release.add_permits(1);
        let result = within("ordered settlement", delivering).await.unwrap();
        if succeeds {
            assert!(result.unwrap());
        } else {
            assert_eq!(
                result.unwrap_err(),
                super::super::ConnectorManagerError::Transport
            );
        }
    }
    assert_eq!(
        *transport.inner.sent.lock().unwrap(),
        vec![
            ("202".into(), "first reply".into()),
            ("202".into(), "second reply".into())
        ]
    );
    assert_eq!(manager.state.read().await.outbound["first"].attempts, 2);
    manager.shutdown().await;
}

#[tokio::test]
async fn restored_manual_delivery_initializes_only_from_lifecycle_loaded_credential() {
    let (original, transport, id) = setup().await;
    original.shutdown().await;
    let restored = super::manager(
        original.state.clone(),
        original.credentials.clone(),
        transport.clone(),
    );
    let delivering = send(&restored, &id);
    within("restored manual send", transport.entered.acquire())
        .await
        .unwrap()
        .forget();
    transport.release.add_permits(1);
    assert!(within("restored settlement", delivering)
        .await
        .unwrap()
        .unwrap());
    restored.shutdown().await;
}
