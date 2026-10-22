# M83: Web search with source links in companion chat

> **For agentic workers:** Follow the controller-bound plan with `superpowers:executing-plans`. The controller has bound the choices and risk rulings below. Implementation begins only after this plan is published; public validation follows the recorded post-gate boundary.

**Goal:** An explicitly granted search tool retrieves bounded public results, supplies usable source URLs to the model, and shows clickable returned sources in the real console during the run and after reload.

**Architecture:** Add a narrow native `web_search` adapter for anonymous Exa MCP, reusing the existing bounded MCP transport. Normalize its response once into a versioned result and typed source projection. Reuse the existing `web_fetch` and Markdown answer renderer; add source links to the existing tool cards rather than a second research UI.

**Stack:** Existing Rust, reqwest/rmcp transport, SDK event/session types, React transcript and Markdown components. No new runtime dependency proposed.

## Controller decision boundary

### Binding amendment from the actual hosted catalogue

The first actual SDK probe on 2026-10-16 connected and joined cleanup but failed
at discovery before any search call. Its source, binary and evidence remain
archived as a failed outcome. A separate metadata-only SDK capture shows
`web_search_exa` with a flat Draft 7 object schema and required string `objective`
alongside `query`; `numResults` remains optional. The protocol version matches.

For native search only, add an explicit discovery seam selecting that expected
tool before schema preparation while retaining catalogue wire/page/name/count
bounds. Accept only the observed flat scalar schema fragment with shared
semantics across Draft 7 and 2020-12; preserve its constraints and reject unknown
structural features, references and dialects. Generic discovery and core schema
policy stay unchanged. When that known string objective is required, derive it
from the same trimmed query; no new model argument or session context is sent.
This amendment supersedes the earlier two-field-only remote translation rule.
Keep old-server query/count fixtures, actual-shaped JSON/SSE fixtures, and
no-call regressions for incompatible descriptors. Rerun affected native gates
before a fresh lookup attempt. The original failed probe is never replayed or
regraded; its logical search-call count was zero.

Root has verified official Exa documentation advertising the anonymous, rate-limited `https://mcp.exa.ai/mcp` endpoint and read-only `web_search_exa` / `web_fetch_exa` tools. Sources: <https://github.com/exa-labs/exa-mcp-server> and <https://exa.ai/mcp>.

Controller has bound the fixed native `web_search` adapter described below. Preserve explicit keyed `exa_search` unchanged; there is no silent paid fallback. Do not use an available assistant search connector as product implementation.

The native adapter is smaller for this user workflow than an owner MCP connection/preset: it avoids requiring a second setup and arbitrary remote-tool grants, while reusing the already integrated transport. Do not duplicate JSON-RPC, SSE parsing, MCP handshake or credential management.

If current Exa handshake/result behavior is incompatible with the existing transport, record the actual failure. Do not loosen transport security, add a provider ladder or bypass initialization implicitly. Controller decides the smallest evidence-backed adjustment.

## Current source evidence

- `tools.rs:661-695` registers `web_fetch` and `exa_search`, but no native keyless `web_search`.
- `tools/web.rs:234-244` requires `EXA_API_KEY`, `EXA_KEY` or `EXA_TOKEN` for existing Exa search.
- `tools/web.rs:355-400` posts to the keyed API; existing result parsing/formatting is at lines 403-480.
- `approvals/policy.rs:66` classifies the current fetch/search pair as Network tools.
- `routes/capabilities.rs:102-105` describes fetch and keyed Exa requirements.
- `mcp/mod.rs:6` exposes transport within the crate; `transport.rs:129-181` exposes validated HTTP launch, factory, discover, call_once and joined close interfaces.
- `TransportControl` supplies shared absolute deadline, cancellation and wire budget. Production HTTP transport disables retry/session reinitialization (`transport.rs:997-1002`).
- MCP output is already bounded through `bound_sdk_result` and `anima_core::external_tools`, including a 16 KiB external-result bound. Preserve these controls.
- `runtime.rs:1638-1655` preserves successful native Content metadata internally. The session HTTP projection deliberately filters it (`routes/contracts/sessions.rs:21-38,438-443`).
- `RunFrame::ToolFinished` carries complete tool text, not Content metadata. `live/observer.rs:233-260` clips that text before public events/snapshots.
- `packages/sdk/src/events.ts` defines `LiveToolCard` and `tool.finished`; neither has sources today.
- `apps/web/src/lib/transcript.ts` creates live/history tool cards. `ToolStepCard.tsx:48-61` renders result text in a `pre` and warns about 2 KiB clipping.
- `ChatScreen.tsx:112-114` already uses `MarkdownMessage`; its safe Markdown link renderer can display answer citations without a new rich-text engine.
- `apps/web/src/lib/agent-access.ts` standard profiles contain neither existing web tool. `SettingsPanel.tsx` has dedicated optional-grant controls but no arbitrary grant checkbox; native registration alone would leave search inaccessible through ordinary owner settings.

