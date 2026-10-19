//! Drain and shutdown for a supervisor (`/api/system/*`): before Stop,
//! Restart, Quit or Update it begins a drain, polls the in-flight counts
//! until they reach zero (or the owner forces it), then asks for shutdown.
//! `supervisor` is the link back to that supervisor, which the console's
//! status reads and lifecycle actions are relayed over.
mod drain;
#[cfg(test)]
mod drain_tests;
mod supervisor;

pub(crate) use drain::DrainController;
pub use drain::InFlight;
pub(crate) use supervisor::{SupervisorRelay, Unreachable, Upstream};
