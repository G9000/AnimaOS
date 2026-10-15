# M58 selected reply to native speech output

Bound lean plan on verified M57/PR63 main. The controller rulings below supersede earlier recommendations. Plan publication and existing SessionView RED precede production source. No actual provider, speaker, service or account activation is released.

## Earlier lean recommendations (superseded by binding below)

## Goal and proposed user flow

Offer an owner an explicit way to hear one selected assistant message using the
server's configured OpenAI Platform speech provider. The user chooses
**Generate speech** on that message; the UI shows that text will be sent to
OpenAI and may incur account charges before the request. On success, show
metadata and separate **Play** and **Stop** actions. Generation and playback
never start from render, a new chat response, selection, or navigation. No
automatic voice, autoplay, message mutation, transcript insertion, or send.

The first slice should accept only the exact visible assistant message text,
after a small bounded length check (proposed maximum 4,096 Unicode code points),
with no model rewrite or voice-design prompt. The server chooses a fixed,
documented OpenAI speech model, default voice, format, and speed from server
configuration. The browser cannot submit provider, model, voice, speed, base
URL, credential, output path, or arbitrary instructions. Provider absence or
failure returns a safe error; no browser or Hermes-provider fallback is
automatic. Existing browser Read aloud stays available as its separate local
SpeechSynthesis action.

## Evidence and reuse comparison

The pinned public Hermes checkout requested for inspection is
`C:\Users\leoca\AppData\Local\hermes\hermes-agent` at source revision
`1fd18277267152893a8a24792b79213e7ca3f899`. Public implementation inspected:
`tools/tts_tool.py` registers `text_to_speech` and bounds provider input;
`tools/tts_tool_openai.py` implements OpenAI `audio.speech.create`, resolves
credentials/configuration on the server, and writes returned audio to a file.
The checkout's `LICENSE` is MIT, copyright Nous Research (2025). The revision
identifier is the source hash captured for this held research; no Hermes code
has been copied. If a later implementation ports code, preserve its MIT notice,
attribute the exact source revision and local changes, and obtain a fresh
license/source review before publication. This inspection did not read profile
data, user configuration, or secrets.

The existing local browser path is `apps/web/src/hooks/useReadAloud.ts` and
`apps/web/src/components/voice/ReadAloud.tsx`. It uses browser
`SpeechSynthesisUtterance`, accepts a message ID and text, and fences callbacks
on conversation/disabled boundaries. It requires no server or provider and
already offers explicit Read aloud / Stop reading. Reuse is the smallest path
for no-cost local speech, but it does not satisfy server-configured OpenAI
voice/output or provide a stable generated audio artifact. Keep it intact and
available; do not silently replace its semantics.

| Approach                         | Advantages                                                                                                                                     | Costs and decision                                                                                                                                           |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Reuse browser `useReadAloud`     | Already implemented, local, no provider request or audio upload                                                                                | Does not produce server-configured OpenAI speech; retain as existing local action, not the proposed native feature                                           |
| Run Hermes as a process adapter  | Could reuse its provider catalog and OpenAI implementation broadly                                                                             | Adds another process/runtime boundary, profile/config/credential isolation, lifecycle, filesystem and packaging concerns; out of scope for one speech action |
| Narrow attributed Rust host port | Keeps credentials and provider call in the Rust daemon, fits current host and voice routes, limits API to one selected text and returned audio | Requires a small provider adapter, server configuration and explicit source attribution if any upstream logic is ported; **recommended**                     |

Recommendation: a narrow Rust daemon implementation, reusing existing voice
authorization and OpenAI client/configuration where compatible. Before coding,
verify whether the existing native transcription configuration can safely
provide the same OpenAI Platform credential and client boundary. Do not assume
that a transcription entitlement or config automatically authorizes speech
generation or billing; capability must represent speech configuration
independently. Avoid the Hermes filesystem output path and its generic tool
surface.

## Proposed API boundary — subject to root verification

