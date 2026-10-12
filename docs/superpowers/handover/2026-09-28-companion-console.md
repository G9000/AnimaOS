# Handover: reviewed owner memory deletion (M0–M31)

## Start here (2026-10-13): M31 complete; publication ledger is authoritative

- M30/PR36 merged at `95b5daf4c347297abce8c2aa30cb68d4c2a6245d`. M31 branch `g9000/companion-memory-m31` closes a concrete owner-memory deletion race. Plan [M31](../plans/2026-10-13-companion-memory-m31.md); exact evidence, gate state and publication/resume are in `.superpowers/sdd/2026-10-13-companion-memory-m31/progress.md`. Native and SDK/web source is reviewed and gated. Finish explicit20-owned-path unsigned commit/push/PR attachment/verified-head standing merge and inactive-main fast-forward if publication is absent; otherwise no selected M31 implementation remains. Do not repeat routine approvals.
- Owner `DELETE /api/memories/{memory_id}` accepts optional strict `{ expectedContent }`. Exactly empty HTTP body keeps legacy ID-only behavior; any nonempty body requires a string and rejects missing/null/unknown fields. Compare complete exact text under the existing memory write lock before mutation/save; missing404/stale409 content-free, no trimming/edit cap or automatic retry. Existing transport byte limit, owner authorization, memory types, cancellation-safe task, store rollback/committed-durability-uncertain behavior, embedding order and citation cleanup remain. This detects content mismatch, not revision/ABA/metadata changes or complete erasure.
- SDK optional second string argument preserves omitted-body callers and serializes present empty/Unicode strings. Console always supplies the click-time record's complete text, pins the displayed record during confirmation and clears it on cancellation/completion. Existing409 reload closes confirmation; review the refreshed text and explicitly reopen before retry. No unguarded fallback, new grant/default/profile, patch/fact/entity mutation change or request framework.
- Two meaningful one-case REDs preceded native/UI production: stale native DELETE200 versus409; console ID-only versus reviewed string after background refresh. One native batch and final SDK/web review found no actionable defects; current web1201tests/typecheck/build and SDK128unit tests/3servicecases skipped/typecheck/build pass. Initial SDK suite mistakenly tried its integration build while native source was editing and failed before daemon startup; failed evidence retained, existing skip setting restored, only the SDK target rerun with service tests skipped. Fresh required Rust2423passed/8ignored/40suites and strictly sequential lint pass, with four new native cases and reused guarded/legacy persistence/citation/404/rollback/OpenAPI fixtures. All13sourceSHA256 remain unchanged through gates; final format/local links/diff verification precedes publication. No code fix wave was needed.
- All fixtures use synthetic seeded data; no actual owner-memory removal, live model task, account/profile/credentials or service was used. Full public pinned Hermes remove/pinned-entry MIT attribution remains. M28 is still the latest ordinary model/memory/file/wrap-up/quiet-restart acceptance. Semantic memory/consolidation, external account delivery, browser/microphone and hosted uptime remain unverified; no complete upstream parity claim. Resume practical [daily use](../../companion/daily-use.md) and [evidence/limits](../../companion/harness-acceptance.md) after publication, choosing a concrete personal-use gap before further work.
- Standing cheap Codex lean plan/five root risks/source workers/native batch+final SDKweb review/one justified fix wave/local gates/unsigned reviewed merge-and-continue persists. Preserve original `nx.json`; explicit stage paths never `nx.json`, `anima.yaml` or private evidence. No worktree/stash/reset/restore/path checkout/deps/features/newtargetdirs/normal profiles/secrets/real connectors/sends/DBservers/containers/Docker55432/public deploy. Root serializes all CARGO_INCREMENTAL=0 operations and waits for actual exit; SDK gates use ANIMA_SKIP_SERVICE_TESTS=1. Owned services stay off. Final context-only update to the authorized separate animaOS chat follows publication; this chat owns kit implementation.

## Historical M30 handover

# Handover: explicit saved-Fact removal verified (M0–M30)

## Start here (2026-10-13): M30 complete; publication ledger is authoritative

- M29/PR35 merged at `c8df3087870b829092116a84e72838b3e4a825fd`, tested source `402c5365a411cf77516da96ee39438b6a48840de`. M30 branch `g9000/companion-memory-m30` adds optional exact-content saved-Fact removal and web opt-in. Plan [M30](../plans/2026-10-13-companion-memory-m30.md). Exact tested source, PR, verified merge and resume are in `.superpowers/sdd/2026-10-13-companion-memory-m30/progress.md`. If publication is absent, finish explicit14-owned-path unsigned commit/push/PR attachment/verified-head standing merge and main fast-forward; otherwise no selected M30 implementation remains. No repeated approval.
- `memory_remove` requires an explicit new tool grant, exact ID/full current content, caller ownership, Fact type and visible text. One write lock covers validation/MemoryMutation/delete/persist; missing/foreign/nonFact/hidden targets refuse generically, stale/partial text and extra/force fields mutate nothing. Bounds256UTF8-byteID/8000characterexpectedtext. Existing precommit rollback and reported committed-durability-uncertain canonical retention remain; related citation cleanup uses established deletion semantics. Canonical absence read lock precedes best-effort embedding write; replacements keep their index. No batch/sweep/fuzzy matching, new store/revision framework or cross-store transaction claim.
- **Allow removing saved facts** in Settings is a separate explicit opt-in. Defaults/profiles/saved agents/helper grants remain unchanged. Toggling preserves every other configured name; unrelated saves omit tools; switching standard profiles clears the option until explicitly rechecked. Failed saves keep the draft without optimistic success, current server grants remain authoritative; busy controls/config reseeding preserve old behavior. Registered names are validated by the daemon, including unsupported old-host errors. No new SDK DTO, query framework or host/port hardcoding.
- Fresh required Rust2419passed/8ignored/40suites and serial lint pass; current web1197tests/typecheck/build and transitive SDK type/build pass. Eight grouped native and five new UI cases pass after meaningful unknown-tool and missing-checkbox REDs. One native batch review found no defects; final SDK/web P2 (API-only selection) fixed in one wave. Seven sourceSHA256 unchanged through gates; no further fix needed. SDK130unit tests remain historical M25 evidence; no redundant unchanged SDK unit run.
- Removal affects an active saved Fact record, not old conversations/reflections/retained temporal facts/receipts/backups or complete erasure. Core MemoryPort revisioned forgetting/ABA/audit semantics are not implemented by this legacy daemon path. Uncertain commits retain shared-store semantics tested by the inherited fixture. All M30 deletion fixtures are synthetic/local JSON and embeddings; no actual owner record, live model task, account/profile/credentials or operator service was used. Full MIT attribution remains for pinned public Hermes remove/pinned-entry intent; no Python/private-profile imports.
- Resume practical use through [daily use](../../companion/daily-use.md) and [evidence/limits](../../companion/harness-acceptance.md). The bounded core loop now includes M29 alternating-cycle advice and an explicitly controlled memory lifecycle; M28 remains the latest ordinary real model/memory/file/wrap-up/quiet-restart acceptance. Semantic memory/consolidation, browser, external account delivery and hosted uptime remain unverified. Do not invent further guard or integration breadth merely for feature count; define a concrete personal-use gap before another milestone. No complete Hermes/OpenClaw parity claim.
- Standing cheap Codex lean plans/five root risk rulings/source workers/batch review/one fix wave/local gates/unsigned reviewed merge-and-continue authority persists for defined work. Preserve unrelated `nx.json`; stage explicit owned paths, never `nx.json`, `anima.yaml` or private evidence. No worktree/stash/reset/restore/path-checkout/deps/features/newtargetdirs/ordinary profiles/secrets/real connectors/sends/DBservers/containers/Docker55432/public deployment. Root serializes CARGO_INCREMENTAL=0 and requires completed session exit before next Cargo; no gate while cargo-run daemon wrapper active. Existing owned operator services remain off and prior synthetic stores/settings/grants preserved. Final context-only update to the authorized separate animaOS chat follows publication; this chat owns kit implementation.

## Historical M29 handover

# Handover: bounded cycle advice verified (M0–M29)

## Start here (2026-10-13): M29 complete; publication ledger is authoritative

- M28/PR34 merged at `9262030949e170aabe670518088dbff3ccc289c0`. M29 branch `g9000/companion-agent-loop-m29` adds bounded repeating-cycle advice. Plan [M29](../plans/2026-10-13-companion-agent-loop-m29.md); exact tested source, PR, verified merge and resume are in `.superpowers/sdd/2026-10-12-companion-agent-loop-m29/progress.md`. Finish explicit-path unsigned publication/attachment/verified-head standing merge/main fast-forward if absent. No repeated approval.
- Three identical complete laps of periods 2–4, with at least two distinct full bounded successful fingerprints, yield one fixed cycle advisory per invocation. Only fresh single physical plain-content successes qualify. Keep at most12digests; errors/ineligible/recovered/parallel/refusal/evaluator/text clear history but retain the once-warned latch. New invocation/restore resets all. Necessary polling and actual calls/results remain; no progress/effect inference, block/stub/suppression/replay or new request/grant.
- Default8rounds can reach period2 after6; periods3/4 need existing explicit9/12 budgets. Cap0 and cap6 no-tools/maxretry0 wrap-up, Stop/provider/receipt/deadline and M24/M27 controls remain. Advice is trusted fixed transient system text, absent from saved conversation/settings/receipts. Public Hermes comparison only from pinned1fd182's `_detect_identical_cycle`; full MIT retained.
- Required fresh Rust2411passed/8ignored/40suites and lint pass; meaningful missing-hint0versus1 RED preceded production;7compact new cases. One native batch and final SDK/web contract review found no actionable issues; no source fix wave needed. First restricted-network mockHTTP failures retained, unchanged narrowly authorized rerun passed. Root recorded one read-only formatting-check scheduling overlap and returned to strict serialized Cargo. No new provider/service/browser task; current scripted advice does not establish live usefulness.
- Continue M30 only after M29's reviewed source merges: ignored `.superpowers/sdd/2026-10-13-companion-memory-m30/draft.md` proposes explicitly granted `memory_remove` for one own visible Fact, exact ID/full current text under one write lock, existing MemoryMutation persistence/index handling and related citation cleanup. Root must bind five risks on the new branch. This is active-record hard removal, not the core MemoryPort's revisioned tombstone/audit or complete erasure. No automatic consolidation/sweep/grant, real-data deletion, profile/credential access or live model task. Owner UI deletion already exists; model-facing removal is missing.
- M28's actual memory/edit/file/wrap-up/quiet-restart acceptance and limits remain [documented](../../companion/harness-acceptance.md). SDK130/web1192/typecheck/build remain historical checks of unchanged surfaces. Semantic consolidation, browser, real connectors/accounts and hosted uptime remain unverified; no complete Hermes/OpenClaw parity claim.
- Standing cheap Codex lean plans/five root risk rulings/source workers/batch review/one fix wave/local gates/unsigned reviewed merge-and-continue authority persists. Preserve unrelated `nx.json`; stage explicit owned paths, never `nx.json`, `anima.yaml` or private evidence. No worktree/stash/reset/restore/path-checkout/deps/features/newtargetdirs/ordinary profiles/secrets/real connectors/sends/DBservers/containers/Docker55432/public deployment. Root serializes all Cargo with CARGO_INCREMENTAL=0; no gates while cargo-run daemon wrapper active. Owned services remain off; prior synthetic settings/grants and stores preserved. Authorized separate animaOS chat receives context only; this chat owns kit implementation.

## Historical M28 handover

# Handover: latest native core loop accepted locally (M0–M28)

## Start here (2026-10-12): M28 accepted; publication ledger is authoritative

- M27/PR33 merged at304184da765875b90a37104272558ac34f12ebc9/source3837cb71533f2068ebd1d231895665affd6de1da. M28 branch g9000/companion-agent-loop-m28 is docs-only latest-core acceptance. Plan [M28](../plans/2026-10-12-companion-agent-loop-m28.md); [evidence and limits](../../companion/harness-acceptance.md). Exact source/PR/verified merge/resume are in .superpowers/sdd/2026-10-12-companion-agent-loop-m28/progress.md. If publication absent, finish explicit5-doc-path unsigned commit/push/PR attachment/verified-head standing merge/main fast-forward; otherwise no selected M28 work remains. No repeated approval.
- One real configuredgpt5.5 task run_5637f9bb-26dc-4d2f-8602-9f71ff5ff71f/chat:bb4bc177-2b7c-4e7c-a9f3-5fab3dabc53c: sequentialmemory_search/exactCASmemory_update onexistingownsyntheticmem-1791783806248-4/oneexact18byteworkspacewrite m28/acceptance.txt/no newline, thenacceptedbudgetWrapup fourthreply.12595reportedtokens/3persistentSucceededreceipts; independentamber-913 Fact unchanged. No resubmission/auxiliary probe/direct effect repair/deliberate rare trigger. Manual session title skips generation; max3round/20s/0retry is not a billing ceiling.
- Quiet same-store restart retainsoriginalcompletedrun/status/usage, all8messageIDs/roles/text/run-step-call-status-durationmetadata/acceptedmarker, every scalarreceiptfield, edited/unrelatedfacts andfilebytes/SHA/mtime. No observedreplay. Restoreonly3temporaryconfigfieldsnull/null/null while stopped, originalmodel/provider/21existinggrantnames retained; no wholeprofile rollback. All14recordeddaemon/wrapper/runneridentitiesgone,8080/4200free,0connectors/0running/0queued/0pendingFlush. Forcedquietcleanup is not graceful-wrapper evidence.
- Native source unchanged fromM27'sfreshrequiredRust2404passed/8ignored/40suites/lint. M25SDK130/web1192/typecheck/build is historical for unchanged surfaces. No redundantfullgates for docs-only M28; fresh evidence/docs review/format/local-link/diff precedes publication. Browser remains unverified; no workaround/closureclaim. Local gates do not establish hostedCI/uptime, semantic recall, external account delivery or fullupstreamparity.
- Controller-only verifier/preflight corrections before/after the same task: snapshotCamelCase/null-array guard, lowercase API fact type, actual hyphenatedlime-604/amber-913 markers, successfulplainContent usesnative toolStatus rather than JSONerror shape. Initial capture retained; no failed/repeatedprovider task. Restartpermissionreview timedoutbeforeexecution, oneallowedretry succeeded. Source/schema/authority unchanged; M20 evidence correctlydeterministic, not liveprovider.
- Resume practical personal use from [daily use](../../companion/daily-use.md), with [harness evidence](../../companion/harness-acceptance.md). The renewed bounded core-loop objective now has implemented/gated budget wrap-up, exact-failure and repeated-success advice, single unknown-tool correction, pre-stream retry, plus current actual memory/file/wrap-up/restart integration. Do not invent another broad milestone merely for feature count. Broader cycles/payload repair, semantic consolidation, connector consent/delivery, microphone/vision, browser and hosted reboot/TLS/off-site restore need concrete scope/evidence; no whole-product parity claim.
- Standing cheap Codex workers/lean plans/5binding risks/batch review/one fix wave/current-tree gates/unsigned reviewed merge authority persist for defined follow-up work. Isolated synthetic testing allowed; do not start ordinary connector-restoring profiles. Preserve unrelatednx.json; explicitpaths nevernx.json/anima.yaml/private data/logs; no worktree/stash/reset/restore/pathcheckout/deps/features/newtargetdirs/normalprofile-secret inspection/real connectors/sends/DBservers/containers/Docker55432/publicdeployment. Cargo CARGO_INCREMENTAL=0 serialized, no gates whilecargo-run wrapperactive; cleanup exactrecordedexecutable/UTC-start-tickPID only. Context-only separateanimaOS chat updatesauthorized; this chat owns kit implementation.

