# @animaOS-SWARM/web-e2e

Browser coverage for `@animaOS-SWARM/web`, the companion console.

## Offline console journeys

```sh
bun x nx run @animaOS-SWARM/web-e2e:e2e-console --skipNxCache
bun x nx run @animaOS-SWARM/web-e2e:typecheck --skipNxCache
bun x nx run @animaOS-SWARM/web-e2e:deployment-contracts --skipNxCache
```

`playwright.console.config.ts` has no `webServer`. Its target builds SDK/web
dependencies, then serves built assets by browser route fulfillment. Fake APIs
validate current session/run requests; browser-local streams deliver real SSE
bytes to the SDK parser and normal reducers. Unexpected requests fail the fixture.
No daemon, dev server, database or container is started. Deployment contracts
inspect configuration files and never import the service-starting gateway suite.

Journeys cover send/tool/final reply, approval, Stop, reload/reconnect, mobile
navigation, attachment-only input and older-daemon compatibility. Fixture state
survives page reload while stream readers are replaced. Tests exercise observable
UI and request contracts rather than injecting finished component state.

Use installed browsers; no browser download is required or performed by this
target. On Windows the console configuration uses the installed Edge channel;
other hosts use packaged Chromium. It creates isolated test contexts, not the
owner's persistent profile. Failure traces/screenshots are diagnostic artifacts.

Simulated browser passes do not establish real HTTP chunking, selected-model
streaming, microphone/TTS support, Caddy/TLS, Linux keyring or container restart
behavior. See [deployment acceptance](../../deploy/vps/README.md).

## Service-starting suites

The default `e2e` and companion configuration suites use preview/dev
servers. They are retained for separately authorized runtime
testing and excluded from the no-service milestone gates:

```sh
bun x nx run @animaOS-SWARM/web-e2e:e2e
```

Do not use the default target when services are prohibited.

## Isolated memory HTTP acceptance (M61)

The normal `bun x nx run @animaOS-SWARM/web-e2e:e2e-console --skipNxCache`
remains offline: 18 cases pass, including a held-response memory editor and
manual-redirect regression. The actual memory HTTP/restart case is skipped unless
an explicit fresh M61 root release enables it. Do not enable it against a normal
profile or start the ordinary Playwright development-server configuration for it.

The manual acceptance uses private, unstaged Windows process-ownership helpers,
fresh JSON/SQLite-file stores, ports18752/18753 and one bounded restart/stop. It
bridges only the declared memory routes; bootstrap/events/accounts/providers and
connectors remain mocked. Natural browser requests preserve owner Fetch Metadata;
the memory-only guard refuses automatic redirects. Each live release/result is
single-use; failed evidence is retained. See the [bound plan](../../docs/superpowers/plans/2026-10-16-companion-memory-http-m61.md)
and [acceptance evidence](../../docs/companion/harness-acceptance.md) for exact scope
and limitations. This test does not provide general provider or host acceptance.
