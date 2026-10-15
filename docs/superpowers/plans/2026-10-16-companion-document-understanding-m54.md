# M54 selected DOCX text understanding implementation plan

Status: controller-bound lean plan on `g9000/companion-document-understanding-m54`,
cut from verified M53/PR59 main `c2f25b241049b33ac95beefcc56fea140b047bbd`.
M53 source `526b81b1e67c198fc89526c768f8261b6add0343` shares tested tree
`9dcde91a9924285208d4141d07ca6de293d3ff51`; ten owned/four source/61 boundary
hashes and unrelated nx.json bytes were preserved. M53 web 1,361 tests in 115
files, typecheck/build/format/docs passed. This is M53 evidence, not M54 gates.

Root has selected the narrow DOCX slice, checked public dependency source/APIs
and bound five risks below. Publish plan/master/handover before the tests-only
semantic RED release; implementation follows demonstrated missing context.
No actual model, metadata refresh, ordinary daemon/account or deployment case.
M47/M52 cases remain immutable/consumed; M52 strict quality remains PARTIAL.
Exact resume: `.superpowers/sdd/2026-10-16-companion-document-understanding-m54/progress.md`.

The original proposal is design context. Final controller rulings supersede its
candidate dependencies, OS CPU-limit proposal and any differing interfaces.

## Historical lean proposal

**Goal:** sending a selected small DOCX in an owner chat supplies bounded, source-located main-body text to that run's existing selected model, while retaining the original download and explaining every unsupported or omitted part.

**Architecture:** keep reusable extraction contracts/rendering and the narrow parser under `packages/core-rust`; run parsing in an owned, resource-limited Rust host child. Reuse durable attachment identities, byte validation, run cancellation, context selection and committed-message persistence. No document service, general conversion tool, vector ingestion or new model loop.

**Tech stack:** existing Rust/Tokio/process-wrap; minimal ZIP/DEFLATE and streaming XML dependencies; a maintained safe Windows resource-limit wrapper. Dependency APIs, licenses and exact resolved versions still need controller verification during binding; none were installed or executed for this proposal.

## Source findings and chosen first slice

- `docs/superpowers/handover/2026-09-28-companion-console.md`, current **Start here**, says M53 is the active four-file web notice change. M47/M52 actual cases and releases remain immutable/consumed; M52 strict quality remains partial. M54 cannot replay them or borrow their provider authorization.
- `docs/companion/parity-roadmap.md`, **PAR-08**, requires selected-only context, isolated bounded parsing, retained originals, derived provenance and honest encrypted/corrupt/OCR limits. Text/image live acceptance and the later formats remain separate evidence.
- hosts/rust-daemon/src/attachments/{mod,validation,files,metadata}.rs already retain agent/session/root/SHA-256 identity, validate envelopes and cap stored documents at 25 MiB. `read_attachment_bytes` rechecks root, exact size/hash and classification. Upload validation is not document parsing.
- `hosts/rust-daemon/src/agent_runs/attachments.rs` resolves only selected IDs for the same agent/session, caps selection at ten, and currently says documents cannot be read automatically. Selected text is a saved-file reference requiring `read_file`; it is not automatically inline text.
- `hosts/rust-daemon/src/attachments/model.rs` hydrates only verified user images in the run's bound session. It acts after context selection; appending document text there without reserving its cost would bypass context budgeting.
- `hosts/rust-daemon/src/state/run_commit.rs` reserves input and agent-file text before selecting whole history turns. `agent_runs.rs` durably starts the run and registers Stop before `agent_files::runtime::admit_run`; this is the placement pattern for cancellable extraction before final context selection, outside state locks.
- anima-core/src/evidence.rs already supplies document/chunk/citation storage contracts. They are not a parsing contract and do not authorize adding an attachment to permanent knowledge. Reuse their provenance/locator conventions where useful; no knowledge write or retrieval subsystem is needed.
- `hosts/rust-daemon/src/mcp/stdio.rs` and browser/transport.rs own Windows child Jobs, bounded pipes, cancellation, retirement and retained leases on uncertainty. They do not presently configure hard parser memory/CPU limits. Their containment is not an OS filesystem/network security sandbox.

**Recommendation: DOCX main-body text only.** Hermes itself keeps a small standard-library DOCX path authoritative rather than sending DOCX through its broad converter. This gives a useful Office slice with paragraph provenance, controlled ZIP expansion and no Python, office-suite, renderer or OCR runtime. PDF first would also require a parser/converter choice, PDF object/stream limits and scanned-page coverage; existing code has none. Queue that deliberately instead of claiming a partial converter is general document understanding.

