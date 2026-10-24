# Fixed main character verification

> Historical verification from before relocation into `poc/village`. Test results, commands, listener addresses and Git/process state below describe that milestone. Use the [current PoC documentation](../README.md) for setup and current paths.

Verified on Windows on 2026-10-25, branch `g9000/tavi-first-playable`. The user selected the supplied pink-haired MC and explicitly removed the character creator. This supersedes editable appearance in the earlier village-day milestone. No commit, push or merge was made.

## Current behavior

- Fresh saves enter the village immediately as the supplied MC. Existing saves retain quest progress, inventory, coins, preferences, language route and NPC memories. There is no creator, appearance editor or appearance mutation request.
- Old appearance-request metadata is cleared without changing server progression. The legacy traveler wire type and Rust API remain compatible, but cannot change the rendered MC or block exploration. Interrupted NPC conversations still preserve exact request identity and speaker-aware recovery.
- The supplied transparent atlas is rendered unchanged with original two-frame idle and four-beat walking animation in front/left/back/right directions. Actual displacement drives walking. Stopping returns to idle; dialogue/journal pauses reset to an idle pose and freeze its clock. Boots/backpack stay inside a shared crop and the foot baseline stays fixed.
- Tavi's collection, Mira's delivery, the evening callback and the magitech setting remain available.

## Original artwork

The four source assets were copied byte-for-byte to `apps/village/public/art/mc/`: `portrait.png`, `spritesheet.png`, `idle.gif`, `walking.gif`. The atlas SHA256 is `d36b2a283c626249a889ca80d2bf6db0d80e99f8cc464ef1050c557f2190316a`. No image generation, redraw or pixel editing was used. [Asset layout, source names and timing](../../web/public/art/mc/README.md) are recorded separately.

## Checks

| Check | Result |
| --- | --- |
| `bun run --filter @animaos/village test` | 34 tests passed across 6 files, including direct entry, legacy metadata, pending NPC recovery, quest inventory/language continuity and three original-atlas animation/resource regressions |
| `bun run --filter @animaos/village typecheck` | Passed; also included in final production build |
| `bun run --filter @animaos/village build` | Passed; existing Three.js bundle warning remains (about 834 kB minified / 227 kB gzip) |
| `bun run --filter @animaos/village lint` | Passed |
| Visual browser walkthrough | Passed fresh null-traveler entry, old appearance metadata, full Tavi/Mira/evening route, reload, language separation, mobile view and reviewed Mira recovery; no page/console errors |
| Original MC WebGL inspection | Passed four directions, both idle frames and all three walking poses with the actual texture/module; no page/console errors |
| Live browser walkthrough | Passed direct entry, actual generated Tavi/Mira dialogue, collection, honey preference, delivery, reviewed evening callback, reload and Mandarin/Japanese separation; no creator/editor or appearance requests |
| Plan/spec/quality review | Approved |
| `git diff --check` | Passed |

The live run started with `traveler:null`, collected exactly two loaves and twenty coins, delivered the loaves once, reached `phase:complete`, reloaded with `questStatus:delivered`, and still had no traveler profile requirement. Tavi and Mira ordinary interactions reported `source:model`; the existing authored milestone callback reported `source:reviewed`. Both Japanese and Mandarin live visits used `anima-harness` through the existing Anima connection.

This change only touches the browser and project documentation/assets. Rust behavior was unchanged; the prior 45 game Rust tests and 4,897-test full Rust verification remain the backend baseline. No main-server rebuild or process change was needed.

## Review images and handoff

- [MC in the village](assets/fixed-main-character.png)
- [Original four-direction idle poses](assets/fixed-mc-directions.png)

These two images are final review deliverables. Temporary browser screenshots, inspection scripts, environment files and logs were removed after recording their results. Existing saves and build caches were retained.

The game is left running at <http://127.0.0.1:24302/> with API28192, using persistent worktree game saves and the existing compiled Rust host. The user's main server was left alone. Refresh the existing game tab to load the new MC. All work remains local on the feature branch. Generated NPC language still needs qualified review before a customer pilot.
