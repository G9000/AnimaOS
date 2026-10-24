# Tavi First Playable Implementation Plan

> Historical plan from before relocation into `poc/village`. Original paths, workspace commands and Git instructions below record that milestone; use the [current PoC documentation](../README.md) for setup.

> For agentic workers: use superpowers:subagent-driven-development or superpowers:executing-plans to implement these checked tasks. The user has authorized implementation in this session.

**Goal:** Ship a local browser-playable shopfront with movement, Tavi's illustrated conversation, Mira's bread order, and a remembered bread preference, backed by Anima's Rust core.

**Architecture:** A new Three.js and React app talks to a dedicated Rust game host. Reusable game state and validation live in a small Rust library; the host embeds anima-harness, resolves the user's existing Anima model connection, and commits authoritative outcomes and request receipts together. Preserve the existing assistant application and its storage.

**Tech stack:** Three.js, React, Vite, TypeScript, Rust, anima-harness, Axum, SQLite, Vitest, Playwright.

## Scope and ownership

The source requirement is the user-supplied Cozy Village Language RPG v1.1, first playable milestone. Build the compact shop scene and one quest, not the three-chapter paid pilot. Japanese is the initial default; support the same reviewed choices in Mandarin. Generated language quality remains subject to human review.

- Browser owner: apps/village, including Vite config, React UI, scene, input and client tests.
- Rust owner: packages/core-rust/crates/anima-village and hosts/village-game, including Cargo workspace membership and persistence/integration tests.
- Root owner: generated art assets, launch tooling under tools/workspace-dev, root scripts and locks, documentation, end-to-end verification.
- No edits to unrelated daemon, web, playground, or core execution behavior.

## Frozen browser API contract

All fields use camelCase. Routes are relative to the browser origin, proxied during development.

- POST /api/village/saves with { language: "ja" | "zh" } creates a save.
- GET /api/village/saves/{saveId} restores a save without incrementing visits.
- POST /api/village/saves/{saveId}/visit with { requestId } enters a conversation and recalls prior committed events.
- POST /api/village/saves/{saveId}/turns with { requestId, message, choiceId?, recoveryOf? } sends free text or a reviewed choice. The optional choiceId identifies a server-validated reviewed choice; clients do not supply world-state changes. A recovery has a new requestId and names the original pending request in recoveryOf. An exact retry retains the original requestId and payload.
- GET /api/village/health returns model connection availability and engine identity without credentials.

All save and interaction routes return a GameView:

    {
      saveId: string,
      language: "ja" | "zh",
      questStatus: "pending" | "collected",
      inventory: [{ id: string, name: string, quantity: number }],
      coins: number,
      breadPreference: "honey" | "sesame" | null,
      visits: number,
      memories: string[],
      journal: [{ text: string, translation: string, reading: string }],
      dialogue: {
        text: string,
        translation: string,
        reading: string,
        expression: "warm" | "thoughtful" | "happy",
        source: "model" | "reviewed",
        suggestions: [{ id: string, text: string, translation: string }]
      },
      runtime: {
        mode: "live" | "reviewed",
        engine: "anima-harness",
        provider: string,
        model: string
      }
    }

Server-reviewed choice ids: collect_order, honey, sesame, ask_recipe, help. The collection choice explicitly requests Mira's two loaves. Choice labels and explanations are returned by the server. A missing connection uses reviewed replies with a visible status; it must never be presented as live generated dialogue. Live generation must run through anima-harness using the user's existing authorized Anima model connection. API errors use { error: string } and appropriate status codes.

Persist pending request identity before generation. At eight seconds expose reviewed recovery in the browser; bound generation on the server as well. Original generation and recovery compete for one atomic committed outcome. Recovery invalidates the original pending generation before applying its validated action. If original generation committed first, recovery returns that committed outcome without another action. A delayed generated reply cannot overwrite recovery. A changed payload using an existing requestId remains a conflict. Test both winner orders and the timeout path.

## Task 1: Rust game rules and persistence

- [x] Write regression tests for order collection validation, preference recall, per-save isolation, retry replay, payload conflicts, and reopening stored saves; watch them fail before implementation.
- [x] Implement bounded typed game actions and a committed event record. Use a small library under packages/core-rust/crates/anima-village; keep HTTP and disk I/O in the host.
- [x] Store save snapshots and idempotent request receipts atomically in SQLite. Repeating a committed request must return its original result; an altered payload using the same id must fail.
- [x] Keep world effects deterministic: only Mira's two-loaf order, one inventory grant, and the two offered preferences. Build NPC context from committed facts.
- [x] Test ambiguous and wrong orders, recipient and quantity clarification across turns, and valid paraphrases. Include Mira's introductory note, Tavi's recipe concern, equal rewards when help or either preference is used, and both offered breads when no preference is chosen.
- [x] Run focused package tests and formatting.

## Task 2: Rust NPC integration and API

- [x] Add a minimal runnable host under hosts/village-game using the frozen API contract.
- [x] Resolve an existing Anima model connection on the server without logging or returning credentials. Verify the owning configuration and adapter seam before implementation.
- [x] Run live messages through anima-harness. Parse a bounded response and validate proposed world actions; commit only after a usable result is produced.
- [x] Implement reviewed fallback choices, finite generation deadlines, safe retry reconciliation, and clear live/reviewed metadata.
- [x] Exercise the actual harness and API with a controlled provider fixture. Verify an actual generated NPC exchange through the user's existing Anima model connection as a completion gate; reviewed replies or a fixture alone do not satisfy live integration.

## Task 3: Playable browser scene and dialogue

- [x] Create apps/village with normal Bun workspace test, typecheck, build and lint scripts.
- [x] Test movement bounds, proximity interaction, restored save state, composition-safe input, dialogue focus, and retry identity before implementation.
- [x] Build an angled overhead Three.js shop scene: timber storefront, roof, lanterns, plants, cobblestone walk, sprite player and Tavi, and a bread plate reflecting the saved preference.
- [x] Add React quest and inventory overlays, a character portrait dialogue view, free typing, editable suggestions, translations/readings, and a small phrase journal.
- [x] Pause exploration controls while dialogue is open; Enter during IME composition must not submit. Dispose renderer resources and listeners on unmount.
- [x] Restore the save id from browser storage; keep authoritative progression on the Rust server. Render a helpful connection error instead of fabricating progression.
- [x] Run frontend tests, typecheck, build and lint.

## Task 4: Art and local launch

- [x] Use imagegen for original project-bound Tavi portrait and character sprite assets, matching the supplied warm village and illustrated dialogue direction. Copy selected outputs into apps/village/public/art and record prompts and paths.
- [x] Add a local launcher in tools/workspace-dev that owns both child processes and configures the API origin. Keep browser packages free of hardcoded backend ports.
- [x] Add root scripts for the game, a concise README with the start command, existing model resolution, local save location, and prototype limitations.
- [x] Add meaningful launcher tests and run workspace-dev tests.

## Task 5: Combined verification

- [x] Review plan, spec compliance, and code quality; fix important findings.
- [x] Run the new Rust library and host tests and the repository-required bun run rust:test suites.
- [x] Run apps/village test, typecheck, build and lint, plus relevant launcher checks.
- [x] Use a browser to play collection and preference, leave and revisit, reload, and confirm persisted basket and remembered preference. Check Japanese composition, rendering errors, and the actual Rust connection.
- [x] Record validation evidence and limitations. Remove task-owned scratch outputs after recording results; retain normal build caches.
- [x] Show the running local game and provide the workspace path and exact start command. Do not publish, merge, or push without a further request.
