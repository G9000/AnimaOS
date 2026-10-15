# M51 bounded catalog body budget implementation plan

Status: controller-bound plan active on `g9000/companion-catalog-budget-m51`,
cut from verified M50/PR56 main `16a89eb4a857ec4c5898f7ff04179de8a153c086`.
M50 source `64b19994de530129691ac8cca4944ee4122cc33e` shares tested tree
`f3d4e4bab81d2e25b9186ebecc3ef534d5ebb210`; it is committed/pushed/merged.
Exact release/resume:
`.superpowers/sdd/2026-10-15-companion-catalog-budget-m51/progress.md`.
The useful JSONL fixture remains unexecuted/HELD/renumberableM52. No generation,
model choice, useful task or actual metadata request is released by this plan.

**Goal:** Accept a bounded larger catalog envelope without changing its authority,
decoded model rules, public contract or single explicit request behavior.
**Architecture:** Adjust the existing core body cap and boundary tests; reuse M50's
typed diagnostics, native credential/cache/deadline fences and held-driver pattern.
**Stack:** Existing Rust/reqwest/serde/Tokio/Nx; no dependency or parsing framework.

## 1. Evidence and justified scope

- Controller reports M50 gates passed: Rust 2,719 / 21 intentional ignores /
  40 suites; all ten default M50 tests GREEN; formatting/build exit 0 and preserved
  six-source/25-compatibility hashes. M50 publication/merge is root-owned work.
- One separately released actual diagnostic failed in 0.84 seconds with the exact
  safe outcome: body / declared_limit / response_received / HTTP 200 /
  unavailable / count 0 / fixed unavailable error. No retry or generation occurred.
- This new response's declared size tripped the 512-KiB guard. The adapter did not
  consume its body or obtain IDs; full size, actual JSON validity and row count
  remain unknown. This does not recover M49's historical unavailable cause.
- A larger cap addresses this observed local rejection only. It cannot prove
  entitlement, supported model IDs, generation/tool success or a useful task.
- Do not change query, version, headers, route, auth or parser to chase a pass.

## 2. Public source comparison and budget proposal

- Existing HERMES-NOTICE pins M49 catalog reference
  `1fd18277267152893a8a24792b79213e7ca3f899`; preserve that notice/full MIT text.
- Public source inspected: `hermes_cli/codex_models.py::_fetch_models_from_api`,
  `agent/model_metadata.py::fetch_codex_catalog_entries`, and public
  `tests/hermes_cli/test_codex_models.py` / `tests/agent/test_model_metadata.py`.
- Those source fixtures demonstrate a models array with slug, visibility,
  priority, supported_in_api and context metadata. They use small response stubs;
  no representative full catalog envelope or measured production size was found
  in the bounded public-source inspection. No private cache/config/account was read.
- Hermes calls response.json and includes fallback behavior; this does not supply
  an Anima byte budget. Do not copy unbounded body reads, version fallback, static
  IDs, token fingerprints or account/header guesses. Root rechecks pinned source
  provenance before any actual adaptation; this draft ran no Git/network command.
- Propose **8 MiB = 8,388,608 application-collected body bytes**, sixteen times the
  old cap, as a finite compatibility budget for one explicit metadata fetch.
- This is a proposed bound, not a measured full-response size or upstream maximum.
  Root must bind it before implementation. An 8-MiB rejection remains a failed
  result; no automatic second increase, alternate request or pass hunt is planned.
- The byte cap is not an RSS cap: serde/JSON allocations add overhead. Preserve
  single-flight and the deadline, and keep parsed models capped at 512 input rows.

## 3. Small source ownership and preserved interfaces

- One source author owns only
  `packages/core-rust/crates/anima-model-adapters/src/chatgpt/models.rs` and
  `packages/core-rust/crates/anima-model-adapters/src/chatgpt/models_tests.rs`.
- If later released, add only a distinct ignored M51 driver/opt-in in existing
  `hosts/rust-daemon/src/chatgpt_auth/models_tests.rs`; reuse its safe report path.
  Native production code, routes, SDK/web and dependency/lock manifests stay frozen.
- `new`/`fetch` String-error wrappers and detailed typed APIs remain exact.
  Keep declared_limit versus streamed_limit, body_read/decode/timeout distinction,
  numeric status only from a response and honest dispatch certainty unchanged.
- Fixed endpoint/query/headers, no redirects/retry/fallback, five-second connect
  and ten-second total native deadline including auth/lock/publication remain.
