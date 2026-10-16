# anima-daemon

## Connector contributions

The Rust connector contract and Telegram pilot support reviewed, trusted source
contributions compiled into this host. See [Contributing connectors](CONNECTOR-CONTRIBUTING.md)
for interfaces, authority boundaries and verification. Community adapters require
a rebuild and run with host privileges; installable modules are deferred.

Reviewed [community bundles](../../docs/companion/community-bundles.md) can save
declarative MCP configuration and inline SKILL.md drafts. Installation creates
disabled connections and pending content, followed by existing separate owner
start, descriptor grants and exact skill approval. It does not install executable
artifacts or native connector modules.

## Scoped browser work

M46 adds four separately granted browser tools for ordinary owner chat runs,
backed by a host-owned Playwright worker and explicit immutable startup policies.
See [Browser work](../../docs/companion/browser-work.md) for installation,
`ANIMAOS_RS_BROWSER_CONFIG_FILE`, one-use chat selection and output handling. The
initial controlled loopback/Windows boundary is narrower than general public
browsing or account automation; current acceptance status is in the
[handover](../../docs/superpowers/handover/2026-09-28-companion-console.md).

## API route inventory

This marked inventory follows `ApiDoc::openapi()`. Detailed authorization and limits
are documented below and enforced by handler tests. `/health` aliases `/api/health`;
`/docs`, `/docs/`, `/openapi.json` and `/metrics` are mounted utility endpoints outside
this generated operation table. Parameter names identify the same scoped resource
as the router placeholders. The legacy blocking `/run` remains a compatibility API;
the companion console sends to session `/runs` and consumes `/events`.

<!-- openapi-route-inventory:start -->

| Method   | Path                                                                                         |
| -------- | -------------------------------------------------------------------------------------------- |
| `DELETE` | `/api/agents/{agent_id}`                                                                     |
| `DELETE` | `/api/agents/{agent_id}/approval-rules/{rule_id}`                                            |
| `DELETE` | `/api/agents/{agent_id}/avatar`                                                              |
| `DELETE` | `/api/agents/{agent_id}/community-bundles/{bundle_id}`                                       |
| `DELETE` | `/api/agents/{agent_id}/connectors/discord/{connector_id}`                                   |
| `DELETE` | `/api/agents/{agent_id}/connectors/gcalendar/{connector_id}`                                 |
| `DELETE` | `/api/agents/{agent_id}/connectors/gdrive/{connector_id}`                                    |
| `DELETE` | `/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}`                           |
| `DELETE` | `/api/agents/{agent_id}/connectors/{connector_id}`                                           |
| `DELETE` | `/api/agents/{agent_id}/mcp-connections/{connection_id}`                                     |
| `DELETE` | `/api/agents/{agent_id}/mcp-connections/{connection_id}/credential`                          |
| `DELETE` | `/api/agents/{agent_id}/schedules/{schedule_id}`                                             |
| `DELETE` | `/api/agents/{agent_id}/sessions/{session_id}`                                               |
| `DELETE` | `/api/agents/{agent_id}/sessions/{session_id}/voice/speech/{operation_id}`                   |
| `DELETE` | `/api/agents/{agent_id}/sessions/{session_id}/voice/transcriptions/{operation_id}`           |
| `DELETE` | `/api/connectors/oauth-apps/{provider}`                                                      |
| `DELETE` | `/api/memories/entities/{entity_id}`                                                         |
| `DELETE` | `/api/memories/facts/{fact_id}`                                                              |
| `DELETE` | `/api/memories/{memory_id}`                                                                  |
| `DELETE` | `/api/providers/chatgpt`                                                                     |
| `DELETE` | `/api/providers/chatgpt/login`                                                               |
| `DELETE` | `/api/skills/{slug}`                                                                         |
| `GET`    | `/api/agents`                                                                                |
| `GET`    | `/api/agents/{agent_id}`                                                                     |
| `GET`    | `/api/agents/{agent_id}/agent-files`                                                         |
| `GET`    | `/api/agents/{agent_id}/approval-policy`                                                     |
| `GET`    | `/api/agents/{agent_id}/approval-rules`                                                      |
| `GET`    | `/api/agents/{agent_id}/attachments/{attachment_id}`                                         |
| `GET`    | `/api/agents/{agent_id}/avatar`                                                              |
| `GET`    | `/api/agents/{agent_id}/browser-policies`                                                    |
| `GET`    | `/api/agents/{agent_id}/browser-scopes`                                                      |
| `GET`    | `/api/agents/{agent_id}/community-bundles`                                                   |
| `GET`    | `/api/agents/{agent_id}/community-bundles/{bundle_id}`                                       |
| `GET`    | `/api/agents/{agent_id}/connectors`                                                          |
| `GET`    | `/api/agents/{agent_id}/connectors/discord`                                                  |
| `GET`    | `/api/agents/{agent_id}/connectors/gcalendar`                                                |
| `GET`    | `/api/agents/{agent_id}/connectors/gcalendar/{connector_id}/writes`                          |
| `GET`    | `/api/agents/{agent_id}/connectors/gdrive`                                                   |
| `GET`    | `/api/agents/{agent_id}/connectors/mail/{provider}`                                          |
| `GET`    | `/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts`                    |
| `GET`    | `/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/messages`                  |
| `GET`    | `/api/agents/{agent_id}/connectors/{connector_id}/messages`                                  |
| `GET`    | `/api/agents/{agent_id}/events`                                                              |
| `GET`    | `/api/agents/{agent_id}/jobs`                                                                |
| `GET`    | `/api/agents/{agent_id}/learning-policy`                                                     |
| `GET`    | `/api/agents/{agent_id}/learning-reviews`                                                    |
| `GET`    | `/api/agents/{agent_id}/learning-reviews/{review_id}`                                        |
| `GET`    | `/api/agents/{agent_id}/mcp-connections`                                                     |
| `GET`    | `/api/agents/{agent_id}/mcp-connections/{connection_id}`                                     |
| `GET`    | `/api/agents/{agent_id}/memories/recent`                                                     |
| `GET`    | `/api/agents/{agent_id}/memory-views`                                                        |
| `GET`    | `/api/agents/{agent_id}/memory-views/candidates`                                             |
| `GET`    | `/api/agents/{agent_id}/runs/{run_id}`                                                       |
| `GET`    | `/api/agents/{agent_id}/runs/{run_id}/browser-scope`                                         |
| `GET`    | `/api/agents/{agent_id}/schedules`                                                           |
| `GET`    | `/api/agents/{agent_id}/schedules/{schedule_id}/history`                                     |
| `GET`    | `/api/agents/{agent_id}/sessions`                                                            |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}`                                               |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}/export`                                        |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}/messages`                                      |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}/runs`                                          |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}/voice`                                         |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}/voice/speech`                                  |
| `GET`    | `/api/agents/{agent_id}/tasks`                                                               |
| `GET`    | `/api/approvals`                                                                             |
| `GET`    | `/api/capabilities`                                                                          |
| `GET`    | `/api/connectors/gcalendar/callback`                                                         |
| `GET`    | `/api/connectors/gdrive/callback`                                                            |
| `GET`    | `/api/connectors/mail/{provider}/callback`                                                   |
| `GET`    | `/api/connectors/oauth-apps/{provider}`                                                      |
| `GET`    | `/api/goals`                                                                                 |
| `GET`    | `/api/goals/{goal_id}/jobs`                                                                  |
| `GET`    | `/api/health`                                                                                |
| `GET`    | `/api/logs`                                                                                  |
| `GET`    | `/api/logs/stream`                                                                           |
| `GET`    | `/api/memories/entities`                                                                     |
| `GET`    | `/api/memories/facts`                                                                        |
| `GET`    | `/api/memories/readiness`                                                                    |
| `GET`    | `/api/memories/recall`                                                                       |
| `GET`    | `/api/memories/recent`                                                                       |
| `GET`    | `/api/memories/relationships`                                                                |
| `GET`    | `/api/memories/search`                                                                       |
| `GET`    | `/api/memories/{memory_id}/trace`                                                            |
| `GET`    | `/api/providers`                                                                             |
| `GET`    | `/api/providers/chatgpt/models`                                                              |
| `GET`    | `/api/providers/chatgpt/status`                                                              |
| `GET`    | `/api/ready`                                                                                 |
| `GET`    | `/api/search`                                                                                |
| `GET`    | `/api/skill-drafts`                                                                          |
| `GET`    | `/api/skills`                                                                                |
| `GET`    | `/api/skills/{slug}`                                                                         |
| `GET`    | `/api/status`                                                                                |
| `GET`    | `/api/swarms`                                                                                |
| `GET`    | `/api/swarms/{swarm_id}`                                                                     |
| `GET`    | `/api/swarms/{swarm_id}/events`                                                              |
| `GET`    | `/api/usage/export.csv`                                                                      |
| `GET`    | `/api/usage/pricing`                                                                         |
| `GET`    | `/api/usage/records`                                                                         |
| `GET`    | `/api/usage/summary`                                                                         |
| `GET`    | `/api/workspace`                                                                             |
| `GET`    | `/api/workspace/avatar`                                                                      |
| `GET`    | `/api/workspace/file`                                                                        |
| `GET`    | `/api/workspace/files`                                                                       |
| `GET`    | `/api/workspace/inspect`                                                                     |
| `PATCH`  | `/api/agents/{agent_id}`                                                                     |
| `PATCH`  | `/api/agents/{agent_id}/community-bundles/{bundle_id}`                                       |
| `PATCH`  | `/api/agents/{agent_id}/connectors/discord/{connector_id}`                                   |
| `PATCH`  | `/api/agents/{agent_id}/learning-policy`                                                     |
| `PATCH`  | `/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}`                |
| `PATCH`  | `/api/agents/{agent_id}/mcp-connections/{connection_id}`                                     |
| `PATCH`  | `/api/agents/{agent_id}/mcp-connections/{connection_id}/grants/{alias}`                      |
| `PATCH`  | `/api/agents/{agent_id}/memory-views`                                                        |
| `PATCH`  | `/api/agents/{agent_id}/memory-views/sources/{memory_id}`                                    |
| `PATCH`  | `/api/agents/{agent_id}/schedules/{schedule_id}`                                             |
| `PATCH`  | `/api/agents/{agent_id}/sessions/{session_id}`                                               |
| `PATCH`  | `/api/memories/facts/{fact_id}`                                                              |
| `PATCH`  | `/api/memories/{memory_id}`                                                                  |
| `PATCH`  | `/api/skills/{slug}`                                                                         |
| `POST`   | `/api/agencies/create`                                                                       |
| `POST`   | `/api/agencies/generate`                                                                     |
| `POST`   | `/api/agents`                                                                                |
| `POST`   | `/api/agents/generate-profile`                                                               |
| `POST`   | `/api/agents/{agent_id}/approval-rules`                                                      |
| `POST`   | `/api/agents/{agent_id}/community-bundles`                                                   |
| `POST`   | `/api/agents/{agent_id}/community-bundles/reviews`                                           |
| `POST`   | `/api/agents/{agent_id}/community-bundles/{bundle_id}/update`                                |
| `POST`   | `/api/agents/{agent_id}/connectors/discord`                                                  |
| `POST`   | `/api/agents/{agent_id}/connectors/discord/{connector_id}/activate`                          |
| `POST`   | `/api/agents/{agent_id}/connectors/discord/{connector_id}/outbound/suppress`                 |
| `POST`   | `/api/agents/{agent_id}/connectors/discord/{connector_id}/restart`                           |
| `POST`   | `/api/agents/{agent_id}/connectors/discord/{connector_id}/retry`                             |
| `POST`   | `/api/agents/{agent_id}/connectors/gcalendar`                                                |
| `POST`   | `/api/agents/{agent_id}/connectors/gcalendar/{connector_id}/writes/{write_id}/approve`       |
| `POST`   | `/api/agents/{agent_id}/connectors/gcalendar/{connector_id}/writes/{write_id}/reject`        |
| `POST`   | `/api/agents/{agent_id}/connectors/gdrive`                                                   |
| `POST`   | `/api/agents/{agent_id}/connectors/mail/{provider}`                                          |
| `POST`   | `/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts`                    |
| `POST`   | `/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts/{draft_id}/approve` |
| `POST`   | `/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts/{draft_id}/reject`  |
| `POST`   | `/api/agents/{agent_id}/connectors/telegram`                                                 |
| `POST`   | `/api/agents/{agent_id}/connectors/{connector_id}/messages`                                  |
| `POST`   | `/api/agents/{agent_id}/connectors/{connector_id}/pairings/{chat_id}/approve`                |
| `POST`   | `/api/agents/{agent_id}/connectors/{connector_id}/restart`                                   |
| `POST`   | `/api/agents/{agent_id}/jobs`                                                                |
| `POST`   | `/api/agents/{agent_id}/jobs/{job_id}/approve`                                               |
| `POST`   | `/api/agents/{agent_id}/jobs/{job_id}/cancel`                                                |
| `POST`   | `/api/agents/{agent_id}/jobs/{job_id}/retry`                                                 |
| `POST`   | `/api/agents/{agent_id}/jobs/{job_id}/review`                                                |
| `POST`   | `/api/agents/{agent_id}/learning-reviews/{review_id}/cancel`                                 |
| `POST`   | `/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}/skill-draft`    |
| `POST`   | `/api/agents/{agent_id}/mcp-connections`                                                     |
| `POST`   | `/api/agents/{agent_id}/mcp-connections/{connection_id}/connect`                             |
| `POST`   | `/api/agents/{agent_id}/mcp-connections/{connection_id}/disconnect`                          |
| `POST`   | `/api/agents/{agent_id}/mcp-connections/{connection_id}/discover`                            |
| `POST`   | `/api/agents/{agent_id}/mcp-connections/{connection_id}/review`                              |
| `POST`   | `/api/agents/{agent_id}/messages`                                                            |
| `POST`   | `/api/agents/{agent_id}/run`                                                                 |
| `POST`   | `/api/agents/{agent_id}/runs/{run_id}/browser-scope/stop`                                    |
| `POST`   | `/api/agents/{agent_id}/runs/{run_id}/learning-review`                                       |
| `POST`   | `/api/agents/{agent_id}/runs/{run_id}/stop`                                                  |
| `POST`   | `/api/agents/{agent_id}/schedules`                                                           |
| `POST`   | `/api/agents/{agent_id}/schedules/import`                                                    |
| `POST`   | `/api/agents/{agent_id}/schedules/{schedule_id}/run`                                         |
| `POST`   | `/api/agents/{agent_id}/sessions`                                                            |
| `POST`   | `/api/agents/{agent_id}/sessions/{session_id}/attachments`                                   |
| `POST`   | `/api/agents/{agent_id}/sessions/{session_id}/compact`                                       |
| `POST`   | `/api/agents/{agent_id}/sessions/{session_id}/runs`                                          |
| `POST`   | `/api/agents/{agent_id}/sessions/{session_id}/voice/speech`                                  |
| `POST`   | `/api/agents/{agent_id}/sessions/{session_id}/voice/transcriptions`                          |
| `POST`   | `/api/approvals/{approval_id}/decision`                                                      |
| `POST`   | `/api/goals`                                                                                 |
| `POST`   | `/api/goals/{goal_id}/status`                                                                |
| `POST`   | `/api/memories`                                                                              |
| `POST`   | `/api/memories/entities`                                                                     |
| `POST`   | `/api/memories/evaluated`                                                                    |
| `POST`   | `/api/memories/evaluations`                                                                  |
| `POST`   | `/api/memories/relationships`                                                                |
| `POST`   | `/api/memories/retention`                                                                    |
| `POST`   | `/api/providers/chatgpt/login`                                                               |
| `POST`   | `/api/providers/chatgpt/models/refresh`                                                      |
| `POST`   | `/api/schedules/preview`                                                                     |
| `POST`   | `/api/skill-drafts/{draft_id}/approve`                                                       |
| `POST`   | `/api/skill-drafts/{draft_id}/reject`                                                        |
| `POST`   | `/api/skills/import`                                                                         |
| `POST`   | `/api/skills/{slug}/approve`                                                                 |
| `POST`   | `/api/swarms`                                                                                |
| `POST`   | `/api/swarms/{swarm_id}/run`                                                                 |
| `POST`   | `/api/workspace/bootstrap`                                                                   |
| `POST`   | `/api/workspace/pick-folder`                                                                 |
| `POST`   | `/api/workspace/resume`                                                                      |
| `PUT`    | `/api/agents/{agent_id}/approval-policy`                                                     |
| `PUT`    | `/api/agents/{agent_id}/avatar`                                                              |
| `PUT`    | `/api/agents/{agent_id}/connectors/discord/{connector_id}/credential`                        |
| `PUT`    | `/api/agents/{agent_id}/connectors/{connector_id}/credential`                                |
| `PUT`    | `/api/agents/{agent_id}/mcp-connections/{connection_id}/credential`                          |
| `PUT`    | `/api/agents/{agent_id}/runs/{run_id}/browser-scope`                                         |
| `PUT`    | `/api/agents/{agent_id}/tasks`                                                               |
| `PUT`    | `/api/connectors/oauth-apps/{provider}`                                                      |
| `PUT`    | `/api/skills/{slug}`                                                                         |
| `PUT`    | `/api/usage/pricing`                                                                         |
| `PUT`    | `/api/workspace`                                                                             |
| `PUT`    | `/api/workspace/avatar`                                                                      |

