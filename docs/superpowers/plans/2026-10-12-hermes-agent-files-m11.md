# M11: Standard Markdown Agent Files — Lean Plan

> Draft only. Controller publishes final plan after five binding rulings. Use superpowers:subagent-driven-development with bounded Codex workers. This assignment prepares an ignored plan only; no source implementation, staging or commit.

**Goal:** Standard SOUL.md and AGENTS.md define agent identity and project instructions in
the existing Rust long-running runtime, with safe bounded loading, isolated run admission,
protected model writes and an owner manifest.

**Architecture:** Reusable prompt structures and identity rendering live in anima-core.
Capability-relative discovery, context filtering, runtime admission and owner API live in
rust-daemon. Each run accepts one immutable snapshot before first model request and context
trimming. Owner preview uses the same loader but is explicitly a current preview, not an
active-run history record.

**Stack:** Existing Rust, tokio, cap-std, cap-fs-ext, sha2, regex, axum, serde, utoipa, SDK,
React, Bun and Nx. No new dependencies or dependency features.

## Authority and rules

- Read AGENTS.md and current companion-console handover.
- M10 PR #16 merged with explicit owner authorization.
- Controller reports merged main f80e653 as base.
- Branch: g9000/hermes-agent-files-m11.
- Historical handover's open-M10 status is superseded.
- Continue M10 lean interface/rule/test structure.
- Owner authorized unsigned controller commits.
- Workers never stage, commit or switch branches.
- Controller stages explicit owned paths only.
- Never stage nx.json or anima.yaml.
- Preserve unrelated local changes.
- Never stash, reset, restore or create worktrees.
- Never read Hermes homes, profiles or tokens.
- Hermes installed source inspection is allowed.
- No installations, new dependencies or features.
- No daemon, dev server, DB or container starts.
- No real provider or connector requests.
- Keep existing ignored Postgres tests ignored.
- ANIMA_SKIP_SERVICE_TESTS=1 for SDK/full gates.
- CARGO_INCREMENTAL=0 for every Cargo invocation.
- Controller serializes Cargo compilation.
- No new CARGO_TARGET_DIR under current disk policy.
- New behavior lives in focused modules.
- Large coordinator files gain wiring only.
- Lock order remains control transaction → state → leaf.
- No std lock across await.
- No separate preflight audit.
- Two bounded daemon batch reviews.
- One final SDK/web combined review.
- One fix wave, then required current-tree gates.
- Exactly five risks await controller rulings below.
- No automatic file creation or migration.
- No rewrite of existing SOUL, AGENTS or anima.yaml.
- USER/MEMORY import is future existing-memory work.
- Do not introduce another durable memory authority.
- M12 connector foundation remains separate.
- No wholesale Hermes Python loop or plugin port.
- No OpenClaw full-parity claim.
- Controller owns delivery, staging and publication.

## Verified upstream reference

Source: C:/Users/leoca/AppData/Local/hermes/hermes-agent.

Pinned revision: f3ef9db68332063a3625661f3a9b259f60abdb4e.

MIT, Copyright (c) 2025 Nous Research.

Inspected symbols:

- agent/prompt_builder.py::load_soul_md.
- agent/prompt_builder.py::\_truncate_content.
- agent/prompt_builder.py::\_omitted_headings.
- agent/prompt_builder.py::\_agents_md_directory_chain.
- agent/prompt_builder.py::\_agents_md_candidates.
- agent/prompt_builder.py::\_hermes_md_candidates.
- agent/prompt_builder.py::\_claude_md_candidates.
- agent/prompt_builder.py::\_cursorrules_candidates.
- agent/prompt_builder.py::discover_context_files.
- agent/prompt_builder.py::build_context_files_prompt.
- agent/system_prompt.py::\_identity_parts.
- agent/system_prompt.py::\_context_files_part.
- agent/context_file_sources.py.
- tools/file_tools_write_guards.py::\_protected_instruction_reason.
- tools/file_tools_write_guards.py::\_check_protected_instruction_write.
- tools/threat_patterns.py::scan_for_threats.
- tools/threat_patterns.py::\_PATTERNS.
- LICENSE.

injection_scanner.py does not exist at this revision. Actual shared scanner is
tools/threat_patterns.py; notices and implementation must use real names.

Hermes treats SOUL as primary identity and prevents duplicate insertion into project
context. It pins explicit profile home to avoid context-variable leakage. Project source
priority is .hermes.md/HERMES.md → AGENTS chain → CLAUDE → cursor. AGENTS per-directory
priority is override, uppercase, lowercase; empty or unreadable files fall through. Chain
contents are deduplicated and the whole chain capped.

Anima deliberately omits ambient cwd/home fallback, config-generated homes, distribution
manifests and owner-authored scanner exceptions. Standard filename compatibility is the
claim. SOUL filename/private namespace does not prove authorship; all sources are
context-scanned. Source may be poisoned by external checkout, uploads, owner mistakes or
approved OS processes.

Retain full upstream MIT license in packages/core-rust/crates/anima-core/HERMES-NOTICE and
hosts/rust-daemon/HERMES-NOTICE for substantially adapted behavior. Include revision, exact
source paths/symbols, selected threat IDs, fixture provenance and intentional differences.
Preserve existing anima-model-adapters/HERMES-NOTICE credits. License must include
permission, notice-retention and disclaimer, not a copyright line alone.

## Verified code anchors

- anima-core/src/runtime.rs::build_system_prompt emits bio, lore, adjectives, topics, knowledge, style, config.system, then provider context.
- Appending SOUL to config.system leaves contradictory identity.
- state/run_commit.rs::build_run_runtime selects history and returns RunBuild.
- RunContextReport excludes fixed system/tool overhead; calibration absorbs residual overhead.
- sessions/context.rs::ContextBudget reserves current input/reply before history.
- agent_runs/compact.rs rebuilds runtime after compaction.
- agent_runs.rs supplies immutable roster/delegation/origin guidance.
- tools/workspace_bytes.rs contains WorkspaceBytes and capability traversal.
- It already uses cap-std, cap-fs-ext, nofollow and nonblocking regular reads.
- sha2 exists in both anima-core and daemon.
- regex exists in daemon, not core.
- tools/workspace.rs canonical-only helpers include ambient fallback.
- Agent todo files use .animaos-swarm/agent-tasks/<hex-id>.json.
- ID encoding is each UTF-8 byte as two lowercase hex digits.
- tools.rs::execute_tool performs live checks, registry lookup, optional approval then dispatch.
- Approval gate can be absent for direct/swarm execution.
- Ordinary write class does not force exact owner approval.
- approvals/gate.rs already waits without locks and rechecks live authority.
- Helpers cannot acquire owner approvals independently.
- routes/workspace_files.rs is existing owner upload/file authority.
- attachments/files.rs::save_upload stores uploads/{date}/{candidate}; root-only discovery does not probe that tree.
- SettingsPanel.tsx is existing settings surface.
- SDK package is @animaOS-SWARM/sdk.
- Initial read-only Nx inventory failed Missing Platform Dependency win32-x64.
- Controller resolves through established M10 environment/escalation, not installation.

