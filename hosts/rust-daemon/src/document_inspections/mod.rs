//! Immutable observations of captured Office bytes; reports confer no write or access authority.
mod service;
mod store;
pub(crate) mod worker;

pub(crate) use service::{
    DocumentInspectionService, InspectionEnvelope, InspectionPageRef, InspectionRequest,
    InspectionRuntime, InspectionSubject,
};
