//! Host-agnostic client for the Agent Client Protocol (ACP,
//! <https://agentclientprotocol.com>): drive a coding agent (Claude Code,
//! Codex, OpenCode, Gemini CLI through their ACP adapters) with typed events,
//! real cancel, and permission requests routed to the host's own approvals.
//!
//! Why our own JSON-RPC rather than Zed's `agent-client-protocol` crate:
//! - It brings its own runtime stack (`async-io`, `async-process`,
//!   `blocking`) next to the daemon's tokio, and its stdio transport spawns
//!   the agent itself, while every owner-launched program here must run under
//!   [`anima_process::Custody`] so Stop can prove the tree gone.
//! - Its API moved through three major versions in four months, and no 3.x
//!   release was two weeks old when this crate was written.
//! - The client surface we need is small: `initialize`, `session/new|load`,
//!   `session/prompt`, `session/cancel`, `session/update`, and serving
//!   `fs/read_text_file`, `fs/write_text_file` and
//!   `session/request_permission`. Unknown fields are ignored, so additive
//!   protocol changes do not break us.
//!
//! Round 1 offers no `terminal/*`; agents use their own shell, which still
//! runs inside the custody tree. We never call `authenticate`: an agent that
//! needs a login surfaces as [`AcpError::AuthRequired`].
mod fs;
mod log;
mod rpc;
mod session;
mod types;

pub use anima_process::Containment;
pub use session::AcpSession;
pub use types::{
    AcpClientHost, AcpError, AcpEvent, AcpLaunch, AgentInfo, AuthMethod, BoxFuture,
    PermissionOption, PermissionOptionKind, PermissionRequest, PlanEntry, PromptHandle,
    ToolCallStatus, ToolKind, TurnStatus,
};

#[cfg(test)]
mod tests;
