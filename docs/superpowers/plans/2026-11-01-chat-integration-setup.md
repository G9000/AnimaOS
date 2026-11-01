# Chat integration and GitHub skill setup

This records the initial implementation phase. The final package/runtime scope,
Metricool compatibility and live acceptance supersede it in
[Full skill packages](2026-11-01-full-skill-packages.md).

The owner authorized implementing both setup capabilities from the screenshot and clarified that the agent should search the web, inspect GitHub installation instructions, and continue supported setup without waiting for another invitation.

## Design and grounding

`manage_mcp` already belongs to direct owner web/API runs, remains advertised with tool discovery, and changes the same MCP manager as Connectors. OAuth exists in `mcp/oauth.rs` and the Connectors sign-in control. Its chat descriptor incorrectly implies OAuth is unsupported. `search_tools` matches descriptions, so a service name alone can produce no match. This does not establish that setup is impossible or that the running daemon is current.

`web_search` and `web_fetch` already allow repository/documentation research where granted. `propose_skill` creates model-authored Agent drafts and can autoactivate in Free. Externally retrieved packages must instead use `SkillService::import_package`, which preserves Import review semantics, validates supporting files, and owns the persistence transaction.

Two independent candidates were compared through pstack Architect/Arena. A narrow GitHub import plus existing MCP setup keeps each manager as the single state owner. A unified integration installer would coordinate unrelated credential, bundle and skill lifecycles, adding another writer and a broad interface. Both candidates selected the narrow bridges and shared setup guidance. Research identifies the supported setup; it never grants authority to downloaded instructions.

## Data shape and boundaries

- `import_github_skill` accepts a public `owner/repository`, revision, skill directory, and optional slug. The agent derives these from the repository URL it inspected; explicit fields avoid ambiguity in branch names containing slashes.
- Resolve the revision to one immutable commit, then import `SKILL.md` and supported text files under the existing package folders at that commit. Never execute fetched scripts or follow server-supplied download URLs. Reject incomplete, unsafe, truncated or oversized packages without a partial draft.
- Retain repository, commit, directory and package hash with the draft and expose the source during review. Use the existing pending Import draft and existing approval path even in Free.
- Restrict import to a direct owner turn, including dispatch revalidation and normal write policy. Helpers, jobs, peers, delegated runs and external channels cannot gain it through a forged serialized descriptor.
- Keep `manage_mcp` as the sole connector writer. Make its setup/authentication instructions and results reflect the existing browser OAuth sign-in path and saved OAuth credential state. A saved connection is not proof of a completed sign-in or successful discovery.
- When asked to install a skill or connect a service, use available web research immediately. Inspect README/SKILL.md, linked supporting files and dependencies, then carry out supported setup. Ask only for a genuinely missing URL/account sign-in/required review. Report the specific unsupported requirement rather than claiming no installer exists.

## Throughput checkpoint

- [x] Blocking first steps. Read actual pstack Feature, How, Architect and Arena sources; inspect current tool scope, catalog, MCP/OAuth, import and approval ownership; fetch current GitHub and MCP documentation through Context7.
- [x] Independent workstreams. One owner implemented GitHub import and integration wiring; a second updated MCP management guidance/results and tests. The coordinator owns shared setup context, docs, reviews and validation.
- [x] Shared mutable state. Source file ownership was disjoint. Managed native validation uses the repository wrapper and existing absolute validation target, under the 32 GiB budget. The initial full SDK run was stopped when its service suites attempted separate Rust builds; SDK unit validation was rerun with those suites disabled, and full native validation runs separately.
- [x] Smallest safe decomposition. Acquisition, imported-draft persistence and owner scope stayed with one implementation owner because their invariants cross those boundaries. Separate MCP changes have no new state owner.

## Implementation and verification

