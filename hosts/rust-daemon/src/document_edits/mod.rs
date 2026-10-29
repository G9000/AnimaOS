//! Private imported-document proposals and exact, capacity-reserved publication authority.
pub(crate) mod approval;
mod owner_inspection;
pub(crate) use owner_inspection::{readonly_scope_in, OwnerInspectionScope};
pub(crate) mod preview_context;
pub(crate) mod projection;
pub(crate) mod recovery;
mod registry;
pub(crate) mod service;
pub(crate) mod store;
pub(crate) mod worker;
pub(crate) mod workspace_boundary;
pub(crate) use service::{validate_final, DocumentEditService, WeakDocumentEditService};
pub(crate) use store::DocumentEditStore;
pub(crate) use worker::DocumentEditWorker;

pub(crate) use registry::{
    DocumentEditDraft, DocumentEditRecord, DocumentEditRegistry, DocumentEditScope,
    DocumentEditSource, DocumentEditStatus, DocumentPublicationPhase, DocumentPublicationReceipt,
    ExpectedDestination,
};

#[cfg(test)]
mod projection_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
pub(crate) use service_tests::{
    metadata_review_registry_fixture, publication_readback_proof_fixture,
};
