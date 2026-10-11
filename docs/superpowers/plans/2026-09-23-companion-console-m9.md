# Companion Console M9: Attachments and Voice Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development to execute
> this plan task by task. This is a lean plan: interfaces, invariants, tests and
> commands are binding; complete code appears only where the boundary is subtle.
> The controller owns staging, signed commits, the decision ledger and publication.

**Goal:** Upload attachments into a session, retain their references through queued
and steered runs, show authenticated previews and downloads, send bounded images
to every provider, and add browser dictation and Read aloud (spec §12.1–§12.2).

**Architecture:** The history store owns attachment records; transcripts and run
inputs retain references, never bytes. The daemon writes and reads files through
capability directories, validates uploads before persistence, and hydrates images
only in a cloned model request after context selection. Provider builders remain
pure and reusable. The SDK owns multipart and binary requests; focused web hooks
own uploads and voice lifecycle; existing large components gain wiring only.

**Tech stack:** Existing Rust/axum/tokio/cap-std/cap-fs-ext/base64/sha2/chrono,
TypeScript/React/Vitest, browser FormData/XMLHttpRequest/speech APIs, Bun and Nx.
No dependency additions or dependency feature changes.

**Authority:**

- `docs/superpowers/specs/2026-09-23-companion-console-design.md`, §12.1–§12.2.
- Also §3.3 session deletion, §4.1–§4.8 runs, §5.2 context, §13 history,
  §14 owner authorization, §15.2–§15.3 transcript/composer, §16 limits, §17 tests.
- `docs/superpowers/plans/2026-09-23-companion-console.md`, M9 T9.1–T9.3.
- `docs/superpowers/handover/2026-09-28-companion-console.md`, Start here.
- `AGENTS.md` and `.superpowers/sdd/2026-09-23-companion-console-m9/rules.md`.
- Controller rulings appended at the end override earlier choices.

## Execution boundaries

- M0–M8 are merged; work on `feat/companion-console-m9`.
- Do not switch branches, create worktrees or use stash/reset/restore.
- Preserve unrelated edits, especially `nx.json` and `anima.yaml`.
- Stage only explicitly named owned paths; the controller performs all staging.
- Implementers do not commit; the controller handles GPG signing.
- Do not add packages, crates or dependency features.
- Do not start a daemon, browser dev server, database or container.
- Do not touch the Docker Postgres on port 55432.
- PostgreSQL conformance tests remain ignored without a configured test database.
- Existing in-process router, temporary-file and mocked-browser tests are allowed.
- No live provider calls or outbound requests during development/validation.
- Prefix every cargo command with `$env:CARGO_INCREMENTAL='0'`.
- Set no new `CARGO_TARGET_DIR`; reuse the targets already configured by Nx.
- Use Nx targets for required checks; narrow cargo filters are allowed for TDD.
- Keep new logic in new modules, hooks and components.
- `agent_runs.rs`, `connectors/runtime.rs` and `ViewHarness.tsx` gain wiring only.
- Keep control-plane transaction → state → leaf lock order.
- Never hold a std mutex guard across `.await`.
- Owner mutation transaction/save/undo bodies run inside `tokio::spawn`.
- Keep model text rendered as text and reveal hidden characters.
- Do not weaken existing hidden-Unicode refusal paths.
- Name user-facing/error strings once and test each constant once.
- Use barriers/channels/fake timers rather than wall-clock sleeps in tests.
- No new `act()` warnings, console warnings or unhandled promise rejections.
- No separate pre-flight audit or per-task owner check-in.
- Record judgments in the local milestone ledger as:
  `Ruling: <what> — <why> — <cost if wrong>`.
- One fix wave after the requested reviews, then the gates.
- M10 starts on its own branch from main after the owner merges M9.

## Inspected interfaces and existing behavior

Paths in this plan are repository-relative unless explicitly stated otherwise.

| Area             | Current interface / observation                                           | M9 consequence                                                      |
| ---------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| Core             | `primitives.rs::Attachment { attachment_type, name, data }`               | Add optional `mime_type`, with backward defaults                    |
| Core persistence | `runtime_serde.rs` explicitly maps attachments into/from `DataValue`      | Update both directions; serde alone is insufficient                 |
| Context          | `context_window.rs::cap_images` already caps to newest four               | Preserve it; ensure request hydration runs afterward                |
| Model boundary   | `ModelGenerateRequest { system, messages, temperature, max_tokens }`      | No persisted binary field or host dependency in core                |
| Providers        | Anthropic/Google/OpenAI/Ollama/ChatGPT have pure body builders            | Add pure image parts, never filesystem reads                        |
| Models           | `model_info(provider, model) -> Option<&'static ModelInfo>` with `vision` | Unknown and false entries get text fallback                         |
| Daemon deps      | cap-std, cap-fs-ext, base64, sha2, chrono already declared                | Use them without manifest changes                                   |
| Writer           | `tools/workspace.rs::write_workspace_bytes` checks then calls `fs::write` | Existing writer is not an atomic collision-safe upload primitive    |
| Reader           | `routes/workspace_files.rs` uses nofollow capability traversal            | Extract reusable bounded file opening without weakening Files rules |
| Multipart        | `routes/multipart.rs::FormLimits`, `parse_form_with`, linear search       | Reuse, with one-part attachment-specific limits                     |
| History          | SQLite `attachments`, PostgreSQL `history_attachments` already exist      | Add methods; no SQL migration/schema bump required                  |
| History access   | `HistoryService::store() -> Arc<dyn HistoryStore>`                        | Direct durable insertion for successful upload                      |
| Deletion         | Store delete_session/delete_agent already include attachment tables       | Extend memory store; verify deletion ordering                       |
| Run ledger       | `RunInput.attachment_ids`, `MAX_RUN_ATTACHMENTS = 10` already exist       | Fill and validate existing field                                    |
| Acceptance       | `AcceptRun` has text/key/mode/source/skill, not attachment IDs            | Add IDs and carry them through acceptance/start/steer               |
| Replay           | queue replay compares session and text today                              | Compare exact text, ordered IDs and skill                           |
| Run routes       | `validate_input` rejects any IDs as unknown until M9                      | Replace placeholder rejection with scoped validation                |
| Steer            | `RunSteer` and `steer_content` currently hold text only                   | Persist IDs; keep references and metadata through hand-on           |
| Telegram         | accepted owner path builds text-only `Content`                            | Add a helper for accepted content; preserve delivery bookkeeping    |
| Session API      | `SessionAttachmentResponse` currently exposes only type/name              | Add optional upload metadata without exposing bytes                 |
| SDK              | `requestJson` already handles `FormData` without JSON content type        | Reuse; add binary request helper and attachment client              |
| Web message      | `ChatMessage.content` drops session attachment metadata                   | Preserve it in `toChatMessage` and event conversion                 |
| Composer         | exported `Composer` lives in `components/ChatScreen.tsx`                  | Add focused child controls and minimal integration                  |
| Sends            | `useSessionSends.ts` owns `SendQueue` and sessionStorage recovery         | Retain attachment IDs in sends/retries/recovery                     |
| Commands         | `useSessionCommands` owns send/steer/sendAgain/startChat                  | Extend payload paths, preserve slash behavior                       |
| Dictation        | `useBrowserDictation` already handles recognition lifecycle               | Reuse, add cancellation for session/disable boundaries              |

## New boundaries and ownership

| Owner slice          | New files / module responsibilities                                                                        |
| -------------------- | ---------------------------------------------------------------------------------------------------------- |
| Core/provider worker | core MIME compatibility; adapters `attachments.rs`; provider body wiring; pure tests                       |
| History/files worker | daemon `attachments/{mod,validation,files}.rs`; history methods and conformance                            |
| Routes/run worker    | daemon `routes/attachments.rs`, attachment contracts; `agent_runs/attachments.rs`; queue/steer integration |
| Hydration worker     | daemon `attachments/model.rs`; adapter wrapping and request-only hydration tests                           |
| SDK/web worker       | SDK attachment client/blob helper; web upload helper/hook/controls/previews; sends metadata                |
| Voice worker         | browser dictation cancellation; read-aloud hook/button; focused lifecycle tests                            |
| Controller           | risk rulings, review batches, one fix wave, gates, docs, signed commits, PR                                |

Independent workers may inspect concurrently, but mutation ownership stays explicit.
Tasks 1–2 and 3–4 have disjoint main files and can be grouped by owner.
Tasks 5–7 run after their prerequisites; no competing edits in queue/routes/state.
SDK begins once contracts are fixed; web begins after SDK contracts exist.
Do not make a second parallel copy of any shared coordinator/component file.

## Contract decisions

### Upload and serve HTTP contract

```text
POST /api/agents/{agent_id}/sessions/{session_id}/attachments
Content-Type: multipart/form-data; boundary=...
Exactly one part named file, with a nonempty filename.

201:
{ attachment: {
  id: string,
  name: string,
  mime: string,
  size: integer,
  kind: "image" | "text" | "document",
  path: string,
  createdAtMs: integer
} }

GET /api/agents/{agent_id}/attachments/{attachment_id}
200: validated bytes, validated Content-Type
X-Content-Type-Options: nosniff
Cache-Control: no-store
Content-Disposition: inline for images; attachment for other kinds
```

