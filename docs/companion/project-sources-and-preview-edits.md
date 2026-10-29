# Connected project sources and preview edits

## Share selected account content with a project

Open **Projects → Sources → Add connected source**. Choose a file or mail conversation from a connected account, review its account and coverage, then confirm sharing with the project's members. The account must belong to a current member agent and have account search enabled. Drive and Microsoft files must also be selected in that account's connection settings.

The project receives read-only access to the selected content for knowledge search and new project chats. Each agent keeps its own account connection, tools and private memory. Sharing a document does not give other agents the account's credentials or access to its other files.

The Sources panel shows indexing state, coverage, omissions and the last index time. Use **Refresh index** to check for source changes. Search results open a citation with a verified passage and source information. **Unlink** removes the source from the project. Removed membership, account access, selected-file scope or search enrollment invalidates the old source grant and citations; sharing again requires a fresh selection.

Connected content is bounded. A selected file can supply up to 2 MiB of input and 1 MiB of extracted text in supported formats. Mail conversations include at most 25 messages and 1 MiB of text, with attachments omitted. IMAP currently supplies only the selected retained message and is labelled partial. Unsupported content, provider failures, pagination limits and omissions appear as partial or unavailable coverage.

## Request a change from a preview

Open a workspace HTML, Word, Excel or PowerPoint file beside the chat. For HTML, choose **Select content to edit** to open a safe text-selection view. For Office files, choose a supported editable paragraph, cell, chart target or slide text from its content preview. The available targets depend on the file's supported structure; the content preview does not promise exact page or slide layout.

Add the selected content to your draft and enter an instruction, such as “shorten this paragraph.” Your existing draft is retained. The selection appears as a removable chip. Selecting content does not send a message or change the file.

Send the instruction to request a proposal. Review the complete supported before/after change, then approve that exact proposal to apply it. A preview-edit request uses the native document-editing flow even when the agent has ordinary file-writing permissions. The request retains this restriction through retries and recovery. A changed source or destination requires a new selection or proposal.

HTML edits replace literal text and preserve the surrounding markup. Entity spelling can appear in the text-selection view; replacement text is escaped for HTML. Scripts, styles, markup, attributes and unsupported content remain outside the editable text targets. Office changes retain the existing editor's supported-format and preservation limits.

Closing, reloading or changing the preview clears a pending selection. On a small screen, **Add to draft** returns you to the composer with that selected content attached. Undo uses the existing file-checkpoint recovery flow when a checkpoint was created; the publication receipt identifies that checkpoint.

Connected project sources remain read-only. Preview edits apply to supported files in the configured workspace.
