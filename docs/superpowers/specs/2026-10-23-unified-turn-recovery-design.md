# Unified turn recovery, crash acceptance and trace evaluation

User approved all three recommendations plus durable secondary-call usage on 2026-10-23. Stay on main; no staging, commits or pushes. Preserve owner approval, tool grants, stop and exact Undo.

## Design

Extend the ordinary Rust AgentRuntime/coordinator path with a shared portable turn-recovery contract. Reuse the host accepted-run transaction, stable input/idempotency identity, action receipts, continuation lineage, snapshots, final transcript commit and existing connector delivery receipts. Avoid introducing a second automatic execution engine alongside those controls. The core durable engine remains reusable; shared turn recovery values/policy must be host agnostic.

A turn keeps one root identity across attempts and binds agent/session/original input to saved progress and terminal reply identity. Save a bounded checkpoint at actual model/tool boundaries before publishing progress or dispatching another effect. Saved progress is untrusted recovery data, never new owner authority. Bind it to current workspace/configuration/input and monotonically increasing sequence. Save failure blocks further model/tool dispatch. Unknown or in-flight effects remain unknown across restart. Final commit closes recovery ownership; completed turns are never rerun.

On restart, recovery has an explicit classification: safely recoverable accepted input, owner review required, already completed, stopped or invalid/stale. Only demonstrably never-started ordinary owner chat input may auto-continue, once per source and within the existing bounded attempt budget; no scheduled/system/helper/browser/channel authority may be manufactured. Started work remains owner-reviewed and its continuation includes saved progress/receipts without replaying completed or uncertain tool invocations. Recheck current permissions and original agent/session before admission. Repeated startup/reconnect or concurrent recovery uses one durable continuation claim and cannot consume the same input twice. Explicit owner stop and deletion stay authoritative.

Do not claim exact instruction-pointer restoration when recovery is a fresh continuation. Expose the root identity/checkpoint/recovery state through existing owner run/recovery contracts and thin SDK types as needed. Keep partial streamed tokens separate from committed final transcript and delivered-channel evidence.

## Secondary usage

Move titles, compaction, profile/agency/evaluation and other secondary-call usage from crash-vulnerable queue-only custody to a bounded durable pending outbox. Enqueue must persist before reporting durable acceptance; history-store acknowledgement removes exactly those persisted IDs only after idempotent upsert. Restart reloads pending usage; failures and capacity saturation are visible and never silently drop old rows. Usage survives run/session deletions. Serialize snapshots with existing control-plane transaction and preserve migration backups/newer-version refusal. Ephemeral mode must remain explicitly ephemeral.

## Crash acceptance

Add reproducible integration acceptance using real daemon child processes and isolated state/workspaces, deterministic local model/transport fixtures, exact process ownership and bounded timeouts. Force process termination at observed acceptance/approval/tool/compaction/delivery or equivalent authoritative barriers, restart the same profile, then assert recovered input custody, no duplicate effect, no automatic unknown-effect replay, unchanged approval/Undo, exact committed history and pending usage recovery. No owner profile, external send or credential is used. Reuse existing lifecycle/process helpers where useful. Report synthetic process evidence separately from live-provider results.

## Trace evaluation

Create reusable Rust trace grading and a broader fixture battery for ordinary tasks, using independent filesystem/results oracles and captured model/tool events rather than model self-report. Compare baseline/candidate under matched settings and repetitions; report correctness, model/tool call counts, errors/repeats, output bytes and available token usage/latency. Include adversarial failure classes such as stale read/edit, ambiguous match, large output, repeated failure and unrelated successful check. Scoring never grants promotion/approval. Support fresh-session skill reuse where applicable and distinguish missing/unknown usage from zero. Add deterministic oracle self-tests and a bounded live run when an available provider is usable.

## Verification

Test-first meaningful regressions; independent spec and quality review. Root serializes Cargo/Nx jobs. Required rust-daemon:test --skipNxCache, rust-daemon:lint, daemon build, packaged supervisor checks when affected, relevant SDK/client tests/build/typecheck and docs inventory checks. Preserve evidence under ignored .tmp/unified-turn-recovery-2026-10-23. Final branch/index/unstaged manifest audit; no commits.