- Upload authorization uses `routes::jobs::authorize(..., false)`.
- Serve authorization uses `authorize(..., true)`.
- Global API-key middleware remains effective on both routes.
- Authorize before reading request body or returning identifying metadata.
- No owner/API credential goes in a URL, thumbnail URL or download query string.
- `path` is a slash-separated workspace-relative path, never an absolute path.
- Attachment IDs are server-generated opaque IDs, not filesystem names.
- Upload requires an existing agent/session with `capabilities.send`.
- Do not upload into helper/job read-only sessions.
- No workspace: 409 with a named constant.
- Unknown agent/session/attachment: 404.
- Wrong-agent attachment: 404 on serve; do not reveal its real owner.
- Malformed multipart/type/filename/content/IDs: 400.
- Oversized body or classified file: 413.
- Persistence or blocking-task failure: 503.
- A linked/special/replaced/missing file gets a safe error, never a raw OS path.
- All outcomes use `no_store`/`rejected` conventions.
- No new list/delete attachment endpoint is needed for M9.
- Removing an attachment chip changes the draft only.
- Session deletion removes records; uploaded files stay in `uploads/`.
- Unsubmitted uploads and harmless failed-upload files may remain in the workspace.
- No cleanup scheduler, thumbnail service, OCR, PDF parser or transcoder.

### Size and type constants

```text
MAX_IMAGE_BYTES = 10 * 1024 * 1024
MAX_TEXT_BYTES = 1 * 1024 * 1024
MAX_DOCUMENT_BYTES = 25 * 1024 * 1024
MAX_UPLOAD_HEADER_BYTES = existing MAX_FORM_HEADER_BYTES (2 KiB)
MAX_UPLOAD_PARTS = 1
MAX_UPLOAD_FORM_BYTES = MAX_DOCUMENT_BYTES + bounded multipart overhead
MAX_UPLOAD_NAME_BYTES = 180
MAX_UPLOAD_COLLISION_ATTEMPTS = 100
```

- Choose a named fixed multipart overhead covering headers and 70-byte boundary.
- The whole-body limit applies before parser copies; it is never unbounded.
- Configure route body handling for 25 MiB plus overhead, not global 1 MiB.
- Do not raise the global limit for unrelated routes.
- Exactly-at-limit succeeds; limit-plus-one fails.
- Count bytes, not characters, for size and stored name bounds.
- Per-part limit remains 25 MiB; classification enforces the smaller limits.
- Bound collision attempts; do not spin until a path becomes free.
- A file is classified from its allowed extension and validated content.
- Client MIME is advisory; magic decides image MIME.
- PNG: full eight-byte signature, plus enough structure to reject header-only input.
- JPEG: SOI and plausible marker structure; reject trivially truncated signatures.
- WebP: RIFF/WEBP with valid declared length/recognized chunk header.
- GIF: GIF87a/GIF89a with logical-screen header present.
- Do not claim full decode validation without a decoder dependency.
- PNG/JPEG/WebP/GIF only; SVG/HTML masquerading as an image is refused.
- Use canonical `image/png`, `image/jpeg`, `image/webp`, `image/gif`.
- Valid UTF-8 text with explicit allowed text/source extensions.
- Canonical text MIME derives from extension; never reflect arbitrary MIME headers.
- Text covers Markdown/plain text/CSV/JSON/YAML/XML and common source extensions.
- A short explicit source-extension list is enough; do not accept every suffix.
- `.html`, `.svg`, executables, scripts disguised as documents, unknown binary: reject.
- Accepted source scripts remain download-only; no evaluation in the browser.
- PDF validates `%PDF-` plus nontrivial header content; saved, not read.
- Legacy `.doc/.xls/.ppt` validate compound-file signature and nontrivial header.
- `.docx/.xlsx/.pptx` validate ZIP signature and expected Office package markers.
- ZIP marker validation must be bounded; do not unzip/decompress or execute macros.
- Reject empty/header-only Office ZIP stubs and arbitrary ZIP renamed as Office.
- No macro-enabled document extensions in M9.
- Document MIME is canonical by extension and bytes are download-only.
- Text notes tell the companion to use `read_file` for the saved relative path.
- Document notes say contents cannot be read automatically yet.

### Saved filename and file identity

- Display/stored names are sanitized, bounded basename names.
- Strip both `/` and `\` path prefixes, including browser fake paths.
- Remove hidden Unicode/control characters and unsafe punctuation.
- Reject or normalize Windows device names, including device names with suffixes.
- Reject drive prefixes, ADS colons, trailing dots/spaces, `.` and `..`.
- Preserve a safe allowed extension; the result cannot change the classified type.
- A fallback basename such as `attachment` plus canonical extension is allowed.
- Do not create hidden files or an extension-only name.
- Collision names use `name-1.ext`, `name-2.ext`, and so on.
- Choose the suffix by atomic exclusive creation, never exists-then-write.
- Save under `uploads/<UTC YYYY-MM-DD>/...` using existing chrono.
- Pin the canonical configured workspace root in the private record.
- Capture/open that root once in the mutation; do not re-read a changed workspace.
- Serve/hydration refuse a record when its root differs from the active workspace.
- Record a sha256 digest using the already declared sha2 dependency.
- Bounded reads recheck size, digest, kind and MIME before serving/hydration.
- Replaced content therefore cannot silently become a different uploaded file.
- Stored root/digest are private fields, absent from HTTP attachment metadata.
- No absolute root appears in model notes or browser responses.

### Transcript, queue and request representation

```text
Core Attachment:
  attachment_type: existing enum
  name: existing String
  data: existing String           // persistent relative path
  mime_type: Option<String>       // new, serde(default), omit None

RunInput:
  text: original user text
  attachment_ids: ordered unique ids (existing field)
  skill: existing optional skill

Content.attachments:
  references {type, name, data=path, mime_type=validated MIME}

Content.metadata["attachmentRecords"]:
  server-produced public metadata, enough to render after history pruning

Content.metadata["attachmentInputText"]:
  original raw input where generated attachment notes changed content.text
```

- Public attachment records in content metadata contain no bytes/root/digest.
- Private record ownership is always checked through history, never metadata.
- Metadata is produced by the daemon; do not trust client-authored equivalent keys.
- Ordinary attachment-less transcripts continue working unchanged.
- Legacy attachment type/name-only entries remain representable.
- Session responses expose optional metadata alongside legacy type/name.
- Prefer optional fields on existing SDK attachment type to avoid breaking consumers.
- Run equality compares exact raw text, ordered IDs and skill.
- Duplicate IDs are rejected instead of silently deduplicated.
- No attachment ID from another agent/session can be accepted.
- A steer carries the same IDs and server metadata as a queued message.
- A leftover steer becomes a run with original text/IDs/skill semantics intact.
- Idempotent retries are resolved before newly missing-file/availability checks.
- Identical retries return their existing run even if a file later disappears.
- Different attachment order/content with an existing key returns 409.
- Recovery preserves references; it never reuploads bytes or automatically resends.
- Persisted messages never contain data URIs or base64 payloads.
- Only a cloned, selected `ModelGenerateRequest` may hold transient data URIs.
- Hydration never changes original transcript/reference metadata.
- At most four selected image attachments are hydrated.
- Vision false/unknown means no image reads, only a clear unavailable-vision note.
- Missing or modified images become a safe textual unavailable-file reference.
- A single unavailable image does not erase the other attachments or turn text.
- Generic provider builders accept transient validated data URI image references.
- They never resolve paths, HTTP URLs or arbitrary file schemes themselves.

## Commands and local environment

The controller verified project names and diagnosed the current Windows Nx issue.
Keep native plugin sockets disabled and use the repository cache directory.
Nx calls require the tool's `require_escalated` shell mode for native cache writes
and Vite realpath access; this is a sandbox workaround, not a package install.

```powershell
$env:NX_DAEMON='false'
$env:NX_ISOLATE_PLUGINS='false'
$env:NX_NO_CLOUD='true'
$env:NX_NATIVE_FILE_CACHE_DIRECTORY=(Join-Path (Get-Location) 'node_modules/.cache/nx-native-m9')
$env:CARGO_INCREMENTAL='0'
```

When starting a new shell invocation, set these variables again.
No edits to nx.json, anima.yaml or package manifests for this workaround.
Use `Select-Object -Last 30` for long outputs and verify `$LASTEXITCODE`.
Piping output does not turn a failing command into passing evidence.

Narrow Rust commands below are for task feedback where no granular Nx target exists.
Required full checks use the existing Nx targets.
Never use a direct cargo command without setting CARGO_INCREMENTAL first.
Do not clean broad target trees automatically; ask the controller to handle corruption.

## Task sequence and review schedule

1. Core MIME and explicit serialization compatibility.
2. Pure image rendering in every adapter, with vision fallback.
3. Attachment records and three history stores.
4. Upload validation and hardened capability file operations.
5. Owner upload/serve routes and transcript metadata contracts.
6. Queued/steered/Telegram attachment inputs and snapshot compatibility.
7. Request-time hydration wrapper and context integration.
8. SDK multipart/blob clients and attachment metadata.
9. Web upload lifecycle, progress, sends and recovery.
10. Composer attachment controls and authenticated transcript previews.
11. Browser dictation and Read aloud lifecycle.
12. Controller reviews, one fix wave, gates, documentation and PR.

Review batch A covers Tasks 1–4 together after their implementation.
Review batch B covers Tasks 5–7 together after their implementation.
Review focus is boundary correctness; reviewers do not rewrite the plan.
One final web+SDK review covers Tasks 8–11 together.
Controller records decisions, then dispatches one fix wave for accepted findings.
Blocker fixes are mandatory before the gates; no repeated speculative audit loop.

---

### Task 1: Core Attachment MIME and backward-compatible serialization

**Owner:** core/provider implementer; bounded compatibility work.
**Depends on:** controller risk rulings.

**Files:**

- Modify `packages/core-rust/crates/anima-core/src/primitives.rs`.
- Modify `packages/core-rust/crates/anima-core/src/runtime_serde.rs`.
- Modify actual `Attachment { ... }` literals in touched Rust test/code files.
- Add focused tests alongside explicit serializer tests.
- No HTTP, filesystem or DB imports in core.

**Exact interface:**

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub attachment_type: AttachmentType,
    pub name: String,
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}
```

