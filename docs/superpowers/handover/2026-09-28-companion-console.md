# Handover: companion console build (M0-M13 merged; M14 active)

## Start here (2026-10-12): selected Drive, then daily-assistant readiness

- Owner standing instruction remains: merge and continue until usable like the reference assistant, reuse/copy actual upstream code into Rust, keep Anima naming. Gated selected-use milestones may merge without another approval question. No dependencies/features, real daemons/services/providers/keyring/profile reads/databases/containers/port55432, destructive Git operations or worktrees. Explicit staging never nx.json/anima.yaml; unsigned per-command override is already authorized.
- PR #19 merged at `7f42df8334af7e0afdb71f83a5d52aeaa48e6d67` with verified source `0a5c74c`. M0-M13 are on main. M13 fresh gates: Rust2268/8ignored(core630/host1638), SDK125/3service-skipped, web1158/107files; SDK/webtypechecks/builds, Rustlint and ownedformat pass. Thirty-six native cases +startupbackup regressions pass; combined review fixes complete. Never repeat finished M13 work/gates absent new changes.
- Active branch `g9000/companion-drive-m14` is cut from that updated main; only unrelated nx.json dirty at activation. Plan `docs/superpowers/plans/2026-10-12-companion-drive-m14.md` contains interfaces, source/protocol verification, five binding risks and controller freeze. Ledger/rules under `.superpowers/sdd/2026-10-12-companion-drive-m14/`.
- M14 implements immutable selected1-16 Drive file IDs, selected filename search via exact metadata GET and bounded GoogleDocs/plaintext reads. Use existing Google app/PKCE/native authority patterns; no global Drive enumeration, writes/share/delete, auto-ingestion, document engine or Picker. Google drive.readonly consent is broader than Anima's allowlist and must be disclosed honestly. Local disconnect must never remotely revoke all project OAuth scopes (Gmail/Calendar impact).
- Actual reusable upstream code: `skills/productivity/google-workspace/scripts/google_api.py::drive_get/drive_download` at read-only local source C:/Users/leoca/AppData/Local/hermes/hermes-agent, pinned pristine revision f3ef9db68332063a3625661f3a9b259f60abdb4e, MIT Nous Research2025. Adapt selected HTTP/format behavior, retain full notice. drive_preview is desktop browser control, not Google Drive. No Python/gws execution, profiles/config/token files or user secrets.
- Native Drive tokens/generations stay separate from working Gmail/Calendar. Source IDs never grant permissions; every read rechecks current agent, connector, selected IDs/account/generation/tool access after waits. Callbacks/refreshes cannot resurrect a disconnect. Managers retain weak daemon references to avoid cycles. Snapshot13 needs native records and v12 pre-Drive backup; old fixtures use current-version constants already updated in M13.
- Next: delegated A nativeclient/model/store/provenance, B manager/state/schema/routes/tools/runtime, C SDK/web selected-file card. Root owns docs/master/handover; one daemon batch and final web/SDK review, one combined fix wave, relevant fresh local gates, explicit commit/push/PR attach/verified merge; then final deterministic daily-flow acceptance/setup. Live account/provider/hosting tests remain unexecuted under no-services constraint; no broad reference-parity claim.

## Historical M13 publication and standing continuation

# Handover: companion console build (M0–M12 merged; M13 active)

## Start here (2026-10-12): continue through the usable assistant target

- Latest owner instruction: "just merge and continue please until we come to stage of usuable similar to the hermes agent. just copy their code."
  This grants standing merge-and-continue authorization for the selected daily-use milestones after their reviews and local gates. Do not repeat merge/check-in questions. Preserve the existing no-services, no-dependencies, explicit-staging and unrelated-edit constraints.
- PR #18 merged at `f7a58084bc6a36273f528882463b4f516ef96bb1`; M0–M12 are on main. Its tested source `76011cd` and publication `8dc1635` are included.
- Active branch `g9000/companion-discord-m13` is cut from that updated main; only unrelated `nx.json` was dirty at activation. Never stage it or `anima.yaml`.
- M13 delivers selected-channel Discord REST chat through the portable Rust contract, with owner-configured sender access, bounded catch-up and durable admission/delivery/recovery. REST limitations are explicit; no Gateway/voice/slash-command parity claim. Tracked plan `docs/superpowers/plans/2026-10-12-companion-discord-m13.md` contains binding risks/interfaces and verified execution outcome.
- M13 implementation/reviews/combined fix wave are complete. Fresh gates: Rust2268/8ignored (core630,host1638), SDK125/3service-skipped, web1158/107files; SDK/webtypecheck/build, Rustlint and ownedformatting pass. Logs under `.superpowers/sdd/2026-10-12-companion-discord-m13/`: `gate-rust-final.log`, `gate-lint.log`, `gate-web-sdk.log`, `gate-format.log`. Old first Rust gate failed stale v11 fixture assertions; full corrected rerun passes. Thirty-six native/registry/route cases and startup backup coverage pass. Web warning origins remain baseline; unused Rust wrappers are fixture entrypoints.
- Native snapshot version12 has version11 .pre-discord.bak backup and nativevalidation. Unknown delivery never auto-replays; owner review suppression sends nothing. Stop during Sending suppresses remaining tail while retaining the current receipt. Terminal poll errors pause for explicit recovery. Initial activation baseline only; restart cursor retained. Owner routes/APIinventory154operations; no manifest/feature/lock changes.
- Next concrete action: explicit-file commit/push/create+attach+merge M13 at its verified head under standing authorization, fast-forward main, cut g9000/companion-drive-m14 and activate its prepared plan. No repeated approval pause. M14 delivers immutable 1-16 selected Drive file IDs, filename search and GoogleDocs/plaintext reads; existing Gmail/Calendar stay intact. Prepared draft: `.superpowers/sdd/2026-10-12-companion-drive-m14/plan-draft.md`. Local disconnect must not revoke all Google project scopes. Actual account/live service readiness remains separately unexecuted.
- Reference source is read-only `C:/Users/leoca/AppData/Local/hermes/hermes-agent`, revision `f3ef9db68332063a3625661f3a9b259f60abdb4e`, MIT Nous Research 2025. Discord code is `plugins/platforms/discord/adapter.py` and `recovery.py`; do not use the old guessed gateway path. Port selected actual code/behavior into Rust, record scope/revision and retain notices. No profiles/config/environment secrets or Python runtime execution.
- Product names stay Anima/AnimaOS. Upstream names remain only for accurate attribution/source evidence and historical identifiers.
- Usable target: provider-connected web/Telegram/Discord chat; Markdown identity/project instructions; workspace/tool access; remembered context; durable schedules/tasks with visible uncertain recovery; existing Gmail/Calendar plus selected Drive read/search. Make source contribution boundaries honest. Assess real code gaps rather than claiming broad reference parity. Account consent, live provider/channel delivery and always-on deployment remain separate live acceptance when the owner permits services.
- Controller uses one branch per milestone from merged main, lean plan plus five binding risks, delegated implementation, one daemon batch and final web/SDK review, one fix wave and relevant local gates. Keep cargo incremental disabled and compile serialized; no new target directory, dependencies/features, daemons, databases, containers or Docker Postgres55432 actions.
- Commits use the already-authorized per-command unsigned override. Do not pause for GPG. Finish each milestone with explicit-path commit/push/PR attachment, then merge its verified head under standing authorization and continue from updated main. Preserve exact resume/evidence in this handover and the ignored milestone ledger.

