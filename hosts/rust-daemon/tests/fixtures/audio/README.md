# M56 selected WAV fixture

`m56-selected.wav` is an independently generated RIFF/WAVE fixture made with the
Python standard library `wave` module. It contains 4,000 zero-valued PCM16 mono
frames at 16,000 Hz: 250 ms of synthetic silence, with no spoken content.

- File size: 8,044 bytes.
- SHA-256: `59460d5690616336b990fc7b1629428e3bd825e422da84469d2c8c8ecfaff43b`.
- It tests strict WAV admission, original-byte download, selected-source identity,
  and deterministic transcription plumbing. It provides no evidence of speech
  recognition accuracy or actual provider behavior.

Keep the fixture unchanged when fixing code. Malformed and boundary inputs are
constructed separately by the core and host tests.
