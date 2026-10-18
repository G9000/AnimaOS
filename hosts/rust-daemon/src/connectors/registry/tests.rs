use super::*;
use crate::connectors::telegram::{
    TelegramSentMessage, TelegramTextUpdate, TelegramTransportError, TelegramUpdateBatch,
};
use crate::connectors::{
    TelegramBotIdentity, TelegramChatKind, TelegramChatMetadata, TelegramSenderMetadata,
};
use anima_service_adapters::connectors::{
    ConnectorErrorKind, ConnectorFailure, ConnectorReceiveRequest, ConnectorSendRequest,
    ConnectorTransport as AsyncConnectorTransport,
};
use async_trait::async_trait;
use std::sync::Mutex;

#[derive(Default)]
struct FixtureTransport {
    calls: Mutex<Vec<(String, String)>>,
    error: Mutex<Option<TelegramTransportError>>,
}

fn chat() -> TelegramChatMetadata {
    TelegramChatMetadata {
        id: "-42".into(),
        kind: TelegramChatKind::Group,
        title: Some("Fixture".into()),
        username: None,
    }
}

fn batch() -> TelegramUpdateBatch {
    TelegramUpdateBatch {
        next_update_id: 73,
        updates: vec![TelegramTextUpdate {
            update_id: 72,
            text: "  fixture\n".into(),
            sender: TelegramSenderMetadata {
                id: "5".into(),
                username: None,
                display_name: Some("Sender".into()),
            },
            chat: chat(),
        }],
    }
}

fn receipts() -> Vec<TelegramSentMessage> {
    vec![
        TelegramSentMessage {
            message_id: "81".into(),
            chat: chat(),
        },
        TelegramSentMessage {
            message_id: "82".into(),
            chat: chat(),
        },
    ]
}

#[async_trait]
impl TelegramTransport for FixtureTransport {
    async fn get_me(
        &self,
        token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        self.calls
            .lock()
            .unwrap()
            .push((token.expose().into(), "identify".into()));
        Ok(TelegramBotIdentity {
            id: "7".into(),
            username: Some("fixture_bot".into()),
            display_name: None,
        })
    }
    async fn get_updates(
        &self,
        token: &TelegramBotToken,
        offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        self.calls
            .lock()
            .unwrap()
            .push((token.expose().into(), format!("receive:{offset}")));
        Ok(batch())
    }
    async fn send_message(
        &self,
        token: &TelegramBotToken,
        destination: &str,
        text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        self.calls
            .lock()
            .unwrap()
            .push((token.expose().into(), format!("send:{destination}:{text}")));
        match *self.error.lock().unwrap() {
            Some(error) => Err(error),
            None => Ok(receipts()),
        }
    }
}

fn request() -> ConnectorSendRequest {
    ConnectorSendRequest {
        destination: "-42".into(),
        text: "  fixture\n".into(),
        attempt_id: "outbound-17".into(),
    }
}

