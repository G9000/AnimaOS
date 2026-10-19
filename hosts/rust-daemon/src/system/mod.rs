//! Drain and shutdown for a supervisor (`/api/system/*`): before Stop,
//! Restart, Quit or Update it begins a drain, polls the in-flight counts
//! until they reach zero (or the owner forces it), then asks for shutdown.
mod drain;
#[cfg(test)]
mod drain_tests;

pub(crate) use drain::DrainController;
pub use drain::InFlight;