## Scope and identity choices

Optional setting agentFilesProfileEnabled is a boolean in existing settings.additional.
Default absent/false means no per-agent profile. True derives
.animaos-swarm/agent-profiles/<hex-agent-id> under captured configured workspace root. Never
accept arbitrary profile path from chat/session/API. Reuse todo namespace/hex convention
without changing task files.

Companion with profile disabled may read workspace-root SOUL.md. Specialists without enabled
profile keep legacy identity even if root SOUL exists. Enabled profiles read only their own
SOUL. Missing profile SOUL does not inherit root. Helpers never inherit parent profile or
companion root. Their immutable bounded helper/delegation role always remains.

Companion classification comes from existing server-owned workspace role, not user message
fields. Explicit profile mode disables root fallback for companion too; absent profile files
remain normal legacy fallback. Enabling does not create directories or seed files. No read
writes canonical config, bio or system text.

Project directory is configured workspace root in M11. No session cwd input or chat path
grant. Internal loader supports explicit root-relative project directory for bounded chain
fixtures/future authorized work-directory use; runtime supplies empty relative path. No
public projectDir setter.

No configured workspace means disabled/unavailable_workspace. Loader never calls
current_dir, OS home, HERMES_HOME or ambient environment fallback. Root comes from same
captured WorkspaceConfig as run tool context. A user mentioning a path cannot change
filesystem authority.

## Source selection

SOUL occupies independent primary identity slot; never duplicate it as project context.
Project source kinds follow verified Hermes order. AGENTS remains documented portable
default; existing HERMES file can shadow it visibly.

Hermes candidate: nearest .hermes.md or HERMES.md within authorized root-to-project chain.
Dotted lowercase wins at same directory. Strip one complete leading YAML frontmatter block
only for Hermes source. Do not parse or execute YAML. Frontmatter cannot change tools,
model, root or profile. Frontmatter-only file falls through.

AGENTS fallback: root-to-directory chain, each directory first usable AGENTS.override.md,
AGENTS.md, agents.md. Empty/unreadable/blocked candidates report state and fall through.
Normalize CRLF, trim, deduplicate equal text, render later directory sections as more
specific guidance under immutable engine authority.

CLAUDE fallback: project directory CLAUDE.md then claude.md. Cursor fallback: .cursorrules
only. Defer .cursor/rules/\*.mdc globbing and conditional metadata; no recursive discovery.
Protect their writes nonetheless. No USER.md/MEMORY.md automatic loading.

SOUL exact spelling on case-sensitive filesystems; case-insensitive OS resolves naturally.
Write-protected basename matching is case-insensitive everywhere. Source selection happens
only after safe read, decode, scan and budget eligibility; blocked higher kind cannot shadow
legitimate lower kind.

Candidates outside root are never probed. Git boundary lookup uses bounded marker existence
within root, not git process or .git content parsing. Root-only runtime does not need
ambient repository detection. Boundary/depth caps apply to internal nested fixtures.

## Attachment authority boundary

Actual inspected M9 save_upload in hosts/rust-daemon/src/attachments/files.rs stores
uploads/{date}/{candidate}, not workspace-root filename. Current root-only loader never
probes uploads or interprets attachment.name as a context path. Preserve this actual
boundary and add regression tests; no upload storage change is necessary for M11.

A file attachment named AGENTS.md/SOUL.md is ordinary data. Instruction discovery accepts
only explicit server-owned project/profile candidate paths. Never use user-message
attachment display name, attachment.data, path hint, uploaded directory or download metadata
to select a control file.

If future trusted nested project directory support can point into uploads, reject that
namespace before discovery. Optional safe reserved-name suffix defense belongs only to a
controller-bound follow-up if another upload path actually writes root candidates. Do not
invent a nonexistent reverse registry lookup or change current display names/IDs.

Reserved instruction names can be shared between protected-write matching and future upload
defense, but the loader only recognizes exact candidates. All existing files receive
filtering; no author trust inferred from storage. Owner authenticated deliberate file edit
remains distinct from attachment transport.

## Pure core contract

New anima-core/src/agent_files.rs owns reusable rendering, truncation and primary identity
types. No HTTP, disk, tokio, cap-std or approval dependencies.

```rust
pub struct RuntimePromptIdentity {
    pub text: String,
}
pub struct RenderedAgentFiles {
    pub identity: Option<RuntimePromptIdentity>,
    pub project_context: String,
    pub estimated_tokens: u64,
}
pub struct AgentFileSection {
    pub label: String,
    pub text: String,
}
pub struct AgentFileRenderBudget {
    pub max_identity_chars: usize,
    pub max_project_chars: usize,
    pub max_total_chars: usize,
}
```

Add transient runtime setter applying accepted identity/context together. Not new
AgentConfig persistent fields. Canonical runtime defaults None. Durable snapshot
serialization excludes text override. Rebuilt primary runtime explicitly reattaches accepted
snapshot; never restore by rereading disk.

SOUL replaces legacy bio/lore/adjectives/topics/style and legacy config.system persona
contribution. Keep config.knowledge as existing nonpersona knowledge. Missing/empty/excluded
SOUL retains complete legacy behavior. Host engine guidance is separate always-appended
transient section; do not guess persona/policy by parsing prose.

Immutable helper role, delegation permissions, roster/origin guidance, tool config,
model/provider/secret selection and deadlines remain code-owned. File text cannot change
them. No copying files into canonical config then restoring; overlapping sessions make that
unsafe.

Prompt composition has one identity contribution, one project contribution and existing
provider context. Preserve summary data framing and established hidden-Unicode behavior.
Source labels are generated safe root-relative strings, not arbitrary Markdown from file
names.

