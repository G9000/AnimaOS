# Browser work

Browser tasks support owner-approved public HTTPS destinations with JavaScript,
forms, typed controls and bounded document downloads. The owner can take over the
dedicated browser page for sign-in, then return it to the agent. Fixed-route
loopback and static-page policies retain their original limits.

The Rust daemon owns task authority and Windows process lifetime. A small worker
uses locked Playwright-core 1.64.1 and a fresh, sandboxed Chromium context. It receives no
personal browser profile, existing cookies, CDP connection or model-selected code,
selectors, executable paths or launch options. Application request interception
does not provide OS network isolation or bound the browser engine's internal memory.

## Websites and dedicated sign-in

On Windows with Node 22 or 24, install the locked workspace dependencies and run
`bun run browser:setup`. This explicit setup installs the paired full Chromium
156.0.8078.4, checks its version in an owned browser process, and records the
program/script hashes under `%LOCALAPPDATA%/AnimaOS/browser/setup.json`.
Restart AnimaOS to load the installation. After worker updates, run
`bun run browser:setup --check` and restart to refresh its hashes. No model tool
can install a browser or select its executable.

In a chat, open **Browser task → Websites and sign-in**. Enter the starting HTTPS
URL and any required additional HTTPS origins, review the list, and add the
website. The starting origin is included; at most eight origins are authorized.
The daemon resolves them once and freezes public IPv4 pins. Reading or creating
this metadata starts no browsing and grants no tools. Explicit browser tool
access and action approvals still apply when the task runs.

For a site requiring sign-in, create a dedicated account, choose **Sign in**, use
the separate browser window, then choose **Finish sign-in**. Cookies and local
storage are sealed outside the workspace; no personal browser profile is attached.
Cookies are forwarded only to the account's primary origin. An account marked
ready means the owner confirmed saving that state, rather than a live provider
health check. Websites must be reauthorized after daemon restart; identical
destination settings retain the logical account binding. **Remove website**
revokes its accounts and closes owned workers.

During an active interactive task, **Take control** pauses model operations and
invalidates its page references. **Return control to agent** requires fresh
observation and approval checks. If an action is busy, refresh after it settles
and request control again; dispatched effects are never replayed. These controls
preserve the original deadline. **Stop browser** retains physical ownership
until process and socket cleanup completes.

The initial interactive limits are 40 actions, 80 requests and 120 seconds,
including blocked/background requests. Each resource or accepted download is at
most 4 MiB, with a 32 MiB aggregate network budget and four output slots.
CSV, text, JSON, PDF, PNG and JPEG use generated filenames and owned attachment
IDs; native browser-managed downloads remain disabled. Public pages receive no
uploaded workspace files.

Only approved origins can be contacted through guarded requests. Private
networks, personal profiles, popup/frame sign-in flows, WebSockets, service
workers, IDN and IPv6-only destinations remain unsupported. Additional CDN or
identity-provider origins require owner authorization; this does not promise
compatibility with every site or SSO flow. Pins do not follow changing DNS.
Controlled acceptance and real-account compatibility are separate checks.

## One pinned public HTTPS site

M64 prepared an ignored real-page test with offline admission checks. Its single independent source read did not establish the required heading, so no pin lookup or real browser case ran. Public rendering remains unverified; ordinary unit-test success does not change that status.

M63 adds an optional static-page mode. It is tested with synthetic local TLS
peers; actual public browsing/rendering remains unverified. Keep the complete
installation/template fields below, replacing the destination fields with:

```json
{
  "origin": "https://docs.example.com",
  "publicIpv4Pin": "<reviewed-canonical-public-IPv4>",
  "routes": [{ "id": "docs", "method": "GET", "path": "/" }],
  "inputIds": []
}
```

Only one public policy may be installed alongside existing loopback policies.
The hostname must be canonical lower-case ASCII, without a port, slash, local
suffix or xn-- label. The reviewed IPv4 address must pass the static conservative
special-use deny list. Node connects only to that address while checking the
normal certificate for the hostname; it never resolves, refreshes or falls back
to another address. A changed address needs deliberate operator config/restart.
Update the worker script hash after upgrading it. Omission keeps public browsing
unavailable; configuration still does not grant browser tools or Exec approval.

Only exact main-frame GET routes and bounded typed CSV downloads work. Redirects,
other resources, scripts, POST, cookies/accounts and selected uploads refuse.
The console explains that files are not shared and hides its public sharing
checkboxes; ordinary chat attachments stay intact. JavaScript sites, arbitrary
links, IDN/IPv6 and rotating CDN addresses are outside this mode. Request guards
and the deny proxy do not provide OS network isolation.