## Historical M27 handover

# Handover: repeated-result advice verified (M0–M27)

## Start here (2026-10-12): M27 complete; publication ledger is authoritative

- M26/PR32 merged at4f25d044094ba6c43ecc590f48279d234ff8ba00/source9335e32e7c19d5fffdfe86e4c3f9caf8a5faa42e. M27 branch g9000/companion-agent-loop-m27 adds bounded factual repeated-success advice. Plan [M27](../plans/2026-10-12-companion-agent-loop-m27.md); exact tested source/PR/verified merge/resume are in .superpowers/sdd/2026-10-12-companion-agent-loop-m27/progress.md. Finish unsigned explicit-path publication/attach/verified-head standing merge/main fast-forward if absent; otherwise cut M28 from that merged main. No repeated approval.
- Three consecutive fresh single-call typed Success/no error/full bounded plain text, same name/canonical args/text, yield one fixed trusted system hint per exact streak on the already-next request. It explicitly permits necessary polling. Identical output proves neither no progress nor effect absence. No block/stub/suppression/replay, extra request/event/result/marker, tool-name taxonomy or permission change. Digest32/count saturated3/warned is local/domain-separated/length-framed; raw private evidence never enters the hint.
- Name256/arguments existing64KiB/depth64/nodes10000/text64KiB bounds precede scans; nonfinite/overlarge/empty/inconsistent/nonempty structured or attached evidence resets. Recovered/parallel/text-evaluator/M25 refusal and new invocation/restore reset; M24 exact-failure policy unchanged. Actual call IDs, paired TaskResults, physical frames/DB/action witnesses remain. Stop/receipt/provider/deadline and M23 cap/no-tools/M26zero retry win; no effect-safety inference.
- Fresh required Rust2404passed/8ignored/40suites and lint pass. One meaningful runtime0-versus1-hint RED preceded production;6compact pure/runtime cases cover full-text middle differences, identity/bounds/reset, original effects/witnesses/recovery/restore, evaluator/refusal/parallel, cap0/cap3/Stop. One native batch/final contract review had no findings; no unnecessary fix wave. Provider responsiveness/usefulness remains untested; threshold-specific host failures follow unchanged controls/inherited gate. SDK/web/API/settings unchanged; no redundant gates or new live-provider/service task for M27.
- Next ignored M27/m28-draft.md is a small core-harness evidence matrix plus at most one latest-main isolated task: relevant own-memory search, exact-current-text edit and a new small workspace file within3rounds/20seconds/explicit0retry. Correct draft evidence: M20 was deterministic, no live model; daily-use M27 paragraph is now verified, not evidence before gates. One logical task can have up to4model steps; no resubmission/auxiliary provider probe or deliberate errors/cycles. Root binds5risks before live activity; inspect actual effects/receipts and quiet restart, preserve original settings and exact owned-process cleanup.
- Prior actual M17–M23 memory/work/helper/tasks/skills/compaction/schedule/restart/budget examples remain historical evidence; M24–M27 new branch contracts are deterministic. General cycles, semantic consolidation, real connector/account consent-delivery, browser and hosted uptime remain unproven; no complete upstream parity claim. Broader integration expansion is outside this renewed core-loop scope.
- Cheap Codex workers/lean plans/binding risks/batch review/one fix wave/current-tree gates/unsigned reviewed merges and isolated synthetic testing remain authorized. Preserve unrelated nx.json; explicit paths never nx.json/anima.yaml/private evidence. No worktree/stash/reset/restore/path checkout/deps/features/newtargetdirs/normalprofile-secret inspection/real connectors/sends/DBservers/containers/Docker55432/publicdeployment. Cargo CARGO_INCREMENTAL=0 serialized, no gate with cargo-run wrapper active; cleanup exact recorded executable/UTC-start-tick PID only. Context-only separate animaOS chat updates authorized; this chat owns kit implementation.

## Historical M26 handover

# Handover: bounded pre-stream transport recovery verified (M0–M26)

## Start here (2026-10-12): M26 complete; publication ledger is authoritative

- M25/PR31 merged at553e020e01e9074e468dc21dd8cdae5947d02416/source9bb5d16ac9dc95ca9f344b6f93a67e4d048b5465. M26 branch g9000/companion-agent-loop-m26 adds one bounded classified ChatGPT pre-stream retry. Plan [M26](../plans/2026-10-12-companion-agent-loop-m26.md); exact tested source/PR/verified merge/resume are in .superpowers/sdd/2026-10-12-companion-agent-loop-m26/progress.md. Finish unsigned explicit-path publication/attach/verified-head standing merge/main fast-forward if missing; otherwise cut M27 from that merged main. No repeated approval.
- ActualHTTP502/503/504 only: at most one250ms awaited retry using one body/same client/account/credential/endpoint/headers. None defaults1, explicit0 disables, positive clamps1. Private typed status/MIME/bounded-read/structured-code facts drive eligibility, never formatted diagnostics. Five recognized transient codes may retry; permanent/unknown JSON, event-stream, oversize/read failure, ambiguous sends/timeouts and every post-consume/partial/sink/final error do not. Unknown bodies need exact normalized plain/HTML+validUTF8. Failed-attempt usage is unknown; no billing/exactly-once guarantee.
- M23 transient wrap-up clone now forces max_retries0 without persistent mutation, including literal settingsNone. Three existing Anthropic/Google/OpenAI-compatible stream predicates honor explicit zero; their normal default retry statuses remain unchanged. Generic core delay reuses existing futures-timer and stays inside existing cancellation select; no dependencies/features/targetdirs/DTO/host/new policy framework. Cancellation/completion race retains existing select semantics, no universal no-second-attempt claim at a simultaneous edge.
- Fresh required Rust2398passed/8ignored/40suites and lint pass. Meaningful RED2core and7of12adapter failures preceded source work. Final gate includes2newcore/15newmockHTTP tests, original API retry controls and all host receipt/restart tests. One batch/final contract review found compile-helper and overly broad unknown-MIME issues; single fix wave uses Option/shared selector/exactMIME and added cases. Initial failure logs retained. SDK/web/settings schemas unchanged; their M25gates remain historical evidence. No actual provider/service/browser run for M26.
- Next ignored M26/m27-draft.md proposes factual advice after three identical fresh plain-text successful single-call results. Identical output cannot prove no progress/effect absence; allow necessary polling, no block/suppression/stub/replay, preserve original witnesses/budget/Stop, digest-only local state and no trusted tool-name taxonomy. Root must bind five risks on the next branch before implementation. General A/B cycles, semantic memory consolidation, real connector/account and hosted-uptime acceptance remain gaps; no full parity claim.
- Prior owned services remain off and8080/4200free; original synthetic settings/grants preserved. Core-loop parity continuation, cheap Codex workers/lean risks/batch review/one fix wave/local gates/unsigned reviewed merges remain authorized. Preserve unrelated nx.json; explicit paths never nx.json/anima.yaml/private data. No worktree/stash/reset/restore/path checkout/dependencies/features/newtargetdirs/normalprofile-secrets/real connectors/sends/DBservers/containers/Docker55432/public deployment. Cargo CARGO_INCREMENTAL=0 serialized, no gates with cargo-run wrapper active; exact recorded executable/UTC-start-tick PID cleanup only. Authorized context-only M24/M25 update reached separate animaOS chat; this chat owns kit work.

## Historical M25 handover

# Handover: unconfigured tool recovery verified (M0–M25)

## Start here (2026-10-12): M25 complete; publication ledger is authoritative

- M24/PR30 merged at7f0ffe11acbbbb88bcadd02214055b5cd2cb879f/source4c6b7cc0765b012c533b2fb9470b850c5ec88ae5. M25 branch g9000/companion-agent-loop-m25 adds one bounded model correction after a single unconfigured tool request. Plan [M25](../plans/2026-10-12-companion-agent-loop-m25.md); tested source/PR/verified merge/resume are in .superpowers/sdd/2026-10-12-companion-agent-loop-m25/progress.md. Finish explicit-path unsigned publication/attach/verified-head standing merge/main fast-forward if absent; otherwise cut M26 from merged main. No repeated approval.
- Only a fresh valid bounded single unconfigured call receives one actual assistant-call/static Error result pair, consumes one tool round and permits ordinary model continuation. Second denial, mixed/malformed/oversized/nonfinite/deep/reused/ambiguous IDs remain terminal. Retained runtime IDs are checked too. No dispatch/callback/pending DB step/physical tool frames/native action receipt, fuzzy name repair, grant or provider change. Configured host authorization/approval/receipt failures retain their existing paths. M23 budget0/max1 and Stop take precedence; M24 streak resets for refused non-execution.
- Fresh required Rust2381passed/8ignored/40suites and lint; SDK130, web1192/typecheck/build pass. Focused8core/1harness pass after meaningful Error-versusSuccess RED. Native batch review had no findings; final SDK/web review found misleading Used labels, fixed in one wave to tool requests and Tools were requested. Transcript-backed refusal regression uses exact static text, real interrupted outcome, zero receipts and error-JSON fallback. Sandbox toolchain/temp-cache failures and initial stale assertion failures are retained in ignored logs; narrow reruns pass. No new provider task/service or browser proof.
- Next M26 draft is ignored M25/m26-draft.md: one typed, pre-stream ChatGPT 502/503/504 retry, same body/account/authority, terminal auth/schema/quota codes and event-stream/read ambiguity excluded, no send-error or partial-stream replay. Default one/explicit zero/clamp one;250ms cancellable existing timer. Root must bind five risks after M25 merge, including retry-disabled M23 shadow configuration and honoring that setting in existing API-provider stream retries. No extra dependency/feature/targetdir or broad retry framework.
- All previous owned services remain off; original synthetic settings/grants restored and no real connector/account/database/container action. Prior actual memory, multi-step work, compaction, schedule and restart evidence remains in [daily use](../../companion/daily-use.md); newer loop branches are deterministic tests, not full parity or provider-quality claims. Successful-call cycles, semantic consolidation, external account acceptance and hosted uptime remain gaps.
- Standing cheap Codex workers/lean plans/five binding risks/batch review/one fix wave/local gates/unsigned merge-and-continue authority persist. Preserve original nx.json; explicit paths never nx.json/anima.yaml/private evidence. No worktree/stash/reset/restore/path checkout/deps/features/new targetdirs/secret-profile reads/real connectors/sends/DBservers/containers/Docker55432/publicdeployment. Cargo CARGO_INCREMENTAL=0 serialized with no cargo-run wrapper active. Exact recorded executable/UTC-start-tick PID cleanup only. This chat owns kit work; context-only updates to the separate animaOS chat are authorized.

## Historical M24 handover

# Handover: native repeat-failure advisory verified (M0–M24)

## Start here (2026-10-12): M24 complete; publication ledger is authoritative

- M23/PR29 is merged at9ee0d50f9575b10fb4b78c5a7289df3cd3f8784d/testedsource11e41df8cd5dd7714fd270ebefcd2dd48b6727b1. M24 branch g9000/companion-agent-loop-m24 adds one conservative exact-failure advisory. Plan [M24](../plans/2026-10-12-companion-agent-loop-m24.md); exact tested source/PR/merge/resume live in .superpowers/sdd/2026-10-12-companion-agent-loop-m24/progress.md. Finish explicit-path unsigned commit/push/create+attach PR/verified-head standing merge/main fast-forward only if the ledger lacks publication. Otherwise cut M25 from that merged main; no repeated approval.
- Three consecutive fresh single-call typed Error results with the same canonical arguments/name/error yield one fixed transient system hint on the next existing request. State is digest/count/warned local to one invocation. Changed, successful, malformed, recovered, parallel or text outcomes reset it. Bounds use existing name256/arguments-result64KiB/depth64/nodes10000 with conservative pre-traversal; uncertainty misses a warning without failing a tool. Call IDs, durations and original result pairs remain unchanged; no raw error/argument interpolation, extra call, blocking, config/DTO/host wiring or dependency.
- Fresh required Nx Rust2373passed/8ignored and lint pass; focused6pure/runtime tests pass after meaningful warning0-versus1 RED. Independent native batch review found no implementation issue. One docs fix corrects Hermes per-signature counters across different failures versus deliberately consecutive native behavior. SDK/web/API unchanged, M23 compatibility boundary retained. New Stop-at-cap/parallel/privacy/reset/oversize/nonfinite/recovered cases cover changed logic; provider/receipt cancellation uses inherited M23/host coverage. No new live-provider quality claim.
- M23's single actual gpt-5.5 read/unfinished-write task remains historical evidence:2steps/3692reportedtokens, one durable read/no output, same run/four messages/saved accepted marker after quiet restart. M24 did not run another provider task or service. All prior owned processes remain gone, ports8080/4200free and original private settings/grants restored. Retain ignored M16/M23 evidence; no browser or hosted-uptime claim.
- Next M25 draft is .superpowers/sdd/2026-10-12-companion-agent-loop-m24/m25-draft.md: one paired static refusal for a single unconfigured tool, no dispatch/effect receipt or permission change, consumes the existing round budget and allows one bounded correction opportunity; second denial terminal. Root must bind five risks on the next branch before source work. Empty/malformed/mixed unknown batches, successful-call cycles, broad retries/classifiers and external integrations remain separate gaps; no full parity claim.
- User renewed core-loop parity despite an older broad-expansion memory freeze. Continue bounded practical Rust work using Codex workers/lean plans/five binding risks/batch review/one fix wave/current-tree gates. Preserve unrelatednx.json and explicit staging never nx.json/anima.yaml/private stores; no worktree/stash/reset/restore/path checkout/deps/features/newtargetdirs/realconnector-accountconsent/sends/DBservers/containers/Docker55432/publicdeployment/credential inspection. Cargo CARGO_INCREMENTAL=0 serialized and no gates while cargo-run wrapper active; exact PID/executable/UTC-start-tick cleanup only. Standing unsigned/merge-and-continue authority persists. Authorized context-only M23 update reached the separate animaOS chat; this chat owns kit implementation.

