# Selected audio to a chat draft

Anima can upload a selected WAV file and, when the server is explicitly
configured, transcribe it through the OpenAI Platform API. Browser dictation and
browser read-aloud remain separate controls.

## Use it

1. Attach a supported `.wav` file in your own Web Chat session. The daemon checks
   its bytes, saves the original attachment, and returns its metadata.
2. Select the audio attachment and review the remote transfer and API charges
   notice. Choose **Transcribe with OpenAI API** to request transcription.
3. Edit the returned preview, then choose **Use transcript** to append it to the
   current conversation's draft, or **Discard** to clear the preview.
4. Review and send the draft normally. Uploading audio or sending an attachment
   does not itself request transcription.

**Stop** requests local cancellation and discards late results. Changing the
conversation, removing the selected audio, or leaving the view also fences late
results. Cancellation cannot guarantee that OpenAI stopped inference or that no
API charge occurred. A lost cancellation request remains bounded by the daemon's
absolute 45-second operation deadline.

An outer 46-second voice request timeout also bounds request-body and scope-lock
waits before the operation starts. Fresh WAV uploads distinguish unsupported
format (`415`), malformed or excessive duration (`422`), and excessive bytes
(`413`); a changed saved source remains unavailable (`404`) for transcription.

The preview is ephemeral. It is not a saved memory, chat message, or scheduled
task. Once you insert and send it, it follows the ordinary draft and chat
lifecycle. The original uploaded attachment follows the existing attachment
retention and download rules.

## Server configuration

Native transcription is disabled by default. On the server, set
`ANIMAOS_STT_ENABLED=true` and configure an OpenAI Platform API key using the
existing provider environment configuration, normally `OPENAI_API_KEY`. Restart
the daemon through your normal workflow after changing startup configuration.
The browser never receives or supplies the key.

This uses a Platform API key, with its own billing. ChatGPT subscription sign-in
and connector OAuth credentials are not transcription credentials. Keep the
OpenAI provider base at its canonical `https://api.openai.com/v1` setting; a
different provider base disables this transcription adapter. There is no browser
endpoint, model, key, prompt, or header override.

The adapter sends one multipart request to
`https://api.openai.com/v1/audio/transcriptions`, with `model=whisper-1`,
`response_format=text`, and the selected original WAV bytes. It disables
redirects, ambient proxies and retries. The
[OpenAI speech-to-text guide](https://developers.openai.com/api/docs/guides/speech-to-text)
documents that API path. The retained model matches the narrowly ported upstream
behavior; this is not a claim that it is the newest or best transcription model.

## Supported input and limits

- Little-endian RIFF/WAVE PCM format 1, 16-bit samples, mono or stereo.
- Sample rates from 8,000 through 48,000 Hz, nonempty frame-aligned sample data.
- At most 4 MiB per file and 120 seconds, with duration rounded up to milliseconds.
- Strict container length, chunk bounds, format consistency and a 128-chunk cap.
- A UTF-8 plain-text transcript of at most 16 KiB, with no empty or NUL-containing
  result. Outer whitespace is trimmed.

Compressed WAV, MP3, M4A, WebM, RF64, live microphone capture and automatic format
conversion are outside this slice. Unsupported files are rejected; supported
original uploads remain downloadable. Audio sent with a normal chat message
remains a file reference, without hidden transcription or binary model input.

The host admits two local operations without queueing, shares four retained
physical source-read leases with document reads, and the HTTP adapter retains
two outgoing audio-body leases until their physical byte owners are dropped.
These are local resource bounds, not bounds on remote provider jobs.

Operation IDs are owner/agent/session scoped, UUIDv4, and held in a bounded
ephemeral map with short terminal tombstones. Duplicate IDs are refused and an
early cancellation creates a tombstone. Restart clears this map and never
replays transcription; this is not durable exactly-once delivery.

## Acceptance boundary

M56 validation uses synthetic PCM silence, deterministic transcription ports,
and finite loopback HTTP peers. It can establish request shape, source authority,
byte limits, lifecycle and draft behavior. It does not establish spoken-audio
accuracy, live provider availability, billing behavior, microphone support,
native speech output, connector voice support, or full assistant parity.

The Rust transport narrowly ports useful Hermes behavior with its pinned MIT
attribution in the model adapter's [notice](../../packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE). Product controls retain Anima
naming. See the [bound M56 plan](../superpowers/plans/2026-10-16-companion-native-voice-m56.md)
and [parity roadmap](parity-roadmap.md) for remaining work.
