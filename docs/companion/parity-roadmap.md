# Companion capability parity roadmap

Status: **scope locked; PAR-01, PAR-02 procedural/profile waves, PAR-03 anchored history, scoped PAR-04 HTTP/Windows MCP plus reviewed bundles, and scoped PAR-05 Windows loopback browser work implemented with local acceptance; remaining work queued**. M40 proves corrected-name context across restart, while its second introduction reply misread whom to introduce. M41 demonstrates one correct source-cited history task. This roadmap covers the gaps against installed Hermes and does not declare full parity or general task quality.

Baseline: Anima M36, `7d3eb52d7b37b8993ab2351fed33b209dff59ab6`; installed Hermes, `1fd18277267152893a8a24792b79213e7ca3f899`. The comparison inspected public source without starting either installation or reading personal profiles, accounts or credentials. M36 has current Rust gates; earlier real chat/task/account evidence stays revision-specific. See [harness evidence](harness-acceptance.md) and [daily use](daily-use.md).

## What “done” means

- **Planned:** work and acceptance criteria are recorded; no implementation claim.
- **Implemented:** current source provides the capability and required local gates pass.
- **Locally accepted:** a predeclared synthetic end-to-end case checks actual outputs, effects, cancellation and recovery on the recorded revision.
- **Account accepted:** the selected real account/destination/resource has a separately recorded, authorized read/write/delivery/revocation case.
- **Host accepted:** the target installation passes startup, streaming, shutdown, reboot, backup/restore and access checks.
- Overall scoped parity requires the applicable acceptance for every selected work item. Test counts, discovery metadata, a saved configuration or one successful example cannot substitute for it. Quality claims require independently checked task outputs.

Exact equality with every Hermes default is not the goal. Keep Anima's Rust engine/host separation, explicit owner control, bounded spending and retained source evidence. The initial integration scope stays Telegram, selected Discord text channels, Gmail, Calendar and Drive. Additional providers, seven terminal backends, unused channels and external user-modeling services are outside this program unless a concrete daily-use need adds them.

## Reuse-first implementation policy

Reusing appropriately licensed Hermes, OpenClaw and other open-source implementations is the default. Before each capability, inspect upstream and compare direct package/component reuse, a supported process/API adapter, and a narrow attributed port. Choose the lowest total integration, maintenance, compute and validation cost that preserves Anima's architecture and required behavior. Custom code needs a concrete recorded reason; originality or branding is not a reason to rewrite working code. This policy does not replace the working Rust engine.

Every work-item brief and review records **Reuse decision/provenance**: alternatives considered, selected approach/reason, upstream repository and pinned commit, source paths, license, separately licensed components and local changes for substantial copies/ports. Retain notices and Anima product branding. Prefer adequate existing dependencies; new dependencies remain allowed with a cost/compatibility justification.

Reuse receives focused integration/regression checks for owner authority, scope, budgets, cancellation and persistence/recovery. Upstream code alone does not prove local parity, semantic quality or exactly-once execution. Do not add model calls merely to compare implementations when deterministic checks suffice. Existing service/account/send/deployment permissions remain unchanged.

All PAR-01–PAR-12 implementer and reviewer briefs inherit this policy; their detailed milestone plans record the concrete decision before implementation.

## Locked work inventory

### Web research addition (M83)

M83 adds native anonymous web search and returned source links to the selected
daily-use scope. It reuses the existing bounded MCP transport and the installed
Hermes keyless Exa response behavior, with explicit owner search/page-read grants.
See [web research](web-research.md) and the
[M83 plan](../superpowers/plans/2026-10-16-companion-web-search-m83.md).
The milestone's recorded outcome distinguishes deterministic harness/console
checks, actual anonymous endpoint compatibility and unverified model research
quality. This complements PAR-04/PAR-05; it does not close interactive arbitrary
website or account acceptance.

M85 accepts one distinct actual Rust2018 research/report/read-back case after
correcting the private live-approval verifier. One anonymous search, two official
page reads, three independently checked cited answers, complete saved read-back,
five-second quiet recovery and owned cleanup pass. Five subscription calls
reported17,129tokens;60captured-artifact console checks pass. M84's earlier failed
actual task remains unchanged. This adds narrow research-quality evidence;
broader autonomous/coding, selected-account and target-host acceptance remain open.
See the [M85 outcome](../superpowers/plans/2026-10-17-companion-live-research-m85.md).

Each PAR ID is stable. Milestone numbers after M38 are assigned when its detailed plan is cut from then-current main. File/API names below are candidates; capability, authority, scope and evidence requirements are binding.