## Historical M23 handover

# Handover: native budget wrap-up verified (M0–M23)

## Start here (2026-10-12): M23 complete; publication ledger is authoritative

- Latest merged baseline is M22/PR28 at a1d1af69c2ae1bd23e2194ba6978b29c7c10b810. M23 branch g9000/companion-agent-loop-m23 implements the renewed agent-loop parity request. Plan [M23](../plans/2026-10-12-companion-agent-loop-m23.md); exact tested source, PR, merge and resume are recorded in .superpowers/sdd/2026-10-12-companion-agent-loop-m23/progress.md. If publication is missing there, finish explicit-path unsigned commit/push/create+attach PR/verify tested head/standing-authorized merge/main fast-forward. Otherwise cut M24 from that merged main. No repeated GPG/merge approval.
- Core keeps the default eight tool-calling rounds, then uses its already-next model call once with explicit empty tools and honest progress guidance. Zero means text-only. Returned ToolCall or End+calls, empty/truncated text and evaluator Retry fail without another request. Stop, provider/stream and native storage failures remain terminal. Only accepted final replies get native budgetWrapup:true; original grants/settings/model and paired results survive. Completed means a saved reply, not verified completion of every requested action.
- Fresh final Nx Rust2367passed/8ignored across40suites and lint pass. Focused9core+1harness,17subscription controls pass; owner-route regression reproduced Null-versusTrue before the additive metadata allowlist fix and passes in the full gate. One native batch and final SDK/web compatibility review found no actionable issues. SDK/web code is unchanged: its existing opaque metadata map carries the new boolean. Outcome-save failure uses inherited M21 cancellation coverage; the new cap=1 coordinator regression targets intent failure.
- One actual gpt-5.5 task run_143165f5-a59a-4dde-9132-91624512e9f7/chat:9fbb0d7b-b9de-4c2d-a270-7b8ecdd40284 read the synthetic input, then honestly reported the write unfinished:2steps/3692reportedtokens, one persistent succeeded read_file receipt and no output file. Native saved marker initially existed but owner API hid it; corrected whitelist/schema without rerunning the provider. Quiet same-store restart exposed the original marker and retained the same run, four message IDs/roles/text, receipt/input hash and absent output. No replay/backfill or second provider task.
- Exact owned test daemon/wrappers are gone, ports8080/4200free, connectors0, running0/queued0/pendingFlush0. Original private max/timeout restorednull/null; grants/model/provider unchanged. Retain ignored M16 synthetic profile plus M23 initial capture/native-marker/final-restart/cleanup proofs. Browser remains unverified; no workaround. Cargo caches repaired narrowly inside the existing verified target root after missing-library errors; no new dependency/feature/targetdir.
- Next bounded M24 is one transient warning after three consecutive identical single-call typed failures, preserving original tool/result pairs and current budget/Stop boundaries. Draft is ignored M23/m24-draft.md; bind five risks before implementing on the next branch. Structured invalid-tool repair and phase-aware provider retries remain later gaps. Successful-call cycles, semantic memory/consolidation, external connector consent/delivery and hosted uptime are not proven; do not claim full upstream parity.
- Preserve unrelatednx.json, stage explicit paths never nx.json/anima.yaml/private stores. No worktree/stash/reset/restore/path checkout, DBservers/containers/Docker55432/public deploy/account consent/sends or profile/credential inspection. Cargo CARGO_INCREMENTAL=0 serialized, never with a cargo-run wrapper active; cleanup exact executable/UTC-start-tick PID only. User authorized Codex workers, isolated synthetic testing, per-command unsigned commits, reviewed-gate merges/continuation and context-only updates to the separate animaOS chat.

## Historical M22 handover

# Handover: selected local daily-use recovery accepted (M0–M22)

## Start here (2026-10-12): publish M22, then personal operator use

- M0–M21 are merged; latest PR27 at4d534dd6e941c75e8194317bcbd315683dce87f4/source9fea0d09cfd7e1dd46d41270abc4e2f009195ccd. M22 branch g9000/companion-overdue-acceptance-m22 completes docs-only overdue unclaimed once-task acceptance. Plan [M22](../plans/2026-10-12-companion-overdue-acceptance-m22.md); exact source/PR/verified merge/resume are appended to .superpowers/sdd/2026-10-12-companion-overdue-acceptance-m22/progress.md after publication. If unpublished, finish owned explicit commit/push/create+attach PR/verify tested head/standing-authorized merge/main fast-forward. Otherwise continue [daily use](../../companion/daily-use.md). No unfinished selected local harness source or acceptance task remains.
- Existing source preserves overdue stored due times and admits enabled unclaimed once work at startup. Actual gpt-5.5 task schedule-1791799707663-1/run_634be5a8-a312-4d6e-b151-d3aeb7669919 was created future, then exact owned daemon/wrapper exited44.464seconds before due. Postdue+5.044seconds persisted proof remained enabled/unclaimed/no matching run/fire/file. Same-store launch10:09:19.5895447UTC; automatic manualfalse fire10:09:21.788UTC(9.139seconds late), completed10:09:27.287UTC(launch+7.698seconds),2steps/6,157reportedtokens and one exact28byte write, persistent succeeded receipt plus matching committed tool/reply/history. No manual trigger, rearm, second provider task or native source fix.
- After original settings restoration, a completed-work restart plus20scheduler ticks retained the same disabled/nonrunning schedule/original due, one spoke occurrence/run/receipt/tool-result, identical messages and exact file bytes/SHA256/mtime. All recorded owned daemon/wrapper identities gone, ports8080/4200free, connectors0; original two private settings restorednull/null. Preserve synthetic M16 stores and M22 captures, including original failed verifier captures. Quiet forced cleanup is not graceful-wrapper proof.
- Controller-only verification corrections: JSON UTC launch decoded as DateTime was incorrectly reparsed through local date text; artifact JSON key ordering was incorrectly compared across PowerShell processes. Root fixed typed UTC/scalar comparisons and read-only reverified the same completed task. Initial capture was32.933seconds after actual launch, actual completion7.698seconds; no clock edits, recorded-time edits, extra inference or production source change. M21 source remains identical: Rust2354passed/8ignored/lint, SDK130/web1191/typechecks/builds inherited and not rerun. M22 freshly executed live acceptance and final owned docs/evidence review/format/local-link checks are separate.
- Scope proves one short-outage unclaimed once task, distinct from M21 already-claimed interruption/review. No claim about recurrent catch-up, arbitrary outages, sleep/clock changes, hosted uptime, external delivery or exactly-once effects. Semantic memory/consolidation, real messaging/Google consent-delivery, microphone/vision and hosted reboot/TLS/offsite restore remain untested and need their own scope. Browser policy remains refused; no workaround/closure/liveUIclaim. Personal operator use is the next step, not unsolicited integration breadth.
- Standing continue/isolated synthetic testing/per-command unsigned commits/reviewed-gate merge authority persist; no GPG/merge pause. No dependencies/features/targetdirs/worktree/stash/reset/restore/path checkout, real DBservers/containers/Docker55432/account consents/sends/public deployment or normal profile/secret/keyring/.env reads. Explicit staging never nx.json/anima.yaml/private stores/logs; preserve unrelatednx.json. CARGO_INCREMENTAL=0 globally serialized and no Cargo gates with active cargo-run wrapper; exact recorded executable/UTC-start-tick cleanup only. This chat owns kit code; authorized context-only updates may go to “Check Git pull and pending work” in separate animaOS without duplication.

## Historical M21 handover

# Handover: selected local companion recovery accepted (M0–M21)

## Start here (2026-10-12): publish M21, then everyday operator use

- M0–M20 are merged; baseline PR26 at5710e6b805ac7c83f3517e345d00fd58ee44426c/sourced63fa73. M21 branch g9000/companion-schedule-recovery-m21 is complete with native durable action receipts and actual interrupted scheduled-work acceptance. Plan [M21](../plans/2026-10-12-companion-schedule-recovery-m21.md); exact source SHA/PR/verified merge and resume are appended to .superpowers/sdd/2026-10-12-companion-schedule-recovery-m21/progress.md after publication. If unpublished, finish owned explicit commit/push/create+attach PR/verify tested head/standing-authorized merge/main fast-forward. Otherwise resume [daily use](../../companion/daily-use.md). No unfinished selected harness code or local acceptance task remains.
- M21 initial real crash preserved the file/no replay but lost native tool evidence before final chat commit. A bounded defaulted run actionReceipts array now saves intent before dispatch and outcome before returning the tool result; only acknowledged receipts publish publicly. Newer ordinary snapshots preserve acknowledged action-only patches on existing rows, without replacing run status/stop/independent fields or resurrecting removed rows. Save failure stops undispatched effects/model continuation; data-only settlement survives caller drop. Source/approval/Stop checks remain current after waits, including individually authorized calls while a sibling approval waits. Temporary no-store receipts are explicitly labelled; configured JSON store remains persistent even in memory persistence mode.
- Fresh local gates: receipt-focused12 GREEN after native missing-witness RED2 and P1 staleordinarysnapshot RED1; full Rust2354passed/8ignored and lint pass. SDK130/web1191, both test/typecheck/build pass; owner view focused40. One daemon batch review found the corrected P1; final web/SDK review found no actionable issues. Original helper-stop/policy/streaming assertions remain meaningful with new intent barrier/independent call IDs. Owned Markdown/Prettier/local-link/diff checks precede publication; hosted Nx Cloud is not verification.
- Real gpt-5.5 repair acceptance: one39byte write, call-linked persistent succeeded receipt while Running/messages0, exact owned daemon hard-crashed2026-10-12T09:19:21.2367213Z. Schedule schedule-1791796742170-1/run_7ebf4146-6c79-41cb-8f58-94390d70c120/call_U9ePcFtztuO6iMObeiDNCg4R. Two same-store restarts each quiet20ticks retained identical action witness, Interrupted/restart_during_run/no owner Stop/no final conversation, disabled/nonrunning schedule_run_interrupted and exactly one failed/manualfalse original occurrence; exact39bytes/SHA256/mtime unchanged. Preserve both this successful retest and the initial PARTIAL artifact/schedule; never backfill the old run.
- All exact recorded owned daemon/wrapper/curl processes stopped, ports8080/4200free, connectors0. Two original synthetic settings restorednull/null. Retain ignored M16 stores/workspace and M21 evidence. Quiet force cleanup is not graceful-wrapper proof. Browser URLpolicy remains refused, no workaround/closure/new live-UI claim. Normal internal ChatGPT auth used without secret/profile/keyring/.env reads.
- Selected local harness is accepted; broad Hermes parity is not claimed. Started means unknown; failed may leave effects; succeeded means handler-reported success. The effect-to-outcome-save crash window, partial intermediate conversation/provider usage and exactly-once effects remain unresolved boundaries. Hard-killed provider consumption is unknown; active store settlement has no hard I/O deadline. New dedicated tests do not cover every protected/deadline/deletion/retention/Postgres race permutation; existing full fixtures pass, live JSON acceptance is separate. Semantic memory quality/consolidation, actual messaging/Google consent-delivery/microphone/vision and hosted TLS/reboot/offsite restore remain untested. Further real account/deployment work needs that specific scope.
- Preserve unrelatednx.json; explicit staging never nx.json/anima.yaml/private stores/logs. No dependencies/features/targetdirs/worktree/stash/reset/restore/path checkout, database servers/containers/Docker55432/public deploy. Cargo CARGO_INCREMENTAL=0 serialized, never with cargo-run wrapper active; cleanup exact recorded executable/UTC-start-tick PID only. Standing continue, isolated synthetic local testing, per-command unsigned commits and reviewed-gate merge authority persist; no GPG/merge pause. Context-only handoff remains authorized to “Check Git pull and pending work” in separate animaOS; this chat owns kit implementation.
- Product names Anima/AnimaOS. Full MIT/source attribution retained for public upstream append_message guarded-write/abort-on-failure flow at clean1fd18277267152893a8a24792b79213e7ca3f899, adapted into native Rust receipts. No Python/profile/token bridge, upstream transcript format or turn-lease import.

## Historical M20 handover

# Handover: selected local companion harness accepted (M0–M20)

## Start here (2026-10-12): publish M20, then everyday operator use

