# Discord thread titles

Approved behavior: strip Discord user, role, and channel mentions from each auto-created thread's initial name, then give it an AI-generated title after its first completed reply. Preserve manual names.

Keep this in the Rust host's Discord adapter. Save the exact initial name and a one-shot title claim on the durable conversation record, with serde defaults that leave older/pre-existing threads alone. The thread-start journal retires with the intake cursor and cannot hold title state. Conversation retention is already bounded. Reuse the coordinator's bounded, metered title-generation call and autoTitle setting. Run the title task in the background under the manager's owned-task/shutdown accounting; do not delay message delivery. Consume the claim atomically in the completed reply's source commit, rolling back with a failed source save. Restart and later replies never repeat an attempted title. A separate title-effect lifecycle claim allows concurrent message delivery while credential changes drain both kinds of effect.

Before a rename, fetch current Discord metadata and compare its name to the saved initial name. Verify thread/guild/parent identity and current connector authority, then PATCH only the name. Keep the initial name on generation, permission, or transport errors. Never retry an uncertain rename or rename on later replies. Discord has no conditional rename API: a manual edit racing between the final GET and PATCH cannot be made atomic; this is the same best-effort guard Hermes uses.

Reference: NousResearch/hermes-agent, plugins/platforms/discord/adapter.py::_derive_auto_thread_name / rename_thread and gateway/run_topics.py::_rename_discord_auto_thread_for_session_title, inspected 2026-10-24. Implement natively using existing Anima transports; preserve attribution.

Validation: transport fixtures for mention cleanup, Unicode limits and PATCH; manager tests for first reply only, manual rename, title failure/disabled setting, pre-existing threads and revoked authority. Run the full required bun run rust:test gate and formatting checks.
