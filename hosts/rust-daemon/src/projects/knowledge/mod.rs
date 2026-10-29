//! Explicit read-only project folder grants and derived, revocable knowledge.

mod cache;
mod extraction;
mod folder;
mod scope;
mod service;
mod types;

pub(crate) use folder::capture_folder;
#[cfg(test)]
pub(crate) use scope::test_support;
pub(crate) use scope::KnowledgeWitness;
pub(crate) use service::KnowledgeService;
pub(crate) use types::KnowledgeSourceState;