The implementation gap is therefore both search availability and provenance surviving projection. A registered tool, mocked final answer or link parsed from clipped preview is insufficient completion evidence.

## Product and authority rules

1. `web_search` is a discoverable native Network tool. Add independent owner Settings controls for `Allow web search` (`web_search`) and `Allow web page reads` (`web_fetch`); do not auto-grant either tool or widen access-profile arrays.
2. Existing live checks and approval policy run before any transport creation. Denial, missing grant or revoked authority causes zero search requests.
3. Backend endpoint and remote tool name are host literals, never model parameters. Only `web_search_exa` can be called by this wrapper.
4. Discovery supplies schema compatibility, not authority. It must not expose, grant or call other discovered tools, including authenticated `agent_run`.
5. Never attach API keys, owner MCP credentials, cookies, accounts, profile identifiers or session history. Send only the requested query and bounded search options plus the existing protocol handshake.
6. Search results and excerpts are untrusted external data. They cannot change instructions, grants, endpoints or tools.
7. Search does not fetch every result automatically. The model may explicitly use existing `web_fetch` when the excerpt is insufficient, subject to its own grant/Network policy.
8. Preserve keyed `exa_search` and its honest missing-configuration error. No new key settings page, secret store, account setup or provider routing framework.
9. Anonymous service can fail or rate-limit. Return a bounded truthful error and no invented sources; no retries, provider switching or paid fallback.
10. Existing Markdown answer links remain model-authored. Returned Sources prove URL provenance from the tool, not factual correctness or that every answer claim was independently verified.

## Owner setup seam

Extend `SettingsPanel.tsx` and its tests using the existing optional document/browser grant pattern: initialize/reset from the selected agent, include each grant in dirty state, preserve custom tool sets for unrelated saves, compose exact tools on explicit Save, and respect saving/resetting/agent-switch lifecycle. Search and page-read checkboxes are independent; neither silently enables the other or keyed `exa_search`.

Existing profile selection clears optional document/browser grants (`SettingsPanel.tsx:752-757`); apply that same explicit profile-change policy to the new toggles, rather than accidentally copying an earlier agent's grant or silently widening a profile. Unrelated setting changes retain stored explicit grants. Keep access-profile tool arrays unchanged; profile classification should ignore the new optional grants in the same place it ignores other dedicated toggles.

Help text: search sends the query to Exa's anonymous public service and may be rate-limited; page reads send the selected URL to the public site. Ordinary Network approval rules still apply. No key field, account connection, test request or automatic network probe on mount/save. Configure the isolated acceptance agent through these existing owner settings/grant paths; never edit the user's `anima.yaml`.

## Native request

Register `web_search(query, num_results?)` with `query` required and default result count 3. Bound query to at most 512 UTF-8 bytes after trimming; result count integer 1 through 5. Reject invalid/oversized input before connecting.

Translate only to `web_search_exa` arguments `query` and `numResults`. Validate against the selected discovered descriptor using the existing external-tools schema helpers. Select exactly one descriptor with the expected name; missing/duplicate/incompatible descriptor is an unavailable/unsupported error, not permission to choose another tool.

Introduce a small host module (`tools/web_search.rs`) owning transport execution and normalization, with a focused sibling test module if useful. Keep `tools/web.rs` keyed search/fetch behavior unchanged unless a specific tested citation seam requires a minimal addition.

Use `DefaultMcpTransportFactory` in production and a constructor-injected test factory behind test-only host setup. Do not expose an endpoint override in model arguments, environment, owner API or production settings.

Lifecycle:

