# M56 selected audio transcription implementation plan

Status: bound plan on `g9000/companion-native-voice-m56`, from verified
M55/PR61 main `285cb59813916e3f3a2293fff4b6dbc83f6d404d`; exact M55 source and
merged tree are recorded below. No M56 implementation or acceptance is claimed.
Publish this plan/master/handover, then record existing-handler WAV upload RED
before production source or dependency feature changes. The historical proposal
below is superseded by **Controller rulings on the risks (binding)** at the end.
No separate pre-flight audit or user check-in. No actual provider/microphone/
ordinary daemon/account/DB server/container/deployment activation.

## Historical lean proposal

# M56 held proposal: selected audio transcription

Status: planning only. Held until M55 has a verified merge. This file is private
execution material; it is not PAR-07 acceptance, public roadmap evidence, or a
release authorization. No source, branch, manifest, public documentation,
dependency, provider, model, microphone, or service activation is authorized by
this plan.

## Proposed slice

Add a user-selected, bounded audio attachment path that produces a draft
transcript for review in the existing composer. Keep the first accepted format
to one fixture-friendly audio container (recommend WAV/PCM) and do not add
always-on recording, live streaming, speech output, automatic send, or
microphone capture. The user explicitly selects an uploaded audio attachment,
requests transcription, reviews/edit the returned text, and separately submits
the message. Ship one real production speech-to-text HTTP adapter for an
explicitly configured OpenAI API account. M56 acceptance still uses
deterministic fixtures and finite loopback HTTP peers only: no actual provider
request, model call, microphone, or service activation is released or claimed.

This is the clearest next PAR-07 gap. Current browser speech recognition only
offers live dictation through the browser service, and read-aloud uses browser
speech synthesis. Neither is daemon-native speech-to-text; audio uploads are
not among the current attachment kinds. A fixture-backed service seam plus
composer review would create one real useful workflow without asserting that a
provider works or that voice quality is accepted.

Scope candidate: daemon-owned audio attachment validation and owner/session
binding; one production transcription adapter plus deterministic fake; a typed
owner route that takes an explicit selected attachment ID; and a composer
action that shows progress/result/error and lets the owner edit or discard the
transcript. Freeze interfaces only after M55 settlement and root risk rulings.

## Source evidence and reuse decision

Repository instructions in `AGENTS.md` and `docs/companion/parity-roadmap.md`
keep reusable engine code under `packages/*`, host process/HTTP under
`hosts/*`, and host selection in `tools/workspace-dev`. PAR-07 lines 171–177
calls for provider-neutral transcription/speech ports, explicit microphone
permission, selected voice attachments through the bounded attachment
lifecycle, duration/bytes/lifetime/output/retention bounds, cancellation,
clear unsupported states, no hidden upload or speech authority, and mocked
fixtures before consented real activation. No activation belongs in this slice.

Current browser speech entry points:

- `apps/web/src/components/voice/DictationControl.tsx` uses
  `useBrowserDictation` and appends final browser-recognized text to the
  composer. It exposes start/stop, interim text, permission/device/language/
  network errors.
- `apps/web/src/hooks/useBrowserDictation.ts` uses the browser's
  `SpeechRecognition` or `webkitSpeechRecognition`, ties lifecycle to the
  active conversation/disabled state, aborts on cleanup, and requests no
  daemon endpoint.
- `apps/web/src/components/voice/ReadAloud.tsx` delegates to the browser
  `ReadAloudController`; it is speech output convenience, not a native TTS
  service.
- `apps/web/src/components/ChatScreen.tsx` and
  `apps/web/src/components/sessions/SessionView.tsx` place these controls in
  the existing composer/session boundary. Preserve those cancellation and
  hidden/inert-conversation rules.

Current daemon attachment path:

- `hosts/rust-daemon/src/attachments/mod.rs` defines only Image, Text and
  Document kinds, with 10 MiB, 1 MiB and 25 MiB kind limits. Upload is one
  multipart part; form overhead and filename are bounded.
