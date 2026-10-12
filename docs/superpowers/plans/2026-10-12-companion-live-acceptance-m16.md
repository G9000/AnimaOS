# M16 isolated local companion acceptance

Draft for controller activation/rulings; source inspection only by the planner.
Owner authorized: "Allow isolated local testing". Root owns execution and evidence.
Branch: `g9000/companion-live-acceptance-m16`, from merged main `63d8eaa`.
Goal: exercise existing web/Rust daily flow with private persistent test data.
No new feature, provider integration, dependency, database server or container.

## Authority and isolation

- Only the root-owned Rust daemon and web console may run for this exercise.
- Root already reserved free loopback ports 8080/4200 and started both surfaces.
- Use the separate private profile under this milestone's `live/` directory.
- Root records exact control-plane, memory, history and workspace paths before use.
- Fresh JSON control plane, JSON memory and local history SQLite are test data.
- SQLite is a local file store, not permission to start a database server.
- Embeddings are disabled; no model downloads or embedding provider probes.
- Disable environment-file autoload; never read/import the owner's normal profile.
- Never inspect existing user tokens, profiles, keyring entries or secret files.
- Keep Telegram/Discord/mail/Calendar/Drive disconnected; no connector sends/consent.
- No containers, Docker, Postgres, port55432, production records or other services.
- Preserve unrelated `nx.json`/`anima.yaml`; no stash/reset/restore/worktree operations.
- Root records owned wrapper/child PIDs and exec session IDs at process creation.
- Logs/evidence contain safe statuses and synthetic content, never auth values.
- No shell/process tool effects, arbitrary web fetches or external writes are needed.
- The planner does not run services, tests, Cargo, Git or live HTTP requests.

## Existing interfaces and evidence

Use the real console at `http://127.0.0.1:4200`, daemon at loopback 8080.
Local-owner browser requests use the existing allowed Origin policy.
Root may use its private owner authorization for originless requests; never print it.
Do not weaken authorization or broaden CORS to make a test pass.

| Surface                 | Existing interface                                                       |
| ----------------------- | ------------------------------------------------------------------------ |
| Readiness/providers     | `GET /ready`, `GET /api/status`, existing provider catalog               |
| Workspace/lead creation | Onboarding; `POST /api/workspace/bootstrap`                              |
| Instruction preview     | Settings preview; `GET /api/agents/{id}/agent-files`                     |
| Chat/history            | Sessions; `GET /api/agents/{id}/sessions/{session}/messages`             |
| Send/stream             | `POST /api/agents/{id}/sessions/{session}/runs`; agent events SSE        |
| Stop/status             | `POST /api/agents/{id}/runs/{run}/stop`; run GET                         |
| Memory                  | Save to memory/Memory; `POST /api/memories`, existing search/edit routes |
| Workspace files         | Files; existing bounded list/preview routes and `read_file`              |

Bootstrap's existing body is `{workspace,agent,workers?}` with camelCase DTOs.
Workspace carries `rootPath,companyName,mission,values`; use synthetic names only.
Use real selected provider/model if available; preserve minimal read-only tool grants.
Send uses `text`, optional attachment/skill fields, and a unique Idempotency-Key.
Reuse the same key only when checking exact replay; never reinterpret it as new work.
Memory create carries `agentId,agentName,type,content,importance,tags,scope` as applicable.
Use existing UI/SDK contracts rather than introducing an acceptance-only endpoint.

## Execution sequence and bounded cases

1. Record baseline: branch/source, owned processes, private paths, readiness/status.
   Confirm no test connectors exist and the visible console uses the isolated daemon.
2. Inspect provider availability through the existing safe catalog/status only.
   A configured flag alone does not prove inference; deterministic is synthetic.
   If no real model is available, record the blocker and request owner UI sign-in.
   Do not recover a key from an existing profile or substitute fake evidence silently.
3. Complete onboarding with one synthetic lead companion and the private workspace.
   Confirm selected provider/model and workspace in the persisted returned record.
