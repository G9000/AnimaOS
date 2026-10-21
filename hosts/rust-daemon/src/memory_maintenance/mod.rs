//! Opt-in, bounded memory reviews. The journal is authoritative across the two stores.
mod records;
mod review;
mod service;
#[cfg(test)]
mod tests;

pub(crate) use records::{
    MaintenanceDecision, MaintenanceOperation, MaintenancePolicy, MaintenanceRegistry, MemoryImage,
};
pub(crate) use service::MaintenanceService;
