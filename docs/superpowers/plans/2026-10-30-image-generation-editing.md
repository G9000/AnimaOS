# Image generation and conversational editing implementation plan

> **For agentic workers:** Use the loaded pstack Feature, How, Architect and Arena workflows. The owner authorized implementation and local checks. Do not commit, push, publish a PR, call a live provider or restart the user daemon.

**Goal:** Create images in chat, edit one explicitly selected image, and preserve immutable generated versions beside the conversation.

**Architecture:** Exact-approved native `generate_image` and `edit_image` tools call a reusable `ImagePort`. The host owns durable operation state and immutable image-version records in the existing conversation snapshot. Validated output bytes use fresh immutable attachment identities. The existing chat model and its transport stay independent.

**Stack:** Rust service ports and provider adapters, Rust daemon, thin TypeScript SDK and React console.

## Pstack workflow

- [x] Ground the affected subsystem through How.
- [x] Compare two structurally distinct architecture candidates through Architect and Arena.
- [x] Name the data structures and record the chosen boundaries before implementation.
- [x] Delegate bounded implementation with disjoint file ownership.
- [x] Verify native operations, SDK behavior and the conversation UI.
- [x] Review the combined changes and fix material findings.
- [x] Skip commits and PR publication. The delegation explicitly requires an uncommitted reviewable result.

## Throughput checkpoint

- [x] Blocking first steps. Actual pstack source and current official OpenAI image guidance were read. Native verification is authorized.
- [x] Independent workstreams. Image source is isolated on `g9000/image-generation-editing`. Scanned-document source belongs to the other chat.
- [x] Shared mutable state. This chat owns the sequential native verification schedule and the primary managed `target/cargo/validation`. The shared 32 GiB ceiling remains in force.
- [x] Smallest safe decomposition. One owner integrates the host. One worker owns the reusable image port/provider. Another owns the thin SDK/gallery/composer selection flow. Native checks have one executor.

## Design comparison and synthesis

Both independent candidates recommended direct native tools over adding Responses image-generation transport and provider conversation IDs to the chat model. A third owner-HTTP generation entry point was rejected because it duplicates the native chat execution path.

The comparison criteria were exact source/charge authority, durable paid-request outcomes, package ownership, chat usability and implementation scope. Direct tools satisfy all five with the existing approval and attachment boundaries. Responses couples the feature to one chat provider and needs a new approval interception boundary inside model transport. Candidate A is the base. Candidate B contributes explicit unknown-outcome state and the rule that display data must never establish image provenance.

Candidate B proposed a new top-level private image/blob registry. This slice instead uses the existing durable conversation snapshot and attachment store. Existing attachment reads detect altered bytes; this does not promise preservation against another process changing workspace files. The UI must report changed or missing bytes honestly.

Pstack's Cursor-only Task roles, Grok/Claude defaults and another-family cross-judge are unavailable here. Both candidates used the configured chat model through Codex delegation. No model diversity or standalone `pstack` command execution is claimed. The owner performs synthesis and a separate read-only review.

## Named data and behavior

- `ImagePrompt` and `ImageInput` are bounded owned provider inputs. `ImageRequest` distinguishes create from edit. `GeneratedImage` contains bounded validated PNG bytes and optional image-specific usage. `ImageError` distinguishes no send, definitive rejection and unknown remote outcome.
- `ImageWitness` binds the direct owned run, conversation generation, prompt and exact source attachment identity/hash before approval. Only a matching owner AllowOnce record creates `ImageOwnerApproval`.
- `ImageOperation` has one durable state enum. Dispatch intent is saved before network I/O. Success contains one immutable `ImageVersion` with output attachment identity and optional parent/source attachment. Interrupted dispatch is an unknown outcome and is never automatically resent.
- `edit_image` consumes exactly one image attached to the current accepted user turn. The gallery's Edit in chat action reuses that saved attachment ID in the normal composer. Ambiguous or absent image selection fails before a provider request. Standard chat attachment disclosure still applies to the selected image; the separate image-provider transfer and charge require exact review.
- The provider is canonical OpenAI Platform with the existing API-key pool. Use one configured `gpt-image-2.5-sunburst` request, medium quality, 1024 square PNG, count one, no redirects, automatic retries, provider fallback or subscription OAuth. Creation and editing preserve originals.
- Reconnect and local status reads never reissue provider work. Stop, clear, deletion, permission loss or changed source blocks late publication. Missing usage/cost remains unknown.

## Implementation units

1. Add `anima-service-adapters/src/image.rs` and `anima-model-adapters/src/image.rs`, exports and focused loopback/unit tests. Assert create/edit payloads, output limits, sanitized errors and no retry.
2. Add host `images` records/approval/service modules. Wire bounded operation persistence through the coordinator, session snapshot validation/recovery and ordinary exact tool approvals. Publish new output attachment/version only after acknowledged persistence. Test duplicates, source/scope changes, stop and unknown outcomes.
3. Add read-only owner image-list routes and SDK `images.ts`. Keep image bytes behind the existing authenticated attachment download route. Register native tools and explicit profile permissions.
4. Add a conversation image gallery/preview with version navigation, download and Edit in chat. Extend the attachment draft hook with a scoped saved-reference handoff. Test reload, source selection, stale scopes and object-URL cleanup.
5. Run required Rust, SDK and focused web checks sequentially where native work is involved. Review and update feature documentation. Keep live-provider acceptance explicitly unverified.

Write meaningful failing tests before their implementation. Run native tests only through the primary managed validation wrapper. Record any unrelated baseline failures separately.

## Verification coordination

Use the primary checkout's task wrapper with an absolute inherited validation target. Run every native test or service command sequentially. Record image results and the scanned-document request revision, source fingerprint, command and exits separately under the primary checkout's `test-output/pstack-coordination` directory. Do not edit scanned-document source.

## Verification evidence

The full repository Rust test command passed, including 3,749 daemon library tests and the packaged supervisor/integration suites. The focused image filter passed 31 tests, and the reusable provider/port checks passed 17. The SDK passed 310 tests across 55 files, its build and typecheck. The web console passed 2,551 tests across 214 files (one skipped), its build and typecheck; the six focused image files passed 153 tests. All six offline desktop/mobile image browser cases passed. Browser-test typechecking passed after restoring its worktree-local SDK dependency link.

After a PC crash, the worktree remained on its original base and its preserved source fingerprint matched. Reversing only the three rustfmt changes reproduced the exact full-Rust-tested native fingerprint: module declaration ordering in `agent_runs.rs`/`lib.rs` and array formatting in `approvals/policy.rs`, including original line endings. Completed runtime results were retained; the interrupted final lint was rerun. Formatting and all-target Clippy passed, with existing warnings. No live provider, billing or image-quality acceptance was performed. No user daemon was restarted, and no commit, push or PR was created.

## Sources

- https://github.com/cursor/plugins/blob/main/pstack/skills/poteto-mode/SKILL.md
- https://github.com/cursor/plugins/blob/main/pstack/skills/architect/SKILL.md
- https://github.com/cursor/plugins/blob/main/pstack/skills/arena/SKILL.md
- https://developers.openai.com/api/docs/guides/image-generation
- https://developers.openai.com/api/reference/resources/images/methods/edit
- https://developers.openai.com/api/docs/pricing
