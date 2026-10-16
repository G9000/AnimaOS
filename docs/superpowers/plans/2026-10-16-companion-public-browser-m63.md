# M63 one pinned public HTTPS browser policy

Status: bound from verified M62/PR68 main. Publish this plan before source-only implementation; no public-site/browser/account activation is released.

## Decision and acceptance boundary

The useful small extension is **one operator-installed public HTTPS DNS hostname**
with an explicit single public IPv4 pin, implicit port 443, and an exact finite
list of GET routes. It uses the existing ordinary owner-chat run, installed
policy, explicit tool grant, approval, scope, limits, cancellation, and physical
lease rules. Missing configuration means unavailable. No model argument can
select a URL, hostname, IP pin, path, method, headers, proxy, or TLS options.

The current numeric `http://127.0.0.1:<port>` policy and its exact GET/POST
routes remain valid without a pin. A public policy has no POST route or selected
input/upload. Do not add arbitrary browsing, wildcard hosts, IPv6, CDN address
rotation, DNS lookup/refresh/fallback, accounts/cookies, authenticated sessions,
public activation, or an additional standing grant. Browsing page content is
untrusted evidence, never policy authority. Continue to describe the proxy as
an application boundary, not OS-wide network isolation.

Root must bind five decisions before any public source release:

1. The public syntax is exactly `https://` plus a canonical lower-case ASCII
   multi-label DNS name, with no explicit port (including `:443`), userinfo,
   trailing dot/slash, query, fragment, percent escape, IP literal, or local
   suffix. Port 443 is implicit. Reject rather than normalize. Decide the exact
   suffix deny list and its test examples; certificates must still validate.
2. A single canonical dotted-quad IPv4 pin is mandatory in the operator
   template. Reject leading-zero, shorthand, signed, octal, hex, mapped IPv6,
   and every known IANA special-use prefix conservatively. The static table is
   versioned with tests; it is not a promise about future allocations.
3. Public policies contain only `GET` routes and no selected inputs. Existing
   route bytes, IDs, cap of 16, strict paths, and all action/request/wire/time
   limits stay. Typed GET downloads may use the current bounded CSV path;
   native downloads, redirects and non-listed resources remain refused.
4. The worker makes each allowed public request directly to the numeric pin,
   while TLS SNI, certificate-name verification, and HTTP `Host` all use the
   configured DNS hostname. No resolver is called and no other address is tried.
   The response/stream and socket must physically close before its owner is
   released; uncertainty retains the existing lease/quarantine behavior.
5. This is an operator-installed destination, not a new permission. The run,
   installation, policy digest, current scope and approval/grant checks must be
   unchanged. Public policy metadata should expose the origin and route IDs,
   not the pin as a new model-controlled tool parameter.