| ID     | Deliverable                                                  | Current state                                                                                                                                                                                                 | Dependency / accountable lane                                                |
| ------ | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| PAR-01 | Budgeted post-task learning and durable advice               | M38 implemented; one real anchored review and quiet restart accepted; advice only                                                                                                                             | M38 native learning + SDK/web; PAR-02 closes adoption/reuse                  |
| PAR-02 | Apply reviewed lessons to memory/skills and prove reuse      | M39 skill reuse and M40 profile/correction effects accepted; broader quality open                                                                                                                             | PAR-01; native skills/memory + owner UI                                      |
| PAR-03 | Rich own-history discovery and anchored reading              | M41 filtered anchors/window, optional owner grant and one source-cited task accepted                                                                                                                          | Existing history; native history + tool contracts                            |
| PAR-04 | Dynamic MCP tools and installable community extensions       | M42–M45 HTTP/Windows MCP and reviewed MCP/skill bundles locally accepted; M65 owner-managed static HTTPS Bearer locally accepted; OAuth/selected servers/non-Windows/contributions open                       | Existing tool policy; native protocol + SDK/web                              |
| PAR-05 | Interactive browser work                                     | M46 loopback and M77 one pinned public static-page content/cleanup case accepted; arbitrary sites, accounts, model reasoning and other hosts remain open                                                      | Existing BrowserPort/worker and owner controls                               |
| PAR-06 | Longer coding and delegated-task acceptance                  | M86 distinct bounded coding/helper case FAILED; M52 original strict quality remains partial; broader repository work open                                                                                     | Current harness; optional PAR-02/PAR-04 adapters                             |
| PAR-07 | Native voice input/output                                    | Partial: M56 selected-WAV transcription, M57 local capture and M58 selected-reply native speech/manual playback; synthetic acceptance                                                                         | Actual device/provider/voice-quality and connector/local-provider acceptance |
| PAR-08 | Text/image acceptance and PDF/Office understanding           | M54 DOCX/M55 PDF text locally accepted; M66 bounded XLSX stored cells and M67 granted workspace document reads with synthetic tool/history evidence; vision conditional; OCR/other Office/answer quality open | Existing document parser/owned worker + native tools/Settings                |
| PAR-09 | Selected real connector workflows                            | Telegram/Discord/Google code exists; real-account acceptance unverified                                                                                                                                       | Current connector code; later relevant media/browser changes                 |
| PAR-10 | Recurring schedules, outages and interrupted-work acceptance | M79 automatic catch-up respects active hours; M59 claim recovery and M62 controlled-clock interval outage/reload accepted; broader recurrence/host evidence incomplete                                        | Current scheduler; optional PAR-06 acceptance fixtures                       |
| PAR-11 | Always-on single-owner installation and recovery             | Deployment instructions/offline checks exist; target-host acceptance missing                                                                                                                                  | Release with required capabilities; deployment/operator lane                 |
| PAR-12 | Clear capability controls and daily-use status               | M81 fresh Tasks reconnect and retained drafts; M78 fresh Memory; M76 interrupted review and M75 truthful inventory; broader lifecycle UI incomplete                                                           | Add incrementally with PAR-01/04/05/07/08/09                                 |

### PAR-01: budgeted post-task learning (M38)

- [x] Add an explicit “review this completed task” action and an optional visible automatic mode, off by default.
- [x] Select bounded evidence from one settled own run. Preserve success/failure/unknown receipts and missing/shortened evidence; a completed response is not proof that the goal succeeded.
- [x] Admit a durable deduped attempt before one no-tools model request. No ordinary runtime fork, whole-transcript clone, evaluator writes or automatic retries.
- [x] Persist inspectable advice with supplied source anchors, review state, usage/unknown usage and decisions. Stop, opt-out and restart must not dispatch or publish an extra late/replayed result.
- [x] Keep canonical facts, skills, instructions and grants unchanged in M38. Proposed advice is not an enabled skill or verified Fact.
- Initial limits: two admitted reviews per workspace/UTC day, one physically active reviewer, ten pending records, 12KiB evidence brief, three advice items/6KiB output, one generation attempt, zero tools/retries and a 20-second returned-result deadline. These are execution/request bounds, not a hard provider billing cap; preserve reservations under uncertainty.
- Interfaces: a reusable no-tools inference/cancellation seam only where needed under `packages/*`; daemon learning records/quota/coordinator/routes under `hosts/rust-daemon`; additive SDK types and simple task-review/settings UI. The [M38 lean plan](../superpowers/plans/2026-10-13-companion-learning-m38.md) fixes the first wave.
- Acceptance: an eligible synthetic task yields anchored useful advice; repeated/concurrent manual/automatic requests make one attempt; foreign/failed/interrupted/helper/review sources refuse; malformed/oversized/tool-call output and cancelled/late results refuse; advice and quota survive reload without replay; memory/skill/instruction/primary-run snapshots stay unchanged.

### PAR-02: close the learning loop through reviewed adoption

- [x] M39 links a stored lesson to the existing SkillDraft create/update path; a separate exact own-memory handoff remains a follow-up. Do not build a second skill approval engine.
- [x] M39 shows edited skill bytes, sources and expected existing hash; changed sources/skill bases conflict. M40 adds exact full-source/hash/version Fact correction and owner selection.
- [x] M40 provides bounded owner-selected profile/agent-note references into canonical private Facts. Context is frozen per run and refreshed next turn; no mirrored text store. Optional standard Markdown exchange remains a distinct reviewed import/export, without reading personal Hermes folders.
- [x] M40 demonstrates the corrected selected name in two fresh generic/relevant replies across a quiet restart. This is preference-effect evidence: the second reply introduced the companion to the user, so that task's broader quality is not accepted. Later opted dialogue-only curation remains a separate case beyond M38's tool-using-task eligibility.
- [x] Approved skills remain hash-pinned and loadable through standard SKILL.md; imported references/scripts remain subject to capability grants, without automatically executing a package.
- [x] M39 proves one owner-adapted verification procedure on two fresh file tasks with actual skill loading, independent exact-byte/output checks and no copied conversation history. This is narrow procedural evidence, not broad learning quality.
- [ ] Define optional unattended Fact curation only as a later distinct policy/grant/budget task with semantic-quality acceptance. It is not M38 and cannot silently weaken existing skill/instruction approvals.
- Owners/files: native `skills/drafts.rs`/service and memory tools, existing owner routes, SDK and Skills/Memory UI. Preserve pending-draft limits, exact CAS, source scope and rollback/uncertain-durability behavior.
- Acceptance: stale/rejected/foreign/duplicate handoffs write nothing; approval saves exactly reviewed content; reload retains approval and sources; later tasks demonstrate actual reuse. A proposal-only pass does not complete this item.

### PAR-03: history as usable evidence