Reuse the existing owner-scoped session path family under
`/api/agents/{agent_id}/sessions/{session_id}/voice` and its current ownership
checks. Add an independent speech capability in the safe, no-provider-request
capability response, or a separately versioned field there. Do not infer speech
availability from transcription availability.

Proposed request:

```ts
type SpeechRequest = {
  operationId: string; // client-generated opaque id for cancellation/fencing
  text: string; // exact selected assistant text, bounded server-side
};
```

Proposed response is one bounded audio body with a validated `audio/*` content
type plus safe headers for the selected fixed format; no JSON/base64 expansion,
disk path, URL, transcript, or provider details. Set a strict response byte cap
and absolute deadline, and reject non-audio/oversize provider output. Confirm the
provider's response format and an appropriate output cap from current official
OpenAI docs before binding. A scoped DELETE requests local cancellation by
operation ID. Report honestly that a client abort may not cancel a provider
operation or prevent billing. Never persist request text, audio bytes, generated
files, or audio URLs.

The existing capability and transcription handlers are in
`hosts/rust-daemon/src/routes/voice.rs`; the SDK owner/session client path is
used by `apps/web/src/components/voice/NativeTranscription.tsx`. The exact new
SDK signature and response type remain unbound until root verifies current
contracts and generated API docs. Reuse authenticated owner scope; browser IDs
must not establish ownership. Revalidate the message/conversation scope before
applying any result.

## UI, state, and privacy rules

- Add a per-assistant-message action with distinct idle, generating, ready,
  playing, and error states. Keep it separate from browser Read aloud.
- Before the first Generate speech action, show a concise disclosure that the
  selected text is sent to OpenAI and may incur charges. Capability checks and
  render make zero provider calls.
- Only an explicit Generate speech click sends the selected text. Bound the
  request on both sides. Never synthesize on chat completion, render, rerender,
  message selection, or browser permission.
- Generation success presents Play. Never autoplay. Stop halts current playback
  and fences late events. Do not add the generated audio to attachment/upload,
  memory, transcript, draft, or send state.
- Bind each operation to the source message ID and conversation/session scope.
  Navigation, message replacement/removal, disabled/offline state, unmount,
  cancel, or a newer operation discards late results.
- If playback uses a Blob URL, keep at most one owned URL; revoke it on Stop,
  replacement, navigation, error, or unmount. Keep only the current bounded
  audio object in memory. Do not use local storage, IndexedDB, analytics, logs,
  or hidden audio previews.
- Keep browser SpeechSynthesis behavior, message controls, transcription, and
  send behavior unchanged. No TTS preview, voice picker, streaming, chunking,
  custom instructions, provider fallback, or conversation-wide playback in v1.

## Proposed internal shape

Bind names after root verifies code boundaries. Keep provider work in the daemon
and browser state in the existing message action/component.

```rust
struct SpeechRequest { operation_id: OperationId, text: String }
struct SpeechAvailability { available: bool, reason: Option<SafeReason> }
trait SpeechProvider {
    async fn synthesize(&self, text: &str, cancel: CancellationToken)
        -> Result<BoundedAudio, SpeechError>;
}
```

`BoundedAudio` owns validated format and bounded bytes only for the request
lifetime. The provider takes server-loaded configuration; request data cannot
select provider settings. Reuse the established voice route owner checks,
operation map, deadlines, safe error mapping, and cancellation semantics where
compatible. Verify operation-ID syntax, duplicate behavior, busy limits, output
caps, and race behavior against existing transcription contracts before reuse.

## Focused acceptance and regression list

Use fake provider and browser audio objects only. Do not request a real provider,
open a real audio device, or exercise account billing.

1. Capability reports speech readiness without invoking the provider; speech
   can be unavailable while transcription remains available, and vice versa.
2. Owner/session authorization, unknown scope, malformed IDs, oversized text,
   empty text, rate/busy behavior, content type, response byte cap, and deadline
   map to stable safe HTTP outcomes.
3. Valid request forwards exactly the selected text and fixed server-configured
   model/voice/format; client-supplied provider/model/voice/speed/base URL or
   instructions are rejected or ignored by a strict schema.
