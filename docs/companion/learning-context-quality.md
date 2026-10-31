# Learning and context quality

For isolated tasks that use tools and change real fixture files, use [complete-task regression comparisons](task-regressions.md). Those owner reports are separate from the response-only skill promotion gate described below.

The Rust daemon can evaluate a proposed skill before promotion, save a small continuity checkpoint during compaction, omit selected old tool output from model requests, and check whether completion evidence covers the changed file versions. Approval, tool grants and exact Undo remain separate controls.

## Evaluate before promotion

A local owner can GET or PUT `/api/skills/{slug}/evaluation-suite`. A suite is opt-in; an absent or disabled suite preserves existing behavior. Enabling it authorizes bounded model calls when a draft is evaluated or promoted. Set `enforced: false` to disable it.

A suite names an existing ordinary agent and supplies independent train and heldout fixtures. For example:

```json
{
  "enforced": true,
  "agentId": "your-agent-id",
  "minImprovement": 1,
  "trialsPerFixture": 1,
  "maxOutputTokens": 512,
  "trialTimeoutMs": 30000,
  "fixtures": [
    {
      "id": "train",
      "heldout": false,
      "required": true,
      "prompt": "Apply the procedure to a training example.",
      "oracle": { "kind": "exactText", "expected": "owner-verified answer" }
    },
    {
      "id": "heldout",
      "heldout": true,
      "required": true,
      "prompt": "Apply the procedure to a different example.",
      "oracle": { "kind": "canonicalJson", "expected": "{\"result\":42}" }
    }
  ]
}
```

Use domain fixtures and expected answers checked independently of the candidate. The example shows the wire format; it is not a useful evaluation suite by itself. `exactText` compares all returned text exactly; `canonicalJson` compares parsed JSON values. Expected answers are withheld from the model requests.

POST `/api/skill-drafts/{draft_id}/evaluation` with the same optional `body`, `files` and `hash` used for approval. This previews the effective whole package, including supporting files and owner edits. GET the same path for its latest report; POST its `/cancel` suffix to stop it logically. The SDK exposes `skills.evaluationSuite`, `setEvaluationSuite`, `evaluateDraft`, `draftEvaluation` and `cancelDraftEvaluation`.

Each baseline/candidate pair uses fresh, tool-free requests with the same pinned current agent/provider/model/settings. Trials use no persistent session, memory, plugins, fallback or side model. A new skill has a no-skill baseline; an existing skill requires the actual complete approved baseline. Required cases cannot regress; every heldout candidate case must pass; the candidate must meet the configured improvement count. Missing, failed, truncated, tool-requesting or oversized trials cannot pass. Reports explicitly describe response-only evidence; they do not establish filesystem effects or general correctness.

Enforced suites gate Agent, Import and File draft promotion, including Free activation. A passing preview does not approve anything. Promotion rechecks current suite, workspace, source, baseline, effective package and model identity under the existing mutation transaction. Direct owner authoring/approve-changed and exact Undo remain authoring/restoration controls and invalidate old evidence.

Limits are 16 fixtures, 1–3 repetitions, 4096 output tokens, 1–30 seconds per trial, 120 seconds per evaluation, eight durable reservations per rolling day, 32 retained reports, and one physical worker per daemon. Evaluation currently refuses the ChatGPT subscription transport before reserving work because that transport does not send an output-token control. Use a supported provider for bounded evaluation. Provider usage can be unknown; these are request/deadline limits rather than a dollar cap. Cancelling or timing out cannot release the physical slot while its transport is still retiring. Drain counts that work, and shutdown waits for retirement within the existing shared run deadline. If that deadline expires, late provider work and usage remain unconfirmed. Restart interrupts saved active evaluations and never replays their model calls.

## Continuity before compaction

The existing tool-free summarization call can return a bounded summary plus a structured checkpoint of the historical goal, decisions and open work. The daemon binds entries to messages from the historical compaction span and retains provenance and integrity-checked artifact references. The current admitted input is excluded. Summary and checkpoint are accepted together before rebuilding the request; stale state and failed saves preserve the previous saved continuity.

A checkpoint is untrusted session data, with a separate prompt budget. It cannot approve an action, grant a tool, become an owner instruction, or write a durable fact. Plain legacy summaries remain supported. Invalid checkpoint data leaves the previous checkpoint available.

## Omit old output from model request copies

Set `settings.toolOutputPruning` to `true` for opt-in pruning. It applies only to positively admitted historical tool results from completed runs. Recent/current results, errors, incomplete output, pending approval, unresolved receipts, attachments, unknown provenance and broken call/result pairs stay intact. Inline output needs native current-workspace provenance; retained artifacts are checked against their immutable store.

Every model iteration projects a fresh request copy. A short marker preserves message/run/call identity, size, digest and any verified artifact ID. The duplicated result payload is also omitted from that copy. Canonical messages, run deltas, saved history and private artifact bytes retain their original representation. Existing history/artifact read permissions still apply. Legacy inline output without native scope remains unchanged.

## Bind completion checks to file versions

Keep the existing `settings.completionReview: true` opt-in. Declare the exact commands relevant to the files using `settings.completionVerificationContracts`:

```json
[
  {
    "id": "rust-host-tests",
    "command": "bun run --filter rust-daemon test",
    "cwd": ".",
    "files": [],
    "directories": ["hosts/rust-daemon", "packages/core-rust"]
  }
]
```

Contracts come from the original accepted owner configuration. Commands and resolved working directories must match exactly. Paths name explicit files or component-boundary directories, without globs. These are owner declarations of relevance, not proof of test semantics. Unknown commands still follow existing execution controls and contribute no inferred coverage.

The daemon snapshots bounded actual file bytes before and after the check, and again before completion. Evidence covers only the matching observed versions. All matching declared contracts are required; separate checks can cover different files. A newer failed/unknown attempt, a stale read, an outside edit, an ambiguous path or an unreadable/oversized file leaves a visible gap. Unrelated green commands cannot clear it. The review gives one bounded advisory reminder and permits an honest limitations report; it never runs a command or grants permission.

Pre/post hashes cannot detect a transient outside edit followed by exact restoration. The observed-file ledger cannot discover every file created by opaque shell commands. Keep those limits separate from a correctness claim.
