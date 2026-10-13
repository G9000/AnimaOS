# Native harness acceptance

The selected local assistant loop is usable: real tasks have exercised workspace
work, memory correction, helpers, saved tasks, approved skills, compaction and
restart recovery. This is evidence for those cases, rather than a claim of
complete Hermes/OpenClaw parity or production uptime.

On 2026-10-12, M28 checked the latest merged native implementation at
`304184da765875b90a37104272558ac34f12ebc9` with one configured gpt-5.5 task.
It searched its own synthetic memory, updated exactly one existing Fact with
current-text matching, wrote the exact 18-byte workspace file and returned an
honest tool-free budget wrap-up. The neighboring Fact remained unchanged.
Four model steps reported 12,595 tokens; all three action receipts were persistent.

A quiet same-store restart retained that run, all eight original messages,
accepted wrap-up marker, action receipts, memory and file bytes/hash/modification
time. No replay was observed. The temporary three-round, 20-second, zero-retry
settings were restored; all recorded test processes were stopped and ports
8080/4200 were free. Testing used synthetic data and the normal internal model
connection, without messaging accounts, database servers or containers.

## What has evidence

Historical live cases establish specific behavior at their tested revisions.
Deterministic tests establish native contracts, rather than model adherence or
general task quality. The M28 ordinary flow ran on its recorded merged revision; it did
not deliberately trigger every recovery branch.

| Capability                  | Evidence                                                                                                                                                                                                                                                                                                                                                                                                              | Limit                                                                                                |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Chat and multi-step work    | M17 real report with independently checked arithmetic; M28 sequential memory/file work                                                                                                                                                                                                                                                                                                                                | Small synthetic tasks, rather than broad workload quality                                            |
| Memory                      | M17 exact correction; M18 corrected recall after 20 newer reflections and eight foreign matches; M20 deterministic eligibility/visibility fixes; M28 exact edit survives restart; M30 synthetic exact own-Fact removal with explicit web grant, persistence/indices/citations/neighbor and stale/privacy checks; M31 reviewed owner deletion; M32 native vector pre-top-k eligibility and eligible-only normalization | General semantic quality, automatic consolidation and latest-text/vector revision agreement unproven |
| Tasks and helpers           | M17 three saved completed report tasks and one read-only helper with matching arithmetic                                                                                                                                                                                                                                                                                                                              | Broad delegation quality unproven                                                                    |
| Skills                      | M17 approved skill loaded and reused against the report                                                                                                                                                                                                                                                                                                                                                               | Autonomous skill maintenance unproven                                                                |
| Compaction                  | M17 real automatic compaction and saved summary under a temporary context budget                                                                                                                                                                                                                                                                                                                                      | Arbitrary-history summary quality unproven                                                           |
| Schedules                   | M19 restored before-due one-shot; M22 overdue unclaimed one-shot runs after startup                                                                                                                                                                                                                                                                                                                                   | Recurrence, long outages and clock changes unproven                                                  |
| Interrupted effects         | M21 crashed scheduled write with native action record survives two restarts without replay; M17 owner-directed continuation preserves checkpoint                                                                                                                                                                                                                                                                      | Unknown effects need inspection; no exactly-once external guarantee                                  |
| Budget wrap-up              | M23 actual completed read/unfinished write; M28 completed three actions and accepted final tool-free reply                                                                                                                                                                                                                                                                                                            | No hard token/billing ceiling or guaranteed reply quality                                            |
| Unconfigured request repair | M25 scripted single paired refusal and correction; malformed/mixed/second denials remain terminal; neutral web request summaries                                                                                                                                                                                                                                                                                      | Live model correction behavior unproven                                                              |
| Transport recovery          | M26 mock HTTP: one classified pre-stream retry, unchanged authority/request, cancellation and explicit zero; no ambiguous or partial-stream replay                                                                                                                                                                                                                                                                    | Live outage handling and failed-attempt usage unproven                                               |
| Repeated results            | M24 exact-failure advice; M27 repeated-success advice; M29 bounded periods 2–4 cycle advice; original results, polling and budget/Stop preserved                                                                                                                                                                                                                                                                      | No inferred progress, effect safety, cycles beyond the bounded policy or live usefulness claim       |

