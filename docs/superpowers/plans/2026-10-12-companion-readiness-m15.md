# Final selected daily-assistant readiness (M15)

Activated after M14 merged; one fresh branch from updated main. Scope is documentation/evidence reconciliation, not more connector breadth or upstream parity.

## Accepted capability map

Daily use is implemented: onboarding/provider selection; sessions/history/stream/Stop; AGENTS.md/SOUL.md/ANIMA.md workspace instructions; memory edits/recall; workspace tools/approvals; attachments/browser speech; durable schedules/tasks/restart review; Telegram/Discord plus Gmail/Calendar/selected Drive. Evidence paths are the existing OnboardingFlow, SessionView, AgentFilesPanel, WorkspaceFiles, ApprovalsPage, DictationControl and AutomationsPage tests, and daemon agent_runs/{queue_tests,approval_tests,agent_files_tests,attachment_tests}, routes/tests/memory_edits, schedules/run_now_tests, connectors/{runtime/tests,discord/runtime_tests,mail/tests,gcalendar/tests,drive/manager_tests}.

No additional implementation blocker was found after the two confirmed Calendar read guards included in M14's current combined fix wave. Custom web/connector tool grants need to be preserved: selecting a standard profile replaces them by explicit UI warning. Browser web_fetch and optional configured EXA search are not interactive browser/OS control. Identity preview is read-only.

## Tasks

1. Activate branch from merged M14 main; root append top-five binding risks, no preflight audit.
2. Write one Anima daily-use setup guide with real model, persistent store/workspace, identity files, chat/memory/approvals, one schedule and selected connectors. Link existing exact technical setup rather than copy all commands/secrets.
3. Add practical future live acceptance checklist: actual provider response; memory/file/conversation after restart; approved external action; connector receive/send; selected Drive success and unselected refusal; unknown-effect review; stopped backup/offsite restore.
4. Record existing fresh combined gate evidence, explicit skipped live checks and current limits. No duplicate full tests for docs-only source; owned Markdown check and local link validation only.
5. One concise docs review/fix if needed, explicit commit/push/PR attach/verified merge under standing authority. Finish main clean except unrelated nx.json, precise current handover/resume and next authorized operator steps.

## Top five risks

1. Configured versus verified: deterministic fixtures do not establish real inference/OAuth eligibility/accounts/microphone/hosting.
2. External effects: Telegram at-least-once, Discord/mail unknown outcomes and interrupted tools require review, not seamless automatic resume.
3. Persistence: consistent control plane/history/memory/uploads/workspace/keyring plus stopped backup; current-image/VPS reboot/offsite restore acceptance unexecuted.
4. Authority: credentials alone are not tool grants; broader Google readonly consent differs from native selected IDs; instruction writes need exact approval.
5. Capability limits: selected Discord text REST, selected Docs/plaintext Drive reads, browser-dependent voice and explicitly granted webtools; no dynamic module installer or broad integrations.

## Controller rulings on the risks (binding)

M14 PR20 is merged at37ffb48f936cba419ccae2a655537a128e31c568, source51d7e8a.
M15 starts on g9000/companion-readiness-m15 from that updated main. A Codex subagent
prepared the lean readiness map and five risks; no separate preflight audit.

1. Ruling: Close the selected feature build with an implementation/evidence guide, explicitly separate unexecuted live acceptance — no more concrete selected-flow code blocker was found after the M14 Calendar guards — cost if wrong: real provider/account issues remain discoverable in the owner checklist instead of being called tested.
2. Ruling: Keep uncertain external effects visible and require inspection before retry — stopping or restarting cannot undo a send/tool side effect — cost if wrong: the owner may need manual reconciliation; no seamless or exactly-once promise.
3. Ruling: Document one consistent persistent store/workspace/vault and stopped backup generation — durable identity does not require an uninterrupted model process — cost if wrong: incomplete storage/operator setup cannot establish continuity; current VPS acceptance remains pending.
4. Ruling: Explain explicit tool grants, protected Markdown writes and provider-versus-native Drive scope — a connected credential or pasted ID is not authority — cost if wrong: an owner must adjust deliberate grants/consent rather than believing all tools are available by default.
5. Ruling: Retain selected Discord text, selected Docs/plaintext Drive and browser speech limits; trusted compile-in contribution only — useful personal integration is the accepted scope, not breadth parity or a new installer — cost if wrong: unsupported needs require a later separately scoped milestone.

Verification: owned Markdown formatting/local links and source-truth review only;
M15 changes no runtime/test/manifests/deployment configuration. Reuse fresh M14
Rust2298/8ignored, web1187, SDK127/3service-skipped, typecheck/build/lint/format
and deterministic connector/identity/memory/approval/schedule evidence. Do not
launch any services or repeat full source gates for documentation-only edits.

Outcome: setup/evidence guide written; one concise source-truth review corrected Google app navigation and proposal review locations. Existing daemon README setup copy is corrected too; API inventory remains158 operations. M15 remains documentation-only, with no runtime/test/manifests/deployment changes. Owned formatting/local links and final publication follow.

Final documentation checks pass: owned Markdown formatting, local targets and all
setup-guide anchors; API inventory remains158. Root Quick Start links the guide.
No further runtime source change or broad parity milestone is required for the
selected target. Publish and merge the verified docs-only head under standing
authority, then resume only the guide's authorized operator/account acceptance.
