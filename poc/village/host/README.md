# Village game host

This loopback Axum host runs Tavi and Mira through `anima-harness` and keeps village saves in a separate SQLite database. The game rules and dialogue integration live in [`../crates/anima-village`](../crates/anima-village/); that library has no HTTP or database dependency. It reuses the repository's host-agnostic harness through a path dependency.

From `poc/village`, start the complete game with `bun dev`. For host-only development, use `bun run dev:host`. Use `bun run test:host`, `bun run build:host`, or `bun run lint` for validation. See the [PoC README](../README.md) for package boundaries, listener overrides and all commands.

## Existing Anima model connection

By default the host reads the saved `Anima` agent's provider/model from `~/.animaos-swarm/data/control-plane.json`. It does not create an assistant agent, modify settings, use assistant conversations/memory, or write to assistant storage. `VILLAGE_ANIMA_PROFILE` selects a different existing `.animaos-swarm` directory; `VILLAGE_ANIMA_AGENT` selects an existing agent by name. An existing `ANIMA_HOME` or `ANIMAOS_RS_CONTROL_PLANE_FILE` is respected.

For ChatGPT, the host reads the same OS keyring service and bounded credential layout as the Anima daemon, then supplies `ChatGptResponsesAdapter` to the harness. It re-reads the credential for each model call. It never logs, returns, refreshes, migrates, or writes credentials. An expired connection requires renewal in Anima before restarting the game. `VILLAGE_KEYCHAIN_NAMESPACE` (or the inherited `ANIMAOS_RS_KEYCHAIN_NAMESPACE`) selects an already configured namespace.

For API providers, the host reads the selected provider's existing environment variable or ready key from `data/provider-keys.json`. `ANIMAOS_RS_PROVIDER_KEYS_FILE` selects the existing key file. `VILLAGE_ENV_FILE` can point to the existing Anima `.env`; it is read in memory without changing process or assistant configuration. Simple `KEY=value`, single/double quoted values, and `export KEY=value` are supported. Provider base-URL environment variables are respected. The game does not update the assistant key-pool usage or rest state.

Set **both** `VILLAGE_PROVIDER` and `VILLAGE_MODEL` to explicitly select another existing connection. The game never switches providers automatically. `VILLAGE_REVIEWED_ONLY=1` explicitly disables generation. Missing/expired credentials use visible `reviewed` metadata and server-authored choices; they are never reported as model dialogue. A failed or timed-out live request remains pending and returns an error. Retry the exact request or explicitly choose reviewed recovery; failure does not finalize a generic no-op reply.

From `poc/village`, inspect redacted availability with `bun run dev:host -- --connection-status`. `bun run dev:host -- --live-smoke` performs one genuine generated greeting through the existing connection and consumes normal model usage. Neither command writes game or assistant state. The wrapper selects this PoC's Cargo manifest explicitly.

## Runtime and storage

- `VILLAGE_API_PORT`: launcher and host script API port, default `28192`. The browser port defaults to `24302`; `VILLAGE_WEB_PORT` overrides it. Both ports are validated centrally.
- `VILLAGE_BIND`: low-level host loopback address. The launcher/host script supplies `127.0.0.1:28192`; a direct Cargo invocation falls back to `127.0.0.1:8092`.
- `VILLAGE_DB_PATH`: separate village database. The launcher and host script use `poc/village/.anima-village/village.sqlite3`; a direct Cargo invocation falls back to `~/.animaos-village/saves.sqlite`. Set a separate path for acceptance tests.
- `VILLAGE_GENERATION_TIMEOUT_MS`: default `18000`; allowed range `1000..60000`. The browser offers reviewed recovery after eight seconds.

The frozen `/api/village` API creates/restores saves, records visits, and accepts messages or reviewed choice ids. Saves use unguessable ids and remain local. Possession of a save id permits access to that save; this prototype has no account login or public-hosting support. JSON bodies are bounded to 16 KiB and free text to 1000 characters.

SQLite stores the authoritative snapshot and committed request receipt in one immediate transaction. Pending request identity is persisted before model work starts. An exact retry of a completed request returns its original result; changing a payload under the same request id returns 409. An exact retry after a failed attempt can claim a fresh bounded generation with the same identity. Recovery uses a new request id and a reviewed choice, invalidates the original generation token, and commits one outcome. If generation committed first, recovery returns that original result. If recovery committed first, late output cannot overwrite it. Pending requests survive process restart and can be retried or recovered. A separate `.owner.lock` lease prevents two hosts from opening the same village database; startup releases abandoned generation ownership only after taking that lease. Model work never holds a database transaction or a global save lock.

Only Mira's two-loaf order grants inventory and its 20-coin reward, once. Collection is recorded independently of remaining inventory, including legacy saves with two loaves. Delivering both loaves to Mira removes the bread, retains the coins and starts evening. Returning to Tavi commits the authored supper callback and completes the chapter; the completed village remains conversational. Preferences and encouragement never change the reward. Free text is interpreted into bounded collection or delivery claims; the rules validate the recipient and quantity. Ambiguous delivery, wrong speakers and repeats cannot advance or reset progression.

Tavi and Mira keep separate bounded conversations and witnessed facts. Mira knows her supper order but does not receive private preferences or encouragement told to Tavi. The saved traveler name is supplied as data. The model has no tools and cannot submit world state. Generated translation/reading quality still needs human language review.

## Village-day API additions

POST /api/village/saves/{id}/traveler accepts {requestId, traveler}. The traveler has name (1–24 trimmed Unicode characters, without controls), skinTone (fair|warm|deep), hairStyle (short|bob|braid), hairColor (chestnut|ink|gold|auburn), outfit (cloak|tunic|overalls), and outfitColor (moss|ochre|berry). Customization validates before admission, runs without model work, and writes the snapshot and receipt atomically. It preserves all progress and the last dialogue. A pending conversation blocks a new customization; resolve that request before onboarding an older save. Retry the exact customization after a lost response; its original receipt remains stable after later progress.

Visits and turns accept optional npcId: tavi|mira; omission defaults to Tavi and retains the legacy request identity. Recovery must name the original speaker. Reviewed choices retain Tavi's existing IDs and add deliver_order and ask_supper for Mira. Wrong-speaker choices never grant or deliver bread.

Every GameView includes traveler (nullable), npcId, and phase (day|evening|complete). questStatus is pending|collected|delivered; inventory reports actual remaining bread. Old snapshots and receipts receive safe defaults when read; replay does not rewrite newer progress. The evening callback emits authored target-language dialogue with any saved Tavi preference/encouragement. In live mode, usable generation must finish before that candidate commits; a failure leaves evening pending. Reviewed recovery of that visit commits the same callback and counts the entry once.
