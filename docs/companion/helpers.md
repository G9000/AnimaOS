# Delegate to a focused helper

Ask Anima to create a helper for a bounded assignment, for example:

> Use a helper to research this project. Its playbook is to inspect the relevant files, check each claim against evidence, and return findings and open questions. Keep working with me while it runs.

`spawn_helper` waits for the saved result. An explicitly granted `spawn_helper_async` returns a saved run handle so the companion can continue and collect the result later. Both accept `name`, `task`, and an optional `playbook` containing the assignment's role, procedure, and output rules. The task, playbook, and section labels must fit together within 32,768 UTF-8 bytes. Existing worktree and completion options still apply.

Helpers use the selected configured model with a focused worker persona and normal project instructions. The parent's biography is also excluded from helper roster context. The main agent keeps its own personality and synthesizes the returned results.

Each assignment has its own conversation and recorded result. An idle helper may be reused with a fresh assignment; its previous conversation and playbook are not added to the new task. Async acceptance records the composed assignment durably, and changing a playbook under the same launch call is rejected as a changed retry. This does not make an interrupted model execution automatically resume after restart.

Delegation does not add permissions. Helpers receive a narrowed subset of the companion's current grants, with approvals, cancellation and shared execution accounting. Worker limits follow owner policy; the defaults are listed below. Helpers cannot create more helpers. Existing specialists continue to use `delegate_to_agent` with their own personalities.

## Model routes and worker limits

In Settings → Model, use **Helper routing and limits** to save named model routes and ordered fallbacks. Without a route, a helper uses the launching conversation's model. A route changes model selection only; it grants no additional tools, credentials, history, or personality.

The owner controls concurrent assignments (1–8), starts per companion run (1–16), tool turns (1–32), assignment deadline (1–600 seconds), and output tokens per response (1–16384). Defaults remain four concurrent assignments, four starts, eight turns, two minutes, and 4096 tokens. Existing agent ceilings and the shared execution budget still apply. Per-assignment limits only tighten these bounds.

Both launch tools accept optional route, limits, and output_schema fields:

~~~json
{
  "name": "Evidence review",
  "task": "Inspect the supplied evidence and summarize uncertainties",
  "playbook": "Check each claim against project files and report blockers",
  "route": "research",
  "limits": { "max_tool_turns": 6, "timeout_ms": 60000, "max_output_tokens": 2048 },
  "output_schema": {
    "type": "object",
    "properties": { "summary": { "type": "string" } },
    "required": ["summary"],
    "additionalProperties": false
  }
}
~~~

Output contracts are bounded local JSON schemas. The helper receives the contract as assignment input. Returned metadata reports whether committed text matches that schema. Invalid output remains available with a validation failure; validation never repeats model calls or completed tool effects.

Accepted background assignments persist their route, model, fallbacks, options, and effective limits. Exact retries return the saved handle; changing options under the same tool-call identity is refused. Editing a route affects subsequent assignments, while accepted model choices remain fixed. Current numeric ceilings may further tighten queued assignments. Interruptions on restart retain existing review and recovery behavior rather than automatically repeating work.

## Batch launch and collection

Enable **Allow background helpers** under Settings → Permissions to grant background launch, batch, collection, inspection, steering, and stopping. Saving model routes alone does not grant these abilities. Partial legacy grants stay unchanged until the owner edits the ability.

The spawn_helpers tool accepts a tasks array of 1–8 ordinary background launch objects. Each omitted completion destination defaults to manual. It validates all input shapes first, then returns one indexed acceptance or refusal per item. The batch is a set of independent operations, with partial acceptance reported explicitly. An outer call and item index have a stable identity: unchanged accepted items replay, changed items conflict, and previously refused items may be retried. Start and concurrency limits remain shared with individual launches.

The wait_helper_runs tool accepts unique run_ids, mode (any or all), and timeout_ms from 0 to 30000. It returns every handle with readiness, committed results or terminal failures, pending handles, and truncation flags. Zero reads one snapshot. Timeout starts no additional work and leaves pending helpers running. Cancelling the caller stops collection. Runtime events wake collection without holding the control-plane transaction.

## Deliberate boundaries

Workers remain focused leaves with a fresh private conversation and the main personality omitted. Nested coordinators, explicit conversation forks, reusable child conversations, and arbitrary callback destinations are not yet enabled. Unsupported policy settings fail closed. Existing specialists, worktree helpers, native jobs, and workflow orchestration retain their own contracts.
