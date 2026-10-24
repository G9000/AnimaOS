# Village PoC main integration plan

**Goal:** Merge the game locally into main entirely under `poc/village/`, outside normal production workspaces, without pushing or disturbing the main server.

**Approved request:** The user explicitly requested the local main merge as a PoC folder. `poc/village/` is the selected location. Main and the feature branch currently share the same base commit; main is clean. All feature work is still uncommitted.

## Final layout and boundaries

- `web/`: existing React/Three.js game, assets and E2Es.
- `host/`: dedicated Rust game host.
- `crates/anima-village/`: game-specific domain and NPC library.
- `tools/`: game launcher/config/tests and Rust task wrapper.
- `docs/`: product draft, historical plans/reviews and current integration evidence.
- Independent private Bun package and lockfile; no root workspace/script/lock changes.
- Explicitly named `web/vite.poc.mts` and `vitest.poc.mts` avoid the repository's recursive automatic `vite.config.*`/`vitest.config.*` test discovery. PoC scripts pass `--config` explicitly.
- Independent Cargo workspace and lockfile; two game crates only. Reuse `packages/core-rust/crates/anima-harness` through path dependencies. No changes to reusable engine crates or production Cargo members.
- Reuse the existing generic `tools/workspace-dev/src/process.ts` and task wrapper through relative imports. Game port selection stays centralized in PoC tooling; browser packages contain no hardcoded API port.
- Local `.gitignore` protects saves, node_modules, dist, target and test-output. Move the stopped game's current save directory intact; copy it into the main PoC after merge because ignored data is not in Git.

## Tasks

- [x] Stop only the separate game, verify main status/base, inspect existing manifests and current library documentation.
- [x] Move only tracked/nonignored game source files into PoC paths with checked absolute source/destination bounds. Preserve original bytes and ignored user data; leave unrelated caches alone.
- [x] Restore feature-only root workspace/lock/script/PRD edits after their game content is preserved inside PoC. Verify the final Git change is exclusively `poc/village/**`.
- [x] Add independent Cargo workspace metadata/profiles and fix both Rust path dependencies. Resolve a local PoC lockfile without updating versions unnecessarily.
- [x] Add independent Bun package, dependencies via Bun commands, Vite/Vitest/TypeScript configuration, launcher paths, root task wrappers and documented commands. No root package changes.
- [x] Test standalone package scope, frontend behavior/build/typecheck/lint, launcher configuration and 45 Rust game tests. Run the required repository Rust suites in the existing isolated validation output directory while the main daemon keeps running.
- [x] Run the migrated browser/full quest and fixed-MC checks against temporary separate ports and a test save DB; retain one current review image if useful. Review isolation and file/path correctness.
- [x] Record current commands/paths and validation, commit only the PoC, recheck main is clean, merge locally, and repeat the relevant checks on the merged result. No push.
- [x] Preserve the user's game save under main PoC, restart the separate game from main, remove exact scratch outputs after recording, and report main commit/path/start command. Keep the feature worktree until its saved user data is safely preserved.
