# Contributing connectors to AnimaOS

M12 provides a Rust transport contract and a Telegram pilot. Community connectors
are reviewed source contributions compiled into a host. They run with the host's
OS privileges and require a rebuild. There is no downloadable connector installer,
dynamic ABI, process/WASM loader, or connector sandbox.

## Where code belongs

- Portable metadata and transport contracts: `anima_core::connectors` in
  `packages/core-rust/crates/anima-core/src/connectors`.
- Native Telegram bridge: `src/connectors/adapters/telegram.rs` in this host.
- Trusted built-in registration: `src/connectors/registry.rs` in this host.
- Host authorization, credential storage, pairing, lifecycle, durable inbound
  admission and outbound settlement: existing connector runtime modules.

`ConnectorRegistry` in core is an immutable metadata catalog. Construction and
lookup do not discover, execute, activate, install, or authorize a transport.
Descriptors contain a protocol ID, contract revision, display name, transport
kind, supported operations and source label. They never contain credentials or
agent/session bindings. The current transport kind is polling; additional kinds
need an explicit contract design before implementation.

## Implementing a transport

Implement `ConnectorTransport` with native associated types. This preserves
protocol-specific sender/chat metadata, cursors, chunk receipts and error semantics
without treating remote input as host authority. The following shape is illustrative;
use existing crates and your adapter's native types, rather than adding dependencies:

```rust
use anima_core::connectors::{
    ConnectorReceiveRequest, ConnectorSendRequest, ConnectorTransport,
};
use async_trait::async_trait;

#[async_trait]
impl ConnectorTransport for NativeAdapter<'_> {
    type Identity = NativeIdentity;
    type InboundBatch = NativeBatch;
    type SendReceipt = NativeReceipt;
    type Error = NativeError; // implements ConnectorFailure

    async fn identify(&self) -> Result<Self::Identity, Self::Error> {
        self.client.identify(self.captured_credential).await
    }

    async fn receive(&self, request: ConnectorReceiveRequest)
        -> Result<Self::InboundBatch, Self::Error>
    {
        request.validate().map_err(NativeError::invalid_request)?;
        self.client.receive(self.captured_credential, &request.cursor).await
    }

    async fn send(&self, request: ConnectorSendRequest)
        -> Result<Self::SendReceipt, Self::Error>
    {
        request.validate().map_err(NativeError::invalid_request)?;
        self.client.send(self.captured_credential, &request).await
    }
}
```

The host privately binds the exact credential it already captured for this
dispatch. Adapters must not reload the vault after a delivery claim, serialize
credentials, or obtain agent/session authority from input or descriptors. Keep
native errors and implement `ConnectorFailure::kind()` with a content-free category.
Timeout/network/server errors may follow a remote effect: `Uncertain` never grants
permission to retry. An `attempt_id` is correlation data, not a provider guarantee
of deduplication or exactly-once delivery. Non-durable Telegram pairing notices
use host-generated correlation IDs and remain non-durable.

Portable request limits count Unicode scalars: cursor 128, destination 256,
attempt ID 128, text 1,048,576. Destination and attempt ID must be nonblank and
control-free; cursor may be empty but is control-free. Text must be nonempty;
whitespace and control characters are preserved for native validation. Adapters
validate before any effect, then apply native parsing and provider limits. Telegram
continues its native text splitting and rejects invalid offsets/chat IDs. Metadata
limits are ASCII protocol ID 64, display name 128 and source 64; registration
rejects duplicate IDs, duplicate operations and unsupported contract revisions.

## Review and verification

Use injected fakes and deterministic barriers/channels. Cover passive registration,
stable metadata ownership, malformed bounds before network effects, exact captured
credentials, native payload/error preservation, and uncertain delivery. Reuse host
regressions for pairing, stop/restart, cancellation, replacement credentials,
durable admission/outbox and recovery. Do not start real providers, services,
databases or containers for these tests, and do not read user profiles or secrets.

Run the existing host gate, which also runs its core Rust dependency tests, with
`CARGO_INCREMENTAL=0`: `bun x nx run rust-daemon:test --skipNxCache`. Run
`bun x nx run rust-daemon:lint --skipNxCache` for Rust formatting. Follow `AGENTS.md`
and the milestone handover for environment and staging constraints.

M12 preserves the owner HTTP API, SDK and console. Its internal registry has no
catalog/installer endpoint. A new provider requires a separate reviewed host
authorization, consent, credential and persistence design; adding a descriptor
alone does not enable it. The selected roadmap is Telegram, Discord, Gmail,
Calendar and Drive. Existing Gmail/Calendar integrations have not been migrated
to this contract in M12; Discord and selected Drive read/search remain future work.

Use Anima/AnimaOS in product names. When adapting upstream code or behavior, record
the exact source revision and scope, retain applicable copyright/license notices,
and distinguish reused behavior from copied code. Existing upstream Markdown
instruction attribution remains in the repository's notices; it does not imply
these connector modules were copied from an upstream implementation.
