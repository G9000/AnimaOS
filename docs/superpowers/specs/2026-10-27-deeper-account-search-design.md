# Deeper connected-account search design

Status: implemented in the shared tree; integration and native verification remain in progress.
Date: 2026-10-27 (Asia/Kuala_Lumpur).
Owner: implementation chat for feature 4, deeper connected-account search.

## Objective and authorized boundary

Let an owner deliberately search older inbox messages, supported attachment text, and selected Microsoft files with useful citations and honest coverage. Keep execution in the native Rust daemon. This is personal account content search, not autonomous web research or a new account-workflow product.

The initial design phase wrote only this spec and its companion plan. The coordinator subsequently released implementation with shared-file ownership and an exclusive Rust build slot. No real account reads or credentials were used; provider research used public documentation and validation uses synthetic data.

## Implementation evidence as of 2026-10-28

- Independent source review cleared the original six account-search findings and the two subsequent Microsoft persistence and access-control findings. This is source-review evidence, not proof of runtime behavior.
- Populated saved-corpus, fresh-runtime, failed-save, and in-flight race fixtures have been added for Microsoft parent and attachment visibility. Their native execution remains pending.
- The account-search browser acceptance tests passed at both 1280 px and 390 px using the static synthetic fixture. They cover explicit source opt-in, history and attachment controls, file-selection bounds, failed-save and pending states, and horizontal overflow. The combined browser run also contained failures in another workstream; these two passing cases do not establish a green combined run.
- The SDK build and typecheck, and the current web-e2e no-output typecheck and lint, passed. Web-e2e lint reported warnings in existing unrelated files.
- The earlier focused daemon compile failed on shared integration gaps; no account-search Rust tests ran in that attempt. Combined native tests and Rust lint are still pending. No real provider account was accessed.

## Baseline inspected during design

The following describes the sources inspected before this implementation was released; it is design baseline context, not a current implementation audit:

- `hosts/rust-daemon/src/connected_search/{mod.rs,service.rs,providers.rs,tests.rs}`: 120-second poll interval, 90-second per-read timeout, 16 MiB serialized index, 8 KiB text per document, 128 sources, and 4,096 independent seen IDs per source. The current successful snapshot replaces all retained source documents. Its normalized content digest drives both search revisions and workflow changes.
- `hosts/rust-daemon/src/connectors/mail/{search.rs,client_search.rs,client.rs,mod.rs}`: Gmail and Outlook read up to 100 recent inbox messages; IMAP reads up to 25. Search is independent of automatic-reply cursors, but `search_documents` also updates the mail connector's recent preview. Historical reads must avoid that preview mutation.
- `hosts/rust-daemon/src/account_workflows.rs`: only newly observed live changes may be admitted after rule baseline; source generation, rule revision, template revision, payload fingerprint, durable receipts, and dispatch checks protect workflow admission. There are 512 workflow event receipts; exhaustion fails closed. History must never consume these receipts.
- `hosts/rust-daemon/src/connectors/drive/`: up to 16 selected Google Drive files; Docs/plain-text content and other formats' metadata. The source generation includes the selected-file set. This feature does not change that existing authorization identity scheme.
- `hosts/rust-daemon/src/attachments/documents/{worker.rs,mod.rs}` and `packages/core-rust/crates/anima-document-content/src/lib.rs`: existing bounded DOCX/PDF/XLSX extraction. Production parsing requires the owned worker process. That worker currently returns unavailable on non-Windows platforms; do not call the parser in-process as a fallback.
- `hosts/rust-daemon/src/routes/connected_search.rs`, `packages/sdk/src/connected-accounts.ts`, and `apps/web/src/components/connected-accounts/`: owner-only configuration, retained-text search/read, source freshness/scope, and the existing nontechnical account UI.
- `hosts/rust-daemon/src/connectors/oauth_apps.rs` and `hosts/rust-daemon/src/routes/mod.rs`: shared Google/Microsoft application setup and lifecycle locks. Microsoft currently advertises only the Outlook callback and checks only mail dependencies before changing that application.

The existing implementation cannot safely gain depth just by increasing the snapshot limit: it would replace historical documents on the next head refresh, feed imported IDs into workflow detection, and make initial extraction look like content changes. Raising limits alone is rejected.

## Approaches considered

1. **Separate live detection, history import, and enrichment inside the existing index service (recommended).** Reuse current owner grants, persistence transactions, search, citations, and native managers. Add bounded import state without turning imports into events. Moderate implementation scope and additive SDK changes.
2. **Search providers on every user query.** Less retained text, but unpredictable latency, repeated provider reads, weak offline/restart behavior, and misleading cross-provider completion. It does not solve safe historical workflow handling.
3. **Introduce a separate database/vector index and general connector framework.** More headroom, but adds deployment, migration, dependencies, and broad shared-file changes beyond this feature. Retain the current lexical search and 16 MiB cap for this release.

