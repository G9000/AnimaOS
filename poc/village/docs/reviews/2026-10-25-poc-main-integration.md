# Village PoC integration verification

The user authorized a local merge into main as a PoC folder on 2026-10-25. This packages the already-playable fixed-MC language RPG under `poc/village/`; it does not change the assistant product or publish the game.

## Isolation and preservation

- All deliverable source, assets, scripts, manifests, locks and game documentation are inside `poc/village/`.
- The PoC has one private Bun package and its own lockfile. Root package files are unchanged, and normal Bun workspace enumeration excludes it.
- The PoC has a separate two-member Cargo workspace for its domain and game host. The normal Cargo workspace remains at 13 members and excludes the game. Shared dependency versions match the existing root lock; only the two game packages are new.
- `web/vite.poc.mts` and `vitest.poc.mts` use explicit configuration paths so root recursive test discovery does not include the experiment.
- Reusable engine and generic process/task helpers are reused through repository-relative paths. No production engine, client, host-selection or daemon behavior changed.
- The old game process was stopped before moving source/data. All 13 existing saves and 33 receipts were preserved with matching file hashes and a passing SQLite integrity check. The logical save snapshot digest stayed `8a4375fa59230151f230ab3a697d71c3e13b4c4a449f96d7824c073573622a66` throughout pre-merge checks.
- Local dependencies, build output, test output, scratch files and personal saves are ignored. No credentials or save databases belong to the commit.

## Pre-merge checks

| Check | Result |
| --- | --- |
| PoC `bun run test:web` | 36 tests across 7 files passed |
| PoC `bun run typecheck` and `bun run build:web` | Passed |
| PoC `bun run test:host` | 18 domain and 27 host tests passed |
| PoC `bun run build:host` | Passed |
| PoC `bun run lint` | Frontend lint, Rust formatting and scoped Clippy passed |
| PoC frozen offline Bun install | Passed without lock/manifest changes |
| Root `bun run --filter workspace-dev test` | 48 normal launcher tests passed; the sandbox-only attempt lacked required symlink/process permissions and the authorized OS-permission rerun passed |
| Root `bun run rust:test` | 4,897 passed, 0 failed, 47 ignored across 69 groups, using the existing isolated validation Cargo output directory |
| PoC `bun run test:visual` | Passed full fixed-MC route, legacy/null traveler, persistence, language routes and reviewed NPC recovery; no browser errors |
| PoC `bun run test:live` | Passed generated Tavi/Mira conversations, collection, honey preference, delivery, authored evening callback, reload and Mandarin/Japanese separation; no browser errors |
| Packaging/spec/quality review | Approved |
| Root file and membership checks | Production tracked files match HEAD; root Cargo/Bun exclude PoC; personal saves remain ignored |

The known LF-sensitive baseline fixture was normalized only for the full Rust run, then restored byte-for-byte to its original checkout. No fixture change remains. The main daemon stayed running; no main-server executable was rebuilt or stopped by the PoC checks.

The game retains its existing prototype limitations, including qualified review of generated language before a customer pilot. Frontend builds succeed with the existing Three.js bundle-size warning (about 834 kB minified / 227 kB gzip).

## Local main handoff

Source commit `f4d6ab19` was fast-forwarded locally into `main`. On the merged checkout, frozen offline install, all 36 frontend/tooling tests, all 45 game Rust tests, both builds, full PoC lint and the visual quest walkthrough passed. Only `poc/village/**` changed.

The preserved save directory was copied into the main PoC with matching transfer hashes and passing SQLite integrity. All 13 saves and 33 receipts remained intact; every save was restored through the real main PoC API with matching inventory, coins and phase. Its snapshot digest still matches the value above.

The game is running from the main PoC at <http://127.0.0.1:24302/> with API28192 and the existing live Anima connection. The main application server was left alone. Temporary verification files were removed; saves, dependencies and build caches were retained. No push was performed.

Use the [PoC README](../../README.md) for current commands. Historical earlier reviews and plans describe the prior workspace locations; their banners identify that history.