- `hosts/rust-daemon/src/attachments/validation.rs` classifies a strict
  extension/MIME allowlist and validates content envelopes. Audio formats are
  rejected today.
- `hosts/rust-daemon/src/routes/attachments.rs` has owner/session-scoped upload
  and retrieval, stores the record in `HistoryStore`, and returns public
  metadata. `routes/contracts/attachments.rs` deliberately projects private
  identity away. `attachments/files.rs` owns bounded read/save and digest
  verification; `attachments/metadata.rs` binds message-selected IDs.
- `hosts/rust-daemon/src/attachments/model.rs` hydrates only verified
  message-selected user attachments; it currently passes through non-image
  attachments and handles documents separately. Transcription must be an
  explicit action and must not silently transform every audio file during
  model hydration.
- `hosts/rust-daemon/src/routes/tests/attachments.rs` and
  `hosts/rust-daemon/tests/document_worker.rs` demonstrate the existing fixture
  and real-worker test boundaries. Reuse attachment identity/read helpers where
  they fit; avoid a second storage or attachment authority system.

Provider boundary evidence:

- `packages/core-rust/crates/anima-model-adapters` implements text generation
  through `ModelAdapter`; repository search found no transcription/audio API.
- `hosts/rust-daemon/src/runtime_model.rs` builds general text-provider
  credentials from `provider_config_from_env` and `ProviderDefinition`; that
  path reads API keys and optional base URLs from environment variables. It
  does not establish a general provider API-key vault.
- `hosts/rust-daemon/src/chatgpt_auth.rs` resolves ChatGPT credentials through
  the OS-keyring-backed `OAuthVaultBackend` for the ChatGPT Responses API.
  `connectors/oauth_apps.rs` has a purpose-specific keyring abstraction for
  OAuth app credentials. Neither is a generic provider API-key vault.
- The adapter must accept only an explicitly configured OpenAI Platform API
  key through the approved server-side provider configuration path, and fail
  closed if absent. Never treat ChatGPT subscription/session OAuth tokens as
  OpenAI Platform API keys or send them to `api.openai.com`; ChatGPT
  subscription access does not imply API billing. If root requires keyring
  storage for the Platform API key before shipping, bind a separate credential
  storage change first. Do not invent generic OAuth support. Never expose a key
  in browser DTOs, transcript responses, logs, or attachment metadata.
- `hosts/rust-daemon/Cargo.toml` already includes async `reqwest` with rustls,
  `serde`, `tokio`, `sha2`, `base64`, and `zeroize`; declared reqwest features
  omit `multipart`. Prefer a small bounded multipart body built from validated
  WAV bytes, or justify any dependency after exact contract review.
  `packages/core-rust/crates/anima-model-adapters` is the natural home for a
  reusable HTTP transcription adapter if its boundary accepts a key and fixed
  endpoint without host/vault types. Daemon owns config/key resolution,
  endpoint authority, attachment authorization and route.

Installed public Hermes source was inspected only under
`C:/Users/leoca/AppData/Local/hermes/hermes-agent/tools/` and its root
`LICENSE`. Relevant candidates: `transcription_common.py` (formats, provider
families, common limits/results), `transcription_tools.py` (dispatch,
validation, local/cloud route split), `transcription_cloud.py` (provider
request adapters), `transcription_chunking.py` (provider-specific limits and
segments), `transcription_command.py` (configured command/plugin extensibility),
and `voice_mode.py`/`voice_client_config.py` (recording/live-session lifecycle).
The installed package root declares MIT and `Nous Research` copyright.

