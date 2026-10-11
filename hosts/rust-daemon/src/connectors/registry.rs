//! Trusted, immutable built-ins. Metadata describes transport, never grants authority.
use std::sync::Arc;

use anima_core::connectors::{
    ConnectorDescriptor, ConnectorOperation, ConnectorRegistry as MetadataRegistry,
    ConnectorTransportKind,
};

use super::adapters::telegram::TelegramAdapter;
use super::credentials::TelegramBotToken;
use super::runtime::TelegramTransport;

pub(crate) struct ConnectorRegistry {
    metadata: MetadataRegistry,
    telegram: Arc<dyn TelegramTransport>,
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
        Self { metadata, telegram }
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
