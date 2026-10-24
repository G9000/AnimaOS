# Fixed Main Character Implementation Plan

> Historical plan from before relocation into `poc/village`. Original paths, workspace commands and Git instructions below record that milestone; use the [current PoC documentation](../README.md) for setup.

> Use superpowers:subagent-driven-development for the bounded UI removal and separate review. The user explicitly replaced character customization with the supplied MC. Keep `g9000/tavi-first-playable`; no push or merge into main.

**Goal:** Enter the existing village as the user's pink-haired MC, with original idle/walk animation and no character creator.

**Architecture:** Frontend-only change. Copy the supplied portrait/GIF references and matching original transparent sprite sheet into the game. Animate the sheet through Three.js texture UVs. Remove creator onboarding/editor/procedural rendering from the browser while retaining the legacy traveler wire type and Rust save/API compatibility. Quest outcomes, model integration and existing save data remain unchanged.

## Asset contract

Source folder: `C:/Users/leoca/Downloads/Julio MC`. The portrait and GIFs were supplied by the user; the matching `MC_spritesheet.png` is alongside them and has been inspected. Copy source files without altering pixels. No generated replacement art.

- Atlas 96×384, 3 columns × 8 rows, cells 32×48.
- Rows 0–3 idle: front, back, right, left; two frames in columns 0/1. Rows 4–7 walk: same direction order, three frames in columns 0/1/2.
- Idle sequence [0,1], walk sequence [0,1,2,1], 300 ms per frame from supplied GIF metadata.
- One fixed crop within every cell: x3,y6,width26,height34. This contains all frame alpha, including backpack and stepping feet, with stable foot alignment.
- Scene directions remain front=0,left=1,back=2,right=3. Map to atlas direction rows [0,3,1,2]. Use nearest filtering, sRGB, real alpha and shared texture disposal. Stop stepping when movement stops; idle continues. Freeze the character during paused dialogue/journal. Preserve rendering depth/shadow behavior.

## Task 1: Remove creator UI

Owner: UI implementation agent. Files: `apps/village/src/App.tsx`, `pending.ts`, associated tests, creator/procedural modules and tests, `apps/village/e2e/*`, app README. Coordinate scene-prop removals with root; root owns scene and new MC renderer.

- [x] Add regressions: fresh save with traveler:null enters exploration immediately; existing saved appearance cannot change MC identity; cached appearance request metadata cannot trap gameplay; language switch and restored pending NPC requests still work.
- [x] Remove creator import/modal/edit action, appearance async state and gating, and stale local appearance metadata. Keep conversation retry/recovery unchanged. Do not mutate or reset existing quest saves to adopt the MC.
- [x] Remove dead CharacterCreator/CSS/traveler-art/procedural tests; retain only legacy traveler data types required by the wire contract. Remove dead appearance parsing/client mutation paths when no longer used; no Rust changes.
- [x] Update visual/live E2Es to start directly in the village and exercise MC, bread collection, Mira delivery, evening callback, reload and language separation. Preserve reviewed recovery and inventory assertions. Document the fixed MC and original asset provenance.
- [x] Run frontend tests, typecheck, build and lint. No server control, commits, push or main changes.

## Task 2: Original MC rendering

Owner: root. Files: new `scene/main-character.ts`, animation helpers/tests; existing `scene/characters.ts`, `scene/scene.ts`, `VillageScene.tsx`; `public/art/mc/*` and asset notes.

- [x] Write regression tests for atlas row/direction/frame mapping, idle/walk transitions and disposal before implementing.
- [x] Copy and inspect original assets; implement a single loaded texture with frame UV updates and consistent feet. Remove procedural appearance texture use and scene appearance props/handles. Keep movement, nearby NPCs, phases and quest props.
- [x] Inspect the real MC in all directions and idle/walk in a browser; retain one final review image.

## Task 3: Review and verification

- [x] Review plan and final combined change for spec compliance and correctness; resolve important findings.
- [x] Run final relevant frontend checks. No Rust behavior changes, so retain the prior Rust verification rather than rebuilding the main server.
- [x] Verify the already-running separate game at24302/API28192 updates correctly, preserves saves and completes the Mira route. Do not change the main server or default ports.
- [x] Record results, remove exact scratch outputs after recording, retain original assets/review image, and report branch plus game URL.
