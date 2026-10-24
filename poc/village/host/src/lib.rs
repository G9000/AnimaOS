//! Local game host. All persistence and transport stay out of anima-village.
pub mod connection;
pub mod http;
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