**Rules:**

- Keep existing attachment enum variants and existing field names.
- Existing serialized JSON lacking MIME deserializes to None.
- Explicit runtime data-value encoding emits `mimeType` only when present.
- Explicit runtime decoding accepts absent/null MIME as None.
- Wrong-type MIME fails consistently with the existing decoder policy.
- Do not infer MIME from a path in core serialization.
- Preserve name/data/MIME round trips exactly.
- Update constructor literals with `mime_type: None` where appropriate.
- Keep context image fixtures with validated MIME where testing provider images.
- Run a targeted search for every Attachment literal, including ignored tests.
- Old history rows and snapshots must remain readable.
- Optional MIME alone does not justify migration of history SQL tables.

**Tests:**

- Old serde attachment JSON without MIME round-trips.
- Serde Some MIME is retained, None remains omitted.
- Runtime snapshot DataValue without MIME remains readable.
- Runtime Some MIME is retained through snapshot restore.
- File and URL attachments remain unchanged.
- Context image-cap test compiles and still passes.
- Literal updates do not change tool input or memory-store semantics.

**Steps:**

- [ ] Write compatibility tests before implementation.
- [ ] Run targeted tests to see the missing-field/round-trip failures.
- [ ] Add the field and update both explicit conversion directions.
- [ ] Update all literal construction sites without broad refactoring.
- [ ] Run the focused tests and compile dependent daemon tests.
- [ ] Format owned Rust sources; report any unrelated rustfmt drift.
- [ ] Return owned paths and verification to the controller for signed commit.

**Commands:**

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-core --lib -- runtime_serde 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-core --lib -- context_window 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo check -p anima-daemon --tests 2>&1 | Select-Object -Last 30
```

Expected: targeted tests pass; daemon test compilation succeeds.
Commit suggestion: `feat(core): preserve attachment MIME in snapshots`.

---

### Task 2: Pure image rendering in every provider adapter

**Owner:** core/provider implementer.
**Depends on:** Task 1.

**Files:**

- Create `packages/core-rust/crates/anima-model-adapters/src/attachments.rs`.
- Create focused `attachments_tests.rs` or inline module tests.
- Modify `.../src/lib.rs` for module/reexports.
- Modify `.../src/anthropic.rs`, `openai_compatible.rs`, `google.rs`, `ollama.rs`.
- Modify `.../src/chatgpt.rs`, `stream.rs` (deterministic behavior only as needed).
- Modify adapter tests for multimodal bodies; avoid growing general test files wildly.

**Interfaces:**

```text
pub fn model_supports_images(provider: &str, model: &str) -> bool
  model_info(provider, model).is_some_and(|row| row.vision)

pub(crate) struct PreparedImage<'a> {
  pub(crate) mime: &'a str,
  pub(crate) base64: &'a str,
  pub(crate) data_uri: &'a str,
}

pub(crate) fn prepared_image(attachment: &Attachment)
  -> Result<Option<PreparedImage<'_>>, String>

pub fn image_reference_text(name: &str, path: &str, reason: &str) -> String
```

**Rules:**

- Use existing model table, including canonical provider/ChatGPT aliases.
- Unknown or vision-false models have no image parts.
- Fallback mentions that the model cannot view the image and preserves name/path.
- Builders do not read disk, fetch URLs, decode images or introduce base64 dependency.
- Accept only supported image MIME and a matching transient data URI prefix.
- Validate payload shape/encoded-size bound with simple checks; daemon validates bytes.
- A plain relative path cannot be mistaken for encoded image data.
- Malformed prepared image returns a bounded error, not raw binary payload text.
- Normal text-only request bodies retain existing shape where possible.
- Multimodal user message text remains present and ordered before image blocks.
- Never drop assistant tool calls, tool results, IDs or role mapping.
- Anthropic user content becomes text/image blocks only when images exist.
- Preserve Anthropic system-to-user coalescing with mixed content arrays.
- OpenAI-compatible user content uses `text` and `image_url` parts.
- Google user parts use `text` and `inline_data { mime_type, data }`.
- Ollama user message carries `images: [base64]`, with text content preserved.
- Ollama tool-capable OpenAI-compatible path gets OpenAI image parts as well.
- ChatGPT Responses uses `input_text` and `input_image { image_url }` user parts.
- Deterministic adapter uses/validates text fallback; response stays deterministic.
- Both generate and stream paths use the same request builder behavior.
- No adapter-specific persistent attachment encoding.
- Do not enlarge model table or change provider pricing in M9.

**Tests:**

- One builder fixture per Anthropic/OpenAI/Google/Ollama/ChatGPT image format.
- Text plus two images retains declared ordering and distinct MIME.
- Text-only builder output remains backward-compatible.
- Tool-use/tool-result message ordering survives adjacent image turns.
- Vision false drops all binary image parts and adds fallback text.
- Unknown model behaves like vision false.
- Known true model supports canonical provider aliases.
- ChatGPT subscription alias resolves the existing OpenAI model row.
- Bare path and mismatched data URI/MIME never become binary image parts.
- Encoded payload is not present in provider error text or fallback references.
- Deterministic fallback preserves name/path without changing fixed response behavior.
- No real provider calls are required; test request JSON directly.

**Steps and commands:**

- [ ] Write pure builder fixtures and negative image cases.
- [ ] Implement shared helpers, then wire each provider builder.
- [ ] Re-run narrow image tests and existing provider request tests.
- [ ] Search all body builders and both generate/stream paths for coverage.
- [ ] Return paths/tests; controller commits.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-model-adapters --lib -- attachment 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-model-adapters --lib -- request 2>&1 | Select-Object -Last 30
```

Expected: fixture and compatibility tests pass without external requests.
Commit suggestion: `feat(adapters): render bounded images for every provider`.

---

### Task 3: Attachment records and all history stores

**Owner:** history/files implementer.
**Depends on:** fixed public contract; can run independently of Tasks 1–2.

**Files:**

- Create `hosts/rust-daemon/src/attachments/mod.rs` (types/constants/helpers).
- Modify `hosts/rust-daemon/src/lib.rs` for module wiring.
- Modify `history/{mod,memory,sqlite,postgres,conformance}.rs`.
- Existing migrations/tables are inspected and retained.
- No outbox/schema migration unless evidence contradicts inspected schema.

**Exact record and store interfaces:**

```text
AttachmentKind: Image | Text | Document
  serde snake_case, as_str()

AttachmentRecord: Clone + Debug + Eq + Serialize + Deserialize
  serde camelCase
  id: String
  agent_id: String
  session_id: String
  name: String
  mime: String
  size: u64
  kind: AttachmentKind
  path: String
  created_at_ms: u64
  workspace_root: String          // private canonical root binding
  sha256: String                  // private lowercase hex digest

HistoryStore::upsert_attachments(&self, rows: &[AttachmentRecord])
  -> Result<(), HistoryError>
HistoryStore::get_attachment(&self, agent_id: &str, attachment_id: &str)
  -> Result<Option<AttachmentRecord>, HistoryError>
```

**Rules:**

- Record IDs are immutable ownership bindings.
- Idempotent write of the same record succeeds.
- An ID collision cannot change agent/session/root/path or bytes identity.
- Existing store uses JSON record columns; serialize full private record there.
- SQLite work uses the existing dedicated connection/spawn_blocking mechanism.
- Postgres uses existing prepared parameterized query conventions.
- Lookup includes agent_id in its SQL/filter predicate.
- Memory store includes attachments in its bounded table accounting.
- Session/agent deletion removes attachment rows using existing scope semantics.
- Usage survives deletion exactly as before.
- Uploaded bytes are not stored in history.
- Direct attachment persistence precedes upload success; no asynchronous-only queue.
- Record writes remain serialized against session/agent deletion by the daemon transaction.
- Do not route upload records through the message outbox without a durable journal.
- No new attachment list API is required.
- API projection uses only the public fields.
- Root/digest never reach public contract serialization accidentally.

**Tests:**