- Acquire a bounded no-queue admission lease before transport creation; proposed process-wide capacity four searches.
- One absolute 30-second deadline spans connect, initialize, discover, call and normalization. Do not restart it per phase. Retain the existing shared 256 KiB wire budget.
- Bridge the existing run cancellation signal into `TransportControl`; cancellation before dispatch makes zero remote calls.
- One session and at most one logical remote search call; HTTP initialization/listing are separately recorded protocol traffic, not counted as searches.
- Always initiate close after success/error/cancellation and when the caller drops. A caller timeout must not abandon ownership of a live session.
- Retain the admission lease until physical close joins. Use the existing transport close/owned-task patterns; no detached worker that releases capacity early.
- Do not wait indefinitely to return a result to the caller. If cleanup continues after deadline, keep its bounded ownership until joined and refuse new work when capacity is held.
- Explicitly test slow close and repeated cancellation/drop reuse. Do not add a general MCP manager/cache/connection service for one fixed backend.

## Normalized result and sources

Define a small versioned JSON envelope as the native tool's `Content.text`. It is the complete model-visible result, and the daemon can parse it before public preview clipping.

Suggested fields: `version: 1`, `provider: "exa"`, `mode: "anonymous"`, `untrusted: true`, `results: [{title,url,excerpt}]`, `truncated: boolean`. A successful empty result is explicit. Keep generated field names host-owned; never merge provider objects into Content metadata.

Limits proposed for controller binding:

- Maximum five results; preserve provider ranking and deduplicate exact normalized URLs.
- Title at most 256 UTF-8 bytes, excerpt at most 1,200 bytes per result.
- URL at most 2,048 bytes and copied whole, never shortened into a different valid URL.
- Complete serialized envelope at most 16 KiB, including JSON escaping, fixed fields and omission flags.
- Typed source list at most 8 KiB serialized; omit whole tail records if needed and report truncation honestly.
- Excerpts/title truncation must be Unicode-safe and explicit; never truncate serialized JSON.

Exa's pinned Hermes adapter parses labelled text blocks (`Title:`, `URL:`, `Highlights:`, separated by `---`). Prefer documented structured results if the actual endpoint returns a supported shape; otherwise implement one bounded labelled parser with fixtures from the pinned public source/official contract.

Rules for text parsing:

- Read URLs only from the recognized record URL field, never from excerpt text or arbitrary Markdown links.
- Reject duplicate/ambiguous URL fields within a record; do not let a page's prose redefine another result's source.
- Preserve inline and multiline highlights; title/author/publication labels are not proof of truth.
- Validate each URL as absolute HTTP(S), with a host, no embedded credentials or control characters, and no local/private literal target. Use existing URL policy helpers where suitable; do not claim DNS verification merely from URL syntax.
- Do not decode/rewrite URLs into different resources or add guessed URLs for missing fields.
- Discard incomplete trailing records after upstream truncation; a cut-off URL that still parses must not become a clickable source.
- Distinguish a recognized empty response from an unsupported/malformed format. If there is no valid parseable result and no recognized empty marker, return an honest error.
- Non-text MCP blocks are not images/attachments to display. Preserve omission/truncation evidence; errors and omitted data cannot create sources.

The descriptor tells the model to treat excerpts as untrusted, fetch when needed, and cite exact returned source URLs with descriptive Markdown links. This instruction supports behavior; it is not acceptance evidence by itself.

## Typed source projection: streaming and reload

Use one small host type and normalizer, conceptually `WebSource { title, url }`, plus the bounded versioned source list. No full taskResult, raw MCP response, arbitrary metadata or query history enters the source field.

### Live path

- In `live/observer.rs`, for successful `name == "web_search"` only, validate the full normalized envelope before calling `preview`.
- Add an optional/empty-by-default `sources` field to `LiveToolView`, `LiveEventBody::ToolFinished` and the snapshot serialization.
- Carry that typed field through `live/registry.rs` update and reconnect snapshots using existing run/step/call identity.
- Do not parse a 2 KiB preview or infer sources from model answer Markdown.
- Reset sources on new running cards and keep error/denied/unknown outcomes empty.

### History path

- Native dispatch supplies a reserved `webSources` metadata value derived from the same normalized successful search envelope.
- Add one tool-dispatch projection choke point covering native and external/MCP return paths: other tool results cannot retain caller/provider-authored reserved `webSources`; unsuccessful search results cannot retain it either. Check early-return paths rather than assuming all dispatch goes through one native handler tail.
- The session response constructs/validates this narrow projection only for a successful Tool-role result. Do not simply add unrestricted `webSources` to the metadata allowlist.
- User/assistant/system messages with a lookalike key expose no Sources; other tool metadata cannot mint this provenance.
- Preserve the original run/step/toolCall IDs. Session reload uses the saved result; it never repeats a search to reconstruct links.
- Existing full result persistence is reused; no new database table or durable source service.