4. Provider absent, rejected, malformed, oversized, timeout, panic/error, and
   cancellation leave no persisted text/audio and return no path or credential.
5. Duplicate/cancel/newer operation and late-completion races fence old results;
   local cancellation is not reported as proof of remote cancellation or no bill.
6. UI: zero requests on render/chat completion; click Generate makes one request;
   disclosure appears before first use; response never starts playback;
   Play/Stop are explicit; selected text and message scope are exact.
7. UI: navigation, removal/replacement, disabled/offline, unmount and rerender
   reject stale response/playback callbacks and revoke any owned Blob URL once.
8. Existing browser Read aloud, microphone capture, native transcription,
   upload, send, and ordinary text remain unchanged; no accidental request or
   message mutation.
9. Fake playback construction/`play()` rejection and ended/error/Stop events
   clean up correctly. No hardware access, autoplay, telemetry, or storage.

## Candidate commands after binding

Confirm Nx target names first. Run only the directly affected gates after a
reviewed implementation exists; these are proposals, not executed commands.

```text
bun x nx show projects --json
bun x nx run rust-daemon:test --skipNxCache
bun x nx run <web-project>:test --skipNxCache
bun x nx run <web-project>:typecheck --skipNxCache
bun x nx run <web-project>:build --skipNxCache
```

Avoid broad Rust workspace gates unless the selected targets require them.
Do not start the daemon or a server for tests. Keep actual provider, deployment,
hardware, and billing acceptance open and separately authorized.

## Top five risks needing controller ruling

1. **Credential/config source:** transcription and speech may have distinct
   entitlements/config. Verify before sharing any credential or readiness bit.
2. **Provider cost and disclosure:** one deliberate click can still incur cost;
   confirm the exact disclosure and fixed model/voice defaults before binding.
3. **Cancellation and duplicate work:** browser abort cannot prove remote
   cancellation. Define server operation capacity, idempotency and busy behavior.
4. **Audio response size/lifetime:** choose a format, hard byte cap, request
   deadline and in-memory/object-URL cleanup behavior from verified provider
   limits before writing the route.
5. **Upstream reuse and licensing:** a narrow port may be unnecessary if existing
   Rust provider code covers speech, but any Hermes-derived code needs MIT notice,
   exact source revision, attribution and local-change record.

## Release gate

Remain HELD until M57 is merged; root confirms the tree, current voice contracts,
OpenAI speech settings/limits and config ownership; the five risks receive
controller rulings; and a binding names exact file ownership, API shapes, tests,
commands and source reuse attribution. This proposal alone is not permission to
write production code or make provider requests.

## Controller rulings on the risks (binding)

M57/PR63 is merged at f721f91f3081f6b997bc34b6641d52db19099f9b; source
dd04ae906d8b3373622c40581e723c74ee31c337 has identical tree
cca532c9e3344e347365c4fe0f5ae48d26a1332c. This branch is
g9000/companion-speech-output-m58 with149 verified baseline hashes. These rulings
supersede held recommendations. Publish the plan, then record a missing native
speech control RED using existing SessionView APIs before production source.
No actual provider/model/account, speaker/microphone, ordinary service, database
server, container or deployment activation is released.

1. Ruling: narrowly port Hermes's direct OpenAI speech request/defaults into the
   existing Rust adapter architecture — this reuses useful behavior without its
   Python process, profiles, provider breadth or filesystem output — cost if wrong:
   native speech fails closed while existing browser Read aloud remains available.
   Public source pin1fd18277267152893a8a24792b79213e7ca3f899, MIT/Nous Research2025;
   tts_tool_openai.py defaults gpt-4o-mini-tts/alloy and audio.speech.create shape.
   Retain full existing MIT notice and add exact source/local changes. No new
   dependency. Existing browser SpeechSynthesis is retained as a distinct action;
   no claim of free/local/native-provider breadth parity. No managed gateway,
   credential/profile copy, chunking, fallback, normalization, custom voice,
   instructions, provider model tool or automatic generation.
