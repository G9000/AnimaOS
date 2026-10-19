# Model resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A failed model call no longer has to fail the reply. Keys rotate within a provider, agents fall back to their listed models, and side tasks can run on a cheaper model.

**Architecture:**
- **Errors:** the adapter crate gets an error classifier, plus a `KeySource` seam so it never reads files itself.
- **Key pool:** the daemon implements `KeySource` with a key file that follows Hermes's rotation rules.
- **Fallbacks:** a per-run fallback adapter in the daemon wraps the existing runtime adapter, tries an agent's fallbacks before any text has streamed, and reports which model served each step so usage is priced per step.
- **Side tasks:** a workspace side-task model is swapped in at the four side-call sites.

**Tech Stack:** Rust (anima-core, anima-model-adapters, hosts/rust-daemon), React + vitest (apps/web), `@animaOS-SWARM/sdk`.

**Spec:** [`docs/superpowers/specs/2026-10-19-model-resilience-design.md`](../specs/2026-10-19-model-resilience-design.md)

## Global Constraints

- The owner works on **main** and wants nothing committed or staged. Every "Commit" step below means: leave the change unstaged and record it in the ledger.
- Other unstaged work in this checkout belongs to other sessions (time-zone work: `clock.rs`, `tools/*`, `components.rs`, `packages/sdk/src/runs.ts`). Don't touch it.
- **Claude is API-key only.** No claude.ai, Pro or Max login code anywhere.
- **Key file:** mode 0600. Write a temp file, then rename it over the old one. Never log or return a full key; masked form is `prefix…last4`.
- New persisted fields use `#[serde(default)]`. Older control planes and key files must load unchanged.
- **Verification:**
  - `bun x nx run rust-daemon:test --skipNxCache` (only the 16 known macOS failures: 14 `fp08_host_*`, 2 `m44_*`)
  - `bun x nx run core-rust:test --skipNxCache`
  - `bun x nx run @animaOS-SWARM/web:test --skipNxCache`
  - `bun x nx run @animaOS-SWARM/web:typecheck`

## Review Focus

