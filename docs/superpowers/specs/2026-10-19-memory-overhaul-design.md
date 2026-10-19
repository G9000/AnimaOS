# Memory overhaul: core memory, real embeddings, conversation reviews

**Date:** 2026-10-19 · **Status:** owner decisions recorded below; plan at `docs/superpowers/plans/2026-10-19-memory-overhaul.md`

## Why

AnimaOS stores memory well (typed facts, entities, temporal facts, hybrid BM25 + vector recall, undo, retention), but the agent uses it badly:

- **Two kinds of saved memory never reach the agent.** Context only takes `Fact`s with no session (`anima-memory/src/memory_manager.rs:125-135`).
  - "Save to memory" in chat sends a `sessionId` (`apps/web/src/hooks/useSaveToMemory.ts:80-89`), so those facts are never used.
  - Learning-review lessons are saved as `Reflection`s (`learning/service.rs:692`), so they are never used either.
- **No always-on memory.** Context is 6 search hits picked by the current message. Something the agent learned about the owner shows up only if the new message happens to match it. The 6 owner-picked pins help, but only the owner can change them.
- **The default vectors are word hashes**, not meaning. The real multilingual model only runs when an env var asks for it.
- **Every reply writes a `Reflection`** ("evaluated response: …"), plus facts from a phrase matcher. This is noise that the context filter already ignores.
- **Reviews are rare.** They run only after tasks with 5+ tool calls, at most 2 a day, so ordinary conversation never teaches the agent anything.
- **Nothing scans memory writes** for prompt injection or secrets.
- **No export.**
- **Memory is lost at restart** unless a memory store env var is set, even when the control plane is saved to a file.

Hermes, by contrast, keeps two small always-loaded notes (`USER.md` at 1,375 characters, `MEMORY.md` at 2,200). The agent edits them with `add` / `replace` / `remove`. A background review every 10 user turns updates them, and writes are threat-scanned.

## Owner decisions

1. **Core memory**: two always-on blocks per agent, maintained by the agent like Hermes.
   - **About you**: the owner's profile, 1,500 characters.
   - **Agent notes**: what the agent has learned about its work, its environment and itself, 2,500 characters.