2. Ruling: independent ANIMAOS_TTS_ENABLED=true/1 plus existing server OpenAI
   Platform API-key configuration, exact canonical base whitelist, fixed
   gpt-4o-mini-tts/alloy/speed1/response_format=pcm — STT configuration or a model
   subscription is not consent to speech charges — cost if wrong: unavailable
   capability and no request. Endpoint fixed https://api.openai.com/v1/audio/speech;
   no proxy, redirects, retries, caller endpoints/headers/model/voice/speed or OAuth.
   Constructor and capability make zero provider requests. Request JSON carries
   exactly model,voice,input,response_format,speed. No upstream random idempotency
   header; local operation IDs are not remote exactly-once/billing guarantees.
   Disclosure before Generate: selected text sent to OpenAI, API charges possible;
   before Play it remains clear the voice is AI-generated. Manual Play only.
3. Ruling: server-authoritative exact own saved visible Assistant reply, max2048
   Unicode scalars and8192 UTF-8 bytes, nonblank/NUL-free, no trimming/rewrite —
   selected-source consent must not become arbitrary browser text or a stale reply
   — cost if wrong: changed/foreign/hidden/missing/uncommitted source refuses; use
   browser Read aloud for long replies. Request includes messageId plus exact text
   to bind the owner's visible selection. Read via bounded-ID get_message under
   exact agent/session; do not scan history. Recheck record equality, role/hidden,
   workspace root, session creation identity and history store identity before
   dispatch and before publication, without holding daemon locks over IO/provider
   awaits. Ordinary chat/messages/memory/files/grants remain unchanged. No extra
   persistence of payload or audio; original history is already persisted.
4. Ruling: one bounded raw PCM24kHz mono16-bit LE response, max120s/5,760,000 raw
   bytes, wrap canonical44-byte WAV locally (max5,760,044) — official PCM format
   avoids assuming streamed WAV headers have final RIFF lengths — cost if wrong:
   unsupported/malformed/odd/empty/large audio refuses, with no automatic retry.
   New speech output type is distinct from unchanged M56 upload WAV4MiB limits.
   Strict200 and closed audio/pcm or application/octet-stream media types (case
   insensitive, trimmed, no parameters); encoding absent or identity only.
   Cap declared and actual decoded body before append; exact sample format is the
   fixed provider PCM contract, not a claim of semantic speech accuracy. Bound
   retained allocations; wrapping may temporarily retain two bounded buffers,
   without claiming a bound on browser/OS/HTTP internals or secure erasure.
   Host max2 nonqueued operations,64 scoped UUIDv4 entries/5min tombstones,45s
   absolute operation and46s outer request budget. Two physical speech-output
   leases acquired before provider dispatch survive lost HTTP await and follow
   output Bytes owners/clones until body retirement. Adapter separately retains
   two small JSON-body leases with actual bytes after cancellation/drop. No async
   timeout/drop silently frees a still-owned buffer. Restart clears map/no replay;
   cancel-before-admit tombstones, duplicate refusal and publication linearization
   follow M56 patterns in a separate speech namespace. Cancellation cannot prove
   remote inference/billing stopped. Do not refactor M56 to add speech.
5. Ruling: one selected-reply panel per active own Web Chat, one retained browser
   speech owner per page, one candidate Blob and at most one player/owned URL —
   per-message audio caches and abandoned async operations would grow unbounded —
   cost if wrong: busy/cleanup pending until observed settlement or reload, browser
   Read aloud remains available. Selection from at most20 newest visible Assistant
   replies in current props, no additional history load/auto selection/generation.
   Generate obtains safe capability then makes one POST only after the same
   explicit click; no HTTP on render/selection and no provider action on capability.
   Never auto-play on result. Play speech creates a player/URL only on its explicit
   click. Stop speech stops playback but can retain the bounded candidate for
   explicit replay after cleanup settles; Discard speech/selection change drops it.
   Scope/disabled/offline/sending/navigation/unmount fences all callbacks and starts
   cancellation at commit via layout cleanup. Retain global owner across pending
   SDK/cancel/play promises; no queue. Cancel HTTP sent at most once only after a
   POST was dispatched, independent2s abort signal; count its settlement too.
   Pause/detach handlers/remove src/load/revoke owned URL exactly once; observe play
   rejection immediately. Late play completion cannot revive retired playback.
   If cleanup is uncertain retain owner, discard output and report cleanup pending.
   No autoplay/audio element on render, audio URL storage/logging, device selection,
   hidden upload/send, transcript/memory mutation or microphone involvement. Stop
   affects our player, with no guarantee about unrelated apps or physical speakers.

