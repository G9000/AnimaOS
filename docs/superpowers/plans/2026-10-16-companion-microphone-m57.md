# M57 bounded microphone capture to selected WAV

Bound lean plan after verified M56/PR62 merge. Root rulings below supersede held recommendations. Plan publication and existing-composer tests-only RED precede production source; no actual microphone/provider/service acceptance or activation is released.

## User outcome

In a chat composer, the owner can explicitly start microphone capture, stop it, and get a bounded PCM16 WAV. That WAV enters the existing upload flow as a selected, ready audio attachment. The owner can then use the existing separately named OpenAI API transcription action, inspect/edit the returned preview, and separately choose Use transcript or Discard. Ordinary send stays separate.

This is one small browser capture step on top of M56's selected-file path. It does not replace browser dictation, silently invoke transcription, send speech, or add a second transcription route. PAR-07 remains open for playback, connector voice attachments, other providers, and real-device acceptance.

## Repository fit and reuse decision

- The roadmap leaves PAR-07 planned and explicitly distinguishes browser speech convenience from daemon-native transcription. Its acceptance requires synthetic/mock evidence before any separately consented device case.
- Current `NativeTranscription.tsx` only offers a named action for a selected ready `audio` attachment. It uses the session scope, fences late results, warns about transfer/charges, and keeps text in an editable preview until Use. `attachments.ts` already uploads `File` values and classifies `.wav`; the host validates actual format and caps audio at 4 MiB/120 seconds.
- Existing `useBrowserDictation` uses browser speech recognition to produce text. It is not an audio capture/encoding implementation and must remain its own convenience.
- No current `MediaRecorder`, `AudioWorklet`, or `getUserMedia` capture implementation was found under `apps/web/src`.
- Reuse existing composer attachment lifecycle, upload progress/errors/removal, and M56 selection/transcription/review. Do not copy Hermes's Python tool or add a browser speech-recognition substitute. No new dependency is proposed.

## Candidate capture approach

Prefer a small browser-only capture controller backed by `getUserMedia`, an `AudioContext` requested at 16,000 Hz, and a bundled `AudioWorklet` that emits mono PCM frames. Verify the actual context sample rate and worklet availability; fail closed with a clear unsupported-browser/device message if unavailable. Do not fall back to `MediaRecorder` because its encoded container/codec is not the host's supported WAV format. Do not create an unbounded Float32 recording and convert it after Stop.

Keep one bounded accumulator for signed little-endian PCM16 mono samples. Saturate normalized floats to [-1, 1], map to signed 16-bit PCM, and use checked arithmetic for every appended chunk. A 16 kHz mono stream is 32,000 payload bytes/second; 120 seconds is 3,840,000 bytes, below the 4 MiB WAV limit after its 44-byte header. Still enforce both the duration and complete WAV size before materializing the Blob/File. Any sample-rate mismatch, counter overflow, excess duration/bytes, or malformed final length stops capture and discards the partial recording.

Before coding, verify current supported-browser behavior for `getUserMedia`, `AudioContext({ sampleRate })`, AudioWorklet module loading, channel mixing, permission outcomes, and track shutdown using official browser documentation. Confirm local app asset handling for bundling the worklet. If 16 kHz capture is not consistently supported, return a revised bounded resampling proposal for controller review; do not quietly widen limits or invent a custom DSP path in this milestone.

## Proposed interaction and lifecycle

Add a separate **Record audio** affordance beside existing composer attachment controls. It should be available only when the conversation can accept attachments and capture APIs are supported. On the explicit click, explain that the browser will request microphone access and that recording stays local until the owner uploads/transcribes it. Then request permission and show recording state plus elapsed time and Stop. Never request permission on render, page load, chat send, or selection change.

Use a generation token for each recording attempt. Permission completion, worklet messages, stop events, upload completion, and errors may update UI only while their generation and conversation/session scope still match. Stop, Cancel, permission rejection, device loss, unsupported format, time/byte cap, conversation switch, disabled composer, and unmount must all close the worklet port, disconnect nodes, stop every acquired `MediaStreamTrack`, close the `AudioContext`, remove listeners, and release temporary buffers. Stopping yields a local WAV candidate; it must not upload automatically.

Show the candidate name, duration, and size with explicit **Add recording** and **Discard recording** actions. Add recording creates one `File` named `.wav` with `audio/wav`, then calls the existing upload path for the current agent/session. The uploaded attachment becomes a selected ready WAV in the existing list. Only the already-present M56 button can initiate remote transcription. Capability lookup is not a transcription request. No draft insertion or chat send occurs until the existing explicit Use and Send actions.

