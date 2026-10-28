# Deeper connected-account search implementation plan

> **For agentic workers:** Steps use checkbox syntax for tracking. The user-directed coordination workflow overrides generic skill suggestions to create more agents, worktrees, or commits.

**Goal:** Add explicit, resumable older-inbox and attachment search plus selected Microsoft file coverage without replaying historical account workflows or weakening source consent.

**Architecture:** Preserve the existing Rust search service and durable transaction. Separate live events, silent history merges, and silent extraction, with independent cursors/import receipts and stable source identities. Reuse the bounded document worker and shared OAuth application lifecycle for a separate Microsoft files connector.

**Tech stack:** Rust daemon, serde/reqwest/chrono, existing mail transports and document extractor; TypeScript SDK and React account settings UI. No new dependency is planned.

**Spec:** `docs/superpowers/specs/2026-10-27-deeper-account-search-design.md`.

**Status:** Implementation is in the shared tree. The coordinated full Rust tests, Rust lint, and SDK tests passed; the full web suite remains failing. The unchecked implementation steps below remain a verification checklist, not a claim that every listed test has run. The root coordination plan is authoritative for combined-tree status.

## Verification evidence as of 2026-10-28

- [x] Independent source review cleared the original six account-search findings and the two additional Microsoft persistence/access-control findings. This does not establish native test results.
- [x] Synthetic account-search browser acceptance passed 2/2 at 1280 px and 390 px, covering opt-in, bounded history and file selection, attachment controls, failed-save and pending states, and page/content horizontal overflow. The combined browser run had separate failures outside these two cases.
- [x] SDK build and typecheck passed; the current web-e2e no-output typecheck and lint passed. Web-e2e lint reported warnings in unrelated existing files.
- [x] The saved focused run `test-output/continuity-clean-connected-search.log` passed 17 tests, including populated source/search restore, silent history ingestion, failed persistence rollback, cancellation, and attachment retry after restart.
- [x] The saved focused run `test-output/continuity-clean-microsoft.log` passed 14 tests, including fresh-restart and failed-save fencing, in-flight selection/revocation, and the exact GET Microsoft callback owner-boundary allowlist and callback rejection tests.
- [x] The full Rust log records the Gmail fixed inbox range history loopback fixture, Outlook cross-origin/wrong-path continuation rejection, Gmail attachment descriptor bounds/body distinction, and supported attachment extension checks. These executed fixtures do not establish the complete Gmail and Outlook backfill/attachment provider-loopback matrix; remaining planned cases are unverified.
- [x] The coordinated `bun run rust:test` passed (`test-output/continuity-full-rust-formatted.log`); the daemon suite reported 3,661 passed, 0 failed, and 38 ignored. `bun run rust:lint` passed with warnings (`test-output/continuity-rust-lint-final.log`). The full default SDK suite passed 51 files and 291 tests (`test-output/continuity-sdk-full-final.log`), including service tests.
- [ ] The full web run remains failing: the coordinator reports 2,284 passed, 165 failed, and 1 skipped. The root coordination plan is authoritative for combined-tree verification.
- [ ] Complete any remaining integrated acceptance, including provider-specific tenant/platform coverage where authorized. No real provider account was accessed during this work.

---

## Execution constraints and ownership

- Work in the existing checkout. Do not create a worktree, switch branches, commit, push, deploy, reset, or touch real accounts/credentials. Preserve all pre-existing dirty files, particularly results-panel and Rust-cache/dev tooling work.
- Use `cmd.exe` with `login:false`. Use Context7 for provider/library documentation. Do not contact real provider account APIs; use fake transports and loopback fixtures.
- Never run concurrent Cargo processes. Ask the coordinator for a build slot through the authorized coordination path before compilation/test/lint. Do not create per-feature target directories or bypass the 32 GiB managed cache limit.
- The exact local and shared paths are listed in the spec's ownership section. Treat `state.rs`, `state/run_commit.rs`, `state/swarm_runtime.rs`, `control_plane_store.rs`, `routes/mod.rs`, `routes/contracts/mod.rs`, `tools.rs`, `connectors/mod.rs`, `oauth_apps.rs`, `approvals/policy.rs`, SDK client/exports, `ConnectorsView.tsx`, and existing dirty READMEs as coordinated integration files until assigned.
- No edits to automatic reply `mailbox.rs`, source identity algorithms, account receipt retention limits, or unrelated extractor internals are planned.
- Use behavioral regression tests first. For each runtime step, add its failing fixture, verify failure in the allocated build slot, implement the minimal change, then verify the focused behavior before continuing. Group focused Cargo checks in one owned wrapper invocation rather than starting parallel compilers.

