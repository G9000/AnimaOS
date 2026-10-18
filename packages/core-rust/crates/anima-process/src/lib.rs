//! Host-agnostic process custody: launch a program so that its whole process
//! tree can be stopped and proven gone. Hosts use it for anything that runs on
//! the owner's behalf (background commands, interactive app sessions).
mod custody;
pub use custody::{Containment, Custody};

#[cfg(test)]
mod tests;