1. **A rate limit on one key with other keys available** must never reach the owner as a failed reply. The next key is tried within the same call (Task 3).
2. **A failure after text has streamed** must not switch models, and must not duplicate or replace text already shown (Task 4).
3. **Keys never leak:** not in API responses, logs, errors (the adapters' sanitizers must still mask them), usage records or the transcript (Tasks 2, 3, 7).
4. **Old data:** a control plane without `fallbacks`/`sideModel` and runs without per-step model fields load, run and price exactly as before (Tasks 4, 5, 6).
5. **The side-task model failing** (wrong model name, no key) must fall back to the agent's model, not break titles or compaction (Task 6).

---

### Task 1: Classify model errors

**Files:**
- Create: `packages/core-rust/crates/anima-core/src/model_error.rs`
- Modify: `packages/core-rust/crates/anima-core/src/lib.rs` (export it)
- Modify: `packages/core-rust/crates/anima-model-adapters/src/adapter.rs:678-705` (append the retry hint)
- Modify: `packages/core-rust/crates/anima-model-adapters/src/chatgpt.rs` (same)
- Test: in `model_error.rs`, and in `anima-model-adapters/src/tests.rs`

**Interfaces:**
- **Produces:**
  - `pub enum ModelFailureKind { RateLimited, Overloaded, AuthFailed, Billing, Timeout, Transport, ModelNotFound, ContextTooLong, BadRequest, Other }`
  - `pub struct ModelFailure { pub kind: ModelFailureKind, pub status: Option<u16>, pub retry_after_s: Option<u64> }`
  - `pub fn classify_model_error(message: &str) -> ModelFailure`
  - `impl ModelFailureKind { pub fn rotates_key(self) -> bool; pub fn falls_back(self) -> bool }`
- The rules are the spec's "Failure classes" table.

- [ ] **Step 1: Write the failing test.** `classifies_adapter_error_formats` asserts each of these maps to its kind:
  - `"Anthropic API error (429): rate_limit_error (retry after 30s)"` → `RateLimited`, `retry_after_s: Some(30)`
  - `"OpenAI API error (402): insufficient_quota"` → `Billing`
  - `"OpenAI API error (403): key limit exceeded"` → `Billing`
  - `"Google API error (401): …"` → `AuthFailed`
  - `"After one retry: Anthropic API error (529): overloaded"` → `Overloaded`
  - `"Model call timed out"` → `Timeout`
  - `"OpenAI request failed: connection refused"` → `Transport`
  - `"OpenAI API error (404): model_not_found"` → `ModelNotFound`
  - `"Anthropic API error (400): prompt is too long"` → `ContextTooLong`
  - `"OpenAI API error (400): bad"` → `BadRequest`
  - `"ChatGPT usage limit reached"` → `RateLimited`
  - `"ChatGPT authorization expired; …"` → `AuthFailed`

  Also assert `rotates_key`/`falls_back` per the spec table.
- [ ] **Step 2: Run** `cargo test -p anima-core model_error`. Expected: FAIL, module missing.
- [ ] **Step 3: Implement.**
  - The classifier uses regexes: `\((\d{3})\)` for the status, `retry after (\d+)s` for the hint, and the body keywords from the spec.
  - In `adapter.rs`, read `retry-after` (seconds, or an HTTP date relative to now) or `anthropic-ratelimit-requests-reset` / `x-ratelimit-reset-requests` from the failed response, and append ` (retry after {n}s)` to the error.
  - Add adapter test `rate_limit_errors_carry_the_retry_hint`: a fake server returns 429 with `retry-after: 12`, and the error ends with `(retry after 12s)`.
- [ ] **Step 4: Run** `cargo test -p anima-core model_error && cargo test -p anima-model-adapters rate_limit_errors_carry`. Expected: PASS.
- [ ] **Step 5: Ledger it** (no commit).

### Task 2: Key pool (daemon)

**Files:**
- Create: `hosts/rust-daemon/src/provider_keys/mod.rs` (pool, rules), `hosts/rust-daemon/src/provider_keys/store.rs` (file I/O)
- Modify: `hosts/rust-daemon/src/lib.rs` (`mod provider_keys;`)
- Test: `hosts/rust-daemon/src/provider_keys/tests.rs`

**Interfaces:**
- **Consumes:** `ModelFailure` and `ModelFailureKind` (Task 1).
- **Produces:**
  - `pub(crate) enum Strategy { FillFirst, RoundRobin, Random, LeastUsed }`, with `FillFirst` as the default
  - `pub(crate) struct KeyPool` (an `Arc<Mutex<…>>` inside, `Clone`)
  - `KeyPool::open(path: Option<PathBuf>, env: &BTreeMap<String, String>) -> Result<KeyPool, String>`
    - `env` maps provider id → the env key from `provider_config_from_env`
    - the env key gets id `env`, source `Env`, and is never persisted
  - `pick(&self, provider: &str, now_ms: u64) -> Option<PickedKey { id: String, key: String }>`
  - `report(&self, provider: &str, key_id: &str, outcome: KeyOutcome, now_ms: u64)`, where `enum KeyOutcome { Ok, Failed(ModelFailure) }`
  - `add(&self, provider, label: Option<String>, key: String, now_ms) -> Result<KeyView, KeyError>`
  - `remove(&self, provider, key_id) -> Result<(), KeyError>`
  - `set_strategy(&self, provider, Strategy)`
  - `list(&self, now_ms) -> Vec<ProviderKeysView>`
  - `KeyView { id, label, masked, source, status: "ready" or "resting", resting_until_ms, last_error, uses }`
  - `KeyError { Unknown, Empty, TooLong, Duplicate, EnvKey, NotPersisted, Io }`
- **File shape:** `{ "version": 1, "providers": { "<id>": { "strategy": "fill_first", "keys": [ { "id", "label", "key", "addedAtMs", "uses", "restingUntilMs", "lastError" } ] } } }`
- **Rest times** are the spec table: 401 → 300 s; 429 and other key failures → 3,600 s; billing → 3,600 s even when it's the only key; a non-billing failure on the only usable key → 60 s; `retry_after_s` overrides all of these.

- [ ] **Step 1: Write the failing tests:**
  - `fill_first_uses_the_first_ready_key_and_skips_resting_ones`
  - `round_robin_cycles`
  - `rest_times_follow_the_rules`: 401 → 300 s; 429 with 2 keys → 3,600 s; 429 on the only key → 60 s; billing on the only key → 3,600 s; retry-after 12 → 12 s
  - `a_rested_key_returns_after_its_rest`
  - `env_key_is_listed_first_never_saved_and_cannot_be_removed`
  - `file_is_0600_and_survives_reopen`: add a key, reopen, the same keys come back; check `metadata().permissions().mode() & 0o777 == 0o600` on Unix
  - `keys_are_masked_in_views`: `sk-abc…wxyz`; never equal to the key
  - `add_rejects_empty_long_and_duplicate_keys`
  - `without_a_file_added_keys_work_but_are_not_persisted`: `add` succeeds; `list` marks the pool `persisted: false`
- [ ] **Step 2: Run** `cargo test -p anima-daemon --lib provider_keys`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - `store.rs` writes `<file>.tmp` with mode 0600 (set via `OpenOptions::mode` on Unix), then `rename`s it over the file.
  - `random` uses `rand` if the daemon already depends on it, else `now_ms % ready.len()`. Record which in the ledger.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Ledger it.**

### Task 3: Adapters pick keys per call and rotate on key failures

**Files:**
- Modify: `packages/core-rust/crates/anima-model-adapters/src/lib.rs:33-42` (add `KeySource`)
- Modify: `packages/core-rust/crates/anima-model-adapters/src/adapter.rs` (`credential_for`, `key_required`, and the request paths at `:163`, `:190`, `:219`)
- Modify: `hosts/rust-daemon/src/runtime_model.rs:24` and `app.rs:272-277` (open the pool, pass it in)
- Test: `anima-model-adapters/src/tests.rs`

**Interfaces:**
- **Produces, in anima-model-adapters:**
  - `pub trait KeySource: Send + Sync { fn pick(&self, provider: &str) -> Option<(String, String)>; fn report(&self, provider: &str, key_id: &str, failure: Option<&ModelFailure>); }`
    - `pick` returns `(key_id, key)`; `failure` is `None` on success
  - `ProviderAdapterConfig.key_source: Option<Arc<dyn KeySource>>`
    - `#[serde(skip)]`, or plain `pub` if the struct isn't serialized
    - defaults to `None`
- **Behaviour:**
  - With `key_source` set, a provider call picks a key before each attempt.
  - When the classified failure `rotates_key()`, it reports the failure and repeats the attempt with the next pick, at most once per key the pool holds.
  - It reports success on a final response.
  - Without `key_source`, behaviour is exactly as today.
  - The existing single 5xx retry is unchanged and counts as the same key.
- **Daemon:** `KeyPool` implements `KeySource` (`now_ms()` inside).

- [ ] **Step 1: Write the failing tests:**
  - `a_rate_limited_key_rotates_to_the_next_within_the_call`: a fake server returns 429 for `Bearer key-a` and 200 for `key-b`; a test `KeySource` with `[a, b]`; the call succeeds, the reports are `a: Some(RateLimited)` then `b: None`.
  - `all_keys_failing_returns_the_last_error`
  - `no_key_source_behaves_as_before`, which reuses an existing success test.
- [ ] **Step 2: Run** `cargo test -p anima-model-adapters rotates`. Expected: FAIL.
- [ ] **Step 3: Implement** the loop around the per-provider attempt, both for `stream_inner` and `generate_inner`. Then wire the daemon: `KeyPool::open(keys_file_path(), &env_keys)` at boot, put it into `ProviderAdapterConfig.key_source`, and keep a handle in `DaemonState` (`provider_keys: KeyPool`) for the routes.
- [ ] **Step 4: Run** `cargo test -p anima-model-adapters && cargo test -p anima-daemon --lib provider_keys`. Expected: PASS.
- [ ] **Step 5: Ledger it.**

### Task 4: Per-agent fallback chain

**Files:**
- Modify: `packages/core-rust/crates/anima-core/src/agent.rs:41` and `:72`
  - `AgentConfig.fallbacks: Vec<ModelChoice>` with `#[serde(default)]`
  - `pub struct ModelChoice { pub provider: String, pub model: String }`
  - `AgentConfigUpdate.fallbacks: Option<Vec<ModelChoice>>`
- Modify: `packages/core-rust/crates/anima-core/src/model.rs:51`
  - add `ModelStreamFrame::Served { provider: String, model: String, reason: String }`
  - update every exhaustive match on the frame enum
- Create: `hosts/rust-daemon/src/fallback_model.rs`
- Modify: `hosts/rust-daemon/src/state.rs:3653` (`session_model_adapter`: wrap the per-run adapter in `FallbackModelAdapter`)
- Modify: the agent validation route (`routes/agents.rs` or its contract): at most 3 fallbacks, none equal to the primary, providers known
- Test: `hosts/rust-daemon/src/fallback_model.rs` tests, `agent_runs` integration test

**Interfaces:**
- **Consumes:** `classify_model_error` and `falls_back()` (Task 1).
- **Produces:** `pub(crate) struct FallbackModelAdapter { inner: Arc<dyn ModelAdapter>, sticky: Arc<Mutex<Option<usize>>> }`, with `new(inner) -> Self`, implementing `ModelAdapter`.
- **Algorithm** (in `stream`; `generate` behaves the same):
  - Build the chain `[primary] + config.fallbacks`, starting at the sticky index.
  - For each choice, clone the config with that provider and model, and stream through a sink wrapper that records whether any `TextDelta` was emitted.
  - On `Ok`:
    - if the choice is not the primary, emit `Served { provider, model, reason }` before `Final`
    - set sticky to this index
    - return
  - On `Err(e)`:
    - classify `e`
    - if text was emitted, or `!falls_back()`, or this is the last choice, return `Err` (on the last choice, with ` (also tried: a/x, b/y)` appended)
    - otherwise continue
  - **Reason text by kind:** RateLimited "was rate limited"; Overloaded "was overloaded"; Timeout or Transport "was unavailable"; Billing "was out of credit"; AuthFailed "did not accept its key"; ModelNotFound "did not have that model".

- [ ] **Step 1: Write the failing tests** with scripted inner adapters:
  - `falls_back_before_any_text_and_says_why`: the primary errors `"OpenAI API error (429): x"`, the fallback answers, and the frames are `Served{.., reason:"was rate limited"}` then `Final`.
  - `does_not_fall_back_after_text`: the primary emits a `TextDelta`, then errors 503; the result is `Err` and the fallback is never called.
  - `does_not_fall_back_on_bad_request_or_context_too_long`
  - `sticks_to_the_working_fallback_within_a_run`: a second call on the same adapter starts at the fallback.
  - `all_failing_lists_what_was_tried`
  - `no_fallbacks_is_unchanged`
  - Agent route test `fallbacks_are_validated`: 4 entries → 400; one equal to the primary → 400; an unknown provider → 400; `[]` clears.
- [ ] **Step 2: Run** `cargo test -p anima-daemon --lib fallback`. Expected: FAIL.
- [ ] **Step 3: Implement** it, and wrap the per-run adapter in `session_model_adapter`.
- [ ] **Step 4: Run** `cargo test -p anima-daemon --lib -- fallback agent_runs` and `cargo test -p anima-core`. Expected: PASS.
- [ ] **Step 5: Ledger it.**

### Task 5: Record and price the model that served each step

**Files:**
- Modify: `packages/core-rust/crates/anima-core/src/runtime.rs` (StepSink: on `Served`, remember it for the step and emit an agent event the transcript can show)
- Modify: `hosts/rust-daemon/src/runs/ledger.rs:165`
  - `RunStepUsage.provider: Option<String>`, `RunStepUsage.model: Option<String>`, `RunStepUsage.fallback_reason: Option<String>`, all `#[serde(default)]`
- Modify: `hosts/rust-daemon/src/usage/mod.rs:186` (price each step with its own provider/model when set)
- Modify: web transcript (`apps/web/src/lib/transcript.ts`, the run activity component that renders a step) to show "Answered by {provider} {model} because {primary} {reason}."
- Test: `usage` tests, the transcript test

**Interfaces:**
- **Consumes:** `ModelStreamFrame::Served` (Task 4).
- **Produces:** the step usage fields above, and an event payload `{ type: "model_fallback", stepId, provider, model, reason, primary: { provider, model } }`.

- [ ] **Step 1: Write the failing tests:**
  - `steps_answered_by_a_fallback_are_priced_at_the_fallback_model`: one step on `openai/gpt-x`, the next on `anthropic/claude-sonnet-5-5`, and the two usage records carry their own provider and model.
  - `old_steps_without_a_model_use_the_run_model`
  - Web: `shows_which_model_answered_after_a_fallback`
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement** it.
- [ ] **Step 4: Run** `cargo test -p anima-daemon --lib usage` and `bun x vitest run src/lib/transcript.test.ts` (in apps/web). Expected: PASS.
- [ ] **Step 5: Ledger it.**

### Task 6: Workspace side-task model

**Files:**
- Modify: `hosts/rust-daemon/src/state.rs` and `control_plane_store.rs`: `side_model: Option<ModelChoice>` with `#[serde(default)]`
- Create: `hosts/rust-daemon/src/routes/side_model.rs` (`GET`/`PUT /api/settings/side-model`), register it in `routes/mod.rs`
- Modify: the side-call sites
  - `agent_runs/approvals.rs:85-141` (smart check)
  - `agent_runs/titles.rs:123-140`
  - `agent_runs/compact.rs:172-250`
  - `learning/service.rs:514-536` with `shadow_config` at `:816`
- Test: `routes/tests/side_model.rs`, plus one test per site

**Interfaces:**
- **Consumes:** `ModelChoice` (Task 4), `classify_model_error` (Task 1).
- **Produces:** `pub(crate) async fn side_call<F, Fut, T>(state: &SharedDaemonState, agent_config: &AgentConfig, call: F) -> Result<T, String>`, where `F: Fn(AgentConfig) -> Fut`.
  - When a side model is set, run `call` with the config's provider and model swapped to it.
  - If that fails with any error, run `call` with the agent's own config.
  - Otherwise run the agent's config once.
- `SecondaryCall::for_config` records whichever config actually ran.

- [ ] **Step 1: Write the failing tests:**
  - `side_model_round_trips_and_validates`: a PUT with an unknown provider → 400; `null` clears.
  - `titles_use_the_side_model_when_set`: the scripted adapter sees `provider = side provider`.
  - `a_failing_side_model_falls_back_to_the_agents_model`
  - `no_side_model_is_unchanged`
- [ ] **Step 2: Run** `cargo test -p anima-daemon --lib side_model`. Expected: FAIL.
- [ ] **Step 3: Implement** `side_call` and use it at the four sites.
- [ ] **Step 4: Run** `cargo test -p anima-daemon --lib -- side_model titles compact approval learning`. Expected: PASS.
- [ ] **Step 5: Ledger it.**

### Task 7: Key routes

**Files:**
- Create: `hosts/rust-daemon/src/routes/provider_keys.rs`, `routes/contracts/provider_keys.rs`, `routes/tests/provider_keys.rs`
- Modify: `routes/mod.rs` (register the routes and add `mod provider_keys;` to tests), `hosts/rust-daemon/README.md` route inventory, `routes/tests/docs_contracts.rs` (add the new operations and bump the count)
- Modify: `GET /api/providers` (`routes/mod.rs:2333`): `configured` becomes true when the pool has any ready key, env or file

**Interfaces:**
- **Consumes:** `KeyPool` (Task 2) through `DaemonState.provider_keys`.
- **Produces:** the five routes in the spec's API table.
  - Errors: `400` (unknown provider, empty key, over 2,000 characters, bad strategy), `409` (duplicate, or removing the env key). Without a key file the list carries `persisted: false`.

- [ ] **Step 1: Write the failing tests:**
  - `keys_are_added_listed_masked_and_removed`: the body never contains the full key.
  - `key_routes_require_the_owner`: a foreign origin gets 403 on all five routes.
  - `bad_keys_and_strategies_are_rejected`
  - `providers_report_configured_from_the_pool`
  - The docs contract test with the new operations.
- [ ] **Step 2: Run** `cargo test -p anima-daemon --lib -- provider_keys docs_contracts`. Expected: FAIL.
- [ ] **Step 3: Implement** the routes and contracts and update the README rows.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Ledger it.**

### Task 8: Web console

**Files:**
- Modify: `packages/sdk/src/` (add `provider-keys.ts` and export it; add `fallbacks` to the agent types; side-model types)
- Modify: `apps/web/src/lib/daemon-api.ts`: `providerKeys`, `addProviderKey`, `removeProviderKey`, `setKeyStrategy`, `sideModel`, `setSideModel`
- Create: `apps/web/src/components/models/ProviderKeys.tsx` (+test), `apps/web/src/components/models/FallbackModels.tsx` (+test)
- Modify:
  - `SettingsPanel.tsx:428-470`: show `FallbackModels` under the agent's model picker, and a "Side-task model" picker on the Model page
  - `onboarding/ModelStep.tsx:30-45`: replace the "set ENV" hint with an "Add API key" field that calls `addProviderKey`
  - `CompanionSetup.tsx:114`: same

**Interfaces:**
- **Consumes:** the routes in Task 7, agent `PATCH` with `fallbacks`, and `GET`/`PUT /api/settings/side-model`.
- **Copy:**

| Element | Text |
|---|---|
| Keys section heading | "API keys" |
| Keys section help | "Several keys for one provider take turns. A key that hits a limit rests and the next one is used." |
| Add button | "Add key" |
| Key status | "Ready", or "Resting until {time} · {reason}" |
| Not-persisted warning | "Keys added here last until the daemon restarts. Set ANIMAOS_RS_PROVIDER_KEYS_FILE to keep them." |
| Fallback heading | "If this model fails" |
| Fallback help | "Tried in order, only before the reply starts." |
| Side model label | "Model for small background tasks" |
| Side model help | "Titles, summaries, the smart check and task reviews. Leave empty to use each agent's own model." |

- [ ] **Step 1: Write the failing vitest tests:**
  - `adds_and_removes_a_key_and_shows_it_masked`
  - `shows_resting_keys_with_their_reason`
  - `reorders_and_saves_fallbacks_up_to_three`
  - `saves_the_side_model`
  - `onboarding_accepts_a_key_instead_of_an_env_hint`
- [ ] **Step 2: Run** `bun x vitest run src/components/models src/components/onboarding` (in apps/web). Expected: FAIL.
- [ ] **Step 3: Implement** it.
- [ ] **Step 4: Run** the same, plus `bun x nx run @animaOS-SWARM/web:typecheck`. Expected: PASS.
- [ ] **Step 5: Ledger it.**

### Task 9: Full verification and live check

- [ ] **Step 1:** Run the Global Constraints verification commands. Expected: green, apart from the 16 known macOS failures.
- [ ] **Step 2: Live check in the test console:**
  - Add a deliberately invalid OpenAI key ahead of the real one. A reply still succeeds, and Settings shows the bad key "Resting … did not accept its key".
  - Give Coder a fallback (for example Anthropic with a key) and set its primary model to a nonexistent name. The reply comes from the fallback with the "Answered by …" note.
  - Set a side-task model and start a new chat. The title appears, and its usage is recorded under the side model.
- [ ] **Step 3:** Update the spec with implementation notes.
