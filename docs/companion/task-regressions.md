# Compare complete tasks before changing a configuration

The local owner can save synthetic tasks, compare a baseline and proposed configuration, and read a report of actual fixture files and simulated effects. The task runs through ordinary `AgentRuntime` model and tool turns. This is separate from the existing response-only skill evaluation and does not promote a skill or configuration.

Open an agent's profile and expand **Compare on saved tasks**. The section stays idle until opened. The default form accepts task instructions, an optional sample file, the expected result file and contents, and an expected reply. **Advanced** accepts the same strict JSON format for multiple cases or mock effects. Saving a task makes no model calls.

Select the saved suite and proposed provider, model, instructions and fixture-tool policy. The baseline and candidate reference the same existing agent configuration. Before **Run comparison**, the form explains that public task instructions and sample inputs go to the selected providers and may consume paid credits. That explicit owner action starts the bounded comparison. Expanding, saving, refreshing, reopening or cancelling does not start another comparison. Long version hashes live under **Version details**.

## What the result establishes

Each trial owns a fresh temporary filesystem and runtime, with no canonical conversation, memory, agent creation, account grants, connectors or project-source import. The selected configuration is projected to provider, model, instructions, temperature and bounded execution settings. Personality, knowledge, plugins, fallbacks, extra settings and canonical tools are excluded. The report binds both the selected configuration digest and the actual projected configuration digest.

Fixture tools can read, write and replace independently permitted files, or record a simulated effect. They cannot run shell scripts, choose an ambient directory, read credentials or call a real account. Simulated effects use a bounded key and payload. The same key and payload reuses one mock receipt; a conflicting payload is refused.

Each file mutation captures its exact proposed contents and current file preimage, then reserves the produced bytes against the retained comparison budget before writing. Replacements charge the complete new contents. Cancelled or stale authority prevents the prepared mutation; uncertain filesystem results retain the reserved upper bound. Initial sample files and reads do not consume the output-byte allowance. Simulated effects preserve their first execution order.

The trusted controller holds private expected values outside actor requests, tools, filesystem and feedback. It seals observations after the actual tool and adapter producers retire. Independent deterministic checks compare the whole file tree, final reply and mock-effect sequence. Expected files must include unchanged input files too. The simple form preserves these automatically.

The core comparator requires the exact declared case/repetition/variant matrix. Every frozen observation carries its complete admitted variant pin, including source and tool-policy digests. The trace must also match the side's served provider/model and effective settings. Missing, duplicate, substituted, incomplete or incompatible observations cannot form an accepted comparison. Changing a recorded failure list cannot override the observed file contents.

`completed` means the full matrix finished. It does not mean every task passed. Baseline and candidate pass counts and individual failures remain separate. An explicit A/A control is allowed. Counts are descriptive fixture evidence, without statistical improvement claims or automatic promotion.

Deterministic/test providers are labeled runtime plumbing. Other runs exercise the selected adapter on the declared fixtures; that label alone is not proof of general live-provider quality. Scripted native acceptance derives numeric answers only from actual public file-tool results and is plumbing evidence, not model-performance evidence.

This first runner supports deterministic files, output and mock-state grading. Arbitrary or registered executable code checks, model grading, human acceptance, physical document rendering and spreadsheet recalculation are not configured here. Their dimensions are unsupported or inconclusive. Artifact references have only a report SHA-256 and bounded observation locator. They confer no quality verdict, grant or approval. A sealed fixture never borrows a canonical `RunLink` or document grant to obtain inspection evidence.

## Owner API

All operations require existing local-owner authorization and return `Cache-Control: no-store`.

| Method | Path | Result |
| --- | --- | --- |
| GET | `/api/task-regression-suites` | Saved suite summaries, without private oracles |
| POST | `/api/task-regression-suites` | Save an immutable structured suite; 201; no dispatch |
| GET | `/api/task-regression-configs/{agent_id}` | Current source digest and visible provider/model/instructions |
| POST | `/api/task-regressions` | Explicitly start or join a comparison; 202 |
| GET | `/api/task-regressions/{id}` | Read current applicability, observations and comparison |
| POST | `/api/task-regressions/{id}/cancel` | Empty object; logical cancellation with physical custody retained |

A minimal saved suite looks like this. Its expected values are independent controller data, not actor instructions.

