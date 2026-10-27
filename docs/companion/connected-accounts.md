# Search connected accounts and start workflows

Open **Connections → Search and account workflows**. Connect an account under Services first, then enable each source you want to keep searchable on this daemon. Enabling search does not grant permission to send mail or apply calendar changes.

Search covers Gmail, Outlook and IMAP inbox content, the files you selected for Google Drive, and your connected Google calendars. It searches the retained local index rather than your entire account. Each source shows its scope, indexed item count, last successful refresh and any error. Enabled sources refresh about every two minutes while the daemon is running; **Refresh account content** requests an earlier refresh.

Use the account and content-type filters to narrow a search. Open a result to read its indexed text and source details. **Open original** links to the provider when available; IMAP messages remain readable through the owner-protected citation view. Indexed text may be shortened, and the preview states when this happened. Disabling a source clears its retained content; reconnecting requires enabling it again.

## Account workflow rules

Save a workflow under **Work**, including any approval checkpoints and named parameters it needs. In Connections, select **New account rule**, choose an enabled source and saved workflow, then add optional title, sender or collection filters. Bind parameters to event content, title, sender, source link, identifiers, observed time or fixed values. Saving pins the workflow version and takes a fresh baseline on the next successful sync.

Rules respond to newly observed inbox mail, selected-file changes and observed calendar changes. Existing content does not start workflows when you first enable a rule. Polling and bounded retention can miss activity between refreshes; this is not a guaranteed incoming-message delivery service. Items disappearing from an inbox or calendar window are not treated as remote deletions.

Rule history records the workflow ID and rule version for each admitted event. Review running work, approval checkpoints and questions under **Work**. Pausing a rule stops further starts. Reconnecting the source or changing its workflow version pauses the rule until you enable it again. Removing a rule retains its existing workflow history.

Source data is treated as untrusted content. A rule starts an existing saved workflow using the agent’s current permissions; it does not add tool grants or bypass approval steps. If an effect has an uncertain outcome, the existing workflow review controls apply. Capacity or persistence failures preserve the previous sync baseline and show a source error so the same event can be safely retried.

## Retention

The index is bounded to 100 recent inbox items for Gmail and Outlook, 25 for IMAP, 16 selected Drive files and 500 calendar events from 30 days ago through 180 days ahead. Each item retains up to 8 KiB of text; metadata and dedupe history count toward the shared 16 MiB bound. Mail attachments are not indexed. Unsupported Drive formats are searchable by metadata only. Large or partially fetched snapshots do not silently replace a previously complete sync. Event identity history is bounded separately; a full history requires disabling and enabling the source again, followed by a fresh baseline.

MCP connections do not automatically become background search or event sources. Native OneDrive and Outlook Calendar coverage are not included in this release.
