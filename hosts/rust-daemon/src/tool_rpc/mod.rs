//! Native programmatic-tool broker. Physical interpreter ownership is separate.
mod broker;
mod config;
mod service;
mod transport;
mod windows;
mod worker;

pub(crate) use broker::{
    Broker, BrokerCredentials, BrokerDisposition, BrokerError, BrokerReply, RpcGrantFence,
};
pub(crate) use config::ToolRpcConfig;
pub(crate) use service::{reviewable_arguments, ToolRpcService};

#[cfg(test)]
mod tests;