- Conformance: insert/read attachment, correct agent only.
- Idempotent same-record upsert does not duplicate.
- Foreign-owner ID rewrite is rejected/not applied.
- Session deletion removes only its attachment rows.
- Agent deletion removes only its attachment rows.
- Rows of another agent/session remain intact.
- Memory capacity enforcement includes attachment records.
- SQLite reopen preserves full attachment record.
- Existing SQLite v1 schema accepts attachment rows without migration.
- Ignored Postgres conformance covers identical semantics.
- Serialization does not contain bytes; public projection omits private root/digest.
- Store error propagates to caller rather than inventing successful metadata.

**Steps and commands:**

- [ ] Extend conformance test helper/implementations before methods.
- [ ] Implement record type, each store and immutable-ID behavior.
- [ ] Run memory/SQLite conformance; leave Postgres ignored.
- [ ] Confirm existing deletion tests still pass.
- [ ] Return paths and pass/ignored counts to controller.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- history 2>&1 | Select-Object -Last 30
```

Expected: memory/SQLite history tests pass; no database is started.
Commit suggestion: `feat(daemon): persist scoped attachment records in history`.

---

### Task 4: Upload classification and hardened workspace byte operations

**Owner:** history/files implementer; security-sensitive boundary.
**Depends on:** Task 3 record type.

**Files:**

- Create `attachments/validation.rs`, `attachments/files.rs` and focused tests.
- Create `tools/workspace_bytes.rs` for reusable capability-based byte operations.
- Modify `tools.rs`, `tools/workspace.rs` for exports/wiring as needed.
- Modify `routes/workspace_files.rs` only to share safe opening helpers if needed.
- Preserve Files listing/preview exclusions; uploads do not broaden them.

**Interfaces:**

```text
ValidatedUpload { name: String, mime: String, kind: AttachmentKind }
validate_upload(filename: &str, bytes: &[u8])
  -> Result<ValidatedUpload, AttachmentError>
sanitize_upload_name(filename: &str, extension: &str) -> String

SavedUpload { path: String, workspace_root: String, sha256: String }
save_upload(root: &Path, date: &str, name: &str, bytes: &[u8])
  -> Result<SavedUpload, AttachmentError>
read_attachment_bytes(root: &Path, record: &AttachmentRecord)
  -> Result<Vec<u8>, AttachmentError>
```

**Capability helper requirements:**

- The generic workspace writer must be hardened or uploads must use a clearly
  named exclusive safe variant sharing its hardened internal operation.
- Preserve `write_file` overwrite semantics if replacing generic implementation.
- Uploads always use exclusive create; they never truncate existing content.
- Open canonical workspace root as `cap_std::fs::Dir`.
- Traverse each relative path component from a held directory handle.
- Reject symbolic links and Windows reparse/junction components.
- Use `open_dir_nofollow` rather than canonicalize-then-open traversal.
- For a missing upload directory, create then reopen nofollow.
- Do not hold a state/std lock during disk operations.
- Final file open uses nofollow + create_new for uploads.
- Final read uses existing nonblocking/special-file-safe opening pattern.
- Recheck opened handle metadata: regular file, no link/reparse marker.
- Read at most declared kind limit + 1; reject overflow.
- Handle metadata length is checked before allocation.
- Hash actual bounded bytes and compare stored identity.
- Validated MIME/type must still match record.
- Current canonical root must match record binding.
- Workspace setting changes cannot redirect a prior upload reference.
- Keep error text bounded and free of absolute filesystem roots.
- Collision suffix retry count is capped and occurs only on AlreadyExists.
- No preflight exists check establishes authority.
- Do not reuse `fs::write` after a path check for this boundary.
- No unsafe code, FFI, new filesystem dependency or platform feature.

**Tests:**

- Magic fixtures for four supported image types.
- Valid text/source extensions and UTF-8; invalid UTF-8 refusal.
- Allowed document signatures and mismatched/renamed payload refusal.
- Size boundary for image/text/document and whole-body bound constant.
- Hostile basename traversal, Windows separators, ADS, devices, hidden Unicode.
- Same filename collision produces distinct files without truncation.
- Parallel same-name creates never overwrite one another.
- Collision exhaustion returns bounded error.
- Parent link outside workspace fails.
- Final link outside workspace fails.
- Unix link-replacement/FIFO cases run under cfg(unix) using existing test patterns.
- Windows reparse checks are exercised with available deterministic metadata fixtures.
- Do not claim platform symlink tests passed when privilege/cfg skipped them.
- File growth beyond size bound fails without unbounded allocation.
- Same-size byte replacement fails digest validation.
- Active workspace change refuses prior record.
- Missing file is a controlled error.
- Generic writer overwrite regression, if generic writer changed.
- Files preview/list exclusions remain unchanged.

**Steps and commands:**

- [ ] Write classifier and capability-operation tests.
- [ ] Implement bounded validators and held-handle operations.
- [ ] Run attachment/workspace-focused tests.
- [ ] Confirm touched helpers introduce no unsafe/new dependency use.
- [ ] Report tested platform coverage honestly.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- attachment 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- workspace 2>&1 | Select-Object -Last 30
```

Expected: bounded classifier/storage tests pass.
Commit suggestion: `feat(daemon): validate and safely store workspace uploads`.

**Review checkpoint A:** batch-review Tasks 1–4; controller records findings.
No separate plan audit. Do not launch a fix-wave loop here.

---

### Task 5: Owner upload/serve routes and transcript metadata contracts

**Owner:** routes/run implementer.
**Depends on:** Tasks 3–4; Task 1 for MIME projections.

**Files:**

- Create `routes/attachments.rs` and `routes/contracts/attachments.rs`.
- Create `routes/tests/attachments.rs`.
- Modify `routes.rs`, `routes/contracts.rs` and actual OpenAPI registration module.
- Modify `routes/contracts/sessions.rs` and `shared.rs` for optional MIME/metadata.
- Add metadata projection helper in `attachments/mod.rs` or focused sibling.
- Extend `routes/tests.rs` registration if current structure requires it.

**Interfaces:**

```text
AttachmentResponse { id, name, mime, size, kind, path, created_at_ms }
  serde camelCase + ToSchema; public projection of AttachmentRecord
AttachmentEnvelope { attachment: AttachmentResponse }

SessionAttachmentResponse retains type/name and gains optional:
  id, mime, size, kind, path, created_at_ms

AttachmentError maps typed validation/storage/persistence failure to ApiError
```

**Upload mutation ordering:**

1. Authorize and syntactically validate route identifiers.
2. Bounded read/parse of exactly one `file` form part.
3. Classify/sanitize before any write.
4. Spawn owned mutation body; lock control-plane transaction.
5. Under state read, confirm agent/session/sendability/workspace and capture root/store.
6. Release state guard, retaining transaction; write file in spawn_blocking.
7. Construct private record and durably upsert in history.
8. Return public metadata only after history success.
9. Release transaction at owned task completion even if request future was dropped.

**Rules:**

- Preserve transaction serialization with session/agent deletion and workspace changes.
- No await while holding std locks.
- Body cancellation before mutation means no work; cancellation after spawn is drop-safe.
- History failure returns 503; never return unattested metadata.
- Failed persistence may leave a harmless workspace file.
- Avoid cleanup that could remove a concurrently replaced file without identity checks.
- Uploaded bytes never enter control-plane snapshot or model transcript here.
- Multipart parts cap is one, including unexpected text fields.
- Require filename/name exactly; reject absent/repeated/unknown parts.
- Ignore client MIME for trust; do not alter multipart parser header semantics unnecessarily.
- Register OpenAPI routes/tags/schemas and byte response MIME descriptions.
- Serve snapshots active session/agent/root/store under normal read conventions.
- Deleted session metadata cannot serve retained bytes after history row deletion.
- If pending deletion exists before mirror flush, fail closed using live session existence.
- Use RFC-safe Content-Disposition fallback and encoded Unicode filename if supported.
- Never interpolate CR/LF/quotes directly into response headers.
- All served kinds set nosniff/no-store.
- Image response is inline; nonimage response is attachment.
- Session metadata projection trusts only daemon-produced attachment record metadata.
- It still renders legacy type/name-only references without an ID.
- Session GET, event message projection and exports remain metadata-only.
- Do not expose the private sha/root via a derive on AttachmentRecord.

**Tests:**

- Authorized upload returns 201 and a durable readable row.
- Unauthorized upload/serve returns 403 before body/file access.
- Global API-key rejection still applies.
- No workspace 409; missing/wrong session 404; read-only kind 409.
- Exactly one file; extra/missing/fileless/malformed parts 400.
- Oversized body/part/classified type returns 413.
- Binary invalid/type mismatch 400; no record created.
- Stored filename/path is safe and appears under uploads/date.
- Storage/history failures never return success.
- Dropping handler future during owned mutation completes safely.
- Deletion overlapping upload cannot resurrect metadata after deletion.
- Serving correct agent succeeds with canonical type/disposition/nosniff/no-store.
- Foreign agent and deleted session refuse without ownership disclosure.
- Missing/modified file produces controlled error.
- Header injection/Unicode filename does not inject headers.
- Legacy metadata-only session message remains compatible.
- New session/event attachment metadata contains ID/MIME/path/size but no bytes.
- OpenAPI contains both routes and attachment schema.

**Steps and commands:**