### Frozen native interfaces and ownership

Core/adapter lane owns new anima-core/src/speech.rs + core lib.rs exports, new
anima-model-adapters/src/speech.rs + its lib.rs exports and HERMES-NOTICE. All are
under packages/core-rust/crates. Tests inline or speech/ submodule owned by same
lane. No HTTP/DB/Tokio dependency in core; existing async_trait/CancelSignal used.

Core exports SpeechAvailability {Available,Disabled,MissingCredentials,
UnsupportedConfiguration,Unavailable}; SpeechError {Busy,Cancelled,TimedOut,
Unavailable,InvalidInput,InvalidResponse,Failed}; SpeechText private String with
new(String)->Result<Self,SpeechError>, as_str(); SpeechAudio private canonical WAV
Vec, from_pcm24k(Vec<u8>)->Result<Self,SpeechError>, wav_bytes(), into_bytes(),
duration_ms()->u32. Neither audio nor text needs Clone. Constants for max input
scalars2048/bytes8192, PCM5_760_000, WAV5_760_044, duration120_000. PCM frames6000=>
250ms. Reject empty/odd/over-cap raw payload; canonical monoPCM16/24k header.

#[async_trait] SpeechPort:Send+Sync with availability()->SpeechAvailability and
synthesize(SpeechText,CancelSignal,Instant)->Result<SpeechAudio,SpeechError>.
OpenAiSpeechAdapter::new(String)->Result<Self,SpeechError>, fixed authority and
defaults. Private cfg(test) constructor accepts only numeric127.0.0.1 explicit
port /v1/audio/speech. Finite peers normalize accepted sockets to blocking, with
existing bounded accept/read/write deadlines; do not repeat M56 partial-peer bug.
Use installed bytes1.11.1 from_owner for retained outgoing JSON owners. No dependency
or lock changes. Retain old STT adapter/exports/behavior. No model generation tests.

Host lane owns new src/speech.rs and src/speech/tests.rs; new routes/speech.rs;
new routes/contracts/speech.rs + contracts/mod.rs; app.rs, routes/mod.rs,
runtime_model.rs, lib.rs and necessary exact bootstrap wiring. Reuse existing
voice::check_scope (map errors) and M56 ownership patterns without moving generic
engine code into host. Root owns compiled README/operation-inventory test updates
and fixture/docs. Host may add a test injection constructor that defaults existing
router helpers to speech disabled; preserve every old constructor/caller behavior.

SpeechService disabled/configured/availability/cancel/generate and a response owner
that carries its output lease into Bytes::from_owner. Active owned task and its
cancel/drop/timeout logic remain bounded, map linearization survives dropped HTTP
await. Revalidate source and scope under original absolute deadline. Get exact
HistoryMessage via existing store get_message; require matching ids, !hidden,
Assistant role and full text equality; compare full saved record before/after.

Routes (existing owner authorization and no-store):

- GET /api/agents/{agent_id}/sessions/{session_id}/voice/speech => JSON capability.
- POST same path => binary audio/wav, fixed inline filename optional, exact
  Content-Length and X-Anima-Speech-Duration-Ms decimal integer; no JSON/base64.
