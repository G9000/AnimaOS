# @animaos/web

Anima's companion console talks to the Rust daemon through `@animaos/sdk`.
The chat front door presents one persistent companion; retained agents and bounded
helpers remain behind it. The separate `apps/playground` is a different surface.

The console provides session navigation, streaming replies, queue/steer/Stop,
approval review, skills, automations, editable memory, usage, logs and health.
Messages use accepted session `/runs` and the agent `/events` stream. Reload
rejoins an existing run from a fresh snapshot; it does not automatically replay
tools. An absent `/api/status` or other required console endpoint shows **Update
the daemon**. Authentication failures are not labeled an old daemon, and the
console does not fall back to the legacy blocking `/run` endpoint.

Attachments upload into the configured workspace, with progress, chips,
authenticated previews/downloads and exact-retry ID recovery. Limits are ten IDs
per input, 10 MiB images, 1 MiB text and 25 MiB documents. Known vision models
can receive four selected images; other models receive references. Documents are
saved/downloadable, with an explicit unread-file note. Session deletion removes
metadata and does not physically delete saved files.

Dictation and read-aloud use browser speech APIs when supported, with microphone
permission for dictation. They cancel when the conversation becomes inactive.
Browser speech may use an external service; these are not daemon voice APIs.
Model text is rendered as text with hidden characters revealed.

## Local development and checks

The normal operator workflow starts the selected Rust host and browser surfaces:

```sh
bun dev --host rust
```

Open the console URL printed by `workspace-dev`. Host selection belongs in that
launcher. The console uses the loopback daemon on `127.0.0.1:8080`; the Rust host
wraps reusable `packages/core-rust` code. Elixir/Python hosts are placeholders.

```sh
bun x nx run @animaos/web:build
bun x nx run @animaos/web:typecheck
bun x nx run @animaos/web:test
```

For offline browser evidence, see [web-e2e](../web-e2e/README.md). Its explicit
`e2e-console` target loads built assets and simulated APIs/SSE without a server.
It proves browser/SDK behavior against fixtures; real provider, microphone,
gateway/TLS and deployment acceptance remain separate checks. See the
[daemon API](../../hosts/rust-daemon/README.md) and
[VPS procedures](../../deploy/vps/README.md).
