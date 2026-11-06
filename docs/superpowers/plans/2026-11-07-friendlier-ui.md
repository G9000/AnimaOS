# Friendlier Anima UI

**Goal:** Make the everyday Anima interface calmer, easier to read, and more welcoming.

**Design:** A warm neutral canvas, generous reading rhythm, a welcoming serif greeting, small optional conversation starters, and a softly rounded composer. Keep the user's dark/light theme and accent. Apply the same surfaces and spacing to settings and menus. The existing simple mode already keeps technical controls out of the everyday chat.

**Architecture:** A scoped presentation stylesheet loaded after existing styles; small copy and markup changes in ChatScreen and WorkspaceShell. Keep the conversation mounted, preserve input state and keyboard behavior, and retain all actions, permission prompts, and advanced settings. No daemon changes.

**Tech stack:** Existing React components and CSS; no new dependencies.

- [x] Inspect actual components in an isolated local preview, including welcome and conversation states.
- [x] Update the shell, typography, welcome, composer, and settings presentation.
- [x] Verify desktop/mobile, dark/light, long content, focus, menu access, and draft persistence.
- [x] Run web tests, build, typecheck, and lint; review diff and remove temporary preview files.

No additional tests are needed for cosmetic declarations or copy that duplicate the implementation; existing behavior tests cover the preserved interactions.

## Validation

- Full web suite: 221 files, 2,694 tests passed, 1 skipped.
- After CSS refinements: visual tokens, ChatScreen, and WorkspaceShell suites passed (138 tests, 1 skipped). After aligning suggestion accessible names with their visible labels, ChatScreen passed again (82 tests, 1 skipped).
- Final web build, typecheck, and lint passed. Existing lint warnings and large-bundle build warning remain.
- Browser preview used the real WorkspaceShell, MessageList, Composer, and Appearance components with synthetic conversation data. Checked dark/light, desktop/390px mobile, 420px short viewport, long names/URLs, draft preservation through settings, suggestion insertion, menus, and keyboard dismissal. The long-content mobile document remained 390px wide with no horizontal overflow.
- Review findings fixed: light menu contrast, active microphone color, long agent-name truncation. Static rereview found no remaining substantive issues in those fixes.
- Preview image: `test-output/friendlier-ui/welcome.jpg`. Temporary harness and owned preview server removed/stopped. No live daemon/provider acceptance was performed.

## Settings follow-up

The owner asked for the same treatment for the settings window. The panel now has a bounded, centered desktop frame, clearer text and controls, and a larger close target. Everyday settings are grouped under Personal and Workspace. Advanced sections are folded initially, expand for a directly selected advanced page, and remain searchable. Permissions stay visible, and the folded Advanced group shows pending skill reviews. Mobile uses a labeled section picker above the content instead of the bottom tab strip. All destinations, save locks, and scope descriptions remain available.

- Added six behavior regressions for disclosure state, external navigation, hidden-section search, pending reviews, mobile navigation, and save locks. Updated shell/controller integration tests to use the new navigation.
- Focused navigation/shell tests: 51 passed. Controller and visual contract tests: 162 passed.
- Final full web suite: 222 files, 2,700 tests passed, 1 skipped. One timing-sensitive existing conversation-reset test failed in the earlier full run and passed in both the focused controller suite and the final full run without changes to that test or its implementation.
- Web build, typecheck, and lint passed; existing warnings remain.
- Browser checks used real WorkspaceShell, SettingsPanel, and Appearance components in a temporary local harness. Verified desktop, 1024px laptop, 390px mobile, dark/light, profile fields, mobile section switching, advanced disclosure, search, and scrolling. Mobile document width stayed 390px. No live daemon/provider acceptance was performed.
- Fixed review findings: preserved custom select chevrons and corrected faint profile labels/actions in light mode. Preview saved at `test-output/friendlier-ui/settings.jpg`; the temporary harness and its server were removed/stopped.
- Task-owned verification logs were removed after recording the results; only the requested visual previews remain.
