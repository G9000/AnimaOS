# Owner-only Discord channel management

Approved request: implement native list, create and edit channel tools, callable only from the owner's Discord account. Implement in the Rust daemon's existing connector, without a separate service.

Expose `list_discord_channels`, `create_discord_channel` and `update_discord_channel` only on direct, live Discord runs backed by an admitted inbound receipt from the bot application's verified owner. Derive the single owner from Discord's application owner or team owner, never conversation labels, approved participants or role membership. Bind the capability to connector, credential generation, configuration revision, run, receipt, sender and current guild. Console follow-ups, helpers, peers, synthetic metadata and other accounts gain no capability. Recheck live authority and application ownership before network effects.

List guild channels. Create text, voice and category channels. Edit supported channels' names, text topics and parent categories; null parent moves a channel out of its category. Reject arbitrary guild/connector arguments, cross-guild targets, unsupported types, invalid names/topics and invalid category nesting. Do not add delete, permission-overwrite or role-management operations.

Use existing bounded REST transport, pacing, owned-effect lifecycle and durable tool journal. Validate Discord acknowledgements. Uncertain writes must not be automatically retried or reported as success. Serialize channel mutations per connector. Keep current channel metadata up to date after edits. Return routing status for created channels: creation does not silently widen an explicit channel allowlist. Existing bot-wide permitted routing continues to discover new channels normally.

Request Manage Channels in the generated invite. Existing installed bot roles still need that permission granted in Discord. No live server restructuring or process restart is part of implementation.

Verify native HTTP contracts and actual model visibility/execution, denied sender and spoofed labels, team-owner identity, cross-guild/category targets, stale/cancelled/reconfigured scopes, discovery, delayed/denied/uncertain/mismatched effects, and routing behavior. Run `bun run rust:test` and `bun run rust:lint` in the final tree.