This avoids changing core `RunFrame`: it already carries the full normalized text needed by the host observer. The core remains source-agnostic.

### SDK and console

- Export a small `WebSource` interface and optional typed source field on live tool cards/events and the appropriate session response type.
- Use runtime shape/bounds/URL checks at the web projection boundary, not a TypeScript cast alone.
- Carry sources through `session-events.ts` initial snapshots and `tool.finished` reduction, and through `transcript.ts` history/live cards.
- Use existing run + step + toolCall identity. Reused call IDs across steps/runs cannot swap source lists.
- Extend `ToolStepCard` with a short Sources list outside the clipped `pre`, accessible when the result is expanded or visibly alongside it per existing layout.
- Render labels as React text and safe HTTP(S) anchors with the existing new-tab/referrer policy. Do not parse result titles as Markdown/HTML, auto-open links, fetch previews or render remote images.
- Keep `MarkdownMessage` unchanged unless a source-backed regression requires it. Answer citations remain ordinary clickable Markdown links.
- No Sources for fabricated answer URLs, failed search, arbitrary user metadata or unsupported result format. Do not badge answer citations as verified.

## Ownership split

Two source-only lanes are useful after the controller binds the shared source contract; neither worker runs commands, Git, services or validation.

### Lane A: native search and projection

Primary files:

- `hosts/rust-daemon/src/tools.rs` and existing `tools/tests.rs`: registration, context injection if needed, reserved source metadata boundary.
- New `hosts/rust-daemon/src/tools/web_search.rs` and focused tests: fixed adapter, normalization, bounded source type, lifecycle.
- `hosts/rust-daemon/src/approvals/policy.rs`: native Network classification and existing policy tests.
- `hosts/rust-daemon/src/routes/capabilities.rs` and existing capability tests: honest anonymous/rate-limited search requirements, explicit keyed alternative.
- `hosts/rust-daemon/src/live/{observer.rs,registry.rs,events.rs}` and existing live tests: typed event/snapshot sources.
- `hosts/rust-daemon/src/routes/contracts/sessions.rs` and `routes/tests/sessions.rs`: safe persisted source projection.
- `hosts/rust-daemon/src/agent_runs.rs` only for test module/context seam; a new focused `agent_runs/web_search_tests.rs` for real harness integration.

`mcp/transport.rs` should need no production protocol changes. A minimal cfg(test) constructor/helper may expose its existing actual HTTP test client seam to the integration fixture. Any production transport change needs recorded concrete incompatibility and review scope.

### Lane B: SDK and console

- `apps/web/src/components/SettingsPanel.tsx` and tests: independent search/page-read grants, save/reset/preserve/busy semantics and honest service help text. Keep `agent-access.ts` profile arrays unchanged; a small optional-grant classification helper change is allowed only if the existing settings-local filtering cannot cover it cleanly.
- `packages/sdk/src/events.ts`, session types and `index.ts` only as needed to export the additive shared shape; associated SDK contract tests.
- `apps/web/src/lib/session-events.ts` and tests: event/snapshot retention.
- `apps/web/src/lib/transcript.ts` and tests: live/history source mapping.
- `apps/web/src/components/sessions/ToolStepCard.tsx` and its focused tests; `ChatScreen.test.tsx` for the complete visible transcript.

Controller owns the published plan and narrow user/setup/capability documentation. Do not touch unrelated Astro settings, `nx.json`, `anima.yaml` or `uploads/`, including reads/staging. No dependency/lock changes are expected.

## Meaningful tests-first matrix

### Native transport and parsing