Truncate Unicode scalar values with 70/30 head/tail allocation after marker reserve. Marker
counts toward caps. UTF-8 slices cannot split emoji/CJK. Marker uses permitted relative
filename only, never absolute path or secret recovery instruction. No unlimited per-file
caps multiplied across chain.

Host may reduce pure character budget according to available tokens. Identity precedes
project allocation. If useful identity frame cannot fit, expose budget_excluded and retain
legacy identity rather than empty persona. Existing unlimited legacy system input remains an
honest preexisting total-budget limitation.

## Host snapshot and manifest contract

Host modules: agent_files/{mod,disk,selection,scan,manifest,runtime,write_policy}.rs. Scope
is host-only and not serialized.

```rust
struct AgentFileScope {
    agent_id: String,
    session_id: String,
    run_id: Option<String>,
    workspace_root: PathBuf,
    project_relative: String,
    profile_enabled: bool,
    companion: bool,
    config_revision: String,
}
struct AcceptedAgentFiles {
    rendered: RenderedAgentFiles,
    manifest: AgentFilesManifest,
}
struct AgentFilesManifest {
    captured_at_ms: u64,
    snapshot_digest: String,
    identity_source: IdentitySource,
    project_source_kind: Option<ProjectSourceKind>,
    estimated_tokens: u64,
    entries: Vec<AgentFileManifestEntry>,
}
```

Manifest DTO excludes root, source text and config secrets. Entry fields: relative_path,
kind, state, raw_byte_count, admitted_char_count, digest nullable, truncated, threat_ids and
reason_code nullable. Closed states: loaded, truncated, shadowed, duplicate, empty, missing,
unreadable, invalid_utf8, linked, non_regular, oversized, blocked, budget_excluded,
timed_out, cancelled.

Top availability: available, unavailable_workspace, stale_scope. Preview response explicitly
says mode=current_preview. It does not claim active accepted snapshot identity. Digest is
SHA-256 over accepted labels/content digests/states/render version/output. Raw file digest
covers actual raw bytes; admitted counts cover normalized render. Do not call ordinary hash
cryptographic.

Threat IDs only, no matched text. Error reason closed codes, not OS messages containing
absolute paths. Absent/unreadable digest null; successfully read empty bytes gets real
digest. Source size/digest appears only from bytes held by accepted capability.

Snapshot is immutable acceptance of individually verified reads, not atomic multi-file
filesystem transaction. Same-length external rewrite is an honest boundary: actual
bytes/digest define accepted content; no claim global filesystem freeze.

Runtime accepted snapshot held only by run/compaction ownership and discarded with run. No
historical manifest cache, persistent trust KV, RunRecord fields, snapshot version bump or
new schema. No body persistence in transcript, history, memory, run changes or logs.
Provider receives admitted system text as normal model request.

Owner current preview invokes same loader with current captured owner scope, then returns
manifest only. If run prompt view exists, it uses actual accepted snapshot, not current
preview loader. Never call preview active prompt. No raw instruction-content endpoint.

## Capability reads, bounds and cancellation

Reuse WorkspaceBytes in tools/workspace_bytes.rs. Extend narrowly with nofollow
metadata/read primitive for instruction files; canonicalize then fs::read is insufficient.
Held capability traversal checks each directory and leaf, refuses links/reparse/junctions
and uses existing regular-file nonblocking open.

read_instruction_file(relative,max_bytes) obtains metadata from held nofollow parent,
regular handle, max+1 bounded bytes and descriptor/final length checks. Refactor
read_bounded only if expected-size signature needs a one-operation adapter. Preserve upload
collision and writer behavior.

Named bounds: MAX_FILE_BYTES=262144, MAX_TOTAL_READ_BYTES=1048576, MAX_CANDIDATES=96,
MAX_DIRECTORY_DEPTH=16, MAX_SCAN_CHARS=65536, MAX_RENDER_CHARS=20000,
DEFAULT_LOAD_DEADLINE=5 seconds. Errors/missing probes count candidates. No recursive scan.

Worker runs in spawn_blocking, owns capability/buffers/cancel flag/semaphore permit, never
state/global/run capacity guard. Check deadline/cancellation before every
candidate/read/scan/render unit. Async caller selects completion versus run
cancel/shutdown/deadline and returns legacy fallback without waiting for blocked worker.

Dropping JoinHandle cannot stop kernel blocking read. Limit worker admission to two permits
acquired under same deadline. Timed-out worker retains permit until actual completion; do
not release early and spawn unlimited replacements. Future admission times out/falls back.
Stop/session ownership never waits for loader permit.

Only async caller can publish accepted result after generation/config/session recheck. Late
result is dropped. No Drop publication. Capture scope under state read lock, release before
loader, reacquire for check. One bounded recapture retry under original five-second total
deadline; second mismatch yields stale_scope.

Kernel-stuck thread termination is outside guarantee; existing process shutdown timeout
remains. Test blocked reader via deterministic hook/channel, not real pathological
filesystem or sleeps. Platform-specific FIFO/link fixtures run only where supported, with
reported exclusions.

## Context filtering

Actual upstream scanner tools/threat_patterns.py uses regex scopes all/context/strict and
MAX_SCAN_CHARS. Adapt focused context policy with existing daemon regex and exact recorded
pattern IDs, not wholesale full scanner claim. Never regex from file content.

Include unambiguous all-scope patterns: prompt_injection, sys_prompt_override,
disregard_rules, bypass_restrictions, html_comment_injection, hidden_div, translate_execute,
deception_hide, exfil_curl, exfil_wget, read_secrets. Include bounded context
exfiltration/agent-env bypass patterns only after verifying Rust regex translation and
false-positive fixtures. Do not import role_hijack/identity_override patterns that block
ordinary SOUL personality declarations.

Every source kind including SOUL gets identical suspicious-content blocking. No
filename/profile owner-authored exemption and no new trust database. Legitimate imperative
AGENTS and 'You are Anima' SOUL must load.

Strip one leading UTF-8 BOM, normalize CRLF, reject invalid UTF-8/NUL and existing
disallowed hidden-control forms. Use established sanitizer where available; no Unicode
dependency. Do not silently remove attacks and label clean.

Scan entire normalized text before render truncation. Text beyond scanner cap is wholly
oversized/excluded; never scan head only then load hostile tail/middle. Byte cap is distinct
from scalar scanner cap. This bounded conservative difference is documented.