- [ ] Write in-process route tests, no real server.
- [ ] Add contracts and owned route handlers.
- [ ] Wire routes/OpenAPI/projections, then run focused suites.
- [ ] Controller checks mutation/deletion race tests before signed commit.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- routes::tests::attachments 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- openapi 2>&1 | Select-Object -Last 30
```

Expected: route/auth/contract tests pass.
Commit suggestion: `feat(daemon): expose owner-authorized attachment routes`.

---

### Task 6: Durable attachment inputs for queue, steer and Telegram

**Owner:** routes/run implementer; concurrency-sensitive task.
**Depends on:** Tasks 1, 3, 5.

**Files:**

- Create `agent_runs/attachments.rs` and `agent_runs/attachment_tests.rs`.
- Modify `agent_runs.rs` for module/helper wiring only.
- Modify `agent_runs/queue.rs` and focused queue/steer tests.
- Modify `routes/runs.rs` and runs route tests.
- Modify `runs/ledger.rs` for defaulted steer IDs.
- Create `connectors/runtime/attachments.rs` for accepted-content helper.
- Modify `connectors/runtime.rs` for helper wiring only.
- Modify `control_plane_store.rs` for snapshot version/backup compatibility.
- Controller owns relevant handover/master-plan changes.

**Interfaces:**

```text
AcceptRun gains attachment_ids: Vec<String>
RunSteer gains attachment_ids: Vec<String>  // serde(default)

resolve_run_attachments(store, agent_id, session_id, attachment_ids)
  -> Result<Vec<AttachmentRecord>, ApiError>

attachment_content(text, records, existing_metadata) -> Content
  saved refs + public metadata + original input text + generated notes

web_start_with_attachments(..., text, key, skill, attachment_ids)
  -> QueuedRunStart

Existing web_start/web_start_with_skill remain wrappers with empty IDs.
Accepted Telegram helper hydrates Content refs from accepted ledger input,
then enters existing delivery/commit path without changing outbound transport.
```

**Rules:**

- Validate max10 unique ordered IDs, same agent and same exact session.
- Validate inside acceptance transaction; route-only validation is insufficient.
- Reject empty ID, oversize ID, duplicates and unknown/foreign IDs with 400.
- Keep ID syntax bounds named and consistent with generated opaque IDs.
- Existing empty-text-plus-no-IDs rejection remains.
- Empty text with valid IDs is accepted.
- RunInput.text stores original text, not generated notes.
- RunInput.attachment_ids is populated at acceptance before persistence.
- Key equality includes session/raw text/ordered IDs/skill for both run and steer.
- Same key/different skill must no longer silently reuse old run.
- Replay resolution happens before new scoped-record/file existence checks.
- A retry of committed input keeps its existing result even after file loss.
- A new input always resolves history ownership under transaction.
- Session deletion remains 409 while its run is queued/running/awaiting approval.
- No copied file bytes in accepted records, snapshots or steering inbox.
- Steer content retains refs and public metadata, not just appended notes.
- Taken-steer extraction recovers raw text and IDs from trusted metadata.
- Leftover/stop/failure hand-on records retain IDs and original input text.
- New starts resolve ledger IDs again against the same session.
- Do not hand an attachment into another session on restart/leftover recovery.
- Restart recovery remains interrupted-before-start/during-run per current policy.
- IDs remain inspectable in interrupted ledger records for manual resend.
- Browser-origin send into Telegram retains attachment content in daemon history.
- Existing Telegram reply delivery is still textual; no Telegram media-send feature.
- Existing direct Telegram owner text APIs remain wrappers with empty attachments.
- Preserve connector lifecycle/send-lock/delivery rollback ordering.
- Never read history while holding a std mutex.
- Do not add extra long awaits under connector lifecycle lock.
- No attachment metadata from older arbitrary Content API establishes trusted ownership.

**Snapshot compatibility:**

- Increment control-plane snapshot version from 10 to 11 for defaulted steer IDs.
- Add named `.pre-attachments.bak` backup branch for v10→v11.
- Preserve older `.pre-*` backup branches and Postgres backup key behavior.
- Old snapshots deserialize missing steer IDs as empty.
- Future snapshot version refusal remains fail-closed.
- Do not migrate history SQL schema, whose record columns already support this.
- Tests assert backup selection for v10 and all existing historical branches.
- README/handover explain rollback requires the pre-attachments backup.

**Tests:**

- Empty text with valid one/many attachments accepted.
- Unknown/wrong-agent/wrong-session/duplicate/eleven IDs rejected.
- Accepted ledger and start Content contain same IDs/refs/MIME.
- Text note mentions read_file; document note says not automatically read.
- Same key/input returns original, exactly one accepted run.
- Changed IDs/order/text/skill with same key returns 409.
- Same input retry succeeds after uploaded file disappears.
- Same input retry after record deletion follows existing session/deletion policy.
- Accepted persistence failure leaves no false accepted run/event.
- Queue ordering unchanged across two attached messages.
- Steer consumed by runtime retains attachment refs and metadata.
- Unread steer hand-on preserves IDs/raw text and does not duplicate notes.
- Stop/failure leftovers preserve reference input for Send again.
- Restart restores interrupted record IDs without automatically rerunning.
- Telegram accepted owner input carries refs; reply delivery queue remains intact.
- Concurrent upload/deletion/acceptance respects transaction serialization.
- Old snapshot fields default and v10 backup is selected.

**Steps and commands:**

- [ ] Write ledger/acceptance/steer regressions before integration.
- [ ] Implement helpers and focused wiring; preserve existing wrappers.
- [ ] Add snapshot bump/backups and compatibility tests.
- [ ] Run queue/steer/attachment/connector owner focused tests.
- [ ] Return exact race-test evidence and touched paths.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- attachment 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- queue 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- steer 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- owner_send 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- control_plane_store 2>&1 | Select-Object -Last 30
```

Expected: run/steer/connector/compatibility tests pass.
Commit suggestion: `feat(daemon): retain attachments through queued and steered runs`.

---

### Task 7: Request-time image hydration after context selection

**Owner:** hydration implementer.
**Depends on:** Tasks 2, 4, 6.

**Files:**

- Create `attachments/model.rs` and focused model tests.
- Modify `state.rs` runtime adapter creation with helper wiring.
- Modify `agent_runs.rs` only if runtime setup needs helper wiring there.
- Modify core context-window tests only for necessary regressions.
- Preserve metering/approval/tool/cancellation wrapper behavior.

**Interface:**

```text
AttachmentModelAdapter {
  inner: Arc<dyn ModelAdapter>,
  store: Arc<dyn HistoryStore>,
  workspace_root: Option<PathBuf>,
  agent_id: String,
  session_id: String,
}

impl ModelAdapter for AttachmentModelAdapter
  provider(): forwards inner.provider()
  generate(): prepares cloned request then forwards once
  stream(): prepares cloned request then forwards existing sink once

prepare_request(config, request) -> Result<ModelGenerateRequest, String>
```

**Rules:**

- Wrap adapters for a specific runtime/session; no global filesystem resolver in core.
- Capture root/session context from runtime construction under existing lock order.
- Adapter wrapper is host code; body rendering stays in reusable adapters.
- Model requests already passed `context_window::select_context` before this boundary.
- Defensively enforce four-image bound in wrapper even for older/manual requests.
- Keep newest images; older ones become existing path/name placeholders.
- No image reads for vision=false or missing table row.
- For true vision, resolve trusted metadata IDs through scoped history.
- Confirm record agent/session/path/MIME before reading.
- Use bounded hardened `read_attachment_bytes`, never raw fs::read.
- Encode with daemon's existing base64 dependency.
- Substitute cloned image attachment data with matching transient data URI.
- Never write hydrated request back to runtime history/state/Content source.
- At most four 10-MiB images; encoded upper bound is named and checked.
- File data URI errors/logs must not include payload contents.
- Missing/changed/root-mismatched files become safe text references.
- Legacy arbitrary path refs without trusted IDs are text fallbacks.
- No external URL fetching or path-only trust bypass.
- Strip attachmentRecords private authority metadata from provider-bound cloned text.
- Existing user text and text/document notes remain intact.
- Preserve stream TextDelta/Usage/Final frames and cancellation semantics.
- No added tool/model call or extra usage row.
- Secondary title/compaction calls remain text-only and retain existing metering.
- Restart-restored runtime gets the same scoped wrapper on construction.
- Do not hold any state/std lock while reading files or awaiting inner adapter.
- Bounds prevent a retained transcript with many files from reading them all.

**Tests:**

- Capturing adapter sees transient prepared image; original Content retains path.
- Stored snapshot/history lacks base64/data URI after successful/failed run.
- Streaming path forwards usage/final/deltas without double calls.
- Vision false/unknown does not invoke file reader.
- Four newest images hydrated; older references are text only.
- Selected context dropping a turn also drops its potential file reads.
- Text/document attachments are not hydrated as images.
- Unknown legacy paths are fallback only.
- Foreign-session record cannot be read through forged metadata.
- Missing/modified image adds note and preserves remaining turn.
- Workspace switch does not read a prior record from the new root.
- Encoded-size bound and read-size bound hold.
- Secondary title/compaction usage remains unchanged.

**Steps and commands:**