1. Missing native registration/current key-required path demonstrates the desired no-key workflow is absent; behavioral RED should exercise the intended tool route, not a compile error from a missing symbol.
2. Actual MCP JSON and request-scoped SSE fixtures initialize, discover and execute exactly one `web_search_exa` call. Assert exact endpoint/tool/arguments and no auth headers, extra tools, retries or fallback requests.
3. Discovery has unwanted tools, missing/duplicate expected descriptor or incompatible schema: fail closed without a tools/call request.
4. Parse realistic labelled results, Unicode, inline/multiline excerpts, repeated URLs and a valid empty result. Assert ranked normalized output and exact safe URLs.
5. Malformed/ambiguous shape, malicious title/URL, injected `URL:` inside excerpts, oversized query/results/body, JSON escaping expansion and incomplete truncated record: bound or reject without inventing sources.
6. HTTP/MCP error, quota refusal, timeout and cancellation at connect/discover/call/close: honest bounded failure, zero source list, owned joined cleanup and retained admission. Caller-drop test proves no orphaned request or early capacity reuse.
7. Missing/denied/revoked grant: zero transport calls. Ask-approval mode pauses with zero requests until ordinary owner approval, then dispatches exactly once; rejection remains zero. Existing keyed `exa_search` still requires its explicit key; new search never reads/falls back to it.

### Real harness and persisted output

8. Use a disposable daemon state/workspace/session, existing ordinary native grants and approval gate, real MCP HTTP fixture and real agent run coordinator. Inject a deterministic model adapter only; do not mock the native tool result.
9. Adapter requests search, inspects the actual returned URL/excerpt, then requests a bounded page fetch when the fixture requires more evidence, and finally produces Markdown citations using those observed URLs. It must fail if the expected evidence is missing or altered.
10. Assert request counts, complete tool arguments/results, successful source projection, model consumption and stored transcript. Read the actual owner session HTTP response and compare source URLs with those returned by the controlled search fixture.
11. Exercise denied/error and altered-source variants: no fake Sources or successful answer fabricated by a fixture that ignores tool results. Keep final model prose distinct from tool provenance assertions.
12. Assert stream event and reconnect snapshot sources match the history projection even when the ordinary result preview exceeds 2 KiB. Repeated call IDs in different steps/runs remain isolated.
13. Inject lookalike metadata through user/assistant messages and unrelated external tool output; it must not become public Sources. Full taskResult and unrelated secret-like metadata remain hidden.

### SDK and web

14. SDK fixtures preserve the additive typed source fields on event, snapshot and session response; old payloads without the field remain readable as no sources.
15. Session reducer/transcript tests cover live finish, snapshot reconnect, history reload, delayed events, duplicate/reused call IDs and failed/running cards.
16. Real `ChatScreen`/`ToolStepCard` DOM test renders clickable returned links and the model's ordinary Markdown citation. Unsafe URL/HTML title is inert or omitted; no background fetch/image request occurs.
17. Clipped preview does not clip or corrupt source URLs. History without live events still shows the same sources; reload initiates no search.
18. A negative answer-only URL does not gain a Sources entry. Optional source fields cannot turn failed cards into successful evidence.
19. Settings starts with both optional grants off unless stored; enabling search alone saves only `web_search`, enabling reads alone saves only `web_fetch`. Unrelated saves preserve existing custom tools/grants; explicit profile changes follow the existing optional-grant reset policy; changing agents resets state; busy/error/reset paths do not fabricate saved grants. Mounting/toggling/saving performs no search.

## End-to-end acceptance boundary

First prove the controlled real-HTTP → native tool → real harness → persisted session/API → SDK/transcript → rendered console chain. The model fixture must consume the actual tool result, and the UI must consume the actual public projection or an exact captured contract fixture, not a hand-authored final answer with arbitrary links.

Reuse existing test infrastructure and isolated fixture ownership. If a browser-level final console check is needed, the controller uses a disposable isolated test setup, not normal profiles or services. Do not build another general guard/preflight milestone.

After required gates, the controller performs one bounded anonymous public query through the actual native harness using a scripted adapter that reads its real tool results (zero paid generation). Carry the actual returned result through history/public session projection and the real console renderer; assert safe clickable links using those exact returned URLs. Reuse ordinary helpers/router and a minimal opt-in public driver, not a new acceptance framework. Record query, endpoint, returned URLs, result status and cleanup; optionally one bounded public page read establishes a returned source can be opened. No real network search/model/account use is authorized during planning; the post-gate public read is explicitly authorized by the user's latest request.

A public provider check establishes current endpoint compatibility, not model citation quality. A deterministic model establishes dataflow, not arbitrary model behavior. Report these separately. If anonymous Exa is unavailable/rate-limited, retain the failure honestly; do not claim live search acceptance or loop through vendors.

## Execution and review sequence