Regex compiled once, bounded haystack and filler. Pattern hits return identifiers; manifest
shows blocked state. Filter has false positives/misses and never proves authorship or
injection immunity.

## Model write authority

Protected matcher covers normalized input and safely resolved target for
write_file/edit_file/multi_edit. Names include all supported sources, AGENTS.override,
future USER/MEMORY and .cursor/rules/\*.mdc. Case-insensitive on all platforms. Unsafe
resolution fails closed; do not follow unsafe links just to classify.

Exact owner decision is required before handler effect even under allow rule/session
allowance. Explicit deny stays deny. Request binds run/session, exact args, target set and
target revision including absence. Mixed multi-edit checks all targets before any effect.

After approval, recheck live authority, canonical aliases, path containment and target
revision. Drift causes conflict/refusal, not approved edit of new bytes. Use existing
digest/revision patterns and compute reads outside global locks. No full private body in new
status metadata.

No approval gate in direct/swarm context means protected write denied. Unknown tool stays
unknown before approval; unconfigured tool stays denied. Helpers cannot borrow parent
approval and obey existing helper owner-gate restriction.

Owner authenticated file API remains deliberate trusted edit surface under existing path
safety. File read grants no authority. External owner-approved shell/process tools retain
existing trusted full OS capability; M11 does not force all bash calls through new prompts
and does not claim a filename sandbox protects arbitrary shell execution.

Document shell/OS bypass honestly. Existing process policy, helper bans and owner approvals
remain. Do not infer comprehensive instruction protection from shell substring parsing.
Ordinary notes writes retain existing policy.

## Token budget and compaction

Current Phase A calls build_run_runtime while holding state and selects history before
Phase B. Implementation must split capture from final selection, rather than merely append
files in Phase B. Capture owned canonical runtime/history/session summary/calibration and
scope, release the lock, load accepted files, then select final history with instruction
reserve. Revalidate admission/config/session before committing started record and trimmed
indicator. Preserve existing idempotency, queued admission, inherited stop and live-run
registration fencing. No asynchronous filesystem read occurs inside the start transaction.

If existing accepted-run registration must precede load for stop visibility, use a narrow
registered admission checkpoint and delay final history/trim report until the accepted
snapshot is ready. Do not persist a provisional zero-instruction trim report. Controller
must select one consistent choreography after inspecting current start rollback semantics;
the invariant is one final budgeted selection before primary/compaction model work.

Current Phase B constructs config.system by prefixing original_config.system to live roster
and delegation text, then appends peer origin. Refactor that host suffix into a separately
attached always-rendered engine section; do not pass the old composite through SOUL persona
suppression. With SOUL, suppress only original_config.system contribution, never whole
Phase B suffix. Tool list modifications remain current Phase B behavior. apply_skills uses
SkillContextProvider rather than config.system, so preserve its ordinary provider context.

Add regression proving helper/delegated guard, peer-origin warning, roster and requested
skill provider still appear once with SOUL. Test compaction rebuilding before Phase B and
then applying host guidance exactly once; avoid repeated persona or engine sections.

Accept snapshot before history trimming/first model call. Estimate final admitted file text
using existing TokenEstimator conventions. Reserve current input/reply/file contribution
before whole-turn selection. Files appear once, not system plus provider duplication.

Extend ContextBudget history_tokens_with_instructions(current,instructions) using saturating
subtraction; keep old helper as zero-instruction compatibility. Add transient
instructions_estimate_tokens to RunContextReport. Include file estimate once in raw
calibration denominator while existing residual fixed prompt/tool overhead stays calibration
convention.

Reduce project contribution before SOUL when token budget tight. Recompute estimate after
truncation; marker included. Avoid calibrated estimate twice. Current input/reply preserve
existing guarantees. Tiny file budget excludes SOUL with manifest state rather than empty
override.

Compaction summarizer receives summary instructions and dropped history, never file
persona/context. Do not summarize SOUL into durable summary. Rebuilt primary reuses same
Arc/reserve; no reread after summarizer. Compaction failure/stop does not change accepted
identity.

Run commits exclude transient prompt overrides; canonical config/transcript remain
unchanged. True overlap tests with two sessions, queued refresh and provider spies prove
request prompt identity, not just setter state.

## Owner API and small UI

GET /agents/{agent_id}/agent-files returns current owner preview manifest. Use existing
authenticated agent access; no arbitrary path query or session-created authority. Capture
current config/root under lock, release for same loader, return capturedAt with
mode=current_preview. Unknown agent 404. Cancellation/deadline apply identically.

Current preview may differ from an already active run; explicit UI says edits affect next
run and preview is current files. No active historical cache or persistent run schema.
Actual prompt view, if existing, remains accepted-run data without reread.

SDK agentFilesManifest(agentId,signal?) returns typed content-free DTO. Encode agent ID and
reuse client errors/cancel. Export via daemon-types/index. Old daemon 404 uses existing
compatibility guidance, no endless polling.

AgentFilesPanel child of SettingsPanel shows identity Legacy/Root SOUL/Profile SOUL,
selected project kind, relative names, states, digest prefix, capture time and exclusions.
No absolute root or source content, editor/import flow, filesystem watcher or new global
poll.

Optional profile checkbox uses existing settings.additional boolean save; server derives
path. Enable does not create files. Late responses cannot replace another selected agent.
Loading/error/empty states follow accessibility and existing styling. Keep ViewHarness
wiring small.

## Ownership and six implementation tasks

### Task 1: Core identity slot and pure rendering

Owned files:

- Create packages/core-rust/crates/anima-core/src/agent_files.rs.
- Create packages/core-rust/crates/anima-core/src/agent_files/tests.rs if needed.
- Modify packages/core-rust/crates/anima-core/src/lib.rs exports.
- Modify packages/core-rust/crates/anima-core/src/runtime.rs narrow wiring.
- Extend packages/core-rust/crates/anima-core/src/runtime/tests.rs.
- Create packages/core-rust/crates/anima-core/HERMES-NOTICE.

Steps:

- [ ] Inspect prompt assembly and snapshot serialization.
- [ ] Write meaningful failing prompt replacement tests.
- [ ] Define transient identity/render interfaces.
- [ ] Implement pure bounded Unicode rendering.
- [ ] Apply runtime override without canonical mutation.
- [ ] Separate immutable host guidance from persona.
- [ ] Preserve old no-file prompt behavior.
- [ ] Retain full MIT/provenance.
- [ ] Run focused core tests via confirmed Nx filter.
- [ ] Report files/tests to controller, no staging.

