//! Content-free selected-source grants; hosts retain all credential and network authority.

use serde::{Deserialize, Serialize};

/// The owner-selected source represented by one project grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectedSourceKind {
    /// A single selected remote document.
    File,
    /// A selected mail conversation with explicitly bounded coverage.
    EmailThread,
}

/// Immutable account and enrollment bindings for a selected shared source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectedSourceGrant {
    /// Agent retaining the original account authority.
    pub owner_agent_id: String,
    /// Existing native connector identity.
    pub connector_id: String,
    /// Existing provider discriminator.
    pub provider: String,
    /// Canonical selected remote identity, never interpreted as a URL or local path.
    pub remote_id: String,
    /// Previously discovered account citation used for explicit selection.
    pub citation_id: String,
    /// Native credential-lifecycle generation.
    pub connection_generation: String,
    /// Connected-search authority generation.
    pub search_generation: String,
    /// Enrollment epoch preventing disable/re-enable resurrection.
    pub grant_epoch: String,
    /// Native selected-file set revision, when the connector supplies one.
    pub selection_revision: u64,
    /// Mailbox UID validity binding for IMAP.
    pub stream_epoch: Option<String>,
    /// Selected source representation.
    pub source_kind: ConnectedSourceKind,
}

impl ConnectedSourceGrant {
    /// Checks persisted bounds and the closed provider/representation contract.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let bounded = |value: &str, maximum| {
            !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
        };
        [
            (&self.owner_agent_id, 256),
            (&self.connector_id, 256),
            (&self.remote_id, 1024),
            (&self.citation_id, 128),
            (&self.connection_generation, 256),
            (&self.search_generation, 128),
            (&self.grant_epoch, 128),
        ]
        .into_iter()
        .all(|(value, maximum)| bounded(value, maximum))
            && self
                .stream_epoch
                .as_ref()
                .is_none_or(|value| bounded(value, 128))
            && self.selection_revision <= 9_007_199_254_740_991
            && matches!(
                (self.provider.as_str(), self.source_kind),
                (
                    "gdrive" | "onedrive" | "sharepoint",
                    ConnectedSourceKind::File
                ) | (
                    "gmail" | "outlook" | "email",
                    ConnectedSourceKind::EmailThread
                )
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectedSourceGrant, ConnectedSourceKind};

    #[test]
    fn refuses_unknown_providers_and_mismatched_source_representation() {
        let mut grant = ConnectedSourceGrant {
            owner_agent_id: "owner".into(),
            connector_id: "drive".into(),
            provider: "gdrive".into(),
            remote_id: "opaque/id".into(),
            citation_id: "citation".into(),
            connection_generation: "native".into(),
            search_generation: "search".into(),
            grant_epoch: "enrollment".into(),
            selection_revision: 1,
            stream_epoch: None,
            source_kind: ConnectedSourceKind::File,
        };
        assert!(grant.is_valid());
        grant.provider = "https://example.test".into();
        assert!(
            !grant.is_valid(),
            "display identifiers cannot introduce another provider"
        );
        grant.provider = "gmail".into();
        assert!(
            !grant.is_valid(),
            "mail requires explicit bounded thread representation"
        );
        grant.source_kind = ConnectedSourceKind::EmailThread;
        assert!(grant.is_valid());
        grant.grant_epoch.clear();
        assert!(!grant.is_valid(), "every grant binds enrollment revocation");
    }
}