- [ ] Controller binds provider, source projection, limits, ownership and acceptance rulings; publishes the plan from clean selected baseline while preserving unrelated local work.
- [ ] Assign the two source-only lanes with a shared frozen source contract; do not overlap native/SDK ownership.
- [ ] Workers write meaningful failing tests first and stop. Controller runs bounded RED and preserves exact source/output/exit status.
- [ ] Workers implement the narrow adapter/projection/UI and report source changes; no worker validation/Git/activation.
- [ ] Controller formats, records initial source freeze and focused GREEN.
- [ ] One daemon batch review and one final combined web/SDK review, then one combined fix wave.
- [ ] Controller runs fresh required gates, the bounded public search check and narrow docs verification; records actual outcome/limitations.
- [ ] Publish via authorized per-command unsigned commit/PR/merge workflow under the controller. Do not stage private evidence or unrelated paths.

## Root-only validation

Use current Nx project targets, confirmed by controller without reading/changing unrelated `nx.json`. Rust remains serialized with `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=2` and the established isolated target directory.

```text
bun x nx run rust-daemon:test --skipNxCache --excludeTaskDependencies --args="--lib m83 -- --test-threads=2"
bun x nx run rust-daemon:test --skipNxCache
bun x nx run rust-daemon:lint --skipNxCache
bun x nx run rust-daemon:build --skipNxCache
bun x nx run @animaos/sdk:test --skipNxCache
bun x nx run @animaos/sdk:typecheck --skipNxCache
bun x nx run @animaos/sdk:build --skipNxCache
bun x nx run @animaos/web:test --skipNxCache
bun x nx run @animaos/web:typecheck --skipNxCache
bun x nx run @animaos/web:build --skipNxCache
```

SDK/web targets are inferred in this workspace; controller confirms available target names before execution and substitutes the repository's existing corresponding contract/type/build target if needed. Do not invent flags or run duplicate equivalent gates.

Rust lint currently performs `cargo fmt --all --check`. Run scoped Prettier checks on touched TS/TSX/Markdown and scoped docs consistency checks. Add Astro build only if an Astro source is actually changed. Do not claim formatting as a semantic lint audit.

## Reuse comparison and provenance

Public pinned Hermes source inspected through read-only `git show` at `1fd18277267152893a8a24792b79213e7ca3f899`:

- `tools/web_tools.py`: search returns titles/URLs/descriptions; page extraction is separate; explicit provider errors remain honest; result payloads are bounded.
- `plugins/web/exa/provider.py`: keyed Exa and keyless search/extract are separate transport choices; source rows normalize URLs/titles/highlights.
- `plugins/web/keyless_mcp.py`: fixed Exa MCP endpoint, exact remote search/fetch names, labelled result parser, explicit rate-limit failures.

Useful narrow reuse is the search-versus-extract workflow and known response-field parsing. Reuse Anima's existing rmcp transport and Markdown renderer directly. Do not port Hermes' requests-based MCP protocol, fallback ring, plugin registry, SDK auto-install, provider cache or profile/config handling.

No code is copied in this draft. If implementing a substantial parser port, retain upstream license/attribution and record file, exact commit and local changes. The endpoint/schema should also be grounded in controller-verified official Exa docs, not treated as permanently valid because Hermes used it.

## Top five risks for binding rulings

1. **Provider choice and authority:** a cheap default becomes implicit paid use or arbitrary MCP access. Fix endpoint/tool, retain native grant/Network policy, no credential/fallback path, and honest anonymous availability limits.
2. **False citation authority:** model links, excerpt text or raw metadata become verified sources. Normalize actual provider records, reserve host projection, gate tool/status/role, preserve IDs and label provenance without asserting factual verification.
3. **Bound/lifecycle failures:** hostile response, slow transport or caller drop leaks work or releases capacity early. Shared deadline/wire caps, no retry, joined close and retained admission with deterministic phase tests.
4. **Stream/history divergence:** 2 KiB preview parsing loses URLs or reload repeats search. Project typed fields before clipping, persist the same bounded source data and test stream/snapshot/history pairing.
5. **False end-to-end evidence or scope growth:** mocks fabricate success, or the implementation grows into secret setup/provider routing/browser infrastructure. Require actual fixture HTTP plus harness/public projection/DOM evidence, one bounded public search after gates, and keep unchanged fetch/Markdown paths unless a concrete failing test requires more.