Tests:

1. None identity preserves old bio/system prompt.
2. SOUL appears once in first model request.
3. Old bio absent with SOUL.
4. Old lore absent with SOUL.
5. Old adjectives absent with SOUL.
6. Old topics/persona absent with SOUL.
7. Old style absent with SOUL.
8. Old config.system persona absent with SOUL.
9. Immutable host helper role remains.
10. Immutable delegation/origin guidance remains.
11. Tool allowlist unaffected.
12. Provider/model settings unaffected.
13. Knowledge retained as nonpersona context.
14. Empty identity never suppresses legacy.
15. Project text rendered once.
16. Summary framing stays data.
17. Durable snapshot excludes file text.
18. Canonical runtime remains untouched.
19. Overlapping runtime overrides stay distinct.
20. Truncation emoji head boundary valid UTF-8.
21. Truncation CJK tail boundary valid UTF-8.
22. Marker included within scalar cap.
23. Tiny budget bounded and valid.
24. No absolute recovery path.
25. Label controls cannot inject headings.
26. Chain cap cannot multiply by file count.
27. Deterministic renderer.
28. Existing prompt/runtime tests remain passing.

Use provider-captured request, not setter-only assertion, for identity replacement. Core has
no regex dependency. Shared pure pattern definitions may live here, but host compiles
supported patterns with existing regex. Do not add dependency or implement a regex engine.

### Task 2: Safe loader, selection and context policy

Owned files:

- Create hosts/rust-daemon/src/agent_files/mod.rs.
- Create hosts/rust-daemon/src/agent_files/disk.rs.
- Create hosts/rust-daemon/src/agent_files/selection.rs.
- Create hosts/rust-daemon/src/agent_files/scan.rs.
- Create hosts/rust-daemon/src/agent_files/manifest.rs.
- Create adjacent focused test modules.
- Modify hosts/rust-daemon/src/tools/workspace_bytes.rs narrow extension.
- Modify host module root declaration.
- Create hosts/rust-daemon/HERMES-NOTICE.

Steps:

- [ ] Create fake workspace fixtures.
- [ ] Write profile/source precedence regressions.
- [ ] Reuse capability root and nofollow reads.
- [ ] Implement fixed bounded enumeration.
- [ ] Implement BOM/frontmatter/UTF-8 normalization.
- [ ] Compile vetted supported upstream patterns.
- [ ] Scan whole admissible text before truncation.
- [ ] Render snapshot/manifest from one byte set.
- [ ] Add owned blocking-worker deadline wrapper.
- [ ] Add bounded semaphore/cancellation behavior.
- [ ] Verify notices and intentional differences.
- [ ] Run focused loader tests.

Selection tests:

1. Root SOUL only companion fallback.
2. Specialist does not inherit root SOUL.
3. Enabled specialist uses own profile.
4. Missing own profile retains legacy.
5. Companion enabled profile disables root fallback.
6. Helpers never inherit parent/root SOUL.
7. UTF-8 hex-ID paths stay one component.
8. Malicious ID separators cannot traverse.
9. No workspace means no ambient probes.
10. No OS home/HERMES_HOME access.
11. Nearest Hermes source within root wins.
12. .hermes.md beats HERMES.md same directory.
13. Frontmatter stripped only Hermes.
14. Frontmatter cannot configure authority.
15. Frontmatter-only falls through.
16. Usable Hermes shadows AGENTS visibly.
17. AGENTS root-to-leaf stable ordering.
18. Override beats ordinary AGENTS.
19. Empty override falls through.
20. Unreadable override falls through with state.
21. Blocked override falls through safely.
22. Uppercase candidate precedes lowercase.
23. Duplicate normalized chain text once.
24. CLAUDE lower-priority fallback.
25. Empty CLAUDE permits lowercase.
26. .cursorrules last fallback.
27. Cursor glob deliberately absent.
28. USER/MEMORY not loaded.
29. Outside-root git ancestor never probed.
30. Candidate/depth limits enforced.
31. Missing digest null.
32. Empty file digest present.
33. Raw digest covers original BOM/CRLF bytes.
34. Invalid UTF-8 excluded without lossy decode.
35. Embedded NUL excluded.
36. Scalar scanner limit excludes whole source.
37. Aggregate byte limit bounds reads.
38. Merged renderer cap respected.
39. Manifest kind exactly rendered kind.
40. DTO contains no text/absolute path.

Filesystem and deadline tests:

1. Leaf symlink refused.
2. Directory symlink refused.
3. Profile namespace link refused.
4. Root ancestor replacement cannot redirect.
5. Windows reparse attributes refused.
6. Junction fixture only when creation permitted.
7. Unix FIFO nonblocking refusal where supported.
8. Socket/device/directory leaves refused.
9. Descriptor check catches metadata/open replacement.
10. Changed file length excluded.
11. Deadline includes semaphore wait.
12. Two held permits bound concurrency.
13. Third loader deadline does not spawn worker.
14. Caller cancellation returns before fake reader.
15. Late worker cannot install result.
16. Timed-out worker keeps permit until finish.
17. Blocking worker owns no global locks.
18. Deleted agent/session prevents acceptance.
19. Stale config retries at most once.
20. Retry remains under original deadline.
21. Temp fixture cleanup uses established patterns.
22. No real Hermes profiles in fixtures.

Scanner tests:

1. Upstream selected positive IDs match.
2. Ordinary imperative AGENTS accepted.
3. Ordinary SOUL identity accepted.
4. SOUL attack blocked without author exemption.
5. Curl/wget secret exfil blocked.
6. Benign API variable discussion accepted.
7. Hidden HTML injection blocked.
8. Bounded filler does not explode runtime.
9. Attack in middle caught before truncation.
10. Case variants match supported patterns.
11. Existing hidden-control rules preserved.
12. Beyond scanner cap wholly excluded.

Pattern fixture provenance names exact upstream IDs/revision. Tests establish bounded
focused filter behavior, not scanner equivalence or immunity.

### Task 3: Protected writes and upload authority

Owned files:

