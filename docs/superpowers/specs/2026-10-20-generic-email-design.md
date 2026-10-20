# Generic email

The owner requested generic email, both inbox/draft approval and an automatically replying agent mailbox. Work stays on main, unstaged and uncommitted; preserve existing Discord/scheduler edits. No push or live mail sending is authorized.

## Behavior

- Add `email` alongside Gmail/Outlook in the existing mail connector APIs and tools. One generic account per agent, configured with address, IMAP and SMTP endpoints, usernames, TLS or mandatory STARTTLS, and separate passwords held only in the OS credential vault.
- Owner setup verifies receiving and SMTP authentication without sending. Inbox returns bounded text previews; reuse immutable drafts and owner approve/reject/send. Generic email requires no OAuth app registration.
- Automatic replies are opt-in with an explicit nonempty sender allowlist and a pinned receiving mail server `authservId`. Require a DMARC pass aligned with the From domain in that server's Authentication-Results header. Enablement establishes an IMAP UIDVALIDITY/high-water baseline; historical messages are never auto-replied to. Poll new UIDs in ascending bounded pages, retain durable claims before agent execution, and use a separate conversation per sender. Reject self, automated/list/bounce messages and ambiguous senders; address replies only to the admitted From address, never a model-chosen recipient or Reply-To.
- Run via the existing coordinator. Commit the generated reply as a durable immutable draft. Automatic sending uses the same one-attempt send/finalization machinery. Interrupted claims and uncertain sends remain visible and are not replayed. Disconnect or changed configuration prevents stale runs from sending. UIDVALIDITY changes disable automatic work until the owner reconnects/re-enables with a fresh baseline.
- Plain text only initially. MIME text decoding is bounded; attachments and HTML execution are excluded. TLS certificates must validate, STARTTLS cannot downgrade, all network work is time and size bounded. No credentials or provider error bodies in logs/API responses/persistence.

## Architecture and contract

Reusable wire protocol lives in `anima-service-adapters::email`; daemon credentials, authorization, state, routing and run ownership stay under `hosts/rust-daemon/src/connectors/mail`. Follow the upstream installed email adapter for IMAP/SMTP, reply headers and auto-response filtering, preserving an attribution notice.

Shared API: `POST /api/agents/{agent_id}/connectors/mail/email` with `{address, imap:{host,port,security,username}, smtp:{host,port,security,username}, imapPassword, smtpPassword, autoReply:{enabled,allowedSenders,authservId}}`. Security is `tls` or `starttls`. Response `{connector}`. `MailConnector.email` optionally exposes the nonsecret configuration and autoReply settings. GET inbox/drafts and POST draft approve/reject reuse existing paths. Reconnect replaces configuration/credentials and rebaselines future automatic messages. Tool provider name is `email`. Automatic replies require that the receiving MTA strips forged Authentication-Results claiming its pinned ID; manual mode remains usable without this prerequisite.

## Validation

Meaningful protocol, configuration, credential-redaction, owner-boundary, dedupe/baseline, automatic filtering, interrupted/unknown send and lifecycle regressions. SDK/UI tests and typechecks. Required final Rust gate: `bun x nx run rust-daemon:test --skipNxCache` (Windows separate validation target if needed). No real mail-provider acceptance without owner-supplied setup and separately authorized sending.