## Product decisions and controls

Existing enabled sources retain their current behavior until an owner changes settings. New configuration fields default to recent content and attachment metadata only. Provider connection alone never enables search or downloads attachment bodies.

For Gmail/Outlook, show:

- **Search back:** Recent inbox / 30 days / 90 days / 1 year. The default is Recent inbox. This release stays in the inbox; it does not silently add sent mail, archives, trash, spam, shared mailboxes, or other folders.
- **Keep locally:** Up to 100 / 500 / 2,000 messages, with an independent serialized per-source byte ceiling of 2 MiB. Finite history choices also expire retained mail by provider received time outside that selected rolling window. The selected count and byte ceilings are limits, never a promise that all messages fit.
- **Include attachment text:** Off by default. The consent copy names plain text, CSV, PDF, Word, and Excel; office/PDF availability depends on the document worker. No OCR, image/audio/video extraction, password prompts, archives, nested Outlook items, or cloud-link traversal.
- **Pause older-content import / Resume**, separate from live refresh. **Remove older content** changes depth to Recent inbox, cancels history work, and purges historical text and its attachment children. Minimal hashed import-suppression receipts remain until source disablement/revocation, so clearing content cannot cause replay.

IMAP remains a truthful recent-inbox source with no historical or attachment backfill in this release. Its transport has neither a paged history interface nor safe attachment-part fetch support. Calendar scope remains 30 days past through 180 days ahead. Google Drive stays selected files; binary-document extraction can use the same explicit attachment/document-text setting through a bounded byte-read extension, without widening selected-file scope or changing its live workflow fingerprint.

Microsoft files get a separate connection card and independent search opt-in. OneDrive supports at most 16 selected files, not recursive account indexing. SharePoint supports files from explicitly approved sites under selected-resource consent, with the same 16-file aggregate connector limit. Microsoft files are search-only in this feature: advertise `workflowEventKinds: []` and exclude them from the account-rule source selector. Adding new Microsoft workflow event semantics is out of scope.

## Provider documentation and consent

Public documentation was checked through Context7 using `/websites/developers_google_workspace_gmail_api` and `/microsoftgraph/microsoft-graph-docs-contrib`, with primary-page checks for details not adequately returned.

| Capability | Existing support / proposed delegated consent | Reconsent and limitations |
| --- | --- | --- |
| Gmail history and attachment bodies | Existing `gmail.readonly` covers reads; retain existing mail scopes without adding a new write scope | No additional scope for this feature. Reduced/invalid grants require reconnect. Gmail metadata-only grants cannot support full text. |
| Outlook history and file attachments | Existing `Mail.Read` covers message and attachment reads | No additional scope. Do not add Microsoft file permission to the Outlook token flow. |
| Google selected-file binary text | Existing `drive.readonly`, same 16 selected IDs | No new scope; owner must explicitly enable document text. Do not widen to all files locally. |
| OneDrive own files | New file-only flow: `offline_access User.Read Files.Read` | Owner must connect Microsoft files and consent. A new callback must be registered in their existing Microsoft application. Tokens are separate from Outlook. |
| SharePoint approved sites | Separate flow: `offline_access User.Read Sites.Selected`, with an external read-role assignment on each approved site | Administrator consent/resource setup is required. The app does not create permission assignments. Personal Microsoft accounts do not offer this SharePoint mode. No automatic `Files.Read.All` / `Sites.Read.All` fallback. |

Do not use `Files.Read.Selected`: Microsoft documents that legacy scope for Office file handlers, not direct Graph calls. `Files.SelectedOperations.Selected` is a possible later, narrower file/folder-grant mode; site-level selection is chosen here to allow owner browsing within an approved library without breaking item-level permission inheritance. The Graph source remains subject to both the user's access and the application's selected grant. Consent by itself does not create access to selected sites. These requirements follow Microsoft's [selected-permission model](https://learn.microsoft.com/en-us/graph/permissions-selected-overview) and [permission reference](https://github.com/microsoftgraph/microsoft-graph-docs-contrib/blob/main/concepts/permissions-reference.md).

