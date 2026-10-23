# Delegation routing and orchestration controls

The companion needs to choose an appropriate worker route, bound its work, launch independent tasks, and collect verifiable results. Its existing isolated assignments, durable background handles, specialist delegation, steering, stopping, worktree support, and shared execution accounting remain the foundation.

## Owner policy

Persist a typed delegation policy with named provider/model routes and fallback chains. Only the owner edits routes. A tool may select a saved route and tighten assignment limits; it cannot provide credentials, endpoints, or extra permissions. Defaults retain four concurrent helpers, four starts, eight tool turns, a two-minute deadline, and 4096 output tokens per response. The owner may configure finite ceilings. Helpers omit the companion personality by default.

## Assignment controls

Both native launch paths accept a route, tighter limits, and an optional JSON output contract. Background admission binds these options to the immutable launch fingerprint and persisted assignment config. A changed retry must fail before creating another child. Missing options preserve existing behavior and old fingerprints. Tool access is always the current grant intersection.

Independent background assignments can be launched as a bounded batch. Each item has a stable child operation identity and returns an independent receipt, including partial failures. A collector waits for any or all selected handles with a bounded timeout, returns only committed results, supports cancellation, and never holds the control-plane transaction while waiting. Repeating collection has no side effects.

## Further coordination

Nested coordinators, explicit conversation forks, and resumable child sessions require lineage-aware admission, root-wide capacity, private-context selection, stop cascade, and restart validation. They must be added through the same durable ledger and shared execution budget rather than bypassing its checks. Default workers remain isolated leaves. Feature status is recorded in the execution plan; unimplemented controls are not advertised as available.

## Acceptance

Verify strict owner configuration, persisted routes across state reload, real native tool admission, model/fallback selection, bounded limits, changed-retry rejection, partial batch receipts, any/all collection, output validation, and omission of parent personality. Run current-tree host tests and affected client checks. Scripted models prove orchestration contracts; provider acceptance requires a configured live provider.
