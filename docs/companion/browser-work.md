# Browser work

M46 adds a scoped browser adapter for ordinary owner chat runs. Its controlled
Windows acceptance passes; see the [current handover](../superpowers/handover/2026-09-28-companion-console.md).
The initial destination boundary is an operator-configured numeric loopback origin
with exact GET/POST routes. M63 adds the pinned public HTTPS static-page mode
below; actual public-site/account/non-Windows acceptance remains open.

The Rust daemon owns task authority and Windows process lifetime. A small worker
uses Playwright-core 1.59.1 and a fresh, sandboxed Chromium context. It receives no
personal browser profile, existing cookies, CDP connection or model-selected code,
selectors, executable paths or launch options. Application request interception
does not provide OS network isolation or bound the browser engine's internal memory.

## One pinned public HTTPS site

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

## Operator installation

Install the workspace dependencies using its pinned lockfile. Supply an absolute
Node 22.12.0 executable, the tracked
`packages/core-rust/browser-worker/worker.mjs`, and an absolute supported Chromium
browser executable. Accepted local compatibility is the already installed
Playwright-paired Chromium headless shell, revision1217, whose package metadata
reports147.0.7727.15. Its executable SHA-256 is
`f7c1ef91a3e287b64509a9733fc8fd43678cf4e2765d67f4d0eceb0e39ecd026`.
Node22.12.0 and Playwright-core1.59.1 are the tested combination. Branded Edge155
and Chrome155 attempts remain unaccepted: native downloads and browser background
traffic exposed incompatibilities with the strict task boundary.
No browser download or installer runs through the agent.

Create a version-1 JSON configuration outside the agent's workspace authority.
Set `ANIMAOS_RS_BROWSER_CONFIG_FILE` to its absolute file path before starting the
real daemon. Deterministic test/embedding constructors do not load this variable.
Omission leaves browser work unavailable. Invalid configured installations fail
validation rather than selecting another program from PATH.

Calculate lowercase SHA-256 for each program/script:

```powershell
(Get-FileHash -LiteralPath 'C:\path\to\node.exe' -Algorithm SHA256).Hash.ToLowerInvariant()
(Get-FileHash -LiteralPath 'C:\path\to\worker.mjs' -Algorithm SHA256).Hash.ToLowerInvariant()
(Get-FileHash -LiteralPath 'C:\path\to\chrome-headless-shell.exe' -Algorithm SHA256).Hash.ToLowerInvariant()
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