4. Create harmless UTF-8 `SOUL.md` and `ANIMA.md` in the isolated workspace only.
   Include a unique identity marker and a short instruction; no hidden instructions.
   Refresh preview: inspect accepted names/states/digests, not private source bodies.
5. Open one chat and ask a short identity question. Record actual provider response,
   stream/run completion and matching session history; note model adherence separately.
6. Send a second bounded request and use Stop while its run is queued/running.
   Verify terminal stopped status, UI/history agreement and no lingering active run.
   If the response finishes before Stop, record the race and retry once at most.
7. Save a synthetic fact through Save to memory. Find it in Memory, edit it once,
   and confirm its exact stored value through the existing read/search interface.
   Ask recall only with the existing memory tool grant; model quality is separate.
8. Create a harmless fixture text file in the private workspace; list/preview it.
   Ask the companion to read it with `read_file` if a real tool-capable model exists.
   Confirm returned content/path and no access outside the selected workspace.
9. Quiet the profile: no active run, approval, tool process or new pending mutation.
   Gracefully stop only the owned daemon; preserve every private store/workspace path.
10. Restart that daemon using the identical recorded private launch configuration.
    Do not reset data, change workspace roots or load the normal owner environment.
    Confirm readiness, lead/session/messages, edited memory, files and instruction
    preview persist; send one fresh chat turn if real inference is available.
11. Record each result as passed/failed/blocked/unexecuted, with source and timestamp.
    Synthetic runtime/UI evidence and actual provider evidence remain distinct.

## Failures, fixes and gates

Stop at the actual failing boundary, capture static errors/status and minimal repro.
Do not restart repeatedly, reset data or weaken permissions to conceal a failure.
Root authorizes a bounded real-bug fix in the current branch when warranted.
Source changes require focused regression plus relevant fresh current-tree gates.
Unchanged source reuses M14/M15 gate evidence; no duplicate full fake suite.
Provider setup/model quality failures do not by themselves prove a runtime bug.
Actual connector accounts, microphone, hosted deployment and off-site restore stay out.

## Owned-process cleanup

Root first sends Ctrl+C to each recorded owned exec session through `write_stdin`.
Wait up to the existing graceful shutdown budget, then verify recorded children exited.
If necessary, use PowerShell only for verified remaining owned PIDs:
`Get-Process -Id $M16DaemonPid -ErrorAction SilentlyContinue`
`Stop-Process -Id $M16DaemonPid -ErrorAction Stop`
`Get-Process -Id $M16WebPid -ErrorAction SilentlyContinue`
`Stop-Process -Id $M16WebPid -ErrorAction Stop`
Repeat those exact commands for recorded wrapper/child PIDs only after verifying
their captured start time/image still match; a reused PID is never owned authority.
Never kill by process name, port, broad descendant query or an unverified computed PID.
Root checks only its reserved ports/listeners after cleanup and records final state.
Retain private test artifacts/evidence for review; no recursive profile deletion.

## Top five risks for controller rulings

1. Isolation: a port or profile mix-up could touch ordinary state. Pin launch paths,
   owner scope and recorded process identities; stop on an unexpected agent/store.
2. Provider evidence: no configured real model may exist. Use owner UI setup or mark
   inference blocked; deterministic tests never stand in for real chat/tool quality.
3. Stop/effects: a fast reply can finish first and cancellation cannot undo effects.
   Use harmless work, inspect terminal state, bound retries and claim only observed Stop.
4. Persistence: restart with different paths can look like data loss or false success.
   Reuse the exact stopped store/workspace generation and verify each retained artifact.
5. Cleanup: wrappers/children may outlive a cancelled session. Drain owned work and
   stop only verified recorded PIDs; keep evidence and leave unrelated services untouched.

Controller appends accepted rulings before dependent acceptance execution proceeds.

## Controller rulings on the risks (binding)