- [x] M41 adds discovery with stable session/message anchors and explicit bounded matching-message time/kind filters; ownership and hidden-source policy remain.
- [x] M41 adds `read_conversation_window` around one validated anchor, at most 20 messages/16KiB fully serialized UTF-8. Roles, anchors, pairing and shortening flags survive; attachments are flagged without their content.
- [x] M41 merges hot/cold history deterministically. Durable cold-pruned anchors remain readable; missing/foreign/hidden sources have generic typed absence, inconsistent stores return unavailable.
- [x] One fresh source-cited task amid time/kind distractors passes, followed by exact quiet restart and source/memory preservation. Three model steps, two tools, 5,420 reported tokens; broader recall quality remains open. See the [history guide](history.md).
- Owners/files: `tools/conversations.rs`, `sessions/views.rs`, history port/store only when needed, native tool registry; SDK/UI only for an exposed owner contract.
- Acceptance: discover and read an older declared conversation amid distractors in a fresh task; cross-agent/pruned/edge-window/cold-store/oversized/pairing cases pass. Lookup itself adds no model call. Quoted history cannot establish tool or memory mutation authority.

### PAR-04: MCP and community extension packages

M42 implements the first bounded anonymous HTTP tool path. See the [MCP guide](mcp.md) and [execution plan](../superpowers/plans/2026-10-13-companion-mcp-m42.md); fresh Rust2565/nine ignored/40 suites/lint and35 focused cases passed; exact publication is in its private ledger. M43 adds typed SDK/owner controls and exact alias settlement, with fresh Rust2574/nine ignored/40 suites/lint/build, SDK136/three service skips/build/typecheck and web1264/112 files/typecheck/build. Controlled browser and quiet restart acceptance made zero runtime provider/tool requests; see the [M43 plan](../superpowers/plans/2026-10-13-companion-mcp-console-m43.md).

M44 adds owned Windows stdio for an owner-selected trusted absolute executable and immutable argv, explicit disabled save/enable/start, bounded Job Object/pipe ownership and disconnected CP18 restore. Its Rust/core **2,592 tests / nine intentional ignores / 40 suites / lint/build**, **18 focused native cases including two real executor cases**, SDK **138 tests / three intentional service skips / build/typecheck** and web **1,269 tests / 112 files / typecheck/build/14 focused panel cases** passed. Actual built-console/SDK/Rust owner routing passed disabled save/enable with zero starts, explicit start/exact review/grant, quiet disconnected restart, reconnect/revoke/remove with the native grant preserved. Its two controlled peers recorded two initialize/two initialized/two lists/zero calls, with no model task/messages/tokens and all five exact owned processes gone. See the [M44 plan](../superpowers/plans/2026-10-14-companion-mcp-stdio-m44.md) and [harness evidence](harness-acceptance.md); PR50 is verified merged at `15e40c397a6ef961639e9375fe50ee513d09c9bf`. Non-Windows stdio is unavailable; HTTP remains portable.

M45 implements strict declarative MCP/inline-SKILL bundles, owner-bound preinstalled programs, exact review receipts, pending separate approval, unchanged-ID preservation, fresh disabled replacements/dormant predecessors and owned disable/remove trash. Fresh Rust/core **2,619 / nine intentional ignores / 40 suites / test/lint/build**, **16 M45 native cases including real executor / 11 pure-core / 187 compiled README operations**, SDK **145 / three intentional service skips / build/typecheck** and web **1,291 / 113 files / typecheck/build/18 bundle-panel cases** pass. Actual built-console/typed-SDK/owner-HTTP review/install, separate skill/MCP approvals, 1.1.0 update, quiet forced restart, disable and confirmed recoverable removal preserved native grants and an independent neighbor. Browser peer **1/1/1/0** initialize/initialized/list/call evidence is distinct from the native Ask/AllowOnce executor's exactly one call and disable's zero extra RPC. All six owned processes stopped; 33 frozen inputs and unrelated Nx hash stayed exact. The idle-scan 409 fix binds semantic state rather than scan attempts. See [bundle controls](community-bundles.md) and [harness evidence](harness-acceptance.md). Exact source, PR and verified merge settlement is recorded in the M45 ledger.

CP19 has an independent pre-community backup. Live uncertain 503 fails closed; immediate restart validates the complete snapshot actually persisted, so prior explicit approvals/grants may survive and final removal may already be absent. Remaining durable cleanup intentions stay unavailable, with no automatic replay/start/adoption/grant or universal restart-unavailable/rollback/atomic-filesystem claim. Retained-intent recovery remains limited. M65 adds only owner-managed static Bearer credentials for HTTPS Streamable HTTP; OAuth/refresh, arbitrary headers, selected real servers, non-Windows stdio, compiled contributions and broader quality remain open. M40's introduction-quality miss and full Hermes parity remain open.

- [x] M42–M44 scoped anonymous HTTP/Windows stdio connections, handshake, discovery, namespaced schemas, invocation, cancellation, disconnect and runtime state.
- [x] Bound schemas, arguments/results, time and calls; explicit reconnect, revision changes and revocation retain authority. Remote hints cannot classify or enable tools.
- [x] Pin descriptors and retain existing grants/approval/receipt authority at dispatch. Model output cannot configure commands, endpoints, credentials or permissions.
- [x] M45 versioned declarative manifest and reviewed MCP/skill install/update/disable/remove, with compatibility, permission requests, digests, attribution and tests.
- [x] One repository-local synthetic example and contribution instructions; scoped actual package lifecycle accepted. Out-of-process execution is not an OS sandbox.
- [x] M65 adds versioned owner save/replace/clear of one static Bearer credential for HTTPS Streamable HTTP, using a fresh endpoint-bound vault reference and secret-free `hasBearerCredential` projection. It does not add OAuth/refresh, arbitrary headers, remote-server validation or automatic connect. Native RMCP/local-TLS, persistence, bundle cleanup and SDK/web gates pass with synthetic credentials. Actual OS-vault/remote-server acceptance remains open.
- [ ] OAuth/refresh, selected real servers, compiled connector contributions and non-Windows stdio need separate scope/evidence; arbitrary writable native loading remains outside the initial design.
- Owners/interfaces: protocol-neutral ports under `packages/*`; daemon `mcp/` transport/lifecycle and trusted registry bridge; vault references; SDK/web connection/tool/package review. Choose current official libraries and license/dependency decisions in the detailed milestone, with dependencies allowed.
- Acceptance: fake server handshake/list/call plus schema collision/change, malformed/large input, grant denial, approval, revocation during a call, timeout/cancel, redaction and restore; discovery cannot dispatch by itself. Install/update/revoke the example through the actual package path. Real connections use concrete owner-selected configuration, without importing personal Hermes secrets.