One-adapter choice: OpenAI Platform audio transcription using the multipart
`audio.transcriptions.create` contract in Hermes `_transcribe_openai`; its
`transcription_common.py` pins the default model name `whisper-1` and Hermes
uses `response_format=text` for that model. Candidate exact request fields are
`model=whisper-1`, file, and `response_format=text`, subject to root's later
contract freeze, at fixed HTTPS authority
`https://api.openai.com/v1/audio/transcriptions`. Production has no arbitrary
base-URL override or cross-authority redirect. A test-only constructor may use
a loopback peer; production configuration cannot reach that override. Send
only the model, audio file and required response-format field; no prompt,
implicit language hint, chunks, conversion, fallback or provider switching.
Return a bounded transcript with provider label `OpenAI API` or `openai_api`,
distinct from ChatGPT subscription/app access.

Eligibility: the owner explicitly selects OpenAI API transcription and a
nonempty OpenAI Platform API key exists through the approved daemon-side
provider config. ChatGPT OAuth/subscription credentials, selected chat model,
another provider key, and ambient defaults do not grant eligibility. Missing
key, wrong provider/model or invalid config returns a stable unavailable state
before audio dispatch. There is no OAuth refresh, generic OAuth, implicit
managed gateway, or user-configurable endpoint.

Use existing reqwest with Bearer authorization and the fixed multipart
contract. Bound request bytes by the selected-audio cap plus multipart
overhead. Apply the rooted deadline; cancellation drops the HTTP future and
releases owned buffers/permit only after the request stops. Consume response
chunks into a bounded buffer and abort on byte cap, deadline or cancellation;
decode the streamed text as bounded UTF-8 and cap transcript length. Map all status
failures and invalid bodies to stable static errors; never return raw provider
text/body or log request/response bodies or headers. No retry, retry-after,
conversion, fallback or late-result publication. Timeout/disconnect leaves
provider completion uncertain and must not be automatically retried or
described as proof that no remote processing occurred. This plan makes no
actual provider request.

Reuse decision: a direct Python import cannot run in the Rust host and would
drag Hermes profile/configuration conventions into Anima. A long-lived Hermes
process/API adapter adds another service, lifecycle, credentials boundary,
runtime dependency and host selection path for this narrow one-request task.
Candidate is a narrow, attributed port only of WAV-envelope or bounded
response concepts if inspection proves a specific helper useful; do not copy
Hermes provider/credential dispatch. Put the reusable HTTP adapter in
`anima-model-adapters` if its interface is host-neutral; keep config/key
selection, fixed endpoint authority and attachment permissions in the daemon.
Preserve MIT notice and Anima branding in any actual port;
record exact installed revision/hash, paths, license, component notices and
local changes at implementation time. Do not port provider catalogs, dynamic
config, chunking, plugin/command hooks, polling/live sessions, TTS, or Hermes
user-profile semantics into M56. Root should compare dependencies again after
interfaces/rulings are set; current reqwest means a provider adapter may need
no new crate, but multipart API availability has not been proven.

## Interface and behavior sketch

This is a boundary proposal, not a frozen API. Prefer an explicit
`transcribe_selected_attachment(agent_id, session_id, attachment_id, request)`
owner route. Resolve the attachment server-side from the current owner/session;
never accept a filesystem path, workspace root, MIME, bytes, provider URL, or
credential from the browser. Verify the stored record's identity, kind, digest,
size and current file bytes before dispatch; use the same bounded file reader
as attachments. Keep original bytes available for download and never replace
the uploaded object with normalized/transcoded bytes.

The daemon carries owned, bounded, digest-verified audio into a host-neutral
transcription interface. Production wiring selects the sole OpenAI API adapter
only from explicit STT config and passes its key server-side. The fake returns
fixed transcript/result/error values and records invocation count, selected
attachment identity, cancellation and whether bytes were exposed. Finite local
HTTP peers exercise the real adapter request shape, status handling, streamed
response cap, timeout, cancellation and secret non-disclosure without
contacting OpenAI. Return a bounded transcript plus safe provider/status
fields; never echo auth headers, raw provider body or audio bytes. The result
is a draft only. The UI must visibly associate it with the
selected attachment, support edit/discard, and invoke ordinary send only on a
separate owner action.

