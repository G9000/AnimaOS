# Snapshot comparison repair

Repair only `src/compare-snapshots.mjs`. It exports `compareSnapshots(before, after)` and returns `{ added, removed, changed }` with ASCII-sorted path arrays. Input records contain exactly `path`, `bytes`, and `sha256`; reject malformed inputs with an ordinary Error. Added and removed paths occur in only one snapshot. Changed paths occur in both snapshots with different bytes or hash. Do not mutate inputs or perform I/O in the module.

The six visible tests are in `test/compare-snapshots.test.mjs`. Run `node verify.mjs` from this directory. It runs the tests and, only if all six pass, calls the fixed CLI to write `changes.json` from `data/before.json` and `data/after.json`. The CLI-generated report is the only permitted new file. Read its full contents after the successful verifier. No package installation is needed.
