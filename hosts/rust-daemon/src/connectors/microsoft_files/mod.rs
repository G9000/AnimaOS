//! Selected OneDrive and organization-approved SharePoint connections.
pub(crate) mod client;
mod manager;
pub(crate) mod model;
pub(crate) mod store;

pub(crate) use manager::{
    can_tombstone_agent, rollback_agent_records, tombstone_agent_records, MicrosoftFileChoice,
    MicrosoftFileChoices, MicrosoftFileConnect, MicrosoftFileManager, MicrosoftSelectedRead,
    WeakMicrosoftFileManager,
};
pub(crate) use model::{
    MicrosoftFileError, MicrosoftFileMetadata, MicrosoftFileMode, MicrosoftFileRecord,
    MicrosoftFileStatus, MicrosoftSelectedFile,
};
