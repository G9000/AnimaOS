use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// An owned immutable catalog. Construction never discovers or executes transports.
#[derive(Debug, Clone)]
pub struct ConnectorRegistry {
    descriptors: BTreeMap<String, ConnectorDescriptor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorRegistryError {
    InvalidProtocolId,
    UnsupportedRevision,
    InvalidDisplayName,
    InvalidSource,
    DuplicateOperation,
    DuplicateProtocolId,
}
impl fmt::Display for ConnectorRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidProtocolId => "invalid connector protocol ID",
            Self::UnsupportedRevision => "unsupported connector contract revision",
            Self::InvalidDisplayName => "invalid connector display name",
            Self::InvalidSource => "invalid connector source",
            Self::DuplicateOperation => "duplicate connector operation",
            Self::DuplicateProtocolId => "duplicate connector protocol ID",
        })
    }
}
impl Error for ConnectorRegistryError {}

impl ConnectorRegistry {
    pub fn new(
        descriptors: impl IntoIterator<Item = ConnectorDescriptor>,
    ) -> Result<Self, ConnectorRegistryError> {
        let mut catalog = BTreeMap::new();
        for descriptor in descriptors {
            let id = &descriptor.protocol_id;
            if !bounded(id, MAX_CONNECTOR_PROTOCOL_ID_CHARS, false, false)
                || !id.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
                })
            {
                return Err(ConnectorRegistryError::InvalidProtocolId);
            }
            if descriptor.contract_revision != CONNECTOR_CONTRACT_REVISION {
                return Err(ConnectorRegistryError::UnsupportedRevision);
            }
            if !bounded(
                &descriptor.display_name,
                MAX_CONNECTOR_DISPLAY_NAME_CHARS,
                false,
                false,
            ) {
                return Err(ConnectorRegistryError::InvalidDisplayName);
            }
            if !bounded(&descriptor.source, MAX_CONNECTOR_SOURCE_CHARS, false, false) {
                return Err(ConnectorRegistryError::InvalidSource);
            }
            if descriptor.operations.iter().collect::<BTreeSet<_>>().len()
                != descriptor.operations.len()
            {
                return Err(ConnectorRegistryError::DuplicateOperation);
            }
            if catalog.insert(id.clone(), descriptor).is_some() {
                return Err(ConnectorRegistryError::DuplicateProtocolId);
            }
        }
        Ok(Self {
            descriptors: catalog,
        })
    }

    pub fn get(&self, protocol_id: &str) -> Option<&ConnectorDescriptor> {
        self.descriptors.get(protocol_id)
    }

    pub fn list(&self) -> impl ExactSizeIterator<Item = &ConnectorDescriptor> {
        self.descriptors.values()
    }
}