- [ ] Write capturing-adapter and reader-count tests.
- [ ] Implement wrapper and scoped runtime wiring.
- [ ] Run attachment/context tests and compile all daemon paths.
- [ ] Inspect source/history snapshots for request-only representation.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-daemon --lib -- attachment 2>&1 | Select-Object -Last 30
$env:CARGO_INCREMENTAL='0'
cargo test -p anima-core --lib -- context_window 2>&1 | Select-Object -Last 30
```

Expected: hydration and existing cap tests pass.
Commit suggestion: `feat(daemon): hydrate selected images only for model requests`.

**Review checkpoint B:** batch-review Tasks 5–7, focusing on authority,
transaction/drop safety, exact replay equality, steer durability and byte leakage.

---

### Task 8: SDK attachment multipart and binary clients

**Owner:** SDK/web implementer.
**Depends on:** Tasks 5–6 contracts.

**Files:**

- Create `packages/sdk/src/attachments.ts`, `attachments.spec.ts`.
- Modify `client.ts`, `client.spec.ts`, `index.ts`.
- Modify `sessions.ts` attachment metadata type, with compatibility tests.
- Modify `runs.spec.ts` to confirm attachment IDs are sent unchanged.
- No runtime dependencies.

**Interfaces:**

```typescript
export interface Attachment {
  id: string;
  name: string;
  mime: string;
  size: number;
  kind: 'image' | 'text' | 'document';
  path: string;
  createdAtMs: number;
}

export class AttachmentsClient {
  upload(agentId: string, sessionId: string, file: Blob, name: string,
    options?: { signal?: AbortSignal }): Promise<{ attachment: Attachment }>;
  download(agentId: string, attachmentId: string,
    options?: { signal?: AbortSignal }): Promise<Blob>;
}

DaemonClient.requestBlob(path: string, init?: RequestInit): Promise<Blob>
DaemonClient.attachments: AttachmentsClient
```

**Rules:**

- Existing requestJson FormData handling is reused, not JSON-stringified.
- Append exactly one part named `file`, with the caller's filename.
- Do not set multipart Content-Type manually; browser/fetch sets boundary.
- Encode each agent/session/attachment segment with encodeURIComponent.
- Forward AbortSignal on uploads and downloads.
- Binary request helper retains existing connection and HTTP error types.
- Non-2xx binary response uses existing error body reader before throwing.
- No unconditional `response.blob()` on failure.
- Reuse client base URL and fetch injection for downloads.
- SDK has no browser-only XMLHttpRequest requirement.
- SDK upload promise represents success/failure; byte progress belongs to web helper.
- Existing SessionMessageAttachment type/name remain required.
- Add optional upload metadata fields so legacy entries stay typed.
- Export public types/client from existing barrel patterns.
- Do not alter auth policy or put credentials in URLs.

**Tests:**

- Exact encoded multipart route and one FormData `file` part.
- Filename survives FormData; content type has no JSON/manual boundary.
- Signal forwarded and abort behavior follows existing request helpers.
- Success envelope typed correctly.
- Download encoded route returns Blob of correct bytes/type.
- HTTP error parsed and thrown with status/body.
- Transport/read failure produces DaemonConnectionError.
- Empty baseUrl and custom base prefix work.
- Client constructor exposes attachments resource.
- Legacy session attachment remains valid.
- Run start retains attachmentIds ordered unchanged.

**Steps and commands:**

- [ ] Write mock-fetch client tests.
- [ ] Implement resource and requestBlob helper.
- [ ] Run SDK tests/typecheck through Nx.
- [ ] Report exact test counts and owned paths.

```powershell
bun x nx run-many -t test,typecheck -p @animaOS-SWARM/sdk --skipNxCache
```

Use the common Nx environment and require_escalated shell mode.
Expected: SDK test/typecheck pass.
Commit suggestion: `feat(sdk): add multipart upload and attachment download client`.

---

### Task 9: Web upload lifecycle, progress, send queue and recovery

**Owner:** SDK/web implementer.
**Depends on:** Task 8.

**Files:**

- Create `apps/web/src/lib/attachments.ts` and tests.
- Create `hooks/useAttachments.ts` and focused hook tests.
- Modify `lib/daemon-api.ts`, tests, `lib/types.ts`.
- Modify `hooks/useSessionSends.ts` and tests.
- Modify `hooks/useSessionCommands.ts` and tests.
- Create focused session-attachment integration hook for new-chat session setup.
- `ViewHarness.tsx` receives wiring/prop composition only.

**Interfaces:**

```text
AttachmentDraft:
  localId, name, kind?, previewUrl?, status: uploading|ready|error
  progress: number|null
  attachment?: SDK Attachment
  error?: string

uploadAttachmentWithProgress(agentId, sessionId, file, options):
  options = { signal, onProgress }
  Promise<Attachment>

useAttachments({ chatKey, agentId, sessionId, ensureSession, enabled }):
  items, readyAttachments, uploading, error
  add(files), remove(localId), clear(), restore(attachments)
```

**Rules:**

- Web progress uses browser XMLHttpRequest, native FormData and upload events.
- Share relative encoded path construction with SDK contracts; no hardcoded host port.
- Existing same-origin auth/session policy is retained.
- If custom owner headers are required, use the application's existing transport policy.
- Do not create a separate secret store in the upload hook.
- Validate response as public attachment metadata before accepting it.
- XHR failures map to existing user error handling; progress never implies persistence.
- Upload progress reaches ready only after server success response.
- At most ten ready/uploading slots in the draft.
- Validate obvious type/size client-side, but server remains authoritative.
- Files with accepted image MIME may get local blob previews; SVG never does.
- New chat must create exactly one session before uploading.
- `ensureSession` shares/coalesces the existing session-start path.
- It may create an empty chat; do not send text just to obtain a session ID.
- Upload cannot silently bind to a later-selected session.
- Capture chatKey/session per operation; stale completion is ignored.
- Switch/disable/unmount aborts pending uploads and revokes local URLs.
- Remove uploading chip aborts its request and ignores late completion.
- Remove ready chip affects draft only; no daemon deletion API.
- Keep ready metadata per chat/session state while user navigates.
- Sending is disabled while uploads are pending or failed chips remain unresolved.
- Attachment-only send works once all selected attachments are ready.
- Clear successfully queued attachment draft only after it is captured into send payload.
- Failed acceptance/recovery restores IDs/metadata, not bytes.
- Extend SessionSend/PersistedSend with optional attachmentIds and display metadata.
- Decode old pendingSends entries as empty attachments.
- Bound recovered arrays/strings and reject malformed IDs/metadata.
- Existing retry key and ordered ID list stay unchanged across automatic retries.
- Existing no-autosend recovery policy remains.
- sendAgain uses run.input.attachmentIds and raw run.input.text.
- Recovery/sendAgain never assigns old-session IDs to a new chat.
- Slash local commands do not silently consume selected attachments.
- For a local slash command with attachments, keep draft chips and show named guidance.
- `/skill` sending may include attachments; existing skill/steer/Telegram rules apply.
- Queue/steer paths carry attachments, including restored messages.
- Existing offline/disabled behavior remains.
- toChatMessage and event conversion retain attachment metadata.
- Do not trust attachment.path as a browser href.

**Tests:**

- XHR progress, success/error/abort; no manually-set boundary header.
- Multiple files reserve ten-slot cap before async completion.
- Empty/oversized/unsupported obvious files fail inline.
- New chat coalesces session creation for multi-file selection.
- Session switch ignores stale response and aborts transport.
- Removal cancels pending upload and does not create ready chip later.
- Blob URLs revoke on removal/unmount/switch, not while still displayed.
- Attachment-only send carries IDs with original retry key.
- Retry retains same ordered IDs; queue lanes remain ordered.
- Recovery serialization retains IDs and metadata, no bytes/blob URL/File object.
- Legacy recovery works with no attachments.
- Malformed recovery is rejected/bounded.
- Failed send retains restorable attachment metadata.
- sendAgain retains IDs; never remaps another session's IDs.
- Slash command keeps chips rather than discarding them.
- toChatMessage/event message preserves metadata fields.
- No act/console noise; async state updates are awaited in tests.

**Steps and commands:**

- [ ] Write transport/hook/queue/recovery tests.
- [ ] Implement focused upload/session hook and send payload extensions.
- [ ] Add only hook wiring in ViewHarness.
- [ ] Run web test/typecheck via Nx; inspect warnings.

```powershell
bun x nx run-many -t test,typecheck -p @animaOS-SWARM/web --skipNxCache
```

Expected: web tests/typecheck pass with pristine output.
Commit suggestion: `feat(web): retain attachment uploads through sends and recovery`.

---

### Task 10: Composer controls and authenticated attachment previews

**Owner:** SDK/web implementer.
**Depends on:** Tasks 8–9.

**Files:**

- Create `components/attachments/AttachmentControls.tsx` and tests.
- Create `AttachmentChips.tsx`, `MessageAttachments.tsx` and tests.
- Create `hooks/useAttachmentPreview.ts` and focused tests.
- Modify `components/ChatScreen.tsx` for controls/preview wiring.
- Modify `components/sessions/SessionView.tsx` and tests for state props.
- Modify relevant CSS with bounded attachment layout styles.
- Modify `ViewHarness.tsx`/test for wiring only if needed.

**Interfaces:**

```text
AttachmentControls:
  items, disabled, onAdd(files), onRemove(localId)
  paperclip file picker; drop and paste handlers

