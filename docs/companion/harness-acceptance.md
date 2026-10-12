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
general task quality. The M28 ordinary flow ran on the latest merged code; it did
not deliberately trigger every recovery branch.

| Capability                  | Evidence                                                                                                                                                                         | Limit                                                                                          |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Chat and multi-step work    | M17 real report with independently checked arithmetic; M28 sequential memory/file work                                                                                           | Small synthetic tasks, rather than broad workload quality                                      |
| Memory                      | M17 exact correction; M18 corrected recall after 20 newer reflections and eight foreign matches; M20 deterministic eligibility/visibility fixes; M28 exact edit survives restart | Lexical retrieval; semantic/vector quality and consolidation unproven                          |
| Tasks and helpers           | M17 three saved completed report tasks and one read-only helper with matching arithmetic                                                                                         | Broad delegation quality unproven                                                              |
| Skills                      | M17 approved skill loaded and reused against the report                                                                                                                          | Autonomous skill maintenance unproven                                                          |
| Compaction                  | M17 real automatic compaction and saved summary under a temporary context budget                                                                                                 | Arbitrary-history summary quality unproven                                                     |
| Schedules                   | M19 restored before-due one-shot; M22 overdue unclaimed one-shot runs after startup                                                                                              | Recurrence, long outages and clock changes unproven                                            |
| Interrupted effects         | M21 crashed scheduled write with native action record survives two restarts without replay; M17 owner-directed continuation preserves checkpoint                                 | Unknown effects need inspection; no exactly-once external guarantee                            |
| Budget wrap-up              | M23 actual completed read/unfinished write; M28 completed three actions and accepted final tool-free reply                                                                       | No hard token/billing ceiling or guaranteed reply quality                                      |
| Unconfigured request repair | M25 scripted single paired refusal and correction; malformed/mixed/second denials remain terminal; neutral web request summaries                                                 | Live model correction behavior unproven                                                        |
| Transport recovery          | M26 mock HTTP: one classified pre-stream retry, unchanged authority/request, cancellation and explicit zero; no ambiguous or partial-stream replay                               | Live outage handling and failed-attempt usage unproven                                         |
| Repeated results            | M24 exact-failure advice; M27 repeated-success advice; M29 bounded periods 2–4 cycle advice; original results, polling and budget/Stop preserved                                 | No inferred progress, effect safety, cycles beyond the bounded policy or live usefulness claim |

M29 on 2026-10-13 passed seven new deterministic cycle cases. It added no real
provider task; M28 remains the latest ordinary live acceptance. Current native source passed **2,411 Rust tests / eight ignored**, plus formatting.
M25's unchanged web/SDK surfaces passed **1,192 web / 130 SDK tests**, type checks
and builds. These are local checks; hosted CI's known Nx Cloud failure is not a
substitute for their evidence.

## Upstream comparison and remaining work

The loop adaptations use specific public Hermes helpers at pinned revision
`1fd18277267152893a8a24792b79213e7ca3f899`. Anima keeps a native Rust runtime;
it does not import the Python runtime, private profiles or credentials. Full MIT
attribution remains with the adapted core and adapter code. The comparison is
limited to those public helpers, rather than the entire current upstream product.

The next useful step is personal use through the [daily-use setup](daily-use.md).
Real Telegram/Discord/Google account delivery, microphone/vision, browser
acceptance, semantic memory and hosted reboot/TLS/off-site restore remain separate
unverified work. Community connectors are compiled reviewed source, without a
dynamic installer or sandbox claim. Extra integration breadth is not required
to use the tested local loop.

The [M28 plan](../superpowers/plans/2026-10-12-companion-agent-loop-m28.md)
records the acceptance scope and exact run/session. Its private ledger retains
raw evidence, controller corrections, publication and resume details.