Do not permanently store transcript text in attachment metadata. If the
transcript must survive a page refresh, root must first decide whether it is a
normal user message/draft record or a separate ephemeral operation result; the
proposal prefers ephemeral UI state until the user deliberately inserts it in
the composer. Do not include it in conversation context before insertion.

The first fixture format should have a deterministic parser that can enforce
container validity, sample rate/channels/encoding, duration, and byte limits
before a provider request. WAV/PCM appears easiest to construct independently,
but root must decide exact subtype and thresholds. Reject malformed, truncated,
unsupported codec, too-long, too-large, zero-duration and unsupported-language
cases with stable user-facing errors. No silent conversion, segmentation,
fallback, automatic retry, provider switching, or hidden local-model download.

## Authority, privacy, and lifecycle rules

- Explicit user action is required for every transcription. Selecting or
  uploading audio alone causes no inference.
- The browser exposes the chosen attachment ID only. Server-side owner,
  agent, session, workspace and digest checks establish authority.
- Each actual dispatch requires a deliberate Transcribe click after a clear
  `OpenAI API` label and concise notice that selected audio is sent to OpenAI
  Platform and may incur API charges. M56 acceptance never makes that request.
- Resolve only an explicit server-side OpenAI Platform API key. Existing repo
  source has environment-based general provider API keys and separate
  ChatGPT/OAuth-app keyring records, no generic API-key vault. Missing or
  ambiguous configuration fails closed. Never log or return raw bodies,
  authorization headers, or provider error text; map them to static bounded
  errors without a credential-pattern redaction heuristic.
- Bound request body, audio bytes, duration, parallel operations, provider
  deadline, response bytes/chars and cancellation latency. Exact numeric values
  require root rulings before source freeze. One invocation per explicit click;
  no retry or fallback after uncertainty.
- Stop, navigation, session change, control disable, component unmount and
  daemon shutdown must cancel owned work and release files/buffers promptly.
  Never let a late result attach to a different session or replace newer text.
- Keep uploaded audio until the existing attachment deletion/retention policy
  removes it. Temporary provider copies, if ever needed, are owned and deleted
  on success, error, cancellation and restart; no transcript/audio copies in
  logs, telemetry, crash reports, or provider raw responses.
- Speech content is untrusted user content. It gains no tools, grants, send
  rights, or execution authority. Ordinary message policy applies only after
  explicit transcript insertion and send.
- M56 acceptance is synthetic fixtures plus mocked provider behavior and finite
  loopback HTTP peers against the shipped production adapter. It is not an
  actual provider request, microphone, semantic transcript quality, account,
  deployment, or production-availability acceptance.

## Deterministic acceptance cases

Native daemon/service and port:

1. Known independently authored WAV fixture with deterministic fake returns
   exact expected transcript and selected-attachment association.
2. Wrong owner, agent, session, workspace, missing record, changed digest,
   changed bytes, wrong kind and unselected ID reject before fake invocation.
3. Valid upload then explicit action calls fake exactly once; upload/ordinary
   chat alone calls it zero times. Duplicate requests follow a rooted policy;
   suggested first-slice behavior is one in-flight operation per attachment
   and no automatic replay after timeout/restart.
4. Malformed RIFF/chunk lengths, truncated data, unsupported encoding/sample
   rate/channel count, zero duration, over-byte and over-duration fixtures
   reject with stable bounded error and no provider invocation.
5. Fake timeout, provider error, oversized transcript, empty transcript,
   unsupported language, explicit cancellation, disconnect and late completion
   leave no transcript in storage/history and do not alter audio bytes.
6. Missing OpenAI Platform API key, ChatGPT OAuth credential present without
   API key, unsupported selected provider/model or invalid config fails closed;
   no credential return, endpoint override, raw response logging, retry or
   fallback. Use in-memory credential fixtures only; no real secret is read.