## Proposed controller rulings for the five risks

These are recommendations for a later root binding; they are not binding decisions.

1. **Container/codec:** accept one browser-generated PCM16 little-endian mono WAV at 16 kHz, at most 120 seconds and 4 MiB total. Why: it directly satisfies the host parser and needs no codec dependency. Cost if wrong: browsers that cannot provide the exact format are unsupported until a reviewed resampling design is approved.
2. **Authority and reuse:** capture only from an explicit click, retain bytes locally until explicit Add recording, then use the ordinary selected attachment upload. Why: this composes with M56's existing scope and retention behavior. Cost if wrong: capture-only users need to use the existing file picker instead.
3. **Permission/resource lifetime:** one active capture per mounted composer; generation-fence all late events and stop tracks/close audio nodes on every terminal path. Why: microphone permission and hardware streams outlive ordinary component callbacks. Cost if wrong: discard the candidate and require a new capture; never keep an orphan stream alive.
4. **Budgets/privacy:** hard-stop at 120 seconds or before the WAV would exceed 4 MiB; do not persist raw samples or recover recording after reload. Why: a finite local buffer bounds memory and makes retention simple. Cost if wrong: reject/discard at the cap and ask for a shorter recording.
5. **Transcription/draft boundary:** keep upload, named remote transcription, preview, Use, and Send as distinct user actions. Why: selected audio transfer has privacy and API-charge consequences, and speech is untrusted input. Cost if wrong: no transcript is inserted or sent; the owner can retry with an explicit action.

## Focused acceptance matrix for a future implementation

Synthetic browser tests should cover: unsupported/missing APIs without permission prompt; permission denied, pending permission then cancel, and permission resolving after scope change; one explicit permission request per recording; generated WAV header fields/length/PCM sign and saturation; exact duration/size boundaries and one-sample-over failure; sample-rate mismatch fail-closed; Stop, Cancel, device-ended, context/worklet errors, cap, scope switch, disable, and unmount all stopping tracks and closing resources exactly once; late worklet/permission/upload results fenced by generation; no upload before Add recording; Discard producing zero upload; Add recording using existing session upload and preserving ready attachment selection; zero transcription calls on capture, stop, upload, normal send, or render; existing selected-file action still requires explicit transcription, preview, Use/Discard, and a separate Send.

Use fake `MediaStreamTrack`, `AudioContext`, worklet port, permissions, and uploader seams for deterministic tests. Do not claim microphone quality or hardware/browser acceptance from these tests. A real microphone/device case remains separately consented and is not part of this held proposal.

### Browser API verification checklist (required before release)

- Confirm microphone access is available only in secure contexts and identify
  how the local development origin satisfies that requirement.
- Confirm how supported browsers report permission denial, device absence,
  device removal, and permission revocation during capture.
- Confirm `AudioContext` accepts the requested 16 kHz sample rate on each
  supported browser and `context.sampleRate` reports the active graph rate.
- Confirm how the browser converts hardware input to that graph rate and mixes
  stereo input into the worklet's single channel.
- Confirm the worklet module can be loaded from the built app in development and
  production, including base paths and content security policy.
- Confirm worklet messages preserve sample ordering and cannot queue without
  bound while the main thread is busy.
- Confirm `AudioWorkletNode` can be disconnected and its message port closed on
  every terminal path, including module/runtime exceptions.
- Confirm `MediaStreamTrack.stop()` releases every acquired track and whether an
  ended event needs separate handling when a device disappears.
- Confirm closing the context behaves predictably while a worklet message is in
  flight; the generation fence must cover any late message.
- Confirm the test matrix includes browsers officially supported by this app,
  not only the developer's current browser.
- Record links and verified behavior in the future implementation decision
  record. Do not rely on remembered API behavior for this release decision.

### PCM and WAV construction constraints

- Use the browser graph's verified 16 kHz rate as the source rate; do not claim
  a particular microphone's native hardware rate.
- Accept only finite Float32 samples. Treat NaN or Infinity as capture failure
  and discard the candidate.
- Clamp each sample to [-1, 1], multiply negative values by 32768 and
  nonnegative values by 32767, then round to an integer.
- Write samples as signed 16-bit little-endian values. Do not serialize native
  platform-endian typed-array bytes as the WAV payload.
- Check both duration and remaining payload capacity before appending each
  chunk; never allocate an over-limit chunk and reject it afterward.
