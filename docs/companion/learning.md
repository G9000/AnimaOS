# Learning and skill refinement

The Rust daemon reviews useful task evidence and can turn a correction or working procedure into a reusable skill. This changes saved procedures and context; it does not train model weights. A completed reply, a skill load and a saved revision are separate observations from verified task success.

## Task reviews

Free autonomy reviews eligible completed tasks automatically. Strict and Custom require the existing automatic-review opt-in, or an explicit **Review this task** request. Task reviews share a limit of two reservations per rolling 24 hours. Ordinary tasks need five settled durable tool calls. A successful skill load, an owner correction signal, or a recovered tool failure can make a shorter task eligible within the same budget. These signals select reviews; they do not certify correctness.

The separate conversation review still runs after ten counted owner messages and proposes durable core-memory edits. Its existing limits and approval rules remain.

Task evidence contains bounded owner input/reply excerpts, settled tool names/statuses, selected paths or arithmetic/verification arguments, and up to two freshly read approved skill excerpts. Arbitrary file bodies and tool output remain omitted. Secret-line filtering is heuristic, not an exhaustive secret detector. Source material remains untrusted data. Automatic procedural learning is limited to direct owner web/API work and proven owner-only connector chats; shared/external senders gain no skill-writing authority.

## Skills

The reviewer can return no lesson, ordinary advice, a new skill candidate or a small patch against an exact supplied package hash. It prefers the skill used in the task, then a related existing skill, then a new reusable task class. Observed old versions are labeled separately from the current package. A patch must quote text actually supplied in this review, match uniquely in the freshly read target, and preserve the remaining procedure. A supporting file is patchable only when that exact file was loaded and its text supplied.

Candidates enter the existing source-linked skill draft pipeline. Strict/Custom leave a draft for the owner. Free can activate the exact draft through the same approval path, with its complete previous package and undo entry in one control-plane save. Current autonomy, source eligibility, source digest, workspace, bundle restrictions and package hash are checked again. Changed or revoked sources and changed bases refuse installation. A failed handoff is visible as skillError; the lesson remains inspectable.

No-op patches, ambiguous replacements and duplicate new procedures are refused. Suggestions containing skill candidates are not also copied into reflection memory. A proposal is still unverified advice; automatic activation does not label it proven.

Completed reviews with an unfinished handoff are reconciled after restart. A linked pending Free draft can resume through the same fresh checks; a rejected/approved draft is never recreated. Review dismissal and version changes continue to fence installation.

The owner can inspect and approve/reject drafts in **Skills** and use the existing undo action for automatic changes. Maintenance never deletes, archives, disables or changes permissions automatically.

## Explicit learning and packages

Send /learn with a workflow, sources and requirements, or bare /learn for the current conversation. The ordinary agent uses its current tools and propose_skill; source text grants no authority. SKILL.md contains the reusable steps, prerequisites, pitfalls and verification. Supporting references, templates, scripts, examples and assets use the existing bounded files array. Scripts stay inert until existing execution permissions allow them.

## Usage and outcome evidence

Successful native load_skill calls and explicit skill context loads record the exact package hash, agent, session and run. Repeated supporting-file loads in the same run/version count once. At most 64 observations per skill are retained; they survive package edits and restart. Terminal usage settles with the run snapshot; interruption recovery never turns an unfinished load into success.

GET /api/skills/{slug}/learning returns observations, per-version counts and non-mutating duplicate/staleness suggestions. Completion counts mean accepted runtime replies, not independently verified outcomes. The current package is flagged stale after 30 days without approval/use; no content is changed.

POST /api/skills/{slug}/feedback is owner-only. Its body names agentId, sessionId, runId and version from an existing observed use, plus corrected and optional verifiedEvidence. A correction can describe any terminal use and clears prior verification. Independent verification requires a completed observed use and a nonempty bounded evidence statement. Models have no self-verification tool. These reports are owner attestations; the server cannot infer that an assertion is true.

## Acceptance

The reproducible harness is tools/skill-learning-acceptance.mjs, with fixtures under packages/core-rust/acceptance/skill-learning. It compares a deliberately incorrect baseline, a corrected task, an automatic source-linked skill revision and an independent fresh fixture. The fresh agent/session has empty canonical/core memory, the same primary configuration, and no foreground skill-writing or memory tools. Passing requires an actual revised-hash load and independently checked output bytes, followed by persistence and undo checks. Offline fixture checks and scripted-adapter tests are reported separately from real-provider acceptance.
