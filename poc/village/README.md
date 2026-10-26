# Lantern Village PoC

An experimental local browser RPG using Anima's reusable Rust agent harness. Explore the village as the supplied pink-haired main character, collect Tavi's two loaves, deliver them to Mira, then return for an evening conversation. Japanese and Mandarin have separate saves. This PoC has its own Bun package, Cargo workspace, lockfiles and save database.

Install the game dependencies once inside `poc/village` with `bun install`. Then start the game from the repository root:

```sh
bun run dev:game
```

You can also run `bun dev` directly inside `poc/village`. Root `bun dev` starts the assistant daemon/API and web console; `bun run dev:all` includes the game, playground and both docs sites.

Open [http://127.0.0.1:24302](http://127.0.0.1:24302). The launcher starts only the Village browser and its Rust host at `127.0.0.1:28192`; the first Rust build can take a while. It reads the existing Anima model connection on the server. Credentials stay on the server, and game saves and NPC memories are separate from assistant data. See [connection setup and overrides](host/README.md).

Saves live at `poc/village/.anima-village/village.sqlite3`. The browser retains a save identity per language and pending conversation requests. Keep the database and its sidecar files together while stopped. Never run two hosts against the same save database. `VILLAGE_DB_PATH` selects a separate database for testing; `VILLAGE_API_PORT` and `VILLAGE_WEB_PORT` override the launcher ports and are validated together.

Run these commands inside `poc/village`:

| Command | Purpose |
| --- | --- |
| `bun dev` | Start the game host and browser together |
| `bun run dev:web` | Browser only; proxy uses the centrally configured game API |
| `bun run dev:host` | Game host only, with the same default save path and API port |
| `bun run build` | Typecheck/build the browser and build the two game Rust crates |
| `bun run build:web` / `bun run build:host` | Build one surface |
| `bun run test` | Browser/tool tests and both game Rust test suites |
| `bun run test:web` / `bun run test:host` | Test one surface |
| `bun run typecheck` | Check browser, launcher and configuration TypeScript |
| `bun run lint` | Browser/tool lint, Rust format check and scoped Clippy correctness |
| `bun run lint:web` / `bun run fmt:host` | Browser/tool lint or Rust format check |
| `bun run test:visual` | Deterministic browser chapter check against a running game browser |
| `bun run test:live` | Real model browser chapter check against a running isolated game host |

The E2Es accept `VILLAGE_URL` and `VILLAGE_TEST_OUTPUT`, close their own browsers, and leave server ownership to the caller. Use a separate `VILLAGE_DB_PATH` for live acceptance: it creates real game saves and consumes normal model usage. Reports and screenshots default to `poc/village/test-output/`. Task scripts use the repository's generic scratch-directory wrapper. Rust outputs use the repository's `target/cargo` cache unless `CARGO_TARGET_DIR` is supplied; game commands select this PoC's explicit Cargo manifest and build only its two members.

`web/` contains the React/Three.js game and original artwork, `host/` the loopback game server, `crates/anima-village/` the game rules and NPC integration, and `tools/` the launcher. The game embeds [`anima-harness`](../../packages/core-rust/crates/anima-harness/) through a path dependency. Normal root Bun workspaces, root Cargo members and automatic root test discovery exclude this PoC. The explicit `vite.poc.mts` / `vitest.poc.mts` names are intentional.

See [game controls and art](web/README.md), [host behavior](host/README.md), and [documentation](docs/README.md). Generated language, translations and readings still need qualified review before a customer pilot. The broader product draft describes future work; this local chapter has no payments, public hosting or multiplayer.

## Persistent resident integration (gated)

Normal executable startup still uses the legacy NPC profile. An embedding host or isolated HTTP fixture can explicitly select `Service::with_durable` with a `DurableVillage` constructed from the shared SQLite execution and memory adapters, an injected model and a clock. This constructor reads no provider credentials or user configuration. Existing request admissions retain their selected profile; an unavailable durable profile cannot retry its pending work through the legacy model loop.

The asynchronous `with_durable` attachment pins both persistent store identities in one game transaction. Replacing either database, or losing the pin while durable history exists, fails before model work. The game database stores owner/player/world/resident/session identities, immutable interpretation and narration bindings, and the shared request allowance before dispatch. The original generation token still owns the game commit. Accepted observations and pending memory effects commit with that winning receipt, then project idempotently into private resident memory. A reviewed recovery records a distinct reviewed winning-source observation and can admit the supper exchange; the abandoned generated result cannot become memory. The next dependent turn requires projection readiness. Explicit corrections and forgetting use the same outbox through `POST /api/village/saves/{id}/memories`; legacy cached conversations are excluded from durable recall.

An accepted supper delivery admits one authored Tavi/Mira exchange with at most two replies, one resident reply per visible-session activation, and a shared finite allowance. Its utterances are frozen public facts; as with the existing chapter callback, a usable generation is required but generated text cannot disclose a private player claim. Each observer retains its own sourced episode and directed relationship evidence. The browser opens a server-issued presence, renews it while open, closes it on page exit, and displays the exchange in the notebook. Server expiry and restart stop new replies; reopening resumes the same interaction within its original allowance, without offline world-time catch-up.

Memory lifecycle actions use exact authorized IDs, independently of bounded recall windows. Corrections require an active target; forgetting also accepts authorized expired or superseded records. Forgetting suppresses recall while retaining canonical audit history and historical game receipts.

The host tests use isolated temporary saves and scripted providers. They include real child-process termination/reopen at engine-result, accepted-event and between-reply boundaries, exact replay, lost projection acknowledgement, private scope isolation, reviewed recovery and memory lifecycle changes. These checks do not establish live-provider behavior or a player walkthrough. The host-only clock capability fixture also verifies the real HTTP path, manifest inventory, shared allowance, exact resident/world policy, and denial before a filesystem executor; that constructor-injected profile is excluded from executable builds. Executable activation and migration of historical private save flags into canonical memory remain gated; old save snapshots and receipts remain readable and are not imported as facts witnessed by every resident.


### Explicit persistent executable startup

The executable retains its existing behavior unless
`VILLAGE_DURABLE_CONFIG_FILE` points to a version-1 JSON file containing
`storage` (execution, memory, owner, key absolute paths), an absolute
`saves` SQLite path, and `mode` (active or draining). Preprovision the UUID
owner file and separate raw 32-byte random seal key. The selected existing Anima
connection must be available; explicit durable startup fails instead of choosing
reviewed/legacy generation when it is unavailable. The same read-only model
adapter instance is retained for the durable residents and legacy request reader.

Draining keeps the persistent engine and canonical memory available for accepted
requests, exact replay, reviewed recovery and memory projection. It blocks new
conversation requests and new supper activations; stages already accepted by an
existing supper interaction may settle. Heartbeats can retry accepted outbox
projection for pending and terminal activities. Existing save authority, store
attachment and protection are checked before reopen can clear pending tokens or
change admission mode. A copied legacy save is not automatically activated by
this executable configuration; its explicit fixture migration path remains an
embedding operation.

The isolated launcher documented in `hosts/rust-daemon/README.md` prepares fresh
fixture-only files and reuses only the source provider/model selection. No real
saves or assistant personality/history are imported. It supports an explicit
existing credential namespace and never refreshes or writes credentials through
the game's read-only subscription adapter. Close with Ctrl+C to request bounded
worker draining. A forced process termination requires restart review.