<!-- openapi-route-inventory:end -->

## ChatGPT subscription

In the web console, open agent settings and choose **ChatGPT subscription** as
the provider. Click **Connect ChatGPT**, follow **Continue on OpenAI**, and enter
the displayed code. Device-code login must be enabled in your ChatGPT security
settings (or allowed by your workspace administrator). Codes expire after 15
minutes. After connection, select a model available to your account and save
the agent settings. `gpt-5.5` is a suggestion; custom model IDs are supported.

The `chatgpt` provider uses subscription inference directly while Anima retains
its agent loop, conversation history, tools, and permission checks. It does not
require Codex CLI or an API key and does not fall back to API-key billing when
subscription limits are reached. Model access and quota depend on the signed-in
account. This does not provide subscription billing for the separate embedding
provider.

The daemon owns device polling, token refresh, and OS credential-store
persistence. Tokens never go to the web client or workspace files. Disconnect
removes Anima's local credential; it does not sign out of ChatGPT in your browser.
Account endpoints require local-owner access and return `Cache-Control: no-store`:

| Method | Endpoint                        | Purpose                                       |
| ------ | ------------------------------- | --------------------------------------------- |
| GET    | `/api/providers/chatgpt/status` | Redacted account and pending login status     |
| POST   | `/api/providers/chatgpt/login`  | Start or recover pending device authorization |
| DELETE | `/api/providers/chatgpt/login`  | Cancel pending sign-in                        |
| DELETE | `/api/providers/chatgpt`        | Disconnect Anima's subscription account       |

SDK callers can use `client.chatgpt.status()`, `.login()`, `.cancel()`, and
`.disconnect()`. The production host uses the OS vault; deterministic test
routers use isolated memory and cannot initiate real authorization.

