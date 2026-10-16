# Duplicate inventory repair

Repair only `src/find-duplicates.mjs`. It exports `findDuplicates(records)` and returns `{ groups, redundantBytes }`. A group contains at least two records with both equal bytes and equal SHA-256, and is `{ paths, bytes, sha256 }`. Sort paths and groups by their first path using ordinary string order. Count `(group.paths.length - 1) * group.bytes` redundant bytes. Inputs contain exactly `path`, `bytes`, and `sha256`; reject malformed or duplicate paths with a bounded ordinary Error. Do not mutate inputs or perform I/O in the module.

The six visible tests are in `test/find-duplicates.test.mjs`. Run `node verify.mjs` from this directory. It runs the tests and, only if all six pass, calls the fixed CLI to write `duplicates.json` from `data/inventory.json`. The CLI-generated report is the only permitted new file. Read its full contents after the successful verifier. No package installation is needed.