- Use checked integer arithmetic for RIFF size, byte rate, block alignment,
  sample count, elapsed duration, and final Blob size.
- Build the canonical 44-byte PCM header only after Stop and after verifying a
  nonempty whole-frame payload with the exact configured format.
- Validate final byte length at most 4 MiB, duration at most 120,000 ms,
  channels exactly one, sample rate exactly 16,000 Hz and bits exactly 16.
- Do not add metadata chunks, compressed payloads, RF64, resampling metadata, or
  a second format variant in this proposal.
- If a worklet message exceeds remaining duration/byte allowance, stop before
  copying it into retained storage and discard the partial recording.
- Prefer bounded chunks or a preallocated accumulator; avoid retaining duplicate
  Float32 and Int16 copies of the full recording.
- If final `Blob`/`File` creation copies all PCM bytes, include that peak copy in
  the browser memory review and test the 120-second boundary.
- If the browser cannot provide the requested exact graph rate, stop and return
  a resampling design for review. Do not widen the format contract silently.

### State transitions to bind before implementation

| State      | Allowed transition           | Required effect                                             |
| ---------- | ---------------------------- | ----------------------------------------------------------- |
| idle       | explicit Record click        | Explain permission/local retention, request microphone      |
| requesting | permission granted           | Create context/worklet for current generation               |
| requesting | denial, cancel, stale scope  | Stop late tracks; show safe reason or return idle           |
| recording  | explicit Stop                | Stop samples, close resources, validate WAV                 |
| recording  | Cancel or lifecycle boundary | Discard bytes, close resources, return idle                 |
| recording  | device/worklet/context error | Discard bytes, close resources, show retryable error        |
| recording  | duration/byte cap            | Stop immediately, discard partial WAV, explain limit        |
| candidate  | Add recording                | Create WAV `File`; invoke existing scoped upload            |
| candidate  | Discard                      | Clear local WAV and return idle                             |
| uploading  | success                      | Add ready attachment, select it, clear local candidate      |
| uploading  | error/cancel/stale scope     | Reuse attachment errors; do not create a phantom ready item |

The future component should not invent a second source of attachment state.
The attachment hook remains authoritative for uploading, retrying, removing,
and recovering metadata. The recorder owns only the live stream, bounded
pre-upload buffer, and current capture generation.

On a conversation/session change, cancel a recording and discard its candidate.
A candidate must not be carried to a different session implicitly. If upload
has started, use the existing upload `AbortSignal` and current-session checks;
never reinterpret success in another conversation.

## Privacy and user-visible states

- Before the permission prompt, say the browser will request microphone access
  and the recording stays local until **Add recording**.
- Before transcription, retain M56's OpenAI transfer and charge notice;
  recording consent is not transcription consent.
- While recording, provide an accessible status label, elapsed seconds, and
  visible Stop and Cancel actions. Stop must not depend on a keyboard shortcut.
- Explain permission denial, no input device, unsupported capture, duration or
  size cap, device removal, and local upload failure separately.
- Keep the byte buffer in component/runtime memory only. Do not put it in local
  storage, IndexedDB, URL parameters, telemetry, logs, or recovery payloads.
- Do not autosave a candidate, upload on Stop, transcribe on upload, insert text
  on transcription completion, or send a chat message from speech.
- On Discard or failure, drop references to buffers and file/blob objects as
  soon as callbacks and UI state no longer need them.
- Do not claim immediate secure memory zeroization; JavaScript garbage
  collection does not promise it. Bound short-lived references instead.
- Existing browser dictation remains labeled as browser speech service behavior
  and stays independent from raw audio capture.

## Expanded future test plan

Capture controller tests should cover:

- Capture APIs missing: no permission request and a clear unavailable state.
- Successful click: one audio stream and one graph; repeated clicks do not
  create duplicate streams.
- Requested-rate mismatch: close the stream and show unsupported capture.
- Permission denial, absent device, device-ended, worklet error, context error,
  and invalid sample values: terminate with no candidate or upload.
- Stop: produce one valid local WAV and close tracks, nodes, port, listeners,
  and context exactly once.
- Cancel while permission is pending: discard any late-granted stream.
- Late permission/worklet callbacks after Stop, unmount, or scope change cannot
  mutate state or recreate a candidate.
- Duration and size checks happen before appending a chunk and at finalization.
- Exact supported limits produce bounded output; one frame beyond either limit
  is rejected without retaining a partial candidate.
- Header fields, RIFF/data lengths, byte rate, block alignment, signed extrema,
  rounding, and little-endian order match an independent fixture.
