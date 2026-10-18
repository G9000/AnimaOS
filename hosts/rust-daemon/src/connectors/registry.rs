//! Trusted, immutable built-ins. Metadata describes transport, never grants authority.
use std::sync::Arc;

use anima_service_adapters::connectors::{
    ConnectorDescriptor, ConnectorOperation, ConnectorRegistry as MetadataRegistry,
    ConnectorTransportKind,
};

use super::adapters::discord::DiscordAdapter;
use super::adapters::telegram::TelegramAdapter;
use super::credentials::TelegramBotToken;
use super::discord::{client::DiscordTransport, credentials::DiscordBotToken};
use super::runtime::TelegramTransport;

pub(crate) struct ConnectorRegistry {
    metadata: MetadataRegistry,
    telegram: Arc<dyn TelegramTransport>,
    discord: Option<Arc<dyn DiscordTransport>>,
}

impl ConnectorRegistry {
    pub(crate) fn new(telegram: Arc<dyn TelegramTransport>) -> Self {
        let metadata = MetadataRegistry::new([ConnectorDescriptor {
            protocol_id: "telegram".to_owned(),
            contract_revision: 1,
            display_name: "Telegram".to_owned(),
            transport_kind: ConnectorTransportKind::Polling,
            operations: vec![
                ConnectorOperation::Identify,
                ConnectorOperation::Receive,
                ConnectorOperation::Send,
            ],
            source: "anima-native".to_owned(),
        }])
        .expect("built-in Telegram descriptor is valid");
        Self {
            metadata,
            telegram,
            discord: None,
        }
    }

    pub(crate) fn with_discord(
        telegram: Arc<dyn TelegramTransport>,
        discord: Arc<dyn DiscordTransport>,
    ) -> Self {
        let metadata = MetadataRegistry::new(["telegram", "discord"].map(|id| {
            ConnectorDescriptor {
                protocol_id: id.to_owned(),
                contract_revision: 1,
                display_name: if id == "telegram" {
                    "Telegram"
                } else {
                    "Discord"
                }
                .into(),
                transport_kind: ConnectorTransportKind::Polling,
                operations: vec![
                    ConnectorOperation::Identify,
                    ConnectorOperation::Receive,
                    ConnectorOperation::Send,
                ],
                source: "anima-native".into(),
            }
        }))
        .expect("built-in transport descriptors are valid");
        Self {
            metadata,
            telegram,
            discord: Some(discord),
        }
    }

    /// Passive preflight access for native channel metadata/history operations.
    pub(crate) fn discord_native(&self) -> Option<&dyn DiscordTransport> {
        self.discord.as_deref()
    }

    pub(crate) fn discord<'a>(
        &'a self,
        token: &'a DiscordBotToken,
        channel_id: &'a str,
    ) -> DiscordAdapter<'a> {
        self.metadata
            .get("discord")
            .expect("Discord is registered at construction");
        DiscordAdapter::new(
            self.discord
                .as_deref()
                .expect("Discord transport is configured"),
            token,
            channel_id,
        )
    }

    /// Binding is passive and borrows the credential already fenced by the host.
    pub(crate) fn telegram<'a>(&'a self, token: &'a TelegramBotToken) -> TelegramAdapter<'a> {
        self.metadata
            .get("telegram")
            .expect("Telegram is registered at construction");
        TelegramAdapter::new(self.telegram.as_ref(), token)
    }
}

#[cfg(test)]
mod tests;