MessageAttachments:
  agentId, attachments: readonly SessionMessageAttachment[]
  legacy entries without IDs render as text chips

useAttachmentPreview(agentId, attachmentId, mime, enabled):
  url: string|null, loading, error
  authenticated SDK Blob fetch + object URL lifecycle
```

**Rules:**

- Paperclip button has accessible name and focus behavior.
- File picker allows multiple selection within ten-slot cap.
- Drop accepts files only and does not hijack ordinary dropped text/links.
- Paste handles file items/images while preserving ordinary pasted text.
- Clipboard file deduplication prevents a single item being added twice.
- Hidden file input resets after selection so same file can be selected again.
- Chip shows safe filename, size/kind, progress/error and Remove action.
- Progress has accessible label/value where byte total is known.
- Pending progress can be indeterminate; no fake upload percentages.
- Drag highlight is local to composer; never a whole-window event trap.
- Input stays keyboard usable and supports existing queue/steer/stop shortcuts.
- Existing Stop remains enabled even when attachments are uploading.
- Model-written filename/name text uses revealInvisible/plain text rendering.
- No innerHTML from filenames, paths, MIME or server error strings.
- Display image thumbnails using authenticated Blob requests.
- Direct `<img src=/api/...>` must not bypass owner/API-key request policy.
- Preview URLs are generated blob URLs, never credentialized daemon URLs.
- Fetch/revoke on agent/ID changes and unmount; ignore stale results.
- One image-load failure shows file chip and retry/download action safely.
- Download uses authenticated SDK Blob then temporary object URL anchor.
- Download name is server-approved name; no arbitrary response path href.
- Revoke temporary download URL after browser action, with bounded lifecycle.
- Text/document remain file chips; no inline document or HTML rendering.
- Legacy attachment metadata without IDs gets no fake download link.
- Message attachments render even if message.text is empty.
- Image dimensions/layout are bounded; no transcript width explosion.
- Keep attachment metadata on pending bubbles where available.
- Existing Copy/Save actions and transcript grouping remain unchanged.

**Tests:**

- Picker adds File list and resets input.
- Drop files adds; drop ordinary text preserves behavior.
- Paste image adds once; plain text paste remains normal.
- Disabled/read-only/offline states prevent upload initiation.
- Chip removal and progress accessible names/values.
- Uploading chips block send but not Stop.
- Attachment-only composer can send when ready.
- Empty-text message still shows thumbnails/chips.
- Preview uses SDK Blob fetch with proper agent/ID, not raw path URL.
- Stale fetch ignored and URLs revoked at correct boundaries.
- Failed auth/preview shows safe error without leaking file path or token.
- Download fetches Blob and uses correct safe filename.
- Legacy entry is text-only chip.
- Hidden-character filename visible; HTML-like name stays literal.
- Existing SessionView/ChatScreen behavior and ViewHarness tests pass.

**Steps and commands:**

- [ ] Write component/preview tests before wiring.
- [ ] Implement focused controls/chips/previews.
- [ ] Wire Composer/Bubble/SessionView with small changes.
- [ ] Run web test/typecheck; preserve pristine console output.

```powershell
bun x nx run-many -t test,typecheck -p @animaOS-SWARM/web --skipNxCache
```

Expected: component and existing console tests pass.
Commit suggestion: `feat(web): add composer attachments and authenticated previews`.

---

### Task 11: Browser dictation and Read aloud lifecycle

**Owner:** voice implementer.
**Depends on:** stable Composer/SessionView interfaces from Task 10.

**Files:**

- Modify `hooks/useBrowserDictation.ts` and existing tests.
- Create `components/voice/DictationControl.tsx` and tests.
- Create `hooks/useReadAloud.ts` and tests.
- Create `components/voice/ReadAloud.tsx` and tests.
- Modify `ChatScreen.tsx`, `SessionView.tsx` for small wiring only.
- Preserve onboarding `WorkspaceBrief` dictation behavior.

**Interfaces:**

```text
useBrowserDictation(onTranscript) retains current return fields;
  add cancel(): abort/detach immediately, clear interim, return idle

DictationControl:
  disabled, conversationKey, onTranscript(text)
  uses existing hook, supported-only microphone control

useReadAloud(conversationKey):
  supported, speakingMessageId, error
  read(messageId, text), stop()

ReadAloud:
  messageId, text, state/actions from session-owned useReadAloud
