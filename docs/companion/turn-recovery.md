# Turn recovery and trace evaluation

Ordinary Rust chat turns now retain a host-bound turn identity and bounded progress checkpoints. The same coordinator still owns run admission, tool grants, approvals, Stop, final transcript commit and reviewed continuation. Progress is recovery data; it never grants permission.

## What is saved

A turn binding identifies the root run, current attempt, agent/session, original and current input digests, and the admitted workspace/configuration. A bounded original input remains available for later reviewed attempts even after old run history is pruned. The runtime awaits checkpoint custody before model dispatch, after a complete model response before tool dispatch, and after a settled tool batch before the next model request. Model tool intentions retain identity and argument digests without becoming authority. Save failure prevents further dispatch; native action receipts remain the evidence for actual tool outcomes. Partial streamed text remains provisional.

Final transcript commit records the reply identity/digest in the turn's terminal projection. Checkpoint progress and final identity are separate: a checkpoint is not proof that the reply was delivered to a device or that a business result is correct.

An active run keeps the prompt and configuration it started with, including across compaction. Native tools still check current grants, workspace ownership and Stop. New admission and restart recovery require the saved binding to match the current configuration.

## Restart and owner recovery

Only a newly bound, persistent, ordinary owner chat input that demonstrably never started can be adopted automatically after restart. Its input, agent/session, workspace and configuration must still match. Legacy rows, synthesized steers, browser-selected turns, schedules, helpers, channel-origin turns, stopped work, and started/uncertain actions do not gain automatic authority. Existing per-session queues and the source's single durable continuation claim serialize adoption. Automatic adoption is allowed once for the verified original input; another crash before that linked attempt starts requires owner review within the same total attempt budget. Repeated startup or request replay returns the same saved claim; completed turns are not rerun.

Started work continues through the existing owner-reviewed recovery API. A continuation is a new bounded attempt using current permissions and quoted saved input, progress, clarification answers and receipts. It does not restore a process, reuse an old approval, or claim exact instruction-pointer resume. Inspect uncertain sends and other external effects before authorizing more work. Existing three-attempt recovery bounds and exact skill/file Undo controls remain in place.

Owner run responses expose optional turn root/input/checkpoint/final-identity summaries. Recovery responses expose root/checkpoint and recovery disposition. Old records remain readable. A legacy run with evidence that execution already began cannot gain a fresh execution allowance through recovery; a verified never-started input can receive a durable binding. The SDK mirrors the optional recovery fields.

## Durable secondary usage

Known usage from titles, compaction, side questions, profile/agency calls and evaluation joins a bounded pending outbox in the control-plane snapshot. Accepted IDs are immutable; capacity rejects a batch instead of evicting accepted rows. Persistent enqueue reports acceptance only after its save; ephemeral mode reports memory custody explicitly. History upsert is idempotent, and exact-ID acknowledgement is saved only after that upsert. Save or acknowledgement failure retains or restores pending rows for retry. Usage survives conversation deletion.

Snapshot format 28 retains shared execution accounting. Before a version-27 JSON snapshot is upgraded, its exact bytes are retained with suffix .pre-execution-budgets.bak. The version-26 turn upgrade retains .pre-turn-recovery.bak; older migration backup rules and newer-version refusal remain. PostgreSQL snapshots use the existing per-version backup key.

Remote provider work whose response/usage was never observed remains unknown after a crash. Durable known usage does not establish complete billing for an interrupted remote request.

## Profile ownership and shared limits

A persistent host holds lifetime ownership of its profile and writable stores. A second cooperating host is refused before it writes, including aliases and overlapping directory resources. Ownership loss stops new model/tool dispatch and approval consumption. Started effects retain custody until their outcome is recorded. Live managers retain ownership while their work is active; weak registrations in daemon state prevent reference cycles from keeping a retired profile locked. PostgreSQL writes use the fenced connection holding the ownership lock. These guarantees require cooperating versions of the host; an older executable that ignores the protocol is outside that guarantee.

Each admitted root turn has one durable execution scope. Helpers, retries, fallbacks, compaction, side questions and source-linked learning calls share its consumed allowance. Reviewed recovery keeps that scope and deadline. Independent background operations receive their own bounded scope. A swarm shares one scope tightened to the strictest configured manager or worker ceiling. Model reservations are saved before a physical attempt; an interrupted attempt stays conservatively charged when its actual usage is unknown. Complete observed usage can settle after Stop without authorizing another dispatch.

Default ceilings are 30 minutes, 96 physical model attempts, 192 tool calls, 24 retries, 16 in-flight operations, 4,000,000 input tokens, 262,144 output tokens and 4,262,144 total tokens. Owner create/PATCH requests accept settings.executionLimits with maxWallTimeMs, maxModelAttempts, maxToolCalls, maxRetries, maxInflightOperations, maxInputTokens, maxOutputTokens and maxTotalTokens. Bounds must be finite and coherent. Active scopes only tighten: updating limits never refunds consumption or restarts the deadline. New independent inputs use the current settings.

The embeddable anima-harness exposes the same controlled model/tool execution, approval policy, Stop signal, observer and checkpoint contracts without requiring the daemon's database or HTTP host. Its default scope is ephemeral; durable restoration requires an explicitly reattached matching execution context.

## Trace evaluation

The portable anima-core trace evaluator captures actual RunObserver model/tool events and grades task output plus a complete isolated filesystem observation against independent fixture oracles. Its six-case battery covers ambiguous edits, stale reads, large output, repeated failure, unrelated successful checks and fresh-session skill reuse. Matched comparison requires the same model/provider/configuration, fixture-set digest and complete repetition matrix. It reports correctness, model/tool counts, errors/repeats, output bytes, available usage and elapsed time. Missing usage remains unknown. Grading does not approve a skill or grant tools.

See packages/core-rust/acceptance/turn-quality/README.md for fixture setup and the Rust API. Deterministic fixture captures prove the grading and runtime path; provider-backed captures must be labeled separately. Trace metadata comes from the runner and is not a signed authenticity attestation. Internal transport retries are not counted as separate logical runtime model steps.

## Crash acceptance

The Rust app tests launch owned child test processes with new temporary homes, stores, workspaces and loopback ports. They forcibly terminate and restart those processes at authoritative barriers, including accepted queued input, pending approval, settled native file effects and known secondary usage before any history flush. The tests assert once-only safe adoption, fresh owner decisions, no automatic effect replay, stable root/final identity and idempotent usage recovery. Existing compaction persistence/cancellation/rollback and connector delivery tests complement these process cases; they remain separate from real external-provider/device acceptance.

The harness reliability gate adds fixed-seed approval/Stop/replay histories, ownership-loss and save-failure cases, and a crash with a reserved model attempt. Its oracle compares durable receipts with actual dispatches and filesystem effects. The Windows/Linux workflow runs the Rust suite and driver checks; PostgreSQL fencing is a separate isolated job. The opt-in live driver requires a complete matched baseline/candidate trace matrix with recorded source, binary and fixture hashes. It never treats a deterministic pass or an identical-build comparison as evidence of provider-backed improvement. See packages/core-rust/acceptance/harness-reliability/README.md.