## Task 1: Lock compatibility and silent ingestion contracts

**Files**

- Modify: `hosts/rust-daemon/src/connected_search/{mod.rs,service.rs,tests.rs}`.
- Create: `hosts/rust-daemon/src/connected_search/{policy.rs,ingestion.rs,history_tests.rs,enrichment_tests.rs}`.
- Coordinated: `hosts/rust-daemon/src/account_workflows.rs`, `hosts/rust-daemon/src/account_workflows/tests.rs`.

- [ ] Record current dirty state and check the coordinator's ownership/build-slot release. Read the current tree again before editing; do not overwrite concurrent changes.
- [ ] Add a legacy serialized index fixture to the existing test module containing a live seen entry, stable parent citation/base revision, rule baseline, and a durable workflow receipt/run. Assert round-trip exact identity and zero queued deeper reads.
- [ ] Add the end-to-end regression skeleton: baseline, new live item, history page, attachment extraction, retention trim, restore, live reentry. Assert exactly one workflow receipt/run and unchanged live source identity. Verify it fails for missing silent-ingestion support.
- [ ] Define `SearchSettings`, `HistoryDepth`, `AttachmentTextMode`, `IngestionOrigin`, `SourceReadContext`, typed import cursors, and history coverage states/reasons from the spec. Add bounded/defaulted serde fields; reject invalid combinations rather than silently normalizing unsupported policies.
- [ ] Split snapshot application narrowly into live application, historical merge, and enrichment application. Keep the old parent citation/live revision algorithms byte-compatible. Normalize only newly fetched raw documents; merging retained normalized documents must not hash their revision again. New extraction has its own content revision.
- [ ] Make only live application construct workflow-eligible changes; reject non-live origins at account admission. Preserve rule baseline behavior and receipt fingerprints. Add new fields to existing synthetic constructors deliberately.
- [ ] Add independent hashed history suppression receipts and the stable conservative live event fence. Test later head reentry after text eviction and invalid/old provider timestamps. Do not insert all imported IDs into the 4,096-entry live `seen` map.
- [ ] Verify legacy restore and the regression together. Expected: history/enrichment change retained search content, never source generation/grant epoch/stream epoch/seen/rule baseline/receipt count.

## Task 2: Implement durable scheduling, merge, retention, and capacity

**Files**

- Create: `hosts/rust-daemon/src/connected_search/backfill.rs`.
- Modify: `hosts/rust-daemon/src/connected_search/{policy.rs,ingestion.rs,service.rs,mod.rs,history_tests.rs}`.
- Coordinated only if required: `hosts/rust-daemon/src/state.rs` restore hooks.

- [ ] Add tests for partial page persistence, restart continuation, expired cursor restart of the same range, duplicate/looped pagination, empty-with-next pages, blocked reads under settings changes, and cancelled callers during save.
- [ ] Implement immutable history pass ranges and atomic page/content/suppression-receipt/cursor publication through the current owned edit transaction. The final cursor is committed only with the matching content and honest coverage.
- [ ] Add settings revisions distinct from source authority. Identical desired settings retry successfully; stale different settings conflict. Migration defaults do not enroll/deepen a source.
- [ ] Add one source work gate, cancellation per enrollment/settings revision, and fair live-first scheduling. Coalesce repeated refresh requests. Limit a cycle to 90 seconds and history/enrichment work to 20 seconds/20 parents/50 requests/4 MiB/2 downloads.
- [ ] Implement per-source 2 MiB and owner count limits, global 12 MiB expansion admission within the existing 16 MiB snapshot cap, and 2,000 lifetime import-receipt limit. Calculate serialized size including metadata. Preserve over-limit legacy state and refuse expansion.
- [ ] Expire finite-depth mail by provider received time; preserve the recent head separately. Remove older content by selecting Recent without touching import suppression receipts or workflow receipts. Never infer historical deletion from absence in the head snapshot.
- [ ] Add bounded historical revalidation, known-denial/deletion purge, persisted rate-limit retry time, pause/resume, and separate history/live freshness. Capacity errors advise reducing retained content; no reset/re-enrollment suggestion.
- [ ] Verify retention and capacity failures leave dedupe/receipt state intact and that live work proceeds while history is paused/full. Expected: partial status explains exact active bound/gap.

## Task 3: Add Gmail and Outlook historical pages

