//! Native Telegram bridge. Credentials never enter portable DTOs or metadata.
use anima_service_adapters::connectors::{
    ConnectorErrorKind, ConnectorFailure, ConnectorReceiveRequest, ConnectorSendRequest,
    ConnectorTransport as AsyncConnectorTransport,
};
use async_trait::async_trait;

use crate::connectors::credentials::TelegramBotToken;
use crate::connectors::runtime::TelegramTransport;
use crate::connectors::telegram::{
    TelegramSentMessage, TelegramTransportError, TelegramUpdateBatch,
};
use crate::connectors::TelegramBotIdentity;

/// Bound only for a dispatch using the runtime's already captured credential.
/// No credential lookup, authority decision, or durable state lives here.
pub(crate) struct TelegramAdapter<'a> {
    transport: &'a dyn TelegramTransport,
    token: &'a TelegramBotToken,
}

impl<'a> TelegramAdapter<'a> {
    pub(crate) fn new(transport: &'a dyn TelegramTransport, token: &'a TelegramBotToken) -> Self {
        Self { transport, token }
    }
}

#[async_trait]
impl AsyncConnectorTransport for TelegramAdapter<'_> {
    type Identity = TelegramBotIdentity;
    type InboundBatch = TelegramUpdateBatch;
    type SendReceipt = Vec<TelegramSentMessage>;
    type Error = TelegramTransportError;

    async fn identify(&self) -> Result<Self::Identity, Self::Error> {
        self.transport.get_me(self.token).await
    }

    async fn receive(
        &self,
        request: ConnectorReceiveRequest,
    ) -> Result<Self::InboundBatch, Self::Error> {
        request
            .validate()
            .map_err(|_| TelegramTransportError::InvalidRequestBounds)?;
        let offset = request
            .cursor
            .parse::<i64>()
            .map_err(|_| TelegramTransportError::InvalidOffset)?;
        if offset < 0 {
            return Err(TelegramTransportError::InvalidOffset);
        }
        self.transport.get_updates(self.token, offset).await
    }

    async fn send(&self, request: ConnectorSendRequest) -> Result<Self::SendReceipt, Self::Error> {
        if request.text.is_empty() {
            return Err(TelegramTransportError::InvalidMessage);
        }
        request
            .validate()
            .map_err(|_| TelegramTransportError::InvalidRequestBounds)?;
        // attempt_id is host correlation only; Telegram supplies no idempotency guarantee.
        self.transport
            .send_message(self.token, &request.destination, &request.text)
            .await
    }
}

impl ConnectorFailure for TelegramTransportError {
    fn kind(&self) -> ConnectorErrorKind {
        match self {
            Self::Configuration => ConnectorErrorKind::Configuration,
            Self::InvalidChatId
            | Self::InvalidOffset
            | Self::InvalidMessage
            | Self::InvalidRequestBounds => ConnectorErrorKind::InvalidInput,
            Self::HttpStatus { status: 401 | 403 }
            | Self::UpstreamApi {
                code: Some(401 | 403),
            } => ConnectorErrorKind::Authentication,
            Self::HttpStatus { status: 429 } | Self::UpstreamApi { code: Some(429) } => {
                ConnectorErrorKind::Retryable
            }
            // Network errors may follow a remote effect. Classification cannot authorize a retry.
            Self::Transport
            | Self::Timeout
            | Self::HttpStatus { status: 500..=599 }
            | Self::UpstreamApi {
                code: Some(500..=599),
            } => ConnectorErrorKind::Uncertain,
            Self::ResponseTooLarge
            | Self::InvalidResponse
            | Self::HttpStatus { .. }
            | Self::UpstreamApi { .. } => ConnectorErrorKind::InvalidResponse,
        }
    }
}