- M0–M19 are merged, latest PR25 at91bc7913585722ee9e5d2be5df2bd4109ff0a483/sourced00314c. M20 branch g9000/companion-memory-search-m20 completes the bounded explicit-memory retrieval repair. Exact source SHA/PR/verified merge and resume are appended to .superpowers/sdd/2026-10-12-companion-memory-search-m20/progress.md after publication. If unpublished, finish owned explicit commit/push/create+attach PR/verify tested head/merge/fast-forward. Otherwise resume [daily use](../../companion/daily-use.md); no unfinished selected harness code or acceptance task remains. Standing reviewed-gate merge-and-continue and per-command unsigned commits persist; no repeated GPG/merge questions.
- M20 reproduced actual existing-API crowd-out: core owner/type/scope/minimportance filters followed a global top-k; hidden content/IDs/tags consumed explicit search/recent slots and hidden current ID/tag edits were allowed. Additive predicate-aware BM25/search/recall/recent methods now filter canonical eligibility before lexical limits and visibility across all six recall admissions before final limits. The daemon shares ID/content/tag visibility for reads and current-record update refusal after same-owner/not-found. Full canonical text/IDs, CAS/stale checks, mutation locking, persistence/rollback and independent fields remain unchanged. Existing explicit scope defaults, BM25 global statistics, weights, vector top-k and automatic context are unchanged; no additional model/embedding call or dependency.
- Fresh M20 gates: Rust2342passed/8ignored, lint pass; focused8core/10host GREEN after meaningful core4fail+1control and host9fail+1control RED. Independent BM25score/order/weightedrecall, six-source admission, zero/empty, hidden crowd-out, exact multiline correction and generic foreign/missing refusal tests pass. One combined review found no actionable issues; owned Markdown/local links/diff checks precede publication. No web/SDK source or HTTP protocol changes; M17 remains last web/SDK code gates. Do not rerun completed suites without new changes or a concrete unresolved failure.
- Actual local model evidence remains separate: M17 gpt-5.5 multistep report, exactly1helper, savedtodos, missingfile honesty, exact existing-fact correction preserving independent memory, owner-approved read-only skill reuse, automatic22message compaction, inflight interruption/no replay and explicit owner continuation, quiet restart/fresh recall. M18 automatic own Fact context recalled compacttable/lime604 after20newreflections/8foreignfacts in1step/2980tokens without tools. M19 autonomous once file task survived pre-due restart, fired137ms after due/manualfalse, completed2steps/6156tokens/exactly1write. After a completed restart+20ticks, same history/run/session/messages/usage, exact34bytes/hash/mtime and one successful write remained; disabled/nonrunning. No independent todo was created by that schedule prompt.
- All recorded owned validation services/wrappers are stopped; ports8080/4200free, connectors0. M19 original two private settings restored. Retain ignored synthetic M16 stores/workspace and milestone logs/evidence, not normal user profiles. Browser URLpolicy remains refused; no workaround/closure/newUIclaim. Normal internally configured ChatGPT auth was used without secret/profile/keyring/.env reads. Quiet forced cleanup is not proof of graceful wrapper shutdown.
- Scope accepted is the selected local harness, not broad Hermes parity. Semantic/vector candidate completeness and quality, autonomous memory consolidation/skill improvement, inflight schedule crash/overdue-unclaimed/recurrence/exactly-once effects remain unverified. External Telegram/Discord/Google consent/delivery, microphone/vision, hosted TLS/reboot/stopped backup/offsite restore also remain untested. Further real account or deployment actions require their specific scope; isolated local acceptance does not authorize sends, consents, containers or starting the normal connector-restoring profile. Use the daily guide for operator configuration and those acceptance boundaries.
- Preserve unrelatednx.json; explicit staging never nx.json/anima.yaml/private data. No newdependencies/features/targetdirs, worktree/stash/reset/restore/checkout-path, DBservers/containers/Docker55432 or publicdeployment. Cargo CARGO_INCREMENTAL=0 serialized, never with cargo-run wrapper active; exact recorded executable/UTCstarttickPID cleanup only. Context-only handoff already delivered to “Check Git pull and pending work” in the separate animaOS checkout; this chat owns the kit implementation.
- Product names Anima/AnimaOS; full upstream MIT/source attribution retained. M17 exact-entry replacement and M18 public compact-memory guidance were adapted at clean revision1fd18277267152893a8a24792b79213e7ca3f899. M20 native BM25 eligibility repair needs no upstream Python or private profile/token bridge. Do not remove accurate license terms or represent lexical selection as upstream storage/session snapshot behavior.

## Historical M19 handover

# Handover: selected local companion work and schedules accepted (M0–M19)

## Start here (2026-10-12): publish M19, then continue bounded memory-search assessment

- M0–M18 are merged, latest PR24 at7144181cf53c4c7482ea8e0d710fb4e30476b6ea/sourceb832b4c. M19 branch g9000/companion-scheduled-acceptance-m19 is docs-only: actual autonomous scheduled-work acceptance passed with no source correction. Source SHA/PR/exact merge/resume are appended to .superpowers/sdd/2026-10-12-companion-scheduled-acceptance-m19/progress.md after publication. Finish owned docs commit/push/PR attachment/verified merge if unpublished; otherwise continue from that merged main. Standing merge-and-continue and unsigned override remain authorized; no GPG/merge pause.
- Real gpt-5.5 once schedule restored enabled/idle10.7seconds before due after owned quiet restart. Fired137ms after due, manualfalse/spoke, completed2steps/6156reportedtokens/exactly1write_file success, finalM19_DONE. Actual file34bytes saffron318/no newline. Schedule-1791789424431-1 links run_6d758702-07f5-475e-b238-c8fa4fe5fed0 and session schedule:schedule-1791789424431-1. After completion and20ticks after another restart, same history/run/session, one successfulwrite, messages/usage/filebytes/hash/modificationtime unchanged. Disabled/nonrunning; no automatic repeat observed. TaskAPI retained2completedprior todos; this prompt created no separate todo row.
- The original two private lead settings were restored while stopped. Recorded exact owned daemon/wrapper processes stopped; ports8080/4200free, connectors0/runs0/queued0/pendingflush0. Only retained synthetic M16 profile/files and normal internally configured ChatGPT connection were used, with no secret reads or .env autoload. Web service stayed off; browser closure was unverified. This does not prove exactly-once effects, intentional inflight schedule recovery, overdue-unclaimed work, recurrence or hosted uptime.
- Unchanged M18 source gates remain current: Rust2324passed/8ignored, lint; focused core/provider/renderer/API pass and reviewed. M19 source/evidence review and owned Markdown/local links/diff checks precede publication. No repeated full gates/model probe for docs-only changes. Private evidence stays ignored; no new dependency/feature/targetdir, UI/SDK/API changes or external account action.
- Continue with a bounded assessment of explicit memory_search: M18 automatic context filters before limits, while existing reusable lexical search limits global index candidates before owner/type filters. Prove an actual deterministic own-memory crowd-out failure before scoping a minimal fix; no semantic/vector or autonomous lifecycle expansion. Keep one lean plan/five binding risks, bounded workers/review and current-tree gates if source changes. The user asked autonomous continuation through practical harness acceptance, not new integration breadth.
- M17 real report/helper/tasks, exact stale-safe correction, approved skill reuse, actual compaction and interrupted/manual continuation remain verified; M18 corrected preference recall after20newreflections/8foreignfacts passed without tools. See [daily-use guide](../../companion/daily-use.md). External Telegram/Discord/Google consent/delivery, microphone/vision, semantic embeddings and public deployment remain untested; do not claim broad upstream parity.
- Preserve unrelatednx.json; explicit staging never nx.json/anima.yaml/private data. No stash/reset/restore/checkout-path/worktree, newdependencies/features/targetdirs, DBservers/containers/Docker55432 or publicdeployment. Cargo CARGO_INCREMENTAL=0 serialized, never with cargo-run wrapper active. Stop only recorded exact executable/UTCstarttickPID. Context-only handoff already delivered to “Check Git pull and pending work” in separate animaOS repo; this chat owns work. Product Anima/AnimaOS; exact upstream attribution is retained where adapted, no Python/profile/token bridge.

## Historical M18 handover

# Handover: companion memory quality verified (M0–M18)

## Start here (2026-10-12): publish M18, then scheduled-work acceptance M19

- M0–M17 are merged; latest main59f391f2d32f4ebc03be2f8e1a83bf9a65a6437a is PR23/sourcef81228a. M18 branch g9000/companion-memory-quality-m18 implements reusable own Fact selection and daemon bounded quoted context. Source SHA, PR and verified merge/resume are appended to .superpowers/sdd/2026-10-12-companion-memory-quality-m18/progress.md after publication. Finish explicit commit/push/PR attachment/verified-head merge if still unpublished; otherwise cut M19 from that merged main. Standing merge-and-continue and per-command unsigned commits remain authorized; no repeated GPG/merge questions.
- Automatic context filters owner, Fact, exact Room authority and hidden ID/content/tags before ranking/limits. Shared/Private own facts require no world/session annotations; unprovided context cannot widen authority. Positive lexical coverage precedes importance, time and ID; up to six records/4,800 characters with intact IDs, quoted content and marked excerpts. Search full current text before exact memory_update. Generic/unrelated prompts get no automatic facts. No extra model/embedding call, canonical deletion/rewrite, evaluator change or background skill mutation.
- Real gpt-5.5 owner-API check passed after20 newer irrelevant own reflections and8 foreign matching facts: fresh prompt gave neither value nor marker and returned compacttable/lime604 in1step/2980reportedtokens with no tools. M17 exact correction, independent memory, helper/todos/report, approved skill, automatic compaction and interrupted/quiet recovery remain verified historical evidence. Semantic/vector recall, autonomous consolidation and broad upstream parity remain unproven.
- Fresh M18 required Rust gate2324passed/8ignored, lint pass; focused core9matched/provider5/renderer1/agentAPI15 pass. Combined review and parser supplement have no remaining actionable findings. The full gate exposed deterministic first-line-only context parsing, fixed with a meaningful RED regression and strict result-text ID/type/quoted-fact assertions. No web/SDK source changed; do not repeat their M17 gates without source changes. Owned Markdown/links/diff checks precede publication; private logs/fixtures stay ignored.
- Next bounded M19: one harmless real-model one-shot file task, restart before due and after durable completion; inspect actual file bytes/hash/modification time, schedule/manual-false/run/session history, then leave disabled and stop owned processes. Prepared lean draft is .superpowers/sdd/2026-10-12-companion-scheduled-acceptance-m19/plan-draft.md; root binds five risks after M18 merge. Do not claim exactly-once effects, recurrence, interrupted schedule replay or hosted uptime. Web service stays off; browser closure is unverified and no browser-policy workaround is allowed.
- User requested context sent to “Check Git pull and pending work” (01a11c45-9fab-7d43-a219-1346fde3521a), in the separate animaOS checkout. Context-only handoff was delivered; this chat owns implementation. Do not duplicate changes there.
- Isolated test daemon stopped; ports8080/4200free, connectors0. Retain synthetic M16 profile/evidence, use normal internal configured ChatGPT connection without reading/copying credentials. No .env autoload, personal files/profiles/keyring, real connector consent/sends, DB servers/containers/Docker55432 or public deployment. Preserve unrelatednx.json; explicit staging never nx.json/anima.yaml/private data. No dependencies/features/new target dirs/worktree/stash/reset/restore/checkout-path. Cargo CARGO_INCREMENTAL=0 and serialized; stop only recorded exact executable/start-tick PID.
- Product naming remains Anima/AnimaOS. M18 reuses concise public upstream memory-curation guidance at clean revision1fd18277267152893a8a24792b79213e7ca3f899 with full MIT retained. The native lexical selector is Anima code; no Python/runtime/profile/token bridge or frozen-session storage semantics imported. External accounts/microphone/deployment need their specific scope; local harness authorization does not imply those actions.

## Historical M17 handover

# Handover: selected local companion harness accepted (M0-M17)

## Start here (2026-10-12): M17 acceptance complete; publication ledger is authoritative

- M0-M16 are merged to main, latest PR22 atba781bf459cf48acf1b96c42c64886b091b1548c. M17 g9000/companion-harness-acceptance-m17 completes the owner-authorized real multi-step work/memory/restart milestone. Exact source SHA, PR, verified merge and resume are appended to .superpowers/sdd/2026-10-12-companion-harness-acceptance-m17/progress.md after publication. If interrupted before that record, finish explicit-path commit/push/create+attach PR, verify its tested head, merge under standing authority and fast-forward main. Otherwise resume daily use from [the guide](../../companion/daily-use.md); no unfinished acceptance or code task remains. Per-command unsigned commits are authorized; no repeated merge/GPG questions.
- Actual gpt-5.5 API-first acceptance: read inputs, recorded tasks, calculated exactly3rows/6units/5100cents, used exactly1bounded helper, handled missingdiscounts once, wrote/readback report and saved3completedtasks. A read-only skill was proposed as pending, exact synthetic body reviewed/approved by root, then actually loaded and used; completed report hash stayed unchanged. Temporary private contextBudgetTokens6000 caused real automatic compaction with a saved summary covering22originalmessages; original budget was returned afterward.
- Concrete memory gap: a correction appended a new fact and falsely claimed replacement while2oldfacts remained. Native memory_update now requires exact own-agent visible record ID and reviewed current text, rejects stale/unrelated/hidden edits and rolls back failed durable saves. Safe search/recent reads provide IDs/type/fulltext. Live retest updated both original fact IDs, preserving independentamber913; fresh chat after quietrestart independently recalled compacttable/lime604. Historical reflections remain historical; no semantic-vector quality or automatic memory-curation parity claim.
- New permission is Write and appears in Collaborate/Operate only. Existing configs remain unchanged/custom until an explicit grant/profile selection; saving unrelated settings never grants it. Root explicitly granted only the synthetic validation lead. No new HTTP/SDK protocol, global owner-memory authority, delete tool or background skill self-modification. Exact pinned-entry replacement behavior adapted from clean public upstreamrevision1fd18277267152893a8a24792b79213e7ca3f899 with fullMIT retained; no Python/profile/token bridge.
- Intentional in-flight restart happened after a durable checkpoint and completed/pending tasks while the actual ledger wasRunning. Recovery was Interrupted/restart_during_run with effects warning, no automatic replay. Explicit owner-directed continuation inspected existing state, wrote only the pending recovery note and completed the remaining task; checkpoint hash unchanged. Quietrestart retained correctedmemory, independentfact, completedtasks, approvedskill, summary and artifacts. This proves reviewed manual continuation, not automatic/exactly-once execution after interruption.
- Fresh source gates: Nxrust-daemon:test2310passed/8ignored (core636,host1674), focused6Rust regressions GREEN after expected RED; NxRustlint pass. Web1188tests/110files, focused16profiletests, typecheck/build pass. One daemonbatch and one finalweb/SDKcontractreview found no remaining actionable issues. SDKsource unchanged; dependencybuilds passed. Ownedformat/diff checks precede publication; logs and synthetic evidence retained in ignored ledger directory. Do not repeat finished gates without source changes.
- Only isolated Rustdaemon ran in M17 with retained synthetic M16stores/workspace, normal internal ChatGPTconnection, no .env autoload and embeddings disabled. Browser URLpolicy refused access; no workaround/webservice/newUIevidence. Recorded owned processes stopped; ports8080/4200 free, connectors0. No personal credentials/files, real connector consents/sends, databases/containers/Docker55432 or public deployment actions. Preserve unrelatednx.json; never stage nx.json/anima.yaml, ignored profiles or credentials. No new dependencies/features/targetdirs/worktrees/stash/reset/restore/checkout-path. Cargo CARGO_INCREMENTAL=0 and serialized.
- Remaining evidence boundaries: external Telegram/Discord/Google delivery/consent, microphone/vision, live schedules, semantic embeddings, autonomous skill improvement, public VPS/TLS/reboot/backup/offsite restore and broad upstream parity. Selected local harness scenarios now pass; choose further scope from actual daily-use needs rather than breadth. Further account or deployment actions need their specific authorization; isolated local testing does not authorize them.

