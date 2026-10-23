# M47 independent coding acceptance inputs

`fixture/` is the entire candidate-copy template. Only
`fixture/src/invoice-summary.mjs` is repairable during the actual useful task.
Its starting implementation deliberately truncates amounts with `parseInt` and
counts void invoices. Visible tests should initially fail for those behaviors.
`out.csv` is the sole allowed generated candidate artifact. Root separately freezes
the CLI, visible tests, input data, README and sentinel bytes/timestamps.

Keep `oracle.mjs`, `oracle.test.mjs`, `expected-manifest.json`, `reference/` and
`neighbor/` outside candidate tool/workspace authority. The independent neighbor
tree is a protected unrelated byte/timestamp witness. An external path does not
itself create OS isolation; root owns actual workspace/tool confinement and hashes.

The oracle compares literal manifest strings, not candidate-computed expected
answers. It checks the sole module export and every case through an actual Node
CLI process. Successful output must be exact UTF-8 bytes. Invalid CSV/arguments
must exit 1 quietly except for the fixed diagnostic and preserve preexisting
output. Each CLI process has a 2-second deadline and 16-KiB capture cap.
This runner executes candidate code: use only the root-owned frozen synthetic copy.

The controller-only reference independently parses rows and uses integer cents;
it is a positive control, never expected-answer authority. Self-tests require the
buggy baseline to fail, the reference to pass, and decimal-truncation, void-count,
UTF-16-sort and CLI-prevalidation-truncation mutants to fail the intended semantic
assertions. The manifest includes primary/changed/reversed input, header-only,
all-void, case/Unicode ordering, large exact cents, CRLF/leading-zeroes and malformed
rows/amounts. Literal expected strings are frozen inputs; root hashes them before
admission and runs the same oracle once on the final candidate and after restart.

The `core-rust` workspace exposes the `coding-acceptance-test` script; no additional dependency is introduced.
Intended target: `bun --no-env-file run --filter core-rust coding-acceptance-test`
executing `node --test packages/core-rust/acceptance/coding-m47/oracle.test.mjs`.
Root must verify the target name/configuration after adding it; these are future
instructions, not passing evidence. Candidate commands are in `fixture/README.md`.
For a frozen candidate, the independent runner interface is:
`node packages/core-rust/acceptance/coding-m47/oracle.mjs <absolute-candidate-directory>`.
Exit 0 means all checks passed, 1 a semantic verdict failure, 2 runner/usage failure.

The fixture is new local synthetic Anima material. Public Hermes at pin
`1fd18277267152893a8a24792b79213e7ca3f899` informed the existing plan's distinction
between actual command exits, prose completion and stale-write evidence. No Hermes
code, profiles or configuration were copied, and no upstream notice was removed.
No provider, runtime integration, process-cleanup or coding-quality pass is claimed
by these source files. Root records actual RED/GREEN and useful-task evidence.