```json
{
  "name": "Add two public numbers",
  "fixtures": [{
    "id": "sum",
    "actor": {
      "prompt": "Read input.txt, add its integers, save the sum to answer.txt and reply with only the sum.",
      "initialFiles": {"input.txt": "17 -4 32"},
      "readableFiles": ["input.txt"],
      "writableFiles": ["answer.txt"],
      "mockEffects": false
    },
    "oracle": {
      "expectedFiles": {"input.txt": "17 -4 32", "answer.txt": "45"},
      "expectedOutput": "45",
      "expectedMockEffects": []
    }
  }]
}
```

After capturing a configuration, start a comparison with the returned suite and configuration digests. Supported candidate changes are provider, model, instructions and the compiled fixture-tool policy. Unknown fields are rejected. Omit an unchanged `system`; an explicit empty string is an intentional empty instruction override.

```json
{
  "suiteId": "trs_<returned-id>",
  "suiteDigest": "<returned-suite-sha256>",
  "baseline": {"agentId": "<existing-agent>", "configDigest": "<captured-digest>"},
  "candidate": {
    "agentId": "<same-existing-agent>",
    "configDigest": "<captured-digest>",
    "model": "<proposed-supported-model>",
    "toolPolicy": "files"
  },
  "repetitions": 2,
  "requestKey": "<stable-key-for-this-owner-action>"
}
```

The SDK exposes `client.taskRegressions.suites`, `define`, `configuration`, `compare`, `report` and `cancel`. It is a thin owner transport. No SDK method executes tasks locally.

Validation returns 422 for invalid bounded domain input, 400 for malformed/unknown JSON, 404 for missing resources, 409 for stale selections or admission conflicts, and 503 for unavailable durable storage. Reports expose exact source/effective configuration, evaluator, fixture-tool implementation, host-source and plan digests. The source fingerprint declares its compiled source coverage; it is not an executable attestation.

## Bounds, cancellation and recovery

Suites hold 1–16 cases, with 1–3 repetitions, default one in the owner form. Case paths are flat portable ASCII filenames. Traversal, separators, alternate streams, reserved Windows names, linked/nonregular files and case aliases are rejected. Each file is at most 64 KiB, at most 64 filenames participate in a case, and a canonical serialized suite is at most 4 MiB. Saved suite capacity is 16. The API request parser has its own finite body bound.

One physical comparison worker runs at a time, with eight durable reservations per rolling day. The logical comparison deadline is 120 seconds. Output tokens are capped at 2048 per model step and tool iterations at 12, intersected with lower selected limits. Existing shared execution budgets remain authoritative, including model/tool attempts, tokens, estimated cost and original deadlines. Monthly-spend admission also remains in force.

Every trial receives a distinct, durably admitted child identity within the same comparison scope. A new trial, correction, helper or restart does not mint allowance. Admission and request identity are persisted before dispatch. The same request key joins the original result. Report retention is bounded at 16; up to 1024 lightweight admission bindings remain after report eviction so an old retry cannot start another paid run. At that bound, new admission stops. An intentional new comparison needs a new request key and reservation.

Logical cancellation, runtime repetition rejection and deadline failure make output ineligible. The host's owned stream producer continues to retain physical polling and late usage. Further attempts are denied when the runtime consumer closes. The physical slot and fixture workspace remain owned until those producers retire. Late output cannot revive a cancelled comparison. Unknown usage remains unknown and native reservations remain conservative.

Already incurred usage stays in the normal durable usage outbox if a trial save fails. A later retirement save can acknowledge those rows without replaying the provider. A report does not expose a completed comparison or complete usage while final persistence is pending; a failed final acknowledgement leaves the result inconclusive. Local producer retirement does not prove remote billing or effect completion.

Drain counts physically retiring comparisons. Shutdown waits within the existing shared shutdown deadline. Restart interrupts saved active work and never replays uncertain calls. Historical pins and observed bytes remain facts; changing source configuration, evaluator or compiled host implementation marks current applicability stale. Reports grant no tool permission, source access, artifact write authority or owner acceptance.

## Validation scope

Offline SDK/UI tests cover transport, lazy entry, the simple/Advanced forms, hidden expectations, request-key retries, version disclosures, paid-provider notice and report/cancellation behavior. Native tests are authored for actual temporary file outcomes, public-input numeric derivation, private-canary denials, identity substitution, corruption/extra files, cancellation custody, runtime-consumer closure and late usage, persistence faults, report eviction, restart, retained attempt limits and produced-byte accounting.

Only the parent integration executor runs native tests in this batch. Focused core tests, daemon task tests and the repository's full `bun run rust:test` remain the required native checks. Passing offline UI tests is not daemon, provider or physical Office acceptance.
