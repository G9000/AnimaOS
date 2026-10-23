# Active personality editor

The profile editor must display and save the personality used by the Rust runtime. Existing SOUL files remain authoritative when admitted by the instruction loader. Otherwise the agent's persisted configuration supplies the personality. No changes to provider credentials, tool permissions, user memory, or existing conversations are part of this feature.

## Contract and persistence

Owner-only `GET /api/agents/{agent_id}/personality` returns `{ source, path, content, revision, maxChars }`. Source is `workspace_soul`, `profile_soul`, or `agent_config`; path is a workspace-relative SOUL path or null. Content is the full editable personality, not a metadata preview. A configuration personality includes the existing bio, backstory, traits, expertise, communication style, and system prompt using the runtime's shared renderer. Knowledge remains separate.

Owner-only `PUT` accepts `{ content, revision }` and returns the same view. Both routes are no-store. The revision covers the source, configuration, workspace, and exact source bytes, so source switches, external edits, and stale tabs return 409. Refuse empty, unsafe, or content over 16,000 Unicode scalar values and unreadable, linked, or rejected SOUL sources; never silently replace a problematic file with configuration. File updates are atomic through held workspace capabilities. Configuration saves replace the legacy personality fields with the complete document in `system`, preserving knowledge and every unrelated setting. Persist before acknowledging success; retain the prior in-memory configuration on a definite write failure and reconcile uncertain publication through durable readback before accepting further work. Reads/saves use the existing local-owner boundary; saves respect profile ownership and the control-plane transaction lock.

New turns admitted after a successful save read the saved personality. Runtime token budgeting still applies to the complete instruction set. Already admitted runs retain their accepted snapshot. SOUL text persists as a local file; configuration text persists in the existing control-plane store. The same loaders restore it after restart.

## Editor

Replace the disconnected personality radio buttons, owner-name extraction, and advanced system-prompt textarea with a single active personality editor. Keep the existing name/avatar/model controls. Show which source is active in plain language and when edits take effect. Explain that workspace SOUL edits are shared by agents using that file.

Offer guided fields for identity and role, personality, values and philosophy, goals, expertise, communication, boundaries, and example replies. These edit Markdown sections within the same document. Preserve introductory prose, unknown sections, and frontmatter; a raw Markdown view permits arbitrary existing personas. Switching views must preserve text. Include concise writing hints and a preview of the current document. Recognized nested sections have disjoint editing ranges; unknown nested sections stay with their parent. Save personality independently with clear pending, success, failure, and stale-revision states; preserve drafts on errors and require explicit reload after a conflict. Disable saves during loading and when unchanged. Agent switches discard only the previous agent's component state and ignore stale fetch results.

## Verification

Add regression coverage for legacy and SOUL persistence, profile isolation, owner authorization, stale edits/source switches, invalid content, symlinks, atomic-write failure, and persistence rollback. Capture actual model requests in two sessions before and after restoring durable state; verify the saved text is included and displaced instructions are absent. Cover editor loading, guided/raw preservation, save failure/conflict, and agent switching. Run relevant Nx SDK/web checks and the required full `rust-daemon:test --skipNxCache`. Use isolated runtime data for live restart acceptance, leaving the user's daemon and files untouched. Report deterministic versus live provider evidence separately.