### PAR-05: interactive browser tasks

M64 adds an offline-tested ignored public-page driver, but its independent source read did not establish the heading oracle. No DNS pin lookup or real public browser case was released; public rendering acceptance remains open. See [harness evidence](harness-acceptance.md).

M63 adds one operator-configured pinned public HTTPS static-page policy with normal TLS identity, exact GET routes, disabled page scripts and physically retained connection cleanup. Native/worker/web fixtures pass, including local TLS peers; no actual public site/browser/account is activated. See [browser setup](browser-work.md) and [harness evidence](harness-acceptance.md). Broader public destinations and dynamic/account workflows remain open.

- [x] M46 exposes a narrow reusable BrowserPort and reviewed host-owned worker for ordinary owner chat, with separate native grants, Exec approval and action receipts.
- [x] M46 supports navigation, bounded accessibility snapshots/screenshots, click/type/scroll and selected upload/download through a real DOM workflow.
- [x] M46 uses isolated contexts and operator-configured numeric-loopback routes, bounded requests/actions/assets/wire, guarded fetch and a deny-only fallback for unrouted HTTP. No profile/cookie import or OS egress-sandbox claim.
- [x] M46 proves Windows whole-job/IO cleanup, Scope Stop and ordinary Run Stop with exact owned process witnesses; page content supplies no grants. Contexts are unavailable after control-plane snapshot restoration.
- [ ] Public destinations, authenticated websites, other browser/host installations and autonomous provider browser-task quality still need their own scoped implementation and acceptance.
- Acceptance: deterministic controlled-page workflow and hostile redirects/page instructions, denied destinations, popups, oversized downloads, cancellation and clean context disposal; then a predeclared useful task whose output is checked. Existing read-only/public and isolated-test authority remains usable; personal-profile/external-write cases need concrete scope.

M46's [bound lean plan](../superpowers/plans/2026-10-14-companion-browser-m46.md) and [operator guide](browser-work.md) record the accepted Node22.12/Playwright-core1.59.1/paired headless-shell1217 installation. One independently checked75-byte supplier CSV, exact single POST map, JPEG,12 succeeded receipts, zero model calls, refused redirect and zero denied-sink requests pass. All five task process/image/kernel-time witnesses and removed private directories were independently verified;26 controlled processes were observed and none remained. This accepts the controlled Windows slice rather than full browser parity. The public Hermes comparison remains pinned to `1fd18277267152893a8a24792b79213e7ca3f899`.

### PAR-06: coding and delegation that finishes real work

M82 keeps legacy swarm member tasks under one existing logical owner across dispatch-time runtime resets. Host task scope and baseline agree; default companion identity, malformed-read refusal and CAS recovery remain. No old ephemeral file is adopted or migrated, and this does not establish process-level swarm restoration or model reasoning quality.

M80 closes a task-loss path after an initial read failure: model writes now require a successfully observed list revision. Refusal preserves exact malformed/current bytes without automatic reads, repair or retries. Explicit todo_read and merged writes recover through existing CAS; owner/internal saves retain their existing authority. Local deterministic task tests pass; this does not repair or accept model reasoning/reporting quality.

M52 now supplies actual useful repair evidence on one distinct JSONL task:
current-account `gpt-6-luna`, one helper, initial failing tests, model repair,
final tests3/3 and an independent exact129-byte module/CLI oracle pass. Seven
recorded calls/23,065 reported tokens,14 durable receipts and quiet restart7to7
were verified. Four canonical task-result/reflection records and two agent
entities retained their memory-file hash; no Fact/retrieval-quality claim.

Strict autonomous-quality acceptance remains partial: the helper's edge-case
explanation was contradictory, and the parent completed an output-verification
todo while reporting that it had not confirmed the file contents. Root verified
the artifact independently. No replay, follow-up generation or manual rescue is
allowed for this consumed case. Required gates passed Rust2725/23intentional-
ignores/40suites/format/build, nine oracle controls and the exact synthetic shell
case. All24 recorded owned identities retired/port18752free. See the
[M52 outcome](../superpowers/plans/2026-10-16-companion-provider-task-acceptance-m52.md).
Reporting/helper reasoning quality, arbitrary coding, broader accounts/hosts and
full Hermes parity remain open. Historical results below stay immutable.

M51 implements the bounded8MiB catalog-envelope correction with fresh required
Rust gates and unchanged inference/public contracts. The one separately released actual check **received and parsed current metadata** in **0.29s**: state **ready**, count **7**, stage **complete**, dispatch **response_received**, reason **none**, HTTP status **not retained**. No model IDs/raw body were retained or model chosen. No retry, fallback query/header/model/provider, generation, helper, daemon, connector, DB server or container followed. The release is consumed. Metadata receipt is not entitlement, tool compatibility or useful coding quality; M47/M49 historical causes are not reconstructed.
See the [M51 outcome](../superpowers/plans/2026-10-15-companion-catalog-budget-m51.md).

M50 adds closed private catalog diagnostics with fresh Rust2719/21intentional-ignores/
40suites/format/build and unchanged SDK/web contracts. Its single actual check
received HTTP200 but rejected the declared body above512KiB in0.84s; no catalog,
retry or generation. This identifies the next bounded catalog-budget fix for that
new request, without reconstructing M49/M47 history or accepting coding quality.
See the [M50 outcome](../superpowers/plans/2026-10-15-companion-catalog-diagnostics-m50.md).