7. The production adapter runs against finite loopback HTTP peers through a
   test-only endpoint override. Assert fixed path/multipart fields, one request,
   bounded streamed response, malformed/oversized response refusal, static
   status error, timeout and cancellation. No internet request.
8. Concurrent clicks stay within declared operation/byte limits. Session
   switch/cancel does not publish stale results into the next conversation.
9. Existing image/text/DOCX/PDF attachment and model hydration semantics remain
   unchanged; verify owner-scoped download still returns original bytes.

SDK/web/composer:

1. Only supported audio shows the action; selected supported attachment gets a
   clear explicit transcription control, unsupported/corrupt statuses are
   explained, and no microphone prompt occurs.
2. Request sends only typed IDs/options; loading, stop/cancel, error, empty,
   success, stale result, session switch and component unmount are exercised.
3. Transcript appears as an editable draft associated with the file; discard
   removes it, edit is preserved, and no send occurs until normal Send action.
4. Browser dictation and read-aloud retain current behavior and clear labels;
   no UI implies daemon-native live speech or TTS.
5. Tests use deterministic fixtures, in-memory keys and finite loopback peers;
   no model, actual provider/network request, microphone, real credential,
   daemon account or service activation.

## Root validation commands to resolve at milestone start

After M55 verified merge, source freeze and root confirmation of the five
unbound risks below, verify project/target names with `bun x nx show projects
--json`; do not assume names or flags. Expected candidate gates from AGENTS.md:

- `bun x nx run rust-daemon:test --skipNxCache` (root must set its existing
  serialized Cargo environment and collect an actual exit result).
- `bun x nx run rust-daemon:lint --skipNxCache` and
  `bun x nx run rust-daemon:build --skipNxCache` if target definitions confirm.
- Relevant core Rust target if reusable port lands under packages/core-rust.
- Relevant web SDK/test/typecheck/build Nx targets after querying project
  configuration; include composer focused cases and complete package suite.
- Run only required gates for touched projects; root owns all Cargo/Nx/fmt/Git,
  process, provider, model and service operations. If Windows locks daemon exe,
  use the AGENTS.md isolated `CI=1 CARGO_TARGET_DIR=target/validation-rust-daemon`
  route, after root checks current process state.

## Five unbound risks for root ruling

1. **Audio container and parser:** WAV/PCM-only remains preferred; root must
   bind exact PCM subtype/chunk support, parser approach and rejection behavior.
2. **Limits and concurrency:** upload/audio bytes, duration, parallel count,
   provider deadline, response chars/bytes, per-owner rate and in-flight
   dedupe/restart semantics.
3. **Provider boundary:** OpenAI Platform API is the one adapter proposal and
   ChatGPT OAuth is ineligible. Root must rule whether existing env-based
   provider config is acceptable for production secrets or keyring API-key
   storage must precede shipping; also freeze model selection and consent/cost
   disclosure.
4. **Transcript lifecycle:** ephemeral composer state vs persisted draft,
   history/context admission, cancellation on navigation/restart, uncertain
   provider completion, and idempotent repeated click handling.
5. **Reuse/licensing and validation boundary:** exact Hermes revision/source
   hashes; narrow port vs no copied code; shared adapter placement; confirm
   fixtures and finite local peers are current acceptance while actual
   provider/account quality remains a later milestone.

Until ruled, these remain UNBOUND. No source implementation, dependency
addition, public roadmap edit, account/provider activation, or acceptance claim
is permitted by this held proposal.

## Controller rulings on the risks (binding)

These rulings supersede differing held proposal text. M55/PR61 is merged at
285cb59813916e3f3a2293fff4b6dbc83f6d404d, source182c1515638ed798542c65c3b1fd128c8c02f27f,
treef8a3743fd8360816c0073ef523295732ab2fa2e3. M56 starts on
g9000/companion-native-voice-m56 with114 checked baseline hashes. This releases
plan publication, then a tests-only existing upload-handler WAV RED. Production
source/dependency features follow that recorded RED and frozen interfaces.
It releases no actual provider/model/microphone/daemon service/account/container.