1. Ruling: Pin fresh validation state/memory/history/workspace and loopback ports, skip .env autoload, leave connectors empty — approval is only for isolated local testing — cost if wrong: stop on unexpected state rather than touch an owner's workspace/accounts or databases. Local history SQLite is an embedded test file; no database server starts.
2. Ruling: Use normal redacted configured-provider UI and the existing ChatGPT connection without inspecting private credentials — real model evidence must use the actual runtime, not the deterministic adapter — cost if wrong: a provider-specific failure is reported and fixed or login requested, never replaced with a fake success. No connector consent or personal data is sent.
3. Ruling: Exercise Stop with harmless synthetic text and at most two bounded attempts, verify saved terminal status — a fast completed reply is not successful cancellation — cost if wrong: cancellation remains explicitly unverified; no external action is used to slow it down.
4. Ruling: Quietly restart the same owned daemon with identical local paths and verify identity, conversation, memory and file separately — a new profile or a marker repeated from chat alone proves no durable memory — cost if wrong: inspect the failed layer rather than claim continuity from health alone.
5. Ruling: Gracefully stop owned sessions and verify exact process identities before any forced cleanup — broad process/port kills can interrupt unrelated work — cost if wrong: report a remaining owned process rather than kill by name or alter user services. Retain private test evidence, never purge profiles.

Activation: owner explicitly allowed isolated local testing after M15 merged.
Root started only Nx rust-daemon:dev and web:serve with CARGO_INCREMENTAL=0,
no automatic env files, fresh local paths and disabled embeddings. Browser
onboarding selects the validation workspace and gpt-5.6-luna; normal UI reports
an existing ChatGPT subscription connection. Synthetic SOUL/note and prompts
contain no user-private data. The planner inspected source only. Root owns actual
acceptance, any concrete fix/review/gates and final publication under standing
authority. No separate preflight audit or new breadth milestone.

## Acceptance outcome (2026-10-12)

The approved isolated web and Rust profile exposed a real provider rejection:
DataValue stores numbers as f64, so integer tool-schema bounds such as minLength
were serialized as 1.0. The shared adapter now canonicalizes only eight JSON Schema
integer keywords in schema-valued positions. Arguments, numeric constraints and
instance data remain unchanged. The same gpt-5.5 fresh request then returned hello.

Ruling: Canonicalize schema integer bounds only — the real provider rejected the
old request representation — cost if wrong: the same-model live test fails;
generic number semantics must remain unchanged.

Terminal errors now preserve a fixed, safe classification from nested/top-level
structured errors, adapting the upstream terminal-error behavior. Unknown fields
and messages remain private. HTTP diagnostics are bounded at 64 KiB. Temporary
diagnostic probes were removed before final validation.

Observed through the actual console and daemon:

- Onboarding created a private test companion and workspace with SOUL.md.
- A real model reply identified itself as Anima validation and used read_file for
  cedar-742 and memory_search for cedar tea and independently saved amber-913.
- Stop cancelled a harmless text reply; the server saved cancelled/stopped with
  partial text and no tool effects.
- After quiet restart with identical stores, conversation, completed and cancelled
  runs persisted. A fresh real chat independently read the file and recalled memory.
- Readiness was ready, history was non-ephemeral SQLite, pending flushes were zero,
  and messaging connectors stayed at zero. Only recorded validation processes were
  stopped; ports 8080 and 4200 were free afterward.

Evidence is retained privately under
.superpowers/sdd/2026-10-12-companion-live-acceptance-m16/. No owner credentials or
personal files were inspected or copied. Quiet recorded-process cleanup is not
proof that every development wrapper propagates graceful shutdown. Live connector
delivery/Google consent, attachments/vision, microphone, schedules, VPS/TLS/reboot
and offsite restore were not exercised. This is selected local companion acceptance,
not broad upstream parity or production deployment proof.

Review: one core/daemon batch plus a bounded schema-fix supplement found no remaining
actionable issues. No web/SDK source changed. Focused adapter regression tests: 18
passed. Current-tree Nx rust-daemon:test passed 2304 tests (core636, host1668),
with 8 host tests ignored. Nx rust-daemon:lint passed. Owned formatting/link checks
and exact publication/merge evidence are recorded in the milestone ledger.