- DELETE same path/{operation_id} =>204 local cancel acknowledgement.
  All three use safe bounded request waits; POST16KiB body max plus46s outer timeout
  covers body/scope waits before45s operation. Request deny_unknown_fields camelCase:
  {operationId,messageId,text,consent:true}; no arbitrary provider options. Valid ids
  use existing safe scoped-ID rules (message <=128 visible safe ASCII); UUIDv4 for
  operations. Wrong role/source404, invalid body/text/id400, unavailable503, duplicate/
  cancel409, busy429, timeout504/outer408, malformed provider502. Owner403; no raw
  provider/error/source details. No media payload/cache/config writes in these routes.

Capability exact v1 fields:
{version:1,provider:'openai',model:'gpt-4o-mini-tts',voice:'alloy',format:'wav',
available:boolean,reason:available|disabled|missing_credentials|
unsupported_configuration|unavailable,maxInputChars:2048,maxInputBytes:8192,
maxAudioBytes:5760044,maxDurationMs:120000}.
Keep existing /voice capability/transcription request/reply v1 unchanged. Separate
ANIMAOS_TTS_ENABLED; canonical OpenAI base and existing API-key env precedence;
no OAuth/profile/connector vault inference. Env tests use pure config seams with
synthetic keys, never actual credentials or ordinary profile/service activation.

### Frozen SDK/web boundary

SDK/web lane owns packages/sdk/src/speech.ts + speech.spec.ts, client.ts/index.ts
exports and bounded-response helper tests as needed; web new
components/voice/NativeSpeech.tsx + tests, optional lib/native-speech.ts + tests;
components/sessions/SessionView.tsx + tests. No ChatScreen/ViewHarness/state-store
refactor unless root rules on a concrete wiring blocker. Existing SessionView API
has agent,session,messages,composer; no new parent props needed. Only own Web Chat
with an existing session; no ensureSession/created session as a speech side effect.

DaemonClient readonly speech:SpeechClient. SpeechClient.capabilities(agentId,
sessionId,{signal}?) -> Promise<SpeechCapabilities>; generate(agentId,sessionId,
{operationId,messageId,text,consent:true},{signal}?) -> Promise<Blob>;
cancel(agentId,sessionId,operationId,{signal}?) ->Promise<void>. Literal capability
fields/reasons closed and availability consistent. Validate request chars/bytes/id
before fetch; reject unknown/mismatched replies. Fixed paths, caller cannot choose
URL/model/voice. No retry/automatic upload/send.

Add DaemonClient.requestBoundedBlob(path,RequestInit,{maxBytes,contentType}) using
the existing central fetch/connection handling. Bound Content-Length + actual
stream bytes before copy, nonempty, exact audio/wav; cancel reader on errors/abort,
release reader lock, observe cleanup promises. On HTTP failure discard body and
throw status-bearing DaemonHttpError with no raw body, rather than unbounded error
JSON. Leave existing unbounded requestBlob callers/behavior unchanged. SpeechClient
checks canonical44-byte monoPCM16/24k header, exact lengths/frame count/duration
before handing Blob to UI. Bound transient copies, no Blob URL in SDK/HTTP reply.

NativeSpeech props {agentId:string,sessionId:string,conversationKey:string,
messages:readonly ChatMessage[],disabled:boolean}. Render one panel in SessionView
under existing nativeVoice/own Web Chat/session conditions; preserve message list,
Composer, readAloud, native STT and capture. Safe unknown/new-session state inert.
Name dropdown 'Assistant reply for speech'; Generate button named 'Generate speech
with OpenAI API'; selected preview clearly identifies exact text/limit. UI static
availability/failure messages only. Manual 'Play speech', 'Stop speech', 'Discard
speech'; generation 'Stop generating speech'. No provider/Audio/URL action on
render. Metadata-only React state; actual Blob/player/ownership in refs/controller.
StrictMode-safe layout cleanup, stale handlers/observer callbacks/ready results
fenced by conversation/session/source text/disabled generation. No automatic fetch
on selection, session lookup, playback, URL creation, transcript insertion or send.

### Evidence, tests and commands