M49 implements deliberate current-account subscription selection and preserves
saved/custom models. Its local gates passed Rust2709, SDK151 and web1320 with
required formatting/type checks/builds. The single metadata-only actual check
returned unavailable in1.45s, with no catalog, retry or generation; the cause is
unknown. M48's separate model-access rejection and M47's failed useful task remain
unchanged. A distinct JSONL acceptance fixture may be prepared, but current-account
selection and independently correct provider coding/delegation stay open. See the
[M49 outcome](../superpowers/plans/2026-10-15-companion-subscription-models-m49.md).

- [x] M52 repaired one small synthetic repository through actual read/edit/test tools, saved todos and one bounded helper; independent code/output oracle and protected-file/time checks passed. Helper edge-case and verification-reporting quality remain open.
- [x] M47 deterministic cases include correction, tool failure, budget exhaustion and Stop/recovery, with paired results and honest wrap-up; actual useful-provider recovery remains separate.
- [ ] Plan any external coding-worker adapter through the reviewed capability bridge. Evaluate helper terminal access only as a separate explicit grant/scope change; retain bounded workers and no self-granting/recursive runaway by default.
- [x] M52 declared parent/helper ten-attempt, one-helper/two-Exec, four-round/zero-retry/30s-call/180s-root boundaries before its one case; seven calls were recorded. Every future case still needs its own bound/release, with no repeated-provider pass hunting.
- Owners: native harness/helper adapters when an actual gap is demonstrated; otherwise isolated acceptance driver and docs. Reuse existing loop, task, shell and filesystem implementations.
- Acceptance: actual fixed tests/output, correct delegated result, limited calls, no unrelated edit, truthful unfinished work and recorded recovery. Full arbitrary coding quality is not inferred from one task.

M47 passed 2,672 Rust/core tests, lint/build, an independent oracle self-test,
actual scripted shell repair and six physical lifecycle cases. Its one real
subscription task failed HTTP 400 before tools: zero helpers/edits/output and no
recorded usage, with failed-call billing unknown. Exact quiet failed-run restart
passed without replay; no canonical memory existed in that fresh profile.
The unchanged buggy candidate still fails the oracle. See the [M47 plan](../superpowers/plans/2026-10-14-companion-coding-acceptance-m47.md).
M48 then passed 2,681 Rust/core tests, lint/build and exact synthetic 14-tool
manager-to-wire handoff, and ported the pinned Hermes model-access classifier
with MIT attribution. Its one distinct zero-tool same-model diagnostic was rejected
as **model_access**: gpt-5.5 is unsupported for this account. Failed-call consumption
is unknown; no retry/model fallback or M47 replay followed. This identifies the
new diagnostic's rejection, not M47's historical cause. See the [M48 plan](../superpowers/plans/2026-10-15-companion-provider-diagnostics-m48.md).
Account-aware subscription selection now precedes the held native-voice draft.
PAR-06 stays partial; deterministic wiring is not successful autonomous task evidence.

M86 one-case update ([plan](../superpowers/plans/2026-10-17-companion-coding-reliability-m86.md)): One distinct actual snapshot-comparison repair is FAILED: gpt-6-luna made3parent +2helper subscription calls,12542reportedtokens in116.4seconds. One helper completed three read-only inspections and returned grounded advice. The driver withheld the first Exec after seeing linked helper frames; zero shell commands/module writes/todo saves/output files. Owner cancelled once; five quiet same-store seconds preserved all observed state, both owned daemons retired and18786 was free. One actual task FAILED at its private helper-stream approval guard; coding repair, six tests, output read-back, completed saved progress and final reporting remain unassessed. The cancelled parent/helper history, empty tasks, unchanged files, usage/config/grants and memory survived five quiet same-store seconds without replay. This does not prove successful coding, in-flight recovery, indefinite uptime, browser interaction or full Hermes parity. No retry/fallback/report repair, normal profile/account/login/service/connector/database server/container/Docker55432 action or historical-case replay; settings/accounting are not hard billing caps.

### PAR-07: native voice

M56 supplies strict selected PCM16 WAV upload/download and one real opt-in OpenAI
Platform adapter to an editable same-scope draft. Limits, cancellation, retained
byte ownership, no replay and no automatic transcription/send are fixture-tested.
Synthetic silence/finite peers do not prove speech accuracy or live account
behavior. M57 adds local microphone-to-WAV capture, explicit Add, retained browser ownership and a built-worklet fixture oracle; fake-device tests do not establish real microphone compatibility. M58 adds selected saved-reply native speech with independent opt-in, manual playback and retained output/browser ownership; see [speech output](speech-output.md). Actual device/provider/voice-quality and connector/local/other provider acceptance remain open. See [voice scope](voice.md) and the [M56 plan](../superpowers/plans/2026-10-16-companion-native-voice-m56.md).

- [ ] Add provider-neutral transcription/speech ports and local/configured-provider adapters, visible microphone permission, transcript review and explicit playback.
- [ ] Support selected chat/connector voice attachments through the existing bounded attachment lifecycle; distinguish browser dictation from daemon-native transcription.
- [ ] Bound audio duration/bytes, inference/stream lifetime, output and retention; make cancellation, unsupported language/provider, noisy input and unavailable-device states clear.
- [ ] No always-on recording, hidden upload or automatic authority from speech. Prefer local options when they fit; selected remote inference must expose its cost and privacy consequence.
- Acceptance: synthetic audio and mocked provider/device tests, then consented capture/transcription/playback with a checked transcript and Stop/cleanup/retention proof. No microphone/account is activated by this roadmap.

### PAR-08: attachment understanding and document work