## Historical M16 handover

# Handover: selected companion and isolated live acceptance (M0-M16)

## Start here (2026-10-12): live companion verified; publication ledger is authoritative

- M0-M15 are merged to main, latest PR21 at63d8eaa9f8d8c7637e587297ffe9fb8318f0bae5. M16 on g9000/companion-live-acceptance-m16 fixes the concrete live ChatGPT blocker and records acceptance. Exact source SHA, PR, verified merge SHA and resume are recorded in .superpowers/sdd/2026-10-12-companion-live-acceptance-m16/progress.md after publication. If interrupted before that record, finish explicit-path commit/push/create+attach PR, verify its tested head, merge under standing authority, then fast-forward main; otherwise resume operator setup from the guide. No repeated merge or GPG questions: per-command unsigned commits are authorized.
- Owner expressly allowed isolated local testing. Only the web console and Rust daemon ran on loopback, with fresh private control-plane/history/memory/workspace, synthetic data, no .env autoload and disabled embeddings. The existing normal ChatGPT connection was used by the runtime; no credentials or personal files were read or copied. Connector count stayed zero; no databases/containers/Docker Postgres55432 or ordinary workspace were touched. History SQLite is an embedded test file. Recorded validation processes were stopped and ports8080/4200 verified free; quiet forced process cleanup does not prove graceful development-wrapper propagation.
- Actual model acceptance passed on gpt-5.5: hello; SOUL identity Anima validation; read_file read cedar-742; memory_search recalled cedar tea and independently saved amber-913. Stop cancelled harmless generation with partial text and server cancelled/stopped status. Same-store restart restored completed/cancelled conversation and run records. A fresh post-restart chat again used both tools and recalled the independent marker. Synthetic screenshot and private evidence remain in the M16 ledger directory.
- Concrete fix: DataValue f64 serialized integer JSON Schema bounds as1.0. The shared model adapter now canonicalizes eight integer-bound keywords only at schema positions; tool arguments, enum/default/instance numbers and numeric constraints stay unchanged. Same-model live success verified the fix. Terminal structured failures now expose only fixed known codes/parameters; bounded64KiB HTTP diagnostics and generic unknown fallback preserve privacy. Temporary diagnostic probes were removed.
- Fresh final source gates: Nx rust-daemon:test2304passed/8ignored (core636,host1668); focused adapter18pass; Nx rust-daemon:lint pass. Core/daemon batch and schema-fix supplement had no remaining actionable findings. No web/SDK source changed; M14 gates remain their last code gates. Owned Markdown/link/format checks precede publication. No new dependencies/features or target directories.
- Resume real operator setup from [daily-use guide](../../companion/daily-use.md). The selected local chat/memory/file/restart path is now live verified. Telegram/Discord delivery, Google consent and live Gmail/Calendar/Drive actions, attachments/vision, microphone, schedules, VPS/TLS/reboot and stopped complete backup/offsite restore were not exercised. No broad upstream parity, all-account readiness or production deployment claim. Further real connector/account or deployment testing needs its specific authorization; isolated local permission does not authorize sends, consents or containers.
- Preserve unrelated nx.json; stage explicit owned paths only, never nx.json/anima.yaml. No stash/reset/restore/checkout-path/worktree. Prefix every Cargo with CARGO_INCREMENTAL=0 and serialize. Product names are Anima/AnimaOS. Source reuse carries full MIT attribution; M16 exact clean upstream revision1fd18277267152893a8a24792b79213e7ca3f899 adapts terminal-error behavior, without a Python/profile/token bridge. Older revisions below are historical.

## Historical M15 handover

# Handover: selected companion build complete (M0-M14 merged; M15 final docs)

## Start here (2026-10-12): daily-use setup and live acceptance

- Selected daily-assistant implementation is complete: Rust long-running companion, web sessions/history/stream/Stop, Markdown identity/instructions, memory, workspace tools/approvals, attachments/browser voice, durable schedules/tasks/restart review, Telegram/Discord/Gmail/Calendar/selected Drive. No further selected-flow code blocker was found by the final source-grounded readiness assessment. Product names remain Anima/AnimaOS; upstream names occur only in accurate notices/source/history.
- PR20 is merged at37ffb48f936cba419ccae2a655537a128e31c568, source51d7e8a. M0-M14 are on main. Final source gates: Rust2298passed/8ignored(core630,host1668); SDK127/3service-skipped; web1187/110files; SDK/webtypechecks/builds, Rustlint/ownedformat pass. All26Drive and6card focused cases pass; Calendarownerreadguards and snapshot13/v12backup/toolinventory regressions are included. Source logs live under .superpowers/sdd/2026-10-12-companion-drive-m14/; use gate-rust-publish.log rather than superseded failures. Do not repeat finished gates without source changes.
- Final branch g9000/companion-readiness-m15 is cut from that merged main. M15 only writes the practical [daily-use setup and acceptance guide](../../companion/daily-use.md), accepted capability/risk plan and this status/handover; no runtime/test/manifest/deploymentconfig change. Root performs one concise docs/source-truth review, owned Markdown/link checks, explicit commit/push/PRattachment/verifiedheadmerge and returns updated main under standing authority. If resuming after interruption, inspect this branch's PR state: finish its publication/merge only if still open; after merge no further feature milestone is needed for the selected target.
- Resume actual operator use from the guide: configure a real model and consistent persistence/workspace/vault, identity files and tool grants; choose the desired connectors; run its live acceptance checklist only when those services/accounts are authorized. Current model/provider/channel delivery, Google OAuth/restricted-scope eligibility, actual file access/refresh, microphone, current images/TLS, VPS reboot and complete stopped backup/offsite restore have NOT been executed by these fixture gates. Do not call broad upstream parity or proven live/always-on readiness.
- Owner standing merge-and-continue permission persists for gated milestones; no repeated merge or GPG questions. Per-command unsigned commits are authorized. Preserve unrelated nx.json; never stage nx.json/anima.yaml. No new dependencies/features, worktree/stash/reset/restore/checkout-path or real services/daemons/DBcontainers/port55432 actions. Prefix cargo with CARGO_INCREMENTAL=0 and serialize it. No actual profile/keyring/account secrets were read. Operator commands in documentation are not execution permission.
- Native Drive enforcement: immutable1-16IDs, selected filename search only, bounded GoogleDocs/UTF8plaintext, no whole-account enumeration/writes/share/delete or auto-ingestion. Google readonly consent is broader and disclosed; local disconnect does not revoke projectwideGoogle permissions. Native Discord is selected text REST polling, not Gateway voice/slash breadth. Community connectors are trusted compile-in source/fullhostprivileges/rebuild, without a dynamic installer/sandbox. Standard access profiles replace custom tools deliberately; preserve selected connector/web grants.
- Upstream reuse remains the pristine read-only local source f3ef9db68332063a3625661f3a9b259f60abdb4e under C:/Users/leoca/AppData/Local/hermes/hermes-agent. Selected actual functions are ported/adapted into Rust and full MIT terms retained; no Python/runtime/profile/tokenJSON bridge. The notice is attribution, not product naming or evidence that AI-generated origin removes its license terms.
- Exact final publication/merge SHA and resume facts are appended to .superpowers/sdd/2026-10-12-companion-readiness-m15/progress.md after the tools verify them. The guide's unexecuted live acceptance is the remaining boundary, not an unfinished implementation milestone.

## Historical M14 publication

# Handover: companion console build (M0-M13 merged; M14 verified)

## Start here (2026-10-12): publish selected Drive, then final daily-use guide

- Owner standing merge-and-continue instruction remains in force. Do not repeat approval/GPG questions. Use Anima naming; exact upstream MIT notices/source evidence remain. No services/daemons/providers/accounts/keyring/profile secrets/databases/containers/port55432, dependencies/features or destructive Git/worktrees. Explicit paths never nx.json/anima.yaml; per-command unsigned commits authorized.
- M0-M13 are merged, latest PR19 main7f42df8. Active g9000/companion-drive-m14 has plan activation3cc4b78 and complete selected Drive code, review fixes and gates. Native Google consent/PKCE, immutable1-16selectedIDs, selected-name search and bounded Docs/plaintext read use separate private generation keys and agent tool authority. Provider readonly consent is broader and disclosed; local disconnect never remotely revokes Gmail/Calendar scopes.
- Final fresh Rust gate:2298passed/8ignored(core630,host1668), including26focused Drive cases plusroutes/schema/startupbackup. SDK127/3service-skipped, web1187/110files; both typechecks/builds pass. Initialcompile and fixturefailures corrected without changing productionWeakownership; deterministic testclock replacesTTLsleep. Logs .superpowers/sdd/2026-10-12-companion-drive-m14/{gate-rust-publish,gate-web-sdk,gate-web-fixture}.log. Final lint/ownedformat precede publication.
- Combined daemon fixes: atomicagentdelete Drive tombstones/cleanup and scopedrollback; boundcleanupqueue refusal; intentbeforevault and exactuncertain-write compensation; callbackwhole45s and expiryafterwait; serializedsearch32KiB; boundedpendingadmissionbeforegrantmutation. Finalwebfix preserves explicitrestartwithcachedexpiredconsentURL. Two confirmed Calendar readguards added with403regression, no runtime migration.
- Snapshot13/v12.pre-drive.bak and secret-freependingrestart. READMEOpenAPI158operations; exactGETDrivecallback gatewayexception added and staticallychecked; existinglistenerfixture updated but neverlaunched. No manifest/lockfeatureschanged; originalnx.json remainsunstaged.
- Next: explicitownedsource/docscommit/push/create+attach+verifiedmerge M14 understandingauthority. Fastforwardmain/cut final readiness branch fromthatmerge. Prepared docs-only assessment/drafts in .superpowers/sdd/2026-10-12-companion-drive-m14/{final-readiness-draft,daily-use-guide-draft}.md. No other selecteddaily-flow implementation blocker found. Finish actual setup/evidence guide, ownedMarkdownchecks, commit/push/PRmerge and precisehandover. Do not repeat fullsourcegates for docs-only milestone.
- Liveaccount/provider/channel/microphone/currentimage/TLS/reboot/restore acceptance remains unexecuted under no-servicesconstraint. Implemented capability is not liveparity proof. Upstream readonlysource f3ef9db at C:/Users/leoca/AppData/Local/hermes/hermes-agent; exact selected google_api.py behavior and MIT retained in nativeDrive notice. No Python/profile/tokenJSON runtime.

## Historical M14 activation

# Handover: companion console build (M0-M13 merged; M14 active)

## Start here (2026-10-12): selected Drive, then daily-assistant readiness

- Owner standing instruction remains: merge and continue until usable like the reference assistant, reuse/copy actual upstream code into Rust, keep Anima naming. Gated selected-use milestones may merge without another approval question. No dependencies/features, real daemons/services/providers/keyring/profile reads/databases/containers/port55432, destructive Git operations or worktrees. Explicit staging never nx.json/anima.yaml; unsigned per-command override is already authorized.
- PR #19 merged at `7f42df8334af7e0afdb71f83a5d52aeaa48e6d67` with verified source `0a5c74c`. M0-M13 are on main. M13 fresh gates: Rust2268/8ignored(core630/host1638), SDK125/3service-skipped, web1158/107files; SDK/webtypechecks/builds, Rustlint and ownedformat pass. Thirty-six native cases +startupbackup regressions pass; combined review fixes complete. Never repeat finished M13 work/gates absent new changes.
- Active branch `g9000/companion-drive-m14` is cut from that updated main; only unrelated nx.json dirty at activation. Plan `docs/superpowers/plans/2026-10-12-companion-drive-m14.md` contains interfaces, source/protocol verification, five binding risks and controller freeze. Ledger/rules under `.superpowers/sdd/2026-10-12-companion-drive-m14/`.
- M14 implements immutable selected1-16 Drive file IDs, selected filename search via exact metadata GET and bounded GoogleDocs/plaintext reads. Use existing Google app/PKCE/native authority patterns; no global Drive enumeration, writes/share/delete, auto-ingestion, document engine or Picker. Google drive.readonly consent is broader than Anima's allowlist and must be disclosed honestly. Local disconnect must never remotely revoke all project OAuth scopes (Gmail/Calendar impact).
- Actual reusable upstream code: `skills/productivity/google-workspace/scripts/google_api.py::drive_get/drive_download` at read-only local source C:/Users/leoca/AppData/Local/hermes/hermes-agent, pinned pristine revision f3ef9db68332063a3625661f3a9b259f60abdb4e, MIT Nous Research2025. Adapt selected HTTP/format behavior, retain full notice. drive_preview is desktop browser control, not Google Drive. No Python/gws execution, profiles/config/token files or user secrets.
- Native Drive tokens/generations stay separate from working Gmail/Calendar. Source IDs never grant permissions; every read rechecks current agent, connector, selected IDs/account/generation/tool access after waits. Callbacks/refreshes cannot resurrect a disconnect. Managers retain weak daemon references to avoid cycles. Snapshot13 needs native records and v12 pre-Drive backup; old fixtures use current-version constants already updated in M13.
- Next: delegated A nativeclient/model/store/provenance, B manager/state/schema/routes/tools/runtime, C SDK/web selected-file card. Root owns docs/master/handover; one daemon batch and final web/SDK review, one combined fix wave, relevant fresh local gates, explicit commit/push/PR attach/verified merge; then final deterministic daily-flow acceptance/setup. Live account/provider/hosting tests remain unexecuted under no-services constraint; no broad reference-parity claim.