- Repeated start/stop/cancel cycles leave no active fake track, message handler,
  connected node, or retained operation.

Composer integration tests should cover:

- Record, Stop, and Discard alone make zero upload, capability, transcription,
  or send calls.
- Explicit Add sends one generated WAV through the existing uploader to the
  current agent/session and shows existing progress/error behavior.
- Successful upload appears as one ready WAV and is selected in the existing
  transcription control.
- Upload rejection/cancellation cannot leave a phantom ready attachment or
  trigger transcription.
- A manually selected existing WAV continues to work without the recorder.
- Normal Send, browser dictation, file upload, capture upload, and session
  recovery stay independent from automatic transcription.
- Capture state clears on conversation change, composer disable, and unmount;
  attachment state follows its existing lifecycle.
- Existing preview editing, Use/Discard and separate Send behavior remain intact.

Use fake devices and worklet data for deterministic coverage. Mark browser API
compatibility assertions as synthetic contract checks. Hardware capture, noise
handling, recognition quality, and remote transcription quality remain
unverified until separately authorized real-device/account acceptance.

## Future implementation lanes and checks

Likely narrow web lane: a capture hook/controller plus a small composer control and focused tests. Touch attachment/upload integration only if current interfaces cannot add the generated `File`; do not alter core WAV validation, host APIs, SDK transcription DTOs, provider settings, service orchestration, or M56 source. Preserve local-only recovery rules: never persist audio bytes or Blob URLs.

After release, inspect the current Nx project/targets and web package scripts before selecting commands. Run focused capture/upload/composer tests and the relevant web test/typecheck/build target through Nx. Check the diff for unrelated M56 edits. Root owns formatting, required gates, ledger, and Git. This proposal itself has no source change and runs no tests/build/format/Git commands.

## Release gate and unresolved questions

Release only after the controller verifies M56 merged and chooses this as the next milestone. Before source work, root must bind the five rulings, verify current official browser API docs and app bundling behavior, choose an explicit supported-browser matrix, and freeze capture-controller/test seams. If `AudioContext` cannot guarantee the requested 16 kHz graph sample rate in that matrix, stop and compare a bounded anti-aliased resampler against the measured cost; do not code speculative DSP in M57.

No microphone, transcription provider/model, credential, account, daemon, database/container, profile, or runtime service is activated by this proposal. No tests/build/formatting or source edits were performed while drafting it.

## Controller rulings on the risks (binding)

M56/PR62 is merged at23f780a2d024924922b0f18a7f66db9313f17cc7, source
8959b07587fbdfbbdf127142429cda1fd9c9ac77, identical tree
2eab189cb9aa1d4ed4bb9ac740277fe32c45e2ba. Branch
g9000/companion-microphone-m57 starts there with142 verified baseline hashes.
These rulings supersede differing held recommendations. They release plan
publication and then a tests-only existing-composer RED. No production capture
source/imports before recorded RED. No actual microphone/provider/model/service,
account, database/container or deployment activation is released.

1. Ruling: record one16kHz mono PCM16 little-endian WAV, with120s sample cap and
   total4MiB cap, and reuse the M56 parser/upload contract — this avoids codec or
   resampler dependencies and produces an actually supported file — cost if
   wrong: fail closed on unsupported rate/browser/device and retain file picker
   and browser dictation. Request16kHz AudioContext and verify its actual rate;
   configure one input/one silent mono output, explicit channelCount1/speakers
   mixing; reject unexpected multichannel/nonfinite input. Never assume128frame
   blocks. Process only the bounded remaining prefix. At the sample cap finish
   a valid local candidate with a visible limit notice; do not discard a valid
   two-minute recording. Empty/malformed input is discarded. No MP3/WebM or DSP
   fallback. This overrides the held proposal's discard-on-cap recommendation.
2. Ruling: one retained capture owner globally per page, with no queue, survives
   cancellation/unmount until pending browser acquisitions and cleanup settle —
   an ignored permission prompt or dropped close await cannot permit unlimited
   late streams/contexts — cost if wrong: Record remains busy/unavailable until
   settlement or reload, while file upload stays available. Only an explicit
   Record click creates/resumes a context or requests getUserMedia; render,
   capabilities, upload and chat do not. Construct/resume within the initial
   user gesture as needed and attach handlers immediately. Capture owner tracks
   permission, module-load/resume and close awaits. Cancel fences all completions,
   stops every late-granted track, closes/disconnects known resources exactly
   once and releases only after observed cleanup; failure/unknown cleanup keeps
   the lease, never silently treats a timeout as physical retirement.
   Publish ready candidate metadata only after known tracks/nodes/ports and
   pending acquisition/context-close work are settled; uncertain cleanup fails
   closed and discards the candidate while retaining its owner lease.