- [x] Add meaningful regressions for source pinning, exact package completeness, bounds, unsupported source entries, pending import in Free, persisted provenance, direct owner availability, denied origins, write approval and revocation.
- [x] Implement bounded GitHub acquisition with fixed trusted HTTPS origins, no credentials/redirects/proxies, safe errors, a total deadline and request/body limits. Reuse package validation and import persistence.
- [x] Register the owner import control consistently in tool selection, live catalog checks and approval classification. Do not broaden web or execution grants.
- [x] Correct MCP OAuth guidance and connection observations. Test existing discovery and OAuth state; keep service lookup generic rather than hardcoding Metricool.
- [x] Add setup context explaining the research-to-setup flow and preserve external instructions as untrusted data.
- [x] Review the combined diff independently, fix findings, run `bun run rust:test` and relevant focused/lint checks. Record failures and distinguish source validation from live daemon/provider acceptance.
- [x] Update companion docs and this checklist with actual evidence. Leave the user daemon and live accounts alone; no account can be connected without its sign-in and no particular GitHub skill URL was supplied for installation.

## Validation evidence

- Expected failing tests were observed before implementation: GitHub owner-control availability, missing source link in the Skills card, and missing OAuth sign-in guidance in the MCP result.
- The first native compile found test-only private-state access; the integration test was moved under `agent_runs` without widening production visibility.
- The first full Rust run reached 3,906 passed, seven failed and 42 ignored in the daemon unit suite. Three failures were exact tool-list/schema expectations for the new control. One revealed that generic secret sanitization also hides credential-named boolean fields; the new tool-only observation is now `accountSignInSaved`, and the secret filter remains unchanged. Three untouched Office directory tests required a short, owner-private temporary root. All four Office-directory checks passed with that configuration before the full rerun.
- The final `bun run rust:test` passed on the current source with that private temporary root. The daemon unit suite passed 3,913 tests with zero failures and 42 intentional ignores; its integration suites, the core suites and packaged supervisor suites also passed.
- SDK unit checks passed 346 tests, with six service tests intentionally skipped using `ANIMA_SKIP_SERVICE_TESTS=1`; SDK build/typecheck and lint passed.
- All 46 relevant Skills UI tests passed across five files. Web typecheck, production build and lint passed. Existing lint warnings remain outside this change.
- Unrelated memory/UI edits appeared while the machine was paused. They were preserved; the current web typecheck, production build and all 46 Skills UI tests were rerun successfully after those edits appeared.
- `bun run rust:lint` passed on the final tree, including repository formatting and workspace Clippy correctness checks. Existing unrelated warnings remain.
- Independent review found one ambiguous setup-result sentence, which was corrected. The reviewer then found no remaining concrete issue in the combined implementation and late-admission/base-change regressions.
- Acquisition routing is tested with a fake GitHub API and current official GitHub documentation. No live GitHub import, Metricool account sign-in, model-provider call, ordinary daemon restart, commit or push was performed. The running app still needs to load the new build.
- The task-owned private test root was checked for its exact owner marker and active processes, then removed after validation finished.

## pstack adaptation

Use available Codex agents with inherited models for independent candidates and review. Cursor Task roles and prescribed Claude/Grok models are unavailable; no model diversity or installed pstack executable is claimed. The user's implementation authorization takes precedence over extra skill approval checkpoints. The initial phase excluded commit, push, PR and service restart; later authorization expanded implementation and requested a local commit.

Sources read: [pstack Feature](https://github.com/cursor/plugins/blob/main/pstack/skills/poteto-mode/playbooks/feature.md), [Architect](https://github.com/cursor/plugins/blob/main/pstack/skills/architect/SKILL.md), [Arena](https://github.com/cursor/plugins/blob/main/pstack/skills/arena/SKILL.md), [How](https://github.com/cursor/plugins/blob/main/pstack/skills/how/SKILL.md), [Metricool setup](https://help.metricool.com/how-to-connect-metricools-mcp-eqp9h), [GitHub contents](https://docs.github.com/en/rest/repos/contents), [Git trees](https://docs.github.com/en/rest/git/trees).
