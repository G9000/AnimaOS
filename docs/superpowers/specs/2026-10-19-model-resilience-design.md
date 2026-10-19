# Model resilience: key pools, fallback chains and a side-task model

**Date:** 2026-10-19 · **Status:** approved by the owner in chat

## Why

Today one API key per provider is read from env vars at daemon start, every model failure fails the reply, and every side task (titles, compaction, smart check, learning reviews) runs on the agent's own, often expensive, model. A rate limit or an expired key stops the companion until the owner edits env vars and restarts.

## Owner decisions

1. **Claude.** Anthropic's Agent SDK docs say third-party developers may not offer claude.ai (Pro/Max) login without prior approval. AnimaOS offers **an Anthropic API key entered in the console**, nothing else for Claude. The owner's subscription keeps working only inside Claude Code used as a coding worker, as today.
2. **Key storage.** Like Hermes, keys live in a **plaintext JSON file readable only by the owner** (mode 0600, atomic writes). Env-var keys still count as the first key of their provider and are never written to the file.
3. **Fallback chains are per agent.** Each agent may list up to 3 fallback models (provider + model), tried in order.
4. **Mid-reply.** A model call falls back **only if it has not streamed any text yet**. After text appears, a failure fails the step as today.
5. **Side-task model** (my call, within the brief). One workspace-wide setting. When set, titles, compaction, the smart check and learning reviews use it; if it fails, they use the agent's own model and its chain. When unset, nothing changes.

## Behaviour

### Failure classes

Adapter errors stay strings, but our adapters format them, so `classify_model_error` reads them reliably. Each class has a fixed effect on keys and on fallback.

| Class | How it's recognised | Rotate key | Fall back |
|---|---|---|---|
| `RateLimited` | 429 | yes | yes |
| `Overloaded` | 500, 502, 503, 504, 529 | no | yes |
| `AuthFailed` | 401, or 403 that isn't billing | yes | yes, once every key is out |
| `Billing` | 402, or a body mentioning `insufficient_quota`, `credit balance`, `billing`, `out of extra usage` or `key limit exceeded` | yes | yes, once every key is out |
| `Timeout` | "timed out" | no | yes |
| `Transport` | "request failed" (connection) | no | yes |
| `ModelNotFound` | 404, or `model_not_found` | no | yes |
| `ContextTooLong` | `context_length_exceeded`, "prompt is too long" | no | no |
| `BadRequest` | other 4xx | no | no |
| `Other` | anything else | no | no |

Adapters append ` (retry after Ns)` to the error when the provider sent `retry-after` or a reset header. The classifier reads it back as `retry_after_s`.

### Key pools (Hermes rules)

- **Strategies** (per provider): `fill_first` (default), `round_robin`, `random`, `least_used`.
- **When a key fails** (`RateLimited`, `AuthFailed`, `Billing`): the key rests, and the same call retries at once on the next available key of that provider. The retries are bounded by the pool size.
- **How long a key rests:**

| Failure | Rest |
|---|---|
| `AuthFailed` | 5 minutes |
| `RateLimited`, or another key failure | 1 hour |
| `Billing` | 1 hour, even if it's the only key |
| A non-billing failure on the only usable key | 60 seconds |
| The provider sent a retry time | that time, overriding the rows above |

- **Rested keys** become usable again after their rest. Removing a key or adding one never wakes a resting key.
- **Display:** a key is shown masked (`sk-…abcd`) with its label, status (`ready`, or `resting until HH:MM — rate limited`) and number of uses.

### Fallback chain

- `AgentConfig.fallbacks: Vec<ModelChoice { provider, model }>`, at most 3, with no duplicate of the primary.
- **On a fallback-worthy failure before any text:** try the next choice. Each choice uses its own provider's key pool.
- **Sticky per run:** once a run has fallen back, its later steps start at the model that worked. The next run starts at the primary again.
- **The owner sees** a note on the reply: "Answered by {provider} {model} because {primary} was {rate limited | overloaded | unavailable | out of credit | not accepting the key}".
- **If every choice fails,** the step fails with the primary's error plus "(also tried: …)".

### Pricing

Each run step records the provider and model that actually answered, and usage is priced per step. Steps without that record fall back to the run's provider and model, as today.

### Side-task model

