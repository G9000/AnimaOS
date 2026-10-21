//! Owner-paired nodes reuse MCP discovery, review, grants and dispatch.
pub(crate) mod node;
mod service;
pub(crate) use service::*;
#[cfg(test)]
mod node_tests;
#[cfg(test)]
mod tests;
