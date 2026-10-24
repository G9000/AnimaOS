# Village Day Expansion Implementation Plan

> Historical plan from before relocation into `poc/village`. Original paths, workspace commands and Git instructions below record that milestone; use the [current PoC documentation](../README.md) for setup.

> Follow-up decision: the user retired character customization in favor of their supplied MC. [The fixed-MC plan](2026-10-25-fixed-main-character.md) supersedes the creator/rendering parts of this historical milestone. Preserve the connected Mira quest; do not restore the creator unless requested.

> For agentic workers: use superpowers:subagent-driven-development for implementation and separate spec/quality reviews. The user approved this milestone and execution in the current chat. Keep all work on `g9000/tavi-first-playable`; do not push or merge into main.

**Goal:** Create a saved traveler, collect Tavi's bread, deliver it to Mira, and return to an evening callback in a livelier village.

**Architecture:** Extend the existing pure Rust game domain and dedicated game host; preserve existing saves, request receipts and reviewed recovery. React owns an accessible creator and contextual dialogue; Three.js renders the selected traveler and two residents. No assistant host changes. The village dev servers were stopped at the user's request and must remain stopped after verification; temporary verification hosts use isolated ports, databases and Cargo outputs where needed.

**Tech stack:** Existing React, Three.js r186, TypeScript, Rust, SQLite, anima-harness, Vitest and Playwright. No new framework.

## Approved scope and frozen contract

- Small creator: name (1–24 trimmed Unicode characters, no control characters), skinTone `fair|warm|deep`, hairStyle `short|bob|braid`, hairColor `chestnut|ink|gold|auburn`, outfit `cloak|tunic|overalls`, outfitColor `moss|ochre|berry`. Values use typed allowlists. All choices visibly affect the preview and exploration sprite, including rear/side movement. Save creation can still precede customization; `traveler: null` opens the creator. Existing progress is retained. Editing later is supported.
- `POST /api/village/saves/{id}/traveler`: `{requestId, traveler:{name,skinTone,hairStyle,hairColor,outfit,outfitColor}}`; idempotent, atomic and model-free. Same ID/different appearance conflicts. Shares the per-save mutation boundary with conversations. Validation rejects before admission. Retry the same request after a lost response; never overwrite a pending conversation.
- Existing visits and turns gain optional `npcId: "tavi"|"mira"`, default Tavi for backward compatibility. NPC identity is included in request identity and persisted pending metadata. Reviewed recovery stays with the original NPC. Existing request IDs, old snapshots and stored response receipts remain readable.
- GameView adds `traveler: Traveler|null`, `npcId: "tavi"|"mira"`, `phase: "day"|"evening"|"complete"`. `questStatus` extends to `pending|collected|delivered`; inventory reflects actual remaining loaves. Existing fields remain.
- Collect once from Tavi: validate Mira and two loaves, retain existing one-time 20 coins. Deliver only to Mira while holding both loaves, remove them once, retain coins. Delivery switches to evening. New reviewed choice `deliver_order`; existing choices retained for Tavi, speaker-appropriate suggestions for Mira. Free-text intent also supports delivery. Wrong NPC, missing bread, ambiguous delivery and repeats cannot duplicate or reset progression. Reject accidental collection-intent regrants after delivery.
- Mira is a friendly, practical supper organizer with her own conversation history and witnessed memories. She knows the supper and her order, not private encouragement/preferences told only to Tavi. Tavi retains their own continuity and uses the saved traveler name as data. Both characters use the same authorized existing model connection through anima-harness.
- Returning to Tavi after delivery triggers the authored evening callback: the traveler reports the successful delivery, Tavi responds about the supper and saved bread preference/encouragement. Mark complete only when that interaction commits; replay/recovery produces the same result. No real-world wait or background LLM loop. Day/evening lighting follows authoritative phase. A completed save remains explorable and conversational.
- Keep the cozy portrait UI and original Tavi assets. Add original Mira portrait expressions and a matching sprite. Add Mira's meeting spot, a guided walking target, clear three-step quest progress and final chapter message. Add simple deterministic idle gestures and ambient scene animation; no additional systems or third NPC.
- User steering during implementation: blend in a little cyberpunk and magic. Use cozy magitech: warm timber/greenery, teal/violet rune lanterns, copper conduits, small holographic signs and drifting magical lights, with a stronger evening contrast. Keep the illustrated characters and readable language UI.

## Task 1: Rust domain, host and compatibility

Owner: Rust implementation agent; files under `packages/core-rust/crates/anima-village`, `hosts/village-game` only.

- [x] Write failing tests for customization validation/persistence/replay, legacy save + receipt defaults, two-speaker routing/recovery identity, delivery prerequisites/replay and evening callback.
- [x] Extend typed state/actions/views and speaker-specific content/context. Add model-free customization and delivery/callback via existing atomic save boundary.
- [x] Keep prior exact retry, conflict, disconnect and timeout tests passing. Add tests that delivery cannot recollect, private memories do not leak, and failed generation cannot commit the callback.
- [x] Run focused host/domain tests, formatting/build/lint for touched Rust crates. Hand off exact API/content and validation evidence.

## Task 2: Creator, scene and chapter integration

Owner: browser implementation agent; files under `apps/village` except generated binary art and its provenance notes.

- [x] Add tests before implementation for creator validation/draft/error/retry behavior, compatible saved traveler, two-NPC pending restoration, delivery objective and completed evening state.
- [x] Implement a shared deterministic modular pixel traveler renderer for creator preview and scene, with visible hair, skin, outfit and palette choices; use nearest-filtered textures and dispose replaced resources.
- [x] Add creator modal/onboarding and edit action. Prevent movement/dialogue during editing; preserve IME behavior and focus on close. Persist customization server-side with a stable request identity across retry/reload.
- [x] Add Mira at a distinct reachable meeting spot, nearest-NPC interaction and guided walking targets. Make dialogue portraits, titles, suggestions and pending restoration speaker-aware.
- [x] Add connected quest objectives, real inventory delivery, evening lighting and the completed callback. Keep optional preference and language helpers. Preserve separate language saves and existing request recovery protections.
- [x] Run frontend tests, typecheck, lint and production build. Update fixture and live E2E scripts for the full route and creator persistence.

## Task 3: Art, review and verification

Owner: root coordinator; generated art, docs, integration and final checks.

- [x] Generate and inspect original Mira portrait expressions and sprite assets with imagegen; save project-bound outputs and complete prompt/layout provenance.
- [x] Review the plan, then completed implementation for spec compliance and code quality. Resolve important findings before handoff.
- [x] Run relevant browser and launcher checks plus new game Rust tests and the full required `bun run rust:test`. Use separate validation Cargo outputs if the main server locks an executable; do not stop the user's main server.
- [x] Run temporary browser acceptance on isolated ports/database: customize, collect, deliver, evening return, reload, appearance and phase retention, JP/ZH save separation, reviewed recovery and live NPC generation. Inspect creator, exploration and both portrait views.
- [x] Stop every task-owned verification server, confirm the default village ports remain closed, remove exact scratch outputs after recording evidence, update this checklist and app/host README.
- [x] Report the branch, completed behavior, validation, limitations and restart command. Leave main untouched and do not push.