Workspace setting `sideModel: { provider, model } | null`, stored in the control plane and set in Settings → Models. It applies to titles, compaction, the smart check and learning reviews. Profile and agency generation already take a model from their request and are unchanged.

### API (owner-only, `Cache-Control: no-store`)

| Route | Does | Returns or errors |
|---|---|---|
| `GET /api/provider-keys` | Lists keys | `{ providers: [{ id, strategy, keys: [{ id, label, masked, source: "env" or "file", status, restingUntilMs, lastError, uses }] }] }`. Full keys are never returned. |
| `POST /api/provider-keys/{provider}` | Adds a key `{ label?, key }` | `400` for an unknown provider, an empty key, or more than 2,000 characters; `409` for a duplicate. Without a key file the key works until restart, and the list says `persisted: false`. |
| `DELETE /api/provider-keys/{provider}/{keyId}` | Removes a key | `409` for the env key |
| `PUT /api/provider-keys/{provider}/strategy` | Sets `{ strategy }` | |
| `GET` / `PUT /api/settings/side-model` | Reads or sets `{ provider, model }` or `null` | |

Agent fallbacks travel on the existing `PATCH /api/agents/{id}` as `fallbacks: [{ provider, model }]`.

### Key file location

`ANIMAOS_RS_PROVIDER_KEYS_FILE`, or else `provider-keys.json` next to `ANIMAOS_RS_CONTROL_PLANE_FILE`. With neither set, keys added in the console live in memory only, and the console says so.

## Not in scope

- claude.ai, ChatGPT-style or Copilot logins for new providers.
- Bedrock, Vertex and Foundry.
- Mixture of agents.
- Fallback for profile and agency generation.
- Cross-process locking of the key file. The daemon is its sole writer.

## Implementation notes (2026-10-19)

- **Where it lives:**
  - `anima-core/src/model_error.rs`: the error classifier.
  - `anima-model-adapters`: the `KeySource` seam and rotation (`adapter.rs`, `lib.rs`), plus stream errors that keep the provider's error type (`stream.rs`).
  - `hosts/rust-daemon/src/provider_keys/`: the key pool (Hermes rules), the 0600 file store and boot.
  - `fallback_model.rs`: the per-run fallback chain.
  - `side_model.rs`: the side-task model.
  - `routes/provider_keys.rs` and `routes/side_model.rs`: the routes.
  - `apps/web/src/components/models/`: the console.
- **Departures from the text above:**
  - A 401 rests 5 minutes even on the only key.
  - Only `retry-after` overrides a rest, and only for rate-limit and overload failures.
  - When every key rests, the error carries the real cause (402, 401 or 429) and the wake time, e.g. "every key is resting — key rejected until 19:22".
  - The fallback event is `model.fallback`.
  - Only the first model of a call decides whether to fall back. After that, every later choice is tried, and if all fail the error is the first model's plus "(also tried: …)".
  - A run sticky on a fallback never returns to the primary within that run.
  - `deterministic` and `test` can't be fallbacks or the side model.
  - A provider counts as configured when it holds any key, ready or resting.
- **Known limits:**
  - Swarm members don't use fallbacks.
  - Each attempt gets its own timeout, so the worst case is (1 + fallbacks) × timeout.
  - Voice and speech still read only the env OpenAI key.
  - Rests apply per key, across all models.
- **Follow-ups closed (2026-10-19):**
  - Side tasks fall back through the agent's whole chain (`fallback_model::call_with_fallbacks`), without the side model a second time. Learning reviews keep the agent's chain too.
  - The side model gets half of the time left to the task's deadline. After that the agent's models get the rest.
  - Helpers inherit the parent's fallbacks along with its model.
  - "Try now": `POST /api/provider-keys/{provider}/{key_id}/wake` and a console button wake a resting key, the env key included.
  - Fallback validation reads provider aliases (`gemini` is `google`) and rejects a model listed twice.
  - A late failure on a resting key only extends its rest.
  - The transcript repeats a fallback note when a primary step came between.
- **Verified live** (test console, 2026-10-19):
  - A fake OpenAI key plus a rejected env key both rested 5 minutes.
  - The reply came from the ChatGPT fallback with "Answered by chatgpt gpt-6-luna because gpt-4o-mini did not accept its key.", and the step was priced at the fallback.
  - With the side model on the all-resting OpenAI pool, titles fell back to the agent's model, and no request was sent to OpenAI.
