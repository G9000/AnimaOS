# M75: Make capability inventory status truthful

## Goal and settled baseline

M74 merged at `71787ee6b19460e522fe972348717d0fea7c1afa` (PR 80). Its fresh strict browser case passed in 14.391 seconds: browser result and all route/save assertions passed, the exact 84-byte fixture was saved, 11 owned processes retired, Job active count reached zero with no errors. Preserve that case and its snapshots as settled evidence.

Fix one current PAR-12 accuracy gap in capability discovery. `hosts/rust-daemon/src/routes/capabilities.rs` currently labels every extension `planned`, says all extension installation is unavailable, and maps `read_document` to `utility`. Current voice, document, and community-bundle implementations are therefore reported as unavailable. The `/api/capabilities` GET is the right existing read-only surface; do not add an endpoint, health probe, activation action, or side effect.

## Truthful inventory contract

- Keep `schemaVersion: 1` and the existing `/api/capabilities` response shape.
- Extension statuses are `planned` or `partial`. Report voice, document engine, and community bundles as `partial`; keep camera, smart home, and external coding agents `planned`.
- Use bounded descriptions that name the available slice and its explicit prerequisites without claiming provider/device health or full capability.
- Voice is partial: selected WAV transcription/local capture and selected-reply speech output exist behind explicit opt-in/configuration; device coverage and provider quality remain unverified.
- Document engine is partial: bounded `read_document` extraction supports PDF, DOCX, and XLSX. Creation, OCR, and other processing remain unavailable.
- Community bundles are partial: reviewed MCP and skill installation exists. This does not provide an arbitrary execution bridge.
- Add `read_document` tool metadata under category `documents` with the existing tool-grant and workspace/document prerequisites. Leave the current browser category and its owner policy/grants unchanged.
- Maintain the existing limitations and revise only claims made stale by these facts. Inventory remains descriptive: it grants no tools, probes providers, starts connectors, reads profiles, or activates extensions.

## API and UI behavior

Extend `packages/sdk/src/capabilities.ts` additively: accept `browser` and `documents` tool categories and `partial` alongside `planned`. Keep the schema version unchanged and preserve the existing endpoint client behavior. Update `capabilities.spec.ts` with a mixed inventory fixture; no new client request or permission behavior.

In `apps/web/src/components/WorkspaceCapabilities.tsx`, keep current registered-tool search and add the two categories to the visible/searchable grouping. Replace the “Room to grow”/all-unavailable language with an “Additional capabilities” section that includes both partial and planned items. Render “Partial implementation · setup required” for partial, “Planned · unavailable” for planned, and “Implementation status unknown” for any unrecognized runtime status. Unknown status must never imply ready, enabled, or usable. Keep no enable/grant buttons.

The page may show a partial capability beside a missing prerequisite, but it must not imply the prerequisite is configured or healthy. Preserve offline, loading, error, retry, authorization, and unknown-tool-category behavior.

## Ownership and RED-first acceptance

One native lane owns `hosts/rust-daemon/src/routes/capabilities.rs` and `hosts/rust-daemon/src/routes/tests/capabilities.rs`. First add a route RED asserting partial extension status for voice, document engine, and community bundles; planned status for the three planned modules; browser metadata remains `browser`; and `read_document` is `documents` with explicit requirements. The existing “all extensions are planned” assertion must be replaced by the explicit inventory contract.

The SDK lane owns `packages/sdk/src/capabilities.ts` and `packages/sdk/src/capabilities.spec.ts`. Add an additive mixed-status/category fixture and retain the single GET request assertion.

The web lane owns `apps/web/src/components/WorkspaceCapabilities.tsx` and `.test.tsx`. Add mixed partial/planned rendering, searchable browser/documents grouping, unknown-status fallback, and no-enable-control assertions using the existing mocked inventory. Preserve the existing offline/error/retry tests.