M30 on 2026-10-13 passed eight grouped native memory-removal cases and five new
web permission cases. Removal is from active saved Fact records; old chats,
reflections, historical derived knowledge, receipts and backups may retain text.
No real memories or model removal task were used. M29 passed seven new
deterministic cycle cases. It added no real
provider task; M28 remains the latest ordinary live acceptance. M30 native source passed **2,419 Rust tests / eight ignored**, plus formatting.
M30's web surface passed **1,197 tests**, type checks and build, including the opt-in permission regressions. Its transitive SDK type/build checks also passed. The unchanged SDK's **130 unit tests** remain M25 historical evidence. These are local checks; hosted CI's known Nx Cloud failure is not a
substitute for their evidence.

## Upstream comparison and remaining work

The loop adaptations use specific public Hermes helpers at pinned revision
`1fd18277267152893a8a24792b79213e7ca3f899`. Anima keeps a native Rust runtime;
it does not import the Python runtime, private profiles or credentials. Full MIT
attribution remains with the adapted core and adapter code. The comparison is
limited to those public helpers, rather than the entire current upstream product.

M31 extends the same pinned-entry intent to reviewed owner deletion. The console
pins the click-time record, and the daemon compares full text under its memory
write lock. Empty-body legacy API deletion stays available; text equality does
not detect content ABA or metadata changes. Fresh M31 native source passed **2,423 Rust tests / eight ignored / 40 suites** and
formatting; web **1,201** tests and SDK **128** unit tests, type checks and builds
passed. Three SDK service tests were explicitly skipped. Four new native, four
web and one SDK case cover strict parsing, locked stale comparison, legacy
records, persistence and explicit re-review. One native batch/final SDKweb review
found no defects. No real record, service or model task was used. Failed initial
SDK integration-build setup and its corrected safe rerun remain in the ledger.

M32 proves one native retrieval correction using the existing local index:
`release briefing style` recalls the own `ship notes` Fact despite stronger
foreign/hidden entries. Canonical-missing IDs and current option/visibility
mismatches are excluded before native top-k; rejected scores cannot suppress
eligible normalization. Five core/four native cases and **2,432 Rust tests /
eight ignored / 40 suites**, plus sequential formatting, pass. Source hashes are
unchanged. SDK/web source and envelopes are unchanged; their M31 gates remain
historical. One README review correction separates filtered zero-limit behavior
from direct legacy portable search. No model/provider/service/account or real
memory was used. Fixed local synonym groups are not general semantic quality;
legacy external indexes can filter only their existing bounded shortlist.
M32 did not change automatic context or implement consolidation.

M33 adds configured semantic recall to automatic context. Existing lexical
matches retain their order; eligible positive vector matches fill spare slots.
The real multilingual E5 test predeclares English and Spanish paraphrases and
requires the intended Fact to rank above eight unrelated own distractors before
checking actual provider output. Both queries now include its exact ID and
quoted full text; stronger foreign and hidden records remain excluded and
canonical memory is unchanged. Two process-wide worker permits and a five-second
result deadline bound lookup, with lexical fallback and discarded late results.
The backend remains operator-selected: default local aliases do not establish
learned quality. These cases prove learned ranking and context inclusion, not
universal semantic quality, live model use of that evidence or remote-provider
latency. Consolidation is the next separate milestone.

The next useful step is personal use through the [daily-use setup](daily-use.md).
Real Telegram/Discord/Google account delivery, microphone/vision, browser
acceptance, broader semantic quality and hosted reboot/TLS/off-site restore remain separate
unverified work. Community connectors are compiled reviewed source, without a
dynamic installer or sandbox claim. Extra integration breadth is not required
to use the tested local loop.

The [M28 plan](../superpowers/plans/2026-10-12-companion-agent-loop-m28.md)
records the acceptance scope and exact run/session. Its private ledger retains
raw evidence, controller corrections, publication and resume details.

M33 final local gates passed **2,441 Rust tests / nine ignored / 40 suites**,
sequential formatting and the explicit real E5 acceptance test. Eight reviewed
source hashes remained unchanged. The single review fix wave removed inference
lock retention and excluded selected lexical duplicates before vector top-k.
SDK/web contracts and source are unchanged; their earlier gates remain historical.
