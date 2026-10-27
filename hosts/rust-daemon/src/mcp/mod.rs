//! Owner-reviewed MCP bindings. Discovery is data, never a grant.
pub(crate) mod credentials;
mod records;
mod service;
mod stdio;
pub(crate) mod transport;

pub(crate) use records::*;
pub(crate) use service::{McpDispatchStamp, McpManager, McpOAuthStart, McpOAuthStatus};

#[cfg(test)]
mod tests;