**Files**

- Modify: `hosts/rust-daemon/src/connectors/mail/{search.rs,client.rs,client_search.rs}` and `hosts/rust-daemon/src/connected_search/providers.rs`.
- Create: `hosts/rust-daemon/src/connectors/mail/client_backfill.rs` and `hosts/rust-daemon/src/connectors/mail/client_backfill_tests.rs`.

- [ ] Add loopback fixtures that capture exact Gmail list/get and Graph inbox requests, return multiple pages, simulate rate limits/expired tokens, and record whether recent-preview or automatic-reply state was touched.
- [ ] Introduce a typed native page method on `MailTransport` and manager wrapper. Use default unsupported implementations for unrelated deterministic transports only where needed to keep fixture scope clear; production providers implement the method explicitly.
- [ ] Gmail: query the exact inbox label and fixed Unix-second date bounds; locally enforce millisecond `[from,before)` received range, bound/validate IDs and page tokens, and fetch at most 20 parent details per page.
- [ ] Outlook: use the current inbox path, received time filter/order, selected fields and text-body preference. Follow only exact-origin/expected-path continuation URLs. Preserve the existing ID representation and never reconstruct `$skip` from untrusted input.
- [ ] Keep manager authorization/app-revision/token checks around each bounded batch and recheck before commit. Historical page methods do not publish `r.messages`, connector preview freshness, or mailbox cursors.
- [ ] Expose IMAP historical capability as unsupported. Do not call a recent transport repeatedly while pretending to advance a history cursor.
- [ ] Verify inbox-only date boundaries, same-timestamp items, cross-origin/loop pagination rejection, revoked grants, page retry idempotency, and absence of preview/reply/workflow side effects.

## Task 4: Add attachment and selected-file enrichment

**Files**

- Create: `hosts/rust-daemon/src/connected_search/attachments.rs`.
- Create: `hosts/rust-daemon/src/connectors/mail/{client_attachments.rs,client_attachments_tests.rs}`.
- Modify: `hosts/rust-daemon/src/connectors/mail/{client.rs,client_search.rs,search.rs}`; `hosts/rust-daemon/src/connected_search/{ingestion.rs,providers.rs,enrichment_tests.rs}`.
- Modify narrowly: `hosts/rust-daemon/src/connectors/drive/{reads.rs,client.rs,model.rs,client_tests.rs,manager_tests.rs}`.
- Reuse unchanged: `hosts/rust-daemon/src/attachments/documents/worker.rs` and existing extraction status/locator contracts.

- [ ] Add fake attachments covering Gmail inline/external base64 parts and Graph file/item/reference types; include malicious MIME depth/count, false size claims, changing parent revisions and oversized chunked streams.
- [ ] Discover at most 10 descriptors per authorized parent, then download at most 2 supported attachments with 2 MiB per-body caps and the cycle budget. Enforce encoded and decoded bounds; count omitted descriptors honestly.
- [ ] Reuse `DocumentExtractor` with cancellation and a deadline for PDF/DOCX/XLSX; use bounded UTF-8 text/CSV decoding. Do not create upload-vault records or invoke session admission to reuse extraction.
- [ ] Persist attachment child documents/status with domain-separated citation identity, parent citation, locators/omissions and independent content revision. Cap children at 256 per source and text at 8 KiB each. Existing body live revisions remain exact.
- [ ] Add selected-only Google Drive binary reads under its current grant/selection checks; feed those bytes to the same enrichment adapter. Keep the provider metadata/body snapshot used by workflows separate.
- [ ] Discard late extraction for a changed/deleted parent, settings revision, selection, or grant. Off toggling immediately purges extracted text/queue and leaves bounded metadata. Parser-version retries are enrichment only.
- [ ] Verify no workflows for extraction/retry, child purge with parent, quoted adversarial text, worker busy/unavailable/no-text/encrypted/partial labels, and zero retained raw byte files. Non-Windows tests assert truthful unavailability rather than an in-process fallback.

## Task 5: Build the Microsoft files connector and consent lifecycle

**Files**