Protocol references: [OpenAI device authorization](https://learn.chatgpt.com/docs/auth#preferred-device-code-authentication-beta)
and [OpenClaw's direct subscription transport](https://github.com/openclaw/openclaw/blob/main/packages/ai/src/providers/openai-chatgpt-responses.ts).

`anima-daemon` is the runnable Rust host in `hosts/rust-daemon`. It is the
current Axum HTTP/SSE boundary for animaOS, wiring the reusable crates in
`packages/core-rust` to real infrastructure such as model providers, optional
Postgres persistence, streaming clients, and Telegram. Telegram polling,
pairing, dedicated message threads, scheduled delivery, and restart recovery
run inside this daemon; there is no second connector process.

For an implementation walkthrough, see [Rust Daemon Architecture](../../docs/rust-daemon-architecture.md).

## Native memory removal

The optional `memory_remove` tool removes one obsolete or explicitly withdrawn
active Fact record belonging to the calling agent. The owner must grant it
explicitly, including through **Allow removing saved facts** in web settings.
Registration and capability inventory do not grant it; standard profiles and
existing agent configurations stay unchanged. Existing Write Allow/Ask/Deny
policy applies, so a grant does not always produce an approval prompt.

Its only arguments are `id` (at most 256 UTF-8 bytes) and `expected_content`
(the full current text, 1–8,000 characters). Search first; do not trim, paraphrase,
use an excerpt, pass a query/batch, or force removal. Own-agent/Fact/visibility and
exact text are rechecked under the same write lock as deletion and persistence.
Foreign, hidden, non-Fact, missing and stale records are refused without mutation.

The operation uses existing snapshot rollback and citation cleanup. Related
citation references change under the established deletion policy; other memory
records remain. A reported committed-but-durability-uncertain save keeps the
committed canonical state while returning an error. Embedding cleanup is best
effort with canonical absence rechecked before index removal. Inspect an error
before retrying; it does not universally prove that removal did not occur.

This is active-record hard removal. Chats, reflections, retained temporal facts,
action receipts and backups may still contain the old text. It is not the core
MemoryPort's revisioned tombstone/forget operation, complete erasure, semantic
consolidation, or a cross-store transaction. Owner API/UI deletion remains a
separate existing operation.

## Long-running work safeguards

- The scheduler keeps scanning while jobs run, admits at most eight scheduled jobs,
  and holds at most one slot per agent through completion and persistence. Jobs
  also share the daemon's global run limit. Excess due work stays unclaimed.
- Each occurrence is persisted before execution. If startup finds a claimed
  occurrence without a terminal outcome, it disables that schedule and records
  `schedule_run_interrupted`, unless its owner had stopped that run: a stopped
  occurrence records `stopped` (`schedule_run_stopped`) and keeps the schedule
  enabled, as a stop does while the daemon runs. Review the agent's actions
  before explicitly re-enabling it. Uncertain external actions are not
  automatically replayed.
  If recovery cannot be persisted, schedule admission stays closed and retries.
  The same reconciliation handles orphaned runs after a storage failure while
  the daemon remains open; active jobs are excluded.
- Swarm `tokenBudget` is a per-dispatch **observed** token limit shared by manager
  and workers. The daemon checks it before each model request; the core also
  checks before agent runs. Zero blocks execution and omission is unlimited.
  Already in-flight requests may overshoot: this is not a hard billing cap.
  Completed totals include retired managers and are stable under UI polling.
- Browser disconnects and HTTP timeouts do not cancel admitted agent or swarm
  runs. Swarm workers retain global admission and per-swarm serialization through
  their final durable commit and completion event. Running intent is saved before
  model execution; failed commits keep that marker for interrupted recovery.
- Full checkpoint-based mission resume across daemon restarts is not yet
  integrated into the host. Interactive runs are not drained on daemon shutdown;
  persisted interrupted runs are reported as failed, not automatically replayed.

---

## Environment variables

| Variable                                              | Required                   | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------------------------------------- | -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ANTHROPIC_API_KEY`                                   | Yes (for Anthropic models) | API key for the Anthropic provider. Aliases: `ANTHROPIC_KEY`, `ANTHROPIC_TOKEN`, `CLAUDE_API_KEY`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `OPENAI_API_KEY`                                      | Yes (for OpenAI models)    | API key for OpenAI-compatible providers. Aliases: `OPENAI_KEY`, `OPENAI_TOKEN`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `DATABASE_URL`                                        | No                         | Postgres connection string. Required when `ANIMAOS_RS_PERSISTENCE_MODE=postgres`. Postgres mode runs migrations, stores host snapshots in `host_snapshots`, and injects `SqlxPostgresAdapter` for step-log persistence plus completed-tool reuse for retried requests that repeat the same metadata retry key.                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ANIMAOS_RS_HOST`                                     | No                         | Bind host (default `127.0.0.1`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `ANIMAOS_RS_PORT`                                     | No                         | Bind port (default `8080`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `ANIMA_PUBLIC_BASE_URL`                               | No                         | Public HTTPS origin for Google Calendar/Gmail/Outlook OAuth callbacks, e.g. `https://companion.example.com`. Must be a canonical origin without credentials, a path, query, or fragment; loopback HTTP is permitted for local testing. Invalid configuration fails startup. Defaults to `http://127.0.0.1:8080` when absent. Register the exact callback URLs shown in connector settings with the provider.                                                                                                                                                                                                                                                                                                   |
| `ANIMA_ALLOWED_UI_ORIGINS`                            | No                         | Comma-separated additional exact browser origins allowed to perform local-owner connector and schedule mutations. The built-in local development origins are `http://localhost:4200`, `http://127.0.0.1:4200`, `http://localhost:4201`, and `http://127.0.0.1:4201`; wildcards and suffix matching are not supported.                                                                                                                                                                                                                                                                                                                                                                                          |
| `ANIMA_LOCAL_ADMIN_TOKEN`                             | No                         | Bearer token for originless local-owner clients. When unset, originless connector and schedule mutations are disabled. The daemon snapshots this value at startup, compares it in constant time, and never returns or logs it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ANIMAOS_RS_MAX_REQUEST_BYTES`                        | No                         | Request body size limit in bytes (default `65536` / 64 KB).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `ANIMAOS_RS_REQUEST_TIMEOUT_SECS`                     | No                         | Timeout in seconds for standard API routes (default `30`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `ANIMAOS_RS_RUN_REQUEST_TIMEOUT_SECS`                 | No                         | Response timeout for agent, swarm, and connector runs (default `600`). An admitted run continues in the daemon if the HTTP response times out.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ANIMAOS_RS_PERSISTENCE_MODE`                         | No                         | Persistence mode: `memory` (default) or `postgres`. `postgres` requires `DATABASE_URL` and fails startup if Postgres is unavailable or migrations fail.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `ANIMAOS_RS_CONTROL_PLANE_FILE`                       | No                         | Host-owned JSON snapshot file for registered agents, swarms, and their latest runtime snapshots. When omitted in Postgres mode, the daemon stores the control-plane snapshot in Postgres `host_snapshots`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `ANIMAOS_RS_HISTORY_SQLITE_FILE`                      | No                         | SQLite history store for session messages and finished runs (default `history.sqlite` beside `ANIMAOS_RS_CONTROL_PLANE_FILE`). A Postgres control plane (Postgres mode without `ANIMAOS_RS_CONTROL_PLANE_FILE`) uses Postgres history tables unless this is set. Without a control-plane file or Postgres, history stays in memory (100,000 rows per table).                                                                                                                                                                                                                                                                                                                                                   |
| `ANIMAOS_RS_MEMORY_FILE`                              | No                         | Host-owned JSON snapshot file for runtime memories, entities, relationships, and temporal records. Mutating memory routes/tools/evaluators autosave the snapshot. Set only one of this and `ANIMAOS_RS_MEMORY_SQLITE_FILE`; when both are omitted in Postgres mode, memory snapshots use Postgres `host_snapshots`.                                                                                                                                                                                                                                                                                                                                                                                            |
| `ANIMAOS_RS_MEMORY_SQLITE_FILE`                       | No                         | Host-owned SQLite snapshot file for runtime memories, entities, relationships, temporal records, and, by default, embedding vectors. Memory snapshots use the daemon `memory_store_snapshots` table; embeddings use separate `memory_embeddings` tables.                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `ANIMAOS_RS_MAX_CONCURRENT_RUNS`                      | No                         | Max number of runs executing at once across the daemon (default `8`). `/api/agents/{id}/run` and `/api/swarms/{id}/run` return `503 Service Unavailable` (`too many concurrent run requests`) when none is free. An `/api/agents/{id}/run` request whose room or agent slots are busy waits for them without holding a run; up to 8 waiting requests per agent (Telegram owner sends included) are accepted, and the next `/api/agents/{id}/run` request for that agent gets the same `503` immediately, as does a waiting request that finds no free run once its room and slot are ready. An owner send beyond the 8 gets `429` (`connector_busy`); an accepted one waits for a free run instead of failing. |
| `ANIMAOS_RS_MAX_RUNS_PER_AGENT`                       | No                         | Max concurrent runs of one agent across different conversation rooms (default `3`; must be `1`-`64`). Runs in the same room always wait in order; generated helpers always run one task at a time regardless of this setting. The daemon refuses to start on an invalid value: `0`, a negative, or a non-numeric value fails with `ANIMAOS_RS_MAX_RUNS_PER_AGENT must be a positive integer`, and a value above `64` fails with `ANIMAOS_RS_MAX_RUNS_PER_AGENT must be between 1 and 64`.                                                                                                                                                                                                                      |
| `ANIMAOS_RS_MAX_BACKGROUND_PROCESSES`                 | No                         | Max number of concurrently running `bg_start` processes allowed by the daemon tool surface (default `8`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `ANIMAOS_RS_SESSION_EVENT_BUFFER`                     | No                         | Events buffered per agent for the live event stream (`GET /api/agents/{agent_id}/events`) before a slow stream lags (default `1024`, rounded up to a power of two; must be `1`-`65536`). A stream that falls behind receives `stream.resync` and keeps going. The daemon refuses to start on an invalid value: `0`, a negative, or a non-numeric value fails with `ANIMAOS_RS_SESSION_EVENT_BUFFER must be a positive integer`, and a value above `65536` fails with `ANIMAOS_RS_SESSION_EVENT_BUFFER must be between 1 and 65536`.                                                                                                                                                                            |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS`                        | No                         | Runtime memory embedding mode: `local` (default), `fastembed`, `ollama`, `openai`, `openai-compatible`, or `disabled`. `local` is deterministic and cheap for tests. `fastembed` runs a real local ONNX embedding model in process. Provider modes call an OpenAI-compatible `/embeddings` endpoint.                                                                                                                                                                                                                                                                                                                                                                                                           |
| `ANIMAOS_RS_MEMORY_TEXT_ANALYZER`                     | No                         | BM25 text analyzer profile: `multilingual`/`unicode` (default, language-neutral Unicode tokenization with CJK character/bigram terms). Core search does not use language-specific stop-word removal or stemming; Arabic scriptio continua, Thai dictionary segmentation, and Indic grapheme/word-boundary handling need future segmenter work.                                                                                                                                                                                                                                                                                                                                                                 |
| `ANIMAOS_RS_MEMORY_EMBEDDING_MODEL`                   | No                         | Embedding model. `fastembed` defaults to `intfloat/multilingual-e5-small` and also accepts aliases such as `multilingual-e5-base`, `multilingual-e5-large`, `bge-m3`, and `paraphrase-multilingual-minilm-l12-v2`. Provider modes default to `text-embedding-3-small` for `openai`/`openai-compatible` and `nomic-embed-text` for `ollama`.                                                                                                                                                                                                                                                                                                                                                                    |
| `ANIMAOS_RS_MEMORY_EMBEDDING_DIMENSIONS`              | No                         | Expected embedding vector size. `local` defaults to `96` with a minimum of `24`; `fastembed` uses the model's fixed dimension and rejects mismatches; `openai` defaults to `1536`; `ollama` defaults to `768`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_BASE_URL`               | No                         | OpenAI-compatible embedding base URL. Defaults to `https://api.openai.com/v1` for OpenAI-compatible modes and `http://127.0.0.1:11434/v1` for `ollama`. Provider-specific `OPENAI_BASE_URL` and `OLLAMA_BASE_URL` are also recognized.                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_API_KEY`                | No                         | API key for provider-backed memory embeddings. `openai` requires this or `OPENAI_API_KEY`; `ollama` and `openai-compatible` treat it as optional for local/private endpoints.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_TIMEOUT_MS`             | No                         | HTTP timeout for provider-backed embedding calls (default `15000`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_CACHE_DIR`              | No                         | Cache directory for `fastembed` model files. Defaults to fastembed's standard cache directory.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_SHOW_DOWNLOAD_PROGRESS` | No                         | Whether `fastembed` shows model download progress on first use (default `true`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE`            | No                         | SQLite file for embedding vectors. When omitted and `ANIMAOS_RS_MEMORY_SQLITE_FILE` is set, vectors use the same SQLite file in separate `memory_embeddings` tables.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |

Other provider keys follow the same pattern: `GOOGLE_API_KEY`, `GROQ_API_KEY`,
`MOONSHOT_API_KEY`, `OLLAMA_API_KEY`, and so on. Moonshot/Kimi uses the
OpenAI-compatible endpoint at `https://api.moonshot.ai/v1`. The model to use is
specified per-agent in the request body (`model`), not via a single global env
var.

The daemon emits structured logs through `tracing` and adds `x-request-id` to
HTTP responses so request logs and client-visible responses can be correlated.
The daemon also keeps the last 2,000 log lines (each at most 4 KiB) in memory
for the owner's log routes; every line is redacted at capture (API keys,
`Authorization` and `Bearer` values, OAuth and Telegram tokens, JWTs, and long
unlabeled tokens become `[redacted]`). Redaction is pattern-based, so treat
`RUST_LOG=debug` (or `trace`) with care: it lets far more detail, such as
request and provider payload fragments, into both the console and the log tail.

---

## HTTP API

This host serves both top-level operational/docs endpoints and `/api/*`
application endpoints. The summary below matches the live router in
`hosts/rust-daemon/src/routes/mod.rs`.

### Operational and docs routes

| Method | Path            | Description                                                                                                                                                                                                |
| ------ | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`  | `/health`       | Liveness check. Always returns `200 OK` with `{"status":"ok"}`.                                                                                                                                            |
| `GET`  | `/ready`        | Readiness check. Returns `200 OK` with `{"status":"ready",...}` when the daemon can serve traffic, otherwise `503` with issues.                                                                            |
| `GET`  | `/metrics`      | Prometheus-style metrics for readiness, memory/runtime counts, persistence mode, configured limits, background-process health, and the counts under Usage, logs, and health. Unauthenticated; counts only. |
| `GET`  | `/api/health`   | Same health payload as `/health`.                                                                                                                                                                          |
| `GET`  | `/api/ready`    | Same readiness payload as `/ready`.                                                                                                                                                                        |
| `GET`  | `/openapi.json` | OpenAPI document for the live daemon routes.                                                                                                                                                               |
| `GET`  | `/docs`         | Scalar API reference for exploring the daemon API in a browser.                                                                                                                                            |
| `GET`  | `/docs/`        | Scalar API reference for exploring the daemon API in a browser.                                                                                                                                            |

### ChatGPT subscription model metadata

| Method | Path                                    | Description                                                                        |
| ------ | --------------------------------------- | ---------------------------------------------------------------------------------- |
| `GET`  | `/api/providers/chatgpt/models`         | Owner-only cached current-connection snapshot; no upstream refresh or generation.  |
| `POST` | `/api/providers/chatgpt/models/refresh` | Owner-only exact `{}` body; one fixed-origin metadata GET, no retry or generation. |

Both routes use `Cache-Control: no-store`, including errors. The direct snapshot
contains state, opaque connection/catalog revisions, nullable fetch/expiry times,
exact model ID rows and a fixed error. Cache lifetime is five minutes, memory only;
restart, disconnect or principal replacement never restores or fetches a catalog.
Refresh has a ten-second total bound, returns `409` while another refresh is active,
and reports metadata failure as unavailable/stale without changing saved agents.
Catalog presence is metadata, not proof of model/tool acceptance. New selection is
deliberate; existing/custom model strings and grants remain unchanged until Save.
The registered OpenAPI inventory now contains 197 operations, including the three explicit owner-scoped voice operations.

### Agents

| Method   | Path                                     | Description                                                                                                                                                                                                                                                                                                                                      |
| -------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GET`    | `/api/agents`                            | List all registered agent snapshots; `messages` and `messageCount` cover only the hot tail (see Sessions). `?view=summary` returns them without `messages`.                                                                                                                                                                                      |
| `POST`   | `/api/agents`                            | Create an agent. Body: `AgentConfig` JSON. Returns `201` with the created snapshot.                                                                                                                                                                                                                                                              |
| `GET`    | `/api/agents/{agent_id}`                 | Get one agent snapshot; `messages` and `messageCount` cover only the hot tail (see Sessions).                                                                                                                                                                                                                                                    |
| `DELETE` | `/api/agents/{agent_id}`                 | Remove an agent runtime and return a deleted flag. `409` while one of its runs is running. Its messages still waiting to start are cancelled (`run.cancelled`, `agent_deleted`, `The companion was deleted before this message ran`), and while the deletion runs a new message to it is refused with `409` (`This companion is being deleted`). |
| `POST`   | `/api/agents/{agent_id}/run`             | Run the agent with `{"text":"..."}`. Blocks until completion and returns the updated snapshot plus task result.                                                                                                                                                                                                                                  |
| `GET`    | `/api/agents/{agent_id}/memories/recent` | Get recent memories for the agent. Optional `?limit=N`.                                                                                                                                                                                                                                                                                          |

### Sessions

Every session route requires local-owner authorization, and reads answer `Cache-Control: no-store`.

| Method   | Path                                                    | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| -------- | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`    | `/api/agents/{agent_id}/sessions`                       | Sessions, newest activity first. Optional `?kind=`, `?archived=`, `?q=`, `?cursor=`, `?limit=` (1–200, default 50), and `?includeHelpers=` (default `true`). Returns `{ sessions, nextCursor }`.                                                                                                                                                                                                                                                                                                                           |
| `POST`   | `/api/agents/{agent_id}/sessions`                       | Create a chat. Body `{ "title"?: string }`. Returns `201` with `{ session }`; `409` for a helper agent; `429` beyond 60 per minute per agent.                                                                                                                                                                                                                                                                                                                                                                              |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}`          | One session with `messageCount`, `preview`, `activeRuns`, `unread`, `capabilities`, and `usage` (the session's usage totals; `null` when they cannot be read; the only session response that carries it).                                                                                                                                                                                                                                                                                                                  |
| `PATCH`  | `/api/agents/{agent_id}/sessions/{session_id}`          | Rename, archive, or mark read: `{ "title"?, "archived"?, "lastReadAtMs"? }`.                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `DELETE` | `/api/agents/{agent_id}/sessions/{session_id}`          | Delete a session its kind allows; `409` while a run in it is active. Memories are kept.                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}/messages` | Messages oldest to newest: `?before=<messageId>&limit=50&includeHidden=false`. Returns `{ messages, nextBefore }`. Each message's `metadata` carries only `toolCalls`, `toolCallId`, `stepId`, `runId`, `toolStatus` (`success` or `error`), `toolDurationMs`, `incomplete`, `stopped`, `revised`, `steer`, `skill`, `clientRequestId`, `communication`, `kind`, and `source`, plus public `attachmentRecords` and `attachmentInputText`; a tool result's full `taskResult` is not exposed.                                |
| `GET`    | `/api/agents/{agent_id}/sessions/{session_id}/export`   | The full transcript as `text/markdown`, including messages kept only in the history store, with silent check-in turns marked rather than hidden.                                                                                                                                                                                                                                                                                                                                                                           |
| `POST`   | `/api/agents/{agent_id}/sessions/{session_id}/compact`  | Compact a session now: every turn its summary does not cover yet except the newest, and any pruned turns it does not cover, are folded into its summary by the agent's own model. Answers `202` with `{ session }` at once; the summary arrives with `session.updated`, and no run starts in the session meanwhile. `409` for a kind that cannot be compacted (`This session cannot be compacted`), nothing to compact (`Nothing to compact yet`), or a run in the session (`A run in this session is still in progress`). |

Every committed message is mirrored to the history store (`ANIMAOS_RS_HISTORY_SQLITE_FILE`, the Postgres history tables, or bounded in-memory tables in ephemeral mode). Every 10 minutes the daemon prunes mirrored messages from the control plane down to a hot tail: each session's newest 200 visible messages (silent check-in turns among them stay too) plus anything from the last 24 hours. It never prunes in ephemeral mode, while a run in the session is active, or while a Telegram delivery still needs the message. `GET /api/agents` and `GET /api/agents/{agent_id}` then cover only this hot tail: their `messages` and `messageCount` (also with `?view=summary`) count hot messages, not the full history. The session routes still cover the full history: a session's `messageCount`, its message pages, and its export.

Each run sends the model the session's newest whole turns that fit its context budget: the agent setting `contextBudgetTokens`, otherwise 60% of the model's context window (at most 200,000 tokens, below the long-context price tiers), otherwise 32,000 tokens, less `maxTokens` (default 4,096, at most half the budget) for the reply and the current message, which is always sent. Silent check-in turns, drafts an evaluator sent back, and its requests to revise them are never sent, and the session summary, when there is one, stands in for the turns it covers. When turns leave the model's view without a summary covering them, whether the budget dropped them or pruning removed them, the session's `contextTrimmed` names the newest one; a run that leaves nothing uncovered clears it. Sessions pruned before this version get their pruned-through mark on the next prune pass. Estimates use each session's calibration: the prompt tokens the provider reported for the last run's first model call over that run's estimate, clamped to 0.5–2.0.

Before a run whose context leaves out turns no summary covers, the run compacts them unless the agent setting `autoCompact` is `false`, announcing `run.progress` with `phase: "compacting"` after `run.started`: the agent's own provider and model, without tools (at most 1,024 tokens, temperature 0.2), merge the previous summary with those turns (pruned ones read back from the history store, the same turns hidden from the model left out) into a summary of at most 8 KiB, which the run then uses in their place; the session announces it with `session.updated`. If summarizing or saving fails, or the summary is still unwritten after 2 minutes (`The summary took longer than 2 minutes`; a helper's compaction also counts toward its own 2-minute deadline), the session's `compactionError` (`{ message, atMs }`) says why until a later compaction succeeds, and the run goes on with its trimmed context. A stop during compaction ends the run at once and changes neither the summary nor `compactionError`.

### Attachments and voice

Selected WAV audio remains a File reference unless the owner explicitly requests
transcription. Native transcription is disabled by default; enable
`ANIMAOS_STT_ENABLED=true` with the server OpenAI Platform API-key configuration.
It returns an ephemeral editable draft, with separate Use/Send, fixed authority,
strict bounds and local cancellation. Upload/chat never invoke it automatically.
ChatGPT subscription/OAuth credentials are excluded. See
[voice configuration and limits](../../docs/companion/voice.md).

Native speech output is independently disabled by default. Enable
`ANIMAOS_TTS_ENABLED=true` with the same server OpenAI Platform API-key
configuration to generate one selected saved own Web Chat assistant reply.
Generation and playback require separate explicit actions; chat never invokes
speech automatically. The fixed gpt-4o-mini-tts/alloy adapter requests bounded
24kHz PCM and returns a canonical WAV, with retained output-body ownership and
local cancellation. No generated audio is saved by the daemon. See
[speech output and limits](../../docs/companion/speech-output.md).

The web console uploads files with `POST /api/agents/{agent_id}/sessions/{session_id}/attachments`: multipart form data containing exactly one `file` part with a filename. The response is `201 { attachment }`, with the saved ID, name, MIME, size, kind, relative path and creation time. Both upload and `GET /api/agents/{agent_id}/attachments/{attachment_id}` require local-owner authorization; downloads return `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`.

Configure the agent's workspace before uploading. Files are saved there through exclusive creation, with safe collision suffixes. Each ID belongs to its original agent and session, canonical workspace root and content digest. Moving the workspace or changing/removing the file makes that ID unavailable. Session/agent deletion removes attachment metadata; it does not delete workspace files. Failed persistence may leave an unreferenced file but never reports a successful upload.

Limits are 10 MiB per PNG/JPEG/GIF/WebP image, 1 MiB per supported UTF-8 text/source file, 25 MiB per PDF/Office document, 4 MiB/120 seconds per supported PCM16 WAV, and ten unique ordered attachment IDs per input. Client MIME declarations do not establish file type. Office ZIP envelopes are checked without extraction; legacy compound Office files receive envelope-only validation and extension-derived MIME, without Office-family parsing. Text attachments tell the companion to use read_file for the saved relative path; selected own Windows Web Chat DOCX/PDF/XLSX can supply bounded quoted text, while other document formats remain saved/download-only with an honest unread-file note. Known vision models receive at most four selected images, hydrated only for the model request. Unknown or non-vision models receive text references. Transcripts, snapshots and public metadata retain references rather than encoded image bytes.

Selected XLSX extraction reads stored cells and explicit hidden-sheet metadata under the existing 2 MiB document input, two usable documents and shared 16 KiB context bounds. It never calculates formulas: literal formulas are labeled not evaluated and cached values may be stale. Styles, formatted dates, charts, merged layout and other workbook features are outside this text view. Oversized, unsupported or malformed packages can be refused; no external link, macro or connection runs. The existing Windows-owned document worker keeps the same process/time/memory limits; other hosts remain unavailable for physical extraction. Public chat badges expose bounded sheet/cell counts and limitations, without sheet names, raw text, formulas, source hashes or private paths. Originals stay unchanged and downloadable. See [document limits](../../docs/companion/documents.md); fixture parsing does not establish answer quality.

Runs and steers accept `attachmentIds` alongside text and skill; attachment-only input is valid. Exact retries preserve raw text, ordered IDs and skill. New consumed steer keys retain that complete input even after transcript pruning. Older consumed keys can replay only while a matching scoped transcript proves equality; otherwise the daemon returns 409. Snapshot version 11 retains attachment-aware pending and consumed steers; its first upgrade writes `<control-plane-file>.pre-attachments.bak` in JSON mode or `control_plane.backup.10` in Postgres mode. For rollback to M8, stop services and restore the complete matching pre-upgrade backup generation with recorded binaries. The snapshot backup is only a component; mixing older JSON with newer history can resurrect rows. Restore history, memory, workspace/uploads and keyring consistently, retaining the original root and vault password. Changes since that backup are lost.

Browser dictation and read-aloud use browser speech APIs when available, and cancel when their chat becomes inactive. Browser support and microphone permission vary; browsers may use an external speech service. These controls remain separate from the opt-in daemon transcription and speech actions described above. Local mocked tests do not establish live provider image acceptance, speech quality or microphone behavior.

An explicitly configured `read_document` tool can read an existing workspace DOCX, PDF or XLSX without an attachment upload. The owner enables it separately in Settings; defaults and unrelated grants stay unchanged. Existing filesystem grant checks also apply to helpers and connector-triggered runs. A held-root bounded read captures at most 2 MiB; the returned untrusted tool envelope includes the relative path, captured-byte SHA-256, typed locators and coverage within 16 KiB total serialized output. It is a byte snapshot, with no current-path freshness promise. Reads share the retained four-slot source pool and the existing single owned Windows document worker under one ten-second deadline. A run cancellation signal is required; there is no retry, formula calculation, OCR, edit or automatic grant. Source files stay unchanged. See [document use and limits](../../docs/companion/documents.md).

### Live runs

Every live-run route requires local-owner authorization and answers `Cache-Control: no-store`, errors included.

| Method | Path                                                | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------ | --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`  | `/api/agents/{agent_id}/events`                     | Server-Sent Events for the agent, its helpers, and sessions delegated from it (`X-Accel-Buffering: no`). The first event is `stream.snapshot`: the active runs with their current step, its text so far (up to 64 KiB), and their tool cards. Then `session.*`, `run.*`, `step.delta`, `message.created`, `tool.started`, and `tool.finished` follow. Every event carries `type`, `agentId`, optional `sessionId` and `runId`, a per-stream `seq` (also the SSE `id`), and `at`. A stream that falls behind receives `stream.resync` with `missed` and keeps going; there is no replay, so a reconnect starts from a new snapshot (`Last-Event-ID` is ignored). Keep-alive comments every 15 seconds. `404` for an unknown agent; `429` (`Too many event streams are open for this agent`) beyond 16 open streams per agent.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `POST` | `/api/agents/{agent_id}/sessions/{session_id}/runs` | Send a message to a session: `{ "text", "attachmentIds"?, "skill"?, "mode"?: "queue" \| "steer" }` with an `Idempotency-Key` header (1–128 visible ASCII characters). Answers `202` with `{ run }`, a `queued` run that starts once the session's earlier messages have finished, waiting for an agent slot and a global permit rather than failing fast. With `"mode": "steer"` while the session has a running run, the message joins that run instead and answers `202` with `{ run, steer: { status: "pending" } }`, `run` being the run it joined (see below); with no run to join it is queued like any message. The same key within 24 hours answers `200` with the original run and creates nothing, or `409` (`Idempotency-Key was already used for a different message`) for another message; a key deduplicates only while its run is still in the ledger, so at most 24 hours and, once the history store holds it, among the agent's newest 50 finished runs. A new chat still titled "New chat" takes its first message's title at once. A Telegram session's message runs the connector's owner turn (`source: telegram`, `sourceRef` = connector id). `skill` names an enabled, active skill whose instructions join that run (spec §8.3). `400` for a missing or invalid key, empty text with no attachments, text over 32 KiB, attachments (over 10, duplicated IDs, or unavailable scoped records), an unknown skill, a skill that is off or waiting for review, a skill with `"mode": "steer"` or in a Telegram session, `steer` on a kind that cannot steer, or a body over 256 KiB; `404` for an unknown agent or session; `409` for a kind that cannot receive messages (`This session cannot receive messages`) or a helper; `429` (`This companion already has 8 queued messages; wait for one to start`); `503` when the control plane cannot be saved or the daemon is shutting down. |
| `GET`  | `/api/agents/{agent_id}/sessions/{session_id}/runs` | The session's runs the ledger holds, newest first: `?limit=` (1–50, default 20). Returns `{ runs }`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `GET`  | `/api/agents/{agent_id}/runs/{run_id}`              | One run as `{ run }`, from the ledger or, once pruned from it, the history store; `404` for another agent's run.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `POST` | `/api/agents/{agent_id}/runs/{run_id}/stop`         | Stop a run; answers `202` with `{ run }`. A queued run is `cancelled` at once (`stopped`, `Stopped by owner`) and never starts. A running run records `stop.requestedAtMs`, which is saved before the run is signalled, and ends `cancelled` at its next checkpoint (see below). Saved with that stop, a running Telegram turn becomes `stopped`: it finishes without a reply and is never run again. A running job attempt saves its stop marker the same way; the attempt is recorded as `stopped`, and the job moves to `needs_review` with the error `Stopped by owner; inspect effects before retrying`, so a retry must set `acknowledgeUncertain`. Stopping again changes nothing, and a finished run is answered as it is, from the history store once the ledger pruned it; stopping a finished Telegram run whose reply is not delivered yet suppresses that reply, so it is never sent. `404` for an unknown agent or run, or another agent's run (a helper's run is stopped through the helper); `503` when the stop cannot be saved, which leaves the run as it was.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

An accepted message is announced as `run.queued` once it is saved, and a new chat it retitles as `session.updated`. A queued run counts in its session's `activeRuns`; a restart ends it as `interrupted` (`restart_before_start`); one that cannot start ends as `failed`, or `cancelled` (`stopped`) when it was stopped while it waited. On a graceful shutdown the daemon first stops starting accepted runs, before it ends the event streams: a new message is refused with `503` (`The daemon is shutting down; send the message again once it restarts`), a queued message whose turn comes, or one still waiting for an agent slot or a global permit, stays queued for the restart to interrupt as never started, and the runs already going get up to 30 seconds to commit. One still going after that is logged and left to the restart, which ends it as `interrupted` (`restart_during_run`). Every run publishes its events on its agent's stream, and a helper's, delegated, or peer run also on the stream of the agent it came from: `session.created` (for a new session) and `run.started` once its start is saved; while it runs, its streamed text as `step.delta` (sent at least every 50 ms or 512 bytes), `tool.started`, `tool.finished`, and `run.steered`; then, after its result is saved, `message.created` for each saved message in order, `session.updated`, and `run.completed`, `run.failed`, or `run.cancelled` for a stopped run. Creating, renaming, archiving, marking read, and deleting a session publish `session.created`, `session.updated`, and `session.deleted`. Nothing streamed is retracted, with one exception: when a run's result is refused by its source or cannot be saved, or its agent is deleted while it runs, the run's messages are discarded and `run.failed` (`commit_rejected`, `commit_failed`, or `agent_deleted`) tells clients to drop the text they were streamed, which never became a `message.created`. A message whose acceptance could not be saved, or a run whose start could not be saved, ends with `run.failed` (`commit_failed`) for any stream that already listed it in its snapshot; a new chat's title that acceptance set goes back, announced as `session.updated`.

A stopped run (spec §4.6) is saved with its stop first, then signalled. The runtime stops before its next model call, while streaming (dropping the request), or before its next tool batch; tools already running finish, except that a `bash` command is killed (its direct child only: a process the command started itself keeps the output open until it exits, and background processes keep running). The streamed text so far stays as an assistant message marked `stopped`, every tool call the model asked for but that never ran gets the result `Cancelled before running (stopped by owner)`, and the run ends `cancelled` with error `stopped` (`Stopped by owner`) and `run.cancelled`; the agent goes back to idle, not failed. The runs it started (helpers, delegations, and peer requests), queued or running, are stopped with it, and so is one its tool starts after the stop. Steers the run had not read yet are not queued as new messages: each becomes an `interrupted` run (`stopped_before_start`, `The run was stopped before this message reached it; it is safe to send it again.`), announced as `run.interrupted` once saved, so a stop stops everything and nothing is lost. This is a deliberate deviation from spec §4.7, which requeues the steers a finished run left. A stopped check-in records the outcome `stopped` (`schedule_run_stopped`) and its schedule stays enabled.

A steer (spec §4.7) is saved with the run it joined before that run can read it. The run reads it before its next model call, as an owner message marked `steer` with its `clientRequestId` and the `runId`, announced as `run.steered`; a retried key answers `200` with the run it joined and, while the steer still waits there, `steer: { status: "pending" }`. A steer the run never read, because it made no further model call, becomes a queued message once the run ends, placed in the session's queue by the time it was accepted; past the 8 waiting messages it becomes an `interrupted` run instead (`queue_full_before_start`, `Eight messages were already waiting when the run this message joined ended; it is safe to send it again.`). A restart before the run's result is saved offers each steer it held again as an `interrupted` run (`restart_before_start`). When the run fails instead (its result refused or not saved, or its task crashed), each steer it held becomes an `interrupted` run at once (`failed_before_start`, `The run this message joined failed before reading it; send it again.`), announced as `run.interrupted` once saved. A retried key of such a steer answers with that run. A steer sent once the run's stop is saved, or once the run is finishing, waits as a message. Acceptance times never go backwards, even when the system clock does, so a session's messages keep their order.

Each open console tab holds one event-stream connection for as long as it is open. Through the dev proxy (HTTP/1.1), browsers allow about 6 connections per origin, so with many tabs open a tab's other requests can wait behind the streams; close the tabs you are not using. On shutdown the daemon ends every open stream first, so an open tab never blocks a graceful exit.

### Action receipts and interrupted work

Run reads include an optional `actionReceipts` array with each native tool call's
`toolCallId`, `name`, `status` (`started`, `succeeded`, `failed`), timestamps and
duration, bounded argument/result previews and their truncation flags, and
`durability` (`persistent` or `memory`). Existing snapshots without the array
remain valid. Owner authorization and run/history retention stay the same.

With a configured control-plane store, intent is saved before handler dispatch
and outcome before returning the tool result or streaming success. A receipt save
failure aborts continuation. Embedded runtimes without a store are supported, but their
receipts do not survive restart and are explicitly labelled temporary. This is
determined by the configured store, not by the memory persistence mode.

Receipts are separate from the final conversation commit. An interrupted run can
retain a reported tool outcome without committed messages. Started means outcome
unknown; the handler may never have run, or its effect may precede a crash that
prevented result storage. Failed handlers can leave partial effects. Succeeded
means handler-reported success, not whole-run completion or an exactly-once
guarantee. Previews are UTF-8-safe, capped at 2048 bytes each; runs admit at most
50 unique recorded calls. Inspect effects before explicitly retrying. No automatic
replay or historical backfill is added. See [daily use](../../docs/companion/daily-use.md).

### Approvals

Every approval route requires local-owner authorization and answers `Cache-Control: no-store`, errors included. A coordinator run's tool call is judged by its agent's policy (`write`, `exec`, `network`, and `delegate` each `allow`, `ask`, or `deny`; by default only `exec` asks), then its rules and the session's allowances; read-class tools never ask, and a helper answers to its companion's policy and rules. A call that needs the owner waits up to 30 minutes (15 for a Telegram-started run) with its run `awaiting_approval`, announced as `run.awaiting_approval` and `approval.requested`. A decision or the timeout (a denial noted `Approval timed out`) settles it once, announced as `approval.resolved` and, when the run waits on nothing else, `run.started`. A Stop (and any other `stopped` settlement) is announced as `approval.resolved` and followed by the run's end instead, not `run.started`. Helpers never wait: a call that would ask is denied (`Needs owner approval; not available to helpers`). A restart expires every pending approval. Decided approvals move to the history store.

**Limits.** A rule for an HTTP client, interpreter, shell, package runner, or git (or an `any` exec rule) lets the companion act as the owner on this machine, including changing its own approval policy and rules through the loopback API. Workspace writes are allowed by default, so `.git/config` or `package.json` can turn "Always allow `git status`" or "`npm test`" into arbitrary code. Keep exec rules narrow. A command-prefix rule ignores the call's `cwd` argument. `web_fetch` follows redirects, so a domain rule covers only the first host. On Windows the shell may be cmd or PowerShell, where `git` can resolve a `git.bat` in the working folder.

**Runs started elsewhere wait too.** With the default `exec: ask`, check-ins, jobs, Telegram, and CLI/API runs that call `bash`, `bg_start`, or `bg_stop` wait for a decision in the web console: 30 minutes (15 for Telegram-started runs), holding that automation, job lane, or chat meanwhile. The legacy `POST /api/agents/{id}/run` answers 408 after `ANIMAOS_RS_RUN_REQUEST_TIMEOUT_SECS` (600 s by default) while the approval keeps waiting, so a later Allow runs the command with nobody watching. For unattended shell work, set that agent's `exec` to `allow` or add a narrow rule.

A decided approval can read "Allowed" for a tool that never ran (allowed, then the run was stopped before the call ran). A delegated specialist or peer is judged only by its own policy and rules: a companion with `exec: deny` can still have `bash` run by a specialist whose policy allows it, so set the specialist's policy too. After a restart during a run, a call that was waiting for approval is listed among the tools that may have started, although it did not run.

| Method   | Path                                              | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| -------- | ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`    | `/api/approvals`                                  | `?status=pending` (the default): approvals waiting for the owner, oldest first. `?status=decided`: approvals created in the last 30 days and since decided, newest first, with `?cursor=` and `?limit=` (1–100, default 50). Optional `?agentId=`. Returns `{ approvals, nextCursor }`; each approval carries its `tool`, `class`, `arguments` (JSON text, at most 16 KiB, with `argumentsTruncated`), `suggestedMatcher`, `matcherKinds`, `createdAtMs`, `expiresAtMs`, `status` (`pending`, `allowed`, `denied`, `stopped`, or `expired`), `revision`, and `resolution`. `400` for an invalid status, cursor, or limit; `503` when the history store cannot be read.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `POST`   | `/api/approvals/{approval_id}/decision`           | Decide a pending approval: `{ "decision": "allow_once" \| "allow_session" \| "allow_always" \| "deny", "note"?, "matcher"?, "revision" }` (`note` at most 1,000 characters; `matcher` `{ "kind": "command_prefix" \| "path_glob" \| "domain" \| "any", "value" }` for `allow_session` and `allow_always`, the suggestion when absent). Saved before the waiting call goes on; returns `{ approval }`. `allow_session` adds a session allowance (at most 50); `allow_always` creates a rule. A denial answers the model `Denied by owner: <note>`. The same decision again returns the approval unchanged, from the history store once it moved there. `400` for an invalid body, note, or matcher; `404` for an unknown approval; `409` (`This approval was already resolved`) once it was resolved another way, including by the timeout, a Stop, or a restart, and (`This approval changed; reload it and decide again`) for a stale revision, (`This companion already has 100 approval rules; remove one first`) when `allow_always` would pass 100 rules, (`This session already has 50 allowances`) when `allow_session` would pass 50, and (`This approval's session no longer exists`) when its session was deleted; `503` when the decision cannot be saved. |
| `GET`    | `/api/agents/{agent_id}/approval-policy`          | `{ policy: { write, exec, network, delegate } }`, each `allow`, `ask`, or `deny`; a helper's is its companion's.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `PUT`    | `/api/agents/{agent_id}/approval-policy`          | Replace the policy (all four classes required); returns `{ policy }`. `409` for a helper; `503` when it cannot be saved (the old policy stays).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `GET`    | `/api/agents/{agent_id}/approval-rules`           | `{ rules, tools }`: the agent's rules oldest first (a helper's are its companion's), and every tool a rule can cover with its `class` and `matcherKinds`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `POST`   | `/api/agents/{agent_id}/approval-rules`           | Add a rule `{ "tool", "matcher" }`; returns `201` with `{ rule }`, or `200` with the agent's identical rule. A command-prefix rule never covers a command holding a shell operator, and a path rule never covers an absolute path or one with `..`. `400` for an unknown or read-class tool or an invalid matcher; `409` for a helper or past 100 rules; `503` when it cannot be saved.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `DELETE` | `/api/agents/{agent_id}/approval-rules/{rule_id}` | Remove a rule; returns `{ deleted: true }`. `404` for an unknown rule; `503` when it cannot be saved (the rule stays).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

### Skills

Every skill route requires local-owner authorization, answers `Cache-Control: no-store` (errors included), and answers `409` (`Skills need a configured workspace`) without a configured workspace. A skill is `<workspace>/skills/<slug>/SKILL.md` (slug `^[a-z0-9][a-z0-9-]{0,63}$`, not `import` and not a Windows device name (`con`, `prn`, `aux`, `nul`, `com0`–`com9`, `lpt0`–`lpt9`)): YAML front matter with `name` (1–64 characters) and `description` (1–300 characters), each on one line, then a Markdown body of 1 byte to 32 KiB. A name, description, or body may not contain invisible Unicode tag or direction-override characters, and a name or description may not contain any other invisible format character or line separator. The registry pins each skill to the SHA-256 of the `SKILL.md` the owner approved; a file edited afterwards reads `changed` and is never loaded until approved again, because every load rereads and rehashes the file. A skill's `status` is `active`, `changed`, `missing`, or `invalid`. The daemon rescans the folder at startup, every 60 seconds, and on every list request; a saved change, or a scan that found something new, is announced as `skill.updated` (with `slug` and `draftId`, either may be `null`) on every companion's event stream. Runs of agents with `load_skill` list up to 50 enabled, active skills in their system prompt as data. Only agents whose tools include `load_skill` get the index (the design says every workspace agent; an agent without the tool could not use the list). A session message sent with `skill` carries that skill's instructions for its one run.

**Limits.** Approved instructions are treated as instructions, so review a draft before approving it. A companion with `write_file` can create `skills/<slug>/SKILL.md` directly; that file is only a draft until the owner approves its exact hash. A `SKILL.md` whose modification time and size did not change is not rehashed by a scan, so its `status` can lag; loading always checks the hash. An exec rule that lets the companion act as the owner (see the Approvals limits above) also lets it approve its own skill drafts and changed skills through the owner routes. A draft whose source is `file` (the Skills page says “Found in the skills folder”) is a `SKILL.md` someone wrote into the workspace, which includes anything the companion wrote with `write_file`; the daemon cannot tell who wrote it.

Retrying a create whose save failed, or approving a stored draft for a new slug whose save failed, answers `409` on the retry, because the written file stays and now awaits review as a file draft: approve or reject `file:<slug>` first.

| Method   | Path                                   | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| -------- | -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`    | `/api/skills`                          | `{ skills }` by slug, after a rescan; each `{ slug, name, description, enabled, status, approvedHash, approvedAtMs, updatedAtMs }`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `GET`    | `/api/skills/{slug}`                   | `{ skill, file }`: the record (or `null`) and what `SKILL.md` holds now, `{ hash, name, description, body, problem }` (or `null`). `404` when neither exists; `503` when the file cannot be read in time.                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `PUT`    | `/api/skills/{slug}`                   | Create or replace the content `{ "name", "description", "body", "enabled"? }`, which approves it: written through the hardened workspace writer with its hash pinned. `201` when new, `200` when replaced, with `{ skill }`. `400` for an invalid slug, name, description, or body; `409` past 200 skills, or when the folder already holds a `SKILL.md` the owner has not reviewed (`A SKILL.md the owner hasn't reviewed is in this folder; review it first`; reject its file draft on the Skills page, or approve it, first); `503` when the file or the registry cannot be saved (a file already written then reads `changed`).                                                 |
| `PATCH`  | `/api/skills/{slug}`                   | `{ "enabled" }`; returns `{ skill }`. `404` for an unknown skill; `503` when it cannot be saved.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `DELETE` | `/api/skills/{slug}`                   | Moves the folder to `.anima-trash/skills/<slug>-<ms>` and drops the record; returns `{ deleted: true, trashPath }`. `404` when there is neither a record nor a folder; `503` when it cannot be saved (the record and, when possible, the folder come back).                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `POST`   | `/api/skills/{slug}/approve`           | Approve a changed skill's current file: `{ "hash" }` must be the `file.hash` the owner reviewed. Returns `{ skill }`. `400` (`hash is required: the SKILL.md you reviewed`) when `hash` is not 64 hexadecimal characters, or when the file is not a valid `SKILL.md`. `409` (`SKILL.md changed since you reviewed it; reload and review it again`) when the file moved on, and (`This skill has no changes waiting for approval`) when it is already approved; `503` when the file cannot be read or the approval saved.                                                                                                                                                            |
| `GET`    | `/api/skill-drafts`                    | `?status=pending` (the default): drafts waiting for the owner, oldest first, after a rescan: proposals (`source: agent`, with `proposedBy { agentId, sessionId, runId }`), imports (`import`), and `SKILL.md` files without a record (`file`, id `file:<slug>`, with `fileHash` and any `problem`). `?status=decided`: approved and rejected drafts of the last 30 days (at most 50), newest first. Each has `baseHash`, `currentHash`, and `stale` (the skill was approved again since). `400` (`status must be pending or decided`) for any other status.                                                                                                                         |
| `POST`   | `/api/skill-drafts/{draft_id}/approve` | Approve a draft, with an optional edited `{ "body" }`; a file draft needs `{ "hash" }`, the `fileHash` the owner reviewed. Writes `SKILL.md` through the hardened writer (an unedited file draft keeps its file) and pins its hash; returns `{ skill, draft }`. `400` for a missing hash or an invalid file; `404` for an unknown draft; `409` once decided, (`SKILL.md changed since you reviewed it; reload and review it again`) when the file moved on, or past 200 skills; `503` when it cannot be saved (the draft stays pending).                                                                                                                                            |
| `POST`   | `/api/skill-drafts/{draft_id}/reject`  | Reject a draft; returns `{ draft }`. A rejected file draft stays hidden until its file changes; the file stays in the workspace. `404` for an unknown draft; `409` once decided.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `POST`   | `/api/skills/import`                   | Multipart `file` (a `SKILL.md`, at most 64 KiB) and optional `slug` (otherwise derived from the name); returns `201` with a pending `{ draft }`. The form is read by a strict bounded parser: at most 80 KiB in all, 8 parts, 2 KiB of headers a part, 64 KiB a part, CRLF line ends, and no text before the first boundary. `400` (`Send the SKILL.md file as multipart/form-data in a field named file`) for anything that is not such a form, (`The imported file must be at most 64 KiB`) for an oversized request or file, or the file's problem or an invalid slug; `409` when 10 imports already wait. `GET /api/skills/import` answers `405` (`import` is a reserved slug). |

### Usage and pricing

Every usage route requires local-owner authorization and answers `Cache-Control: no-store` (errors included). Each model call is one usage record: run steps, plus titles, compaction, and profile and agency generation. A call is priced when it is recorded, from the built-in price table (`tableDate`) or an owner override; `chatgpt` calls are `subscription` (no cost), local providers are `free`, and anything the table does not know is `unknown` (no cost, counted as `unpricedCalls`). Only providers that report usage before a call's last chunk keep it when the call is stopped or fails; there is no token estimation for the others. Money is micro-USD integers.

| Method | Path                                                                         | Description                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------ | ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`  | `/api/usage/summary?from=&to=&agentId=&sessionId=&groupBy=&tzOffsetMinutes=` | `{ from, to, groupBy, tzOffsetMinutes, totals, groups, truncated }`. `from`/`to` are epoch ms (default: the last 30 days; at most 366 days apart); `groupBy` is `day` (in the caller's offset, ascending), `model`, `source`, or `session` (top 20), sorted by tokens; `truncated` when the scan stopped at 200,000 rows. `400` for an invalid range, group, or offset; `503` when the store cannot be read.                            |
| `GET`  | `/api/usage/records?from=&to=&agentId=&cursor=&limit=`                       | `{ records, nextCursor }`, newest first; `limit` 1–200 (default 50); `cursor` is the previous `nextCursor`. `400` for an invalid range, limit, or cursor.                                                                                                                                                                                                                                                                               |
| `GET`  | `/api/usage/export.csv?from=&to=&agentId=`                                   | The range as `text/csv` attachment (`anima-usage-<from>-<to>.csv`), oldest first; cells that start with `=`, `+`, `-`, `@`, tab, or CR get a leading `'` so spreadsheets read them as text. `400` past 100,000 calls (choose a shorter range).                                                                                                                                                                                          |
| `GET`  | `/api/usage/pricing`                                                         | `{ overrides, tableDate }`.                                                                                                                                                                                                                                                                                                                                                                                                             |
| `PUT`  | `/api/usage/pricing`                                                         | Replace all overrides with `{ overrides: [{ provider, model, inputMicrosPerMtok, outputMicrosPerMtok, cachedInputMicrosPerMtok? }] }` (at most 100; `model` is a lowercase prefix and the longest match wins; each provider, by canonical name, and model may appear once). New prices apply to calls recorded afterwards; stored rows keep their price. `400` for an invalid list; `503` when it cannot be saved (the old list stays). |

### Logs

Both log routes require local-owner authorization and answer `Cache-Control: no-store` (errors included). They serve the daemon's in-memory log tail: at most 2,000 lines captured at the subscriber's level (default `anima_daemon=info,tower_http=info`), redacted and cut to 4,096 bytes at capture. A line is `{ seq, at, level, target, message }`; `seq` rises by one per line.

| Method | Path                                | Description                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------ | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GET`  | `/api/logs?level=&q=&after=&limit=` | `{ lines, newestSeq }`, oldest first. `level` is the lowest level to show (`error`, `warn`, `info`, `debug`, `trace`); `q` is case-insensitive text in the message or target (at most 200 characters); without `after`, the newest `limit` matches; with it, the first `limit` matches after that seq; `limit` 1–1,000 (default 200). `400` for an invalid `level`, `q`, `after`, or `limit`.                                                    |
| `GET`  | `/api/logs/stream?level=&q=&after=` | Server-Sent Events. `event: log` carries one line; `event: resync` carries `{ "newestSeq": n }` when the stream fell behind (refetch the list after your newest seq; the stream continues after `n`). Without `after`, only new lines are sent; with it, the buffered lines after it first, then live lines, with no gap or repeat. At most 8 streams are open at once (`429`, `Too many log streams are open`); a closed stream frees its slot. |

### Usage, logs, and health (owner)

The Usage and Logs routes above and `GET /api/status` below are the owner's view of what the companion costs, what the daemon is doing, and whether it is healthy. Every one requires local-owner authorization and answers `Cache-Control: no-store` (errors included); `/metrics` is the one exception.

| Method | Path          | Auth  | Query or body | Answers                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Errors                                                |
| ------ | ------------- | ----- | ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------- |
| `GET`  | `/api/status` | owner | none          | One snapshot: `version`, `buildRevision` (null unless the build set `ANIMAOS_BUILD_REVISION`), `startedAtMs`, `nowMs`, `uptimeSeconds`, `readiness`, `storage` (persistence mode, control plane, and the history store's health, queue sizes, redacted last error, and flush-error count), `providers` (whether each is configured, never a key), `connectors` (at most 50, never a credential), `automations`, `approvals` (pending), `runs`, `events`, `logs`, and `limits`. | `403` `local owner authorization required` for others |

- Pricing overrides are control-plane state (snapshot version 10), and a run's usage rows are priced when the run is first mirrored; a later override changes only calls recorded afterwards. Providers that report usage before the last chunk keep it for stopped or failed calls; nothing is estimated for the others.
- Logs are redacted at capture (bearer tokens, `sk-`, `xox`, and `ghp_` keys, `Authorization` headers, API-key query parameters, and JSON `apiKey`, `token`, and `secret` fields), then bounded: 2,000 lines of at most 4 KiB. `RUST_LOG=debug` can still surface more detail in the log tail.
- `/metrics` stays unauthenticated and carries counts only: no agent names, ids, prompts, or costs. It adds uptime, runs by status (the run ledger prunes old finished runs, so these are not lifetime totals), pending approvals, open event streams, history outbox backlog, usage rows queued, whether the history store is failing, failing automations, buffered log lines, and the counters `anima_daemon_event_lagged_total`, `anima_daemon_history_flush_errors_total`, and `anima_daemon_messages_pruned_total`.
- A steer sent while eight messages already wait for the companion (queued messages plus steers held by running runs) is refused with `429` and the daemon's message, like a ninth queued message.
- Rolling back to a daemon older than M8 needs the `.pre-usage.bak` backup; see the rolling-back notes in Operational notes.

### Automations

Every automation route requires local-owner authorization (the list too, since M6), and answers `Cache-Control: no-store` with errors as `{ code, error }`. An automation is a prompt the daemon sends its agent on a trigger: `interval { intervalMs }` (whole seconds), `daily { hour, minute, timeZone }`, `cron { expression, timeZone }` (five fields — minute, hour, day of month, month, day of week — with `*`, numbers, `JAN`–`DEC`, `SUN`–`SAT`, ranges, steps, and lists, or `@hourly`, `@daily`, `@midnight`, `@weekly`, `@monthly`, `@yearly`, `@annually`; `L`, `W`, `#`, and `?` are refused; when both day fields are restricted either may match), or `once { atMs }`, which turns itself off once it fires. Cron and daily times are wall-clock times in their IANA `timeZone`: a time a daylight-saving jump skips does not fire that day, and a repeated time fires once. `activeHours { start, end, days, timeZone }` (`HH:MM`, days 0 for Sunday to 6) keeps fires inside a window; a window whose `end` is before its `start` runs overnight from the day it starts; an interval waits for the window to open; a one-time automation takes no active hours. Each automation has a `name` (1–80 characters on one line, from the prompt when omitted), `createdBy` (`owner`, or `agent` with the session, run, and tool call that made it), `preset` (`heartbeat` or null), `counters { runs, failures, consecutiveFailures }`, and `running`. Every saved change, claim, and outcome is announced as `automation.updated` (`scheduleId`, `deleted`) on the agent's event stream. Each occurrence's outcome (`silent`, `spoke`, `failed`, or `stopped`) is kept as a fire record, saved with the outcome and mirrored to the history store's `schedule_runs` table; fire records go with their agent, not with their automation.

**Limits.** An agent has at most 20 automations (a legacy browser import is exempt). Prompts may not contain invisible Unicode tag or direction-override characters, and names may not contain any invisible format character or line break. A companion with `create_automation` (a write-class tool, allowed by default) can schedule its own future runs; each one shows in its chat with Undo and on the Automations page, must be at least 5 minutes apart over its next 10 fires, and counts toward the 20. Set the companion's write policy to `ask` to approve each one. Silent check-in pairs (`CHECKIN_OK` replies) leave the control plane's hot tail once they are mirrored and older than 24 hours, wherever they sit in their session; the history store keeps them.

| Method   | Path                                                     | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| -------- | -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GET`    | `/api/agents/{agent_id}/schedules`                       | `{ schedules }`, oldest first.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `POST`   | `/api/agents/{agent_id}/schedules`                       | Create `{ prompt, trigger, target?, name?, activeHours?, enabled? }` (target defaults to `workspace`, the automation's own thread), or `{ preset: "heartbeat", timeZone, … }` (every 30 minutes from 08:00 to 22:00 in `timeZone`; any other field named replaces the preset's). `201` with `{ schedule }` (`200` when a legacy import key already made it). `400` for an invalid field (`prompt and trigger are required unless preset is heartbeat`, `timeZone is required for the heartbeat preset`, or the trigger's problem); `409` (`This companion already has 20 automations; delete one first`) or for an unavailable Telegram target; `503` when it cannot be saved. |
| `PATCH`  | `/api/agents/{agent_id}/schedules/{schedule_id}`         | Change `{ prompt?, trigger?, target?, enabled?, name?, activeHours? }` (`activeHours: null` clears them); the next fire is computed again when the trigger or the active hours change, or when it is turned back on (a one-time automation whose time passed needs a new `atMs`).                                                                                                                                                                                                                                                                                                                                                                                              |
| `DELETE` | `/api/agents/{agent_id}/schedules/{schedule_id}`         | Delete; returns `{ deleted: true }`. Its check-in session can then be deleted; its fire records stay until its agent is deleted.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `POST`   | `/api/agents/{agent_id}/schedules/import`                | Import legacy browser check-ins (unchanged).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `POST`   | `/api/agents/{agent_id}/schedules/{schedule_id}/run`     | Run now: `202` with `{ schedule }`. The due time and the switch stay, and the fire is recorded as `manual`. `404`; `409` (`This automation is already running`); `429` (`Too many automations are running; try again shortly`, at eight running automations); `503`.                                                                                                                                                                                                                                                                                                                                                                                                           |
| `GET`    | `/api/agents/{agent_id}/schedules/{schedule_id}/history` | `{ runs }`, newest first, `?limit=` 1–50 (default 50): `{ id, scheduleId, agentId, firedAtMs, finishedAtMs, outcome, runId, sessionId, errorCode, manual }`. `400` (`limit must be from 1 to 50`); `404`; `503` (`automation history is unavailable`) when the history store cannot be read.                                                                                                                                                                                                                                                                                                                                                                                   |
| `POST`   | `/api/schedules/preview`                                 | `{ trigger, activeHours? }` → `{ nextRuns }`, the next three fire times (one for `once`), so the browser never computes schedules. `400` with the trigger's problem.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |

### Agencies

| Method | Path                     | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ------ | ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `POST` | `/api/agencies/create`   | Generate an agency and materialize a CLI-style workspace under the daemon workspace root. Body includes `name`, `description`, `teamSize`, optional `provider`, optional `model`, optional `modelPool`, optional `outputDir`, optional `seedMemories`, and optional `overwrite`. Writes `anima.yaml`, `org-chart.mmd`, `README.md`, and `agents/*/profile.md` plus workspace placeholders, and can also write `agents/*/memory/seed.json` files with LLM-generated starter memories. The response includes the created file list plus seed-memory counts. |
| `POST` | `/api/agencies/generate` | Generate an agency draft from a plain-language description. Body includes `name`, `description`, `teamSize`, optional `provider`, optional `model`, and optional `modelPool`. Returns a mission, values, and normalized agent definitions that can be spawned into a swarm without unsupported tool slugs.                                                                                                                                                                                                                                                |

### Memories

Native hybrid vector recall checks current canonical records and the supplied
search filters before choosing its top vector matches. The model's
`memory_search` also applies its existing own-agent and hidden-text checks there.
Foreign, hidden or canonical-missing indexed IDs therefore cannot consume that tool's
vector shortlist. Only eligible finite positive hits normalize vector scores;
the fusion weights and exact-lexical vector suppression remain unchanged.

This uses the existing embedding provider and one query embedding per nonzero
search. The local provider has fixed synonym groups and hashed terms, so local
fixtures do not establish general semantic quality. Automatic context remains
lexical. External `MemoryVectorIndex` implementations that use the compatibility
default can filter only their old bounded shortlist; native backfill is not a
promise for those implementations. No new grants, provider configuration,
storage format, automatic consolidation or public response shape is added.

| Method | Path                              | Description                                                                                                                                                                                                                                                                                                                                                                  |
| ------ | --------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `POST` | `/api/memories`                   | Store a memory. Required fields: `agentId`, `agentName`, `type`, `content`, `importance` (0-1). Optional fields: `scope` (`shared`, `private`, `room`), `roomId`, `worldId`, `sessionId`, and `tags`.                                                                                                                                                                        |
| `GET`  | `/api/memories/search`            | Keyword search. Required `?q=`. Optional `?type=`, `?agentId=`, `?agentName=`, `?scope=`, `?roomId=`, `?worldId=`, `?sessionId=`, `?limit=`, `?minImportance=`.                                                                                                                                                                                                              |
| `GET`  | `/api/search`                     | Alias for `/api/memories/search` with the same query parameters and response shape.                                                                                                                                                                                                                                                                                          |
| `GET`  | `/api/memories/recent`            | Recent memories. Optional `?agentId=`, `?agentName=`, `?scope=`, `?roomId=`, `?worldId=`, `?sessionId=`, `?limit=`.                                                                                                                                                                                                                                                          |
| `POST` | `/api/memories/entities`          | Create or update a memory entity. Required fields: `kind` (`agent`, `user`, `system`, `external`), `id`, `name`. Optional fields: `aliases`, `summary`.                                                                                                                                                                                                                      |
| `GET`  | `/api/memories/entities`          | List memory entities. Optional `?entityId=`, `?kind=`, `?name=`, `?alias=`, `?limit=`.                                                                                                                                                                                                                                                                                       |
| `POST` | `/api/memories/evaluations`       | Evaluate a candidate memory without storing it. Accepts the memory create fields plus optional `minContentChars` and `minImportance`; returns `store`, `merge`, or `ignore`.                                                                                                                                                                                                 |
| `POST` | `/api/memories/evaluated`         | Evaluate and conditionally store a memory. Returns the evaluation plus the stored memory when the decision is `store`; duplicate/low-value candidates do not append new memory records.                                                                                                                                                                                      |
| `GET`  | `/api/memories/recall`            | Hybrid recall. Required `?q=`. Optional memory filters plus `?entityId=`, `?recallAgentId=`, `?limit=`, `?lexicalLimit=`, `?recentLimit=`, `?relationshipLimit=`. Returns score breakdowns for lexical, vector, relationship, recency, and importance signals.                                                                                                               |
| `GET`  | `/api/memories/readiness`         | Memory readiness report. Runs the baseline memory eval harness and returns embedding provider, model, vector count, persistence status, total checks, passed checks, and failure messages.                                                                                                                                                                                   |
| `GET`  | `/api/memories/{memory_id}/trace` | Return an evidence trace for one memory, including the memory, relationships that cite it, and involved entities.                                                                                                                                                                                                                                                            |
| `POST` | `/api/memories/retention`         | Apply an explicit retention/decay policy. Optional fields: `maxAgeMillis`, `minImportance`, `maxMemories`, and `decayHalfLifeMillis`. Returns decayed memory adjustments plus removed memory and relationship IDs.                                                                                                                                                           |
| `POST` | `/api/memories/relationships`     | Create or update a directed memory relationship edge. Required fields: `sourceAgentId`, `sourceAgentName`, `targetAgentId`, `targetAgentName`, `relationshipType`. Optional fields: `sourceKind`, `targetKind` (`agent`, `user`, `system`, `external`; default `agent`), `summary`, `strength`, `confidence`, `evidenceMemoryIds`, `tags`, `roomId`, `worldId`, `sessionId`. |
| `GET`  | `/api/memories/relationships`     | List relationship edges. Optional `?entityId=`, `?agentId=`, `?sourceKind=`, `?sourceAgentId=`, `?targetKind=`, `?targetAgentId=`, `?relationshipType=`, `?roomId=`, `?worldId=`, `?sessionId=`, `?minStrength=`, `?minConfidence=`, `?limit=`.                                                                                                                              |

Set `ANIMAOS_RS_MEMORY_SQLITE_FILE=/path/to/memories.sqlite` to load the daemon-owned memory snapshot on startup and autosave runtime memory writes from HTTP routes, tools, runtime evaluators, and retention policy runs. For lightweight JSON memory persistence, set `ANIMAOS_RS_MEMORY_FILE=/path/to/memories.json` instead; embeddings remain in process unless `ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE` is also set. Set only one memory store variable. BM25 memory search uses the multilingual analyzer and does not remove stop words or stem terms by language. Runtime evaluation uses evaluated writes for reflection evidence, extracts explicit user-stated preference/remember facts, indexes stored memories for semantic recall, and links the responding agent to the user entity when request metadata includes `userId`/`userName`.

### Memory editing (owner)

These routes let the owner fix what the companion remembers. Each requires local-owner authorization (`403` `local owner authorization required` otherwise; `GET /api/memories/facts` needs owner read authorization) and answers `Cache-Control: no-store`, errors included. The older memory routes above keep their shapes and authorization: a non-owner can still reach them, and only the routes below are owner-only. Every mutation runs to completion even when the client disconnects, saves the memory store, and on a failed save restores the previous state and answers `503`. The one exception is a save that reached the store but whose durability is uncertain: the change then stays in effect (as with the older memory routes), the embedding may be stale until the memory is edited again, and the answer is still `503`.

| Method   | Path                                 | Request                                      | Answers                                                                                                                                                                                                                                              |
| -------- | ------------------------------------ | -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PATCH`  | `/api/memories/{memory_id}`          | `{ content?, importance?, tags? }`           | `200` the memory. `tags: null` clears the tags, an array replaces them, absent keeps them. Content is re-indexed and re-embedded. `400`, `403`, `404`, `503`.                                                                                        |
| `DELETE` | `/api/memories/{memory_id}`          | optional `{ expectedContent }`               | `200` `{ id, removedRelationships, updatedRelationships, updatedFacts }`. Exact current text is checked under the write lock when supplied; stale text returns `409`. Empty body retains legacy ID-only deletion. `400`, `403`, `404`, `409`, `503`. |
| `GET`    | `/api/memories/facts`                | `?agentId=&subject=&includeInactive=&limit=` | `200` `{ facts }`, newest first. `limit` is 1 to 500 (default 100); `includeInactive` is `true` or `false`. `400` (`limit must be from 1 to 500`, `includeInactive must be true or false`), `403`.                                                   |
| `PATCH`  | `/api/memories/facts/{fact_id}`      | `{ value }` (1 to 500 characters)            | `200` `{ fact, superseded }`: the fact is replaced by a new active one that supersedes it. `400`, `403`, `404`, `409` (`Only an active fact with a value can be edited`), `503`.                                                                     |
| `DELETE` | `/api/memories/facts/{fact_id}`      | none                                         | `200` `{ id }`. `403`, `404`, `503`.                                                                                                                                                                                                                 |
| `DELETE` | `/api/memories/entities/{entity_id}` | `?kind=agent\|user\|system\|external`        | `200` `{ kind, id, removedRelationships, removedFacts }`. `400` (`kind query parameter must be one of agent, user, system, external`), `403`, `404`, `409` (`This entity still has memories; delete them first`), `503`.                             |

Notes:

- An entity's key is its kind plus its id, so `entities/{id}` takes a required `kind` query parameter (the one addition to the path). Deleting an entity also removes the relationships and the facts that name it, because the store would otherwise recreate the entity on load. An `agent` entity that still owns memories, including the companion itself, answers `409`. Deleting the `user` entity is soft in practice: the evaluator recreates it on the next message that names the user.
- Facts carry no agent of their own, so `?agentId=` keeps a fact when at least one memory in its evidence belongs to that agent; a fact with no evidence is listed only without `agentId`. `?subject=` matches the subject id, or the subject name ignoring ASCII case.
- Guarded memory deletion accepts a complete exact string, including empty or Unicode text, without trimming or edit-character limits; existing request byte limits apply. Every nonempty body requires string `expectedContent` and rejects null/missing/unknown fields. The SDK optional second string argument preserves no-body callers; the console always supplies the reviewed string and requires review/reconfirmation after409. Text equality does not detect ABA or metadata-only changes. Existing persistence uncertainty and separate embedding cleanup remain; an error does not always mean no effect.
- Deleting a memory removes it from the citations of facts and relationships. A fact or relationship left with no evidence is kept (only agent relationships left empty are removed), so the owner can forget it directly.
- Edited memory content, tags, and replacement fact values must not contain invisible tag or direction-override characters (`400`).
- Rolling back: M7 changes no control-plane snapshot and no memory store format, so an earlier daemon reads the same data.

### Swarms

| Method | Path                            | Description                                                                                                                                                 |
| ------ | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`  | `/api/swarms`                   | List all registered swarm snapshots.                                                                                                                        |
| `POST` | `/api/swarms`                   | Create a swarm. Body includes `strategy`, `manager`, `workers`, and optional runtime limits such as `maxTurns`. Returns `201` with the created swarm state. |
| `GET`  | `/api/swarms/{swarm_id}`        | Get one swarm snapshot.                                                                                                                                     |
| `POST` | `/api/swarms/{swarm_id}/run`    | Dispatch a task (`{"text":"..."}`) to the swarm. Blocks until completion and returns the updated swarm state plus task result.                              |
| `GET`  | `/api/swarms/{swarm_id}/events` | Subscribe to the swarm's server-sent event stream. Event names include lifecycle updates plus task, tool, and token events.                                 |

---

## Local development

For the normal workspace loop, run:

```bash
bun dev --host rust
```

This starts the daemon and browser surfaces. After control-plane persistence is
loaded, the daemon automatically starts every enabled Telegram connector whose
credential is available in the OS credential store. Add or replace bot tokens
through the web connector settings or daemon API; do not set a shared Telegram
token environment variable.

To run only the host process through Nx, use:

```bash
bun x nx run rust-daemon:dev
```

If you want the raw daemon process without the Nx wrapper, use the `cargo run -p anima-daemon` profiles below.

For local memory development, the simplest durable profile is:

```bash
ANIMAOS_RS_MEMORY_SQLITE_FILE=./data/runtime-memories.sqlite \
ANIMAOS_RS_MEMORY_EMBEDDINGS=local \
ANTHROPIC_API_KEY=sk-ant-... \
cargo run -p anima-daemon
```

On Windows, if a running daemon keeps `target/debug/anima-daemon.exe` locked during validation, rerun tests with an isolated cargo target dir:

```bash
CI=1 CARGO_TARGET_DIR=target/validation-rust-daemon bun x nx run rust-daemon:test --skipNxCache
```

Contributor-focused local workflow docs live in `apps/contributor-docs/src/content/docs/hosts/rust-daemon/local.mdx`.

---

## Starting the daemon

```bash
# From the workspace root
ANTHROPIC_API_KEY=sk-ant-... cargo run -p anima-daemon

# With Postgres persistence for step logs, control plane, and memory snapshots
ANIMAOS_RS_PERSISTENCE_MODE=postgres \
DATABASE_URL=postgres://user:pass@localhost/anima \
  ANTHROPIC_API_KEY=sk-ant-... \
  cargo run -p anima-daemon

# With durable runtime memory in SQLite
ANIMAOS_RS_MEMORY_SQLITE_FILE=./data/runtime-memories.sqlite \
  ANTHROPIC_API_KEY=sk-ant-... \
  cargo run -p anima-daemon

# With restart recovery for the agent/swarm control plane
ANIMAOS_RS_CONTROL_PLANE_FILE=./data/control-plane.json \
  ANTHROPIC_API_KEY=sk-ant-... \
  cargo run -p anima-daemon

# With real local multilingual semantic memory embeddings
ANIMAOS_RS_MEMORY_SQLITE_FILE=./data/runtime-memories.sqlite \
ANIMAOS_RS_MEMORY_EMBEDDINGS=fastembed \
ANIMAOS_RS_MEMORY_EMBEDDING_MODEL=intfloat/multilingual-e5-small \
  ANTHROPIC_API_KEY=sk-ant-... \
  cargo run -p anima-daemon

# With Ollama/OpenAI-compatible semantic memory embeddings
ANIMAOS_RS_MEMORY_SQLITE_FILE=./data/runtime-memories.sqlite \
ANIMAOS_RS_MEMORY_EMBEDDINGS=ollama \
ANIMAOS_RS_MEMORY_EMBEDDING_MODEL=nomic-embed-text \
  ANTHROPIC_API_KEY=sk-ant-... \
  cargo run -p anima-daemon
```

Verify it is up:

```bash
curl http://127.0.0.1:8080/health
# {"status":"ok"}

curl http://127.0.0.1:8080/ready
# {"status":"ready","controlPlaneDurability":"postgres",...}
```

Browse the Scalar UI at `http://127.0.0.1:8080/docs`.

---

## Online staging profile

Use the Dockerfile from the repository root:

```bash
docker build -f hosts/rust-daemon/Dockerfile -t anima-daemon:staging .
```

For staging, set `ANIMAOS_RS_PERSISTENCE_MODE=postgres` and `DATABASE_URL`, leave `ANIMAOS_RS_CONTROL_PLANE_FILE`, `ANIMAOS_RS_MEMORY_FILE`, and `ANIMAOS_RS_MEMORY_SQLITE_FILE` unset, and run one daemon replica. In that profile, Postgres stores step logs plus control-plane and memory snapshots in `host_snapshots`.

The optional `memory_consolidate` tool requires an explicit agent grant and the
existing Write approval policy. Its strict request is `content` plus two to eight
`sources` objects containing `id` and exact full `expected_content`. It validates
the complete own visible durable Fact cohort, same scope/current room and tag
capacity under one memory write lock, then saves one new summary Fact and archives
sources as Reflection in one mutation. Source IDs/text/citations remain intact;
a shared group tag associates them. Historical search may still return sources;
this is active-Fact consolidation, not erasure or disk compaction. A stale source
rejects the entire batch. Existing committed-durability-uncertain errors can still
have effects; do not retry blindly. Embeddings are best effort after canonical
save. The console exposes an opt-in **Allow consolidating saved facts** checkbox;
standard profiles do not grant it. Exact duplicate default-scope Fact `memory_add`
returns the existing record without changing metadata or writing the store/index.
No extra model/summarizer request is introduced; summary accuracy is model/owner
judgment, not inferred from embedding similarity.
Automatic chat context uses the configured memory embedding backend once before
its run loop. It keeps lexical matches first and fills spare slots with eligible
semantic matches, using the same own-Fact and visibility rules. Context remains
frozen for that run and retains six-record, 800-character excerpt and
4,800-character total limits. Two process-wide worker slots and an inclusive
five-second result deadline bound blocking lookup; unavailable or late lookup
falls back to the lexical snapshot. A timed-out worker retains its slot until it
exits. Remote embedding configuration may add one embedding request per run.
The default `local` backend stays download-free; choose `fastembed` for learned
multilingual recall. Its first model initialization may download public weights.
Declared English/Spanish retrieval cases require separate real-model acceptance;
positive similarity alone is not general quality or truth evidence.
Vectors remain process-local for v1 staging. Start with `ANIMAOS_RS_MEMORY_EMBEDDINGS=local`; if cold-start vector rebuilds become expensive and your platform has a persistent volume, set `ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE=/var/lib/anima/memory-embeddings.sqlite`. See `.env.staging.example` for the full recommended env profile.

---

## Startup sequence

1. Read `ANIMAOS_RS_HOST`, `ANIMAOS_RS_PORT`, `ANIMAOS_RS_MAX_REQUEST_BYTES`, `ANIMAOS_RS_REQUEST_TIMEOUT_SECS`, `ANIMAOS_RS_PERSISTENCE_MODE`, `ANIMAOS_RS_CONTROL_PLANE_FILE`, `ANIMAOS_RS_MAX_CONCURRENT_RUNS`, and `ANIMAOS_RS_MAX_BACKGROUND_PROCESSES`, then bind the TCP listener.
2. Build `RuntimeModelAdapter::from_env()` to load provider API keys and base URLs.
3. Initialize `tracing` so every HTTP request is logged with method, URI, latency, and `x-request-id`, and log the daemon as an `ephemeral` control plane at startup.
4. If persistence mode is `postgres`, require `DATABASE_URL`, connect, run embedded migrations from `./migrations`, and fail startup immediately if any step fails.
5. Inject `SqlxPostgresAdapter` into shared daemon state so all new agents get step persistence automatically, including reuse of completed tool results when a caller retries with the same metadata retry key.
6. Configure memory persistence: explicit JSON/SQLite env vars win, otherwise Postgres mode stores the memory snapshot in `host_snapshots`.
7. Configure control-plane persistence: explicit `ANIMAOS_RS_CONTROL_PLANE_FILE` wins, otherwise Postgres mode stores registered agents and swarms in `host_snapshots`.
8. Start Axum with request-id propagation, tracing middleware, request timeouts for both standard and blocking `/run` routes, a semaphore-backed concurrent-run admission limit, readiness and metrics endpoints, and graceful shutdown on `Ctrl+C`.

---

## Operational notes

- Without `ANIMAOS_RS_CONTROL_PLANE_FILE` or Postgres mode, the daemon acts as an `ephemeral` control plane and agent/swarm registrations live only in memory.
- With `ANIMAOS_RS_CONTROL_PLANE_FILE` or Postgres mode, the daemon restores registered agents, registered swarms, latest snapshots, and swarm message history after process restart. Work that was marked running before the restart is restored as failed/interrupted; the daemon does not resume a model turn from the middle.
- Postgres persistence stores step logs, control-plane snapshots, and memory snapshots in the host database. Step logs also let retried runs reuse completed tool results when the request repeats the same metadata retry key.
- Rolling back to a pre-M2 daemon in JSON mode: stop the daemon, restore `<ANIMAOS_RS_CONTROL_PLANE_FILE>.pre-sessions.bak` over the control-plane file (the first M2 startup wrote this backup before upgrading the snapshot to version 5; spec §13.3), and move `history.sqlite` (or the file named by `ANIMAOS_RS_HISTORY_SQLITE_FILE`) aside before starting the older daemon. The older daemon never reads the history store, but leaving that file in place and upgrading again later would bring back the messages this rollback just discarded.
- Rolling back from M3 to an M2 daemon: M2 refuses the version-6 snapshot that the first M3 start saves (`unsupported control plane store version: 6`). Stop the daemon, then in JSON mode restore `<ANIMAOS_RS_CONTROL_PLANE_FILE>.pre-live-runs.bak` over the control-plane file, or in Postgres mode copy the `control_plane.backup.5` row of `host_snapshots` over the `control_plane` row. The first M3 start wrote this backup before upgrading the snapshot from version 5 to 6 and left the pre-M2 `.pre-sessions.bak` as it was. Keep the history store in place: M2 reads it for the messages it pruned from the control plane. Control-plane changes made since the upgrade are lost, but the messages and runs mirrored to the history store after the upgrade stay there.
- Rolling back from M4 to an M3 daemon: stop the daemon and restore `<file>.pre-approvals.bak` over the control-plane file (JSON store) or the `control_plane.backup.6` row over the current one (Postgres). Control-plane changes made since the upgrade are lost, including approval policies and rules, sessions, and runs. The history store stays in place; an M3 daemon ignores its `approvals` rows.
- Rolling back from M5 to an M4 daemon: M4 refuses the version-8 snapshot the first M5 start saves (`unsupported control plane store version: 8`). Stop the daemon, then restore `<file>.pre-skills.bak` over the control-plane file (JSON store) or the `control_plane.backup.7` row over the current one (Postgres). Control-plane changes made since the upgrade are lost, including the skills registry and skill drafts. The `skills/` folders and `.anima-trash/skills/` stay in the workspace; an M4 daemon ignores them.
- Rolling back from M6 to an M5 daemon: M5 refuses the version-9 snapshot the first M6 start saves (`unsupported control plane store version: 9`). Stop the daemon, then restore `<file>.pre-automations.bak` over the control-plane file (JSON store) or the `control_plane.backup.8` row over the current one (Postgres). Control-plane changes made since the upgrade are lost, including automation names, active hours, cron and one-time automations, counters, and fire records not yet in the history store. Rows already in the history store's `schedule_runs` table stay; an M5 daemon ignores them.
- Rolling back from M8 to an M7 daemon: M7 refuses the version-10 snapshot the first M8 start saves (`unsupported control plane store version: 10`). Stop the daemon, then restore `<file>.pre-usage.bak` over the control-plane file (JSON store) or the `control_plane.backup.9` row over the current one (Postgres). Control-plane changes made since the upgrade are lost, including the owner's pricing overrides. Usage rows already in the history store's `usage` tables stay; an M7 daemon ignores them.
- The history store's Postgres tables (migration `20260923000000_history_store.sql`: `history_messages`, `history_runs`, `history_usage`, `history_approvals`, `history_schedule_runs`, `history_attachments`) require **Postgres 12+**, because `history_messages.search` is a generated column. Rolling back to a pre-M2 daemon in Postgres mode needs more than restoring `control_plane.backup.<version>`: sqlx's migrator refuses to start when an applied migration is missing (`VersionMissing`), so also run `DELETE FROM _sqlx_migrations WHERE version = 20260923000000;` before starting the older daemon, and move the `history_*` tables aside too (rename or drop them) — the same as JSON mode's `history.sqlite`, keeping them and upgrading again later would bring back the messages this rollback just discarded.
- Readiness reports `database: "missing"` and returns `503` when `ANIMAOS_RS_PERSISTENCE_MODE=postgres` is set without a working database adapter.
- Metrics include configured limits such as `anima_daemon_max_concurrent_runs` and `anima_daemon_max_background_processes`, plus current counts like `anima_daemon_agents`, `anima_daemon_swarms`, and `anima_daemon_background_processes`.

---

## Architecture note

This host implements `ModelAdapter` (via `RuntimeModelAdapter`) and
`DatabaseAdapter` (via `SqlxPostgresAdapter`) from the reusable Rust core,
wiring those infrastructure-free crates to real provider APIs and Postgres at
the process boundary.

## Companion transport, shutdown and retention

Default provider clients have a 15-second connection timeout and 300-second
whole-request timeout, including streamed bodies. Caller-injected clients keep
their caller's policy. Ctrl+C and Unix SIGTERM close task producers before accepted
runs drain (30-second coordinator bound, 40-second overall shutdown deadline; Compose allows 45 seconds). Unsettled work after that deadline requires interruption review on restart. Cancellation
records distinguish owner Stop, helper deadline and daemon shutdown. Partial output
and usage remain accounted for; cancellation does not roll back external effects.

Telegram delivery releases the global control-plane transaction during transport.
An ephemeral scoped claim prevents duplicate in-process selection; generation and
outbound-row checks fence settlement after replacement/deletion. Delivery remains
at-least-once. A network success followed by failed save is uncertain and can be
retried; retry uses the existing outbound text instead of running the model again.

Session list views derive search/snippets from captured state outside the read
lock. Hot-tail pruning retains protected turns and full history remains in the
history store. Session records are retained until owner deletion; automatic record
expiry has no product policy yet. Archive hides a record rather than deleting it,
and retained metadata/query cost can grow with session count.

Use the console's accepted runs and SSE endpoints for new clients. SDK additions
are additive; missing `/api/status` returns an older-daemon update instruction in
the console, while auth/transport failures retain their actual meaning. Browser
voice requires no server speech endpoints. Focused Hermes provider-format reuse
is credited with its MIT notice in
[HERMES-NOTICE](../../packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE).
Selected-channel Discord REST chat is available through the native connector
setup and durable session flow. See [Contributing connectors](CONNECTOR-CONTRIBUTING.md#selected-discord-channel)
for activation, sender access, polling limits and uncertain-output review. Its
focused upstream adaptation is credited in
[Discord notice](src/connectors/discord/HERMES-NOTICE). Installable community
modules remain a follow-up. Selected Drive filename search and Google Docs/plain
text reads use native agent-bound tools; see [Selected Drive files](#selected-drive-files).

For upgrades and rollback, use a stopped complete snapshot/history/memory/uploads/
keyring/Caddy backup generation with recorded binaries and vault password. The
v13 pre-Drive snapshot backup (`.pre-drive.bak` for v12 input) is only a component;
restoring JSON alone can
leave newer history rows that reappear later. Do not edit snapshot versions or
apply historical pre-M2 SQL instructions to an M9/M10 rollback. See
[VPS backup and acceptance procedures](../../deploy/vps/README.md).

## Standard Markdown agent files

Anima's long-running Rust agent uses standard Markdown identity and instructions.
Selected upstream helpers from revision `f3ef9db68332063a3625661f3a9b259f60abdb4e` are credited in
`HERMES-NOTICE` and the core crate notice. This supports familiar instruction files;
behavioral differences and prompt-screening limits are documented below.

Place UTF-8 `SOUL.md` in the configured workspace root to define the companion's
identity. The companion is the agent with `settings.workspaceRole: lead`. Other
agents and helpers do not inherit that root identity. An accepted SOUL replaces
legacy bio, lore, adjectives, topics, style and `system` persona for the isolated
run; knowledge and the daemon's mandatory role/delegation/origin guidance remain.
Missing, empty, blocked or excluded SOUL retains the complete legacy persona.
Mandatory tool permissions and owner authority remain server policy.

An optional boolean `settings.agentFilesProfileEnabled: true` selects only that
agent's `.animaos-swarm/agent-profiles/<hex-agent-id>/SOUL.md`. Encode each UTF-8 byte
of the server-generated agent ID as two lowercase hexadecimal digits. The fake ID
`demo` would use `.animaos-swarm/agent-profiles/64656d6f/SOUL.md`. Profile mode does
not fall back to root SOUL even for the companion. Configure this through the
existing agent creation/workspace configuration flow; the existing agent PATCH
route does not edit settings, and the console preview has no profile toggle.
Enabling the setting creates no directories or files. Loading never writes or
migrates existing instructions. No ambient OS home, process cwd, Hermes profile,
caller-selected project directory or uploaded attachment is discovered.

Project instruction priority at the configured root is:

1. `.anima.md`, then `ANIMA.md`; complete initial YAML frontmatter is stripped
   from these files without granting it configuration authority.
2. `AGENTS.override.md`, then `AGENTS.md`, then `agents.md`.
3. Optional Claude instruction filenames, tried in this order:

   ```text
   CLAUDE.md
   claude.md
   ```

4. `.cursorrules`.

The loader implements a bounded root-to-project AGENTS chain for internal scoped
calls, with nearest Anima precedence, but public runs currently use the configured
root. Missing, empty, unreadable or blocked candidates fall through to a usable
candidate and appear in the manifest. This deliberately differs from upstream
blocked-source shadowing. Identical normalized AGENTS text is included once.
`.cursor/rules/*.mdc`, `USER.md` and `MEMORY.md` automatic loading are deferred;
existing durable memory remains authoritative.

The source text is read once at run admission, before final history selection.
Active runs retain that snapshot through tools, steers and compaction; queued runs
read current files when they start. Agent configuration is frozen at durable run
start, as before; later configuration edits affect later runs. Files consume a
reserved part of the context budget, with identity retained before project text.
File bodies are not copied into configuration, transcript, semantic memory or
persisted run metadata by this feature. The selected model provider receives the
accepted prompt text. The Settings preview and owner-authorized
`GET /api/agents/{agent_id}/agent-files` return relative names, states, byte/character
counts, digests and threat IDs only. `mode: current_preview` and `capturedAtMs`
explicitly describe a current load, which can differ from an active run's snapshot.
Refresh does not reload an active run.

Reads are capability-relative, reject linked/reparse/nonregular files and stay
within the configured workspace. Bounds are 262,144 bytes per file, 1,048,576
aggregate bytes, 96 candidates, 16 directory levels, 65,536 normalized scan
characters, 20,000 rendered characters and a five-second total loading deadline.
Two retained worker permits bound blocking reads; cancellation returns without
waiting for the worker, and late results cannot install instructions. An OS-stuck
read can retain a permit until it returns; this does not promise thread termination
or an atomic multi-file filesystem transaction. Head/tail truncation is Unicode
scalar safe, uses a bounded 70/30 allocation after reserving its marker, and counts
markers and headings within the render budget.

All sources, including SOUL, undergo bounded suspicious-instruction screening
before truncation. A filename does not attest authorship; this Rust port has no
upstream owner-authored SOUL exemption or trust database. Invalid UTF-8, NUL,
hidden controls, oversized input and scanner-limit excess are excluded. The
focused regex filter can miss attacks or reject benign prose; passing it grants
no permissions and does not establish injection immunity. Full Unicode NFKC is
not implemented; full-width ASCII folding is the documented bounded adaptation.

Agent `write_file`, `edit_file` and `multi_edit` calls targeting instruction names
require an exact owner decision even under reusable allow rules. Explicit denies
still apply. Targets and revisions/absence are checked before waiting and again
before dispatch; drift refuses the write. Such decisions create no reusable grants.
Direct/swarm calls without an approval gate refuse these writes. Protected names
also cover deferred USER/MEMORY/cursor rules, with case-insensitive matching.
Ordinary uploads retain their existing sanitized display/collision names under `uploads/<date>/...`; root
instruction discovery never promotes them. Authenticated owner file editing keeps
its existing authority. Approved shell/background process execution retains full
OS access and can change instructions: file-tool protection is not a sandbox.

Validation uses fake workspaces, provider captures and in-process owner requests.
It does not exercise live providers, real daemons, deployment restart, containers
or Postgres. M12 connector preparation follows this milestone; no new connector
provider or dynamic community loader is introduced here.

## Selected Drive files

Configure the existing Google OAuth application in Connectors → Google OAuth app, enable the Google
Drive API for that project, and register the exact redirect URI displayed there:
`/api/connectors/gdrive/callback` under the configured `ANIMA_PUBLIC_BASE_URL`.
The local default is `http://127.0.0.1:8080`. In the agent's Connectors view, paste
1–16 Google Drive/Docs links or file IDs, review the extracted IDs, then open Google
consent. Links are parsed locally; they do not discover files or grant access.

Google grants `https://www.googleapis.com/auth/drive.readonly`, which permits
read/download access across the account. Anima separately restricts this agent to
its immutable selected IDs. Only the connected agent receives `drive_search` and
`drive_read`; IDs supplied to a tool do not grant authority. Search checks selected
file names, using exact-ID metadata requests, rather than whole-account or content
search. Change the selection by disconnecting and connecting again.

Google Docs export as plain text; stored `text/plain` requires valid UTF-8. Other
formats, folders and shortcuts return unsupported metadata. Reads reject trashed
or download-denied files, stream at most 1 MiB, and return at most 32,768 Unicode
scalars with explicit truncation, source link, timestamps and source/returned-text
digests. Metadata changes observed around a read discard its content; this does
not guarantee an atomic provider revision. File contents remain untrusted tool
evidence and are never automatically instructions or memory writes.

Disconnect removes local agent access and generation-specific credentials. It
never calls Google's project-wide revocation endpoint. Pending private key cleanup
is shown explicitly. Google's account app-access page can revoke the broader
permission, but that can also disconnect Gmail and Calendar. Drive tokens remain
separate from those integrations. A restart invalidates unfinished consent; start
a fresh attempt. Live account consent, access and refresh lifetime require separate
acceptance. The selected Rust port retains [source attribution](src/connectors/drive/HERMES-NOTICE).