The ordinary workflow stays select attachment → Send → existing run. Before sending, the existing attachment chip may say **“DOCX text only”**. No additional extraction approval or provider call is introduced. Selection permits use of this document's bounded text in that message; it never grants filesystem, shell, connector, network or knowledge-write tools.

## Reuse decision / provenance

Public reference inspected statically at `C:/Users/leoca/AppData/Local/hermes/hermes-agent`; root's current handover pins `https://github.com/NousResearch/hermes-agent` to **`1fd18277267152893a8a24792b79213e7ca3f899`**. No upstream code, helper, installer or private profile was executed/read. Root should retain its revision verification at binding, since this planner did not run Git.

| Option                          | Evidence and integration cost                                                                                                                                                                                                                                                                                                                                                             | Decision                                                                                                                                                                                                                             |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Direct Hermes package/component | tools/read_extract.py::\_extract_docx, `_open_zip`, `_zip_xml`, `_joined`; tools/file_tools.py::\_read_extracted_document and tests/tools/test_read_extract.py. The DOCX route uses Python `zipfile` + ElementTree, materializes XML/tree/text, and has no corresponding decompression/process memory/CPU controls. Importing the module also exposes broad anydoc/OCR/self-repair paths. | Do not import the whole Python tool/runtime into Rust or reuse its unbounded parser unchanged.                                                                                                                                       |
| Supported process/API adapter   | `pyproject.toml` declares `firecrawl-anydoc==0.2.4`; `read_extract.py` uses it for PDF/legacy Office and can repair imports or use hosted OCR. The separate skills/productivity/pdf/scripts/pdf_read.py uses pdfplumber/pypdf for page text/metadata/tables.                                                                                                                              | Keep for later PDF evaluation. A Python/converter process adds installation/packaging/licenses and still needs Anima ownership/resources/coverage controls. No package repair, network, hosted OCR or terminal fallback is imported. |
| Narrow attributed Rust port     | Port only `_extract_docx` paragraph/run semantics: WordprocessingML namespace, `w:t`, tab/break/CR, paragraph order, exclusion of `w:rt` ruby guides; host owns selected input and worker owns parsing.                                                                                                                                                                                   | **Selected.** Preserve behavior worth reusing while adding streaming, budgets, precise outcomes and Anima authority. No broad tool schema or converter dispatch copy.                                                                |

