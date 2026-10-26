# Agent Conversation Recall Implementation Plan

> Execute inline using superpowers:executing-plans. The user has authorized implementation and inspection of agent exchanges.

**Goal:** Recall actual incoming agent contacts in regular chats and let the owner inspect the exchange.

**Architecture:** Derive evidence from existing own session lineage, run records, and transcripts. Reuse bounded history search/windows and session navigation. Supply recent incoming evidence through a dedicated provider composed with existing context providers.

**Tech Stack:** Rust daemon, existing anima-core providers, React/TypeScript web console.

### 1. Own agent-conversation history
- [x] Add failing history tests for incoming delegated/peer sessions, anchored cold/hot reading, sender attribution, and exclusion of foreign/private/generated helper data.
- [x] Add a shared incoming-session predicate in `hosts/rust-daemon/src/sessions/interactions.rs`; reuse it in `sessions/history_read.rs`.
- [x] Extend `search_conversations` kind filter and schemas with `helper` (agent conversations) in `tools/conversations.rs` and `tools.rs`; attach source/sender/session lineage to search and window results.
- [x] Run focused daemon tests; confirm regression fails before implementation and passes after.

### 2. Regular chat awareness
- [x] Add tests that a regular owner chat receives recent actual incoming request provenance, even for old records, with bounded excerpts and honest incomplete-history guidance.
- [x] Render a bounded recent-interaction provider in `sessions/interactions.rs`; compose it in `state/run_commit.rs`, preserving summary/checkpoint/memory providers and tool grants.
- [x] Include request anchors and run status; never inject the sender's private chat or treat requests as current instructions.
- [x] Verify focused context/isolation tests.

### 3. Owner conversation inspection
- [x] Add a UI regression test for a regular chat exposing incoming/outgoing sessions and opening the exact agent/session.
- [x] Integrate Agent conversations into the selected agent's Other conversations history filter, using `SessionSidebar` and existing session navigation; keep the regular chat header clear.
- [x] Show sender/recipient on history rows; retain date grouping, search, pagination, archive controls, and the existing read-only transcript.
- [x] Run focused web tests, web typecheck, web build/lint.

### 4. Validate and review
- [x] Run full required `bun run rust:test` (isolated validation target if the live daemon locks its executable).
- [x] Run relevant web verification; inspect final diff and request independent code review.
- [x] Remove only task-owned scratch artifacts. Report exact validation and any required running-daemon restart; do not claim a live model test without running one.

## Validation outcome

Full core/daemon and supervisor suites passed individually after an old helper-kind rejection expectation was updated and a supervisor exit-code flake was rerun with serialized tests. Focused web checks (114 component/access tests and 3 navigation tests), web build/typecheck/lint, Rust formatting, and independent review passed. The broader ViewHarness file retains the same 26 failing test names as the saved baseline, with 121 passing tests and no new failure names. Reports are retained under test-output/agent-conversation-recall-*. The live browser check was stopped by URL verification. The running daemon needs a restart; no live model messages were sent.

## History placement revision

Moved inspection out of the chat header at the owner's request. Entry: selected agent menu → Other conversations → Agent conversations. The existing history list provides participant names, exact transcript navigation, search, archive filtering and pagination. Removed the temporary header component and its SessionView props. Regression tests first confirmed the misplaced header control and absent history filter, then passed with the relocated control. Relevant component/history suites (118 tests), three controller navigation tests, web build/typecheck/lint, and independent review passed. The existing peer-navigation assertion now includes both participant names in its accessible row label. Rust code was unchanged in this revision.