## Historical M13 publication and standing continuation

# Handover: companion console build (M0–M12 merged; M13 active)

## Start here (2026-10-12): continue through the usable assistant target

- Latest owner instruction: "just merge and continue please until we come to stage of usuable similar to the hermes agent. just copy their code."
  This grants standing merge-and-continue authorization for the selected daily-use milestones after their reviews and local gates. Do not repeat merge/check-in questions. Preserve the existing no-services, no-dependencies, explicit-staging and unrelated-edit constraints.
- PR #18 merged at `f7a58084bc6a36273f528882463b4f516ef96bb1`; M0–M12 are on main. Its tested source `76011cd` and publication `8dc1635` are included.
- Active branch `g9000/companion-discord-m13` is cut from that updated main; only unrelated `nx.json` was dirty at activation. Never stage it or `anima.yaml`.
- M13 delivers selected-channel Discord REST chat through the portable Rust contract, with owner-configured sender access, bounded catch-up and durable admission/delivery/recovery. REST limitations are explicit; no Gateway/voice/slash-command parity claim. Tracked plan `docs/superpowers/plans/2026-10-12-companion-discord-m13.md` contains binding risks/interfaces and verified execution outcome.
- M13 implementation/reviews/combined fix wave are complete. Fresh gates: Rust2268/8ignored (core630,host1638), SDK125/3service-skipped, web1158/107files; SDK/webtypecheck/build, Rustlint and ownedformatting pass. Logs under `.superpowers/sdd/2026-10-12-companion-discord-m13/`: `gate-rust-final.log`, `gate-lint.log`, `gate-web-sdk.log`, `gate-format.log`. Old first Rust gate failed stale v11 fixture assertions; full corrected rerun passes. Thirty-six native/registry/route cases and startup backup coverage pass. Web warning origins remain baseline; unused Rust wrappers are fixture entrypoints.
- Native snapshot version12 has version11 .pre-discord.bak backup and nativevalidation. Unknown delivery never auto-replays; owner review suppression sends nothing. Stop during Sending suppresses remaining tail while retaining the current receipt. Terminal poll errors pause for explicit recovery. Initial activation baseline only; restart cursor retained. Owner routes/APIinventory154operations; no manifest/feature/lock changes.
- Next concrete action: explicit-file commit/push/create+attach+merge M13 at its verified head under standing authorization, fast-forward main, cut g9000/companion-drive-m14 and activate its prepared plan. No repeated approval pause. M14 delivers immutable 1-16 selected Drive file IDs, filename search and GoogleDocs/plaintext reads; existing Gmail/Calendar stay intact. Prepared draft: `.superpowers/sdd/2026-10-12-companion-drive-m14/plan-draft.md`. Local disconnect must not revoke all Google project scopes. Actual account/live service readiness remains separately unexecuted.
- Reference source is read-only `C:/Users/leoca/AppData/Local/hermes/hermes-agent`, revision `f3ef9db68332063a3625661f3a9b259f60abdb4e`, MIT Nous Research 2025. Discord code is `plugins/platforms/discord/adapter.py` and `recovery.py`; do not use the old guessed gateway path. Port selected actual code/behavior into Rust, record scope/revision and retain notices. No profiles/config/environment secrets or Python runtime execution.
- Product names stay Anima/AnimaOS. Upstream names remain only for accurate attribution/source evidence and historical identifiers.
- Usable target: provider-connected web/Telegram/Discord chat; Markdown identity/project instructions; workspace/tool access; remembered context; durable schedules/tasks with visible uncertain recovery; existing Gmail/Calendar plus selected Drive read/search. Make source contribution boundaries honest. Assess real code gaps rather than claiming broad reference parity. Account consent, live provider/channel delivery and always-on deployment remain separate live acceptance when the owner permits services.
- Controller uses one branch per milestone from merged main, lean plan plus five binding risks, delegated implementation, one daemon batch and final web/SDK review, one fix wave and relevant local gates. Keep cargo incremental disabled and compile serialized; no new target directory, dependencies/features, daemons, databases, containers or Docker Postgres55432 actions.
- Commits use the already-authorized per-command unsigned override. Do not pause for GPG. Finish each milestone with explicit-path commit/push/PR attachment, then merge its verified head under standing authorization and continue from updated main. Preserve exact resume/evidence in this handover and the ignored milestone ledger.

## Historical M12 publication before the authorized merge

# Handover: companion console build (M0–M11 merged; M12 ready)

## Start here (2026-10-12): M12 modular connector foundation ready

- Owner explicitly instructed merge and continue. PR #17 merged at
  `37218944488001caa4e34ae7f6fa13cde1f36d6b`, including Anima naming correction
  `21120aa`. M0–M11 are on main; no repeated M11 review/gates needed.
- Active branch `g9000/companion-connectors-m12` was cut from that updated main.
  Unrelated `nx.json` remains untouched/unstaged; never stage it or `anima.yaml`.
- Product naming remains Anima/AnimaOS. ANIMA.md/.anima.md and anima source kinds
  are merged; upstream names only belong in accurate attribution/source evidence
  and retained historical identifiers.
- Executable lean plan: `docs/superpowers/plans/2026-10-12-companion-connectors-m12.md`.
  Five binding controller risk rulings are supplemented by post-merge interface
  decisions. Ledger/rules: `.superpowers/sdd/2026-10-12-companion-connectors-m12/`.
- M12 implements pure Rust connector descriptors/metadata registry and async
  transport contract, then the existing Telegram adapter/registry pilot. Associated
  native payload/error types preserve Telegram metadata and uncertainty. Private
  per-dispatch adapters borrow the exact already-captured token; core requests and
  metadata contain no secrets, agent/session/provider authority or prompt paths.
- Preserve existing pairing, vault generations, durable admission/outbox, lifecycle,
  delivery attempt claims, recovery and stop behavior. Registry discovery is passive;
  community contributions are trusted compile-in source with host privileges.
  Installable ABI/sidecars/WASM and new Discord/Drive providers remain deferred.
  Existing owner API/SDK/UI do not gain installer or activation claims.
- Implementation and contribution guide are complete. One daemon batch review and
  final API/SDK/web/docs review found no actionable issues; the single fix wave
  reconciled the host trait aliases and formatting. Public core trait remains
  `ConnectorTransport`; host imports alias it as `AsyncConnectorTransport`.
- Fresh local Nx host test gate passed, including its core dependency:
  **2,230 Rust tests passed** (core 630, host 1,600), **8 ignored** (five Postgres,
  one live subscription smoke, one model-download test and one inventory printer).
  All six new contract/adapter tests pass, with existing lifecycle/claim/recovery
  fixtures traversing the new seam. Rust lint (`cargo fmt --all --check`) passes.
  Logs: `.superpowers/sdd/2026-10-12-companion-connectors-m12/gate-rust.log` and
  `gate-lint.log`. Explicit owned Markdown formatting is checked at publication.
- HTTP owner API, SDK/web sources, snapshot version 11 and dependency manifests
  are unchanged. TypeScript and Astro gates are conditional and were not repeated
  for unchanged sources. Local fakes establish compatibility, not live delivery.
- Contribution instructions: `hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md`.
  Portable text bounds are 1,048,576 Unicode scalars; excessive requests fail
  before transport, with a static native error. `attempt_id` is host correlation,
  not a provider deduplication promise. No new lifecycle or durable schema.