Hermes root `LICENSE`: MIT, Copyright (c) 2025 Nous Research. Its PDF/DOCX skill directories have separate MIT 2026 Nous Research notices, but no skill script is selected for copying. The upstream extraction tests mention Kilo-Org/kilocode ancestry (#10733/#10737/#10740); do not copy that test suite or assume its ancestry is newly relicensed. Author local fixtures independently, and retain any additional notices discovered for an actual substantial port. Record chosen functions, source revision, retained license and local differences in the new parser crate's `HERMES-NOTICE`.

Dependency evidence: root `Cargo.lock` already resolves `quick-xml 0.41.0`, `flate2 1.1.9`, `process-wrap 10.0.1` and Windows bindings; no `zip`, PDF or Office parsing crate was found. `process-wrap` is already a direct daemon dependency with tracing defaults disabled. Promote existing adequate XML/DEFLATE versions where compatible. Add a maintained ZIP reader with only stored/DEFLATE support and a safe Windows Job memory/CPU API (candidate `win32job`) only after root checks their current API/features/license and pins the lockfile. Candidate support is **not verified** by this static proposal. Do not handwrite ZIP/XML/Windows FFI, relax workspace `unsafe_code = forbid`, or turn a soft heap/byte bound into a hard memory claim. If the safe resource API is inadequate, reduce the supported host scope or revise the concrete dependency choice within this task; do not silently ship parsing without it.

## Proposed frozen interface and file ownership

| Lane                          | Candidate owned files                                                                                                                                                                                                                                                              | Responsibility                                                                                                                                                                                                                                                                                             |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Core/parser                   | packages/core-rust/crates/anima-core/src/document_content.rs (new), src/lib.rs, src/context_window.rs; packages/core-rust/crates/anima-document-content/{Cargo.toml,src/lib.rs,src/docx.rs,HERMES-NOTICE} (new); root `Cargo.toml`/`Cargo.lock` controller-owned                   | Small bounded extraction/result/render contract; pure token-cost accounting; independently reusable parser. Core gains no HTTP/DB/host parser/process dependency. Invoke the repo's generation skill before creating a new crate; root resolves the actual existing Nx layout, without changing `nx.json`. |
| Host                          | hosts/rust-daemon/src/attachments/documents/{mod,worker,tests}.rs (new), attachment `mod.rs`/`metadata.rs`/`model.rs`/tests, `agent_runs.rs`, agent_runs/attachments.rs, state/run_commit.rs, `state.rs`, `main.rs`, `lib.rs`, host `Cargo.toml`; focused tests/document_worker.rs | Host-produced selection/snapshot binding, owned worker lifecycle, one early worker entry mode, cancellation and provenance; budget reservation before selection; project only trusted summary metadata. No unrelated runtime refactor. Root narrows exact glue files before dispatch.                      |
| SDK/web after contract freeze | `hosts/rust-daemon/src/routes/contracts/sessions.rs` + focused route tests; `packages/sdk/src/sessions.ts`/tests; apps/web/src/components/attachments/{AttachmentChips,MessageAttachments}.tsx/tests and attachment lib only if needed                                             | Optional per-attachment extraction summary; existing chip/message controls state text and source locators. Originals use the unchanged download action. No new page/panel, CSS redesign or overlap with M53 ChatScreen/transcript edits.                                                                   |
| Controller                    | this held plan promoted to the public plan, parity-roadmap/master/handover, .superpowers/sdd/.../progress.md                                                                                                                                                                       | Five binding rulings, dependency/provenance record, exact commands, review/gate evidence and remaining PAR-08 limits.                                                                                                                                                                                      |

**Minimal contracts, not a general media API:** `DocumentTextPort::extract(format, bounded_bytes, fixed_limits)` returns `ExtractedDocumentText { extractor_version, status, blocks: [{locator,text}], omissions }` or a closed content-free failure. Source identity is attached by the host, never trusted from worker output. Worker input carries bytes + format only: no attachment path, owner IDs, grants, credentials, arbitrary options or output directory. A dropped future initiates physical close; the host retains the worker lease until observed retirement.

The daemon wraps results in an internal reserved `documentExtracts` snapshot tied to attachment ID, exact private source SHA-256, format and extractor/schema version. Keep it on the committed selected user message using existing bounded message persistence; do not create a new global cache/database/index. Its public projection has only status/version, bounded locators and omissions, not raw worker output/private root/digest/error body. All public client metadata paths must strip the new reserved key before admission, as they strip `attachmentRecords`/`attachmentInputText`; model text can never create it.

Core rendering/counting uses the exact same bounded excerpt representation; the host validates it against its selected durable record before exposing text to the model. Count current and historical excerpt cost before whole-turn selection, including after compaction/rebuild/reload. Existing ordinary text/image routing and paired tool messages stay intact. Do not use post-selection hydration to add uncounted text. Derived text is framed as quoted attachment data in the **user** message, never system instructions, tool output, a capability grant or a claimed assistant verification. Host-produced locators such as word/document.xml#paragraph=3 support grounded answers; DOCX has no stable page numbers.

## Proposed behavior and hard limits

| Boundary       | Proposed first-wave rule                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Selection      | Web owner chat only; same agent + session + selected opaque ID + durable root/size/hash/MIME match. At most **two DOCX** per accepted message; additional selections are saved with explicit not-included status. Existing ten-upload cap and other attachment families remain. Helpers/connector inputs and arbitrary `read_file` conversion are queued.                                                                                                                                                                                                                                                          |
| Original       | Existing 25 MiB upload/download behavior remains; automatic extraction accepts **at most 2 MiB per DOCX**. Larger originals stay downloadable with `too_large`, never silently truncated as input bytes. Recheck identity before parsing and before publishing.                                                                                                                                                                                                                                                                                                                                                    |
| Archive        | **256 entries**, **512-byte entry names**, **16 MiB aggregate declared uncompressed size**, **4 MiB actual aggregate bytes consumed** across selected XML parts, **100:1 actual expansion ratio**. Only stored/DEFLATE, single-volume non-ZIP64 packages. Reject encrypted flags, duplicates/ambiguous part names, traversal/absolute names, malformed/overlapping records, unsupported compression and inconsistent sizes/CRC. Never unpack members to disk or open a nested archive. Parent/worker enforce finite input independently; OS limits cover a library allocation before semantic checks.              |
| XML/text       | Only bounded `[Content_Types].xml` (64 KiB) and word/document.xml; streaming namespaces, **depth 64**, **100,000 events**, **2,000 body paragraphs**, **16 KiB UTF-8 emitted text per message** including labels/locators. Reject DTD/entity declarations, external entities and nonstandard expansion; permit predefined/numeric XML escapes. Stop before allocation/output exceeds bounds. Content/text paragraph limits return an explicitly partial prefix only when the consumed prefix is valid; structural/resource failure returns no usable extract.                                                      |
| Token budget   | The 16 KiB excerpt is an absolute output ceiling, not a promise it fits every model. Reserve its conservative byte-based token cost plus labels within the existing calibrated context budget/reply reserve before history selection; reduce at complete paragraph/UTF-8 boundaries when needed. Explicit `partial`/`context_omitted` if no room. This is an estimate, not exact provider-token accounting or a universal model-window guarantee.                                                                                                                                                                  |
| Worker         | One process-wide physical parser slot, no unbounded waiting queue; busy means saved/unavailable, no retry. Fixed same-release daemon executable + internal `--document-worker-v1` mode, chosen by host without PATH/shell/user argv. Dispatch this mode before ordinary tracing/config/store/provider/network initialization. Private empty cwd, cleared environment plus only platform essentials, no inherited secret handles, no document paths or child subprocess features. Read attachment bytes in host with existing bounded capability I/O; send bounded bytes over stdin only after a small READY frame. |
| OS resources   | Initial support **Windows only**, using maintained safe APIs. Before READY and before reading any document bytes install/query a Job **256 MiB process committed-memory cap**, **5 s process CPU cap** and owned termination. Parent **10 s wall deadline** includes spawn/handshake/write/read, or an earlier run/Stop deadline. Exact working limit flags are bound and physically tested, not inferred from byte caps. If unsupported/assignment/verification fails, return `worker_unavailable` without sending bytes. Non-Windows fails before spawn until equivalent memory/CPU/retirement proof exists.     |
| IPC/retirement | Versioned length-bounded request/result, **64 KiB total stdout**, **4 KiB drained/discarded stderr**; malformed, extra output, stream overflow, timeout, Stop or lost owner closes the owned Job and rejects late results. Retain slot until process/tree/pipes exit is observed; quarantine on uncertainty. Never release a slot merely because an await was dropped. No restart retry, worker pool, background extraction or replay after daemon restart.                                                                                                                                                        |
| Retention      | Original retained as now. A settled versioned excerpt/safe status follows its committed message/session lifecycle and survives ordinary reload/restart. Interrupted extraction is not a settled result; restart marks it interrupted and does not automatically parse again. No global cache, temporary document copy, content logging or broad deletion policy change.                                                                                                                                                                                                                                            |

**DOCX coverage:** transitional WordprocessingML main-body paragraphs, including table-cell paragraphs in document order, with run text/tab/line-break handling and ruby-guide exclusion. Preserve source text spacing. No rendering/layout/table-grid/formula/list-number synthesis. Explicitly omit headers/footers/footnotes/endnotes/comments, drawing/textbox/embedded-object content, images, field instructions and deleted revisions; no links/relationships are fetched. Reject macro-bearing or strict-namespace/encrypted formats as unsupported rather than guessing. Empty/image-only body reports `no_text`; it does not claim OCR or full document coverage.

Closed user-visible statuses: `ready` (covered main-body text), `partial` (known body/context limit), `unsupported`, `encrypted`, `malformed`, `too_large`, `no_text`, `unavailable`, `cancelled`. Keep reasons content-free and finite. A filename/extension does not prove encryption; when the format cannot be identified safely, say unsupported/malformed rather than guessing. Missing optional summary on historical messages means no extraction evidence, not ready. “Ready” never claims the model answered correctly.

Job/process isolation here protects daemon availability and bounded resources. It is **not** a restricted Windows token/AppContainer or proof that a compromised worker cannot access the user account. The fixed Rust parser performs no file/network/macro/relationship actions; malicious archive/XML contents cannot request them. Do not advertise a security sandbox beyond the observed containment. A later general converter/native-code/OCR mode needs its own stronger execution-boundary decision.

## Test list and lean execution sequence

- [ ] **Bind once after M53 merge.** Root verifies current branch/main and scopes, confirms available safe resource APIs/dependency licenses, records exact versions/source notices, chooses final paths/contracts/limits and rules on the five risks below. Do not run a separate broad audit or ask the owner for another implementation approval. No model/service needed for binding.
- [ ] **Meaningful RED first.** Author a generated DOCX fixture with distinct literal strings in selected main-body paragraphs, tab/break/ruby/table cases, unrelated uploads and embedded injection prose. Capture the actual current absence of extracted text/status and the small-budget reservation failure through real handlers/recording model adapter. Root runs RED; parser/lifecycle failures may be demonstrated in focused new tests without replaying old cases.
- [ ] **Core/parser lane.** Implement bounded contracts, independently generated fixture builders and attributed DOCX algorithm. Cases: namespace-prefix changes, mixed runs/whitespace, paragraph/table ordering, ruby guides, Unicode, escaped text, empty body, omissions/unsupported namespace; duplicate parts, missing parts/types, wrong family, malformed XML/ZIP, encrypted container/flags, ZIP64/multi-volume, bad CRC/sizes, excessive entries/names/depth/events/paragraphs, actual DEFLATE bomb, malicious entity/DTD and macro member. Assert exact text + locators + status, not just success.
- [ ] **Host lane.** Real-handler tests prove selected-only agent/session checks; fabricated IDs/metadata/worker identity, changed hash/root/MIME, wrong owner, deleted session, unavailable bytes, unsupported extension; zero unrelated-file reads and zero grants/tools/memory writes. Two selected attachments share the aggregate limit. Place extraction after durable running/Stop registration and before final context/file admission; never parse under a daemon/history lock. Check normal/steered/rebuilt/history paths either use a validated snapshot with accounted cost or explicitly decline extraction.
- [ ] **Physical worker tests.** A fixed test mode/peer exercises readiness-before-bytes; memory-limit exhaustion and CPU spin under actual configured Job limits; wall timeout, cancellation during spawn/write/read, stdout flood, malformed/multiple frames, stderr flood, crash and descendant/held-pipe retirement. Independent identities show only owned children are retired, no result after Stop, and the permit survives until observed close/quarantine. Check normal daemon launch still follows its existing path and worker mode initializes no provider/store/listener/config. These are finite synthetic processes for root to release/run, not permission to run a daemon now.
- [ ] **Context/persistence tests.** Recording generate **and stream** adapters see exactly the selected, attributed excerpt in user data and no private keys/raw bytes/unselected text. Tiny budgets, history selection, compaction, wrap-up and reload neither exceed the derived limit nor duplicate/reparse excerpts. Same-store reload keeps exact settled source/version/locators/text/status; interrupted parse has no result/no automatic replay. Preserve tool-pairing and existing text/image tests.
- [ ] **SDK/web lane after native contract freeze.** Add optional summary/status types; missing historical fields remain display-only. Show ready main-body-only/partial/unsupported/no-text/unavailable states and existing original download. Source label points to the selected attachment and paragraph locator, never fabricated page numbers or arbitrary links. Check send-without-extra-dialog, Stop, reload, failed extraction, keyboard controls and stale-owner response suppression. No model result can manufacture a native ready status.
- [ ] **One combined native/core review, final SDK/web scope review, one justified fix wave.** Review resource installation/retirement, reserved metadata and pre-selection cost as one cross-layer behavior. Root runs current required gates, retains exact command/output/revision evidence, then publication/reviewed merge under standing authority. No repeated pass hunting.

## Future root-only commands (not executed by this planner)

Resolve names/targets first using `bun x nx show projects --json` and `bun x nx show project <resolved-name> --json`; preserve `nx.json` and `anima.yaml`. Root copies already-resolved M53 SDK/web gates where still valid. New crate tests must be included in the resolved Rust workspace test target; update only the narrow owned project target if its current inputs exclude them.

For every Cargo-backed invocation retain `CARGO_INCREMENTAL=0`, one build job and two test threads. Required known targets:

```text
bun x nx run rust-daemon:test --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run rust-daemon:build --skipNxCache
```

If the running daemon locks the binary, use the repository's Windows validation environment (`CI=1`, CARGO_TARGET_DIR=target/validation-rust-daemon) rather than touching that process. Root records exact focused test-filter/format commands from resolved target/help; this proposal invents no pass-through flags. Run resolved SDK/web test/typecheck/build/format targets for their changed files with `ANIMA_SKIP_SERVICE_TESTS=1` on SDK gates. No Astro/dev-launcher tests unless those sources actually change. No source success/implementation claim until fresh relevant gates pass.

## Top five proposed controller rulings

1. **Proposed: ship DOCX main-body text first —** Hermes has an independently small authoritative path and local upload identity already exists; PDF/other Office adds separate semantics/runtime/coverage costs — **cost if wrong:** useful PDF-first users wait; PAR-08 remains partial and visible, not falsely closed.
2. **Proposed: Windows owned child with verified hard memory/CPU limits before bytes —** existing Job cancellation is reusable but is not a memory/CPU cap; streaming limits alone do not establish isolation — **cost if wrong:** fail closed/unavailable on unsupported installs rather than risking daemon resources; no arbitrary-host/security-sandbox claim.
3. **Proposed: bind results only to server-selected agent/session/source hash and reserve exact rendered excerpt cost before selection —** filenames, client metadata, child echoes and extracted instructions confer no authority — **cost if wrong:** omit/refuse extraction rather than read another attachment, bypass a grant or add unbudgeted text.
4. **Proposed: persist one bounded versioned selected-message snapshot/status with locators —** the existing message lifecycle is enough for reproducibility; global caches/knowledge ingestion create unnecessary scope and invalidation work — **cost if wrong:** no reread/pagination/full-history document platform; known partial coverage stays explicit and originals remain usable.
5. **Proposed: synthetic local acceptance only for M54 —** this draft authorizes no model/provider/service release, and old M47/M52 cases cannot be recycled — **cost if wrong:** model answer/citation quality, actual text/image/PDF routing, non-Windows and wider Hermes parity remain unverified until separately bounded fresh cases are explicitly released.

## Completion boundary and queued remainder

Complete M54 only when the bound DOCX slice, physical resource/cancel behavior, selected-source/request wiring, retained original and settled reload evidence pass current gates/review. Report **implemented + locally accepted DOCX main-body text**, with the exact supported host and limits; no actual provider understanding/semantic-quality claim without a new explicit case release.

Queued: bounded PDF text-layer extraction with page/scan coverage; OCR as its own opt-in mode; XLSX sheets/cell/formula semantics, PPTX slide semantics, legacy/encrypted/password documents, strict OOXML/richer DOCX sections/layout; selected connector/helper document handling; checked exports/artifact creation; independently checked actual text/image/PDF/DOCX answers/citations; stronger hostile-process sandbox and other-host support. Native voice remains PAR-07. M52 reporting/helper quality remains partial and M53's notice retains its own separate scope/evidence.

**Freeze:** planner may write only this file. Static source review only; no tests, builds, formatter, Cargo/Nx/Git mutation, upstream execution, install, daemon, browser, model or network operation was performed. Root alone promotes or changes the held proposal.

## Controller rulings on the risks (binding)

These final rules supersede the historical proposal wherever they differ. This
is the implementation release after the verified M53 merge; no provider task,
metadata refresh, ordinary daemon, account, container or deployment is released.

1. Ruling: deliver selected DOCX main-body text in one useful native milestone —
   the pinned Hermes extraction is small and independently reusable, whereas
   PDF/OCR/other Office need different parsers and evidence — cost if wrong:
   PDF-first use waits; PAR-08 remains partial and those formats are explicitly
   unsupported. Port only paragraph/run text, tab/break/CR and ruby-guide exclusion
   from tools/read_extract.py at Hermes commit
   `1fd18277267152893a8a24792b79213e7ca3f899`; retain full MIT copyright 2025
   Nous Research, source pin and local changes in the parser crate HERMES-NOTICE.
   Root verified selected source/LICENSE clean. Generate independent fixtures;
   do not copy upstream tests with unresolved ancestry. No Python/runtime port.
2. Ruling: use exact rawzip 0.5.2, existing flate2 1.1.9 and quick-xml 0.41.0,
   and Windows-only win-custody 0.1.1 with existing process-wrap 10.0.1 — verified
   public APIs avoid handwritten ZIP/XML/Win32 FFI and preserve workspace unsafe
   prohibition — cost if wrong: source/API/physical tests block integration.
   Root downloaded checksum-verified published source only, not installed code.
   rawzip is MIT, MSRV1.85, no normal dependencies; win-custody MIT OR Apache-2.0,
   MSRV1.77, windows-sys0.61; current compiler1.93. Existing XML is MIT and
   DEFLATE MIT/Apache-2.0. Keep default features minimal and record the lock diff.
   Reject win32job2.0.3: working-set controls do not establish committed-memory
   caps. No Rust upgrade or broad competing harness dependency.
3. Ruling: hard Job committed-memory/process limits plus a parent wall deadline,
   with owned retirement, replace the draft's unsupported OS CPU quota — the
   selected safe wrapper supports memory/query but no CPU setter — cost if wrong:
   no 5-second kernel CPU claim; unsupported/failed limits refuse before bytes,
   and physical timeout/memory/cancel tests block release. Require 256 MiB process
   and job commit, one process, kill-on-close; query the exact installed settings.
   Retain process-wrap outer ownership. The parent may assign a resource Job via
   its safe borrowed child handle, or the fixed worker may query/create and
   assign_current_process before READY; choose one, document it and test it.
   Parent 10-second wall deadline covers spawn/handshake/write/read, or an earlier
   run cancellation/deadline. This is not an independent kernel CPU quota or an
   AppContainer/security sandbox. No permissive fallback. One process-wide slot,
   no waiting queue/retry; retain lease until whole owned process/pipes retire,
   quarantine uncertainty. Fixed internal worker mode before tracing/Tokio/service
   configuration, empty private cwd, clean environment, bounded Tokio IPC.
4. Ruling: only selected own agent/session bytes produce a bounded persisted
   snapshot, reserved before context selection and validated before hydration —
   post-selection image-style injection alone would omit its token cost and
   caller metadata cannot establish provenance — cost if wrong: refuse/omit the
   extract, never expand grants or expose foreign text. Preserve original download,
   upload identity, owner text, ordinary text/image behavior, Stop and tool pairs.
   No global index/cache/knowledge write, extraction retry or restart replay.
   Retain settled private version/source-ID/hash/blocks/status with the message;
   public projection exposes only safe status/count/coverage. No private hash/root,
   extracted text or worker error body in the public summary.
5. Ruling: meaningful real-handler semantic RED first, then frozen contracts and
   disjoint source authors, one combined native/core review plus final SDK/web
   review, one fix wave and fresh gates — source-backed cases are cheaper and
   clearer than paid model reruns — cost if wrong: no publication until evidence
   passes; M47/M52 stay immutable/consumed and M52 quality PARTIAL. Root alone
   executes/fmts/Git/processes. Dependency source inspection is not acceptance.
   No separate preflight audit or repeated owner check-in. Existing reviewed/gated
   merge-and-continue and per-command unsigned commits remain authorized.

### Bound interfaces and minimal ownership

New reusable parser crate: packages/core-rust/crates/anima-document-content.
Use the current Rust crate layout; Nx installed/local generator discovery found
no Rust crate generator, so no unrelated JS generator/plugin or nx.json edit.
Root owns root Cargo.toml/Cargo.lock and existing core-rust project target lists.
Append the new crate to existing core build/test/format package lists. No new Nx
project is needed. The resolved project name is `core-rust`, not an npm scope.

Core lane owns new anima-core document_content.rs plus lib.rs/context_window.rs,
and the parser crate Cargo.toml/src/lib.rs/src/docx.rs/tests/HERMES-NOTICE. Core
defines only small versioned bounded DTOs/rendering/cost helpers; it must not
depend on the parser, host process, HTTP or DB. The parser depends on core types.
Initially freeze the contract before host/worker authors consume it.

Worker lane owns new daemon attachments/documents/worker.rs and its focused
tests, plus the early main.rs worker entry and minimal lib.rs entry export.
Use exact `--document-worker-v1` with no user-selected program/path/options.
Avoid a second executable/build artifact. Normal no-argument daemon startup
remains equivalent; internal mode initializes no listener/store/provider.
The worker gets format and bounded raw bytes only, never owner IDs/paths/grants.
Parent supplies and validates its own provenance independently of worker output.
Versioned bounded request/result and READY framing must be documented in source;
2 MiB request bytes, 64 KiB total stdout, 4 KiB stderr ceiling. Never use blocking
read_to_end_bounded as a timeout/EOF guarantee. No user-supplied executable config.

Host lane owns documents/mod.rs/admission.rs, agent_runs/attachments.rs and narrow
agent_runs.rs/state/run_commit.rs/attachments/model.rs glue, metadata projection,
reserved-key stripping and route regression tests. Extraction runs after durable
running state/Stop registration, outside daemon locks, before file admission/final
history selection. Capture current config and scope once; retain existing rebuild
and compaction bindings. Only owner Web runs may initiate extraction. Helpers,
connectors, steered new documents and arbitrary read_file conversion are queued;
they cannot silently initiate a worker. Existing settled history can be reused
when its owned attachment identity still matches. Missing/stale/malformed snapshot
means no excerpt; no automatic repair/reparse. Interrupted work follows existing
run recovery and does not gain a settled ready record.

Reserved internal key is `documentExtracts`; strip it from every public client
metadata admission along with attachmentRecords/attachmentInputText. The pure
core contract validates version/closed enums/limits before counting/rendering.
Do not append the extract to saved owner text. Count the exact same rendered
quoted data before selection: ordinary text keeps existing estimates; document
bytes use a conservative one-token-per-rendered-UTF8-byte reserve, scaled by at
least1.0 calibration. Count current input and historical messages, including
compaction/rebuild/wrap-up. At the existing attachment model boundary hydrate only
that accounted snapshot in user data, after own record/source validation, for
generate and stream. This is untrusted document data, never system instructions.

Parser ceilings: 2 MiB input/doc, 256 entries, 512-byte names, 16 MiB declared
uncompressed aggregate, 4 MiB actual selected XML, 100:1 actual expansion,
64 KiB content-types, depth64/events100,000/paragraphs2,000. Stored/DEFLATE only;
reject ZIP64/multivolume/encryption/duplicates/traversal/overlap/selected-part CRC or size errors.
Never unpack to disk/fetch relationships. Use rawzip's checked entry/CRC reader,
also bound actual reads; advertised lengths cannot authorize allocation. Strict
namespace handling, no DTD/custom/external entities, no macro-bearing package.
Main-body paragraphs include table-cell text in order, but no layout/table grid,
headers/footers/notes/comments/drawing/textbox/fields/deleted revisions/OCR.
Return a closed failure without usable text for structural/resource failures;
limit-based prefix requires complete validated input and paragraph boundaries.

At most two selected DOCX are extracted per message; remaining selected originals
are retained with context_omitted and selection-limit coverage status. Aggregate model excerpt including
labels/locators is at most16 KiB and may be reduced at complete UTF8/paragraph
boundaries to fit the existing context budget. No room means context_omitted.
Paragraph locators use `word/document.xml#paragraph=N`, never fabricated pages.
Private snapshots retain source identity/locator/text/version. Public optional
per-message attachment `extraction` has `{version:1,status,paragraphCount,omissions}`;
no upload-record rewrite. Closed status values: ready, partial, unsupported,
encrypted, malformed, too_large, no_text, unavailable, cancelled, context_omitted.
Coverage/omissions are bounded closed values, not worker prose or URLs. Missing
historical field means no extraction evidence. Ready describes covered main-body
text, not correct model reasoning or full document coverage.

SDK/web author starts after this native projection is frozen: existing sessions
DTO/type exports/tests, MessageAttachments and AttachmentChips/test/lib only as
needed. Show finite status and main-body-only coverage beside existing original
download. No new page, source viewer, guessed page anchors, model action or CSS
redesign; no overlap with M53 ChatScreen/transcript. Keep original text/copy/speech.

### Binding verification and release sequence

Root first publishes this plan/master/handover. Test author then changes only
existing native attachment route tests, using an independently generated tiny
DOCX and recording adapter to assert its literal selected paragraph reaches the
model. Demonstrate the current missing text as semantic RED without importing
new/unimplemented APIs. Only after that RED release source/contracts/dependencies.
Root alone generates fixtures/locks dependencies and executes synthetic workers.
No actual account/model/daemon or M52 replay is part of this milestone.

Required fresh gates after review/fix: resolved rust-daemon test/lint/build and
core-rust test/lint/build including new parser, focused physical resource/Stop/
framing/retirement cases, SDK/web test/typecheck/build for changed contracts and
UI, explicit touched-file formatting/docs/diff checks. CARGO_INCREMENTAL=0,
jobs1/threads2, cleared real-service/provider environment and ANIMA_SKIP_SERVICE_TESTS=1.
No Astro/dev-launcher gate unless their source changes. Root records genuine
unsupported behavior rather than weakening expectations to produce passes.

Primary dependency evidence: [rawzip tagged manifest](https://github.com/nickbabcock/rawzip/blob/v0.5.2/Cargo.toml),
[rawzip reading/validation](https://docs.rs/rawzip/0.5.2/rawzip/),
[win-custody pinned Job implementation](https://github.com/115dkk/Win-Custody.rs/blob/ea96b2b8633dfa0e9a5f29e2b000aea11c565798/src/job.rs),
and [Microsoft Job limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information).
Registry checksums and exact source-inspection receipts are retained privately.