```

**Rules:**

- Reuse existing speech recognition hook, not a new STT provider.
- Supported detection includes SpeechRecognition/webkitSpeechRecognition.
- Hidden control when unsupported; no disabled placeholder promising voice.
- Final transcript appends to latest draft with a single separating space.
- Interim preview is separate from draft and never submitted automatically.
- Repeated final result index does not duplicate committed text.
- Draft edits during recognition are respected by callback refs.
- Stop during starting cancels safely; stop listening permits final results.
- Session switch/disable/unmount cancels immediately and detaches handlers.
- Late recognition result/error/end from old instance cannot mutate new draft.
- Permission/audio/no-speech/network/start errors remain inline and named once.
- Dictation never auto-sends or starts a daemon/model call.
- Do not request microphone until owner clicks the supported control.
- Read aloud appears on assistant messages with nonempty textual replies.
- Hidden when speechSynthesis or SpeechSynthesisUtterance is absent.
- One speech controller per active session; not one competing global utterance per bubble.
- Read starts by cancelling the previous utterance and resets ownership generation.
- Toggle current speaking message stops; another message replaces it.
- Stale onend/onerror handlers cannot clear a newer utterance.
- Session switch/unmount cancels speech and detaches handlers.
- Speech state/error tracks actual browser callbacks, not guessed duration.
- Speech text comes from visible/plain message text, with hidden chars revealed.
- No HTML interpretation or synthesis of tool metadata/hidden system prompts.
- No audio recording/upload, TTS provider, voice storage or extra settings page.
- Button labels expose Read aloud/Stop reading and speaking state accessibly.
- Existing Copy/Save to memory actions stay intact.

**Tests:**

- Unsupported browser hides both controls.
- Existing onboarding dictation tests remain valid.
- Start/listen/stop/interim/final recognition flow.
- Duplicate final result does not duplicate draft.
- Latest draft edits survive final transcript append.
- Switch/disable/unmount aborts recognition and ignores late events.
- Recognition permission/start errors inline; no console noise.
- Read aloud calls speak with assistant text only.
- Starting second message cancels first once and ignores stale end/error.
- Same-message toggle stops and clears speaking state.
- Session switch/unmount cancels and detaches.
- Speech start/end/error callbacks update UI correctly.
- Read aloud ignores empty/tool/system messages.
- Hidden Unicode is revealed consistently with visible response.
- Fake browser APIs/fake timers only; no real microphone or speech service.

**Steps and commands:**

- [ ] Write mocked recognition/synthesis lifecycle tests.
- [ ] Add cancellation API without breaking existing onboarding caller.
- [ ] Implement focused dictation/read-aloud controls and session ownership.
- [ ] Wire props and run web tests/typecheck.

```powershell
bun x nx run-many -t test,typecheck -p @animaOS-SWARM/web --skipNxCache
```

Expected: voice and existing onboarding/console tests pass without warnings.
Commit suggestion: `feat(web): add dictation and browser read aloud to sessions`.

---

### Task 12: Reviews, one fix wave, local gates and publication

**Owner:** controller.
**Depends on:** all implementation tasks.

**Review process:**

- [ ] Verify review batch A findings cover core/providers/history/files together.
- [ ] Verify review batch B covers routes/queue/steer/Telegram/hydration together.
- [ ] Request one final web+SDK review of Tasks 8–11.
- [ ] Record accepted/rejected findings and rationale in progress.md.
- [ ] Dispatch one bounded fix wave with explicit file ownership.
- [ ] Validate the accepted fixes with focused regressions.
- [ ] Do not repeat whole review cycles without a newly observed blocker.
- [ ] Verify all task commits are signed, owned and preserved.

**Review questions:**

- Is file authority established by held capability handles and private record binding?
- Can upload/history/deletion cancellation create successful orphan metadata?
- Are session-scoped IDs preserved through every queue/steer/recovery path?
- Does exact replay equality compare IDs/order/skill, not generated notes?
- Can binary images leak into snapshots/history/errors/logs?
- Do unknown/nonvision models receive clear text without file reads?
- Are every provider's stream/generate request builders wired?
- Does owner auth cover image previews and downloads without query tokens?
- Can stale upload/recognition/synthesis callbacks affect a different chat?
- Do legacy MIME-free snapshots/messages/recovery entries remain usable?

**Required final gates, current tree:**

```powershell
$env:NX_DAEMON='false'
$env:NX_ISOLATE_PLUGINS='false'
$env:NX_NO_CLOUD='true'
$env:NX_NATIVE_FILE_CACHE_DIRECTORY=(Join-Path (Get-Location) 'node_modules/.cache/nx-native-m9')
$env:CARGO_INCREMENTAL='0'
bun x nx run core-rust:test --skipNxCache
```

```powershell
$env:NX_DAEMON='false'
$env:NX_ISOLATE_PLUGINS='false'
$env:NX_NO_CLOUD='true'
$env:NX_NATIVE_FILE_CACHE_DIRECTORY=(Join-Path (Get-Location) 'node_modules/.cache/nx-native-m9')
$env:CARGO_INCREMENTAL='0'
bun x nx run rust-daemon:test --skipNxCache
```

```powershell
$env:NX_DAEMON='false'
$env:NX_ISOLATE_PLUGINS='false'
$env:NX_NO_CLOUD='true'
$env:NX_NATIVE_FILE_CACHE_DIRECTORY=(Join-Path (Get-Location) 'node_modules/.cache/nx-native-m9')
bun x nx run-many -t test,typecheck,build -p @animaOS-SWARM/sdk @animaOS-SWARM/web --skipNxCache
```

```powershell
$env:CARGO_INCREMENTAL='0'
cargo fmt --all --check
```

```powershell
$env:NX_DAEMON='false'
$env:NX_ISOLATE_PLUGINS='false'
$env:NX_NO_CLOUD='true'
$env:NX_NATIVE_FILE_CACHE_DIRECTORY=(Join-Path (Get-Location) 'node_modules/.cache/nx-native-m9')
bun x nx format:check --base=origin/main
```

- Nx calls use require_escalated shell mode as diagnosed by the controller.
- Core-rust test is necessary because daemon tests do not execute dependency unit tests.
- Record actual pass/ignored counts; no assumed baseline numbers.
- Do not start ignored database tests just to increase passing count.
- Local gates are the verification; known Nx Cloud CI failure is not a pass.
- If full format detects unrelated pre-existing nx.json/anima.yaml drift, report it
  precisely and run explicit touched-path formatting evidence without staging them.
- A required gate failure is not completion; fix owned failures and rerun affected gate.
- Do not broaden tests repeatedly after all relevant gates pass.

**Formatting:**

- Run rustfmt on owned files only when workspace check finds owned drift.
- Use Prettier via `bun x nx format:write --files=<explicit touched files>`.
- Recheck git diff/stat; no unrelated line-ending normalization is staged.
- No `git add -A`, `git add .`, `git commit -a` or forbidden git recovery commands.
- Signed commit waiting for passphrase stays staged; ask owner to unlock.

**Documentation and publication:**

- [ ] Update master-plan M9 status row with plan path and completed evidence.
- [ ] Update handover Start here: M9 PR open, snapshot v11, gates, resume points.
- [ ] Update `hosts/rust-daemon/README.md` for upload limits/path/root binding and backup.
- [ ] Note saved-not-read document support and browser voice support limits.
- [ ] Note no live-provider/manual browser acceptance was performed.
- [ ] Note no VPS/database/container deployment was performed in M9.
- [ ] Stage docs/implementation by explicit path; exclude nx.json/anima.yaml.
- [ ] Commit signed documentation and push feat/companion-console-m9.
- [ ] Open PR targeting main with concrete behavior and local validation evidence.
- [ ] Attach created PR through Codex artifact tool.
- [ ] Do not merge until owner explicitly says merge.
- [ ] Ledger/handover say M10 resumes after M9 merge, from new main-based branch.

**Manual evidence limits:**

- Fixtures prove provider payload shape and bounded authority behavior.
- Mock browser tests prove event/lifecycle wiring, not microphone/browser service quality.
- Local tests do not prove a real provider accepts images for every configured model.
- A live-provider attachments/voice smoke test remains manual acceptance.
- Owner forbids real daemon/dev server/container starts in this task.
- These limits are recorded as evidence limits, not excuses to leave implementation partial.

---

## Top five risks for controller rulings

Exactly five risks; no separate pre-flight audit is requested.
The controller appends `Controller rulings on the risks (binding)` after reviewing
these choices and records the corresponding decisions in the local ledger.

### R1: Filesystem replacement, collision and workspace authority

The inspected generic byte writer still uses `fs::write` after canonical checks.
That is insufficient for collision-safe uploads and does not hold the final path.

Proposed choice: capability traversal with nofollow/reparse rejection, exclusive
create with bounded suffix attempts, bounded regular-file reads, canonical root
binding and digest checks. Share generic hardening while retaining write_file's
overwrite semantics, or expose an exclusive safe sibling using the same primitive.

Cost if wrong: overwritten local files, out-of-workspace writes/reads, a changed
workspace serving unrelated content, or a required Windows-specific follow-up.

### R2: Attachment identity lost through replay, steer and hand-on

Acceptance and steering currently compare/store text; steers contain no IDs and
leftover content can already have generated notes. Adding upload alone loses input.

Proposed choice: validate ordered unique scoped IDs under transaction, persist IDs
in RunInput/RunSteer, keep daemon-owned metadata through inbox/taken/leftover paths,
and compare exact raw text/IDs/skill for retries before new availability checks.
Snapshot moves to v11 with pre-attachments backup.

Cost if wrong: duplicate or mismatched runs, cross-session file access, disappeared
attachments on Stop/restart/recovery, or silent input changes on idempotent retries.

### R3: Upload success outruns durable history or session deletion

File creation and history insertion are different persistence systems. Request
cancellation/deletion must not create durable metadata for a vanished session.

Proposed choice: owned spawned mutation holds the existing control-plane transaction
through captured root write and direct history insertion, returns 201 only after
durability, and permits harmless orphan workspace files on failure/crash. No
in-memory attachment outbox, broad cleanup or false-success retry behavior.

Cost if wrong: apparently uploaded files disappear after restart, deletion resurrects
attachment records, cancellation leaves inconsistent ownership, or cleanup deletes
someone else's replaced file.

### R4: Image bytes leak or unknown vision models get binary requests

Core/reference history cannot own host filesystem operations; adapters have no
base64 dependency. Request cloning must happen after existing context selection.

Proposed choice: daemon-scoped adapter wrapper hydrates at most four validated
selected images into request-only data URIs using existing base64. Pure provider
builders emit each provider format. Unknown/vision-false models get text fallback
without reading bytes; legacy untrusted path references remain text-only.

Cost if wrong: enormous persisted histories, uploaded secrets in logs/snapshots,
unbounded image reads, tool-message corruption, or rejected real-provider calls.

### R5: Browser uploads and voice callbacks cross session boundaries

Uploads/new-chat creation/speech APIs are asynchronous, while the owner can switch
chats, remove files, disable the composer or recover an unaccepted send.

Proposed choice: stable chat/session operation binding, coalesced ensureSession,
AbortSignal/generation guards, URL revocation, retained recovery IDs without auto
send, and session-owned recognition/synthesis cancellation. Authenticated Blob
requests own previews/downloads; no credential-bearing URLs.

Cost if wrong: files attach to the wrong chat, recognition overwrites another draft,
speech continues after navigation, memory leaks, duplicate recovered sends or
unprotected downloads.

## Controller rulings on the risks (binding)

These rulings override earlier choices. No separate pre-flight audit is required.

1. **R1 — File authority and collisions.** Use held capability directories, no-follow traversal and reparse rejection, exclusive `create_new`, bounded collision attempts and bounded regular-file reads. An exclusive safe sibling of `write_workspace_bytes` is permitted; generic `write_file` overwrite behavior must remain. Pin canonical workspace root and sha256 in private records; validate both before serving or hydration. Public responses and model notes expose relative paths only. Cost if wrong: file overwrite or disclosure; link, replacement and collision tests are mandatory.
2. **R2 — Durable inputs and replay.** Accept at most ten unique ordered IDs owned by the same agent/session under the control-plane transaction. Preserve raw text, IDs and skill in queues, steers, consumed-steer metadata, leftovers and manual recovery. Replay equality includes all three, and exact replay resolves before new file-availability checks. Snapshot v11 and the pre-attachments backup are required for new persisted steer semantics. Cost if wrong: duplicate runs or missing/foreign input.
3. **R3 — Upload durability and deletion.** The spawned mutation captures workspace authority, serializes against session/agent deletion and completes file creation plus direct history persistence before 201. No state/std lock crosses disk or async store work. Storage failure must not report success; harmless orphan files are accepted and never cleaned through a path whose identity may have changed. Cost if wrong: missing uploads or resurrected metadata.
4. **R4 — Request-only image bytes.** Hydrate a cloned selected request in daemon code only, with the existing newest-four cap and bounded bytes. Every pure provider serializer supports its required image shape. Missing model entries and `vision: false` use text fallback without reading files; missing/changed files retain a safe reference and the rest of the input. No bytes enter transcripts, snapshots, logs or public metadata. Legacy path-only references are untrusted. Cost if wrong: disclosure, excessive memory or provider rejection; local fixtures do not prove live provider acceptance.
5. **R5 — Browser lifetime.** Bind asynchronous work to a stable session generation, coalesce new-session creation, abort/ignore stale upload callbacks, revoke object URLs and cancel dictation/synthesis on session changes or disabled/unmounted surfaces. Authenticated blob retrieval carries owner credentials in headers, never URLs. Manual retry retains IDs with no automatic resend. Cost if wrong: wrong-session sends, lost drafts or stale speech.

**Process adaptation:** The owner explicitly authorized Codex subagents after Claude reported signed out. Use the available subagents with bounded contexts; reserve the most careful review for file authority and transaction/steer boundaries. The controller owns explicit staging and signed commits. Batch A and B reviews record findings; accepted findings are resolved in one final fix wave, then the required local gates. M10 starts only after the owner merges M9 and a new milestone branch can be cut from main.