#[derive(Default)]
struct DiscordFixture {
    calls: Mutex<Vec<(String, String)>>,
}
use crate::connectors::discord::{
    client::{DiscordTransport, DiscordTransportError},
    credentials::DiscordBotToken,
    model::{DiscordBotIdentity, DiscordChannelMetadata, DiscordMessageBatch, DiscordSendReceipt},
};
#[async_trait]
impl DiscordTransport for DiscordFixture {
    async fn get_me(
        &self,
        token: &DiscordBotToken,
    ) -> Result<DiscordBotIdentity, DiscordTransportError> {
        self.calls
            .lock()
            .unwrap()
            .push((token.expose().into(), "identify".into()));
        Ok(DiscordBotIdentity {
            id: "7".into(),
            username: None,
            display_name: None,
        })
    }
    async fn get_channel(
        &self,
        _: &DiscordBotToken,
        _: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        panic!("metadata discovery cannot call native preflight")
    }
    async fn latest_cursor(
        &self,
        _: &DiscordBotToken,
        _: &str,
    ) -> Result<String, DiscordTransportError> {
        panic!("metadata discovery cannot poll")
    }
    async fn receive(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        cursor: &str,
    ) -> Result<DiscordMessageBatch, DiscordTransportError> {
        self.calls
            .lock()
            .unwrap()
            .push((token.expose().into(), format!("receive:{channel}:{cursor}")));
        Ok(DiscordMessageBatch {
            messages: Vec::new(),
            next_cursor: cursor.into(),
            boundary_proven: true,
        })
    }
    async fn send_chunk(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        text: &str,
        attempt: &str,
    ) -> Result<DiscordSendReceipt, DiscordTransportError> {
        self.calls.lock().unwrap().push((
            token.expose().into(),
            format!("send:{channel}:{text}:{attempt}"),
        ));
        Ok(DiscordSendReceipt {
            message_id: "88".into(),
            channel_id: channel.into(),
        })
    }
}
#[tokio::test]
async fn discord_registry_is_passive_and_bound_dispatch_preserves_captured_token_channel() {
    let native = Arc::new(DiscordFixture::default());
    let registry =
        ConnectorRegistry::with_discord(Arc::new(FixtureTransport::default()), native.clone());
    assert_eq!(
        registry
            .metadata
            .list()
            .map(|d| d.protocol_id.as_str())
            .collect::<Vec<_>>(),
        vec!["discord", "telegram"]
    );
    assert!(registry.discord_native().is_some());
    assert!(native.calls.lock().unwrap().is_empty());
    let old = DiscordBotToken::parse("captured-old").unwrap();
    let current = DiscordBotToken::parse("captured-current").unwrap();
    registry.discord(&old, "42").identify().await.unwrap();
    registry
        .discord(&old, "42")
        .receive(ConnectorReceiveRequest {
            cursor: "9007199254740993".into(),
        })
        .await
        .unwrap();
    registry
        .discord(&current, "42")
        .send(ConnectorSendRequest {
            destination: "42".into(),
            text: "fixture".into(),
            attempt_id: "outbound:0".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        *native.calls.lock().unwrap(),
        vec![
            ("captured-old".into(), "identify".into()),
            ("captured-old".into(), "receive:42:9007199254740993".into()),
            (
                "captured-current".into(),
                "send:42:fixture:outbound:0".into()
            )
        ]
    );
}
#[tokio::test]
async fn discord_adapter_rejects_arbitrary_destinations_and_oversized_native_chunks_before_effect()
{
    let native = Arc::new(DiscordFixture::default());
    let registry =
        ConnectorRegistry::with_discord(Arc::new(FixtureTransport::default()), native.clone());
    let token = DiscordBotToken::parse("fixture").unwrap();
    let adapter = registry.discord(&token, "42");
    for (destination, text) in [
        ("43", "valid".into()),
        ("42", "🦀".repeat(1001)),
        ("42", "x".repeat(2001)),
    ] {
        assert_eq!(
            adapter
                .send(ConnectorSendRequest {
                    destination: destination.into(),
                    text,
                    attempt_id: "attempt".into()
                })
                .await
                .unwrap_err(),
            DiscordTransportError::InvalidInput
        );
    }
    for cursor in ["01", "-1", "18446744073709551616"] {
        assert_eq!(
            adapter
                .receive(ConnectorReceiveRequest {
                    cursor: cursor.into()
                })
                .await
                .unwrap_err(),
            DiscordTransportError::InvalidInput
        );
    }
    assert!(native.calls.lock().unwrap().is_empty());
    assert_eq!(
        DiscordTransportError::Uncertain.kind(),
        ConnectorErrorKind::Uncertain
    );
}

#[tokio::test]
async fn registered_bridge_is_passive_and_preserves_credentials_payloads_and_cursor() {
    let native = Arc::new(FixtureTransport::default());
    let registry = ConnectorRegistry::new(native.clone());
    let old = TelegramBotToken::parse("7:old_fixture").unwrap();
    let current = TelegramBotToken::parse("7:current_fixture").unwrap();
    let adapter = registry.telegram(&old);
    let descriptors: Vec<_> = registry.metadata.list().collect();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].protocol_id, "telegram");
    let metadata = serde_json::to_string(&descriptors).unwrap();
    assert!(!metadata.contains("fixture"));
    assert!(native.calls.lock().unwrap().is_empty());
    assert_eq!(
        adapter.identify().await.unwrap().username.as_deref(),
        Some("fixture_bot")
    );
    assert_eq!(
        adapter
            .receive(ConnectorReceiveRequest {
                cursor: "70".into()
            })
            .await
            .unwrap(),
        batch()
    );
    assert_eq!(
        registry.telegram(&current).send(request()).await.unwrap(),
        receipts()
    );
    assert_eq!(
        *native.calls.lock().unwrap(),
        vec![
            ("7:old_fixture".into(), "identify".into()),
            ("7:old_fixture".into(), "receive:70".into()),
            ("7:current_fixture".into(), "send:-42:  fixture\n".into()),
        ]
    );
}

#[tokio::test]
async fn invalid_portable_requests_fail_before_native_effect() {
    let native = Arc::new(FixtureTransport::default());
    let registry = ConnectorRegistry::new(native.clone());
    let token = TelegramBotToken::parse("7:fixture").unwrap();
    let adapter = registry.telegram(&token);
    for cursor in ["-1".to_owned(), "abc".to_owned(), "9".repeat(129)] {
        assert!(adapter
            .receive(ConnectorReceiveRequest { cursor })
            .await
            .is_err());
    }
    let mut invalid = request();
    invalid.attempt_id = "x".repeat(129);
    assert_eq!(
        adapter.send(invalid).await.unwrap_err(),
        TelegramTransportError::InvalidRequestBounds
    );
    let mut invalid = request();
    invalid.text = "x".repeat(1_048_577);
    assert_eq!(
        adapter.send(invalid).await.unwrap_err(),
        TelegramTransportError::InvalidRequestBounds
    );
    assert!(native.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn native_errors_remain_exact_and_ambiguous_sends_stay_uncertain() {
    let native = Arc::new(FixtureTransport::default());
    let registry = ConnectorRegistry::new(native.clone());
    let token = TelegramBotToken::parse("7:fixture").unwrap();
    for error in [
        TelegramTransportError::Timeout,
        TelegramTransportError::Transport,
        TelegramTransportError::HttpStatus { status: 503 },
        TelegramTransportError::UpstreamApi { code: Some(401) },
    ] {
        *native.error.lock().unwrap() = Some(error);
        assert_eq!(
            registry.telegram(&token).send(request()).await.unwrap_err(),
            error
        );
    }
    assert_eq!(
        TelegramTransportError::Timeout.kind(),
        ConnectorErrorKind::Uncertain
    );
    assert_eq!(
        TelegramTransportError::Transport.kind(),
        ConnectorErrorKind::Uncertain
    );
    assert_eq!(
        TelegramTransportError::UpstreamApi { code: Some(401) }.kind(),
        ConnectorErrorKind::Authentication
    );
}
