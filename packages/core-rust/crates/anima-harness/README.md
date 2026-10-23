# anima-harness

An embeddable agent harness built on the animaOS Rust core. It composes
`anima-core`'s agentic loop (`AgentRuntime`) with `anima-model-adapters`' provider
catalog behind a small builder API: system prompt + tools + agentic loop + events.

```rust
use anima_harness::Harness;

# async fn example() -> Result<(), anima_harness::HarnessError> {
let mut harness = Harness::builder()
    .provider("anthropic") // resolves ANTHROPIC_API_KEY from the environment
    .model("claude-sonnet-4-5")
    .system("You are a helpful assistant.")
    .build()?;

let result = harness.run("Say hello").await;      // single-shot
let reply = harness.chat("And again?").await;     // keeps conversation history
# Ok(())
# }
```

## Tools

Implement `HarnessTool` and register it on the builder:

```rust
use std::collections::BTreeMap;
use anima_harness::anima_core::{Content, DataValue, ToolDescriptor};
use anima_harness::HarnessTool;
use async_trait::async_trait;

struct GetTime;

#[async_trait]
impl HarnessTool for GetTime {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "get_current_time".into(),
            description: "Get the current Unix time in milliseconds.".into(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        }
    }

    async fn execute(&self, _args: BTreeMap<String, DataValue>) -> Result<Content, String> {
        Ok(Content { text: "0".into(), attachments: None, metadata: None })
    }
}
```

## Credentials

Provider credentials resolve in this order: explicit `.api_key(...)` /
`.base_url(...)` builder overrides, then the provider's standard environment
variables (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, ...), then the provider's
default base URL. For tests or bespoke providers, supply any
`anima_core::ModelAdapter` via `.adapter(...)`.

## Execution controls

The builder and the corresponding `Harness::set_*` methods attach the core
runtime's host controls:

| Builder method | Host responsibility |
| --- | --- |
| `.run_control(control)` | Supply Stop cancellation and the steering inbox. |
| `.run_observer(observer)` | Receive streaming, usage and tool-result frames inline. |
| `.run_id(id)` | Bind repeated inputs to one host run and its finite allowance. |
| `.execution_limits(limits)` | Configure finite limits for newly created ephemeral scopes. |
| `.execution_context(context)` | Attach an existing shared budget port, identity, deadline and consumption. |
| `.turn_checkpoint(binding, sink)` | Persist acknowledged turn progress for one matching attempt. |
| `.tool_policy(policy)` | Approve and revalidate each exact model-requested tool invocation. |

```rust
use std::sync::Arc;
use anima_harness::Harness;
use anima_harness::anima_core::{ModelAdapter, RunControl, RunObserver};
use anima_harness::anima_core::execution_limits::ExecutionLimits;

# fn example(adapter: Arc<dyn ModelAdapter>, observer: Arc<dyn RunObserver>)
#     -> Result<(), anima_harness::HarnessError> {
let control = RunControl::new();
let limits = ExecutionLimits {
    max_model_attempts: 8,
    max_retries: 2,
    ..ExecutionLimits::default()
};
let mut harness = Harness::builder()
    .model("host-selected-model")
    .adapter(adapter)
    .run_control(control.clone())
    .run_observer(observer)
    .execution_limits(limits)
    .build()?;

// Another task holding this clone can request Stop while run/chat is pending.
control.cancel.cancel();
# Ok(())
# }
```

A fresh, unbound builder creates an independent finite in-memory scope for each
`run` or `chat` input. `chat` retains conversation history without sharing that
previous input's allowance. An explicit run ID, checkpoint attempt, or execution
context instead binds the harness to one scope: subsequent calls consume its
remaining allowance. Passing an execution context retains the host's existing
budget; builder limits do not reset that context's consumption.

Default `ExecutionLimits` allow 30 minutes, 96 model attempts, 192 tool calls,
24 retries, 16 in-flight operations, 4,000,000 input tokens, 262,144 output tokens
and 4,262,144 total tokens per scope. Invalid or unbounded limits are rejected.
`max_tool_iterations` remains a separate loop cap and cannot grant more shared
budget. Reservations are conservative local accounting, not a guarantee of
complete provider billing; unknown usage remains charged. Custom adapters must
honor the core's execution-context contract or explicitly attest that each
adapter call performs one physical attempt with no hidden retries. Complete
usage must also be attested before its reservation can be released accurately.

## Tool approval and Stop

Registering a tool grants trusted embedder execution authority by default. With
a `ToolPolicy`, descriptor exposure alone grants no approval. The policy receives
an immutable `ToolInvocation` containing the agent and execution identity,
tool-call ID, exact arguments and descriptor. The harness retains the selected
tool implementation before waiting for approval. Denial, callback failure,
Stop, deadline or lost execution authority prevents the effect.

`ToolPolicy::approve` is asynchronous and must be cancel-safe and perform no
external effects. After the last budget wait, `ToolPolicy::revalidate` runs
synchronously immediately before execution; use it to reject revoked host
grants. It must return quickly without I/O. The default revalidation accepts the
earlier approval, so hosts with revocable grants must override it. Durable
approval records and exact Undo remain host responsibilities. Calling
`ToolSet::execute` directly uses trusted embedder authority and bypasses the
harness policy and budget.

Stop and deadlines bound approval/checkpoint waits and prevent new dispatch.
Once an actual tool effect starts, the runtime retains its future until physical
retirement and preserves its known result. A tool that never retires can therefore
keep the run pending after Stop. If the embedder drops the entire run future, it
must independently retain custody of any started physical operation.

## Checkpoints and restore

The harness is portable and does not supply durable storage. A durable embedder
owns the `ExecutionBudgetPort` and `TurnCheckpointSink`, including atomic
admission before dispatch, idempotent settlement, unknown/in-flight reservations,
writer ownership and durable writes that survive cancellation of their caller's
acknowledgement wait. Checkpoint failure prevents progress across the affected
boundary; checkpoint evidence never grants tool approval or permission to replay
an uncertain effect. A checkpoint binding must match the execution run ID.

`snapshot()` captures conversation and execution binding hints, not live
authority, controls, observers, policy or storage handles. After `Harness::restore`,
explicitly call `set_execution_context` with the matching scope and run identity
and a deadline that does not extend the saved binding. Reattach RunControl,
RunObserver, checkpoint custody and ToolPolicy as required by the host before
running. A new in-memory context with the same run ID has a different scope and
cannot replace the saved allowance. Restore never creates fresh allowance,
including for legacy snapshots without a binding hint; the host must deliberately
attach authority.

## Example

```bash
ANIMA_PROVIDER=anthropic ANIMA_MODEL=claude-sonnet-4-5 \
  cargo run -p anima-harness --example chat
```