1. Ruling: support selected PCM16 little-endian RIFF/WAVE audio only, reusing
   the existing attachment store and download authority — this produces a useful
   bounded file-to-draft path without converters or hidden model downloads —
   cost if wrong: reject unsupported audio and retain the original supported
   attachment download; broader format/live-mic/native-TTS parity stays open.
   Add host AttachmentKind::Audio, canonical audio/wav, case-insensitive .wav;
   Core AttachmentType remains File. Validate before saving/transcribing:
   max4MiB, max120000ms (ceiling), PCM format1/bits16, channels1or2, sample rates
   8000..48000; exact blockAlign/byteRate, nonzero frame-aligned data. Exact RIFF
   declared size, checked chunk arithmetic/padding, max128 chunks, one fmt16
   before one data chunk; reject duplicate/truncated/overflow/RF64/RIFX/extensible
   or compressed inputs. Unknown bounded chunks may be skipped. No audio decoding.
2. Ruling: keep two nonqueued local operations, four retained physical file-read
   leases and two retained outgoing audio-body leases, with one absolute45s
   deadline — cancelling an await must not permit unbounded retained payloads —
   cost if wrong: return busy/timeout, retain byte leases until their owners drop,
   and hold publication on lifetime tests. Reuse existing retained source-reader
   mechanism, with Audio's4MiB record-size+sentinel cap; perform pre/post source
   hash verification under the same deadline. The reusable HTTP adapter owns a
   separate global2body-byte lease pool. Build outgoing payload with checked
   bytes1.11.1 Bytes::from_owner, whose owner retains Vec and lease through all
   Bytes clones/HTTP retention; Part::stream/body must not copy into an unleased
   Vec. Client operation cancellation does not establish remote inference stopped
   or charges were prevented. Client/physical-byte bounds are not remote-job bounds.
3. Ruling: ship one real opt-in OpenAI Platform STT adapter, using existing server
   API-key configuration and fixed HTTPS authority — a fake-only control is not
   a usable feature, while subscription/OAuth token reuse would cross purposes —
   cost if wrong: capabilities remain unavailable and no audio leaves the host.
   New ANIMAOS_STT_ENABLED defaults false. Reuse OpenAI provider API-key env
   resolution; never read/return/copy real credentials during this build. Do not
   reuse ChatGPT subscription or generic OAuth/keyring tokens. Require canonical
   OpenAI base configuration: https/api.openai.com/default443, no credentials,
   query/fragment, path /v1 or /v1/; other base overrides disable this adapter.
   Production endpoint fixed https://api.openai.com/v1/audio/transcriptions,
   model whisper-1, multipart model/file/response_format=text, filename audio.wav
   and MIME audio/wav. No client URL/model/language/prompt/header override.
   Disable redirects, ambient proxy discovery and retries/fallback/auth refresh;
   no provider discovery request on capabilities/construction/upload. Bound API
   response to16KiB+sentinel, require successful text/plain UTF8, trim outer
   whitespace (Hermes behavior), reject empty/NUL/overlong text. Static errors
   only; never log/return provider bodies, keys, request audio or transcript.
4. Ruling: transcription is an explicitly consented ephemeral draft operation,
   with scoped cancellation and bounded terminal tombstones — it must never
   overwrite a newer conversation or silently send/replay speech — cost if wrong:
   discard the result and require a new explicit action; no exactly-once or remote
   cancellation guarantee is claimed. Host tracks at most64 scoped operation IDs
   (UUIDv4), two active/nonqueued; completed/failed/cancelled/uncertain tombstones
   last at most5min, evict terminal entries only, never active work. No transcript
   text/audio/credentials in this map or durable storage. Duplicate ID returns409
   without a provider call; cancel-before-admission creates a scoped tombstone.
   Explicit Stop sends best-effort owner-scoped DELETE then aborts local await;
   navigation/unmount also fence late results. The deadline remains the bound
   if cancellation delivery is lost. Restart clears this ephemeral map and never
   starts/replays operations; dedupe is not durable exactly-once.
   UI shows provider/remote-transfer/API-charges notice before the named action.
   Result preview has explicit Use transcript/Discard; insertion appends to the
   current same-scope composer draft only on that separate click. Ordinary Send
   remains separate. Upload and ordinary chat trigger zero transcription calls.
