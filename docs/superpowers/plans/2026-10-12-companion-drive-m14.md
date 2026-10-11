# Anima selected Google Drive M14 implementation plan

> For agentic workers: use superpowers:subagent-driven-development or superpowers:executing-plans task by task. This lean plan freezes interfaces, meaningful tests and ownership; controller rulings supersede draft choices. Checkboxes track implementation, not preparation. Root publishes and activates this plan only after M13 merges and cuts its M14 branch from updated main.

**Goal:** Let the owner connect one Google account to an agent, select 1–16 exact Drive file IDs, and let that agent search selected filenames and read bounded Google Docs/plain UTF-8 text with faithful provenance.

**Architecture:** Add an on-demand native Drive manager/client inside the existing Rust daemon. Reuse the existing Google OAuth application service, mail PKCE and generation/transaction patterns, and private zeroizing Google token value. Selection and agent tool grants are host authority. Drive is a tool service rather than a polling chat transport; leave working Gmail/Calendar and the M12/M13 messaging registry intact.

**Tech stack:** Existing Rust axum/reqwest/tokio/serde/base64/sha2/uuid/zeroize/keyring; existing SDK/React web; Bun/Nx. No dependency, feature, package, service or database additions.

**Activation:** M12 is merged at `f7a58084bc6a36273f528882463b4f516ef96bb1`; M13 is in progress on `g9000/companion-discord-m13`. This draft deliberately makes no claim about its final merge SHA or validation. Root activates `g9000/companion-drive-m14` from merged M13 main, updates base/source facts, freezes five risks, then dispatches bounded workers.

## Read first and current evidence

- `AGENTS.md`.
- Start-here section of `docs/superpowers/handover/2026-09-28-companion-console.md`.
- `docs/superpowers/plans/2026-10-12-companion-connectors-m12.md`.
- `docs/superpowers/plans/2026-10-12-companion-discord-m13.md` and its final controller rulings.
- `.superpowers/sdd/2026-10-12-companion-discord-m13/progress.md` and `usable-target.md`.
- Existing Google services: `hosts/rust-daemon/src/connectors/oauth_apps.rs`, `connectors/mail/{mod,client}.rs`, `connectors/gcalendar/{mod,client,store}.rs`.
- API/tool seams: `routes/{mod,mail,gcalendar}.rs`, `routes/contracts/{mod,gcalendar}.rs`, `tools.rs`, `tools/{mail,calendar}.rs`, `state/{run_commit,swarm_runtime}.rs`.
- SDK/UI seams: `packages/sdk/src/connectors.ts`, `apps/web/src/components/ConnectorsView.tsx`, `apps/web/src/hooks/useAgentIntegrations.ts`.

Existing facts were inspected narrowly on 2026-10-12:

- `OAuthAppService::locked_config` returns a provider lifecycle lease with resolved configuration and revision. Google update/remove guards currently consult mail and calendar dependencies; add Drive to both guards.
- `ResolvedOAuthApp::callback_uri` owns callback origin. Google redirect-URI status currently contains Gmail and Calendar; append Drive without changing those entries.
- Mail already uses S256 PKCE, a zeroizing verifier in an in-memory pending map, expiry, configuration revision, auth generation, per-agent locks and control-plane transaction rollback.
- Calendar's `GoogleOAuthTokens` is zeroizing and redacted. `GoogleCredentialStore` is keyed only by connector ID; do not pretend it already fences generations or scoped Drive credentials.
- Mail and calendar add/remove only their tool names using `tool_registry.resolve_descriptors`. Preserve all other agent tools and policy grants.
- `ToolExecutionContext` carries mail/calendar handles; run and swarm paths construct it independently. Both paths need Drive integration.
- M13 snapshot destination is version 12 with a pre-Discord backup. Wait for its final source; persistent Drive fields require version 13, a pre-Drive backup and future-version rejection.
- Drive support is absent. No actual account, credentials, owner snapshots, user profiles, configuration or provider connection was read during preparation.

## Actual upstream reuse and license

Source-only reference: `C:/Users/leoca/AppData/Local/hermes/hermes-agent` at controller-verified revision `f3ef9db68332063a3625661f3a9b259f60abdb4e`, MIT, Copyright (c) 2025 Nous Research.

Actual Google Drive code exists in `skills/productivity/google-workspace/scripts/google_api.py`:

| Upstream symbol  | Rust adaptation                                                                | Deliberate limit                                                                                                        |
| ---------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| `drive_get`      | `connectors/drive/client.rs`: one-ID metadata GET and selected response fields | No owners/parents metadata or arbitrary user-supplied URLs                                                              |
| `drive_download` | Metadata-first branching between Google-native export and stored-file download | Google Docs export `text/plain`; stored `text/plain` UTF-8 only; bounded in-memory read rather than local file creation |
| `drive_search`   | Selected metadata result shape: ID, name, MIME, modification, source link      | Do not copy global `files.list`, raw-query support or unsafely interpolated `fullText` query                            |

