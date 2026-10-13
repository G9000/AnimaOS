//! Owner-reviewed MCP bindings. Discovery is data, never a grant.
mod records;
mod service;
pub(crate) mod transport;

pub(crate) use records::*;
pub(crate) use service::{McpDispatchStamp, McpManager};

#[cfg(test)]
mod tests;