Primary transport evidence for the later binding: Node 22.12.0 HTTPS
[`https.request` options](https://nodejs.org/download/release/v22.12.0/docs/api/https.html)
and [TLS `servername`/identity checks](https://nodejs.org/download/release/v22.12.0/docs/api/tls.html)
show why an IP target must supply the logical DNS name explicitly. Use the
[IANA IPv4 Special-Purpose Address Registry](https://www.iana.org/assignments/iana-ipv4-special-registry)
as the reviewed source for a conservative deny table; never infer that a
syntactically valid dotted quad is public.

## Current seam and path ownership

- `packages/core-rust/crates/anima-core/src/browser.rs` validates
  `BrowserScope`, origin, routes and operations, and computes the canonical
  scope digest. The port remains `BrowserPort::open(scope, control)` and
  `BrowserSession::execute(operation, control)`; no general URL operation.
- `hosts/rust-daemon/src/browser/config.rs` reads the startup-only verified
  installation and policies. `records.rs::InstalledBrowserPolicies::new`
  independently enforces the numeric-loopback policy today. Both checks must
  admit a public template only when the pin and GET-only route policy pass.
- `hosts/rust-daemon/src/browser/service.rs` remains the authority for owner,
  run, active scope, grant, admission, budgets, receipts, cancellation, and
  physical lease. `transport.rs::PreparedBrowserTask.scope` carries the sealed
  template to the worker bootstrap. Do not add a public endpoint or a model
  request field for the pin.
- `packages/core-rust/browser-worker/worker.mjs` mirrors strict scope checks,
  intercepts all HTTP in `guardRequest`, and currently calls injected/global
  `fetchImpl` from `guardedResponse`. Its context has a deny-only proxy,
  blocked service workers/downloads, manual redirects and
  `ignoreHTTPSErrors:false`. Public network transport belongs here, below the
  existing guard and above `readBoundedResponse`; leave loopback fetch intact.
- `worker.test.mjs`, core browser tests, host `browser/tests.rs` and
  `browser/transport_tests.rs` own regressions. The later implementation may
  update `docs/companion/browser-work.md` to disclose the operator contract;
  this private draft changes no public documentation.

## Proposed data contract

Add an optional operator-issued `publicIpv4Pin` field to `BrowserScope`, with
serde omission when `None`. The existing loopback JSON bytes and digest must
remain unchanged. In Rust, `validate_scope` accepts exactly either:

```
Loopback: origin=http://127.0.0.1:<canonical nonzero u16>, publicIpv4Pin absent
Public:   origin=https://<canonical DNS hostname>, publicIpv4Pin=<public IPv4>
          all routes GET, inputIds empty
```

The optional field is scope admission data, not a grant. `scope_digest` includes
it for public scopes so a pin change invalidates the witness. Worker
`validateScope`/`parseBootstrap` mirror the same pair, and the host's installed
policy constructor independently rejects mismatches. Avoid a loose URL parser
that silently canonicalizes a malformed operator string; compare the exact
serialized form. No change to `BrowserOperation` or to limits. Existing Rust
scope literals/tests need the new `None` field; a loopback fixture must prove
its old canonical digest is stable. Do not serialize the pin in tool arguments,
policy lists, observation provenance, errors or logs.

Define one shared Rust `public_ipv4_pin` validator near `origin` for core and
have host records call it through `validate_scope`, plus its own explicit
public/loopback policy check. Mirror a reviewed deny-prefix table in the worker;
cross-language vector tests must match. Reject **all** entries in the current
IANA IPv4 special-purpose registry (last updated 2025-10-09), even entries
marked globally reachable, plus 224/4 and explicitly documented platform
addresses such as `168.63.129.16`. At minimum cover 0/8, 10/8,
100.64/10, 127/8, 169.254/16, 172.16/12, 192.0.0/24, 192.0.2/24,
192.88.99/24, 192.168/16, 198.18/15, 198.51.100/24, 203.0.113/24,
224/4 and 240/4, including prefix boundaries and all candidate addresses
the IANA review identifies. Document the static vector set; it does not prove
that every remaining address is reachable or provide OS egress isolation. If
Rust/JS classification cannot be kept byte-for-byte equivalent, keep the
feature unavailable rather than trusting only one side.

The operator's version-1 startup JSON can place `publicIpv4Pin` in the one
policy `template`. The loader must enforce at most one public policy across
its otherwise existing policy list. This needs no second config path, DNS
data or worker-provided pin. The installation is still verified once at
startup, and the worker script hash changes with a deliberate operator
reinstall/restart.

## Pinned public request path

Keep the production HTTPS helper inside the existing witnessed
`packages/core-rust/browser-worker/worker.mjs`; do not add an unwitnessed
sibling module/import or broaden the installation integrity claim. After
`guardRequest` has charged and accepted an exact current-page, main-frame
GET request, choose the public transport by the validated scope kind. Construct
the target from the configured origin and route path only. Node's ordinary
`fetch` has no supported per-request address-pin contract in this worker, so
do not pass a public URL to it. A narrow `https.request` adapter using built-in
Node modules can present a fetch-like response to the existing
`readBoundedResponse`:

```
https.request({
  hostname: scope.publicIpv4Pin, port: 443, path: route.path,
  method: 'GET', servername: configuredDnsName,
  headers: { Host: configuredDnsName, 'Accept-Encoding': 'identity' },
  rejectUnauthorized: true, agent: false, signal,
  maxHeaderSize: 8192
})
```

This sketch is the security-critical transport shape, not a promised exact
Node call signature. Root must check the pinned Node documentation and test
actual behavior. In particular, an IP `hostname` alone does not give the
right SNI; set `servername` explicitly. Keep default certificate trust and
name verification; never set `rejectUnauthorized:false`, disable
`checkServerIdentity`, or rewrite the URL to an IP. Do not use `lookup`, DNS,
fallback, redirect-following, a proxy, connection pool, retry or automatic pin
refresh. Send no browser cookies, authorization, referrer or arbitrary request
headers. For public GET, forward no page-supplied `content-type`.

Adapt incoming status/headers/body to the interface
`readBoundedResponse` already consumes (`status`, iterable `Headers`, a
single reader). Preserve `redirect:'manual'` semantics by accepting only 2xx;
do not dispatch a `Location`. Preserve identity encoding: reject any
`Content-Encoding` other than absent/identity so compressed bodies never evade
the byte cap. Keep the existing 8 KiB header, 64 KiB asset, 16 KiB CSV,
1 MiB aggregate wire, 80 request, 10-second per-request and task deadline
checks. A declared length over cap fails before body retention; chunked
responses are counted on each consumed chunk. Never buffer first and check
later. A failed request consumes its admitted request budget.

The adapter must own the `ClientRequest`, `IncomingMessage`, socket and close
acknowledgement from dispatch until physical settlement. Abort/deadline/Stop
destroys both request and response, and `createMaintainedSession.close()` joins
every public request owner after its existing `routeTasks` before claiming
`engineClosed:true`. A stalled or uncertain close cannot free the Rust
physical permit; use its established quarantine/kill path. A late response
must not fulfill a page after cancellation or start a second connection.

## Finite tests, in implementation order

1. **Core RED:** exact public origin + pin succeeds; missing pin, pin on
   loopback, malformed host, explicit port, path/query/userinfo, non-ASCII,
   IP literal, GET/POST mix, nonempty inputs, unknown fields and oversize
   fail. Verify loopback GET/POST and its digest/serialization are unchanged;
   changing only the public pin changes its digest. Use independent IANA
   boundary vectors, including all private, metadata, CGNAT, documentation,
   benchmark, multicast and reserved classes.
2. **Host RED:** the startup loader and installed-policy constructor refuse
   public origin without a pin, a forbidden pin, a POST, or a second public
   policy; accept one operator template. A public host still cannot
   save/open without current ordinary chat run, installed policy, explicit
   browser tool grant and current approval. Policy list does not reveal pin as
   an actionable model option. Existing loopback tests remain unchanged.
3. **Worker pure RED:** mirrored scope validator agrees with Rust vectors;
   request guard accepts only exact same-origin main-frame GET route, counts
   refusals and budgets, and rejects POST, unlisted path, subresource, frame,
   popup, websocket, redirect, downgrade, port switch and content-encoding.
   Ordinary `fetchImpl` spy sees zero public calls; loopback retains its old
   injected fetch behavior.
4. **Transport-option RED:** inject only the low-level request factory into
   the public adapter, capture one call and assert numeric pin as target,
   port 443, DNS `servername` and `Host`, certificate verification true,
   `agent:false`, GET, identity encoding, no cookie/authorization/proxy,
   no lookup/retry, and no second attempt on socket/TLS failure. This seam
   must not be reachable from worker bootstrap/IPC or model input.
5. **Finite local TLS peer:** a test-only request-factory seam may redirect
   the physical test dial to a loopback TLS server with an ephemeral port and
   inject a synthetic test CA; the _logical_ DNS host, SNI, Host and
   `rejectUnauthorized:true` remain. Positive cert for that name returns a
   bounded fixture. Wrong-name and untrusted certs both refuse, with zero
   redirected/fallback requests. Include abort during headers and body, late
   close, and exactly-once socket retirement. Never install test CA or dial
   override in `runCli` or production bootstrap.
6. **Lifecycle and integration:** prove redirect status, denied resources,
   chunked overrun, concurrent aggregate cap, Stop, deadline, source/grant
   change, uncertain close and lease retention. Preserve existing M46 test
   witnesses, strict worker environment, deny proxy, fresh context and
   process ownership. No public DNS/socket or provider/service is contacted.

If the existing `readBoundedResponse`/`routeTasks` seam cannot represent a
socket until close acknowledgement, the minimal alternative is a small
public transport owner in `worker.mjs` with a response adapter and explicit
`close()` obligation. Do not weaken the byte stream checks or free a permit
on cancellation alone to avoid that change.

## Top five risks for root review

1. **Pin bypass:** passing the DNS URL to ordinary `fetch`, using a resolver,
   or retrying another address loses the verified destination. The low-level
   request options and live local TLS test must prove the actual dial target.
2. **TLS identity drift:** connecting by IP without explicit DNS SNI/name and
   Host can authenticate the wrong peer. Wrong-name and untrusted certificates
   must fail with normal verification enabled.
3. **Special-use classification:** an incomplete or mismatched Rust/JS deny
   table could admit a private, metadata, documentation or reserved address.
   Root should sign off the IANA-derived static vectors and boundary tests.
4. **Retirement uncertainty:** an aborted read can settle its promise before
   the socket closes. The worker must retain the request/socket owner, and
   Rust must retain/quarantine its physical lease until witnessed closure.
5. **Authority expansion:** a public origin could accidentally grant POST,
   input upload, extra policies or a standing browser permission. Enforce
   GET-only/one-policy at load, and preserve run/grant/admission checks.

## Verification after root authorizes implementation

Use the repository's Nx targets, with no live public host:

```
bun x nx run core-rust:browser-worker-test --skipNxCache
bun x nx run core-rust:test --skipNxCache
bun x nx run rust-daemon:test --skipNxCache
bun x nx run core-rust:lint --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run rust-daemon:build --skipNxCache
```

On Windows, if a running daemon locks the default build executable, use the
AGENTS.md isolated validation target for `rust-daemon:test`. Record exact
focused and full results; a synthetic local TLS peer proves wiring and TLS
identity under a test CA, not live public reachability or provider behavior.

## Reuse and provenance

Pinned Hermes revision `1fd18277267152893a8a24792b79213e7ca3f899`
(`tools/browser_tool.py::_url_is_private`, `_hybrid_routes_locally`) is a
behavior reference for private-address refusal. Direct reuse would bring its
Python browser supervisor and DNS-based routing decision; an adapter would
introduce a second process/authority; neither supplies connection-time
pinning here. Choose a narrow attributed implementation in the existing Rust
validator and Node worker using Node HTTPS. Do not copy Hermes code. If any
copy is later proposed, inspect the pinned license first, record exact
source/commit and retained local changes, and preserve the required notice.

## Controller rulings on the risks (binding)

1. Ruling: add optional omitted-None publicIpv4Pin to the host-issued scope, with exactly one installed public policy, GET routes only and no selected inputs — this extends the existing operator destination without granting tools or arbitrary navigation; loopback bytes/digest and all run/grant/approval checks stay — cost if wrong: admission tests fail and public policy remains unavailable; no SDK/API/model-selected pin field.
2. Ruling: require a lower-case ASCII multi-label DNS hostname, maximum253 bytes/63 per label, alphanumeric label ends and alphabetic character in final label; reject explicit port, slash, trailing dot, userinfo, percent/query/fragment and suffixes localhost/local/internal/lan/home/onion/test/invalid/example/arpa — canonical strings and fixed port443 avoid parser ambiguity — cost if wrong: unsupported destinations refuse rather than normalize; no reachability/general-web claim.
3. Ruling: strict canonical dotted IPv4 pin, deny all reviewed IANA special-purpose prefixes plus224/4 and Azure168.63.129.16; the complete collapsed table is 0/8,10/8,100.64/10,127/8,169.254/16,172.16/12,192.0.0/24,192.0.2/24,192.31.196/24,192.52.193/24,192.88.99/24,192.168/16,192.175.48/24,198.18/15,198.51.100/24,203.0.113/24,224/4,240/4 and168.63.129.16/32 — Rust/JS boundary vectors must agree on this static conservative set, including globally reachable special-purpose exceptions — cost if wrong: public admission fails; no future-allocation/OS-egress-isolation guarantee. Sources: IANA registry lastupdated2025-10-09 and [Microsoft platform-address documentation](https://learn.microsoft.com/en-us/azure/virtual-network/what-is-ip-address-168-63-129-16).
4. Ruling: numeric-pin Node HTTPS request with logical DNS SNI/Host, normal certificate/name verification, agent:false, identity encoding, fixed headers and bounded streamed reads; never DNS/ordinary-fetch/fallback/retry/redirect/pool — the actual connection must preserve both address and TLS identity — cost if wrong: transport tests refuse and no public case releases. Every admitted request owns request/response/socket through close acknowledgement; abort destroys and joins, uncertainty retains existing physical lease/quarantine. Keep helper inside the witnessed worker script; no bootstrap-accessible CA/dial override.
5. Ruling: source-only Sol native contract/test lane and Astra connection/lifecycle lane, root-controlled RED/fix gates, one native/worker batch review and one final web/SDK contract review, one bounded fix wave — a new network boundary merits focused security/concurrency work while unchanged browser UI/SDK need scope review rather than redundant suites — cost if wrong: no acceptance/publication; full Rust/core/worker/format/build and owned docs/hash gates remain required. Synthetic local TLS peers are allowed under existing isolated testing authority; no real public host, DNS, account, ordinary profile, browser or daemon activation.

## Execution and resume

Root alone runs commands, formatting, tests and Git. Source-only workers first
write meaningful failing tests, freeze them for root RED, then implementation
release. No new dependency is needed. Pin belongs only in operator scope/config;
public policy/list/tool schemas remain unchanged. Small mechanical None additions
in existing Rust scope literals are allowed and must be explicit in source paths.
The API inventory/source may be reviewed without broadening the change.

Every Cargo invocation uses CARGO_INCREMENTAL=0/jobs1/threads2, serialized to actual
exit. Use approved native file permissions for atomic test persistence. Prefix Nx
with bun --no-env-file and scrub provider/service environment names. Never stage
nx.json/anima.yaml/.superpowers; no stash/reset/restore/path checkout/worktree.
Per-command unsigned commits and reviewed merge-and-continue remain authorized.
M47/M52 consumed cases and all four failed M61 cases remain immutable; M52 PARTIAL.
No ordinary profiles/credentials, account sends, devices, DB servers, containers,
Docker55432 or deployment. Update master/parity/harness/operator docs/handover,
push/PR/attach/standing merge and verify exact source/merge tree plus raw hashes.
Resume .superpowers/sdd/2026-10-16-companion-public-browser-m63/progress.md.
