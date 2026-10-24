# Lantern Village: a village day

A working title for the cozy language RPG prototype: explore as the supplied pink-haired main character, collect Tavi's two loaves, deliver them to Mira, and return to Tavi for an evening conversation. The warm timber village mixes copper conduits, teal rune lanterns, violet magic lights and a small floating supper sign. Three.js renders the environment and directional sprites; React presents painted portrait dialogue.

From `poc/village`:

```sh
bun install
bun dev
```

The launcher starts a separate Rust game host and the browser at [http://127.0.0.1:24302](http://127.0.0.1:24302). The first Rust compilation can take longer than the web startup. It uses your existing Anima model connection on the server. Credentials are never sent to the game client. See the [game host README](../host/README.md) for connection resolution and overrides, or the [PoC README](../README.md) for all commands and package boundaries.

The village opens directly into exploration. The main character uses the user's original sprite atlas for idle and walking animations in all four directions. There is no character creator or appearance editor. Original portrait, animation references and atlas are preserved in [public/art/mc](public/art/mc); source and animation details are recorded in [the art notes](public/art/README.md).

Use WASD or arrow keys, click the ground, or choose the current objective's walking button. Approach the nearest resident and press E or **Talk to Tavi/Mira**. Tavi is at the shop; Mira is beside the supper table on the right of the square. Collecting gives you two loaves and twenty coins once. Delivering removes the loaves and lights the evening lanterns. Returning to Tavi completes the chapter with an authored callback to the delivery and your saved bread preference. You can keep exploring and talking afterward.

Selecting a suggested reply fills the input; you can edit it before sending. Meaning and reading controls reveal help. Exploration pauses during a conversation or journal. Press Escape or **Back to the village** to leave a conversation.

Japanese is the initial route; the language switch creates a separate Mandarin save with separate chapter progress. Tavi and Mira keep their own witnessed memories. The notebook keeps useful phrases and remembered facts from the current resident.

Saves live in the local .anima-village/village.sqlite3 file, independently of assistant data. The browser stores per-language save identities and unresolved conversation request payloads. After a lost response, retry uses the same request ID and exact payload, including the original resident. Existing saves retain quest, language, journal and memory progress. Legacy traveler fields remain in the save wire contract but do not affect the main character; retired local appearance request metadata is cleared when its save opens. This prototype is local to your machine; it has no payment flow or public multiplayer service. Generated Japanese/Mandarin and reading quality still need review by qualified speakers before a customer pilot.

## Verification

```sh
bun run test:web
bun run typecheck
bun run build:web
bun run lint:web
bun run test:host
```

The supplied main character's provenance, generated NPC art prompts and layout notes are in [public/art/README.md](public/art/README.md).

`bun run test:visual` checks a deterministic full chapter fixture, old appearance metadata, reviewed Mira recovery and desktop/mobile layouts. `bun run test:live` checks direct exploration, both live NPCs, collection, delivery, the reviewed callback, reload persistence and Japanese/Mandarin separation against a running isolated game host. Both assert that the creator/editor is absent and no appearance request is sent. Both accept `VILLAGE_URL` (default `http://127.0.0.1:24302`) and `VILLAGE_TEST_OUTPUT`; default screenshots live under the PoC's `test-output/`. They close their own browsers and do not start or stop servers. The live check creates real saves and consumes normal model usage; run it against a separate test database.