Gmail uses timestamp `after`/`before` queries and `pageToken`; use Unix seconds plus local millisecond range checks, avoiding date-string timezone ambiguity. The history endpoint is a change feed, not historical-mail enumeration; expired history IDs require a new sync. Use [message listing](https://developers.google.com/workspace/gmail/api/guides/list-messages), [filtering](https://developers.google.com/workspace/gmail/api/guides/filtering), [attachment get](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages.attachments/get), and [sync guidance](https://developers.google.com/workspace/gmail/api/guides/sync).

Outlook uses inbox message paging, `receivedDateTime` filtering/order, `$select`, text-body preference, and validated `@odata.nextLink`; keep the current external-ID representation. Do not add `Prefer: IdType="ImmutableId"` to an existing source without a separate alias migration, because that would change citation/event identities. File attachment bytes use `/$value` or bounded `contentBytes`. Item/reference attachments remain unsupported, as described in [attachment get](https://learn.microsoft.com/en-us/graph/api/attachment-get?view=graph-rest-1.0) and [message listing](https://github.com/microsoftgraph/microsoft-graph-docs-contrib/blob/main/api-reference/v1.0/api/user-list-messages.md).

Microsoft file content uses Graph drive/item endpoints. Graph returns a temporary download redirect; follow it with a separate unauthenticated download client after URL/network checks, never forwarding the Graph bearer token. Do not persist or expose the temporary URL. See [content download](https://learn.microsoft.com/en-us/graph/api/driveitem-get-content?view=graph-rest-1.0). Selected-file polling is sufficient; a whole-drive delta feed would read beyond the selected local corpus and is unnecessary here.

## Native types and state separation

Keep source generation and `grant_epoch` as authorization/enrollment identities. A depth, retention, pause, parser-version, or attachment-setting change increments `settings_revision`; it must not call `SourceIndex::new`, clear `seen`, alter `stream_epoch`, reset rule baselines, or replace workflow receipts.

Proposed crate-private Rust contracts, serialized in camelCase where exposed:

```rust
enum HistoryDepth { Recent, Days30, Days90, Days365 }
enum AttachmentTextMode { MetadataOnly, SupportedText }

struct SearchSettings {
    history_depth: HistoryDepth,
    retained_item_limit: u16, // only 100, 500, 2_000
    attachment_text: AttachmentTextMode,
    history_paused: bool,
}

enum IngestionOrigin { LivePoll, HistoryBackfill, ContentEnrichment }

struct SourceReadContext {
    authority: SourceAuthority,
    grant_epoch: String,
    settings_revision: u64,
    selection_revision: Option<u64>,
}

enum ImportCursor {
    Gmail { page_token: Option<String> },
    Outlook { next_link: Option<String> },
}

struct HistoryPage {
    documents: Vec<IndexedDocument>,
    next_cursor: Option<ImportCursor>,
    exhausted: bool,
    // Validated internally; never a provider assertion of whole-account coverage.
}

trait SourceReader {
    async fn read_live(&self, context: &SourceReadContext)
        -> Result<SourceSnapshot, SearchError>;
    async fn read_history_page(&self, context: &SourceReadContext,
        request: &HistoryPageRequest) -> Result<HistoryPage, SearchError>;
}
```

`HistoryPageRequest` contains a fixed `[from_ms, before_ms)` range, bounded provider cursor, page size, deadline, and cancellation signal. Provider implementations receive typed requests, not arbitrary URLs supplied by clients. Cap opaque cursor bytes at 8 KiB, validate the variant against the provider, and validate Graph origin plus expected inbox path on every continuation.

Add defaulted fields to `SourceIndex`:

- `settings`, `settings_revision` (legacy default 1).
- `history`: fixed range, next cursor, `pass_id`, state/reason, attempts/success times, enumerated count, retained count, and `not_before_ms` for rate limits.
- `imported`: at most 2,000 `(citation_id, base_revision)` suppression receipts, independent of the existing live `seen` map. Never evict these receipts to make room for text or attachments. Stop new history admission when this bound is reached, even if text has subsequently expired.
- `live_event_fence_at_ms`: a stable cutoff used only to reject unknown old mail from workflow admission; legacy sources set it at migration without clearing their existing baseline/seen state.
- `attachment_work`: a bounded resumable queue keyed by parent citation, attachment identity, parent base revision, and parser version.

Extend `IndexedDocument` additively with defaulted content metadata: body/file/event/attachment role, optional parent citation, provider received timestamp, provider modified timestamp, retained origin, attachment format/status/omissions, and bounded extraction locators. Preserve existing parent citation calculation and existing live `revision` calculation. Normalize newly fetched raw provider documents once; do not run retained normalized documents through the digest again while merging. Add a separate `content_revision` for extracted content; it never participates in live event IDs. A mail attachment keeps `DocumentKind::Mail` plus `contentRole: "attachment"`, so existing kind filters continue to work.

Attachment identities use the provider's part/attachment ID under the exact parent identity; generate a domain-separated child citation digest. Do not use filenames as IDs or filesystem paths. Parent and child authorization, retention, and deletion are inseparable. Search results/read responses expose `parentCitationId` and text locators such as PDF page, DOCX paragraph, or XLSX cell. Render the extracted source as quoted data; never execute macros, formulas, links, or document instructions.

## Ingestion invariants and algorithm

Three explicit paths replace overloaded snapshot application:

1. **Live poll:** read the current head under existing native tool permissions. Update the live fingerprint/seen map using only the original provider body/metadata fields. Preserve retained history and enrichment while updating/removing the live head membership. Only this path can construct workflow-eligible `SourceChange`s. Add `origin: LivePoll` and reject other origins in workflow admission as defense in depth. A rule's first fresh live poll still establishes its baseline; history cannot establish one.
2. **History page:** merge parent content by stable citation into retained history and add independent import-suppression receipts. Never call workflow admission, update the live seen map, change the live baseline, or mutate mailbox previews/automatic-reply cursors. Atomically commit documents, suppression receipts, cursor, and coverage. A malformed/partial page never advances its cursor. Replays are idempotent.
3. **Enrichment:** add/update child text/status for an unchanged parent base revision. Do not alter the parent live revision, live seen map, baseline, or workflow state. Late results for a changed/deleted parent or stale settings revision are discarded. Initial extraction, retries, parser upgrades, and enabling attachment text produce zero workflow events.

Before applying any result, recheck agent, connector generation, grant epoch, settings revision, selected resources, current native read permission, and active status inside the existing durable transaction. For model calls also recheck `connected_search` at execution time. Never hold daemon state/transaction locks across provider I/O or document parsing.

When a previously imported parent later appears in a live head snapshot, its import receipt suppresses `Observed`; seed/update live comparison state silently if needed. If a receipt is absent, an unknown mail item with provider received time before the stable live event fence, or with missing/invalid time, is conservatively baseline-only. Provider timestamps are evidence for exclusion, not proof that a newly observed event occurred. `observed_at_ms` is assigned only by the live poll and is never copied from `internalDate`, `receivedDateTime`, or file modification time. This deliberately prefers missed ambiguous old-mail triggers over running workflows for history. Existing receipts/event IDs remain valid.

The mailbox's actual IMAP UIDVALIDITY change remains an independent stream reset under existing behavior; a history setting or parser result must never synthesize such a reset. This release does not backfill IMAP.

## Backfill lifecycle, freshness, retention, and budgets

Selecting deeper history creates a fixed range ending at the owner's configuration time. Schedule one bounded page after live work, then persist its cursor. On later polls continue that pass. When complete, the rolling window advances through live refresh plus periodic bounded historical reconciliation. Do not chase a moving upper bound during enumeration. Gaps remain visible until a pass completes.

Each source has one ingestion gate. The scheduler prioritizes live work and rotates sources fairly; history cannot hold the mail manager's per-account lock for an entire import. Proposed budgets:

| Resource | Ceiling |
| --- | --- |
| Serialized connected-search state | Existing 16 MiB, including new metadata/queues/receipts |
| Historical/extracted expansion | At most 12 MiB globally; reserve 4 MiB of the existing cap for live state/receipt growth |
| Retained source content/metadata | At most 2 MiB; preserve valid legacy state above this cap and admit no expansion until it fits |
| Parent mail / imported suppression receipts | Owner-selected 100/500/2,000 retained parents; 2,000 lifetime import receipts per enrollment |
| Attachment children | At most 256 per source; enumerate at most 10 descriptors and extract at most 2 attachments per parent |
| Text per parent/child | Existing 8 KiB UTF-8, with shortening/coverage labels |
| History page | At most 20 parent items and 50 provider requests |
| History/enrichment work per cycle | At most 20 seconds, 4 MiB downloaded bodies, 2 attachment downloads, shared single document worker |
| Single downloaded attachment/file | At most 2 MiB decoded/streamed bytes, regardless of provider size claims |
| Poll/read | Keep 120-second poll cadence and 90-second read ceiling; cap an owned cycle at 90 seconds across scheduled work |

Global admission includes actual serialized JSON size, not just text bytes. Never grow a source beyond the 16 MiB registry bound, discard seen history, or drop account-workflow receipts. If live state needs room, evict only expendable historical/attachment text with retained suppression receipts and truthful capacity status. If metadata alone cannot fit, stop that admission with existing state intact. Replace the current capacity advice to disable/re-enable with advice to reduce depth/retained content; reset is not a routine remedy.

Pause records progress and stops new historical I/O; live polling continues. Restart resumes only if authorization/settings still match. Expired page tokens cause a restart of the same bounded range, without resetting source identity or either dedupe ledger. Respect provider `Retry-After` through persisted `not_before_ms`; retries are bounded and wait for a later cycle.

Absence from a live head page does not prove historical deletion. Purge a retained parent/children on an explicit not-found/access-loss response, selection removal, expiry, source disablement, or revocation. Bounded historical revalidation revisits at most 20 parents per cycle after live work; an interrupted pass cannot claim all older items are current. Report per-item `verifiedAtMs` and source historical verification coverage. Do not keep serving a resource known to be denied.

Gmail/Outlook received time remains separate from modification time. Gmail `internalDate` can be based on a migrated mail's date, so it is not an observed-arrival event clock; this is documented in [the message resource](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages). Search ranking should use lexical relevance then provider date/modified time, with a stable citation tie-breaker, so today importing an old message does not make it appear newly written.

## Attachment and selected-file extraction

Discover attachment metadata only inside a currently authorized parent read. For Gmail, walk bounded MIME parts, distinguish named attachments from body alternatives, fetch `attachmentId` bodies when needed, and enforce encoded/decoded sizes before allocation. For Outlook, inspect attachment type and stream only `fileAttachment` bytes; do not follow `referenceAttachment` URLs or recursively expand item attachments.

Plain UTF-8 text/CSV use bounded decoding with an explicit malformed/unsupported result for unhandled encodings. Document formats use the existing `DocumentExtractor`/`DocumentWorker` with cancellation/deadline and output validation; do not create attachment-vault records, session messages, runs, or tool calls merely to extract background content. PDF is text layer only and Excel is stored-cell text only. Keep those omissions and locators even when the result is useful. On non-Windows systems, retain filename/metadata and return `unavailable` for office/PDF extraction.

SharePoint/OneDrive selected file content uses this same extraction adapter. Google Drive adds a selected-only binary byte-read method next to `read_for_search`; the original metadata/body used for file workflow detection is preserved independently of enriched text. Unsupported/encrypted/oversized/malformed/no-text/worker-busy/rate-limited resources retain bounded metadata plus a visible reason, never a false full-text claim. Worker busy/temporary failures can be retried in later cycles; unsupported content is not repeatedly downloaded without a changed provider revision or owner retry.

Raw bytes live only in bounded memory and are dropped after parsing/cancellation. No provider attachment is persisted into temp files or the session upload vault. The existing extraction worker's physical limits remain authoritative.

## Microsoft files connector

Create `connectors/microsoft_files/` following the existing selected Google Drive connector's lifecycle pattern. Its manager owns OAuth PKCE/state, separate zeroizing vault grants with granted scopes/account identity, refresh rotation, cleanup intents, selection validation, and reads. Reuse `OAuthAppService` for application configuration and the Microsoft lifecycle lock; do not reuse the Outlook account token or convert an Outlook connection into a file source.

Use two mutually exclusive connection modes: `oneDrive` and `sharePoint`. This prevents a broad OneDrive scope from accidentally broadening an approved-site connection. Records contain connector ID, agent ID, auth generation, app revision, account/tenant identity, mode, active/reauth state, selection revision, up to 16 selected `(driveId,itemId)` targets, and approved SharePoint site identity where applicable. Public responses contain no tokens or temporary download URLs.

OneDrive: owner can browse paged metadata under their own drive and select files. Folder navigation is bounded and is not recursive background indexing. SharePoint: owner supplies an approved site URL, Graph resolves/probes only that site, then lists its approved library metadata. If selected consent cannot resolve that site or list the library, show that it needs organization approval; never enumerate the tenant or request broader scope automatically. An admin-provided canonical site identifier may be accepted in setup tooling, but ordinary UI labels and flows use site names/URLs.

Treat site-resolution behavior under selected grants as an acceptance item to verify with transport fixtures and later optional owner-run tenant acceptance. Documentation establishes selected resource access, not the availability of every discovery endpoint. Unsupported discovery must fail truthfully rather than quietly widening consent.

The native `microsoft_files_read` tool accepts only a connector ID and an already selected opaque file key. It reads bounded retained/selected content, rechecking current permission. No arbitrary Graph URLs, paths, token input, account-wide search, writes, sharing, or permission management. A separate owner-only metadata browser is not available to the model.

Removing a selected file purges its parent/extraction content immediately and increments selection/settings revision to reject in-flight results; it does not rotate connector auth generation. Adding selected files imports a baseline only. New source providers are `onedrive` and `sharepoint`, both `DocumentKind::File`. Their source authority requires active matching connection, source opt-in, and `microsoft_files_read`; model search additionally requires `connected_search`.

File original links must be HTTPS and match verified OneDrive/Microsoft or the exact approved tenant SharePoint hostname; reject userinfo, ports, fragments that imply action, and deceptive suffixes. Temporary downloads use a provider-issued location captured from an authenticated Graph response, a separately validated public-network destination, bounded redirects/bytes, and no bearer header. If the destination cannot be validated safely, retain metadata and mark text unavailable.

## Owner API and SDK contracts

Keep all existing paths and `Cache-Control: no-store`. Owner guards run before parsing bodies or looking up private state. Unknown fields remain rejected. Additive fields are optional/defaulted for old state and clients.

`PUT /api/agents/{agent}/connected-search/sources/{connector}`:

```json
{
  "enabled": true,
  "expectedGeneration": "existing-authority-generation",
  "expectedSettingsRevision": 1,
  "settings": {
    "historyDepth": "days90",
    "retainedItemLimit": 500,
    "attachmentText": "supportedText",
    "historyPaused": false
  }
}
```

`settings` absent preserves the existing policy. Supplying settings requires `expectedSettingsRevision`; updates are full replacements, with identical retries recognized before checking the stale expected revision. Otherwise a stale revision is a 409. The same source generation and grant epoch survive successful settings changes. Respond with the source and settings revision only after durable save; queue work afterwards. Capability-incompatible settings return 400 `unsupported_setting` with a safe explanation, not a success that silently ignores the control.

`POST .../connected-search/refresh` adds optional `work: "live" | "history" | "all"`, default `live`. It queues bounded work and returns current source status; repeated refreshes coalesce under the gate. It does not unpause history, reset a cursor, or report completion before the work finishes. Existing `{connectorId?}` callers continue to work.

`GET .../connected-search/sources` and search responses retain existing fields and add:

- `settingsRevision`, `settings`, `capabilities` (`history`, supported attachment formats, `workflowEventKinds`).
- `historyCoverage`: requested range; state `notRequested|pending|running|paused|complete|partial|unsupported|error`; reason `itemLimit|byteLimit|receiptLimit|timeBudget|rateLimited|authorization|provider|unsupported`; enumerated/retained counts; oldest retained provider date; last history attempt/success/verification times; retry time. Counts are bounded actual observations, not provider estimates or account totals.
- `attachmentCoverage`: indexed, partial, omitted, failed, pending; per-source supported formats/availability.
- `retainedBytes`, limits, and a bounded `coverageSummary` suitable for model responses.

Existing `lastSuccessAtMs` continues to describe live refresh. It does not advance for a successful history page or extraction. History completion means that fixed requested inbox range was enumerated without a known gap; it does not mean all account content, all bytes, or all attachments are searchable. Missing metadata/unsupported extraction still appears in attachment coverage.

Hits/items add `contentRole`, `parentCitationId`, `providerReceivedAt`, `providerModifiedAt`, `verifiedAtMs`, extraction status/omissions, and bounded locators. Preserve existing `sourceModifiedAt` for compatibility, but use correctly labeled new fields in UI. Empty results say no match in retained content and include partial/unsupported source coverage. Model projections include quoted content and compact coverage and must still fit the existing 128 KiB bounded-result check; mark any omitted optional detail explicitly rather than dropping caveats.

New Microsoft endpoints:

| Method/path | Request / result |
| --- | --- |
| `GET /api/agents/{agent}/connectors/microsoft-files` | Redacted status, mode, selections, app-setup and consent availability |
| `POST /api/agents/{agent}/connectors/microsoft-files/connect` | `{mode, approvedSiteUrl?}` -> `{connectorId, consentUrl, expiresAtMs}`; pending single-use PKCE state |
| `GET /api/connectors/microsoft-files/callback` | Existing private static callback-page pattern; nonce/code consumed once, no reflection |
| `GET /api/agents/{agent}/connectors/microsoft-files/{id}/files` | Owner-only bounded folder/library metadata; opaque local continuation, no raw provider URLs |
| `PUT /api/agents/{agent}/connectors/microsoft-files/{id}/selection` | `{expectedGeneration, expectedSelectionRevision, fileKeys}` -> updated selection; maximum 16, current browsed/approved resources only |
| `DELETE /api/agents/{agent}/connectors/microsoft-files/{id}` | `{expectedGeneration}` -> disconnected status after durable purge/cleanup intent |

All connection mutations are owner-authorized. `connect` reuses a current pending flow for an identical agent/mode/site while valid, rather than creating unbounded pending records. Selection replacement and disconnect are idempotent for the same desired state and never accept resources outside the authorized mode/site. Errors distinguish 400 invalid/unsupported input, 403 owner required, 404 unavailable target, 409 stale generation/settings/selection, and 503 unconfigured/reauth/provider/persistence/capacity. Provider rate-limit retry timing lives in source status. Never return upstream error bodies, tokens, or denied-resource names.

SDK adds `MicrosoftFilesClient` in a separate file, plus additive connected-search types/settings in `connected-accounts.ts`. Do not fold file credentials into the mail API. UI uses existing owner-fetch and request-lifetime cancellation patterns.

## Persistence, restart, revocation, and migration

Keep the existing control-plane transaction as the atomic publication boundary. Snapshot state includes Microsoft connector metadata and history/settings/cursors/receipts; secrets remain exclusively in the connector's own vault namespace. Add defaulted fields and validate both legacy and new snapshots before publication. Do not bump or repurpose unrelated storage formats.

On legacy load, preserve every existing citation, base revision, seen entry, baseline, grant epoch, stream epoch, account rule and event receipt. Default history to Recent, attachment text to metadata, and settings revision to 1. Establish a conservative stable event fence in the first durable migration transaction. No background deeper read is caused by migration.

New parser/content metadata is optional and migrates without recalculating parent live revisions. Persistence failure rolls back content, settings, cursors, suppression receipts, tools, and any workflow changes together; provisional state is not observable. Restart derives pending work from durable state, rechecks permissions first, and resumes without source reenrollment. A missing file token causes reauthorization status, not Outlook-token fallback.

Disable/disconnect, native read-tool removal, agent deletion, auth generation change, revoked/reduced grants, and explicit provider access denial immediately remove searchable text/citations, queued extraction, cached status containing private titles, and provider cursors for the affected scope. Increment/cancel the in-memory work token so late responses cannot republish. Pause related existing account rules using current behavior; preserve their durable workflow receipts and already created runs. A deliberate new enrollment may establish a fresh baseline; depth/attachment settings never do.

Microsoft OAuth application replacement/deletion must count both Outlook and Microsoft-file active/pending/cleanup dependencies under the same lifecycle lock. Advertise the additional callback for Microsoft setup. No external credential/account operation is part of implementation testing.

## Proposed file ownership and integration touch points

The coordinator must assign ownership before edits. Feature-local ownership requested:

- Existing `hosts/rust-daemon/src/connected_search/{mod.rs,service.rs,providers.rs,tests.rs}`; new `policy.rs`, `ingestion.rs`, `backfill.rs`, `attachments.rs`, and focused `history_tests.rs`/`enrichment_tests.rs` under that directory.
- Existing `hosts/rust-daemon/src/connectors/mail/{search.rs,client_search.rs,client.rs}`; new `client_backfill.rs`, `client_attachments.rs` and corresponding fixture tests. Do not modify `mailbox.rs` or automatic-reply polling semantics.
- New `hosts/rust-daemon/src/connectors/microsoft_files/{mod.rs,model.rs,client.rs,authorization.rs,reads.rs,store.rs,tests.rs}`.
- Existing `hosts/rust-daemon/src/connectors/drive/{reads.rs,client.rs,model.rs}` only for explicitly selected bounded binary reads and their tests.
- Existing `hosts/rust-daemon/src/routes/connected_search.rs`; new `routes/microsoft_files.rs` and `routes/contracts/microsoft_files.rs`.
- Existing `hosts/rust-daemon/src/tools/connected_search.rs`; new `tools/microsoft_files.rs`.
- Existing `packages/sdk/src/{connected-accounts.ts,connected-accounts.spec.ts}`; new `microsoft-files.ts`/`microsoft-files.spec.ts`.
- Existing `apps/web/src/components/connected-accounts/{ConnectedAccountsPanel.tsx,ConnectedAccountsPanel.test.tsx,presentation.ts}`; new `SourceSearchSettings.tsx`/test and `MicrosoftFilesCard.tsx`/test. `AccountRuleEditor.tsx` needs a capability filter for Microsoft search-only sources.

Exact cross-feature/shared files requiring coordination:

1. `hosts/rust-daemon/src/state.rs`: new connector map/default/save/load/validation, purge on lifecycle changes.
2. `hosts/rust-daemon/src/control_plane_store.rs`: defaulted Microsoft file records and restoration fixtures.
3. `hosts/rust-daemon/src/routes/mod.rs`: connector initialization, app state, routes/OpenAPI registration, Microsoft OAuth dependency checks.
4. `hosts/rust-daemon/src/connectors/mod.rs`: new native module declaration.
5. `hosts/rust-daemon/src/connectors/oauth_apps.rs`: callback inventory and associated tests, reuse of Microsoft lifecycle.
6. `hosts/rust-daemon/src/tools.rs`: native read tool registration/context and manager wiring.
7. `hosts/rust-daemon/src/approvals/policy.rs`: classify bounded selected-file read consistently with existing `drive_read`.
8. `hosts/rust-daemon/src/account_workflows.rs` and `account_workflows/tests.rs`: reject non-live origins, preserve receipt semantics; no new Microsoft workflow kinds.
9. `packages/sdk/src/client.ts` and `packages/sdk/src/index.ts`: new client construction/exports only.
10. `apps/web/src/components/ConnectorsView.tsx` and `ConnectorsView.test.tsx`: Microsoft file card and setup description/callback status.
11. `hosts/rust-daemon/README.md` and `packages/sdk/README.md`: existing dirty files; coordinator applies narrowly scoped integration docs without overwriting prior changes.
12. `hosts/rust-daemon/src/state/run_commit.rs` and `hosts/rust-daemon/src/state/swarm_runtime.rs`: wire weak Microsoft-file manager handles into direct and swarm tool dispatch, matching current `with_drive` custody.
13. `hosts/rust-daemon/src/routes/contracts/mod.rs`: declare/re-export the Microsoft-file response contracts.

No new dependency is expected: reuse reqwest, serde, existing MIME/base64 handling, chrono, and the document worker. Do not modify Cargo/package manifests unless implementation uncovers an unavoidable, separately reviewed dependency. Never touch the existing results-panel/cache-tooling edits. Generated inventories, if required by existing tooling after release, remain coordinator-owned.

## Meaningful validation and acceptance

Use only synthetic accounts, mock transports/vaults, and loopback HTTP fixtures. No real account network reads, consent completion, credential inspection, or account writes.

- Legacy round-trip retains exact generations/citations/base revisions/seen/rule baselines/workflow receipts and schedules no deeper reads.
- Live baseline -> one new mail event -> history import -> extraction -> trim -> restart -> live reentry yields exactly one existing workflow receipt/run. History/enrichment do not change either live seen entries or workflow receipts. Test all these boundaries together, not only isolated enum branches.
- A new rule during active backfill waits for a fresh live baseline. A settings change during a blocked provider read rejects the obsolete page; disable/re-enable and permission removal reject obsolete content even when provider data arrives later.
- Cursor/documents/suppression receipts commit atomically; failed persistence/cancelled callers publish none of the provisional page. Expired cursor retry, duplicate pages, empty-with-next pages, loops, malformed pagination and final-page coverage are explicit cases.
- Gmail timestamp boundaries and Outlook filters stay inbox-only. Graph continuations/redirects cannot exfiltrate bearer tokens or target arbitrary hosts/private addresses. Existing Outlook IDs remain unchanged.
- Historical timestamp != observed time. Old imported mail moved into the live window and retained text evicted before reentry cannot fire workflows. Invalid provider timestamps are excluded conservatively.
- Initial attachment extraction and parser retries never change parent live revision. Parent changes/deletion or denied selection invalidate late extraction. Child citation reads require current parent permission and are purged with it.
- Office/PDF parser coverage, worker busy/unavailable, oversized streams, malformed MIME/base64, encrypted/scanned documents, and unsupported link/nested attachments produce truthful partial/unsupported metadata. No session attachments/runs or raw-byte temp files appear.
- Limits stop imports without evicting dedupe/receipt state; live work keeps priority. Poll/refresh storms coalesce. UTF-8 clipping and maximum model response serialization retain the 128 KiB tool bound and caveats.
- Microsoft file PKCE expiry/single use/cross-agent attempts, reduced scope refresh, token rotation/cleanup compensation, setup dependency locks, personal-vs-work capability, selected-site denial, per-file purge and restart are covered with fake providers. Tenant-specific discovery remains explicitly unverified until an owner later chooses live acceptance.
- UI/SDK tests cover old callers, settings concurrency, stale async responses, pause/resume, clear-history semantics, unsupported IMAP, extraction omissions, empty partial results, correct provider-vs-index dates, and exclusion of Microsoft files from workflow creation.

After coordinator release/build-slot allocation, run repository wrappers sequentially: `bun run rust:test`, `bun run rust:lint`, SDK test/build/typecheck, web test/build/typecheck. No concurrent Cargo commands; keep the shared managed validation target and 32 GiB ceiling. Root workspace checks/inventory generation are coordinator-owned. Clean only exact task-owned scratch outputs after recording results.

## Coordinator decisions and remaining uncertainties

Recommended scope is 365-day inbox backfill, opt-in supported attachment/file text, OneDrive own selected files, and SharePoint selected sites when externally approved. Unlimited history, IMAP history, arbitrary shared links, broad tenant discovery, OCR, and new Microsoft workflow triggers are intentionally excluded.

The product choice needing cross-feature review is SharePoint onboarding: this proposal preserves narrow consent and may require organization setup before browsing works. If the desired scope instead includes every site the owner can access, `Files.Read.All`/`Sites.Read.All` is a separate broader-consent decision and must not be silently substituted. Also confirm the suggested 2,000-message lifetime import-receipt bound/16 MiB overall cap are acceptable; exhaustion remains a visible partial state, not an automatic dedupe reset.

The first implementation milestone should deliver history/enrichment safety and Gmail/Outlook depth before Microsoft connector integration. Source/file ownership and Rust verification time are the only release dependencies; no human reapproval of the already requested feature is being requested here.
