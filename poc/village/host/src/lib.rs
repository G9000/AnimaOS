//! Local game host. All persistence and transport stay out of anima-village.
pub mod agent_activity;
pub mod connection;
pub mod engine_bridge;
pub mod http;
mod legacy_memory;
pub mod memory_lifecycle;
pub mod service;
pub mod store;
pub use service::{Customization, Interaction, Service};
pub use store::Store;

#[derive(Clone, Debug)]
pub struct Error {
    pub status: u16,
    pub message: String,
}
impl Error {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    pub fn storage() -> Self {
        Self::new(503, "Village save storage is unavailable")
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod day_tests;

#[cfg(test)]
mod engine_bridge_tests;
#[cfg(test)]
mod engine_capability_tests;

pub mod startup;
