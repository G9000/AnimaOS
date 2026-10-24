# Tavi first playable verification

> Historical verification from before relocation into `poc/village`. Test results, commands, listener addresses and Git/process state below describe that milestone. Use the [current PoC documentation](../README.md) for setup and current paths.

Verified on Windows on 2026-10-25, in the isolated `g9000/tavi-first-playable` worktree. This delivers the first shop milestone in the v1.1 product draft; the three-chapter paid pilot remains future work.

## Delivered behavior

- Three.js shopfront with sprite movement, proximity interaction, lanterns, greenery, and a preference-dependent counter plate.
- React portrait conversation with editable suggestions, Japanese/Mandarin routes, English meaning, readings, and a phrase notebook.
- Authoritative Rust order validation: Mira's two loaves grant one basket and 20 coins. Ambiguous or wrong requests cannot grant inventory.
- Tavi's recipe concern, optional honey/sesame choice, and a remembered next visit. Language routes use separate saves.
- Actual NPC generation through `anima-harness`, using the existing Anima primary connection (`chatgpt/gpt-6-luna`) on the server. No assistant daemon dependency and no credentials in the browser.
- SQLite save snapshots and request receipts, bounded generation, exact retries, reviewed recovery, and reconciliation after an interrupted browser request.

## Verification evidence

| Check | Result |
| --- | --- |
| `bun run --filter @animaos/village test` | 15 tests passed across 3 files |
| `bun run --filter @animaos/village typecheck` | Passed; also rerun by the final production build |
| `bun run --filter @animaos/village build` | Passed |
| `bun run --filter @animaos/village lint` | Passed |
| `bun run --filter village-game test` | 11 domain and 18 host tests passed |
| `bun run --filter workspace-dev test` | 50 tests passed |
| `bun run --filter workspace-dev build` and `typecheck` | Passed |
| `bun run rust:test` | 4,897 passed, 0 failed, 47 ignored across 69 suite groups |
| New Rust crates: formatting, build and scoped Clippy | Passed |
| `bun apps/village/e2e/visual.mjs` | Rendering and conversation fixture checks passed |
| `bun apps/village/e2e/live.mjs` | Actual live-model browser acceptance passed with no page or console errors |
| Plan, spec and code quality reviews | Passed after the retry, rejection, cache and content findings were fixed |
| `git diff --check` | Passed |

The final live browser run covered a generated Japanese greeting, free-text collection of Mira's order, a honey preference, page reload, a generated remembered visit, a separate generated Mandarin visit, and a switch back to the completed Japanese save. Every conversation response reported `source: model`; the engine reported `anima-harness`. Inventory and preference survived reload. Screenshots were inspected for the exploration view and illustrated dialogue; generated verification screenshots and logs were then removed.

Request tests cover duplicate replay, changed-payload conflicts, separate saves, database reopening, both generation/recovery winner orders, timeouts, disconnect continuation, persistent pending client requests, pre-admission rejection, language switching and IME-safe submission.

The full Rust run initially exposed an existing fixture that compares a literal LF string against a Windows CRLF checkout. Normalizing only `packages/core-rust/acceptance/provider-task-m52/fixture/src/log-summary.mjs` to LF allowed the unmodified test to pass; its source content was unchanged. The original checkout line endings were restored after validation. Rust validation used `CI=1` and the original checkout's shared `target/cargo` cache to avoid a duplicate compile. These temporary validation settings are not required by the game.

## Handoff and limits

Run `bun install`, then `bun run village` from this worktree. Open <http://127.0.0.1:4302/>. The launcher owns the browser and dedicated game host; saves remain in `.anima-village/village.sqlite3` in this worktree. The current local launcher is left running for review. Nothing was committed, pushed, merged or published.

This is a local first playable. Generated Japanese, Mandarin, translations and readings still need qualified language review before a customer pilot. There is no payment flow, public account system or production hosting. The Vite build warns about the combined Three.js bundle size (about 825 kB minified, 224 kB gzip); it builds successfully. Latency and cost at ten concurrent players have not been benchmarked.

Original generated asset layouts and full prompts are recorded in [the art README](../../web/public/art/README.md). Start and control instructions are in [the app README](../../web/README.md).