2. **Where core memory lives** (Claude's recommendation, taken by default after the owner asked for a plainer explanation):
   - The **memory store** is the source of truth, so core memory gets history, undo, the console, export and scanning.
   - The daemon also writes a **read-only file copy** (`USER.md`, `MEMORY.md`) so the owner can open it in an editor, and git or other tools can read it.
   - Edits to the copy are overwritten; edits go through the console or the agent.
3. **Today's 6 pins fold into core memory.** Profile pins become "About you" entries and agent-note pins become "Agent notes" entries. The pin feature then goes away.
4. **Embeddings.**
   - `fastembed` with `multilingual-e5-small` becomes the default, downloaded on first use.
   - Until the download finishes, or if it fails (for example offline), recall keeps working on hashed vectors plus BM25.
5. **Conversation reviews run about every 10 owner messages**, in addition to task reviews.
6. **The per-reply evaluator and its phrase matcher are removed.** "Remember X" goes to core memory through the agent.
7. **Memory writes are threat-scanned** with the existing agent-file scanner.
8. **Export**: JSON (everything) and Markdown (readable), from the console.

## Behaviour

### Context bugs

- **Saved from chat.** These facts no longer carry a session.
  - Boot repair: existing facts tagged `saved-from-chat` have their `session_id` cleared, once, and the repair is persisted. `MemoryPatch` can't change a session today, so `anima-memory` gains a narrow `detach_session(id)` edit.
  - Facts the agent saved with a session through `memory_add` keep their session, as today.
- **Lessons.** Context eligibility also accepts a `Reflection` tagged `learning-review`. Lessons stay Reflections, so their history and console labels don't change.

### Core memory

- **Entries.**
  - A core entry is a `Fact` in the memory store with tag `core:user` (About you) or `core:notes` (Agent notes).
  - It is private to its agent, with no room, world or session.
  - Entries keep creation order.
- **Budgets** count the characters of all entries in a block:
  - About you: 1,500.
  - Agent notes: 2,500.
  - An entry is at most 500 characters.
- **Every run's context starts with both blocks**, before the searched memories:

  ```
  ## About you [1,234/1,500 chars]
  - prefers metric units
  - …
  ## Agent notes [310/2,500 chars]
  - …
  ```

  - **Empty blocks** are left out.
  - **Search results** never repeat a core entry.
  - **Snapshot timing.** The blocks are read once at run start. An edit during a run shows in the tool result and reaches context on the next run.
- **Tool `core_memory`** takes `{ action: add | replace | remove, block: user | notes, text?, old_text? }`. It follows Hermes's semantics:
  - **`old_text`** is a unique substring of one entry. It is an error if it matches none or several.
  - **`add`**
    - Over budget: refused with "About you is full (1,480/1,500 chars). Replace or remove an entry first."
    - Duplicate: an exact duplicate entry is a no-op success.
  - **`replace`** swaps the matched entry's text and keeps its position.
  - **`remove`** deletes the matched entry. It is an ordinary memory delete, so undo works.
  - **Approval:** the same policy as `memory_add`.
  - **Description:** the tool description tells the agent to use it when the owner says "remember …", corrects it, or states a lasting preference. Detailed or one-off facts go to `memory_add`.
- **Owner edits** go through the same rules (budget, uniqueness), via `/api/agents/{id}/core-memory`. The console shows both blocks with a usage bar and lets the owner add, edit and delete entries. Entries keep creation order; there is no reordering.
- **File copy.**
  - **When:** after every committed change, and once at boot.
  - **Where:** `<workspace>/.animaos-swarm/agent-profiles/<hex>/USER.md` and `MEMORY.md`. That is the same directory as the profile `SOUL.md`; these files are not loaded as agent files.
  - **Contents:** a one-line header ("Generated by AnimaOS from core memory. Edits here are overwritten; edit in the console.") followed by `- entry` lines.
  - **Agent writes** to these two paths are refused outright with a message pointing at `core_memory`, instead of asking for approval.
  - **Failures:** a failed copy write is logged and never fails the memory change.
- **Pin migration**, at boot when the control plane still holds memory-view selections:
  - Each selected fact that is still eligible gets the matching core tag, while the block's budget allows.
  - Facts that don't fit are left untouched and logged.
  - The selections are then dropped from the control plane.
  - The `memory-views` routes, `memory_views.rs` and `ProfileMemoryPanel` are removed.

### Durable memory by default

- **Default location:** when no memory store is configured (`ANIMAOS_RS_MEMORY_FILE`, `ANIMAOS_RS_MEMORY_SQLITE_FILE`, Postgres) but `ANIMAOS_RS_CONTROL_PLANE_FILE` is set, memory is kept in `memory.sqlite` next to the control-plane file.
- **Embedding vectors** already default next to the memory store.
- **Fully ephemeral daemons** (no control-plane file) stay memory-only, as today.

### Embeddings

- **Default.** The default mode is `fastembed` (`multilingual-e5-small`). `ANIMAOS_RS_MEMORY_EMBEDDINGS=local` keeps today's hashed vectors, and `off` still disables vectors.
- **Cache.** The model is cached in `ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR`. If that isn't set, it goes in `models/` next to the memory store, or, with no store, in a per-user cache directory (`~/Library/Caches/animaos/models` on macOS, else `$XDG_CACHE_HOME/animaos/models` or `~/.cache/animaos/models`).
- **Boot never waits for the download.**
  - The daemon starts on hashed vectors and loads the model in the background.
  - When the model is ready, the runtime swaps embedders and rebuilds every vector in the background.
  - Recall uses BM25 alone for memories not yet re-embedded, so hashed and model vectors are never mixed in one search.
- **Failure.** If loading fails, the daemon stays on hashed vectors and logs the reason. It does not retry until restart.
- **Status.** The memory readiness route and the console's search status show `downloading`, `rebuilding (n/total)`, `ready`, or `fallback: <reason>`.
- **Other providers.** The OpenAI, Ollama and compatible providers are unchanged.

### Per-reply evaluator removed

- `default_evaluators` returns no evaluator, and `ReflectionMemoryEvaluator` and its phrase matcher (`extract_explicit_user_memory`) are deleted.
- Existing `memory-evaluator` Reflections stay where they are. They are already out of context, and the owner can delete them in the console. Nothing is deleted automatically.
- Known cost: relationships and temporal facts no longer grow on their own. They come only from tools and reviews.

### Conversation reviews

- **Counting.** The daemon counts owner messages per agent, across sessions. Check-ins, silent replies and swarm or helper traffic don't count. The count is persisted in the control plane.
- **Trigger.** At 10 messages, after the reply finishes, a review with trigger `Conversation` is admitted, and the count resets.
  - The review needs learning to be on for the agent (`learning_auto_allowed`).
  - The same run is never reviewed twice.
- **Evidence.** The last 10 owner messages and the agent's replies, under the existing brief limit, plus the current core-memory blocks.
- **Prompt.** Conversation reviews get their own prompt. The task-review prompt forbids personal facts, which is right for tasks but would block "About you".
- **Output.**
  - Up to 5 core-memory proposals (`add` / `replace` / `remove` on a block, with a reason), plus lesson proposals as today.
  - **Free mode** applies them through the same `core_memory` rules.
  - **Strict mode** lists them on the review card for the owner to accept or reject.
  - A proposal that breaks a rule (over budget, no match, flagged by the scanner) is shown as failed with the reason.
- **Model.** Reviews use the side-task model when one is set, then the agent's chain, as learning reviews do today.
- **Limits.** Conversation reviews have their own limit of 8 per agent per 24 hours. Task reviews keep their 2 per day.

### Memory write scanning

- **Shared scanner.** `agent_files::scan` moves to a shared `threat_scan` module (agent files keep using it).
- **Writes that are refused.** A flagged write names the pattern ids ("memory not saved: looks like prompt_injection"). These writes are scanned:
  - `memory_add`, `memory_update` and `core_memory`.
  - Lessons applied by reviews.
  - Core-memory proposals.
- **Owner writes from the console** are not blocked. The console shows a warning with the pattern ids.
- **Core blocks are scanned again at render.** A flagged entry renders as `[entry hidden: flagged prompt_injection]`, which covers migrated pins and older data.

### Export

- **Routes.**
  - `GET /api/memories/export?format=json|md&agentId=…`
  - Owner-only, `Cache-Control: no-store`, sent as a download (`Content-Disposition: attachment`).
- **JSON** holds `{ version: 1, exportedAt, agents: [{ id, name, core: { user, notes }, memories, entities, relationships, temporalFacts }] }` with every memory type. The all-agents export also has `unattributed: { entities, relationships, temporalFacts }` for records that belong to no agent. Embedding vectors are not included.
- **Markdown** holds, per agent, the two core blocks followed by facts grouped by type, newest first.
- **Console.** The Memory page gets an Export menu (JSON or Markdown, this agent or all agents).
- Import is out of scope.

### Pluggable external memory provider

The owner asked for a plug-in point so an external memory service can be added later, with the local store as the default.

- **Model** (Hermes's: a built-in store plus at most one external provider):
  - The local store is always on.
  - Core memory, history, undo, scanning and export stay local whatever provider is chosen.
  - At most one external provider runs **beside** it. It doesn't replace the store.
- **Interface:** trait `ExternalMemoryProvider` in `anima-memory`, so every host can use it.

  | Method | Called when | Does |
  |---|---|---|
  | `id()` | at registration | a stable name, e.g. `none`, `mem0` |
  | `recall(agent_id, query, limit)` | building each run's context | returns `[{ text, score, sourceId? }]` |
  | `sync_turn(agent_id, session_id, user_text, reply_text, at_ms)` | after a completed owner turn | lets the service learn from the conversation |
  | `on_memory_write(agent_id, action, memory_type, memory_id, content)` | after a local memory change is persisted | mirrors adds, updates and removes |
  | `status()` | readiness route | `ready`, or `unavailable: <reason>` |

- **Default:** `none`, which does nothing, so behaviour is identical to having no provider.
- **Selection:** `ANIMAOS_RS_MEMORY_PROVIDER=<id>` at boot. An unknown id stops boot with the list of known ids. Provider settings and keys come from the provider's own env vars and are never logged.
- **Recall is untrusted:**
  - It gets 2 seconds inside the existing context deadline.
  - Each item is threat-scanned, and flagged items are dropped.
  - Results are capped at 1,500 characters.
  - They render after the local results, under "From <id> (external, unverified)".
  - An error or timeout leaves the section out and never fails the run.
- **Sync and mirroring:**
  - Run in the background and never delay a reply.
  - Errors are logged and not retried.
  - The same turns are left out as for conversation reviews (check-ins, silent replies, swarm and helper traffic).
- **Status:** the console's memory status shows the active provider and its status.
- **Not in this task:** any real external provider, provider-defined tools, and choosing a provider from the console. A later task can add, for example, an HTTP adapter or a Mem0 provider by implementing the trait and registering an id.

## Not in scope

- Importing memory, or any concrete external memory provider (Honcho, Mem0 and others). Only the plug-in point is built; see above.
- Session summaries or episodic memory.
- Changing BM25, retention or undo.
- Swarm members writing core memory. They read their own agent's blocks only.

## Implementation notes (2026-10-20)

- **Where it lives:**
  - `threat_scan.rs`: the shared scanner. It is used for memory writes, and agent files re-export it.
  - `core_memory/`: the edit rules (`mod.rs`), the shared commit (`commit.rs`), the file copy (`files.rs`) and the pin migration (`migration.rs`).
  - `routes/core_memory.rs` and `routes/memory_export.rs`.
  - `learning/conversation.rs`: owner-turn counting and core proposals.
  - `memory_embeddings.rs`: the background model load and status.
  - `memory_provider.rs` and `anima-memory/src/external_provider.rs`.
  - Console: `CoreMemoryPanel.tsx` and `ExportMenu.tsx`.
- **Departures from the text above:**
  - Saved lessons (Reflections tagged `learning-review`) reach context through the daemon's visibility rule. The core crate admits Fact and Reflection, and the daemon filters which ones get through.
  - `memory_update` and `memory_consolidate` refuse core entries. Generic owner routes refuse core tags and PATCHes of core entries.
  - A core entry is `core:*`-tagged and eligible. A tagged memory that isn't eligible behaves as an ordinary memory everywhere.
  - Rendering stops at each block's budget and adds "N more not shown".
  - `old_text` matching ignores whitespace differences.
  - Flagged core entries are also hidden in `memory_search` and `recent_memories`.
  - Retention never touches core entries.
  - The all-agents export adds `unattributed: { entities, relationships, temporalFacts }`.
  - Conversation reviews keep their own retained cap: 200 per agent. The oldest settled review is pruned first.
  - Owner turns count only from the web, the API, a private Telegram chat, or a Discord DM. Discord connectors bind guild channels only, so Discord turns never count today.
  - Undo of a review edit refuses if the entry changed since the review.
  - The model loads on a detached thread, so Ctrl-C during the first download doesn't hang.
  - With only `ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS` set, the daemon keeps hashed `local` vectors and logs a warning.
  - The external provider's mirror and sync calls go through one ordered queue: 1,024 events, a 10 s timeout per call, and drop-with-warning when full. `status()` has a 2 s timeout. Provider panics are caught.
- **Known limits:**
  - No memory-delete undo exists outside review-applied core edits.
  - Relationships and temporal facts no longer grow from replies.
  - The two minor residuals once parked in the plan ledger are closed (see "Follow-ups closed" below).
- **Verified live** (test console, 2026-10-20):
  1. "Remember …" made a `core_memory` add.
  2. The entry survived a restart and was used in another session.
  3. A fact saved from chat was recalled in a different session.
  4. The 10th owner message triggered a review that applied 3 core edits in Free mode.
  5. The scanner flagged and hid an injection entry, and `MEMORY.md` showed it hidden.
  6. The model downloaded and reached "ready". A Spanish question matched the English fact best.
  7. Export worked in JSON and Markdown.
  8. `USER.md` and `MEMORY.md` matched the blocks.
- **Follow-ups closed** (2026-10-20):
  - Owner turns typed from the web console into a Telegram or Discord thread count again: `learning::counts_as_owner_turn` admits a connector run whose first user message passes `sessions::is_owner_web_turn`. Shared-chat messages from other people still don't count.
  - A generic edit (`PATCH /api/memories/{id}` or `memory_update`) that makes a legacy core-tagged memory eligible must pass `core_memory::generic_edit_fits` (entry limit, then the block budget). Otherwise it is undone and refused: the route answers 409 with the core-memory route hint, and the tool gives the core-entry error.
  - Swarm members can't write core memory, even if their config lists `core_memory`: `state/swarm_tools.rs` refuses the tool with a short error.