- Create hosts/rust-daemon/src/agent_files/write_policy.rs.
- Extend hosts/rust-daemon/src/approvals/gate.rs.
- Extend hosts/rust-daemon/src/approvals/policy.rs.
- Modify hosts/rust-daemon/src/tools.rs minimal wiring.
- Extend hosts/rust-daemon/src/attachments/files.rs tests only if needed for verified upload path regression.
- Extend existing upload/attachment fixtures and approval tests.
- Keep attachment IDs/display names and registry handlers unchanged.

Steps:

- [ ] Write allow-rule/no-gate bypass regressions.
- [ ] Classify normalized and resolved targets safely.
- [ ] Force exact owner decision before allowances.
- [ ] Preserve explicit deny/live tool authority.
- [ ] Bind approval to target revision and args.
- [ ] Recheck after owner decision.
- [ ] Refuse protected direct/swarm no-gate calls.
- [ ] Verify root-only discovery never follows upload namespace.
- [ ] Preserve existing display metadata and uploads/date collision behavior.
- [ ] Document trusted full OS process limit.
- [ ] Run focused approval/upload tests.

Tests:

1. write_file AGENTS asks under default policy.
2. Always-allow rule cannot bypass.
3. Session path allowance cannot bypass.
4. Explicit deny still denies.
5. edit_file SOUL asks.
6. multi_edit all targets checked before effect.
7. Mixed target batch writes nothing before owner.
8. AGENTS.override protected.
9. Hermes names protected.
10. CLAUDE/cursor names protected.
11. Deferred cursor .mdc writes protected.
12. USER/MEMORY writes protected.
13. Case variants protected across OS.
14. Normalized traversal cannot evade.
15. Canonical alias cannot evade.
16. Unsafe resolution refuses.
17. Profile disabled target still protected.
18. New file approval binds absence.
19. Target drift while waiting refuses.
20. Changed args cannot reuse approval.
21. Different target cannot reuse approval.
22. Different run/session cannot reuse approval.
23. No reusable allowance from protected decision.
24. Direct no-gate protected write denied.
25. Swarm no-gate protected write denied.
26. Unknown tool stays unknown.
27. Unconfigured tool denied before request.
28. Helper cannot borrow parent decision.
29. Owner deny/timeout writes nothing.
30. Halt/delete while waiting writes nothing.
31. Authority revoked after approve writes nothing.
32. Ordinary notes write unchanged.
33. Authenticated owner edit allowed.
34. Existing process policy unchanged.
35. Upload AGENTS remains under uploads/date.
36. Upload SOUL remains under uploads/date.
37. Case-variant attachment cannot become control source.
38. Hermes/override attachment cannot become source.
39. Upload collision allocator unchanged.
40. Original display filename unchanged.
41. Attachment ID/download stored path unchanged.
42. Loader never probes uploads namespace.
43. Ordinary upload behavior unchanged.
44. Approval details omit full private body.
45. No lock held across owner wait.
46. Existing settlement races remain passing.

No blanket bash forcing. Shell remains trusted full OS; docs explicitly limit filename
protection. This is existing authority, not permission to alter model process policy
broadly.

### Task 4: Runtime admission, budget and compaction

Owned files:

- Create hosts/rust-daemon/src/agent_files/runtime.rs.
- Modify hosts/rust-daemon/src/state/run_commit.rs.
- Modify hosts/rust-daemon/src/sessions/context.rs.
- Modify hosts/rust-daemon/src/agent_runs/compact.rs.
- Modify hosts/rust-daemon/src/agent_runs.rs wiring only.
- Create hosts/rust-daemon/src/agent_runs/agent_files_tests.rs.
- Extend focused queue/context/stop/compaction tests.
- Modify state exports as needed.

Steps:

- [ ] Capture scope under state read lock.
- [ ] Release lock before loader.
- [ ] Admit before first model/history selection.
- [ ] Check generation/config/session after load.
- [ ] Attach same Arc to primary/rebuild.
- [ ] Keep immutable role guidance separate.
- [ ] Reserve file tokens before trimming.
- [ ] Count file tokens once in calibration.
- [ ] Exclude file text from commit delta.
- [ ] Reuse snapshot after compaction.
- [ ] Run meaningful overlapping-session regressions.

Tests:

1. First request includes accepted identity/context.
2. Later tool turn uses exact same snapshot.
3. Old persona absent, host role remains.
4. No-file prompt preserved.
5. Active edit does not change current run.
6. Queued edit affects newly admitted run.
7. Steer does not reload files.
8. Concurrent sessions have independent snapshots.
9. Specialists cannot share profile text.
10. Helpers cannot inherit parent identity.
11. Delete during load prevents model request.
12. Halt during blocked read returns promptly.
13. Shutdown observes existing deadline.
14. Late result cannot publish.
15. Single stale retry bounded.
16. Second mismatch yields stale_scope.
17. No-workspace legacy run still works.
18. Blocked SOUL legacy fallback visible.
19. Blocked project contributes zero text.
20. Timeout does not accept partial identity.
21. No file body in transcript/history.
22. No file body in semantic memory.
23. No file body in canonical config.
24. No new RunRecord/snapshot schema field.
25. Token reserve reduces history predictably.
26. Current input/reply preserved.
27. File estimate counted exactly once.
28. Calibration not applied twice.
29. Saturating subtraction no underflow.
30. Project sheds before identity.
31. Tiny budget excludes unusable identity.
32. Marker tokens included.
33. Unicode estimate valid.
34. Secondary compaction has no file persona.
35. Durable summary does not copy file text.
36. Rebuilt primary keeps original Arc.
37. Edit during secondary does not affect rebuild.
38. Compaction failure preserves snapshot.
39. Compaction stop cannot install late identity.
40. Provider spy proves actual request behavior.

Do not rewrite run coordinator. New module owns orchestration; existing run/session
ownership and whole-turn delta invariants stay intact.

### Task 5: Current owner preview and SDK

Owned files:

- Create hosts/rust-daemon/src/routes/agent_files.rs.
- Create hosts/rust-daemon/src/routes/contracts/agent_files.rs.
- Create hosts/rust-daemon/src/routes/tests/agent_files.rs.
- Modify hosts/rust-daemon/src/routes/mod.rs registrations.
- Modify existing routes/agents.rs settings-validation hook.
- Create packages/sdk/src/agent-files.ts.
- Create packages/sdk/src/agent-files.spec.ts.
- Modify packages/sdk/src/daemon-types.ts.
- Modify packages/sdk/src/client.ts and index.ts.

