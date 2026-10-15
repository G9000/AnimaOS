# Native speech for a selected reply

Native speech turns one selected saved assistant reply into generated audio.
It is separate from the browser's **Read aloud** control and from microphone
capture or transcription. Nothing generates or plays automatically.

## Use it

1. Open your own existing Web Chat. Choose a reply in **Assistant reply for
   speech**. The list contains at most twenty recent eligible assistant replies
   already loaded in that conversation.
2. Review the selected text and the notice that it will be sent to OpenAI and may
   incur API charges. Choose **Generate speech with OpenAI API**. The console
   checks availability, then makes one speech request if configured.
3. Review the ready state and AI-generated voice notice. Choose **Play speech**
   explicitly. **Stop speech** stops your player; after cleanup settles you can
   replay the same candidate without generating it again.
4. **Discard speech** clears the local candidate. Changing the reply or
   conversation also discards it. Generation has a separate **Stop generating
   speech** action.

The source must be a saved, visible assistant message in that exact own Web Chat.
The server checks its role, text and scope before sending text and before
returning audio. A changed, deleted, hidden or uncommitted source is unavailable.
Speech does not create a chat session, edit the reply, insert a transcript, save
memory, upload an attachment or send a message.

## Server configuration

Native speech is disabled by default. Set `ANIMAOS_TTS_ENABLED=true` on the server
and use the existing OpenAI Platform API-key configuration, normally
`OPENAI_API_KEY`. Apply startup changes through the ordinary operator workflow.
The browser does not receive or submit the key. Enabling transcription does not
enable speech output; ChatGPT subscription sign-in and connector credentials do
not provide the speech key.

Keep the OpenAI provider at its canonical `https://api.openai.com/v1` base. A
different base makes this adapter unavailable. This slice fixes the model to
`gpt-4o-mini-tts`, voice `alloy` and normal speed. It sends one JSON request to
`https://api.openai.com/v1/audio/speech`, without redirects, ambient proxies,
retries, custom instructions or provider fallback.

The [OpenAI speech guide](https://developers.openai.com/api/docs/guides/text-to-speech)
describes the API and AI voice disclosure. The model and voice follow the narrowly
ported upstream OpenAI defaults; this is not a claim of measured speech quality
or support for every provider or voice.

## Limits and lifecycle

- One complete reply of at most 2,048 Unicode scalars and 8,192 UTF-8 bytes.
  Blank or NUL-containing input is refused. Text is preserved exactly, including
  Markdown, without normalization, splitting, truncation or a model rewrite.
- The provider returns raw 24 kHz mono signed PCM16 little-endian samples. The
  adapter bounds them to 120 seconds / 5,760,000 bytes and adds a canonical WAV
  header, for at most 5,760,044 bytes. These output limits are separate from the
  unchanged 4 MiB upload limit for transcription.
- The host admits two operations without a queue. Two physical output leases
  remain attached to returned byte owners, including a slow HTTP response;
  two small request-body leases also follow their outgoing allocations.
- Operations have a 45-second absolute deadline and a 46-second outer request
  deadline, including slow request bodies and scope waits. A bounded ephemeral
  map holds scoped UUIDv4 IDs and terminal tombstones for up to five minutes.
  Restart clears this state and does not replay generation.
- The page retains one speech owner, one bounded candidate and at most one player
  and owned audio URL. A pending request, cancellation or play promise can keep
  another operation busy until cleanup is observed. Unknown cleanup stays
  unavailable until settlement or reload. Audio is not saved in browser storage.

Stop, navigation, disabled/offline/sending state and source changes fence late
results. Local cancellation cannot guarantee that OpenAI stopped inference or
that no charge occurred. Operation IDs do not establish durable exactly-once
delivery. Stop affects Anima's player, without a guarantee about unrelated
applications or physical speakers.

For a long reply, use the existing browser Read aloud action. Native chunking,
custom voices, local/other speech providers, streaming playback, automatic voice
replies and connector delivery are outside this slice.

## Acceptance boundary

Tests use synthetic PCM silence, fake provider/playback objects and finite local
HTTP peers. The independent WAV oracle contains 6,000 frames at 24 kHz: 250 ms and
12,044 bytes. It checks container conversion and lifecycle contracts, not spoken
content, browser/device compatibility, live API availability, billing or voice
quality. No real provider or speaker is activated by those tests.

The narrow Hermes adaptation retains its pinned MIT
[notice](../../packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE).
Product controls retain Anima naming. See [voice input](voice.md),
[harness evidence](harness-acceptance.md) and the
[parity roadmap](parity-roadmap.md) for remaining acceptance work.
