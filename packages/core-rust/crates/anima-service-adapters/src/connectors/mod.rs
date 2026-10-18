//! Passive connector metadata and host-implemented transport contracts.
//! Hosts retain credentials, authorization, delivery state and lifecycle ownership.

mod registry;
pub use registry::{ConnectorRegistry, ConnectorRegistryError};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

pub const CONNECTOR_CONTRACT_REVISION: u16 = 1;
pub const MAX_CONNECTOR_PROTOCOL_ID_CHARS: usize = 64;
pub const MAX_CONNECTOR_DISPLAY_NAME_CHARS: usize = 128;
pub const MAX_CONNECTOR_SOURCE_CHARS: usize = 64;
pub const MAX_CONNECTOR_CURSOR_CHARS: usize = 128;
pub const MAX_CONNECTOR_DESTINATION_CHARS: usize = 256;
pub const MAX_CONNECTOR_ATTEMPT_ID_CHARS: usize = 128;
pub const MAX_CONNECTOR_TEXT_CHARS: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorTransportKind {
    Polling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorOperation {
    Identify,
    Receive,
    Send,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorDescriptor {
    pub protocol_id: String,
    pub contract_revision: u16,
    pub display_name: String,
    pub transport_kind: ConnectorTransportKind,
    pub operations: Vec<ConnectorOperation>,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorErrorKind {
    Configuration,
    Authentication,
    InvalidInput,
    InvalidResponse,
    Retryable,
    Uncertain,
}

/// Classification preserves the host's native error without copying sensitive payloads.
pub trait ConnectorFailure: Error + Send + Sync {
    fn kind(&self) -> ConnectorErrorKind;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorReceiveRequest {
    pub cursor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorSendRequest {
    pub destination: String,
    pub text: String,
    pub attempt_id: String,
}

/// Static diagnostics intentionally omit input contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectorValidationError {
    pub field: &'static str,
}
impl fmt::Display for ConnectorValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid connector {}", self.field)
    }
}
impl Error for ConnectorValidationError {}

pub(super) fn bounded(value: &str, max: usize, allow_empty: bool, allow_controls: bool) -> bool {
    (allow_empty || !value.trim().is_empty())
        && value.chars().take(max + 1).count() <= max
        && (allow_controls || !value.chars().any(char::is_control))
}
impl ConnectorReceiveRequest {
    pub fn validate(&self) -> Result<(), ConnectorValidationError> {
        if bounded(&self.cursor, MAX_CONNECTOR_CURSOR_CHARS, true, false) {
            Ok(())
        } else {
            Err(ConnectorValidationError { field: "cursor" })
        }
    }
}
impl ConnectorSendRequest {
    pub fn validate(&self) -> Result<(), ConnectorValidationError> {
        for (field, valid) in [
            (
                "destination",
                bounded(
                    &self.destination,
                    MAX_CONNECTOR_DESTINATION_CHARS,
                    false,
                    false,
                ),
            ),
            (
                "attempt_id",
                bounded(
                    &self.attempt_id,
                    MAX_CONNECTOR_ATTEMPT_ID_CHARS,
                    false,
                    false,
                ),
            ),
            (
                "text",
                !self.text.is_empty() && bounded(&self.text, MAX_CONNECTOR_TEXT_CHARS, true, true),
            ),
        ] {
            if !valid {
                return Err(ConnectorValidationError { field });
            }
        }
        Ok(())
    }
}

/// A host binds credentials privately in its adapter; core supplies no secret DTO.
/// Native associated payloads retain protocol metadata without inventing authority.
#[async_trait]
pub trait ConnectorTransport: Send + Sync {
    type Identity: Send;
    type InboundBatch: Send;
    type SendReceipt: Send;
    type Error: ConnectorFailure;
    async fn identify(&self) -> Result<Self::Identity, Self::Error>;
    async fn receive(
        &self,
        request: ConnectorReceiveRequest,
    ) -> Result<Self::InboundBatch, Self::Error>;
    async fn send(&self, request: ConnectorSendRequest) -> Result<Self::SendReceipt, Self::Error>;
}

#[cfg(test)]
mod tests;