5. Ruling: narrowly port useful Hermes OpenAI transcription semantics into the
   existing reusable Rust model-adapter package, with MIT attribution — the local
   Python helper is a useful source while a separate Hermes process adds another
   runtime/profile/credential owner — cost if wrong: dependency/source/license/
   contract tests block publication, and actual voice quality remains unverified.
   Public Hermes HEAD1fd18277267152893a8a24792b79213e7ca3f899 and selected
   transcription_cloud.py/common.py/tools.py/LICENSE are clean; exact hashes are
   in public-source-receipt.json. Retain full MIT notice/pin/local changes for the
   adapter port and Anima product naming. No provider catalog/chunking/commands/
   plugins/live recording/TTS/profile Python copy. Root may enable existing
   reqwest multipart and add already-locked bytes/Tokio normal dependencies only
   after RED, reviewing the minimal lock/MSRV/license delta. No toolchain upgrade.
   Core stays HTTP/env/host independent; host owns credential selection, file
   authority and operation lifecycle. One native/core review, one final SDK/web
   review, one fix wave and fresh required gates; root alone executes/fmts/Git.

### Frozen interfaces and source lanes

- Core module audio_content: constants MAX_AUDIO_INPUT_BYTES=4*1024*1024,
  MAX_AUDIO_DURATION_MS=120000, MAX_TRANSCRIPT_BYTES=16\*1024. Opaque
  WavAudio::parse(Vec<u8>)->Result<WavAudio,AudioValidationError>, byte/info
  accessors and into_bytes; no Deserialize/Clone of owned payload authority.
  WavInfo carries sample_rate/channels/duration_ms. Closed validation errors
  malformed/unsupported/too_large/too_long. Pure validate_transcript_text helper
  enforces nonempty trimmed bounded NUL-free text at adapter and host boundary.
- Core TranscriptionPort Send+Sync: availability() returns a closed safe status;
  async transcribe(audio:WavAudio,cancel:CancelSignal,deadline:std::time::Instant)
  ->Result<String,TranscriptionError>. Closed errors busy/unavailable/cancelled/
  timed_out/invalid_response/failed. No attachment IDs, env, paths, credentials,
  HTTP types, Tokio or host authority in this reusable trait.
- Model adapter OpenAiTranscriptionAdapter: private production-only fixed endpoint,
  secret key excluded from Debug/logs, no redirect/proxy/retry; multipart body
  owner retains outgoing bytes lease through every transport clone. Only cfg(test)
  may inject finite loopback peer URLs/synthetic keys. Real requests never run in
  this milestone. Revalidate text/remaining absolute deadline, fail closed.
- Host voice service: resolve configured adapter on normal startup, disabled by
  default; test-only injectable port. GET
  /api/agents/{agent}/sessions/{session}/voice returns safe
  {version:1,provider:"openai",model:"whisper-1",available:boolean,reason:closed}.
  Resolve and authorize existing own Web Chat scope before every endpoint.
- POST same prefix /voice/transcriptions: deny-unknown body
  {operationId:UUIDv4,attachmentId:string,provider:"openai",consent:true}. No
  filesystem/MIME/URL/key/model fields accepted. Scope, immutable selected
  attachment record, workspace, current bytes/hash, WAV validity and operation
  admission checked before any provider invocation. Use pre/post source checks.
  Response only {version:1,operationId,attachmentId,provider:"openai",
  model:"whisper-1",text}. No private hash/root or raw provider diagnostic.
