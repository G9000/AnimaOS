# Lantern Village PoC

An experimental local browser RPG using Anima's reusable Rust agent harness. Explore the village as the supplied pink-haired main character, collect Tavi's two loaves, deliver them to Mira, then return for an evening conversation. Japanese and Mandarin have separate saves. This PoC has its own Bun package, Cargo workspace, lockfiles and save database.

From the repository root:

```sh
cd poc/village
bun install
bun dev
```

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