3. Ruling: keep raw PCM in one worklet-owned fixed buffer, send one terminal
   buffer instead of per-quantum sample messages, and fence every callback —
   a blocked main thread must not create an unbounded audio queue — cost if
   wrong: discard the candidate and retire/retain its owner rather than publish
   malformed audio. Max frames1,920,000, max allocation3,840,044bytes. WAV header
   lives in this buffer; transfer the full fixed allocation with checked used
   byte length, then construct File only from the used prefix. Budget at most
   one such transferred buffer plus one bounded File copy per generation; drop
   temporary references, never claim JS secure erasure or total browser memory
   quota. One terminal message, no periodic audio/status messages. A local timer
   updates elapsed UI; a120s wall watchdog requests Stop, and a bounded2s stop
   acknowledgement timeout cancels/cleans without releasing unsettled resources.
   Worklet terminal flag stops writes even if browser continues process calls.
   Browser scheduling/track sharing prevents a strict physical-microphone-off
   wall guarantee: stop owned tracks on callback execution, no claim about other
   apps/tracks using the device. No live monitoring/audible input connection.
4. Ruling: local recording, Add recording, native transcription, Use and Send
   remain distinct owner actions, reusing existing attachment/session authority —
   capture permission alone does not consent to upload or paid transcription —
   cost if wrong: discard stale output and require a fresh action. New Record,
   Stop recording, Cancel recording, local candidate name/duration/size, Add
   recording and Discard recording controls. Add transfers one recording.wav File
   to the existing scoped onAdd callback, which owns upload/progress/errors;
   recorder invents no second upload/session state. Then select the ready WAV
   in M56's existing control. No automatic selection/transcription/draft/send.
   Candidate and events are bound to conversationKey and composer enabled scope;
   navigation, disabled/offline/sending state or unmount cancel/discard and fence
   late permission/module/port/timer/Use callbacks. No storage/URL/telemetry/log
   audio, device IDs or Blob URLs. No preview playback/TTS in this slice. Normal
   file picker, browser dictation, read-aloud and M56 File/STT behavior persist.
5. Ruling: use a self-contained same-origin worklet asset through Vite8.0.10
   explicit URL/no-inline import, with feature detection and synthetic acceptance
   only — this keeps the capture module deployable without Blob scripts, CSP
   relaxation or new dependencies — cost if wrong: capture unavailable and the
   built-asset gate blocks publication. Context7 official v8.0.10 docs and local
   noInlineRE confirm URL import/no-inline asset handling. Verify build emits
   exactly one standalone worklet file and references it, without starting a
   server; evaluate own emitted script under fake AudioWorklet globals in tests.
   Secure context/getUserMedia/AudioContext/AudioWorkletNode feature checks only,
   no user-agent catalog or hardware enumeration. Existing VPS config contains
   no microphone-denying policy; do not edit/deploy it or weaken CSP. Reuse
   Hermes's already-attributed M56 transcription behavior through normal APIs;
   there is no useful Python microphone code to import into the browser. No
   upstream profile/process/credential copy and Anima branding remains.

### Frozen UI/controller and worklet interfaces

- apps/web/src/lib/audio-capture.ts exports `CaptureError` closed reasons:
  unsupported, busy, permission_denied, device_unavailable, device_lost,
  invalid_audio, empty, capture_failed, cleanup_pending.
- `CaptureState` is a metadata-only discriminated union: idle; requesting;
  recording with startedAt:number; stopping; ready with fileName:"recording.wav",
  sizeBytes:number, durationMs:number, limitReached:boolean; error with
  reason:CaptureError. No File/raw buffer in observer state.
- `MicrophoneCaptureController`: start():void, stop():void, cancel():void,
  takeRecording():File|null (once, ready only; clears state), dispose():void.
  `createMicrophoneCapture(onState:(state:CaptureState)=>void)` creates an idle
  controller without permission/context/network side effects.
  `isMicrophoneCaptureSupported():boolean` performs safe feature checks only.
  Lifecycle author may define private/internal test environment/factory seams;
  no caller-supplied URL/device/credential, public backend API or SDK change.
