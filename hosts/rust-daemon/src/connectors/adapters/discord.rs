//! Native bridge bound to the exact captured credential and owner-selected channel.
use crate::connectors::discord::{
    client::{DiscordTransport, DiscordTransportError},
    credentials::DiscordBotToken,
    model::{DiscordBotIdentity, DiscordMessageBatch, DiscordSendReceipt, DiscordSnowflake},
    text::valid_chunk,
};
use anima_core::connectors::{
    ConnectorErrorKind, ConnectorFailure, ConnectorReceiveRequest, ConnectorSendRequest,
    ConnectorTransport as AsyncConnectorTransport,
};
use async_trait::async_trait;
pub(crate) struct DiscordAdapter<'a> {
    transport: &'a dyn DiscordTransport,
    token: &'a DiscordBotToken,
    channel: &'a str,
}
impl<'a> DiscordAdapter<'a> {
    pub(crate) fn new(
        transport: &'a dyn DiscordTransport,
        token: &'a DiscordBotToken,
        channel: &'a str,
    ) -> Self {
        Self {
            transport,
            token,
            channel,
        }
    }
}
#[async_trait]
impl AsyncConnectorTransport for DiscordAdapter<'_> {
    type Identity = DiscordBotIdentity;
    type InboundBatch = DiscordMessageBatch;
    type SendReceipt = DiscordSendReceipt;
    type Error = DiscordTransportError;
    async fn identify(&self) -> Result<Self::Identity, Self::Error> {
        self.transport.get_me(self.token).await
    }
    async fn receive(
        &self,
        request: ConnectorReceiveRequest,
    ) -> Result<Self::InboundBatch, Self::Error> {
        request
            .validate()
            .map_err(|_| DiscordTransportError::InvalidInput)?;
        if DiscordSnowflake::cursor(&request.cursor).is_none()
            || DiscordSnowflake::parse(self.channel).is_none()
        {
            return Err(DiscordTransportError::InvalidInput);
        }
        self.transport
            .receive(self.token, self.channel, &request.cursor)
            .await
    }
    async fn send(&self, request: ConnectorSendRequest) -> Result<Self::SendReceipt, Self::Error> {
        request
            .validate()
            .map_err(|_| DiscordTransportError::InvalidInput)?;
        if request.destination != self.channel
            || DiscordSnowflake::parse(self.channel).is_none()
            || !valid_chunk(&request.text)
        {
            return Err(DiscordTransportError::InvalidInput);
        }
        self.transport
            .send_chunk(self.token, self.channel, &request.text, &request.attempt_id)
            .await
    }
}
impl ConnectorFailure for DiscordTransportError {
    fn kind(&self) -> ConnectorErrorKind {
        match self {
            Self::Configuration => ConnectorErrorKind::Configuration,
            Self::InvalidInput => ConnectorErrorKind::InvalidInput,
            Self::Authentication | Self::PermissionDenied => ConnectorErrorKind::Authentication,
            Self::RateLimited { .. } | Self::Transport | Self::HttpStatus { status: 500..=599 } => {
                ConnectorErrorKind::Retryable
            }
            Self::Uncertain => ConnectorErrorKind::Uncertain,
            Self::InvalidResponse
            | Self::BacklogLimit
            | Self::HttpStatus { .. }
            | Self::ResponseTooLarge => ConnectorErrorKind::InvalidResponse,
        }
    }
}
