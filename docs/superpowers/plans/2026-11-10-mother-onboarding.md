# Optional M.O.T.H.E.R onboarding implementation plan

**Goal:** Offer M.O.T.H.E.R as the new companion's personality, primary durable memory and shared context during personal onboarding, while keeping both products independent and preserving all existing agents and chats.

**Architecture:** Add a local/M.O.T.H.E.R choice to the active personal `CompanionSetup` flow. M.O.T.H.E.R alone owns its persona, grants and per-agent sharing policy. Once the new companion exists, guide the owner through `mother connect animaos` for that named agent in Companion mode; use the existing owner-only workspace-context metadata endpoint to check the saved binding and exact reader connection. The UI never creates grants, reads private configuration files, changes another agent, calls a model, or reports the radio choice as a completed connection. Its local personality remains the existing runtime's offline fallback.

**Alternatives:** Automatic grant provisioning inside AnimaOS would duplicate M.O.T.H.E.R's authority and couple the applications. Leaving setup only in Settings hides the requested onboarding option. Guided owner pairing with a checked, explicit pending state reuses the implemented boundary and is the selected approach. Pairing is local; no tunnel or provider API key is required by this feature.

**Files:** `apps/web/src/components/onboarding/CompanionSetup.tsx`, its existing tests, new `MotherOnboarding.tsx` / `.test.tsx`, and `docs/workspace-context.md`. Reuse existing onboarding classes and `daemon.workspaceContext`; no backend or SDK contract changes.

## Tasks

- [x] Verify the existing companion onboarding baseline in the isolated checkout. Install locked dependencies if needed.
- [x] Add regression tests for the local default, optional M.O.T.H.E.R choice, exactly-once new-agent creation, deferred completion, preserved fallback personality, and pairing later without pretending activation.
- [x] Add focused pairing-component tests: independent/shared/disconnected/unreviewed/wrong-subject states never show ready; only a configured companion binding for the new agent with a reviewed row matching both connection ID and reader tool in `ready` state can show “Saved connection ready.” Clear cached readiness when a recheck starts or fails; cleanup ignores stale replies. Include the coder know-how warning → pairing guide → completion path so warnings never bypass pairing.
- [x] Implement the choice and post-create pairing guide. Preserve coder know-how notices and ordinary local completion. Clearly distinguish configured, offline and not-yet-paired states. Only explicit owner setup outside AnimaOS creates grants/bindings.
- [x] Run focused onboarding tests, web typecheck/build/lint and inspect the UI. Update the guide with actual setup, pending and offline behavior.
- [x] Finish the authorized installed-app update using a native package, preserving user data and retaining rollback binaries. Use the updated frontend artifact. Check native build metadata, installed hashes, backend health and the new binding endpoint. No model prompts and no existing agent connection changes.

## Rollout constraints

The owner chose app updates only for existing agents and a new project folder while preserving agents/chats; do not reset runtime data, erase memory, or automatically connect Anima/Mom. The published native build was already started before the onboarding request arrived; rebuild the embedded UI afterward. Keep source changes reviewable in `g9000/mother-onboarding`; this new feature has not been authorized for a separate GitHub push. M.O.T.H.E.R's verified installed release hash is `320f044c566d4855a3a7a9963113b0d16054c3d64651a2a321e6a0767f4f4dde`.