The [IANA registry](https://www.iana.org/assignments/iana-ipv4-special-registry)
supplies the reviewed static special-use classes (2025-10-09 snapshot), with
additional conservative multicast/platform exclusions. This is a destination
policy, not a reachability guarantee. TLS behavior follows the pinned
[Node22.12 HTTPS](https://nodejs.org/download/release/v22.12.0/docs/api/https.html)
and [TLS identity](https://nodejs.org/download/release/v22.12.0/docs/api/tls.html)
contracts. Test certificates are never installed or accepted by production config.

## Manual operator configuration

Install the workspace dependencies using its pinned lockfile. Supply an absolute
Node 22 or 24 executable, the tracked
`packages/core-rust/browser-worker/worker.mjs`, and an absolute supported Chromium
browser executable from the locked Playwright pairing. Interactive and sign-in
work require the full browser, rather than the headless shell. Earlier M46/M77
acceptance used Node22.12.0, Playwright-core1.59.1 and Chromium headless revision1217;
those are historical results and do not establish compatibility with this worker.

Create a version-1 JSON configuration outside the agent's workspace authority.
Set `ANIMAOS_RS_BROWSER_CONFIG_FILE` to its absolute file path before starting the
real daemon. Deterministic test/embedding constructors do not load this variable.
If this variable is omitted, the daemon uses the explicit setup record described
above when present. Without either configuration, browser work is unavailable.
Invalid configured installations fail
validation rather than selecting another program from PATH.

Calculate lowercase SHA-256 for each program/script:

```powershell
(Get-FileHash -LiteralPath 'C:\path\to\node.exe' -Algorithm SHA256).Hash.ToLowerInvariant()
(Get-FileHash -LiteralPath 'C:\path\to\worker.mjs' -Algorithm SHA256).Hash.ToLowerInvariant()
(Get-FileHash -LiteralPath 'C:\path\to\chrome.exe' -Algorithm SHA256).Hash.ToLowerInvariant()
```

Replace every angle-bracket placeholder in this example. The origin and routes
must describe the controlled application you intend to use:

```json
{
  "schemaVersion": 1,
  "installation": {
    "id": "local-chromium",
    "nodeExecutable": "<absolute-node-path>",
    "nodeSha256": "<lowercase-sha256>",
    "workerScript": "<absolute-worker-path>",
    "workerSha256": "<lowercase-sha256>",
    "browserExecutable": "<absolute-browser-path>",
    "browserSha256": "<lowercase-sha256>"
  },
  "policies": [
    {
      "id": "local-quotes",
      "template": {
        "id": "operator_template",
        "revision": 1,
        "origin": "http://127.0.0.1:43220",
        "routes": [
          { "id": "home", "method": "GET", "path": "/" },
          { "id": "app", "method": "GET", "path": "/app.js" },
          { "id": "quote", "method": "POST", "path": "/quote" },
          { "id": "quote_csv", "method": "GET", "path": "/quote.csv" }
        ],
        "inputIds": [],
        "limits": {
          "actions": 40,
          "requests": 80,
          "scopeMs": 120000,
          "frameBytes": 65536,
          "wireBytes": 1048576,
          "observationNodes": 128,
          "observationTextBytes": 8192,
          "inputBytes": 16384,
          "outputBytes": 16384,
          "screenshotBytes": 262144
        }
      }
    }
  ]
}
```

The daemon issues the actual run/scope identities and selected input IDs. The
template cannot grant tool access. Installed policy and program witnesses remain
fixed for that daemon instance; a worker/script update requires updating its
operator witness and restarting the daemon.

## Using a task

In Settings, explicitly allow the browser tools you need: navigation, observation,
interaction and file transfers. Existing Exec approval rules still apply. In a chat,
open **Browser task**, refresh its status, choose an installed policy and select
the uploaded text files to share. Send the task normally. Busy or ineligible runs
are refused rather than waiting with browser authority. Browser selection applies
once and is not restored from a recovered draft or page reload.

Reading policies/scopes or changing their metadata starts no browser. The first
admitted tool call must pass current run, scope, grant, approval and action-receipt
checks. A scope expires after its original deadline; it is never renewed by a retry
or restored after daemon restart. A repeated message key returns its original run
without reopening the scope.

Refresh **Browser task** to inspect counters, status and accepted output files.
**Stop browser** requests retirement using the current server revision. An open or
unconfirmed physical lease keeps the task slot occupied. Accepted CSV outputs use
the existing owned attachment download and remain available after normal browser
cleanup. Original uploads remain intact.

Typed downloads resolve the current observed link to an exact configured GET
route, then use the same guarded, bounded HTTP fetch as intercepted resources.
They do not click the link or trust a browser-managed download. Native downloads
are disabled and unexpected download events retire the task. An owned deny-only
loopback proxy refuses browser HTTP that bypasses interception; it never forwards
requests or CONNECT tunnels. Refusals count against the task budget, including
browser startup traffic. This application boundary is not OS network isolation.

The controlled quote task produced the exact75-byte CSV, one declared form POST,
12 succeeded action receipts and an accepted JPEG. Scope Stop retained the CSV;
separate runs checked ordinary Stop, redirect refusal, blocked links and a
page-initiated native download with zero sink requests. Independent Windows process
witnesses covered all five tasks and their removed private directories. A control-plane
snapshot restore interrupted unfinished runs without restoring browser contexts.
These zero-model cases do not establish autonomous provider planning, authenticated
websites, public browsing or another host/browser installation.

Observations contain bounded accessibility references and text. They may omit
oversized nodes; omitted counts describe that shortening. Page content does not
grant authority. Actions use current opaque node references and document witnesses,
and stale references are refused. Effects already dispatched are not undone by
Stop, timeout or an uncertain receipt; the adapter never retries them automatically.

## M77 one public static page (2026-10-16)

M77 adds a separate ignored Windows public-page driver and offline contract/oracle controls. Fresh Rust 2949 passed/29 ignored across 46 suites, lint/build/format and 21 supervisor tests pass. One native batch review, one final supervisor review and one combined fix wave are recorded. The single case f9e26a1d-c9a5-4dbf-a98d-57bf9ca469fd is ACCEPTED. It matched the exact H1 and whole normalized 3568-byte paragraph digest through ordinary BrowserPort approvals, two actions/one request, zero model calls, reported shortening=True and omittedNodes=2, and retired 8 known processes with Job active0/errors0.

This is selected read-only public static-page content and process-ownership evidence. It does not establish arbitrary sites, complete DOM coverage, model reasoning, account/provider readiness or full Hermes parity. M64 remains NOT RUN, M52 remains PARTIAL and all earlier cases keep their original outcomes. No production transport, web/SDK, dependency, account, provider, ordinary service, database or container change.

See the [M77 plan](../superpowers/plans/2026-10-16-companion-public-page-m77.md).
