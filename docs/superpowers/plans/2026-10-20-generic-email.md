# Generic Email Implementation Plan

> Execute in the existing main checkout with bounded subagents under the subagent-driven-development skill. No commits, staging, worktree, push or live provider sends.

**Goal:** Support generic IMAP/SMTP inbox/draft workflows and opt-in automatically replying agent mailboxes.

**Architecture:** Reusable Rust protocol adapter, daemon-owned credentials/state, existing mail approval workflow, durable mailbox claims, existing web Connectors surface.

**Tech stack:** Rust, Tokio, TLS IMAP/SMTP libraries, React, TypeScript SDK, Nx.

## Tasks

- [x] Protocol: add `packages/core-rust/crates/anima-service-adapters/src/email` with validated endpoint/account models, redacted zeroized secrets, cursor/message/outgoing contracts, injectable transport and production IMAP/SMTP. Write protocol/parser/validation tests first, verify failure then implement. Maintain upstream attribution. Only this task owns adapter Cargo.toml and Cargo.lock updates.
- [x] Daemon: extend existing `mail` provider/records/manager, add vault storage, configuration, connect/refresh/send/disconnect generic branches and owner HTTP setup, reuse immutable drafts and tools; tests cover request validation, no secret persistence, no OAuth dependency, sends and lifecycle. Daemon task owns mail/mod.rs, client.rs, generic.rs, credentials.rs, routes/mail.rs and daemon tools/docs contracts.
- [x] Mailbox: new `mail/mailbox.rs` with persistent baseline, bounded UID claim processing, sender/automated-message filters, native coordinator execution, immutable automatic reply drafts and one-attempt delivery. Daemon task supplies shared module/manager fields and calls. Test baseline, restart, unknown sends and disconnect fencing.
- [x] Client: SDK `email` types/setup method, generic email setup and both workflows in Connectors UI; tests for payloads, secret clearing, opt-in allowlist and draft approvals. Preserve existing Discord edits in shared SDK files.
- [x] Independent review: review spec/implementation for material correctness and lifecycle gaps, apply one bounded fix wave.
- [x] Focused verification: 44 daemon mail tests, 23 protocol tests, 80 web tests and 40 SDK tests passed; web/SDK/UI typechecks and scoped Rust formatting/diff checks passed.
- [ ] Full Rust gate: attempted the required target; unchanged anima-worktree Windows canonical-path failures block the core dependency before the daemon target. No worktree-source changes were made.

## Protocol contract

`EmailAccount { address, imap: EmailEndpoint, smtp: EmailEndpoint }`; endpoint `{host,port,security: tls|starttls,username}` with validated constructors/validate method. `EmailSecrets` contains separately redacted zeroized `imap_password` and `smtp_password`.

`EmailCursor {uid_validity:u32, last_uid:u32}`. `EmailInbox {uid_validity:u32,uid_next:u32,messages:Vec<EmailMessage>}`. `EmailMessage {uid:u32,from:String,subject:String,body:String,received_at:String,message_id:Option<String>,automated:bool,authentication_results:Vec<String>}`. `EmailOutgoing {id:String,to:Vec<String>,subject:String,body:String,in_reply_to:Option<String>,automatic:bool}`.

`#[async_trait] trait EmailTransport: Send + Sync`: `verify(&self, account:&EmailAccount,secrets:&EmailSecrets)->Result<EmailInbox,EmailError>` authenticates receiving and sending without sending; `inbox(&self,account,secrets,after:Option<EmailCursor>)->Result<EmailInbox,EmailError>`; `send(&self,account,secrets,message:&EmailOutgoing)->Result<(),EmailError>`. `None` returns newest 25 in ascending UID order; `Some` returns next at most 25 strictly newer UIDs ascending; UIDVALIDITY mismatch => CursorChanged. Errors are typed/redacted Invalid, Unauthorized, Unavailable, CursorChanged, Unknown. Any potentially accepted SMTP send error is Unknown. `EmailClient::new()` production implementation.

## Daemon/mailbox handshake

Provider `Email`; `MailRecord.email: Option<mailbox::EmailMailboxState>` with serde default, containing a cursor, bounded receipts and persistent review reason. `MailConnector.email: Option<GenericEmailSettings>` with camelCase serde/schema; settings `{address,imap,smtp,autoReply:{enabled,allowedSenders,authservId}}`. Generic credentials use separate `EmailCredentialStore`.

`MailManager` adds `runs: AgentRunCoordinator`, `email_transport: Arc<dyn EmailTransport>`, `email_credentials: Arc<dyn EmailCredentialStore>`, a shutdown token and a joined worker. State retains only WeakMailManager to avoid an ownership cycle. `mail/generic.rs` defines setup models, settings and send helpers. `mail/mailbox.rs` owns durable intake and `poll_email_mailbox(&self, agent,id)`, called from the mail background loop for Email records. Tests inject protocol transport and credential store; no live network. Email runs, sessions and usage preserve external provenance through compaction.

## Final evidence

- `bun x nx run rust-daemon:test --excludeTaskDependencies --skipNxCache --args='--lib mail'`: 44 passed on the final source, including generic setup compensation, rollback of new tool grants, owner-boundary requests, automatic replies, restart/unknown-send behavior, source attribution and email compaction.
- `anima-service-adapters` email protocol: 23 tests passed; all 23 also passed in the required core run. Independent re-review found no outstanding material issues after the logging, cursor paging and outgoing-limit fixes.
- Web tests: 80 passed across ConnectorsView, SessionView, session groups and usage. SDK tests: 40 passed across generic email, connectors, sessions and usage. Fresh web typecheck passed with its SDK/UI dependency checks.
- Required `bun x nx run rust-daemon:test --skipNxCache`: blocked in unchanged Windows `anima-worktree` tests because Git rejects `//?/C:/...` paths. The initial restricted run also blocked loopback HTTP fixtures; a named fixture and the full model-adapter tests passed with local access. The workspace-local temp setting adds a misleading not-a-repo fixture failure; that fixture passed with normal OS temp. No repository worktree was created for this feature.
- Logs: `target/generic-email-validation-2026-10-20/`. Existing unrelated warnings and edits were preserved.
- No real mailbox connection/send acceptance, staging, commits, branch changes or push. User setup remains under Connectors -> Email.