- [ ] Verify text and compatible-model image inputs reach the actual selected model and produce independently checked answers tied to the selected attachment.
- [ ] Extend document coverage and OCR as separate supported modes where needed. M54 DOCX main-body, M55 PDF text-layer and M66 bounded XLSX stored cells/hidden-sheet metadata are implemented with typed locators, honest coverage and original downloads. M66 uses synthetic parser/selection tests plus a finite Windows worker CLI case; formulas are not calculated. OCR, other Office, larger-workbook/visual fidelity and actual answer quality remain open. See [document coverage](documents.md).
- [ ] Isolate parsing; cap decompression/pages/sheets/text/tokens/CPU/memory/time. Do not execute macros or treat extracted instructions as permissions. Unsupported/corrupt/encrypted inputs explain the limitation.
- [ ] Only message-selected attachments enter context. Add checked export/artifact creation where a daily-use case calls for it; do not imply general Google Sheets/Slides support from local document parsing.
- Owners: daemon attachments/document worker, host-neutral content ports/model adapters when needed, SDK and composer/preview UI.
- Acceptance: generated fixtures, malformed/encrypted/large archives, wrong-owner access, cancellation, supported-model routing and source citations; actual text/image/PDF cases with declared content and no fabricated vision/extraction claim.

### PAR-09: the five selected connector workflows

- [ ] **Telegram:** pairing, one inbound task/reply, selected attachment if supported, disconnect/revocation and restart/uncertain-send handling.
- [ ] **Discord:** selected-channel text receive/reply and permission/recovery proof. Gateway events, voice, threads/slash commands remain separate optional scope rather than a breadth requirement.
- [ ] **Gmail:** selected mailbox read and reviewed draft, then separately authorized send if wanted; verify actual destination state and revocation. No automatic full-mailbox cleanup.
- [ ] **Calendar:** selected event read and exact reviewed create/update/delete in a test calendar, with observed external result and cleanup.
- [ ] **Drive:** owner-selected Docs/plaintext files, literal-name discovery and bounded reads; verify scopes versus local allowlist and revocation.
- Owners: existing native connector managers/runtime and owner UI; use fake transports first. Integration code and fixture tests already exist; implement only demonstrated gaps.
- Account acceptance is queued per named account/resource/destination and concrete action. Keep read, local draft and outbound/write evidence distinct; unknown delivery is not failure-free delivery and must not be automatically resent. Use synthetic owner-only test content, not customer data.

### PAR-10: recurring and unattended jobs

M79 fixes automatic catch-up admission outside owner active hours. An overdue occurrence waits unchanged until a permitted tick; existing cadence, manual Run now and malformed-record peer isolation remain. Controlled boundary/overnight/weekend/DST and scheduler tests pass. Already-claimed work and broad target-host/external-effect guarantees remain outside this fix.

M62 adds enabled unclaimed interval recovery after five elapsed periods, four explicit ticks [1,0,0,1] across two fresh JSON states and exactly two fake-model calls after all joins. The existing skip-missed policy runs once late, avoids a catch-up storm and resumes normal recurrence; IDs, counters and saved state agree. This is controlled-clock local acceptance, not actual cadence, daily/cron/DST, target-host uptime or external exactly-once proof. The broader checklist below stays open. See [harness evidence](harness-acceptance.md).

M59 adds a finite unresolved-claim JSON load into a new daemon and a second fresh reload. Startup disables the interrupted schedule, preserves its occurrence/next due and records a durable local interrupted outcome with zero mock-model calls. This is synthetic claim-recovery evidence; it does not complete the broader recurrence/outage checklist or prove external exactly-once delivery. See [harness evidence](harness-acceptance.md).

- [ ] Test recurring interval/daily/cron/time-zone and DST/clock-change behavior using controlled clocks; define skipped/missed-occurrence behavior and avoid catch-up storms.
- [ ] Test long outage, before/after durable claim, in-flight shutdown/restart, cancelled/disabled jobs and uncertain effects without automatic replay.
- [ ] Run one finite isolated recurrence case with fixed occurrence/model-call ceilings, checked file bytes/history/receipts and one restart. Disable it when the observation ends.
- Owners: existing native scheduler/ledger/receipt tests; source fixes only for observed contract gaps. A browser closing is not a scheduler stop; the daemon must run.
- Acceptance: declared due times/counts, no duplicate claimed effect observed, manual review for uncertainty, cleanup and no overnight paid loop. Do not infer exactly-once external delivery.

### PAR-11: always-on operation

- [ ] Prepare the single-owner release/config/TLS/private-ingress/service setup and upgrade/rollback instructions against actual current paths.
- [ ] On an approved target, verify authenticated access, SSE/streaming, persistent sessions/memory/uploads/skills/tasks, SIGTERM drain and reboot.
- [ ] Produce an encrypted backup, restore onto an isolated target and compare declared IDs/content/effects/permissions; retain a rollback generation and test failed upgrade.
- [ ] Define health/failure reporting and what an operator must review after interruption; no uptime claim from unit tests or instructions.
- Owners: `deploy/vps` and runtime shutdown/persistence only for demonstrated gaps; operator evidence identifies exact host/revision.
- Public deployment, real containers/database servers and normal-profile credentials remain outside current activation authority. Code/package preparation and offline fixtures continue; actual activation waits for concrete target approval.

### PAR-12: a usable owner console

M81 connects both Tasks entry points to current online state. Offline reads/writes stop, late results stay obsolete, reconnect checks the saved revision and same-agent drafts remain read-only until safe. Changed revisions require explicit discard; submitted saves are not retried. Full affected web gates pass; broader lifecycle/account/host acceptance remains open.

M78 prevents stale Memory rows, evidence and action errors across reconnect/companion/epoch changes. Submitted mutations run once without automatic retry; old completions cannot trigger offline reads. Current-context partial data and error channels remain usable. Full affected web gates pass; memory ranking and target-host acceptance remain separate.

M76 adds read-only review guidance for recorded interrupted automation runs, with exact saved-thread navigation and fresh agent/schedule/version/offline history. Opening review never runs or resumes work; existing owner execution controls remain separate. Required web gates pass; broader recurrence and target-host acceptance remain open.

