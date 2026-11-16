# A6 capability detection and read-only graph recall

> For agentic workers: use `superpowers:executing-plans` for this approved slice.
> Reviewers are read-only; implementation remains with the coordinator.

**Goal:** Add bounded graph recall to AnimaOS's existing additive Mother provider,
without changing native memory, live grants, publication or owner records.

**Architecture:** Keep the selected agent/connection and existing v1 bootstrap.
Discover graph support only through already-reviewed capability/query/get tools.
Capability availability is not permission: Mother enforces independent
`garden:read` on every query/get, and AnimaOS retains its current dispatch/grant
checks. Supplement v1 candidates with current typed graph records and bounded
neighbors. Rank in AnimaOS, preserve original graph JSON and optional extensions,
and keep the existing four-item/8 KiB output and three-second deadline.

**Tech stack:** Existing Rust daemon, serde JSON and MCP transport. No dependency,
Mother database, binding-schema, account or live client configuration changes.

The owner explicitly selected this first A6 slice after authorizing publication
of record selection. That fix is on origin/main at `c9ddf98`. Full A6 graph
publication/mapping and persistent change-feed caching remain later work.

## Design decisions

Use optional capability-gated supplementation rather than replacing v1 recall or
requiring a new connection. Requiring graph support would break existing agents;
unconditionally invoking graph tools would confuse a v1 grant with graph access.
Existing context-only or old servers therefore retain their v1 path unchanged.

Only `mother_get_capabilities`, `mother_garden_query` and `mother_garden_get` join
the adapter's read allowlist. All three must already be reviewed on the selected
connection. No tool is reviewed, granted or enabled automatically. Validate the
capability response's version, workspace/generation and actual supported v2 query
capability; defined future contracts alone do not activate graph use. Preserve
workspace/generation mismatch and restart-required errors through the read helper
as shared-authority invalidation rather than optional graph denial. A graph-only
credential is not a new supported binding in this slice: the current additive
binding still requires its v1 context reader and authority.

Use up to two short literal topic queries, no vector preparation/downloads, and
at most one one-hop neighbor query with at most two starts, eight edges and 32
work units from returned entity candidates. Query pages
remain opaque, first-page bounded selections. Fetch up to four distinct current
graph records, prioritizing topic entities needed to interpret assertions, then
relevant assertions/evidence. Validate typed IDs, current lifecycle/revision,
workspace/generation and consistent visibility epochs. Preserve coverage
completeness, truncation, reason, watermarks and whether another cursor exists
separately from envelope shortening. A change or read failure
discards the graph batch; native/v1 recall remains available. Binding/generation
failures still invalidate shared context under the existing authority boundary.

Final ranking uses exact topic and event overlap, preserving v1 owner-pinned and
assigned precedence. Complete graph assertion items can include their freshly
read subject entity as context; unknown endpoints remain typed references.
Preserve valid/observed/transaction times, lifecycle, contributor/actor/approver,
evidence references and unknown optional extensions. Never infer dates,
relationships, resolved evidence content or factual verification from an ID.
Keep graph locators typed and distinct from v1 record IDs.

Each graph query requests a small complete window; an explicitly partial envelope
may be retried once with one hit. Never use partial JSON or retry mutations.
Graph reads remain inside the same absolute three-second deadline. Optional graph
work gets at most 750 ms and only the time left after reserving 500 ms for final
v1 authority validation; skip it when that reserve is unavailable. Optional graph
failures discard graph candidates; v1 is returned only when final authority
validation succeeds. Canceling an entered MCP call retires its connection under
the existing safety policy. That invalidates all shared results and retains
native fallback until a fresh turn reconnects and validates again. Do not add a
new reconnect layer or relax transport retirement to reuse stale v1 context.
Cancellation and binding/generation mismatch still invalidate shared context.
Test slow capability, query and Get separately through `WorkspaceRunContext`.
V1 retains its existing four full-fetch cap; graph adds at most four current full reads, with
at most four recalled items and 8 KiB combined complete output. Do not promise
exhaustive recall. No graph result survives across requests, so this slice needs
no durable change cursor or derived-cache invalidation.

## Implementation and verification

- [x] Add a synthetic graph-capable MCP fixture and provider regressions before
  implementation. Reproduce absent graph recall through `WorkspaceRunContext`.
  Cover current other-harness facts, later corrections and preserved temporal/
  extension data; no capability tool, inactive/unsupported capability, propose-only
  denial, missing tool review, wrong identity, stale epochs, inactive records,
  complete-output limits and local/native fallback.
- [x] Run the new behavior filter and retain its failing output outside Git.
- [x] Add `hosts/rust-daemon/src/memory_provider/graph_wire.rs` for pure v2 core
  shape/semantic validation, preserving unknown optional extension payloads.
  Keep graph query planning, typed locators and read-only gathering in `graph.rs`.
  Modify `mother.rs` only to share checked reads and combine bounded candidates.
  Keep writes/sync hooks unchanged. Update `workspace_context.rs` guidance only
  as needed to distinguish typed graph references from v1 locators.
- [x] Rerun focused provider checks. Extend the existing synthetic native-owner
  integration fixture in `mcp/graph_acceptance_tests.rs` to exercise provider
  recall through the real Mother executable and disposable data/grants. Include
  revoked/missing `garden:read`, cross-harness correction and unchanged native
  memory/publication state. Never read the installed private client config.
- [x] Obtain read-only review, fix meaningful findings, then run required full
  `bun run rust:test`, `bun run rust:lint` and a locked bundled native build at
  fixed Rust source hashes, serially under the managed 32 GiB cache ceiling.
- [x] Document supported behavior and limits in `docs/workspace-context.md` and
  the validation report. Preserve the installed qualified selection build and
  live profile unchanged; do not widen owner graph grants as acceptance.
- [x] Confirm origin publication of selection, unchanged live data/config/grants,
  save an explicit Mother technical checkpoint and archive/remove only owned
  scratch after evidence is recorded. Leave A6 source local for review.

Focused development command:

```text
bun tools/workspace-dev/scripts/run-rust-daemon-tests.ts --lib memory_provider_
```

The real-Mother acceptance remains explicitly selected and uses only a supplied
executable with an owned synthetic root. Full Rust tests are required before
claiming the final source qualified. Test counts must be nonzero. Existing failed
trial evidence, private backups and the original dirty additive worktree remain
retained.

Completion: 55 focused checks, one explicitly selected native Mother acceptance,
6,001 full Rust passes (68 ignored across 115 groups), lint and the locked bundled
build passed at identical Rust source hashes. Read-only review found no remaining
P1/P2. The interrupted first full run and failed development trials are retained.
Preservation audits passed; owned scratch was archived/removed after process and
path checks. Mother checkpoint `8329b8df-a697-4dda-b3b6-14b3d62112a7` revision 13
records completion. A6 remains local/uninstalled; live grants are unchanged.