Use current implementation sources as evidence: M67 owner-configured document access, M56–M58 native voice controls, M42–M45 reviewed community-bundle and installation flows. Use the pinned public Hermes source only to compare the existing discovery pattern; reuse Anima’s API and UI. Do not port the Python tool or introduce additional discovery requests.

## Validation and risks

After RED review, implement in parallel ownership lanes, run one native review and one final SDK/web contract review, then use one combined fix wave. Run Rust daemon test, lint, build, and format gates; SDK test/typecheck/build; web test/typecheck/build; targeted formatting; and capability documentation consistency checks. Root owns all commands and merge gates.

Top risks:

1. **Overstating readiness:** partial means implemented in a bounded slice, not configured, healthy, provider-verified, or fully usable.
2. **Authority leakage:** inventory is descriptive only; never mutate grants, start connectors, or probe external services.
3. **Schema churn:** retain version 1 and the existing GET shape; make only additive category/status changes.
4. **Hidden omissions:** keep planned status for camera, smart home, and external coding agents; describe remaining limits accurately.
5. **UI false confidence:** render unknown status as unknown, and preserve prerequisite language and the absence of enable actions.

This plan authorizes only the truthful inventory/API/UI update. It makes no full-parity or live-provider claim.

## Controller rulings on the risks (binding)

1. Ruling: partial is implementation scope only — descriptions name supported slices and missing setup/acceptance; never emit ready/enabled/healthy from this inventory — cost if wrong: owners could mistake implementation for operational readiness.
2. Ruling: retain GET-only discovery with zero activation — no provider/voice/MCP probe, profile read, account access, grant mutation or connector start; use existing registry/state facts — cost if wrong: viewing status could dispatch or spend.
3. Ruling: keep schema1 and additive closed SDK values — add partial and browser/documents without changing payload keys or client request; old planned values stay valid — cost if wrong: older typed consumers may need an upgrade, but existing shape remains.
4. Ruling: describe exactly implemented subsets — voice, bounded PDF/DOCX/XLSX reads and reviewed MCP/skill bundles are partial; camera/smart home/external coding remain planned, arbitrary execution/OCR/document generation remain unavailable — cost if wrong: inventory could conceal a real limitation.
5. Ruling: unknown UI states remain unknown — mixed planned/partial cards and new categories render without enable actions, retain offline/retry/stale-response fences — cost if wrong: a future daemon value could produce false readiness.

Ruling: preserve historical rebuild inputs before validation — archive exact prior web-dist bytes before rebuilding, verify old sealed closures against those snapshots and pinned source Git blobs, and use a separate Rust validation target to preserve the pinned old daemon image and ordinary runtime — cost if wrong: archival verification blocks publication. Immutable outcomes are not re-executed against a new build. No real runtime case is planned for this metadata/UI change.

Reuse decision: extend Anima's existing capability discovery/SDK/component and current voice/document/bundle implementations. The pinned public Hermes implementation remains a behavior reference; importing its Python discovery/lifecycle would add a second runtime and is unnecessary. No literal port or dependency addition.

## Recorded outcome

M75 implemented truthful capability discovery: voice, bounded document reads and reviewed community bundles are partial; camera, smart home and external coding workers remain planned. Browser/document tools are searchable, and unknown status stays unknown. Fresh Rust 2943 passed/28 intentional ignores/46 suites, lint/build/format; SDK 201 passed/3 service skips; web 1519 passed; SDK/web typecheck/build and both reviews passed.

Implementation metadata is not configuration, health, tool authority or live acceptance. No endpoint, provider probe, grant, dependency or service activation was added. Historical M69–M74 cases, raw snapshots and pinned Git versions remain unchanged; archived build bytes preserve their original closures after normal rebuilds.

The single fix wave corrected semantic region names in the regression. A later typecheck gate found a missing `.js` extension in a new type-only SDK test import; it was corrected and the SDK suite plus full type/build gates passed again. No product change followed review.
