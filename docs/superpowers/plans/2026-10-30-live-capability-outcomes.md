# Live capability and outcome checks

Goal: Distinguish registered support, configuration, current chat authority and physically observed outcomes. Keep old unused chats quiet and known failures visible.

## Pstack feature steps

- [x] `how` over the affected subsystem.
- [x] `architect` for parallel design exploration.
- [x] Write the throughput checkpoint as four todo items.
- [x] Delegate code-writing to a subagent using named data shapes and bounded scope.
- [ ] Verify on the matching surface.
- [ ] Rebase into small, ordered commits. Skip: commits and rebases are excluded in this batch.
- [ ] If the design is contested, `interrogate` before shipping. Skip: independent candidates agree on the smaller image slice.
- [ ] Run `Opening a PR`. Skip: publication is excluded.

## Grounding

Global capabilities own registration/configuration inventory. Chat tool_scope owns captured execution ceilings and current policy. Native catalog checks current grants and epochs. CompletionReview already trusts host byte snapshots rather than model claims. Image operations own exact run/call/approval identities and preserve unknown outcomes without replay. The gallery assumes its ledger API exists. The ledger validates metadata but does not check current bytes.

## Candidate comparison and synthesis

Candidate A adds versioned inventory contracts and response-only saved-image readback. Candidate B negotiates by 404 and extends workspace completion receipts. A wins on boundary size, incident relevance and preservation of receipt settlement ordering. B contributes preserving known operations on refresh failure and treating a 404 as unavailable, never evidence of remote failure. Independent default Codex agents produced both candidates. No different-model review ran.

Model the Domain shaped typed configuration and observation states instead of a universal verified flag. The host observation identifies operation, attachment, immutable version, digest, observation time and saved_bytes_only limits. Evidence comes from physical byte reads, never model claims. Historical provider status remains separate.

## Type and caller sketch

RuntimeCapabilityObservation { instanceId, observedAtMs, images: { ledgerVersion:1, observationVersion:1, configuration }, pdfPages: {version:1} , limits }
ConfigurationObservation { state: configured|unconfigured|unknown, reason }
ImageObservation { state: bytes_matched|unavailable|not_checked, operationId, attachmentId, versionId, observedAtMs, limit:saved_bytes_only, sha256/byteLength or reason }
ImageGalleryState { kind:loading|ready|unsupported|unavailable, operations }

The inventory supplies API support. Existing search_tools and describe_tool return the same running-instance observation to the model after current authority checks. Scope/policy remain distinct. A successful code check never becomes live deployment proof.

## Throughput checkpoint

- [x] Blocking first steps. Verify exact clean base, create fresh managed worktree, ground existing authorities and compare independent designs.
- [x] Independent workstreams. Image observer and SDK image parser have one owner; capability/gallery work has another. Source files are disjoint.
- [x] Shared mutable state. Worktree isolation avoids other chats' source writes. Parent executor alone uses primary absolute target/cargo/validation with 32 GiB cap.
- [x] Smallest safe decomposition. One feature integrator and a fresh independent reviewer. No general effects verifier or receipt-order rewrite.

## Acceptance and verification

Old inventory without image contract means no ledger request for unused chats. Known image activity has compact visible compatibility guidance. Supported empty ledgers stay quiet. Failed refresh retains known operations. Current exact saved bytes produce hash/time/version evidence. Missing, changed, over-budget or stale bytes remain unavailable or unchecked. Unknown paid outcomes stay unknown without retry. SDK rejects malformed and target-mismatched evidence while legacy pages remain unobserved.

Run focused pure SDK/web specs, SDK build/typecheck, web typecheck/build and isolated browser fixture acceptance. Parent executor runs wrapped Rust tests/lint. Native/service builds, live providers, daemon restart, commits and publication remain excluded here.

## Recorded verification

Pure SDK image/capability checks passed 39 tests. Focused gallery/session/diagnostics checks passed 81 tests. Browser fixtures passed 10 desktop/mobile cases, including authenticated blob pixels, compatibility and no replay. SDK/web build and typecheck passed; web-e2e typecheck passed. Existing JSDOM canvas/React act warnings and Vite config/chunk warnings remain. Native source is frozen in request revision 3 for the parent executor. Full rust:test and rust:lint remain mandatory combined gates. No native success is claimed here.

The first independent review found no substantive correctness or authorization issue. Parent review found unbounded overlapping physical reads. The correction now shares two physical reader slots across GET and post-generation readback. The owned permit remains inside the blocking closure after async cancellation. A fresh independent review found no substantive issue in that correction or Doctor wiring. Remaining test scope excludes a full HTTP disconnect or concurrent-clear race across real I/O.

A read-only health/OpenAPI observation at 2026-10-30T14:18:32Z saw health ok, 366 operations and image/PDF routes. This differs from the earlier 363-operation incident. No daemon was restarted, no provider was called and no new deployment is inferred.

Combined read-only integration review found that the SDK incorrectly required wall-clock observations to follow saved-version timestamps. The two-file SDK correction removes that ordering assumption, retains positive safe-integer time and exact target/hash/length validation, and passed an observed failing-then-passing rollback regression. No other actionable combined issue was found. Integration remained read-only.
