# Outcome execution implementation plan

> For agentic workers, use the resolved design below and the [outcome task boundaries](../../companion/outcome-tasks.md). The coordinator alone runs native validation. No commits or publication in this batch.

**Goal:** Let owners attach public success criteria to a native task, inspect actual output, make at most two bounded repairs, and distinguish evidence from owner acceptance.

**Architecture:** Embed a bounded outcome record in RunRecord. One outcome journal publishes outcome-only patches. Existing native execution, grants, action receipts, shared budgets and jobs/workflows remain authoritative.

**Tech stack:** Rust core and daemon, existing serde and SHA-256, existing SDK and React console.

- [x] Author core reducer tests, then implement validated definitions, observations, iterations and settlement in `anima-core/src/outcomes.rs`. Core owner exports the module.
- [x] Author retained-child budget tests, then implement one stable child binding plus dispatch reservation in `execution_budgets.rs`. Publish the exact seam early.
- [x] Author host real-byte and repair-fence tests. Implement host objective checks in `outcomes/verification.rs`, journal/evaluator in `outcomes/mod.rs`, and owner routes in `routes/outcomes.rs`.
- [x] Wire optional admission into the existing owner run request and ledger. Reject outcome steering and connector delivery. Keep criteria as public data.
- [x] Persist outcome-only candidate patches before checking or entering repair. All cloned ToolContexts recheck the durable phase before dispatch and after approval. Only exact target file operations and bounded reads are permitted during repair.
- [x] Register the outcome evaluator and terminal finalizer. Require current source/model/task/subject/reply identities. Unknown effects, save failure, Stop, exhausted budgets and unavailable quality engines cannot pass.
- [x] Ensure jobs/workflows never treat an attached semantic failure as accepted success. Owner acceptance checks fresh evidence and report revision.
- [x] SDK/UI owner adds optional Advanced input fields and an existing-run status line. Focused mocked-fetch/UI checks may run only after confirming no native setup.
- [x] Write usage documentation and frozen changed-file manifest. Get bounded independent source review, resolve findings, and hand the integrated tree to the coordinator for native checks.

Native proposed commands, run only by coordinator in the combined copy with the primary absolute validation target and 32 GiB cap:

```powershell
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test -p anima-core outcomes::
bun tools/workspace-dev/scripts/run-task.ts --cwd . --rust-cache validation -- cargo test -p anima-daemon outcome_
bun run rust:test
```

No native red/green result is claimed by this child. Tests are authored before implementation and their execution is deferred by the explicit coordinator constraint.