- Worklet name `anima-pcm-capture-v1`, protocol version1, fixed16kHz/mono. Main
  sends `{version:1,type:"stop"}` or `{version:1,type:"cancel"}`. Terminal reply
  `{version:1,type:"complete",buffer:ArrayBuffer,usedBytes:number,frames:number,
limitReached:boolean}` with the fixed bounded allocation and actual WAV prefix;
  or `{version:1,type:"error",reason:"invalid_audio"|"empty"}`. Exactly one
  terminal message; no copied frame messages. Unexpected/bad shape fails closed.
  Main checks version, allocation/used/frame lengths, canonical44byte WAV header,
  rate/channels/format/bits/byteRate/blockAlign/data length, cap and nonempty
  content before File construction; server remains authoritative on upload.
- apps/web/src/lib/pcm-capture.worklet.js is plain self-contained JS, no imports
  or TypeScript/runtime wrapper. One preallocated buffer, DataView little-endian
  writes, finite floats clamped [-1,1], negative*32768/positive*32767 and rounding.
  Maxframes and header constants tested independently against M56's unchanged
  stdlib WAV silence fixture. Terminal transfer drops processor buffer references.
- `MicrophoneCapture.tsx` owns accessible UI only, props conversationKey:string,
  disabled:boolean, onAdd:(files:readonly File[])=>void. The controller is scoped
  to mounted/enabled conversation; stale saved callbacks cannot request capture
  or add a candidate after a boundary. No session lookup until existing onAdd.
  Busy/unsupported/denied/empty/device/cleanup errors have safe static messages.
  Privacy notice visible before Record: local until Add; existing M56 notice
  still names OpenAI transfer/charges before transcription. No extra modal.
- Source lanes after RED: worklet+pure tests; browser controller+tests+asset type
  declaration if necessary; component/composer+tests. Disjoint ownership. Root
  alone fixtures/docs/commands/format/Git/processes, no source-only worker tests.

### Evidence and tests

Verified primary browser APIs (links are implementation evidence, not hardware
acceptance):
https://developer.mozilla.org/en-US/docs/Web/API/AudioContext/AudioContext
https://developer.mozilla.org/en-US/docs/Web/API/AudioContext/close
https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia
https://developer.mozilla.org/en-US/docs/Web/API/MediaStreamTrack/stop
https://developer.mozilla.org/en-US/docs/Web/API/AudioWorkletProcessor/process
https://developer.mozilla.org/en-US/docs/Web/API/AudioNode/channelCountMode
https://github.com/vitejs/vite/blob/v8.0.10/docs/guide/assets.md
Permission may remain pending; close resolves when context-blocking resources are
released but references also need clearing; stop changes owned track state and
does not promise shared hardware shutdown. Worklet block/channel lengths vary.

First RED: existing Composer render/helpers only, attach-capable conversation,
require visible Record audio control and zero permission calls on render. It
currently lacks this control. No new production imports/APIs before that RED.
Then fixture header/sample/sign/clamp/endian/nonfinite/cap/stop/cancel/terminal
tests, variable block lengths/channels, bounded one-buffer transfer and exact
silence equality; denied/absent/unsupported context/rate; pending permission,
module/resume/stop/close and late events; retained global busy lease across
cancel/unmount/cleanup failure; track/device-ended/processor error; exactly-once
known-resource cleanup, timeout without false retirement, silence/no feedback;
scoped ready candidate and once-only Add, no network before Add/no automatic
STT or chat/ordinary convenience regressions; real emitted asset evaluation.

One combined capture/worklet review and one final UI/SDK interaction review,
one fix wave, then relevant Nx web test/typecheck/build, cheap SDK regression
test, owned Prettier/diff/docs/hash gates. No Rust/SDK source change planned;
M56 compiled source hashes must remain unchanged, so do not repeat expensive
Cargo gates without a new Rust change/failure. All checks via direct native
Python/cmd runner, cleared provider/service env, NX_DAEMON=false, no cloud claim.
No new dependencies or real device/browser permission invocation. Actual device
capture/noise quality, remote STT, native output and full Hermes parity stay open.

Explicit staging only, never nx.json/anima.yaml/.superpowers. Per-command unsigned
commits and standing reviewed merge-and-continue authorization persist. Preserve
nxSHA3B71AABE3A3ACF91B43F6C0B4F7F53B486645EB9316D7E66B16F9B85F9DBAC90.
No stash/reset/restore/path checkout/worktree, ordinary profiles/credentials,
real accounts/DB servers/containers/Docker55432/public deployment. M47/M52 actual
cases/releases immutable, no replay/rescue/regrade; M52 strict grade PARTIAL.