- Source is committed and pushed at `76011cd`; [PR #18](https://github.com/G9000/animaOS-SWARM/pull/18)
  is open against main and attached to the chat. Final publication documentation
  follows that source commit. Only unrelated `nx.json` remains dirty.
- Resume: await explicit owner PR #18 merge authorization. After that merge,
  cut the next milestone branch from updated main. The next bounded scope is
  Discord REST chat; Gmail/Calendar contract migration and selected Drive read/search
  follow separately. Do not create another milestone branch from unmerged M12.
- No new dependencies/features, services/daemons/providers/keyring reads/databases/
  containers or Docker Postgres55432 actions. Prefix cargo with CARGO_INCREMENTAL=0;
  serialize compilation, keep existing target dirs. Explicit file staging only;
  no worktree/stash/reset/restore/checkout-path. Unsigned per-command commit override
  persists; do not ask for GPG unlock or change global signing config.

## Historical M11 publication before the authorized merge

# Handover: companion console build (M0–M10 merged; M11 ready)

This document is for the next agent picking up the companion console upgrade.

## Start here (2026-10-12): M11 Anima Markdown definitions; PR #17 open

- Owner naming rule: our assistant is Anima/AnimaOS. Use ANIMA.md/.anima.md alongside AGENTS.md/SOUL.md; source kinds and UI use Anima. Upstream Hermes names belong in provenance/license credits only, with historical branch/ledger identifiers retained for continuity. Naming correction is verified: Rust2224/8ignored, web1133, SDK123/3service-skipped; SDK/web typecheck/build and formatting pass. This correction is committed and pushed on PR #17 before stopping. The owner naming rule is persistent for future work.

- M0–M10 are on main. Owner explicitly authorized PR #16 merge; `f80e653`
  includes final M10 head `eee9bb8`. Do not merge another PR without a new explicit
  owner instruction.
- Owner clarified Markdown defines agents and instructed reuse of installed
  Hermes code/behavior ported to an independent Rust long-running runtime. M11
  implements the focused AGENTS/SOUL port on `g9000/hermes-agent-files-m11`, cut
  from merged main `f80e653`. Implementation `5c7c496` is committed and pushed; [PR #17](https://github.com/G9000/animaOS-SWARM/pull/17) is OPEN against main and attached to the chat. Final publication documentation follows that source commit.
- Published lean plan: `docs/superpowers/plans/2026-10-12-anima-agent-files-m11.md`.
  Controller binding five-risk rulings, two daemon batch reviews, final web/SDK
  review and one fix wave are complete. A1 hard-link bypass and B1 summary reserve/
  B2 compaction workspace binding are fixed. A2 path-deny finding was rejected:
  actual policy denies risk classes, while rules/allowances grant allows only.
- Portable core renderer/transient identity lives in anima-core; safe host file
  loader, selected threat-pattern port, manifest, protected writers and runtime
  admission live in focused daemon modules. SOUL replaces legacy persona while
  preserving knowledge and immutable host role/delegation/origin. Runs keep one
  snapshot through tools, steers and compaction; later runs read current files.
  Configuration remains frozen at durable start. No durable schema/version change.
- File scope is configured workspace only, with companion-only root SOUL fallback
  or optional hex-ID profile. No ambient home/cwd/Hermes profiles, uploaded-file
  promotion, automatic migration, new memory store or Python runtime. Enabling
  `settings.agentFilesProfileEnabled` creates no files; it uses existing agent
  creation/workspace configuration, with no new Settings PATCH/console toggle.
- Current Settings preview and owner API show content-free metadata with explicit
  `current_preview`, capture time and digest; they do not claim active-run history.
  Model instruction writes demand exact owner decisions, create no reusable grants,
  recheck revisions, and refuse multiply hard-linked targets. Approved shell remains
  trusted full OS execution; no injection-immunity/sandbox guarantee is claimed.
- Hermes source revision verified: `f3ef9db68332063a3625661f3a9b259f60abdb4e`.
  MIT copyright/provenance retained in `hosts/rust-daemon/HERMES-NOTICE` and
  `packages/core-rust/crates/anima-core/HERMES-NOTICE`. Focused pattern/source
  behavior is adapted; differences and limits are documented in daemon README.
- Fresh local gates on final functional sources: Rust **2,224 passed, 8 ignored**
  (core dependency 627; host 1,597), web **1,133 passed**, SDK **123 passed, 3 skipped**.
  SDK/web typechecks/builds pass. Cargo fmt and explicit owned-file Nx format checks
  pass. Logs are ignored under `.superpowers/sdd/2026-10-12-hermes-agent-files-m11/`:
  `gate-naming-rust.log`, `gate-naming-web-sdk.log`, with earlier evidence in
  `gate-web-sdk.log`, plus formatter logs. The seven Postgres tests and one
  manual inventory-print helper remain ignored; SDK service tests are explicitly
  skipped. Existing web warning origins remain Operations/Dashboard/unknown
  connection; focused preview tests have no new act/unhandled warnings.
- No real daemon, dev server, database, container, live provider, microphone or
  Hermes runtime/profile/credential access during M11. Tests use fake workspaces,
  provider captures and in-process routes. Windows junction/hard-link boundaries
  and cancellation/compaction fixtures pass; Unix-only cases remain unexecuted.
  Browser/live deployment/restore acceptance was not rerun and is not implied.
- M12 connector preparation is now tracked and reviewable at
  `docs/superpowers/plans/2026-10-12-companion-connectors-m12.md`, with binding
  controller five-risk rulings. No connector source is implemented. After explicit
  M11 merge, create `g9000/companion-connectors-m12` from updated main only when
  implementation is authorized. Scope foundation/trusted compile-in Rust traits
  and registry plus existing Telegram pilot; later Discord, Gmail/Calendar reuse
  and selected Drive reads follow separately verified plans. Installable ABI is
  deferred; do not promise sandboxed/drop-in community modules.
- Unsigned per-command controller commits remain owner-authorized. Never request
  GPG unlock or change global signing config. Stage explicit files only; preserve
  unrelated `nx.json`, never stage it or `anima.yaml`. Continue no dependencies/
  features, services, Docker Postgres55432, worktree/stash/reset/restore/checkout-path.
- Resume: await explicit owner authorization to merge PR #17. All tracked M11 work is committed and pushed. Do not repeat implementation,
  reviews or passing gates without new changes/failures. Ledger/rules retain exact
  decisions at `.superpowers/sdd/2026-10-12-hermes-agent-files-m11/`.

The following section retains M10 publication evidence.

## Historical M10 publication (2026-10-12): PR #16 was open; now merged

- M0–M9 are merged to main. Owner explicitly authorized M9 PR #15 merge;
  merge commit `647d606` includes final M9 handover `c9909df`.
- Current branch `feat/companion-console-m10` was cut from that merged main.
  Implementation is committed through `f2d0814`, formatting/docs through `af2b5f8`, with web `9c3b298` and core
  `bfc4d87`. [PR #16](https://github.com/G9000/animaOS-SWARM/pull/16) is OPEN against main and attached to the chat. All tracked M10 work is committed and pushed; do not repeat
  implementation or reviews. **Resume: review/merge PR #16 only after a new explicit owner merge instruction.** Current local gates are finished. Live deployment/Unix signal/provider/restore acceptance is a separately authorized follow-up; do not start services under the existing constraint.
- Published lean plan: `docs/superpowers/plans/2026-09-23-companion-console-m10.md`.
  Binding five-risk rulings plus the evidence-driven shared 40-second shutdown
  refinement are recorded. Codex implementers, both daemon batches, final web/SDK
  review and one fix wave are finished. A1 fixed job/schedule claim admission after
  producer closure; four deterministic regressions preserve already admitted work.
- Snapshot remains **v11**. Delivery claims are ephemeral and fenced by committed
  credential/row identity. Telegram transport holds no global transaction/state
  guard, and uncertain delivery remains at-least-once. Session search/snippet work
  runs after capturing selected state outside the read lock. Module/test extraction
  preserves the original 60 connector and 59 inline agent-run test names.
- Fresh functional gates on the final sources:
  - Rust Nx host test **2,186 passed, 8 ignored**: core dependency 621 and host
    1,565. Seven ignored tests require Postgres; one is an explicit README inventory
    print helper, not a required correctness test. Both Nx tasks passed.
  - Web **1,124 passed**; SDK **117 passed, 3 skipped** with
    `ANIMA_SKIP_SERVICE_TESTS=1`. All SDK/web test/typecheck/build dependency tasks passed.
  - Offline installed-Edge browser **16 passed**, browser typecheck passed;
    deployment contracts **4 passed**; dummy Compose `config --quiet` passed.
  - Functional logs: ignored `.superpowers/sdd/2026-09-23-companion-console-m10/`
    `gate-rust.log`, `gate-web-sdk.log`, `gate-browser-deploy.log`. Full web warnings
    have only test origins already present in M9 logs; focused 122 tests were clean.
    Existing chunk-size warnings remain. Cargo fmt and Nx format:check against origin/main both pass; formatter-only source adjustments are verified and the quote-independent deployment contracts/parser rerun passes.
- No real daemon, dev server, database, container, provider or microphone started
  during M10. Browser fixtures load built assets, normal SDK/SSE bytes and reducers;
  they do not prove real HTTP chunking, TLS, keyring/container restart or live
  providers. Current Unix signal/VPS/restore acceptance remains unexecuted under
  the no-service rule. `deploy/vps/README.md` scopes old 2026-09-17 smoke evidence
  and documents the complete stopped five-volume/env backup generation.
- Owner authorized **unsigned commits** on 2026-10-12 to avoid GPG while absent.
  This overrides every historical signing/pinentry instruction below. Controller
  uses per-command `git -c commit.gpgsign=false commit`, never global config changes.
- Preserve unrelated `nx.json`; never stage it or `anima.yaml`. Continue all
  no-dependency/no-service/explicit-path/no-worktree/stash/reset/restore rules.
  Ledger and worker notes: `.superpowers/sdd/2026-09-23-companion-console-m10/`.
- Hermes focused provider formats/MIME behavior are ported with MIT provenance in
  `packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE` (revision
  `f3ef9db68332063a3625661f3a9b259f60abdb4e`). Scoped recovery/orderly stop inform the
  independent Rust runtime. Telegram/Discord/Gmail/Calendar/Drive priorities and
  community connector lifecycle remain follow-ups; Discord/Drive/community lifecycle
  are not shipped here. Automatic session-record TTL awaits a product policy.

The following sections retain historical M9 evidence.

## Historical state (2026-10-11): M9 PR #15 open; M10 plan prepared

**State**

- M0–M8 are merged to `main`. The last merges were #12 (M6), #13 (M7) and #14 (M8); `main` is at `86cc87f`.
- M9 (attachments and browser voice) implementation is complete on `feat/companion-console-m9`, cut from `main`. Lean plan: `docs/superpowers/plans/2026-09-23-companion-console-m9.md`; the owner authorized Codex subagents because Claude was signed out. Both daemon batch reviews, the final web/SDK review and one fix wave are finished. The full Rust gate caught old snapshot expectations; test-only corrections preserved historical migration coverage and passed the rerun.
- M9 is signed and pushed through `9efe2d6`. [PR #15](https://github.com/G9000/animaOS-SWARM/pull/15) is OPEN against `main` and attached to the Codex chat; it is not merged. Both waiting daemon commits finished, and the latest implementation/documentation signatures were verified. Only unrelated `nx.json` remains locally modified.
- The working tree uses snapshot **v11** with `.pre-attachments.bak` / `control_plane.backup.10`; merged `main` still uses v10.
- Hermes image routing/provider formats were used as Rust porting references. `packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE` retains the upstream MIT notice and revision `f3ef9db68332063a3625661f3a9b259f60abdb4e`. The owner clarified “remember” means reuse code/behavior, not a new memory feature.
- The lean M10 plan draft (10 tasks, about 1,200 lines) and controller top-five binding risk rulings are ready in the ignored `.superpowers/sdd/2026-09-23-companion-console-m10/plan-draft.md`; its ledger records the precise resume point. No M10 branch or tracked implementation starts before the owner explicitly merges M9.
- Compatibility: new v11 consumed steer keys preserve full input after transcript pruning; older keys require a matching scoped transcript to prove equality, otherwise replay returns 409.
- Fresh local gates on this working tree:
  - `rust-daemon:test`: **2,156 passed, 7 ignored** in aggregate: host 1,542 plus its `core-rust:test` dependency 614. The ignored tests require Postgres.
  - SDK/web test, typecheck and build: **web 1,117 passed; SDK 117 passed, 3 skipped**. Set `ANIMA_SKIP_SERVICE_TESTS=1`; those three SDK tests start a real service and are excluded under the owner's rule.
  - `cargo fmt --all --check`: passed after a controller-only test-module ordering correction. Nx format:check against origin/main also passed.
  - Logs: `.superpowers/sdd/2026-09-23-companion-console-m9/gate-rust-rerun.log` and `gate-web-sdk.log` (ignored local working files). Full web output includes act warnings from the unchanged `WorkspaceOperations` removed-filter test; the touched 312-test focused set was clean. Source matches `origin/main` and its API methods are mocked.
- Evidence limits: no live-provider image acceptance, real microphone/TTS, Unix link fixtures, Postgres execution or VPS deployment. One earlier SDK run accidentally started its existing temporary daemon fixture; it cleaned up, and subsequent controller process checks found no daemon. The opt-out now prevents recurrence.

**Resume M10 after explicit M9 merge**

1. Do not repeat M9 implementation or reviews. PR #15 is ready for the owner’s merge instruction; never infer merge approval from “continue” or “go.”
2. After the owner explicitly says “merge,” merge PR #15, update local `main`, and cut `feat/companion-console-m10` from that merged main. Preserve the unrelated `nx.json` edit; no stash/reset/restore/worktree operations.
3. Copy the prepared draft into `docs/superpowers/plans/2026-09-23-companion-console-m10.md`, recreate M10 rules from M9, verify target/interface names against merged main, then sign the plan. Its binding controller rulings are already appended; no separate pre-flight audit is needed.
4. Execute the ten M10 tasks with bounded Codex implementers (owner authorized), daemon batch review, one final web/SDK review, one fix wave and gates. Use focused Hermes code/behavior reuse with notices. Runtime/container/TLS/provider acceptance remains unverified under the no-service constraint; automatic destructive session TTL is not invented without a product policy.
5. Local ledgers, gate logs and draft artifacts remain under ignored `.superpowers/sdd/`; all M9 tracked work is committed and pushed. Re-run relevant checks only for new changes.

**Milestone workflow reference (M9 implementation is already complete)**

1. Have **Sonnet** write a **lean plan**, modelled on `docs/superpowers/plans/2026-09-23-companion-console-m8.md`:
   - Interfaces, rules, test lists and commands; full code only for tricky pieces; about 1,000–2,000 lines.
   - Output: `docs/superpowers/plans/2026-09-23-companion-console-m9.md`.
   - Sources: spec §12.1–§12.2, the master plan's M9 section, and the codebase.
2. The controller rules on the plan's top-5 risks: append a "Controller rulings on the risks (binding)" section and commit the plan. No separate pre-flight audit.
3. Implement task by task:
   - Sonnet implementers; Opus only for concurrency- or security-subtle tasks.
   - Implementers read the plan directly plus `.superpowers/sdd/2026-09-23-companion-console-m9/rules.md`. Copy it from the M8 folder and change m8 to m9; those working files are gitignored, so recreate them from `rules.md` in this handover if missing.
4. Reviews:
   - Batch-review the daemon tasks (2–4 at a time, Sonnet; Opus for risky areas).
   - One final web+SDK review covers all web tasks.
   - One fix wave.
5. Run the gates:
   - Use `NX_DAEMON=false`, `NX_ISOLATE_PLUGINS=false`, `NX_NO_CLOUD=true`, `CI=1`, `ANIMA_SKIP_SERVICE_TESTS=1`, and `NX_NATIVE_FILE_CACHE_DIRECTORY=node_modules/.cache/nx-native-m9`; set `CARGO_INCREMENTAL=0` before cargo or Rust Nx targets. Sandbox native-cache/realpath restrictions require escalated local checks; no dependency installation is needed.
   - `bun x nx run rust-daemon:test --skipNxCache`
   - `bun x nx run-many -t test,typecheck,build -p @animaOS-SWARM/sdk @animaOS-SWARM/web --skipNxCache`
   - `cargo fmt --all --check`
   - `bun x nx format:check --base=origin/main`
6. Update the master-plan status row and this handover, push, and open a PR to `main`. Merge only when the owner says "merge".
7. Keep a ledger in `.superpowers/sdd/<milestone>/progress.md`, recording each decision as `Ruling: <what> — <why> — <cost if wrong>`.

**Rules every agent follows**

- Prefix every cargo command with `CARGO_INCREMENTAL=0`.
- Commits and staging:
  - Stage by explicit path. Never stage `nx.json` or `anima.yaml`; they show as modified locally but aren't ours.
  - Never use `git stash`, `reset`, `checkout -- <path>`, `restore` or `worktree`.
- Environment:
  - No new dependencies or dependency features.
  - Don't start a real daemon, dev server, database or container. No Postgres is available, so Postgres tests stay `#[ignore]`.
  - Don't touch the Docker Postgres on port 55432.
- Code shape:
  - `agent_runs.rs`, `connectors/runtime.rs` and `ViewHarness.tsx` only gain wiring lines.
  - Lock order: control-plane transaction → state lock → leaf mutexes. No std lock across `.await`.
  - Owner mutations run their transaction/save/undo body in `tokio::spawn`, so they're drop-safe.
- Content and tests:
  - Model-written text renders in the web only as text, with hidden characters revealed (`lib/skills.ts` `revealInvisible`, `components/memory/RevealedText.tsx`).
  - Refuse hidden Unicode wherever the model writes (`skills::is_smuggling_character`, `memory_text.rs`).
  - Strings are named constants, each tested once.
  - Tests are never wall-clock-racy, and web tests stay pristine (no new `act()` warnings).

**Gotchas**

- **GPG:** commits are GPG-signed. When gpg-agent's cache expires, `git commit` blocks on a pinentry dialog until the owner types the passphrase. Run long commits in the background and ask the owner.
- **Line endings:** with `autocrlf`, git can check files out with CRLF, which `nx format:check` flags. `nx format:write` on those files fixes it, and nothing is committed if only line endings differ.
- **Corrupted build artifacts:** errors such as "invalid metadata", "not found in rlib format" or an internal compiler error mean the build artifacts are corrupted. Fix them with `cargo clean -p anima-daemon -p anima-core -p anima-model-adapters -p anima-memory`, then rerun.
- **CI:** it always fails at the Nx Cloud step. The local gates are the verification.
- **`gh pr edit`:** it fails; use `gh api -X PATCH repos/G9000/animaOS-SWARM/pulls/N`.
- **Stacked PRs:** they don't retarget automatically. PATCH `base=main` before merging.

**Open follow-ups**

- Owner direction (2026-10-11): use Hermes as a focused behavioral reference and keep Anima's independent Rust engine. Start with Telegram, Discord and Gmail, Calendar and Drive rather than integration parity. Connectors should support optional community modules. Today's TS `ModPlugin` is stateless/tool-only and Rust connectors are built-ins; a lifecycle-capable community connector interface is still needed. This is outside M9.
- Owner auth for the older memory routes.
- Hidden-text refusal for evaluator extractions.
- A `memory.updated` event.
- Token estimation for end-only usage providers.
- A combined usage summary endpoint.
- Telegram/CLI notice that an approval is waiting.
- Skip a skill scan while the previous one's blocking work is still running.

## Update 2026-10-09: M8 done, PR open (stacked on M7)

- **M8 (Usage, logs, health) is complete** on `feat/companion-console-m8`. Its PR targets `feat/companion-console-m7`; merge #12, then #13, then M8.
- **Gates at `65122bd`, all green:**
  - `rust-daemon:test`: 2,082 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 1,042 tests).
  - Formatting, and a log-secrets audit.
- **Snapshot v10** (pricing overrides). The first start writes a `.pre-usage.bak` backup.
- **Gotcha:** the first gate run failed on corrupted build artifacts ("invalid metadata", an internal compiler error). Running `cargo clean -p anima-daemon -p anima-core -p anima-model-adapters -p anima-memory` fixed it and freed about 87 GB.
- **GPG signing still times out** when the passphrase cache expires; Task 4's commit waited on the owner.
- **Follow-ups:**
  - Token estimation for providers that report usage only at the end.
  - A combined usage summary endpoint.
  - The Logs page can't tell an open-but-quiet stream from one that hasn't connected (no "open" event).
- **Next:** M9 (attachments and voice).

## Update 2026-10-04 (later): M7 done, PR open (stacked on M6)

- **M7 (Memory) is complete** on `feat/companion-console-m7`. The branch is based on the M6 branch, so its PR targets `feat/companion-console-m6`; merge M6 (#12) first.
- **Gates at `e33f919`, all green:**
  - `rust-daemon:test`: 1,925 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 932 tests).
  - Formatting.
- **Lean plan, about 900 lines,** with a cheap process: Sonnet implementers (Opus for Task 2), one daemon batch review, one web final review, one fix wave.
- **Formatting gotcha:** with `autocrlf`, git can check files out with CRLF line endings, which `nx format:check` flags. `nx format:write` on those files fixes it, and if only line endings differ, nothing gets committed.
- **Post-M7 follow-ups:**
  - Owner auth for the older memory routes.
  - Hidden-text refusal for evaluator extractions.
  - A `memory.updated` stream event.
- **Next:** M8 (usage, logs, health), with a lean Sonnet plan.

## Update 2026-10-04: M6 done, PR open

- M5 is merged (PR #11).
- **M6 (Automations) is complete** on the new branch `feat/companion-console-m6`. Each milestone now gets its own branch, cut from `main` after the previous merge.
- **Gates at `ccce91b`, all green:**
  - `rust-daemon:test`: 1,874 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 845 tests).
  - Formatting.
- **Cost mode (the owner asked for it):**
  - No separate pre-flight audit: the controller rules on the plan writer's risk list.
  - Sonnet does most of the implementing, with Opus for the risky tasks.
  - Batched reviews, and a web-only final review.
- **Ledger:** `.superpowers/sdd/2026-09-23-companion-console-m6/progress.md`.
- **Next:** M7 (memory). Keep it cheap: Sonnet writes a shorter plan (interfaces and tests, not full code), then the same cost mode.

## Update 2026-10-03: M5 done, PR open

- **M5 (Skills) is complete** on `feat/companion-console`, and a PR to `main` is open.
- **Work done:** all 13 tasks, then the final review (daemon and web+SDK reviewers) and one fix wave (`2060912` web, `0b08b2b` daemon).
- **Gates at `e9d57f9`, all green:**
  - `rust-daemon:test`: 1,775 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 807 tests).
  - `cargo fmt --check` and `nx format:check`.
- **Merging:** only when the owner says "merge".
- **Next:** M6 (automations), following the master plan. Write the plan, run a pre-flight audit, write the rulings, then execute.
- **Ledger:** every M5 ruling is in `.superpowers/sdd/2026-09-23-companion-console-m5/progress.md`.
- **Post-M5 follow-up:** skip a scan while the previous scan's blocking work is still running (hung-drive case).

## Update 2026-10-02 (latest): M5 in progress (Tasks 1–7 committed)

- **Done:** the M5 audit rulings are committed in the plan (`be0029f`). Tasks 1–5 are implemented, reviewed and fixed. Tasks 6 (`5729eab`, skill routes) and 7 (`61c6123`, draft/import routes and the multipart reader) are committed but **not yet reviewed**.
- **Resume:** run one combined review of Tasks 6 and 7, a fix round, then Tasks 8–14, the final review, the fix wave and the gates, then a PR.
- **Working files:** the ledger, briefs and rules are in `.superpowers/sdd/2026-09-23-companion-console-m5/`. These are local and gitignored; regenerate the briefs from the plan if they're missing.
- Task 7 hardened the multipart reader now (per-part caps, a linear boundary search) instead of deferring that to M9 (audit m19). Drop that deferral note.

## Update 2026-10-02 (later): M5 planned and audited, not started

- M4 is merged to `main` (PR #9, merge commit `f34313b`).
- **M5 (Skills) plan:** `docs/superpowers/plans/2026-09-23-companion-console-m5.md`, 14 tasks. Snapshot moves to v8, with `.pre-skills.bak` / `control_plane.backup.7` written first.
- **Pre-flight audit:** `docs/superpowers/plans/2026-09-23-companion-console-m5-preflight-audit.md`, with 0 Blocker, 4 Important and 19 Minor findings.
- **Resume here:**
  1. Rule on the audit findings: insert a `#### Controller rulings from the pre-flight audit (binding)` block per affected task and commit the plan. The auditor's recommended rulings are in the audit, and none needs a question for the owner.
  2. Extract briefs to `.superpowers/sdd/2026-09-23-companion-console-m5/` and copy `rules.md` from the M4 folder.
  3. Run Tasks 1→14 as in M4.
- **The four Important findings:**
  - **I1:** hidden Unicode (tag characters, bidi overrides, zero-width characters, U+2028/2029) in drafts.
  - **I2:** the page's Edit button can approve text nobody reviewed.
  - **I3:** Windows device names (`con`, `nul`, `com1`…) pass as slugs.
  - **I4:** `act()` warnings in the Skills page tests and `useSkillCommands`.
- **Tiers:** the auditor recommends the most capable model for Tasks 4, 5, 9 and 13.

## Update 2026-10-02: M4 done, PR open

- **M4 (Approvals) is complete** on `feat/companion-console`, and [PR #9](https://github.com/G9000/animaOS-SWARM/pull/9) to `main` is ready for review.
  - All 13 tasks are done, then a whole-milestone final review (split into daemon and web+SDK reviewers) and one fix wave (`9de5ea3` web, `20c6221` daemon).
  - Gates at `20c6221`: `bun x nx run rust-daemon:test --skipNxCache` passed 1,654 with 7 ignored; SDK and web test, typecheck and build are green (web 751 tests, SDK 63); `cargo fmt --check` and `nx format:check` are clean.
  - Merge only when the owner says "merge".
- **Next:** M5 (skills) per the master plan. Write its plan with an opus subagent from the spec, the master plan and the carry-forwards, run a pre-flight audit, then execute it the same way.
- **Post-M4 follow-ups:**
  - Tell Telegram and CLI users that an approval is waiting in the web console.
  - When a request's save fails, its card can linger until the tab reconnects; this is accepted, as in M3.
- **Ledger:** `.superpowers/sdd/2026-09-23-companion-console-m4/progress.md` holds every ruling.
- **Commits are GPG-signed.** When gpg-agent's cache expires, a commit blocks on a pinentry dialog until the owner answers it.

## Where things stood (2026-09-28)

- **Done and merged to `main`:**
  - M0 (security groundwork), M1 (run coordinator), M2 (sessions), and M3 (live runs).
  - PR [#6](https://github.com/G9000/animaOS-SWARM/pull/6) merged M0–M2; PR [#7](https://github.com/G9000/animaOS-SWARM/pull/7) merged M3 (merge commit `51f1f5b`).
- **In progress: M4 (Approvals).** The plan is written and committed, and its pre-flight audit is done. **No M4 code exists yet.**
- **Branch:** keep working on `feat/companion-console`. Don't switch branches in the shared checkout. Open the next PR from this branch when M4 is done.
- **M3 gates at its end:**
  - `bun x nx run rust-daemon:test --skipNxCache`: 1,564 passed / 7 ignored.
  - Web: 59 files / 703 tests. SDK: 16 files / 56 tests.
  - Typecheck, build, `cargo fmt --all --check`, and `bun x nx format:check --base=origin/main` all green.

## Key documents

- Spec (the binding authority): `docs/superpowers/specs/2026-09-23-companion-console-design.md`
- Master plan (milestone table and the "Carried from M3" list): `docs/superpowers/plans/2026-09-23-companion-console.md`
- Milestone plans:
  - `docs/superpowers/plans/2026-09-23-companion-console-m0.md` through `-m3.md` (done)
  - `-m4.md` (next, 14 tasks)
- M4 pre-flight audit: `docs/superpowers/plans/2026-09-23-companion-console-m4-preflight-audit.md`. Its rulings are not yet written into the plan (see "Next steps").
- Local-only working files: `.superpowers/sdd/2026-09-23-companion-console-m*/`. They hold the ledgers (`progress.md`), task briefs, reports, and review diffs, and are gitignored. On the same machine, the M4 ledger is `.superpowers/sdd/2026-09-23-companion-console-m4/progress.md`. Another machine won't have these files; everything essential is in this document and the committed plans.

## How the work is run

The workflow is the `superpowers:subagent-driven-development` skill:

1. One implementer subagent per task. It reads a brief extracted from the plan and commits its work.
2. A task review covers spec compliance and quality.
3. Up to 5 fix rounds follow, then a scoped re-review.
4. After the last task comes a whole-milestone final review. M3 split it into three parallel area reviewers: core+live, daemon, and SDK+web.
5. Then one fix wave, done in sequential slices, followed by the gates.

Decisions are recorded in the ledger as `Ruling: <what> — <why> — <cost if wrong>`. Opus handles concurrency-heavy tasks and reviews; sonnet handles mechanical ones.

## Repo rules (every agent, every task)

- Prefix every cargo command with `CARGO_INCREMENTAL=0`; test filters go after `--`.
- Stage files by explicit path only. Never use `git add -A`, `git add .`, or `git commit -a`.
- Implementers never stage `docs/` or `.superpowers/`; the controller commits docs.
- Never use `git stash`, `git reset`, `git checkout -- <path>`, `git restore`, or `git worktree`, and never switch branches.
- Don't start a real daemon, dev server, database, or container. No Postgres is available here, so Postgres tests stay `#[ignore]`.
- Don't touch the Docker Postgres on port 55432; it belongs to another project.
- No new dependencies.
- Keep touched files `cargo fmt` and Prettier clean. Format only touched files, never the whole repo.
- Lock order in the daemon: control-plane transaction mutex → state lock → live registry or fanout mutex. Room leases come before the transaction. No std lock is held across `.await`.
- Put new code in new modules or hooks. `agent_runs.rs` (about 5,600 lines), `connectors/runtime.rs` (about 7,500), and `apps/web/src/ViewHarness.tsx` (about 1,200) are already large.

## Gotchas learned the hard way

- **Disk is tight** (about 13 GB free); the Nx Rust gate needs about 12 GB. Leftover rustc codegen files pile up in `target/debug/deps/*.rcgu.o`. When no cargo or rustc is running, deleting ones older than 30 minutes freed about 13 GB once. Don't add extra `CARGO_TARGET_DIR`s, and never delete anything outside the repo; the user frees space there.
- **CI always fails** at `nx start-ci-run` (an Nx Cloud setup issue), including on `main`. The local gates are the verification.
- **Merging PRs:** auto mode denies Claude's self-merge unless the user explicitly says "merge". `gh pr edit` fails (GraphQL Projects deprecation); use `gh api -X PATCH repos/G9000/animaOS-SWARM/pulls/N`, and `gh api -X PUT …/pulls/N/merge -f merge_method=merge` once the user says to merge.
- **Subagent stalls** (the 600 s watchdog) happened on long single responses. Tell implementers to work one item at a time, commit after each, and pipe test output through `tail -30`. If an agent stalls twice, start a fresh one on the remaining items.
- **Plans:** fence partial code fragments as `text`, because Prettier mangles partial JSX fenced as `tsx`. End every SDK-changing task with `bun x nx run @animaOS-SWARM/sdk:build`, so later web vitest runs resolve the new exports.
- The model adapters' HTTP client has no request timeout, so every new long wait or model call needs its own bound.

## Next steps (M4)

1. Read the M4 plan's header and Global Constraints, and the pre-flight audit.
2. Rule on the audit's findings: 0 Blocker, 6 Important, 14 Minor. Then insert a `#### Controller rulings from the pre-flight audit (binding)` block at the end of each affected task in the plan, as M3's plan does, and commit the plan. The six Important findings, with the auditor's recommended rulings:
   - **I1:** Task 5's two race tests depend on 250/500 ms real-time timing and can flake under the parallel Nx gate. Rewrite them with patient timeouts, an explicit `settle_approval(TimedOut)` task, and `yield_now()` ordering, without paused time.
   - **I2:** A broad exec rule (curl, an interpreter, git, npm) lets the companion rewrite its own approval policy with a forged-Origin loopback request, or run code through default-allowed writes to `.git/config` or `package.json`. The auditor recommends documenting the limit in the README and warning in the UI, with no per-approval token. **Ask the user** whether they want stronger protection.
   - **I3:** With `exec: ask` as the default, check-ins, jobs, Telegram and CLI runs all wait for a web decision. The spec intends this. The legacy `/run` answers 408 after 10 minutes while the approval keeps waiting. Add README and UX copy.
   - **I4:** Document the snapshot v7 rollback (`.pre-approvals.bak` / `control_plane.backup.6`) in the README, as M2 and M3 did, and keep the v7 bump.
   - **I5:** Task 8's `cargo check` runs build a second set of artifacts on a disk-tight machine. Use `CARGO_INCREMENTAL=0 cargo test -p anima-daemon --no-run` plus a grep instead.
   - **I6:** "Always allow" on a delegated specialist's approval creates a rule the Approvals page can't show or remove. Hide "Always allow" when `approval.agentId !== companion.id`, or list those agents' rules.
3. Run Tasks 1→14 strictly in order. Task 14 is the gate plus the master-plan status row.
4. Final review, fix wave, gates, then a PR from `feat/companion-console` to `main`.
5. After M4, M5 through M10 follow the master plan: skills, automations, memory, usage/logs/health, attachments/voice, and deployment/docs. Each gets a plan written by an opus subagent from the spec, the master plan, and a carry-forwards file, then a pre-flight audit, then execution.

## Open items carried forward

These are listed in the master plan under "Carried from M3":

- **M8:** usage for stopped calls, compaction, and titles; the steer queue cap.
- **M10:** rewrite the Playwright specs that mock `/run`, split the large files, and add provider HTTP timeouts, SIGTERM handling, and shutdown producer ordering.
- **Manual acceptance:**
  - One real xAI call, which sends running usage totals.
  - One gpt-6 call, to check whether it accepts `temperature: 0.2` for compaction and titles.
  - Anthropic and OpenAI streaming end to end.

## Using what's built today

Run `bun dev --host rust`. It starts the Rust daemon on `127.0.0.1:8080` plus the web console and playground. Everything from M0–M3 works:

- Parallel sessions with a sidebar.
- Live streamed replies with tool cards.
- Stop, steering (Ctrl/⌘+Enter), and queued sends with safe retries.
- Slash commands, compaction, AI titles, and check-in replies.
- Helper sessions, and the `search_conversations` tool.

The first start after upgrading migrates the control-plane snapshot to v6, and the JSON store keeps a `.pre-live-runs.bak` backup. Approvals (M4) are not there yet, so tools run under the old rules with no approval prompt.