- Keep zero-upstream GET, explicit exact-{} POST, owner checks/no-store, six states,
  seven snapshot fields, closed error strings and busy/body/owner/vault envelopes.
- Preserve 512 input rows before filtering, 1..128-byte exact ASCII ID grammar,
  visibility/priority validation, supported_in_api-ignore, sort/deduplication rules.
- Preserve epoch/operation CAS, lifecycle/manifest/account scope, drop/Stop ownership,
  no lock across HTTP, ephemeral cache, no background fetch and no model/grant edits.
- No new configuration override, diagnostics field, logging, captured raw response,
  streaming JSON parser, truncation-to-success or partial catalog acceptance.

## 4. Meaningful RED and exact local boundary matrix

- [ ] Root verifies M50 main, binds 8 MiB/risks/interfaces and freezes RED inputs.
- [ ] Add one valid exactly 600-KiB HTTP 200 body with one literal model
      `synthetic-budget-model` plus an ignored ASCII padding field. Independently
      assert the expected exact singleton ID. Observe actual RED under 512 KiB:
      declared_limit for known length, or streamed_limit for chunked transport.
- [ ] Change only the body budget after root observes the intended semantic RED.
- [ ] Known-length transport: valid bodies at 8 MiB minus one, exactly 8 MiB and
      8 MiB plus one; first two decode the singleton, above rejects declared_limit
      before explicit body consumption, preserving HTTP 200/response_received.
- [ ] Chunked/no-length transport: the same below/at/above cases across multiple
      chunks; above rejects streamed_limit before appending beyond the cap.
- [ ] Pure decoder: below/at success and above fixed invalid-catalog failure;
      transport overflow remains fixed unavailable, preserving existing semantics.
- [ ] Construct exact byte lengths from fixed JSON prefix/suffix and ASCII padding;
      assert literal lengths and output IDs, never parser-derived expected answers.
- [ ] Rebase existing old-cap overflow fixtures to the new literal bound so they
      still exercise overflow, not a newly admitted malformed body. Keep a named
      old-512-KiB-plus valid regression as evidence of the intended behavior change.
- [ ] Retain non-2xx rejection before body reads, redirect refusal, malformed JSON/
      UTF-8/consumed-field rejection, 513-row rejection even within 8 MiB, hidden-row
      counting, exact IDs/order and supported_in_api:false inclusion unchanged.
- [ ] Retain request counts = 1, timeout/body-read/drop behavior, safe Debug/Display,
      no sensitive-marker exposure and native scope/deadline/cache regressions.
- [ ] Peers join/retire before assertions; no uncontrolled system DNS, server,
      private credential, model or real metadata request in default tests.
- [ ] One combined core/native source review and final SDK/web compatibility scope
      check; one combined fix wave, formatting/freeze and fresh required Rust gates.

## 5. Future root commands and evidence

These commands are proposed future steps, not preparation results:

```powershell
bun --no-env-file x nx show project core-rust --json
bun --no-env-file x nx show project rust-daemon --json
bun --no-env-file x nx run rust-daemon:test --skipNxCache
bun --no-env-file x nx run rust-daemon:lint --skipNxCache
bun --no-env-file x nx run rust-daemon:build --skipNxCache
```

Root confirms target dependencies/forwarding before omitting duplicate core runs
or using proposed focused prefix `m51_catalog_budget_`. Use documented Windows
validation target-dir recovery if an owned daemon locks the executable. Retain
incremental-off/jobs1/two full-suite thread settings from current lean workflow.
SDK/web/routes remain hash-frozen: their prior gates are not fresh M51 evidence;
repeat their relevant gates only if a concrete scope change requires it.
Keep RED/GREEN exits, exact byte/call counts, detailed error tuples, source/boundary
hashes, peer cleanup and current-tree gate outcomes. No broad benchmark campaign.

## 6. One later metadata refresh: separately HELD

- After gates only, root may release one distinct M51 ignored selector and exact
  opt-in checked before auth/client/vault/I/O. M49/M50 releases stay consumed.
- Bind verified binary/source, the same fixed request destination/query, normal
  application-owned credential resolution, ten-second async / fifteen-second
  outer driver bounds, zero-or-one catalog GET and one bounded safe report.
- Emit only existing state/count/stage/dispatch/reason/status/static-error fields;
  no raw body/headers/errors, IDs, account data, paths or credential fingerprints.
- Any outcome consumes the release, including pre-dispatch failure or new size
  rejection. No second refresh to refine unknowns, header/version change or model.
