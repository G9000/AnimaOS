# Village day expansion verification

> Historical verification from before relocation into `poc/village`. Test results, commands, listener addresses and Git/process state below describe that milestone. Use the [current PoC documentation](../README.md) for setup and current paths.

> Historical milestone: the user subsequently replaced the creator with a fixed, supplied MC. See [the current fixed-MC verification](2026-10-25-fixed-main-character.md). The creator and editable appearance described below are no longer part of the game.

Verified on Windows on 2026-10-25 in the isolated `g9000/tavi-first-playable` worktree. The user approved character customization, Mira, bread delivery and an evening callback, then requested a light cyberpunk and magic blend. All work remains local and uncommitted. Nothing was pushed or merged into main.

## Delivered experience

- A saved traveler with a Unicode name, three skin tones, three hairstyles, four hair colors, three outfits and three clothing colors. The creator preview and four exploration directions use the same original modular pixel source. Appearance can be edited without resetting progress.
- Tavi and Mira have distinct portraits, dialogue, witnessed memories and a reachable place in the square. Guided walking and nearest-resident interaction connect the route.
- Collect Mira's two loaves from Tavi once, receive the existing twenty coins, deliver both loaves to Mira, then return to Tavi at lantern time. Delivery consumes the bread once and changes the scene to evening. The committed callback recalls the delivery and optional preference/encouragement; completed saves remain explorable.
- Warm timber and greenery with copper conduits, teal rune lanterns, violet magical lights, floating signage, drifting motes and resident idle animation. Evening changes the lighting from authoritative saved phase.
- Appearance saves are model-free. Normal Tavi and Mira conversations run through `anima-harness` using the existing Anima model connection. The milestone callback uses authored reviewed content with explicit reviewed metadata, after live generation validates or reviewed recovery wins.
- Existing saves, receipts and omitted-NPC retries remain compatible. Pending conversations take priority over onboarding. Appearance and conversation requests retain stable identity across reloads; recovery remains bound to the original NPC.

## Verification

| Check | Result |
| --- | --- |
| `bun run --filter @animaos/village test` | 42 tests passed across 8 files |
| `bun run --filter @animaos/village typecheck` | Passed; also included in final build |
| `bun run --filter @animaos/village build` | Passed |
| `bun run --filter @animaos/village lint` | Passed |
| `bun run --filter village-game test` | 18 domain and 27 host tests passed |
| Touched Rust crates: formatting, build, scoped Clippy correctness | Passed; dependency warnings remain |
| `bun run --filter workspace-dev test` | 50 tests passed |
| `CI=1 CARGO_TARGET_DIR=…/target/cargo/validation-rust-daemon bun run rust:test` | 4,897 passed, 0 failed, 47 ignored across 69 suite groups |
| Stateful visual browser acceptance | Passed full chapter, appearance persistence, desktop/mobile views, language switching and restored Mira reviewed recovery; no page/console errors |
| Live browser acceptance | Passed real Tavi/Mira dialogue, free-text collection/delivery, preference, callback, reload, appearance editing and language-save separation; no page/console errors |
| Plan, creator, Rust and browser spec/quality reviews | Approved after fixes |
| `git diff --check` | Passed |

The live browser run reported `mode: live`, `engine: anima-harness`, provider `chatgpt` and model `gpt-6-luna`. Tavi and Mira greetings and ordinary turns reported `source: model`. Mira's delivery removed the loaves, retained twenty coins and entered evening. Returning to Tavi reported the authored reviewed callback, entered `complete` and remembered honey bread. Reload restored the completed save and edited traveler named Ren. Mandarin started a separate traveler and generated a live greeting; returning to Japanese restored the completed chapter.

Regressions include Unicode/control validation, visible appearance differences in all directions, draft retention and stable retry IDs, focus during save/retry, Enter and Escape during IME composition, old receipts and pending payloads, wrong-NPC recovery, no duplicate bread after delivery, private NPC knowledge, callback failure/recovery, phase-aware prepared replies, inventory display and texture disposal on appearance replacement/unmount.

The initial full Rust check hit the user's running main-server executable lock (`anima-daemon.exe`, Windows error 5). The required isolated output-directory rerun passed; no main-server process was stopped. The existing LF-sensitive acceptance fixture was normalized without source-content changes for validation, then restored to its original CRLF checkout. No fixture change remains.

## Review images and assets

- [Character creator preview](assets/village-day-creator.png): final stateful visual run.
- [Live evening village preview](assets/village-day-evening.png): real delivery completed, before returning to Tavi.
- Original generated Mira assets: `apps/village/public/art/mira-portraits.png` and `mira-sprites.png`. Built-in image_gen was used; complete prompts, reference roles, dimensions and crop rectangles are in [the art README](../../web/public/art/README.md).

The two review images are intentional deliverables. Temporary screenshots, scripts, environment files, logs and the isolated test save database were removed after recording results. Build caches and the user's existing game saves were retained.

## Handoff

All default village listeners (4302/8092) and temporary verification listeners (24302/28192) are stopped. The user's main server was left alone. To play later, run `bun run village` from this worktree; see [the app README](../../web/README.md).

This remains a local prototype. Generated Japanese, Mandarin, translations and readings need qualified language review before a customer pilot. Production hosting, payments, concurrency/latency and cost benchmarks remain outside this milestone. The final Vite build succeeds with a combined Three.js bundle warning (about 843 kB minified, 231 kB gzip).