## Completion claims

Report which of these actually passed: controlled integration, safe source rendering/reload, anonymous public endpoint check, and page-read check. State remaining real-model or endpoint limitations plainly. Do not replace missing live evidence with tool registration, invented URLs, prompt literals or a screenshot of a fabricated answer.

## Controller rulings on the risks (binding)

1. Ruling: default to the fixed anonymous Exa MCP search through the existing bounded transport and an explicit native Network grant — this removes the key/setup barrier without silently buying API usage or granting arbitrary MCP tools — cost if wrong: anonymous availability can block search, which must return a clear error. Keep keyed exa_search separate and unchanged; no paid fallback, credentials, profile import or provider ladder.
2. Ruling: expose only host-normalized sources from successful native web_search results, labelled Search sources — model links, provider metadata and excerpt prose do not establish verified claims — cost if wrong: fabricated links appear to have retrieval provenance. Reserve webSources at every dispatch exit, gate history by Tool/success and validate the narrow source value; live parses the full canonical native envelope before clipping. Never expose taskResult or unrelated metadata.
3. Ruling: keep one absolute30s operation deadline, four no-queue physical search leases, existing256KiB wire limit and joined cleanup — cancellation/drop cannot authorize another request while the prior transport is still owned — cost if wrong: background traffic or unbounded resource use survives Stop. One remote search call, no retry. Retain cleanup ownership/lease until physical close and report unknown cleanup honestly.
4. Ruling: bind one additive source contract across live events, snapshots, history, SDK and console before delegation — clipped previews and reused call IDs cannot identify source provenance — cost if wrong: reconnect loses or swaps links. Use WebSource{title,url}; optional sources on live cards/events, reserved webSources array on successful Tool session metadata. Arrays are derived together from canonical results and bounded to8KiB; trim whole tail records from both results/projection if needed. Preserve complete URLs, existing identities and old payload compatibility.
5. Ruling: prove the controlled native-HTTP/harness/history/console chain, then one bounded anonymous public query using a scripted adapter that consumes actual results — registration, hand-authored answers and prompts are insufficient evidence — cost if wrong: shipped search may not work or evidence may overstate model quality. No paid model calls, account/profile/container/database/normal-service activation or consumed-case replay. Record actual public failures honestly and fix demonstrated blockers inside this milestone instead of creating another guard milestone.

Ruling: keep explicit independent owner controls for search and page reads, with standard profile tool arrays unchanged — current profiles cannot enable research and owner choice must remain clear — cost if wrong: the feature stays inaccessible or existing authority expands silently. Preserve custom grants on unrelated saves; explicit profile selection follows the existing optional-grant reset behavior. No network request on mount/toggle/save.

Ruling: reuse Hermes's pinned public response-field/workflow behavior and Anima's existing rmcp transport, URL/approval paths and Markdown renderer — porting its protocol/provider framework would increase maintenance without helping this workflow — cost if wrong: any needed compatibility adjustment must be demonstrated, attributed and reviewed. Preserve notices for copied code and use Anima product names.

Ruling: use meaningful initial RED against existing registration/projection/owner-UI interfaces, then add detailed adapter tests with the new internal seam — missing-symbol compiler failures are not feature evidence and repeated native builds add cost — cost if wrong: new adapter behavior could be under-tested. Keep original failing assertions; detailed fixtures must exercise real transport/harness paths before focused GREEN and final gates.

## Recorded outcome

M83 adds anonymous native web_search through the existing MCP transport, independent owner search/page-read grants, and bounded Search sources links across live events, snapshots, saved Tool history, SDK and console. Sources stay outside the shortened preview; reload never re-searches. Fresh Rust 2989 passed/31 ignored across 46 suites; web1648passed/1opt-in skipped; SDK203passed/3skipped. Rust lint/build, SDK/web typecheck/build, touched formatting/docs and the native plus web/SDK reviews pass.

One controlled actual-HTTP harness/history/console chain and one anonymous public search through the real native harness and console renderer pass with a scripted model consuming actual returned evidence. The initial discovery-only failure sent no query and stays archived as failed; the observed hosted schema/objective mismatch was fixed only for native search. Zero paid model calls. This establishes endpoint compatibility and source propagation, not general LLM research quality or independently verified answer claims. Anonymous service availability/rate limits remain external. No account/profile/normal-service/database/container activation or historical-case replay.
