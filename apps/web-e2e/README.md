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

The default `e2e`, companion configuration and `e2e-live` suites use preview/dev
servers or live harnesses. They are retained for separately authorized runtime
testing and excluded from the no-service milestone gates:

```sh
bun x nx run @animaOS-SWARM/web-e2e:e2e
bun x nx run @animaOS-SWARM/web-e2e:e2e-live
```

The live suite checks legacy websocket integrations; it does not substitute for
the current session/SSE console contract. Do not use default/live targets when
services are prohibited.
