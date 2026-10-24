# Rename the current Discord thread

The user approved an agent-callable rename for the current Discord thread, with confirmation only after Discord accepts the change. Implement `rename_discord_thread(title)` in the Rust host. Accept only a plain-text title, bounded to the transport's 80 UTF-16-unit limit; reject rather than silently truncate explicit names.

Carry a host-created Discord thread capability in the native run's typed room, then into its tool context. User message metadata and tool arguments cannot select a connector, channel, thread, or another run. Advertise the tool only for native, live, non-helper Discord thread runs, including tool discovery; retain normal write approval and durable action receipts. Refuse stale, stopped, reconfigured, upstream, or mismatched runs.

Reuse the existing native GET/PATCH transport and lifecycle fence. Explicit renames can replace manually chosen titles. Serialize title effects against automatic naming, clear the auto-title marker before dispatch, and make the automatic worker recheck that marker, preventing a delayed initial title from replacing an explicit rename. Preserve original routing identity throughout.

Return success only after the PATCH response proves the same thread, guild, parent, and accepted title, and metadata persistence succeeds. Permission denial, rate limits, uncertain transport results, and persistence failures return errors; uncertainty must state that the title may have changed and must not trigger a blind retry. Await existing action-journal settlement before the model receives the result. Tool guidance requires calling the tool and waiting for success before claiming a rename; this does not establish universal model honesty.

Verify registration, actual model visibility and current-thread execution, delayed acknowledgement, failed/uncertain/mismatched responses, invalid/cross-thread arguments, scope restrictions, cancelled/revoked runs, discovery, and automatic-title races with deterministic fixtures. Run `bun run rust:test` and formatting checks. Do not start or restart the ordinary server; no live Discord message, commit, push, or restart is part of this request.