- DELETE /voice/transcriptions/{operationId}: same owner/agent/session authority;
  idempotent local cancellation request with204, including a bounded pre-cancel
  tombstone; active work signals CancelSignal and cannot publish known-cancelled
  text. Return stable bounded existing daemon error envelopes for invalid400,
  unknown-scope/source404, duplicate409, unsupported415/invalid422, busy429,
  unavailable503, failed502 and timeout504. Cancellation is a local status,
  never a claim about remote billing or completion.
- An audio attachment sent normally remains a File reference with honest audio
  placeholder and original download; no binary inline input or automatic STT.
  Reserved document snapshot logic stays Document-only. Existing DOCX/PDF/image/
  text and browser dictation/read-aloud behavior remains unchanged.
- SDK has typed capabilities/transcribe/cancel methods with AbortSignal, no
  retries, and closed Audio kind. Web helpers reuse existing upload/draft/session
  ownership, show one selected audio action and ephemeral preview; no new page,
  hidden recording, automatic send, new UI dependency or key-entry flow.
- Source lanes after RED: core WAV/trait (core/audio_content.rs + exports/tests);
  reusable HTTP/byte lifetime adapter (model-adapters/transcription.rs + focused
  tests, root Cargo/lock); host attachments/voice/routes + tests; SDK/web after
  DTO freeze with disjoint files. Root owns fixtures/docs/ledger/commands/Git.

### Evidence, tests and gates

Official OpenAI documentation fetched before root binding:
https://developers.openai.com/api/docs/guides/speech-to-text . It establishes the
fixed transcription endpoint, API-key Bearer/multipart request and the retained
whisper-1 text response path. The guide now recommends gpt-transcribe for new
general-purpose transcription; this slice deliberately retains Hermes's narrow
whisper-1 contract without claiming best/current model quality. Reference
https://developers.openai.com/api/reference/resources/audio/subresources/transcriptions/methods/create
was fetched but exposed only a generated-page notice, so the actual guide was
used instead. No API-key helper tool is available; no credential is needed for
documentation/fixture-only acceptance.

First RED: root independent stdlib PCM WAV fixture, existing upload/router APIs
only, expected201/kind audio/original bytes; current handler rejects WAV. No new
API/imports/feature dependencies/production source before that recorded RED.
Then strict container/duration/input arithmetic and malformed/unsupported cases;
wrong scope/source/hash and selection; disabled/absent/alternate-base credentials
with zero calls; one explicit call, no automatic upload/chat invocation;
operation duplicate/cancel-before-admit/deadline/drop/late-result/restart-no-replay;
physical source/body lease retention and bounded no-queue concurrency; finite
peer request shape, no redirects/retries, error/no raw bodies, response size/UTF8;
editable/discardable same-scope draft and unchanged browser features.

Root commands use the verified existing Nx targets: full rust-daemon:test with
core dependency, rust-daemon:lint/build; SDK/web test/typecheck/build and focused
composer/voice cases, owned Prettier/diff/docs scans. CARGO_INCREMENTAL=0/jobs1/
threads2, CI/ANIMA_SKIP_SERVICE_TESTS, cleared provider/service env, NX_DAEMON=false/
NX_ISOLATE_PLUGINS=false/NX_NO_CLOUD=true/native cache as prior milestone.
Use the direct native Python runner through cmd.exe after the recorded cancelled
PowerShell startup stalls. Do not start ordinary services, providers, microphones,
real accounts, DB servers/containers/Docker55432 or deployments. M47/M52 actual
cases and consumed releases remain immutable; M52 strict quality stays PARTIAL.
Commit explicit paths only, never nx.json/anima.yaml/private. Preserve nx SHA256
3B71AABE3A3ACF91B43F6C0B4F7F53B486645EB9316D7E66B16F9B85F9DBAC90.