## Historical M12 publication before the authorized merge

# Handover: companion console build (M0–M11 merged; M12 ready)

## Start here (2026-10-12): M12 modular connector foundation ready

- Owner explicitly instructed merge and continue. PR #17 merged at
  `37218944488001caa4e34ae7f6fa13cde1f36d6b`, including Anima naming correction
  `21120aa`. M0–M11 are on main; no repeated M11 review/gates needed.
- Active branch `g9000/companion-connectors-m12` was cut from that updated main.
  Unrelated `nx.json` remains untouched/unstaged; never stage it or `anima.yaml`.
- Product naming remains Anima/AnimaOS. ANIMA.md/.anima.md and anima source kinds
  are merged; upstream names only belong in accurate attribution/source evidence
  and retained historical identifiers.
- Executable lean plan: `docs/superpowers/plans/2026-10-12-companion-connectors-m12.md`.
  Five binding controller risk rulings are supplemented by post-merge interface
  decisions. Ledger/rules: `.superpowers/sdd/2026-10-12-companion-connectors-m12/`.
- M12 implements pure Rust connector descriptors/metadata registry and async
  transport contract, then the existing Telegram adapter/registry pilot. Associated
  native payload/error types preserve Telegram metadata and uncertainty. Private
  per-dispatch adapters borrow the exact already-captured token; core requests and
  metadata contain no secrets, agent/session/provider authority or prompt paths.
- Preserve existing pairing, vault generations, durable admission/outbox, lifecycle,
  delivery attempt claims, recovery and stop behavior. Registry discovery is passive;
  community contributions are trusted compile-in source with host privileges.
  Installable ABI/sidecars/WASM and new Discord/Drive providers remain deferred.
  Existing owner API/SDK/UI do not gain installer or activation claims.
- Implementation and contribution guide are complete. One daemon batch review and
  final API/SDK/web/docs review found no actionable issues; the single fix wave
  reconciled the host trait aliases and formatting. Public core trait remains
  `ConnectorTransport`; host imports alias it as `AsyncConnectorTransport`.
- Fresh local Nx host test gate passed, including its core dependency:
  **2,230 Rust tests passed** (core 630, host 1,600), **8 ignored** (five Postgres,
  one live subscription smoke, one model-download test and one inventory printer).
  All six new contract/adapter tests pass, with existing lifecycle/claim/recovery
  fixtures traversing the new seam. Rust lint (`cargo fmt --all --check`) passes.
  Logs: `.superpowers/sdd/2026-10-12-companion-connectors-m12/gate-rust.log` and
  `gate-lint.log`. Explicit owned Markdown formatting is checked at publication.
- HTTP owner API, SDK/web sources, snapshot version 11 and dependency manifests
  are unchanged. TypeScript and Astro gates are conditional and were not repeated
  for unchanged sources. Local fakes establish compatibility, not live delivery.
- Contribution instructions: `hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md`.
  Portable text bounds are 1,048,576 Unicode scalars; excessive requests fail
  before transport, with a static native error. `attempt_id` is host correlation,
  not a provider deduplication promise. No new lifecycle or durable schema.