- A ready/empty response proves only current metadata receipt/parsing. It does not
  admit generation; useful JSONL/M52 requires later explicit selection/release.

## 7. Five risk proposals awaiting root binding

1. **Finite budget:** bind 8 MiB, retain 512 rows/deadlines/single-flight and disclose
   extra JSON allocation. Cost if wrong: bounded failure, no unlimited body or loop.
2. **Evidence meaning:** fix this observed declared-size rejection only; full size,
   contents and historical M49 cause remain unknown. Cost if wrong: no false cause,
   model-access or generation claim from a 200/header/metadata result.
3. **Exact semantics:** known/chunked/pure boundary tests and old-cap valid RED,
   unchanged parser/error classifications. Cost if wrong: reject rather than truncate
   or partially publish an invalid catalog; no speculative parser framework.
4. **Authority preservation:** same query/headers/wrappers/public contracts and
   native scope/CAS/deadline fences. Cost if wrong: hold acceptance, never rotate
   account/model or widen requests/grants to make the test pass.
5. **One release:** one author, combined review/scopecheck/fix wave and gates, then
   at most one separately released metadata refresh. Cost if wrong: retained failed
   evidence; no retry, generation, M47 replay or premature useful-task/voice breadth.

This proposal is HELD. No implementation, execution or real request is authorized.

## Controller rulings on the risks (binding)

1. **Finite envelope.** Ruling: bind **8,388,608 application-collected body bytes**
   for this single explicit catalog fetch — M50 observed a successful HTTP response
   exceeding the chosen512KiB limit — cost if wrong: bounded unavailable, with no
   automatic second increase or unbounded read. This is an engineering budget,
   not measured response size or upstream maximum. Preserve declared/streamed
   enforcement before append,512 input rows,10-second async deadline and host
   single-flight; JSON allocation overhead means this is not a hard RSS cap.
2. **Evidence meaning.** Ruling: change the evidenced local budget only — the
   response body/size/validity and M49 historical cause were not captured — cost
   if wrong: a visibly failed catalog check, never guessed model access or coding
   quality. Retain the original M50 failed receipt; no repair/replay/relabeling.
   A ready or empty catalog proves bounded metadata parsing only, not generation.
3. **Exact boundary behavior.** Ruling: observe one600KiB valid known-length
   catalog RED before changing production, then independently literal below/at/
   above8MiB known/chunked/pure cases — malformed oversized data alone cannot
   demonstrate valid compatibility — cost if wrong: the gate rejects the cap or
   classification error. Preserve one exact synthetic ID, exact byte lengths,
   requestcount1 and peer cleanup. Rebase old overflow data to8MiB+1 so its original
   overflow assertion remains meaningful; never weaken decoder/row/field checks,
   truncate a result or publish a partial catalog. Keep the M50 Connect fixture.
4. **Authority and source.** Ruling: one author owns only the core catalog source/
   tests and one new guarded native test driver — existing typed errors/public
   snapshots/inference and principal/deadline/cache fences already passed review —
   cost if wrong: hold acceptance, never rotate account/model, retry or widen
   grants/query/headers. Native production, routes, SDK/web/manifests/locks remain
   frozen. Preserve MIT provenance; public Hermes stubs do not establish a measured
   envelope, so no fictitious upstream size claim or copied unbounded/fallback path.
5. **Review and actual release.** Ruling: semantic RED, one combined core/native
   review and final SDK/web scope check, one fix wave and required fresh Rust test/
   formatting/build gates — a constant edit still changes accepted external input —
   cost if wrong: retained failure and held actual operation. Root alone commands/
   fmt/Git/services/models with CARGO_INCREMENTAL=0/jobs1/two full-suite workers.
   Only afterward may root separately release one M51 metadata-only driver with
   distinct exact flag/selector, normal application vault path, same fixed GET,
   10-second async/15-second outer bound and bounded existing seven-field report.
   Any outcome consumes that release. No second refresh, alternate query/header/
   model/provider, generation, M47 replay or manual useful-task rescue. Existing
   M47/M48/M49/M50 releases stay exhausted; useful JSONL/voice remain held.

### Initial release and ownership

After root publishes this plan/status/handover, the author first adds only the
600KiB valid-catalog semantic RED to the existing core models_tests.rs, using
joined synthetic HTTP and literal singleton expected output. Production cap/source
and native tests remain unchanged until root observes RED. Then root may release
the minimal budget/test matrix plus a distinct ignored native M51 driver/opt-in.
No other source writer, new project/dependency or actual operation is released.