Steps:

- [ ] Define closed content-free DTO.
- [ ] Use same loader under captured current scope.
- [ ] Add owner auth route and OpenAPI schema.
- [ ] Validate profile boolean only.
- [ ] Add SDK method/exports.
- [ ] Test auth/error/cancel/content absence.
- [ ] Run host route and SDK checks.

Tests:

1. Authenticated owner preview succeeds.
2. Missing auth rejected.
3. Wrong auth rejected.
4. Unknown agent 404.
5. Deleted agent cannot preview.
6. Current preview explicitly mode labelled.
7. capturedAt supplied from same load.
8. Manifest corresponds exact loaded bytes.
9. No claim it equals active run after edit.
10. DTO excludes source text and absolute root.
11. Blocked source only threat IDs.
12. Timeout only reason code.
13. Truncated counts/digest coherent.
14. Null/empty digest conventions preserved.
15. Closed enum OpenAPI schemas registered.
16. No arbitrary path/profile query.
17. Profile absent preserves existing config.
18. Profile false disables mode.
19. Profile true enables future runs only.
20. String path setting rejected.
21. Enabling creates no directory.
22. SDK safely encodes agent ID.
23. SDK AbortSignal propagated.
24. SDK error behavior follows client convention.
25. Exports available via package entry.
26. Service integration skipped with env flag.

Preview invokes same loader but does not modify any running snapshot. No reload endpoint,
trust database or manifest history store is needed.

### Task 6: Settings, docs and final acceptance

Owned files:

- Create apps/web/src/components/AgentFilesPanel.tsx.
- Create apps/web/src/components/AgentFilesPanel.test.tsx.
- Co-locate focused hook or use existing hooks convention.
- Modify apps/web/src/components/SettingsPanel.tsx insertion.
- Extend existing owner API fixture module.
- Modify existing styles only if required.
- Controller publishes final plan under docs/superpowers/plans.
- Controller updates companion handover.
- Controller updates hosts/rust-daemon/README.md.
- Controller edits appropriate existing operator docs.
- Do not create a docs app or settings redesign.

Steps:

- [ ] Add focused current-preview child panel.
- [ ] Guard late selected-agent responses.
- [ ] Show relative names/state/digests only.
- [ ] Explain preview versus fixed active run.
- [ ] Optionally expose boolean profile checkbox.
- [ ] Add accessible loading/empty/error states.
- [ ] Write setup/limits/examples with fake data.
- [ ] Complete batch reviews and one fix wave.
- [ ] Run required current-tree gates.
- [ ] Update handover evidence and M12 dependency.

Web tests:

1. Legacy identity label.
2. Root companion SOUL label.
3. Agent-profile SOUL label.
4. Relative derived path only.
5. Loading accessible.
6. Unavailable workspace clear.
7. Blocked reason no body.
8. Truncated count visible.
9. Shadowed source priority clear.
10. capture time and preview mode visible.
11. Next run refresh explanation.
12. Old daemon 404 compatibility handling.
13. Other error bounded retry.
14. Selection change cancels request.
15. Late old-agent response ignored.
16. No content/absolute root in DOM.
17. Checkbox default false.
18. Save sends boolean, no path.
19. Enable creates no files/session.
20. Existing settings focus/styles retained.
21. No new act warning or rejection.
22. Existing SettingsPanel tests pass.

Documentation checklist:

- Root companion SOUL placement.
- Derived optional profile placement.
- UTF-8 standard Markdown.
- No automatic creation/migration.
- AGENTS portable default.
- Verified Hermes priority/shadowing.
- Override and chain limits.
- CLAUDE/cursorrules fallback.
- Deferred cursor glob.
- USER/MEMORY future existing authority.
- Standard names, not full parity.
- No ambient home/cwd.
- Active run immutable.
- Queued run admission refresh.
- Current preview versus actual prompt.
- File body not transcript plumbing.
- Selected provider receives prompt text.
- Read/scan/render/deadline limits.
- Filter misses/false positives.
- Filename never attests authorship.
- Exact owner instruction-write approval.
- Full OS process capability limitation.
- Upload namespace excluded from discovery.
- Original attachment display name retained.
- Authenticated owner editor authority.
- Local deterministic versus live evidence.
- M12 dependency, no connector implementation.

Use invented agent IDs and content. Verify hex path example from fake ID. Never copy private
local instruction bytes, tokens or Hermes config into docs.

## Execution and review

Order: Task 1 contract, Task 2 loader. Task 3 independent approval/upload ownership after
names freeze. Task 4 consumes accepted contracts. Task 5 API/SDK then Task 6 UI/docs.

First daemon batch review covers Tasks 1–3. Second covers runtime and host API. One final
SDK/web review, one fix wave, final gates. No nested drafting agents or additional audit
cycle.

Workers report files/tests/uncertainty to controller. Shared file coordination through
controller. Reviews require concrete trigger, file/line and impact; do not expand milestone
into generic cleanup. Controller binding rulings override conflicting draft details.

Nx environment:

```powershell
$env:CARGO_INCREMENTAL='0'
$env:ANIMA_SKIP_SERVICE_TESTS='1'
$env:NX_DAEMON='false'
$env:NX_ISOLATE_PLUGINS='false'
$env:NX_NO_CLOUD='true'
$env:NX_NATIVE_FILE_CACHE_DIRECTORY=Join-Path (Get-Location) 'node_modules/.cache/nx-native-m11'
bun x nx show projects --json
bun x nx show project rust-daemon --json
bun x nx show project '@animaOS-SWARM/sdk' --json
bun x nx show project '@animaOS-SWARM/web' --json
```

Confirm actual resolved targets before tests. Initial inventory failed existing native
dependency loading. Established escalation may solve sandbox/native cache access; do not
install or reset dependencies. Focused filters require existing target/help verification,
not guessed flags.

Final gates:

```powershell
$env:CARGO_INCREMENTAL='0'
$env:ANIMA_SKIP_SERVICE_TESTS='1'
bun x nx run rust-daemon:test --skipNxCache
bun x nx run '@animaOS-SWARM/sdk:test' --skipNxCache
bun x nx run '@animaOS-SWARM/sdk:typecheck' --skipNxCache
bun x nx run '@animaOS-SWARM/sdk:build' --skipNxCache
bun x nx run '@animaOS-SWARM/web:test' --skipNxCache
bun x nx run '@animaOS-SWARM/web:typecheck' --skipNxCache
bun x nx run '@animaOS-SWARM/web:build' --skipNxCache
```

