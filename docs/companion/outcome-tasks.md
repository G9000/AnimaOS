# Outcome checks for a chat task

Advanced input can attach public success criteria to an ordinary owner chat task. Ordinary chats do not run outcome checks unless criteria are attached. The existing task remains the executor. Its grants, approvals, Stop control and shared execution budget remain in force.

A definition can require response text, UTF-8 file text, an exact file hash, a JSON value at a JSON Pointer, complete native document rendering, or spreadsheet recalculation without errors. Text and JSON expectations are public criteria and can guide corrections. Private regression expected answers remain inside the separate regression runner and never enter actor prompts, workspace files, quality judge input or correction feedback.

The daemon observes actual saved file bytes. It can request at most two corrections in the admitted run. During correction, native controls allow only reads and version-bound repairs of the declared files. Arbitrary commands, sends, browser actions, delegation and unrelated writes are denied. File repair requires native checkpoint custody; a changed preimage is refused. A criterion is data and does not grant a tool or approve an action.

Document checks call the native inspection service against the exact current input bytes. Complete rendering means the report covers the entire exported view. It does not establish that text is unclipped or that a design looks good. Spreadsheet checks require complete native formula inventory coverage and no calculation errors. Missing engines, incomplete evidence, denied access and unsupported inputs remain inconclusive.

An independent model quality rubric is represented separately and remains inconclusive when no owner-authorized quality verifier is configured. The producer's own statement that an output looks good cannot satisfy it.

Pinned inspection references also need a path-based document check for the same current bytes. A stored report alone cannot establish current source freshness.

The outcome line beside the task's reply distinguishes criteria met, owner acceptance, unmet criteria, unavailable evidence, budget exhaustion and Stop. Its disclosure contains the checked criteria and immutable evidence identities. Owner-required acceptance names the displayed revision and report digest and rechecks current evidence. File, source, model, criteria or report changes invalidate an older pass. Report-only success never creates owner acceptance.

Close task releases a retained terminal review without rerunning work or resetting accounting. A retry of the original send uses its original request key and exact criteria. Generic conversation retry, connector delivery and generic recovery cannot replay an outcome task. Restart retains evidence and charges, interrupts active work, and quarantines previous current passes for review. Inspection reports are historical after restart until independently checked again; the producer is never replayed automatically.

The first implementation admits outcomes only on original ordinary owner chat runs through the existing run-admission route. It does not add job or workflow criteria admission, a scheduler or background automatic retries. Selected project revisions, workspace sources and explicitly selected chat evidence are rechecked. Connected or indexed knowledge sources whose exact versions cannot be revalidated leave the outcome in review. No source access is inferred from a criterion.

Owner API operations are an optional `outcome` on session run creation, GET `/api/agents/{agent_id}/runs/{run_id}/outcome`, POST its `/accept` suffix with `{revision, reportSha256}`, and POST its `/close` suffix with `{revision}`. There are at most 16 unresolved outcome roots per profile, 16 criteria per definition and two correction admissions per run. SDK methods are `outcomes.get`, `outcomes.accept` and `outcomes.close`.

Native validation and physical document rendering/calculation acceptance must use the actual current implementation and available engines. Synthetic fact tests, saved hashes and metadata do not establish a successful physical render.