`drive_download` upstream defaults Docs to PDF, Sheets to CSV, Slides to PDF and Drawings to PNG. M14 intentionally supports a smaller readable subset and does not copy that map wholesale. Its Python client, optional `gws` subprocess, setup, token JSON files and broad `drive` scope are not imported or executed.

`tools/drive_preview_tool.py` is desktop/browser control, not Google Drive. `feishu_drive_tool.py` is another provider. Neither is credited as this implementation's source.

Create `connectors/drive/HERMES-NOTICE` with full MIT license/copyright, revision, exact source path/symbols and behavior/code adaptation description. Module headers reference it. Preserve other upstream notices. Product names and new public symbols remain Anima/AnimaOS; claim selected behavior adaptation, not full upstream parity.

## Primary protocol verification

Official Google pages read on 2026-10-12:

- [Drive scopes](https://developers.google.com/workspace/drive/api/guides/api-specific-auth): `drive.readonly` grants provider-wide view/download and is restricted. `drive.file` is narrower but manual arbitrary IDs do not establish a Picker/open-with grant.
- [files.get](https://developers.google.com/workspace/drive/api/reference/rest/v3/files/get): metadata and stored-file content by exact ID; `alt=media` does not export Google Docs.
- [files.export](https://developers.google.com/workspace/drive/api/reference/rest/v3/files/export): requested export MIME, provider export ceiling 10 MB. Anima imposes a smaller limit.
- [Export formats](https://developers.google.com/workspace/drive/api/guides/ref-export-formats): Google Documents can export `text/plain`.
- [Download guide](https://developers.google.com/workspace/drive/api/guides/manage-downloads): inspect `capabilities.canDownload`; export/download can still be denied.
- [Search terms](https://developers.google.com/workspace/drive/api/guides/ref-search-terms): no general file-ID membership query term. Do not invent `id in (...)` or enumerate an account before filtering locally.
- [about.get](https://developers.google.com/workspace/drive/api/reference/rest/v3/about/get) and [user info](https://developers.google.com/workspace/drive/api/guides/user-info): explicit field selection can return authenticated user identity; `permissionId` is opaque and email may be absent.
- [OAuth web server](https://developers.google.com/identity/protocols/oauth2/web-server): offline refresh, returned scopes, state validation; provider revocation removes the project's Google grants, affecting existing Gmail/Calendar.
- [PKCE](https://developers.google.com/identity/protocols/oauth2/native-app): 43–128-character verifier, S256 challenge with unpadded base64url SHA-256. M14 uses the already-present mail generation pattern.

These sources justify protocol shape, not live account readiness. Do not copy official sample dependencies or storage mechanisms.

## Global execution constraints

- Preparation edits only this ignored draft. No source, Git operations, Cargo/Nx tasks, services, token reads or account setup until root activates M14.
- Implementation adds no dependencies or dependency features, services, containers, database/profile changes, keyring reads or live provider calls during validation. No port 55432 or Docker actions.
- Preserve unrelated `nx.json`/`anima.yaml`; no staging them, stash/reset/restore/worktree/checkout-path.
- Root owns branch/compilation/formatting/gates/explicit staging/unsigned-per-command commits/push/PR attachment/merge. Workers do not run Cargo or Git.
- Cargo remains serialized, `CARGO_INCREMENTAL=0`, existing target directory. Use fakes/loopback HTTP, not owner stores.
- No preflight audit: bounded workers, one daemon review, final SDK/web review, one combined fix wave, required local gates.
- Working Gmail/Calendar stay separate native services. No registry-parity rewrite, universal JSON authority envelope, polling Drive, Python runtime, installer or broader provider expansion.
- No Drive writes, delete/trash/share, permissions editing, whole-account search, synchronization, automatic ingestion, document engine, Picker or remote token revocation.

## Five risks for controller binding

1. **Broader provider consent vs selected authority:** request `drive.readonly`, disclose provider-wide read/download; Anima enforces an immutable 1–16-ID allowlist in its native manager. Cost: stronger provider enforcement requires a later Picker/`drive.file` design. Browser/tool IDs never expand the allowlist; discovery is exact-ID GET only.
2. **Disconnect/refresh isolation:** Drive-only disconnect durably revokes local authority and cleans its private generation keys; never call Google's project-wide revoke. Reuse the Google application lease and mail PKCE/transaction shape, not shared live mail/calendar token records. Cost: Google account-wide revoke is a separate owner action, with Gmail/Calendar impact explicitly stated.
3. **Captured authority and stale races:** capture exact agent/connector/generation/selected IDs/account binding, revalidate after network/vault awaits and before returning text. No stale refresh may resurrect a disconnected generation. Cost: concurrent disconnect/reauth yields a static conflict and discards fetched content; a provider read already sent cannot be undone.
4. **Formats, bounds and provenance:** metadata-first supported MIME check, no shortcut following/export links; stream cap/time cap, strict UTF-8, limited returned context, provenance over exact fetched and returned bytes. Cost: oversized, changed or unsupported files return explicit results without content rather than pretend to parse PDF/Sheets/binary.
5. **Persistence/restart and grants:** version 13 only because new records/cleanup intents persist, pre-v12-Drive backup, future-version rejection, atomic records/tool grants rollback. Pending PKCE remains private/in-memory, so restart requires a new consent attempt. Cost: old binaries use the pre-Drive backup; no owner snapshot migration during tests.

Controller already binds local disconnect only and explicit Google account-wide revocation impact. Remaining detailed choices below are the proposed freeze for publication.

## Frozen model and private interfaces

Create `hosts/rust-daemon/src/connectors/drive/{mod,model,client,store,tests}.rs` plus `HERMES-NOTICE`. Split manager/tests if needed; no new crate.

```text
DRIVE_TOOL_NAMES = ["drive_search", "drive_read"]
DRIVE_SCOPE = "https://www.googleapis.com/auth/drive.readonly"
DriveFileId: nonempty ASCII [A-Za-z0-9_-], <=256 bytes; never path/URL/query.
Selected IDs: 1..=16; duplicates rejected; durable canonical sorted list.
Query: trimmed 0..=256 Unicode scalars; empty lists selected metadata.

DriveStatus = Pairing | Active | ReauthRequired | Disconnected
DriveRecord {
  id, agent_id, status, generation: String,
  selected_file_ids: Vec<DriveFileId>,
  account_permission_id: Option<String>, account_label: Option<String>,
  oauth_config_revision: u64,
  created_at_ms, updated_at_ms,
  cleanup_generations: Vec<String>  // bounded private credential deletion intents
}
DrivePending { connector_id, agent_id, generation, selected_ids_digest,
  oauth_config_revision, expires_at_ms, verifier: Zeroizing<String> }
  // In-memory only, indexed by opaque state nonce; no verifier/nonce in public APIs.
DriveAccount { permission_id: String, label: Option<String> }
DriveFileMetadata { id, name, mime_type, version: String,
  modified_time: Option<String>, size_bytes: Option<u64>, trashed: bool,
  can_download: bool, source_url: String }
DriveFileAvailability = Available | Unsupported | Unavailable
DriveSearchResult { files: Vec<DriveSearchItem>, searched_selected_count,
  search_scope: "selected_names", fetched_at_ms }
DriveReadResult = Text(DriveTextResult) | Unsupported{metadata, reason}
DriveTextResult { metadata, text, representation: "google_docs_plain_text"|"stored_plain_text",
  source_bytes, returned_bytes, returned_scalars, truncated,
  source_bytes_sha256, returned_text_sha256, fetched_at_ms }
DriveError: InvalidInput | NotFound | NotConnected | ReauthRequired | Unconfigured |
  NotSelected | Conflict | PermissionDenied | RateLimited | Transport |
  InvalidResponse | TooLarge | InvalidUtf8 | Deadline | Credential | Persistence
  // Static Display/Debug, never upstream body/request/token details.
```

Public status exposes selected IDs and account label to the owner, not credential generations, account permission ID, cleanup intents or private OAuth state. Metadata version is provider metadata, not an atomic revision guarantee.

```text
#[async_trait] trait DriveTransport: Send + Sync {
  exchange_code(config:&DriveOAuthConfig, code:&str, verifier:&str)
    -> Result<DriveTokenGrant, DriveError>
  refresh_tokens(config:&DriveOAuthConfig, refresh_token:&str)
    -> Result<DriveTokenGrant, DriveError>
  identify(access_token:&str) -> Result<DriveAccount, DriveError>
  metadata(access_token:&str, id:&DriveFileId) -> Result<DriveFileMetadata, DriveError>
  read_text(access_token:&str, metadata:&DriveFileMetadata)
    -> Result<DriveTextBytes, DriveError>
}
DriveTokenGrant { tokens: GoogleOAuthTokens, granted_scopes: Vec<String> }
DriveTextBytes { bytes: Vec<u8>, representation }  // <=1 MiB, validated UTF-8 later

#[async_trait] trait DriveCredentialStore: Send + Sync {
  load(connector_id:&str, generation:&str) -> Result<Option<DriveTokenGrant>, DriveError>
  put(connector_id:&str, generation:&str, grant:DriveTokenGrant) -> Result<(), DriveError>
  delete(connector_id:&str, generation:&str) -> Result<(), DriveError>
}
DriveManager {
  inner: Arc<DriveManagerInner>
}
DriveManagerInner {
  state: Weak<RwLock<DaemonState>>, transactions: Arc<Mutex<()>>,
  oauth_apps: OAuthAppService, credentials: Arc<dyn DriveCredentialStore>,
  transport: Arc<dyn DriveTransport>, pending: Mutex<HashMap<String, DrivePending>>,
  per_agent_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>
}
status(agent_id) -> Result<DriveStatusEnvelope, DriveError>
begin_connect(agent_id, selected_ids) -> Result<DriveConnectEnvelope, DriveError>
complete_connect(state_nonce, code) -> Result<DriveConnectorResponse, DriveError>
disconnect(agent_id, connector_id) -> Result<(), DriveError>
search_for_agent(agent_id, query) -> Result<DriveSearchResult, DriveError>
read_for_agent(agent_id, file_id) -> Result<DriveReadResult, DriveError>
has_provider_dependency(OAuthProvider) -> bool
```

The manager retains only weak daemon state and the shared transaction gate, not a strong `AgentRunCoordinator`. `DaemonRuntime`/`AppState` own it. A weak manager handle in `DaemonState` supplies upgraded handles to run/swarm tool contexts without the M13 ownership-cycle pitfall. Registry discovery and manager construction have no network/vault effects.

### Credential and OAuth lifetime

- New Drive vault service `animaos.connector.drive`; generation-specific validated accounts. Reuse `GoogleOAuthTokens` value, redaction and keyring error discipline; leave the existing gcalendar service/records untouched.
- Store payload version 1 includes tokens/scopes and private binding; tokens/verifier remain zeroizing, never serde snapshot/public response/debug dumps.
- Connect obtains Google app lease, per-agent gate, validates IDs and agent, makes a new generation, and durably writes Pairing before publishing consent URL. New selection replaces no active list: different IDs require disconnect then fresh connection.
- Reauth may reuse connector ID only for exactly equal selected IDs and pinned account. It rotates generation, blocks reads while Pairing and queues old-generation cleanup durably.
- PKCE verifier is two random UUID v4 simple strings as mail uses (64 chars); nonce independent random UUID. Pending TTL 10 minutes, maximum 1 live attempt per agent, bounded pending map/expiry cleanup.
- Consent uses the host-owned callback, S256, offline, prompt=consent and exactly Drive readonly request; do not request incremental scope union. Verify required readonly grant in token response; no silent missing-scope acceptance.
- Callback has no owner browser header: authorize through single-use host pending state, expiry, captured revision, generation, agent existence and selection digest. Consume pending state before exchange. Invalid/error callback consumes the matching pending attempt without echoing code/state/provider errors.
- Hold Google application lease through exchange/identity/activation. Identify with `about.get?fields=user(permissionId,emailAddress,displayName,me)`; require me=true and a bounded nonempty opaque permission ID; email/display are labels only.
- Validate selected file metadata by exact ID before activation; inaccessible IDs fail the attempt. Unsupported formats may be selected but are labelled unsupported. No folder/shortcut resolution.
- Vault put precedes activation persistence; authority is granted only after persisted Active and tool grants. On failure roll back state/tools and compensate or queue newly written key cleanup; never leave an active in-memory record after save failure.
- On refresh keep the app lease/per-agent generation gate; preserve prior refresh token when Google's refresh response omits a replacement. Required scope validation on exchange; refresh absent scope inherits the stored validated grant, explicit reduced scope fails closed.
- Retain app configuration revision and pinned account across reauth; no stale generation/configuration refresh writeback. Network awaits never hold `DaemonState` write locks or the global persistence transaction gate.
- Disconnect first durably marks Disconnected, invalidates pending/grants/generation and queues private generation cleanup; only then deletes keys. Failed key deletion reports cleanup pending in a static owner status; local authority stays revoked.
- Cleanup is bounded and on demand/at existing boot initialization, no new poller/service. Tombstones retain outstanding cleanup intents; cap intents per connector (16), block repeated reauth if cleanup backlog is full.
- Agent deletion invalidates Drive records/grants and queues cleanup under the existing control-plane gate. Google app remove/update remains blocked while a non-disconnected Drive connection or cleanup dependency exists, using the existing dependency guard path.
- Restart restores Active metadata but never trusts Pairing as active; lost private pending verifier yields ReauthRequired. Missing vault generation yields ReauthRequired on tool access. No eager credential reads in passive status/catalog.

### Native read/search authority and limits

- Tool manager resolves from current `AgentState.id`; tool arguments never accept agent ID, connector ID, OAuth configuration, raw query, scope, export MIME, URL or destination path.
- Confirm exact tool name is currently granted using existing run/delegation permission policy; enabled connection alone is insufficient.
- Capture agent/connector/generation/account/selection before vault or network access. Reject unselected read ID before loading a token or sending any HTTP request.
- Search enumerates only the captured 1–16 selected IDs using `files.get`; sequential calls, no account `files.list`, `q`, folder traversal or shortcuts. Case-insensitive literal substring on selected current file names, not full-text content.
- Search inaccessible/trashed IDs return minimal selected-ID unavailable entries; do not fall back to cached content/global search. Other upstream failures return static operation error. Results and counts are bounded to the selected list.
- Metadata request fields: `id,name,mimeType,version,modifiedTime,size,trashed,capabilities(canDownload)`. Provider ID must equal requested ID. Source URL is host-constructed `https://drive.google.com/file/d/{validatedId}/view`; never fetch/return arbitrary provider export/thumbnail/download URLs.
- Metadata 64 KiB/request; bounded names (1024 scalars), MIME/time/version/size; require nonempty decimal version and coherent fields. No permission/owner/email/parent enumeration.
- Read supports only `application/vnd.google-apps.document` via `files/{id}/export?mimeType=text/plain`, or metadata `text/plain` via `files/{id}?alt=media`.
- Other formats including PDF, Office, Sheets, Slides, folders, shortcuts and binary return Unsupported metadata without content fetch. `canDownload=false` or trashed is unavailable/denied; never acknowledge abuse.
- Fixed HTTPS Google API/token/consent hosts only; percent-encoded IDs/query pairs. HTTP redirects disabled. Test-only loopback base injectable privately, never through owner input.
- Existing reqwest stream feature supports capped chunk reads. Content-length precheck is advisory; enforce 1 MiB on streamed bytes including unknown/chunked lengths. Abort cap+1, do not buffer huge `.bytes()`/`.text()` then check.
- Connect timeout 5 s, individual request 10 s; whole search 20 s, read 20 s and callback 45 s. No unbounded retry/rate-limit sleeps or spawning parallel per-ID requests; rate-limited results are static actionable errors.
- Strict UTF-8; reject undecodable bytes, no lossy text/binary pretending. Return first 32,768 Unicode scalars with explicit truncation, maximum bounded serialized tool result 256 KiB. Search tool result <=32 KiB.
- For successful text compute SHA-256 of complete bounded downloaded/exported bytes and separately exact returned UTF-8 text. Digests are not Google revision IDs. Do not claim a full digest when a transfer was aborted.
- Fetch metadata again after content; if ID/version/modifiedTime/MIME changed, return Conflict and discard content. This detects observed races; it is not an atomic Google revision read. No automatic retry.
- Revalidate captured native binding and current tool grant before returning search/text; disconnect/deletion/reauth revokes fetched content. Cancellation/deadline drops stream and content.
- Text and metadata are untrusted evidence, never agent instructions. No auto memory write, index, embedding, file export, schedule mutation or retained content cache. Ordinary bounded tool output/history follows existing retention rules.

## Owner API / SDK contract

Use `gdrive` as public connector type/path consistently; tool names remain `drive_search`/`drive_read`.

| Method/path                                                      | Input                                    | Response/authority                                                                         |
| ---------------------------------------------------------------- | ---------------------------------------- | ------------------------------------------------------------------------------------------ | ------------------------ |
| GET `/api/agents/{agent_id}/connectors/gdrive`                   | none                                     | owner-read; `{configured, connector:null                                                   | DriveConnectorResponse}` |
| POST same path                                                   | strict JSON `{selectedFileIds:string[]}` | owner-mutation; 201 `{connector, consentUrl}`; immutable selection                         |
| GET `/api/connectors/gdrive/callback`                            | `state`, `code` or provider `error`      | single-use private pending authority; static HTML outcome, no reflected input              |
| DELETE `/api/agents/{agent_id}/connectors/gdrive/{connector_id}` | none                                     | owner-mutation; `{deleted:true}` after durable local revoke, key cleanup may remain queued |

No public content-read/global search endpoint is necessary: tools are agent-bound; owner card can inspect status and selected IDs. No provider revoke route.

```text
DriveConnectorResponse {
  id, agentId, type:"gdrive", status:"pairing"|"active"|"reauthRequired"|"disconnected",
  accountLabel:string|null, selectedFileIds:string[],
  requestedScope:"https://www.googleapis.com/auth/drive.readonly",
  enforcement:"animaSelectedFiles", cleanupPending:boolean,
  createdAtMs:number, updatedAtMs:number
}
ConnectDriveInput { selectedFileIds:string[] }
DriveStatus { configured:boolean, connector:DriveConnector|null }
DriveConnectResult { connector:DriveConnector, consentUrl:string }
SDK ConnectorsApi:
  driveStatus(agentId):Promise<DriveStatus>
  connectDrive(agentId,input):Promise<DriveConnectResult>
  disconnectDrive(agentId,connectorId):Promise<void>
```

- Guard all owner routes before state/vault/network access: GET `authorize_read`, mutations `authorize`; callback solely bound state/PKCE flow. Do not copy Calendar routes' missing read guards.
- Every response/error/HTML `Cache-Control:no-store`; callback static CSP-safe page with no script/links containing provider code. Bound query/code/state lengths and reject duplicate conflicting query keys.
- Body limit 16 KiB; `deny_unknown_fields`; no external URL fetch or trusting browser-selected metadata. Status missing agent 404; invalid input 400, forbidden 403, conflict/reauth 409, vault/persistence/provider 503.
- Add utoipa operation/schema registration to existing route contract collection and inventory tests; root derives final operation count from merged M13 source, adds four operations.
- Append exact Drive callback to Google OAuth app redirect-URI list and configured-origin tests. Existing Google settings card shows it; no new credentials UI or provider registration.

## SDK/web setup

- SDK adds typed Drive contracts/methods to `packages/sdk/src/connectors.ts` and exports via `index.ts`; URL-encode each path segment through the existing helper.
- Add `apps/web/src/hooks/useDriveIntegrations.ts` with stale agent-generation/request protection, immediate data clearing on agent switch, bounded explicit refresh and serialized mutations; no endless status polling.
- Add a `GoogleDriveCard.tsx` imported by `ConnectorsView.tsx`; small native card avoids rewriting its mail/calendar/Discord flows.
- Owner enters exact IDs, one per line (1–16), before consent. Prefer ID-only accepted input with explanatory example; no arbitrary link fetch or Picker illusion.
- Setup copy: “Google grants read/download access across your Drive. Anima limits this agent to the selected file IDs. Search checks those file names. Google Docs and plain text can be read; other formats return metadata.”
- Pairing card retains immutable selected IDs and explicit “Open Google consent”/“Refresh status” action. Reauth requests same selection; different selection requires local disconnect and new connect.
- Active card shows account label, selected count/IDs, read-only/content limits and “Disconnect Drive” action. Disconnected/cleanup pending never says provider permission revoked.
- Add explicit Google account app-access link with copy that account-wide revocation can also disconnect Gmail and Calendar. Do not invoke remote revoke from the link/button.
- Consent URL must pass existing safe link discipline (HTTPS Google consent host); links use `noopener noreferrer`, text rendered as React text, no HTML injection. User IDs never become tool grants directly.
- Connection completion adds only the two native Drive tool descriptors; disconnect removes only those. General permissions/approvals UI uses its existing discovered names and existing policy semantics.
- Unsupported connector route in older daemons gets a static upgrade-needed/unavailable state; do not call a different provider or lose working card data.

## Worker ownership and implementation tasks

Worker A: native models/client/store, source provenance. Worker B: manager/state/snapshot/routes/tools/runtime integration. Worker C: SDK/web/tests. Root: freeze interfaces, documentation, reviews/gates/publication. A owns new native transport/store/model files; B consumes those interfaces, owns `drive/mod.rs` manager. No shared file edits between workers.

### A — Native typed client, generation vault and source adaptation

Files: create `connectors/drive/{model,client,store}.rs`, corresponding focused tests, `HERMES-NOTICE`. B registers module/tests. Do not edit manifests, gcalendar token serialization or host registry.

- [ ] Write meaningful fake HTTP/store regressions first; provide exact focused test names to root for serialized red/green.
- [ ] Implement model bounds/typed static errors, required scope/account identity and deterministic source URLs.
- [ ] Adapt upstream `drive_get` metadata projection and `drive_download` metadata-first export/media branch; exact attribution.
- [ ] Implement fixed-host no-redirect client, bounded JSON/stream, per-request deadlines, supported MIME and no external links.
- [ ] Implement generation-specific private keyring/in-memory store with zeroizing token/scopes payload and static errors; no actual vault calls in fixtures.
- [ ] Verify fake tests: path-ID rejection, wrong metadata ID, oversized/chunked response, invalid UTF-8, redirect rejected, unsupported format no content request, Docs text export, stored-text media, denied download, scope missing/reduced, refresh token retention, account missing/mismatch and secret redaction.

### B — Native authority, persistence, owner routes and tools

Files: create `connectors/drive/mod.rs`, `routes/gdrive.rs`, `routes/contracts/gdrive.rs`, `tools/drive.rs`, manager/route regressions. Modify `connectors/mod.rs`, `app.rs`, `state.rs`, `state/{run_commit,swarm_runtime}.rs`, `control_plane_store.rs`, `routes/{mod,contracts/mod}.rs`, `routes/tests/{docs_contracts,...}.rs`, `tools.rs`, affected existing test fixtures.

- [ ] Freeze consumption of A interfaces with root before source edits.
- [ ] Implement manager with weak ownership, per-agent locks, private pending PKCE, app lifecycle lease and atomic persistence rollback.
- [ ] Implement immutable selection, generation-aware credentials, callback single-use/expiry/config/account checks, local revoke then cleanup; no global provider revoke.
- [ ] Wire native manager to runtime/AppState and weak state handle; add Drive dependency checks to both Google app update/remove flows.
- [ ] Add persisted Drive records version 13/serde defaults, validation, pre-Drive backup for v12 and future-version rejection. Preserve M13 Discord/Telegram fields.
- [ ] Wire `drive_search`/`drive_read` descriptors, tool grants and current-agent dispatch through both run and swarm contexts; integrate existing delegation permission checks.
- [ ] Add four guarded routes/contracts plus Drive callback status; callback authorizes state/PKCE rather than owner headers.
- [ ] Meaningful manager tests: unselected read before token/network, other-agent denial, removed tool denial, literal selected-name search/no files.list, stale callback, callback replay, reconnect exact-selection conflict, wrong account, provider revision changed, refreshed generation disconnected during await, fetched content revoked before return.
- [ ] Failure tests: Pairing save rollback/no consent link, Active save failure/no leaked grant, credential put failure, local disconnect save failure remains reviewable, vault cleanup failure leaves revoked tombstone, agent deletion, revoked/missing key, pending restart invalidation, version12 defaults/backup/version14 refusal, secret-free snapshots.
- [ ] Tool result tests: full/returned-byte provenance digests, scalar truncation, post-read changed metadata discarded, unsupported binary only metadata, cancellation/timeouts, untrusted contents never auto memory/action.
- [ ] Route tests: all owner guards before state/transport, no-store on all statuses/callback, strict/bounded body/query, owner agent/connector binding, callback generation/replay, static errors and OpenAPI operation/schema registration.

### C — SDK and native web card

Files: `packages/sdk/src/{connectors,connectors.spec,index}.ts`; create `apps/web/src/hooks/useDriveIntegrations.ts`/test and `components/GoogleDriveCard.tsx`/test; modify `components/ConnectorsView.tsx`/test; minimal daemon API re-export if the existing SDK surface requires it.

- [ ] Consume frozen public contracts only; source from SDK, no browser-created grants or provider tokens.
- [ ] Add encoded Drive status/connect/disconnect methods and tests for exact input/envelopes/errors/paths.
- [ ] Implement stale-response-safe hook and card with selected IDs, consent disclosure, supported-format/search limits, reconnect/disconnect and provider-wide revocation warning/link.
- [ ] Verify setup ID bounds/duplicates, no empty selection, disabled busy actions, pairing to Active explicit refresh, missing Google app setup, unsupported daemon, reauth, cleanup pending and safe consent URLs.
- [ ] Verify rapid agent switch/mutation response cannot leak prior agent IDs/status/account; unmount does not mutate data; other connector cards preserve working flows.

## Controller review, validation and publication

- [ ] Root rechecks final M13 source/version/controller seams after merge; publishes this plan and five binding risks before implementation.
- [ ] Root runs focused red/green checks requested by workers; no new target directory or parallel Rust compilation.
- [ ] One daemon batch review plus final SDK/web review; one combined fix wave only unless new meaningful failure warrants another.
- [ ] Root uses already-resolved project names `rust-daemon`, `@animaOS-SWARM/sdk`, `@animaOS-SWARM/web`; refresh project metadata with read-only Nx command if merged M13 changes target contracts.
- [ ] Required current-tree Rust gate in PowerShell: `$env:CARGO_INCREMENTAL='0'; $env:CI='1'; bun x nx run rust-daemon:test --skipNxCache`. Expected all required tests pass; ignored live/Postgres tests remain reported separately.
- [ ] Rust formatting gate: `bun x nx run rust-daemon:lint --skipNxCache` with same serialized environment and existing target directory.
- [ ] SDK/web: `bun x nx run-many --targets=test,typecheck,build --projects=@animaOS-SWARM/sdk,@animaOS-SWARM/web --skipNxCache` with `CI=1`; use bounded tests, no daemon/service launch. Root checks flags if final workspace target setup differs.
- [ ] No workspace-dev/Astro gate unless those sources are actually changed; no launcher change is in this scope. Markdown consistency/formatting only for owned docs.
- [ ] Root updates daemon README, existing Google OAuth setup docs, connector contribution guidance and handover for selected Drive scope, provider consent/setup/API enablement, unsupported formats, local disconnect and v13/pre-Drive backup. No hosted/live readiness claim.
- [ ] Verify no manifest/lock/dependency features changed; preserve unrelated paths and exact staging list. Root explicitly commits/pushes/creates and attaches PR, merges verified head under standing authorization, updates main/handover, and resumes final selected daily-flow readiness assessment.

## Completion evidence and limits

Local milestone acceptance: fixture owner can configure Google app, connect selected IDs via PKCE, activate native agent tools, search only selected names, read permitted Docs/plain text with bounded provenance, reject unselected IDs, disconnect locally and recover persisted metadata safely. Existing Gmail/Calendar/Telegram/Discord and full required gates remain passing.

Live acceptance is separate: owner Google app/client registration and Drive API enablement, callback allowlist, provider consent/restricted-scope eligibility, actual account/file access, refresh lifetime and provider-connected model tool use. No live account/token/services are accessed merely to close this implementation milestone.

M14 does not establish an always-on hosted assistant, full upstream parity, PDF/Sheets parsing, whole-Drive content search, provider-enforced per-file OAuth permissions or automatic memory ingestion. Those are truthful boundaries, not hidden setup failures.

## Controller activation and rulings (binding, 2026-10-12)

PR #19 is merged at `7f42df8334af7e0afdb71f83a5d52aeaa48e6d67`, source `0a5c74c`.
M0-M13 are on main. Active `g9000/companion-drive-m14` was cut from that updated
main; the prepared source/protocol plan above is now executable. Latest owner
standing merge-and-continue instruction remains in force; no approval check-ins.

1. Ruling: Request provider drive.readonly with explicit broad-consent disclosure, enforce immutable selected1-16 file IDs in the native manager — no Picker/new dependency exists, and manual IDs cannot fabricate a provider drive.file grant — cost if wrong: Anima enforces selection while Google authorization is broader; no whole-account enumeration or provider-enforced per-file claim.
2. Ruling: Drive disconnect revokes local agent authority and generation-specific keys only; never call remote project-wide Google revoke — that could invalidate existing Gmail/Calendar tokens — cost if wrong: owner-wide Google account revocation remains a clearly labeled external action; queued private key cleanup stays visible and cannot resurrect access.
3. Ruling: Reuse existing mail PKCE, Google app lease and transaction/generation patterns, with separate private Drive tokens and weak daemon-state references — stale callbacks/refreshes cannot reconnect a revoked selection or form an ownership cycle — cost if wrong: races return static conflicts/discard fetched content and require fresh consent; a read already sent cannot be undone.
4. Ruling: Selected-name search uses exact metadata GETs, Docs/plaintext reads are streamed/time-bounded and return faithful provenance — the owner wants small useful integration and no new document dependencies — cost if wrong: PDF/Sheets/Slides/binary/shortcuts return explicit unsupported metadata; broad search/content ingestion/cache remains absent.
5. Ruling: Introduce snapshot13 solely for native Drive records/cleanup intents, with v12 pre-Drive backup, future-version rejection and atomic tool-grant rollback — persisted authority must survive restart without leaking private PKCE/token data — cost if wrong: rollback uses the recorded backup generation, and restart requires a new consent attempt rather than silently reconstructing private pending authority.

Additional freeze:

- Owner A only new drive/{model,client,store}.rs plus distinct client_tests/store_tests/model_tests and drive/HERMES-NOTICE. Self-wire client/store tests; B owns drive/mod.rs + manager modules/model test wiring, state/schema, tools/registry/contexts, owner routes/contracts, OAuth callback/dependency guard and app/runtime injection. C owns SDK/web. Root owns docs/master/handover/rulings/gates/publication. No worker Cargo/Git/services; compilation and formatting are controller-serialized.
- A uses existing gcalendar::store::GoogleOAuthTokens with new DriveTokenGrant and scopes. Names DriveTransport, DriveClient::new()->Result<Self,DriveError>, InMemoryDriveCredentialStore, OsKeyringDriveCredentialStore, exact signatures from frozen plan. OAuth config carries host callback URI, app revision and borrowed/private app credentials; announce concrete constructor/method/type fields to B before implementation rather than silently diverge.
- Status omits private verifier, token, generation, pending nonce and account permission ID. Opaque state necessarily appears only inside the owner consent URL for the OAuth exchange; never as a standalone status field, logs or snapshot. PKCE verifier remains private and zeroizing.
- Refresh missing scopes may inherit only the previously stored validated grant; explicit reduced scopes fail closed. Refresh token omission retains the stored refresh token. Initial exchange must establish readonly scope and identified pinned account.
- UI supports pasting selected Google Drive/Docs links as well as IDs, extracts only the recognized Google host's exact file ID locally, previews selection before consent and sends canonical IDs. This is parsing, not network discovery or a new grant; server still validates every ID and immutable allowlist. New resource/format access is not implied by a link.
- Search/query is literal selected filename matching only; no files.list/raw provider q. Whole search/read deadline20s, callback45s; metadata64KiB, content1MiB streamed, result32768scalars, toolJSON256KiB and explicit digests/truncation. No unbounded provider retry sleeps; M13 pacing fixes remain intact.
- Existing Gmail/Calendar are not migrated merely for parity. A narrow verified defect affecting the selected daily flow can be fixed with evidence, but no unrelated broad audit. After M14 gates/merge, perform deterministic daily-assistant readiness checks and useful setup documentation under the no-services rule; do not represent it as live account/hosting verification.