- Create: `hosts/rust-daemon/src/connectors/microsoft_files/{mod.rs,model.rs,client.rs,authorization.rs,reads.rs,store.rs,tests.rs}`.
- Create: `hosts/rust-daemon/src/routes/microsoft_files.rs`, `hosts/rust-daemon/src/routes/contracts/microsoft_files.rs`, `hosts/rust-daemon/src/tools/microsoft_files.rs`.
- Modify: `hosts/rust-daemon/src/connected_search/{mod.rs,providers.rs,service.rs}`.
- Coordinated: `hosts/rust-daemon/src/{state.rs,control_plane_store.rs,tools.rs}`, `hosts/rust-daemon/src/state/{run_commit.rs,swarm_runtime.rs}`, `hosts/rust-daemon/src/connectors/{mod.rs,oauth_apps.rs}`, `hosts/rust-daemon/src/routes/{mod.rs,contracts/mod.rs}`, `hosts/rust-daemon/src/approvals/policy.rs`.

- [ ] Add fake vault/OAuth/Graph fixtures first: PKCE/state expiry/single use, cross-agent callback, app revision changed mid-consent, reduced/missing scope on refresh, token rotation and failed publication cleanup.
- [ ] Implement separate OneDrive and SharePoint modes/grants. OneDrive requests `Files.Read`; SharePoint requests `Sites.Selected` and requires externally approved sites. Both use offline account consent and separate file token vault entries, never Outlook tokens. Add the new callback to application setup status.
- [ ] Add connector metadata/default/save/load/validation and cleanup intents. Include active, pending and cleanup file connections in Microsoft OAuth app dependency checks under the existing lifecycle lock.
- [ ] Add owner-only paged metadata browsing of own OneDrive or one approved SharePoint site/library, at most 50 rows per response. Return opaque local cursors/file keys with no raw download URLs. Denied site discovery returns organization-approval status without a wider-scope retry.
- [ ] Add explicit 16-file selection with generation/selection revision concurrency, immediate removed-item purge, and typed bounded metadata/content reads. Selected-file addition is a silent baseline.
- [ ] Implement validated unauthenticated temporary downloads: no forwarded bearer token, no persisted signed URLs, bounded redirects/body, deny private/local or unrelated destinations. Return metadata-only unavailable text when safe validation fails.
- [ ] Register `microsoft_files_read` with selected-key-only arguments and current permission checks. Wire weak manager handles through direct and swarm dispatch alongside `with_drive`, so idle shells retain no token custody. Construct new `onedrive`/`sharepoint` search source authorities requiring that permission. Mark workflow event capability empty.
- [ ] Verify restart, missing vault, explicit denial purge, selection/read races, app dependency locks, mode/account restrictions, unsupported formats, and secret-free snapshot/errors/logs. No real OAuth flow is completed during testing.

## Task 6: Publish owner API and SDK coverage contracts

**Files**

- Modify: `hosts/rust-daemon/src/routes/connected_search.rs`, `hosts/rust-daemon/src/tools/connected_search.rs`, native model types.
- Modify: `packages/sdk/src/{connected-accounts.ts,connected-accounts.spec.ts}`.
- Create: `packages/sdk/src/{microsoft-files.ts,microsoft-files.spec.ts}`.
- Coordinated: `packages/sdk/src/{client.ts,index.ts}`, `hosts/rust-daemon/src/routes/mod.rs` OpenAPI registration.

- [ ] Add request fixtures showing old `{enabled,expectedGeneration}` and `{connectorId?}` still work; new settings writes require settings concurrency and reject unsupported provider settings.
- [ ] Implement additive source capability/history/attachment coverage and settings fields; preserve `lastSuccessAtMs` as live freshness. Add `work` to refresh with default live; no client accepts provider cursors or arbitrary provider URLs.
- [ ] Add content role, parent citation, provider received/modified dates, verification time, status/omissions and locators to hits/items. Leave old kind/sourceModifiedAt/citation fields compatible.
- [ ] Keep owner guard before private state/body parsing and `no-store` on every new result/error/callback. Ensure error codes never reflect upstream sensitive bodies or query tokens.
- [ ] Add a compact, explicitly caveated model projection and maximum encoded result test so extended source coverage does not exceed the existing 128 KiB bounded-result allowance.
- [ ] Add MicrosoftFilesClient request/response contracts and client exports. Test exact owner paths, encoded IDs, stale revisions, selection limits, abort signals, and no raw provider URLs/tokens in public results.
- [ ] Run SDK test/build/typecheck wrappers after source changes settle. Expected: existing callers and new contracts pass together.

## Task 7: Add nontechnical settings, coverage, and Microsoft file UI

**Files**

