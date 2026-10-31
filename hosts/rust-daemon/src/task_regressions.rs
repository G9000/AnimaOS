//! Explicit owner comparisons of complete tasks in private fixture workspaces.
mod contracts;
mod fixture_tools;
mod runner;
mod service;
#[cfg(test)]
mod tests;

pub(crate) use contracts::{CompareRequest, RegressionError, RegressionRegistry};
pub(crate) use service::TaskRegressionService;