M75 corrects the existing capability inventory and console: voice, bounded PDF/DOCX/XLSX reads and reviewed MCP/skill bundles show partial implementation with setup requirements. Browser/document tools are grouped and searchable. Planned and unknown capabilities remain explicit; this read-only inventory grants nothing and checks no provider health. Required local Rust, SDK and web gates passed. Broader lifecycle integration and live acceptance remain open.

M61 locally accepts built-page memory edit/status/search/delete through actual Rust HTTP across one quiet forced restart with fresh synthetic stores. Shell/provider/account APIs stay mocked; ordinary profiles, actual semantic quality and broader lifecycle acceptance remain open. See [harness evidence](harness-acceptance.md).

M60 adds an explicit Memory-page index snapshot through the existing readiness endpoint, with daemon-wide counts, disk cache configuration and separate fixed local fixture results. It makes no semantic-quality/provider-health/durability claim and does not trigger inference or writes. Other capability controls stay in their owning pages; broader lifecycle integration remains open.

- [ ] Integrate controls with each feature: off/not configured/enabled/needs review/degraded/unavailable, meaningful reason, effective grants and active work.
- [ ] Add Learning advice review/adoption, MCP/package lifecycle, browser/media controls and connector health where the underlying behavior exists.
- [ ] Task modes select approved capabilities; they cannot grant tools, connect accounts or bypass approvals. Preserve unrelated settings and custom grants.
- [ ] Keep labels nontechnical and distinguish configured, running and verified. Show unsupported features honestly; do not manufacture “ready” from a saved descriptor.
- Acceptance: state transitions/failed saves/revocation/cancel/reload, keyboard/mobile access and SDK compatibility; every displayed action corresponds to real dispatcher/state behavior.

## Execution waves and delegation

1. **M37 now:** publish this locked inventory, master status and handover, plus the first lean M38 plan. Documentation completion is not feature completion.
2. **Learning wave:** PAR-01/M38; PAR-02 reviewed adoption/reuse; PAR-03 history. First deliver a closed bounded learning workflow, then richer evidence retrieval.
3. **Tool wave:** PAR-04 MCP/community packaging; PAR-05 browser; PAR-06 larger coding/delegation acceptance. Detailed planning can run in parallel; shared runtime implementation remains sequenced.
4. **Media wave:** PAR-08 text/image/document pipeline and PAR-07 voice as separate useful milestones. SDK/web work runs beside native work only after interfaces freeze and files are disjoint.
5. **Reliability wave:** PAR-09 selected connector cases one at a time, PAR-10 recurrence/outage, PAR-11 target-host operation. Preparation and mock tests need not wait for account/deployment approval.
6. **PAR-12 spans the waves** as the corresponding capability ships. Final scoped parity audit checks every work item's required evidence and unresolved limitations; no percentage score.

The controller owns milestone order, branch, contracts, risk rulings, integration, gates, ledger and publication. Standard Codex workers own bounded disjoint native/core and SDK/web tasks; a concurrency/security-specialist reviewer is reserved for subtle MCP/worker/cancellation/authority changes. Current team drafts covered harness/learning and interfaces/integrations; controller covered operational acceptance. No separate user-owned chats were created.

Use one branch per implementation milestone from verified merged main. This shared checkout has no worktrees, so different milestones' code does not run concurrently. Within a milestone: meaningful RED, frozen interfaces, disjoint implementation workers, one native batch review, final web/SDK review, one justified fix wave and current relevant gates. Every Cargo command has CARGO_INCREMENTAL=0 and is serialized to actual exit. SDK gates use ANIMA_SKIP_SERVICE_TESTS=1. Stage explicit owned paths; never nx.json/anima.yaml/private evidence. Dependencies are allowed with a recorded reason; reuse existing where adequate. Ports/accounts/secrets/containers and public actions retain their existing authorization boundaries.

## Controller rulings on the risks (binding)

1. Ruling: track all twelve work IDs now and implement in source-compatible waves — shared runtime/SDK/persistence changes cannot be safely scattered across competing branches in one checkout — cost if wrong: some implementation is sequential; no feature disappears from the backlog.
2. Ruling: M38 is bounded advice only and PAR-02 proves explicit adoption/reuse; optional unattended curation is a distinct opted policy task — proposals alone are not full learning and owner controls must not be silently bypassed — cost if wrong: full learning remains open until reuse and its applicable policy acceptance pass.
3. Ruling: preserve controller-owned grants, cancellation, receipts, source scope and budgets across dynamic tools/media/learning — remote descriptions, generated advice and document/browser content are untrusted — cost if wrong: refuse a call/proposal rather than silently expand authority.
4. Ruling: distinguish implementation, local, account and host acceptance per work item — code/configuration and synthetic tests cannot prove delivery, semantic quality or uptime — cost if wrong: scoped parity remains partial; no fabricated completion label.
5. Ruling: carry already-authorized source work, read-only checks and isolated tests forward; request only still-required concrete account/write/profile/hosting activation — user asked continuous progress and has allowed dependencies — cost if wrong: those live evidence steps wait, while code/mock preparation continues. Retain upstream notices for actual ports and Anima product naming.

## Change and completion record

Each completed subtask records source/revision, actual checks, observed outputs/effects, owner of the next step, material limits and the next resume path in `.superpowers/sdd/<milestone>/progress.md`. Use `Ruling: what — why — cost if wrong` for changes in scope/design. Changes to this inventory need a reason and evidence, not another broad feature brainstorm. Refresh the detailed plan at milestone start; do not hardcode future dependency versions or fake readiness dates.