- Source is committed and pushed at `76011cd`; [PR #18](https://github.com/G9000/animaOS-SWARM/pull/18)
  is open against main and attached to the chat. Final publication documentation
  follows that source commit. Only unrelated `nx.json` remains dirty.
- Resume: await explicit owner PR #18 merge authorization. After that merge,
  cut the next milestone branch from updated main. The next bounded scope is
  Discord REST chat; Gmail/Calendar contract migration and selected Drive read/search
  follow separately. Do not create another milestone branch from unmerged M12.
- No new dependencies/features, services/daemons/providers/keyring reads/databases/
  containers or Docker Postgres55432 actions. Prefix cargo with CARGO_INCREMENTAL=0;
  serialize compilation, keep existing target dirs. Explicit file staging only;
  no worktree/stash/reset/restore/checkout-path. Unsigned per-command commit override
  persists; do not ask for GPG unlock or change global signing config.

## Historical M11 publication before the authorized merge

# Handover: companion console build (M0–M10 merged; M11 ready)

This document is for the next agent picking up the companion console upgrade.

## Start here (2026-10-12): M11 Anima Markdown definitions; PR #17 open

- Owner naming rule: our assistant is Anima/AnimaOS. Use ANIMA.md/.anima.md alongside AGENTS.md/SOUL.md; source kinds and UI use Anima. Upstream Hermes names belong in provenance/license credits only, with historical branch/ledger identifiers retained for continuity. Naming correction is verified: Rust2224/8ignored, web1133, SDK123/3service-skipped; SDK/web typecheck/build and formatting pass. This correction is committed and pushed on PR #17 before stopping. The owner naming rule is persistent for future work.

- M0–M10 are on main. Owner explicitly authorized PR #16 merge; `f80e653`
  includes final M10 head `eee9bb8`. Do not merge another PR without a new explicit
  owner instruction.
- Owner clarified Markdown defines agents and instructed reuse of installed
  Hermes code/behavior ported to an independent Rust long-running runtime. M11
  implements the focused AGENTS/SOUL port on `g9000/hermes-agent-files-m11`, cut
  from merged main `f80e653`. Implementation `5c7c496` is committed and pushed; [PR #17](https://github.com/G9000/animaOS-SWARM/pull/17) is OPEN against main and attached to the chat. Final publication documentation follows that source commit.
- Published lean plan: `docs/superpowers/plans/2026-10-12-anima-agent-files-m11.md`.
  Controller binding five-risk rulings, two daemon batch reviews, final web/SDK
  review and one fix wave are complete. A1 hard-link bypass and B1 summary reserve/
  B2 compaction workspace binding are fixed. A2 path-deny finding was rejected:
  actual policy denies risk classes, while rules/allowances grant allows only.
- Portable core renderer/transient identity lives in anima-core; safe host file
  loader, selected threat-pattern port, manifest, protected writers and runtime
  admission live in focused daemon modules. SOUL replaces legacy persona while
  preserving knowledge and immutable host role/delegation/origin. Runs keep one
  snapshot through tools, steers and compaction; later runs read current files.
  Configuration remains frozen at durable start. No durable schema/version change.
- File scope is configured workspace only, with companion-only root SOUL fallback
  or optional hex-ID profile. No ambient home/cwd/Hermes profiles, uploaded-file
  promotion, automatic migration, new memory store or Python runtime. Enabling
  `settings.agentFilesProfileEnabled` creates no files; it uses existing agent
  creation/workspace configuration, with no new Settings PATCH/console toggle.
- Current Settings preview and owner API show content-free metadata with explicit
  `current_preview`, capture time and digest; they do not claim active-run history.
  Model instruction writes demand exact owner decisions, create no reusable grants,
  recheck revisions, and refuse multiply hard-linked targets. Approved shell remains
  trusted full OS execution; no injection-immunity/sandbox guarantee is claimed.
- Hermes source revision verified: `f3ef9db68332063a3625661f3a9b259f60abdb4e`.
  MIT copyright/provenance retained in `hosts/rust-daemon/HERMES-NOTICE` and
  `packages/core-rust/crates/anima-core/HERMES-NOTICE`. Focused pattern/source
  behavior is adapted; differences and limits are documented in daemon README.
- Fresh local gates on final functional sources: Rust **2,224 passed, 8 ignored**
  (core dependency 627; host 1,597), web **1,133 passed**, SDK **123 passed, 3 skipped**.
  SDK/web typechecks/builds pass. Cargo fmt and explicit owned-file Nx format checks
  pass. Logs are ignored under `.superpowers/sdd/2026-10-12-hermes-agent-files-m11/`:
  `gate-naming-rust.log`, `gate-naming-web-sdk.log`, with earlier evidence in
  `gate-web-sdk.log`, plus formatter logs. The seven Postgres tests and one
  manual inventory-print helper remain ignored; SDK service tests are explicitly
  skipped. Existing web warning origins remain Operations/Dashboard/unknown
  connection; focused preview tests have no new act/unhandled warnings.
- No real daemon, dev server, database, container, live provider, microphone or
  Hermes runtime/profile/credential access during M11. Tests use fake workspaces,
  provider captures and in-process routes. Windows junction/hard-link boundaries
  and cancellation/compaction fixtures pass; Unix-only cases remain unexecuted.
  Browser/live deployment/restore acceptance was not rerun and is not implied.
- M12 connector preparation is now tracked and reviewable at
  `docs/superpowers/plans/2026-10-12-companion-connectors-m12.md`, with binding
  controller five-risk rulings. No connector source is implemented. After explicit
  M11 merge, create `g9000/companion-connectors-m12` from updated main only when
  implementation is authorized. Scope foundation/trusted compile-in Rust traits
  and registry plus existing Telegram pilot; later Discord, Gmail/Calendar reuse
  and selected Drive reads follow separately verified plans. Installable ABI is
  deferred; do not promise sandboxed/drop-in community modules.
- Unsigned per-command controller commits remain owner-authorized. Never request
  GPG unlock or change global signing config. Stage explicit files only; preserve
  unrelated `nx.json`, never stage it or `anima.yaml`. Continue no dependencies/
  features, services, Docker Postgres55432, worktree/stash/reset/restore/checkout-path.
- Resume: await explicit owner authorization to merge PR #17. All tracked M11 work is committed and pushed. Do not repeat implementation,
  reviews or passing gates without new changes/failures. Ledger/rules retain exact
  decisions at `.superpowers/sdd/2026-10-12-hermes-agent-files-m11/`.

The following section retains M10 publication evidence.

## Historical M10 publication (2026-10-12): PR #16 was open; now merged

- M0–M9 are merged to main. Owner explicitly authorized M9 PR #15 merge;
  merge commit `647d606` includes final M9 handover `c9909df`.
- Current branch `feat/companion-console-m10` was cut from that merged main.
  Implementation is committed through `f2d0814`, formatting/docs through `af2b5f8`, with web `9c3b298` and core
  `bfc4d87`. [PR #16](https://github.com/G9000/animaOS-SWARM/pull/16) is OPEN against main and attached to the chat. All tracked M10 work is committed and pushed; do not repeat
  implementation or reviews. **Resume: review/merge PR #16 only after a new explicit owner merge instruction.** Current local gates are finished. Live deployment/Unix signal/provider/restore acceptance is a separately authorized follow-up; do not start services under the existing constraint.
- Published lean plan: `docs/superpowers/plans/2026-09-23-companion-console-m10.md`.
  Binding five-risk rulings plus the evidence-driven shared 40-second shutdown
  refinement are recorded. Codex implementers, both daemon batches, final web/SDK
  review and one fix wave are finished. A1 fixed job/schedule claim admission after
  producer closure; four deterministic regressions preserve already admitted work.
- Snapshot remains **v11**. Delivery claims are ephemeral and fenced by committed
  credential/row identity. Telegram transport holds no global transaction/state
  guard, and uncertain delivery remains at-least-once. Session search/snippet work
  runs after capturing selected state outside the read lock. Module/test extraction
  preserves the original 60 connector and 59 inline agent-run test names.
- Fresh functional gates on the final sources:
  - Rust Nx host test **2,186 passed, 8 ignored**: core dependency 621 and host
    1,565. Seven ignored tests require Postgres; one is an explicit README inventory
    print helper, not a required correctness test. Both Nx tasks passed.
  - Web **1,124 passed**; SDK **117 passed, 3 skipped** with
    `ANIMA_SKIP_SERVICE_TESTS=1`. All SDK/web test/typecheck/build dependency tasks passed.
  - Offline installed-Edge browser **16 passed**, browser typecheck passed;
    deployment contracts **4 passed**; dummy Compose `config --quiet` passed.
  - Functional logs: ignored `.superpowers/sdd/2026-09-23-companion-console-m10/`
    `gate-rust.log`, `gate-web-sdk.log`, `gate-browser-deploy.log`. Full web warnings
    have only test origins already present in M9 logs; focused 122 tests were clean.
    Existing chunk-size warnings remain. Cargo fmt and Nx format:check against origin/main both pass; formatter-only source adjustments are verified and the quote-independent deployment contracts/parser rerun passes.
- No real daemon, dev server, database, container, provider or microphone started
  during M10. Browser fixtures load built assets, normal SDK/SSE bytes and reducers;
  they do not prove real HTTP chunking, TLS, keyring/container restart or live
  providers. Current Unix signal/VPS/restore acceptance remains unexecuted under
  the no-service rule. `deploy/vps/README.md` scopes old 2026-09-17 smoke evidence
  and documents the complete stopped five-volume/env backup generation.
- Owner authorized **unsigned commits** on 2026-10-12 to avoid GPG while absent.
  This overrides every historical signing/pinentry instruction below. Controller
  uses per-command `git -c commit.gpgsign=false commit`, never global config changes.
- Preserve unrelated `nx.json`; never stage it or `anima.yaml`. Continue all
  no-dependency/no-service/explicit-path/no-worktree/stash/reset/restore rules.
  Ledger and worker notes: `.superpowers/sdd/2026-09-23-companion-console-m10/`.
- Hermes focused provider formats/MIME behavior are ported with MIT provenance in
  `packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE` (revision
  `f3ef9db68332063a3625661f3a9b259f60abdb4e`). Scoped recovery/orderly stop inform the
  independent Rust runtime. Telegram/Discord/Gmail/Calendar/Drive priorities and
  community connector lifecycle remain follow-ups; Discord/Drive/community lifecycle
  are not shipped here. Automatic session-record TTL awaits a product policy.

The following sections retain historical M9 evidence.

## Historical state (2026-10-11): M9 PR #15 open; M10 plan prepared

**State**

- M0–M8 are merged to `main`. The last merges were #12 (M6), #13 (M7) and #14 (M8); `main` is at `86cc87f`.
- M9 (attachments and browser voice) implementation is complete on `feat/companion-console-m9`, cut from `main`. Lean plan: `docs/superpowers/plans/2026-09-23-companion-console-m9.md`; the owner authorized Codex subagents because Claude was signed out. Both daemon batch reviews, the final web/SDK review and one fix wave are finished. The full Rust gate caught old snapshot expectations; test-only corrections preserved historical migration coverage and passed the rerun.
- M9 is signed and pushed through `9efe2d6`. [PR #15](https://github.com/G9000/animaOS-SWARM/pull/15) is OPEN against `main` and attached to the Codex chat; it is not merged. Both waiting daemon commits finished, and the latest implementation/documentation signatures were verified. Only unrelated `nx.json` remains locally modified.
- The working tree uses snapshot **v11** with `.pre-attachments.bak` / `control_plane.backup.10`; merged `main` still uses v10.
- Hermes image routing/provider formats were used as Rust porting references. `packages/core-rust/crates/anima-model-adapters/HERMES-NOTICE` retains the upstream MIT notice and revision `f3ef9db68332063a3625661f3a9b259f60abdb4e`. The owner clarified “remember” means reuse code/behavior, not a new memory feature.
- The lean M10 plan draft (10 tasks, about 1,200 lines) and controller top-five binding risk rulings are ready in the ignored `.superpowers/sdd/2026-09-23-companion-console-m10/plan-draft.md`; its ledger records the precise resume point. No M10 branch or tracked implementation starts before the owner explicitly merges M9.
- Compatibility: new v11 consumed steer keys preserve full input after transcript pruning; older keys require a matching scoped transcript to prove equality, otherwise replay returns 409.
- Fresh local gates on this working tree:
  - `rust-daemon:test`: **2,156 passed, 7 ignored** in aggregate: host 1,542 plus its `core-rust:test` dependency 614. The ignored tests require Postgres.
  - SDK/web test, typecheck and build: **web 1,117 passed; SDK 117 passed, 3 skipped**. Set `ANIMA_SKIP_SERVICE_TESTS=1`; those three SDK tests start a real service and are excluded under the owner's rule.
  - `cargo fmt --all --check`: passed after a controller-only test-module ordering correction. Nx format:check against origin/main also passed.
  - Logs: `.superpowers/sdd/2026-09-23-companion-console-m9/gate-rust-rerun.log` and `gate-web-sdk.log` (ignored local working files). Full web output includes act warnings from the unchanged `WorkspaceOperations` removed-filter test; the touched 312-test focused set was clean. Source matches `origin/main` and its API methods are mocked.
- Evidence limits: no live-provider image acceptance, real microphone/TTS, Unix link fixtures, Postgres execution or VPS deployment. One earlier SDK run accidentally started its existing temporary daemon fixture; it cleaned up, and subsequent controller process checks found no daemon. The opt-out now prevents recurrence.

**Resume M10 after explicit M9 merge**

1. Do not repeat M9 implementation or reviews. PR #15 is ready for the owner’s merge instruction; never infer merge approval from “continue” or “go.”
2. After the owner explicitly says “merge,” merge PR #15, update local `main`, and cut `feat/companion-console-m10` from that merged main. Preserve the unrelated `nx.json` edit; no stash/reset/restore/worktree operations.
3. Copy the prepared draft into `docs/superpowers/plans/2026-09-23-companion-console-m10.md`, recreate M10 rules from M9, verify target/interface names against merged main, then sign the plan. Its binding controller rulings are already appended; no separate pre-flight audit is needed.
4. Execute the ten M10 tasks with bounded Codex implementers (owner authorized), daemon batch review, one final web/SDK review, one fix wave and gates. Use focused Hermes code/behavior reuse with notices. Runtime/container/TLS/provider acceptance remains unverified under the no-service constraint; automatic destructive session TTL is not invented without a product policy.
5. Local ledgers, gate logs and draft artifacts remain under ignored `.superpowers/sdd/`; all M9 tracked work is committed and pushed. Re-run relevant checks only for new changes.

**Milestone workflow reference (M9 implementation is already complete)**

1. Have **Sonnet** write a **lean plan**, modelled on `docs/superpowers/plans/2026-09-23-companion-console-m8.md`:
   - Interfaces, rules, test lists and commands; full code only for tricky pieces; about 1,000–2,000 lines.
   - Output: `docs/superpowers/plans/2026-09-23-companion-console-m9.md`.
   - Sources: spec §12.1–§12.2, the master plan's M9 section, and the codebase.
2. The controller rules on the plan's top-5 risks: append a "Controller rulings on the risks (binding)" section and commit the plan. No separate pre-flight audit.
3. Implement task by task:
   - Sonnet implementers; Opus only for concurrency- or security-subtle tasks.
   - Implementers read the plan directly plus `.superpowers/sdd/2026-09-23-companion-console-m9/rules.md`. Copy it from the M8 folder and change m8 to m9; those working files are gitignored, so recreate them from `rules.md` in this handover if missing.
4. Reviews:
   - Batch-review the daemon tasks (2–4 at a time, Sonnet; Opus for risky areas).
   - One final web+SDK review covers all web tasks.
   - One fix wave.
5. Run the gates:
   - Use `NX_DAEMON=false`, `NX_ISOLATE_PLUGINS=false`, `NX_NO_CLOUD=true`, `CI=1`, `ANIMA_SKIP_SERVICE_TESTS=1`, and `NX_NATIVE_FILE_CACHE_DIRECTORY=node_modules/.cache/nx-native-m9`; set `CARGO_INCREMENTAL=0` before cargo or Rust Nx targets. Sandbox native-cache/realpath restrictions require escalated local checks; no dependency installation is needed.
   - `bun x nx run rust-daemon:test --skipNxCache`
   - `bun x nx run-many -t test,typecheck,build -p @animaOS-SWARM/sdk @animaOS-SWARM/web --skipNxCache`
   - `cargo fmt --all --check`
   - `bun x nx format:check --base=origin/main`
6. Update the master-plan status row and this handover, push, and open a PR to `main`. Merge only when the owner says "merge".
7. Keep a ledger in `.superpowers/sdd/<milestone>/progress.md`, recording each decision as `Ruling: <what> — <why> — <cost if wrong>`.

**Rules every agent follows**

- Prefix every cargo command with `CARGO_INCREMENTAL=0`.
- Commits and staging:
  - Stage by explicit path. Never stage `nx.json` or `anima.yaml`; they show as modified locally but aren't ours.
  - Never use `git stash`, `reset`, `checkout -- <path>`, `restore` or `worktree`.
- Environment:
  - No new dependencies or dependency features.
  - Don't start a real daemon, dev server, database or container. No Postgres is available, so Postgres tests stay `#[ignore]`.
  - Don't touch the Docker Postgres on port 55432.
- Code shape:
  - `agent_runs.rs`, `connectors/runtime.rs` and `ViewHarness.tsx` only gain wiring lines.
  - Lock order: control-plane transaction → state lock → leaf mutexes. No std lock across `.await`.
  - Owner mutations run their transaction/save/undo body in `tokio::spawn`, so they're drop-safe.
- Content and tests:
  - Model-written text renders in the web only as text, with hidden characters revealed (`lib/skills.ts` `revealInvisible`, `components/memory/RevealedText.tsx`).
  - Refuse hidden Unicode wherever the model writes (`skills::is_smuggling_character`, `memory_text.rs`).
  - Strings are named constants, each tested once.
  - Tests are never wall-clock-racy, and web tests stay pristine (no new `act()` warnings).

**Gotchas**

- **GPG:** commits are GPG-signed. When gpg-agent's cache expires, `git commit` blocks on a pinentry dialog until the owner types the passphrase. Run long commits in the background and ask the owner.
- **Line endings:** with `autocrlf`, git can check files out with CRLF, which `nx format:check` flags. `nx format:write` on those files fixes it, and nothing is committed if only line endings differ.
- **Corrupted build artifacts:** errors such as "invalid metadata", "not found in rlib format" or an internal compiler error mean the build artifacts are corrupted. Fix them with `cargo clean -p anima-daemon -p anima-core -p anima-model-adapters -p anima-memory`, then rerun.
- **CI:** it always fails at the Nx Cloud step. The local gates are the verification.
- **`gh pr edit`:** it fails; use `gh api -X PATCH repos/G9000/animaOS-SWARM/pulls/N`.
- **Stacked PRs:** they don't retarget automatically. PATCH `base=main` before merging.

**Open follow-ups**

- Owner direction (2026-10-11): use Hermes as a focused behavioral reference and keep Anima's independent Rust engine. Start with Telegram, Discord and Gmail, Calendar and Drive rather than integration parity. Connectors should support optional community modules. Today's TS `ModPlugin` is stateless/tool-only and Rust connectors are built-ins; a lifecycle-capable community connector interface is still needed. This is outside M9.
- Owner auth for the older memory routes.
- Hidden-text refusal for evaluator extractions.
- A `memory.updated` event.
- Token estimation for end-only usage providers.
- A combined usage summary endpoint.
- Telegram/CLI notice that an approval is waiting.
- Skip a skill scan while the previous one's blocking work is still running.

## Update 2026-10-09: M8 done, PR open (stacked on M7)

- **M8 (Usage, logs, health) is complete** on `feat/companion-console-m8`. Its PR targets `feat/companion-console-m7`; merge #12, then #13, then M8.
- **Gates at `65122bd`, all green:**
  - `rust-daemon:test`: 2,082 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 1,042 tests).
  - Formatting, and a log-secrets audit.
- **Snapshot v10** (pricing overrides). The first start writes a `.pre-usage.bak` backup.
- **Gotcha:** the first gate run failed on corrupted build artifacts ("invalid metadata", an internal compiler error). Running `cargo clean -p anima-daemon -p anima-core -p anima-model-adapters -p anima-memory` fixed it and freed about 87 GB.
- **GPG signing still times out** when the passphrase cache expires; Task 4's commit waited on the owner.
- **Follow-ups:**
  - Token estimation for providers that report usage only at the end.
  - A combined usage summary endpoint.
  - The Logs page can't tell an open-but-quiet stream from one that hasn't connected (no "open" event).
- **Next:** M9 (attachments and voice).

## Update 2026-10-04 (later): M7 done, PR open (stacked on M6)

- **M7 (Memory) is complete** on `feat/companion-console-m7`. The branch is based on the M6 branch, so its PR targets `feat/companion-console-m6`; merge M6 (#12) first.
- **Gates at `e33f919`, all green:**
  - `rust-daemon:test`: 1,925 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 932 tests).
  - Formatting.
- **Lean plan, about 900 lines,** with a cheap process: Sonnet implementers (Opus for Task 2), one daemon batch review, one web final review, one fix wave.
- **Formatting gotcha:** with `autocrlf`, git can check files out with CRLF line endings, which `nx format:check` flags. `nx format:write` on those files fixes it, and if only line endings differ, nothing gets committed.
- **Post-M7 follow-ups:**
  - Owner auth for the older memory routes.
  - Hidden-text refusal for evaluator extractions.
  - A `memory.updated` stream event.
- **Next:** M8 (usage, logs, health), with a lean Sonnet plan.

## Update 2026-10-04: M6 done, PR open

- M5 is merged (PR #11).
- **M6 (Automations) is complete** on the new branch `feat/companion-console-m6`. Each milestone now gets its own branch, cut from `main` after the previous merge.
- **Gates at `ccce91b`, all green:**
  - `rust-daemon:test`: 1,874 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 845 tests).
  - Formatting.
- **Cost mode (the owner asked for it):**
  - No separate pre-flight audit: the controller rules on the plan writer's risk list.
  - Sonnet does most of the implementing, with Opus for the risky tasks.
  - Batched reviews, and a web-only final review.
- **Ledger:** `.superpowers/sdd/2026-09-23-companion-console-m6/progress.md`.
- **Next:** M7 (memory). Keep it cheap: Sonnet writes a shorter plan (interfaces and tests, not full code), then the same cost mode.

## Update 2026-10-03: M5 done, PR open

- **M5 (Skills) is complete** on `feat/companion-console`, and a PR to `main` is open.
- **Work done:** all 13 tasks, then the final review (daemon and web+SDK reviewers) and one fix wave (`2060912` web, `0b08b2b` daemon).
- **Gates at `e9d57f9`, all green:**
  - `rust-daemon:test`: 1,775 passed, 7 ignored.
  - SDK and web test, typecheck and build (web 807 tests).
  - `cargo fmt --check` and `nx format:check`.
- **Merging:** only when the owner says "merge".
- **Next:** M6 (automations), following the master plan. Write the plan, run a pre-flight audit, write the rulings, then execute.
- **Ledger:** every M5 ruling is in `.superpowers/sdd/2026-09-23-companion-console-m5/progress.md`.
- **Post-M5 follow-up:** skip a scan while the previous scan's blocking work is still running (hung-drive case).

## Update 2026-10-02 (latest): M5 in progress (Tasks 1–7 committed)

- **Done:** the M5 audit rulings are committed in the plan (`be0029f`). Tasks 1–5 are implemented, reviewed and fixed. Tasks 6 (`5729eab`, skill routes) and 7 (`61c6123`, draft/import routes and the multipart reader) are committed but **not yet reviewed**.
- **Resume:** run one combined review of Tasks 6 and 7, a fix round, then Tasks 8–14, the final review, the fix wave and the gates, then a PR.
- **Working files:** the ledger, briefs and rules are in `.superpowers/sdd/2026-09-23-companion-console-m5/`. These are local and gitignored; regenerate the briefs from the plan if they're missing.
- Task 7 hardened the multipart reader now (per-part caps, a linear boundary search) instead of deferring that to M9 (audit m19). Drop that deferral note.

## Update 2026-10-02 (later): M5 planned and audited, not started

- M4 is merged to `main` (PR #9, merge commit `f34313b`).
- **M5 (Skills) plan:** `docs/superpowers/plans/2026-09-23-companion-console-m5.md`, 14 tasks. Snapshot moves to v8, with `.pre-skills.bak` / `control_plane.backup.7` written first.
- **Pre-flight audit:** `docs/superpowers/plans/2026-09-23-companion-console-m5-preflight-audit.md`, with 0 Blocker, 4 Important and 19 Minor findings.
- **Resume here:**
  1. Rule on the audit findings: insert a `#### Controller rulings from the pre-flight audit (binding)` block per affected task and commit the plan. The auditor's recommended rulings are in the audit, and none needs a question for the owner.
  2. Extract briefs to `.superpowers/sdd/2026-09-23-companion-console-m5/` and copy `rules.md` from the M4 folder.
  3. Run Tasks 1→14 as in M4.
- **The four Important findings:**
  - **I1:** hidden Unicode (tag characters, bidi overrides, zero-width characters, U+2028/2029) in drafts.
  - **I2:** the page's Edit button can approve text nobody reviewed.
  - **I3:** Windows device names (`con`, `nul`, `com1`…) pass as slugs.
  - **I4:** `act()` warnings in the Skills page tests and `useSkillCommands`.
- **Tiers:** the auditor recommends the most capable model for Tasks 4, 5, 9 and 13.

## Update 2026-10-02: M4 done, PR open

- **M4 (Approvals) is complete** on `feat/companion-console`, and [PR #9](https://github.com/G9000/animaOS-SWARM/pull/9) to `main` is ready for review.
  - All 13 tasks are done, then a whole-milestone final review (split into daemon and web+SDK reviewers) and one fix wave (`9de5ea3` web, `20c6221` daemon).
  - Gates at `20c6221`: `bun x nx run rust-daemon:test --skipNxCache` passed 1,654 with 7 ignored; SDK and web test, typecheck and build are green (web 751 tests, SDK 63); `cargo fmt --check` and `nx format:check` are clean.
  - Merge only when the owner says "merge".
- **Next:** M5 (skills) per the master plan. Write its plan with an opus subagent from the spec, the master plan and the carry-forwards, run a pre-flight audit, then execute it the same way.
- **Post-M4 follow-ups:**
  - Tell Telegram and CLI users that an approval is waiting in the web console.
  - When a request's save fails, its card can linger until the tab reconnects; this is accepted, as in M3.
- **Ledger:** `.superpowers/sdd/2026-09-23-companion-console-m4/progress.md` holds every ruling.
- **Commits are GPG-signed.** When gpg-agent's cache expires, a commit blocks on a pinentry dialog until the owner answers it.

## Where things stood (2026-09-28)

- **Done and merged to `main`:**
  - M0 (security groundwork), M1 (run coordinator), M2 (sessions), and M3 (live runs).
  - PR [#6](https://github.com/G9000/animaOS-SWARM/pull/6) merged M0–M2; PR [#7](https://github.com/G9000/animaOS-SWARM/pull/7) merged M3 (merge commit `51f1f5b`).
- **In progress: M4 (Approvals).** The plan is written and committed, and its pre-flight audit is done. **No M4 code exists yet.**
- **Branch:** keep working on `feat/companion-console`. Don't switch branches in the shared checkout. Open the next PR from this branch when M4 is done.
- **M3 gates at its end:**
  - `bun x nx run rust-daemon:test --skipNxCache`: 1,564 passed / 7 ignored.
  - Web: 59 files / 703 tests. SDK: 16 files / 56 tests.
  - Typecheck, build, `cargo fmt --all --check`, and `bun x nx format:check --base=origin/main` all green.

## Key documents

- Spec (the binding authority): `docs/superpowers/specs/2026-09-23-companion-console-design.md`
- Master plan (milestone table and the "Carried from M3" list): `docs/superpowers/plans/2026-09-23-companion-console.md`
- Milestone plans:
  - `docs/superpowers/plans/2026-09-23-companion-console-m0.md` through `-m3.md` (done)
  - `-m4.md` (next, 14 tasks)
- M4 pre-flight audit: `docs/superpowers/plans/2026-09-23-companion-console-m4-preflight-audit.md`. Its rulings are not yet written into the plan (see "Next steps").
- Local-only working files: `.superpowers/sdd/2026-09-23-companion-console-m*/`. They hold the ledgers (`progress.md`), task briefs, reports, and review diffs, and are gitignored. On the same machine, the M4 ledger is `.superpowers/sdd/2026-09-23-companion-console-m4/progress.md`. Another machine won't have these files; everything essential is in this document and the committed plans.

## How the work is run

The workflow is the `superpowers:subagent-driven-development` skill:

1. One implementer subagent per task. It reads a brief extracted from the plan and commits its work.
2. A task review covers spec compliance and quality.
3. Up to 5 fix rounds follow, then a scoped re-review.
4. After the last task comes a whole-milestone final review. M3 split it into three parallel area reviewers: core+live, daemon, and SDK+web.
5. Then one fix wave, done in sequential slices, followed by the gates.

Decisions are recorded in the ledger as `Ruling: <what> — <why> — <cost if wrong>`. Opus handles concurrency-heavy tasks and reviews; sonnet handles mechanical ones.

## Repo rules (every agent, every task)

- Prefix every cargo command with `CARGO_INCREMENTAL=0`; test filters go after `--`.
- Stage files by explicit path only. Never use `git add -A`, `git add .`, or `git commit -a`.
- Implementers never stage `docs/` or `.superpowers/`; the controller commits docs.
- Never use `git stash`, `git reset`, `git checkout -- <path>`, `git restore`, or `git worktree`, and never switch branches.
- Don't start a real daemon, dev server, database, or container. No Postgres is available here, so Postgres tests stay `#[ignore]`.
- Don't touch the Docker Postgres on port 55432; it belongs to another project.
- No new dependencies.
- Keep touched files `cargo fmt` and Prettier clean. Format only touched files, never the whole repo.
- Lock order in the daemon: control-plane transaction mutex → state lock → live registry or fanout mutex. Room leases come before the transaction. No std lock is held across `.await`.
- Put new code in new modules or hooks. `agent_runs.rs` (about 5,600 lines), `connectors/runtime.rs` (about 7,500), and `apps/web/src/ViewHarness.tsx` (about 1,200) are already large.

## Gotchas learned the hard way

- **Disk is tight** (about 13 GB free); the Nx Rust gate needs about 12 GB. Leftover rustc codegen files pile up in `target/debug/deps/*.rcgu.o`. When no cargo or rustc is running, deleting ones older than 30 minutes freed about 13 GB once. Don't add extra `CARGO_TARGET_DIR`s, and never delete anything outside the repo; the user frees space there.
- **CI always fails** at `nx start-ci-run` (an Nx Cloud setup issue), including on `main`. The local gates are the verification.
- **Merging PRs:** auto mode denies Claude's self-merge unless the user explicitly says "merge". `gh pr edit` fails (GraphQL Projects deprecation); use `gh api -X PATCH repos/G9000/animaOS-SWARM/pulls/N`, and `gh api -X PUT …/pulls/N/merge -f merge_method=merge` once the user says to merge.
- **Subagent stalls** (the 600 s watchdog) happened on long single responses. Tell implementers to work one item at a time, commit after each, and pipe test output through `tail -30`. If an agent stalls twice, start a fresh one on the remaining items.
- **Plans:** fence partial code fragments as `text`, because Prettier mangles partial JSX fenced as `tsx`. End every SDK-changing task with `bun x nx run @animaOS-SWARM/sdk:build`, so later web vitest runs resolve the new exports.
- The model adapters' HTTP client has no request timeout, so every new long wait or model call needs its own bound.

## Next steps (M4)

1. Read the M4 plan's header and Global Constraints, and the pre-flight audit.
2. Rule on the audit's findings: 0 Blocker, 6 Important, 14 Minor. Then insert a `#### Controller rulings from the pre-flight audit (binding)` block at the end of each affected task in the plan, as M3's plan does, and commit the plan. The six Important findings, with the auditor's recommended rulings:
   - **I1:** Task 5's two race tests depend on 250/500 ms real-time timing and can flake under the parallel Nx gate. Rewrite them with patient timeouts, an explicit `settle_approval(TimedOut)` task, and `yield_now()` ordering, without paused time.
   - **I2:** A broad exec rule (curl, an interpreter, git, npm) lets the companion rewrite its own approval policy with a forged-Origin loopback request, or run code through default-allowed writes to `.git/config` or `package.json`. The auditor recommends documenting the limit in the README and warning in the UI, with no per-approval token. **Ask the user** whether they want stronger protection.
   - **I3:** With `exec: ask` as the default, check-ins, jobs, Telegram and CLI runs all wait for a web decision. The spec intends this. The legacy `/run` answers 408 after 10 minutes while the approval keeps waiting. Add README and UX copy.
   - **I4:** Document the snapshot v7 rollback (`.pre-approvals.bak` / `control_plane.backup.6`) in the README, as M2 and M3 did, and keep the v7 bump.
   - **I5:** Task 8's `cargo check` runs build a second set of artifacts on a disk-tight machine. Use `CARGO_INCREMENTAL=0 cargo test -p anima-daemon --no-run` plus a grep instead.
   - **I6:** "Always allow" on a delegated specialist's approval creates a rule the Approvals page can't show or remove. Hide "Always allow" when `approval.agentId !== companion.id`, or list those agents' rules.
3. Run Tasks 1→14 strictly in order. Task 14 is the gate plus the master-plan status row.
4. Final review, fix wave, gates, then a PR from `feat/companion-console` to `main`.
5. After M4, M5 through M10 follow the master plan: skills, automations, memory, usage/logs/health, attachments/voice, and deployment/docs. Each gets a plan written by an opus subagent from the spec, the master plan, and a carry-forwards file, then a pre-flight audit, then execution.

## Open items carried forward

These are listed in the master plan under "Carried from M3":

- **M8:** usage for stopped calls, compaction, and titles; the steer queue cap.
- **M10:** rewrite the Playwright specs that mock `/run`, split the large files, and add provider HTTP timeouts, SIGTERM handling, and shutdown producer ordering.
- **Manual acceptance:**
  - One real xAI call, which sends running usage totals.
  - One gpt-6 call, to check whether it accepts `temperature: 0.2` for compaction and titles.
  - Anthropic and OpenAI streaming end to end.

## Using what's built today

Run `bun dev --host rust`. It starts the Rust daemon on `127.0.0.1:8080` plus the web console and playground. Everything from M0–M3 works:

- Parallel sessions with a sidebar.
- Live streamed replies with tool cards.
- Stop, steering (Ctrl/⌘+Enter), and queued sends with safe retries.
- Slash commands, compaction, AI titles, and check-in replies.
- Helper sessions, and the `search_conversations` tool.

The first start after upgrading migrates the control-plane snapshot to v6, and the JSON store keeps a `.pre-live-runs.bak` backup. Approvals (M4) are not there yet, so tools run under the old rules with no approval prompt.