Root verified public Hermes commit/license and hashes in upstream-source.json:
tts_tool_openai.py76461D00E7082D32CD64B815012FD463FEF5BC2893973261AAC131A00781B713;
tts_tool.py3F4F8D607F8F30CEB963581D1F7CDB4E3A72E9130EE7C49B502023153F1780C2;
LICENSE E29DFAF6905A8D396A58EA4BD84AD27057756BBD9599B5BEC0971AE34D2A2118.
No personal source/config/credentials read. OpenAI Docs MCP search/fetch and
Context7 /websites/developers_openai_api_reference verified request fields,
4096-character provider cap and gpt-4o-mini-tts/alloy support; our2048 scalar/8192
byte cap is intentionally smaller (also <=4096 UTF16 units). Primary guide states
PCM24kHz/16-bit signed little-endian and AI voice disclosure. MIME allowlist is a
fail-closed compatibility decision; actual returned wire/voice quality unverified.
https://developers.openai.com/api/docs/guides/text-to-speech
https://developers.openai.com/api/reference/resources/audio/subresources/speech/methods/create
https://developer.mozilla.org/en-US/docs/Web/API/HTMLMediaElement/play
https://developer.mozilla.org/en-US/docs/Web/API/HTMLMediaElement/pause
https://developer.mozilla.org/en-US/docs/Web/API/HTMLMediaElement/load
https://developer.mozilla.org/en-US/docs/Web/API/URL/revokeObjectURL_static
play promise may wait for permission and reject; load aborts pending media work;
pause affects own element; URL revoke releases our reference, not a global device
shutdown/secure erasure guarantee. Fake playback only in acceptance.

First RED existing SessionView requires visible native Generate control without
new imports/APIs. Then core text/UTF8/PCM/header/caps against independent root
stdlib WAV fixture24k/6000frames/250ms/12044bytes; adapter exact JSON/headers,
fragmented finite peer/full-body reads, types/status/empty/odd/large/deadline/drop/
retained JSON body ownership; host scope/foreign/hidden/role/exact source pre/post,
session/store replacement, consent/unknownfields, disabled STT/TTS independence,
2active/64map/2physicaloutput body retention/duplicates/tombstones/deadlines/cancel/
drop/panic/restart-no-replay/no persistence. SDK binary chunk/body/type/header/
length/WAV/cancellation and unchanged old blob behavior. UI explicit Generate then
manual Play/Stop/replay/Discard, zero requests on render/selection, one retained
global owner through pending request/cancel/play cleanup, URL exact cleanup, safe
failed media APIs, no late navigation/disabled/source replacement result, StrictMode,
no automatic STT/capture/draft/send/browser speech regression.

Source-only workers do no tests/fmt/Nx/Git/process/hardware/network activation.
Root alone commands. One native batch review and one final SDK/web review, one
combined tests-first fix wave, then required current-tree Nx rust-daemon:test,
lint/build (core dependency included), SDK/web test/typecheck/build, owned
Prettier/cargo fmt, compiled OpenAPI inventory/docs/hash gates. Cargo always
CARGO_INCREMENTAL=0/jobs1/testthreads2, serialized; use separate documented target
dir if actual exe is locked, never start/stop ordinary daemons. No new dependencies.
Direct native Python/cmd runners, not stalled PowerShell; cleared provider/service
env, CI=1/ANIMA_SKIP_SERVICE_TESTS=1/NX_DAEMON=false/NX_NO_CLOUD=true. Local gates
are verification, no Nx Cloud success or full Hermes parity claim. Actual speaker,
speech quality/billing/connector/local provider acceptance stays open.

Explicit path staging only; never nx.json/anima.yaml/.superpowers. Per-command
unsigned commits and standing reviewed merge-and-continue remain authorized.
Preserve nx SHA3B71AABE3A3ACF91B43F6C0B4F7F53B486645EB9316D7E66B16F9B85F9DBAC90.
No stash/reset/restore/path checkout/worktree, ordinary profiles/credentials, real
accounts/DB servers/containers/Docker55432/public deployment. M47/M52 consumed
cases immutable, no replay/rescue/regrade; M52 strict quality remains PARTIAL.