M67 adds explicitly granted `read_document` for existing workspace PDF/DOCX/XLSX
files, reusing M54–M66 extraction and owned worker with source path/hash snapshots
and bounded ordinary tool results. An owner Settings checkbox preserves defaults
and other grants; existing helper/connector filesystem authority applies. Local
recording-model/history tests establish plumbing, not independent answer quality.
See [document use and limits](documents.md).

M68 closes the selected report-delivery gap with an explicit owner Files download
for current bounded text/Markdown/CSV bytes, reusing the existing safe reader,
retained leases and authenticated SDK. A local synthetic write/list/HTTP oracle
and SDK/web tests establish exact byte delivery, cancellation and URL retirement;
no actual browser save, general binary artifact, chat output button, report-quality
or full Hermes parity claim. See [report use](documents.md#download-a-generated-report).

M69 actual built-web report-save acceptance remains **PARTIAL**: offline policy/console/typecheck pass, but the one isolated Edge launch failed before navigation. All 14 captured identities retired and the source fixture is unchanged; no browser-save success is claimed. M70 will repair that launch boundary using a fresh separately bound case while preserving the failed case and byte oracle.

M70 profile repair remains **PARTIAL**: profile18/policy47/console65pass3liveskips/typecheck pass, but the actual case failed during process capture before any browser-result witness. All14 known identities retired/portsfree/profilewitness stable; final descendant capture failed and complete cleanup is unaccepted. No browser-save or profile-repair success is inferred; a new guard repair needs deterministic regressions before further live acceptance. M69 remains failed and unchanged.

M71 proves the narrowed retired-error31 guard behavior through18deterministic tests and one actual complete retained-Job termination/drain to zero. Actual browser report save remains **PARTIAL**: error87 at opening a discovered member stopped the one case before a browser witness; no handle/image/creation-time or individual retirement is inferred for that PID. A narrow discovery-gap repair needs deterministic tests before any fresh case. All original failed cases remain immutable.

M72: Guard 28, profile 18, policy 47 and offline console gates/typecheck passed. One ownership review, one final E2E review, one combined fix wave and mechanical case reuse are recorded. Fresh case e73821b6-dad1-41c7-96d3-33fe497f76b0 completed in 17.312s: browser save PARTIAL, 1 saved reports, 20 known identities retired, 0 discovery gaps covered by zero-error Job drain, active count zero before close, ports free, stable private profile and exact 84-byte source preserved. The browser passed the three ordered real Files GETs, explicit click, one download event, filename/blob URL and exact saved/source byte checks, then failed the single-context-page assertion at spec line414: a second page was present. Its cause is unknown. The full browser case remains PARTIAL; it was not retried or regraded. No discovery gap occurred in this actual case; error87 coverage is deterministic unit evidence. This validates selected synthetic Files/browser behavior and process ownership only; it does not establish full agent-loop or live-provider parity.

M73: Offline paired-runtime admission10 passed; Guard 28, profile 18, policy 47 and offline console gates/typecheck passed. One ownership review, one final E2E review, one combined fix wave and mechanical case reuse are recorded. Fresh case 615aa155-ad14-4a3c-8217-e82f28afd7b8 completed in 13.063s: browser save PARTIAL, 1 saved reports, 11 known identities retired, 0 discovery gaps covered by zero-error Job drain, active count zero before close, ports free, stable private profile and exact 84-byte source preserved. Exact browser failure logs are sealed; no retry or regrade. The installed engine1.59.1/headless-shell1217 was used with the original single-page, route/save and cleanup assertions. Old M72 remains PARTIAL; no cause is inferred for its second page. The browser test/result itself passed all strict checks, including single-page and post-context closure; overall supervisor acceptance remains PARTIAL. Its exact failure was process-image error5/PID26340 with creation134366078117796641 and initial wait258, before retaining that identity; whether the process retired during image query is unknown. Final known11 identities retired with active0/errors0. Next use deterministic tests for a same-retained-handle post-error wait; never assume an active inaccessible child is retired or weaken primary-image admission. This validates selected synthetic Files/browser behavior and process ownership only; it does not establish full agent-loop or live-provider parity.

M74: Offline paired-runtime admission10 passed; Guard 34, profile 18, policy 47 and offline console gates/typecheck passed. One ownership review, one final E2E review, one combined fix wave and mechanical case reuse are recorded. Fresh case 2de6769f-ecbe-4969-862c-d004469a5dcf completed in 14.391s: browser save ACCEPTED, 1 saved reports, 11 known identities retired, 0 discovery gaps covered by zero-error Job drain, active count zero before close, ports free, stable private profile and exact 84-byte source preserved. The installed engine1.59.1/headless-shell1217 was used with the original single-page, route/save and cleanup assertions. Old M72 remains PARTIAL; no cause is inferred for its second page. M73 browser flow passed but its overall supervisor case remains historical PARTIAL; this new case never regrades it. Descendant image5/31 now requires a confirmed signaled observation on the same retained handle; still-active, unknown-error and primary-image paths remain strict. This validates selected synthetic Files/browser behavior and process ownership only; it does not establish full agent-loop or live-provider parity.

M77: M77 adds a separate ignored Windows public-page driver and offline contract/oracle controls. Fresh Rust 2949 passed/29 ignored across 46 suites, lint/build/format and 21 supervisor tests pass. One native batch review, one final supervisor review and one combined fix wave are recorded. The single case f9e26a1d-c9a5-4dbf-a98d-57bf9ca469fd is ACCEPTED. It matched the exact H1 and whole normalized 3568-byte paragraph digest through ordinary BrowserPort approvals, two actions/one request, zero model calls, reported shortening=True and omittedNodes=2, and retired 8 known processes with Job active0/errors0. This is selected read-only public static-page content and process-ownership evidence. It does not establish arbitrary sites, complete DOM coverage, model reasoning, account/provider readiness or full Hermes parity. M64 remains NOT RUN, M52 remains PARTIAL and all earlier cases keep their original outcomes. No production transport, web/SDK, dependency, account, provider, ordinary service, database or container change.