- Create: `apps/web/src/components/connected-accounts/{SourceSearchSettings.tsx,SourceSearchSettings.test.tsx}`.
- Create: `apps/web/src/components/{MicrosoftFilesCard.tsx,MicrosoftFilesCard.test.tsx}`.
- Modify: `apps/web/src/components/connected-accounts/{ConnectedAccountsPanel.tsx,ConnectedAccountsPanel.test.tsx,presentation.ts,AccountRuleEditor.tsx}`.
- Coordinated: `apps/web/src/components/{ConnectorsView.tsx,ConnectorsView.test.tsx}`.

- [ ] Add UI tests for current defaults, explicit older-content/text selection, pending/running/paused/partial states, IMAP unsupported options, stale settings conflicts and unmounted/agent-switch response races.
- [ ] Render Search back/Keep locally/Include attachment text using source capabilities. Explain the local limit and supported formats before the owner saves; no advanced provider vocabulary in the ordinary flow.
- [ ] Add pause/resume and Remove older content using the settings API. Do not implement these by toggling the entire source off/on.
- [ ] Show live freshness separately from imported-range/attachment coverage. Results label provider dates and indexed/verified times correctly. Empty partial results state that the retained range is incomplete.
- [ ] Add attachment child/parent navigation, locators and omissions; keep source text in plain quoted/preformatted rendering. Use only validated original links.
- [ ] Add Microsoft file-only connection flow, own-drive/approved-site browsing, explicit file selection, missing-app/callback/organization-approval states, reconnect and disconnect. Search enablement remains a separate owner control.
- [ ] Filter account-rule sources by supported event capability; Microsoft file sources have no workflow event choices in this feature.
- [ ] Run web tests/build/typecheck wrappers after source changes settle. Inspect the nontechnical flow using synthetic data if a local preview is used; do not load connected account data.

## Task 8: Integration verification and review handoff

**Files**

- Feature-local test modules above.
- Coordinated docs: `hosts/rust-daemon/README.md`, `packages/sdk/README.md`; preserve their existing edits.
- Coordinator-owned generated artifacts only if required by repository tooling.

- [ ] Re-run the full baseline/new-event/history/enrichment/trim/restore/reentry regression with both Gmail and Outlook fake providers. Check the exact unchanged durable workflow receipt payload as well as the count.
- [ ] Verify revoke/read-permission removal during each blocked operation and persistence-failure compensation. Search/citation reads must not expose revoked content after the authority change saves.
- [ ] Test combined 16 MiB state bounds and 4 MiB live reserve, the lifetime suppression-ledger ceiling, maximum UTF-8 serialization, scheduler fairness and refresh storms. Preserve receipts and report the active capacity reason.
- [ ] Add narrowly scoped docs for depth/local retention, attachment formats/platform limits, Microsoft callbacks/consent/admin setup, incomplete coverage, purge and workflow silence.
- [ ] Obtain the coordinator's exclusive Rust validation slot, then run these commands sequentially in the current combined tree:

```text
bun run rust:test
bun run rust:lint
bun run --filter @animaos/sdk test
bun run --filter @animaos/sdk build
bun run --filter @animaos/sdk typecheck
bun run --filter @animaos/web test
bun run --filter @animaos/web build
bun run --filter @animaos/web typecheck
```

- [ ] Expected result: every relevant command passes in the current tree. Report failures and ownership clearly; do not claim unrun commands passed. Root-wide tests/build/typecheck/lint and generated inventories stay under coordinator control to avoid concurrent changes/builds.
- [ ] Record evidence and clean only exact task-owned scratch/report paths after checking active users. Leave tool caches and managed Cargo bookkeeping intact.
- [ ] Report final source paths, contract changes, verification results, remaining platform/SharePoint tenant acceptance limitations, and integration risks. Do not commit, push, deploy, or send cross-chat messages without the human's explicit authorization for those actions.

## Design acceptance checklist for coordinator release

- [ ] Confirm selected-site SharePoint scope and truthful organization-setup limitation, rather than broad account/tenant discovery.
- [ ] Confirm 365-day inbox maximum, 100/500/2,000 retained-message choices, 2,000 lifetime hashed import receipts, and existing 16 MiB registry budget.
- [ ] Assign the shared files listed in the spec, especially state/routes/tools/SDK exports/ConnectorsView.
- [ ] Ensure any other feature changing `SourceChange`, account workflows, document extraction, or connection UI adopts the live-only ingestion/citation/permission contracts before implementation merges in the shared checkout.
- [ ] Release implementation in milestone order: silent history/enrichment safety, provider backfill, extraction, Microsoft integration, API/SDK/UI, then integrated checks.

These are coordinator design decisions for the already authorized implementation; they are not additional human approval prompts.