Names reflect M10 gates but require current inventory. Report dependency tasks actually
executed, not imaginary independent checks. Service integration remains skipped. Existing
ignored Postgres suite is not run. No default/live E2E; use only existing offline browser
fixture if needed.

Formatting: cargo fmt --all --check under existing Rust workspace with CARGO_INCREMENTAL=0;
format owned sources only. Nx format check explicit changed TS/TSX/Markdown paths after
help/config verification. No unrelated formatter staging. Docs-app source changes require
relevant docs build --skipNxCache; README/handover need textual/format review.

Logs under ignored milestone directory. Report aggregate counts and exact platform/service
exclusions. No success until current-tree required gates pass. Prompt spy proves
composition, not provider obedience, injection immunity or deployment readiness.

Controller owns explicit stage/unsigned commit via git -c commit.gpgsign=false commit; no
global config change. No automatic merge without owner instruction. Draft writer creates no
PR.

## Exactly five risks for binding rulings

### 1. Primary identity without losing engine role

Requested ruling: SOUL replaces legacy persona only in isolated runtime; immutable host
helper/delegation/origin remains. Missing/blocked SOUL complete legacy fallback. No file
body writes to canonical config.

Why: Append-only SOUL leaves conflicting bio/system persona. Removing mixed host guidance
silently loses runtime constraints.

Cost if wrong: Config.system policy users may need documented separation; never parse
arbitrary persona prose to guess authority.

### 2. Scope, provenance and attachment promotion

Requested ruling: Derived hex-ID profile, root companion-only fallback, no ambient
home/cwd/chat paths. Verified Hermes priority. Existing uploads/date namespace excluded from
loader; no name-based attachment promotion. All sources filtered without inferred owner
authorship.

Why: Shared root/persona and name-derived attachment paths could promote untrusted content.
Private namespace alone proves no author.

Cost if wrong: Existing filenames may shadow AGENTS unexpectedly; manifest and docs expose
actual selection. No trust/schema infrastructure added.

### 3. Blocking reads and late acceptance

Requested ruling: Capability-relative nofollow reads, five-second total deadline, two
retained worker permits, caller-only publication and one stale retry. Active/compaction
snapshot fixed; queued admission refreshes.

Why: spawn_blocking cannot cancel kernel read; locks/late publication can block halt or
revive stale scope.

Cost if wrong: Stuck workers consume permits and later files fall back. Honest limit
preferable to unbounded threads; physical thread kill is outside guarantee.

### 4. Protected writes and trusted OS limit

Requested ruling: Exact owner gate overrides reusable allow rules for protected file writes;
no-gate denies; canonical aliases/post-wait drift checked. Existing shell remains trusted
full OS, explicit limit documented, no blanket new prompts.

Why: Optional approval and path aliases bypass naive guards. Shell effects cannot be
comprehensively inferred from names.

Cost if wrong: More file decisions and stricter direct/swarm writes; approved full OS
execution can still alter instructions. Do not claim sandbox protection.

### 5. Budget, compaction and truthful visibility

Requested ruling: Reserve accepted text before trim, count once, reuse after compaction, no
body persistence. Owner API current preview uses same loader/capture time and does not claim
historical active snapshot.

Why: Late-added text overflows context; reread manifests diverge; storing text creates new
authority/privacy leak.

Cost if wrong: Small budgets exclude useful files. Existing legacy prompt size limit
remains; visible states and actual request tests keep claims honest.

## Controller completion checklist

- [ ] Bind the five decisions.
- [ ] Resolve actual Nx inventory.
- [ ] Freeze exact interfaces before workers.
- [ ] Publish final plan.
- [ ] Execute six bounded tasks.
- [ ] Complete reviews and one fix wave.
- [ ] Pass current-tree relevant gates.
- [ ] Retain complete MIT/source notices.
- [ ] Update handover evidence/M12 dependency.
- [ ] Controller stages, commits and publishes.

## Controller rulings on the risks (binding)

1. Ruling: Accepted SOUL replaces all legacy persona, including config.system, only in a transient run instance; knowledge and separately supplied immutable host role/delegation/origin remain — appending competing identities defeats primary SOUL semantics; arbitrary prose cannot be parsed safely into policy — cost if wrong: owners using config.system as policy must move mandatory behavior into host policy; no canonical config rewrite occurs.
2. Ruling: Read only configured workspace roots and optional hex-ID profiles, with companion-only root SOUL fallback; uploads/date never participates; all sources are filtered without inferred authorship — source names and private folders do not attest trust — cost if wrong: stricter SOUL filtering and usable-candidate fallback differ from Hermes. Missing, empty, unreadable or blocked priority candidates fall through visibly; cursor glob, USER and MEMORY loading remain deferred. Bounded 70/30 head/tail rendering after marker reserve deliberately differs from Hermes 70/20 allocation.
3. Ruling: Five seconds includes worker admission and a single scope retry; two worker permits remain held until actual worker completion; use capability-relative nofollow reads and caller-only acceptance — cancelled blocking reads cannot safely be killed and must never publish late — cost if wrong: stuck workers exhaust permits and later loads fall back, without promising an atomic filesystem snapshot or thread termination.
4. Ruling: Protected file tools always require an exact owner decision despite reusable allows, honor deny rules, bind revisions and revalidate after waiting; no-gate execution denies — names, aliases and stale approval can otherwise bypass instruction ownership — cost if wrong: extra owner decisions and narrower direct/swarm writes. Existing approved shell remains trusted full OS execution and can modify files; this is not a sandbox guarantee.
5. Ruling: Reserve accepted file tokens before history selection, count them once, reuse one snapshot after compaction, and persist no source bodies; expose content-free current_preview from the same loader — late additions overflow context and rereads misrepresent active instructions — cost if wrong: small budgets exclude useful files and current preview can differ from active runs; provider prompt capture and visible states establish the bounded behavior.

Execution authorization: the owner explicitly requested this Rust port now and authorized Codex subagents. Their cheap batch-review/one-fix-wave workflow supersedes per-task skill review loops. No worktrees, extra approval pauses, new dependencies or services. Six tasks are scoped implementation rather than full Hermes parity; meaningful behavioral regressions may consolidate the draft's overlapping test lists.
