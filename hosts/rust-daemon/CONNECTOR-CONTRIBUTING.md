# Contributing connectors to AnimaOS

M12 introduced the Rust transport contract and Telegram pilot. M13 adds native
selected-channel Discord REST chat. Community connectors
are reviewed source contributions compiled into a host. They run with the host's
OS privileges and require a rebuild. There is no downloadable connector installer,
dynamic ABI, process/WASM loader, or connector sandbox.

## Where code belongs

- Portable metadata and transport contracts: `anima_service_adapters::connectors`
  in `packages/core-rust/crates/anima-service-adapters/src/connectors`.
- Native Telegram bridge: `src/connectors/adapters/telegram.rs` in this host.
- Native Discord bridge and lifecycle: `src/connectors/adapters/discord.rs` and
  `src/connectors/discord/` in this host.
- Trusted built-in registration: `src/connectors/registry.rs` in this host.
- Host authorization, credential storage, pairing, lifecycle, durable inbound
  admission and outbound settlement: existing connector runtime modules.

`ConnectorRegistry` in the adapter crate is an immutable metadata catalog. Construction and
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
use anima_service_adapters::connectors::{
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

M12 preserved the owner HTTP API, SDK and console; M13 adds a separate Discord
owner namespace and setup card. The internal registry has no
catalog/installer endpoint. A new provider requires a separate reviewed host
authorization, consent, credential and persistence design; adding a descriptor
alone does not enable it. The selected roadmap is Telegram, Discord, Gmail,
Calendar and Drive. Existing Gmail/Calendar integrations retain their native
services. Registry migration alone is not required for functional integrations.
Selected Drive read/search is the next bounded milestone.

## Selected Discord channel

The owner configures one guild text channel and a nonempty list of permitted
user IDs. A bot's access to the channel does not establish its privacy; the owner
checks channel permissions and confirms the selection. New records are saved
disabled. First activation establishes a latest-message baseline, so earlier
history is not dispatched. Later enable/restart preserves the saved cursor.
The bot requires View Channel, Read Message History and Send Messages permissions.
Message Content intent restrictions also affect REST responses; configure the
bot in the Discord Developer Portal. See the official [message resource](https://docs.discord.com/developers/resources/message)
and [intent documentation](https://docs.discord.com/developers/events/gateway).

The client polls the selected channel and proves a bounded backward catch-up
window before the host atomically commits permitted receipts and its cursor.
An unproven or oversized backlog pauses visibly. Sender IDs, message IDs and
channel IDs are native decimal strings; they never become floating-point numbers
or grant agent authority. Self, bot, webhook and unsupported messages do not run
the agent. Channel/user selection changes require disconnecting and recreating
the connector. Credentials remain private, generation-bound and independently
stored from Telegram.

Each reply chunk has a durable send intent and receipt. Ambiguous/partial sends
are Unknown and do not automatically replay. The owner checks Discord before
explicitly suppressing uncertain output; this retains delivered receipts and
history, sends nothing and allows new work. Safe Retry refuses Unknown output.
Provider retry delays are honored; recent nonce deduplication does not provide
durable exactly-once delivery. All mentions are disabled on outgoing messages.
At most eight conservatively bounded chunks are sent, with an Anima truncation
notice if needed; the complete reply remains in session history.

REST polling has seconds of latency and cannot observe Gateway edits, deletions,
reactions, threads, slash commands or voice. Messages deleted before polling can
be missed. Stop is available through the web conversation controls. These limits
are explicit; this is not a complete upstream Discord adapter port.

Snapshot version 12 adds native Discord journals. A version 11 JSON store is
backed up as `.pre-discord.bak` before upgrading; existing earlier backup suffixes
remain unchanged. Older binaries refuse the newer snapshot, so rollback uses
the pre-upgrade backup. Verification uses fake stores/transports and does not
touch an owner's actual store or perform live Discord delivery.

Use Anima/AnimaOS in product names. When adapting upstream code or behavior, record
the exact source revision and scope, retain applicable copyright/license notices,
and distinguish reused behavior from copied code. Existing upstream Markdown
instruction attribution remains in the repository's notices; it does not imply
these connector modules were copied from an upstream implementation.

## Selected Drive service

Drive is an on-demand native tool service, not a messaging transport. It uses the
existing Google app lease and PKCE flow with separate generation-specific token
keys. Broad provider `drive.readonly` authorization must be disclosed alongside
Anima's immutable 1–16-ID allowlist. Never turn a browser ID, fetched document or
model argument into authority. Every asynchronous read rechecks the current agent,
connector generation, selected IDs, pinned account, app revision and tool grant.
Managers retain weak daemon-state references; state locks never span network or
persistence waits. Callback and refresh completion cannot resurrect disconnect.

Only exact-ID metadata GETs, selected filename matching, Docs text exports and
stored UTF-8 plain text are supported. No files.list, raw provider query, folder or
shortcut traversal, arbitrary URL fetch, write/share/delete, indexing or automatic
memory ingestion. Read bounds/provenance and unsupported formats are documented
in the [daemon setup guide](README.md#selected-drive-files). Adapted source scope
and full MIT terms are in [Drive attribution](src/connectors/drive/HERMES-NOTICE).

Local disconnect durably removes tool authority before private key cleanup; failed
cleanup remains a tombstone/intention and cannot restore access. Do not remotely
revoke Google project scopes as a Drive-only operation: Gmail and Calendar can be
invalidated too. Native records and cleanup intentions use snapshot version 13;
v12 JSON input gets `.pre-drive.bak` before upgrade. Pending nonce/verifier and
credentials are not snapshot data. Tests use fake transports/vaults, deterministic
race barriers and temporary stores, never an owner's real credentials or files.
