# Web search and source links

After updating, restart an existing dev session with `bun dev --host rust` so it
uses the new daemon and console.

In the companion's Settings, enable **Allow web search**, then save. This grants
the native `web_search` tool. Enable **Allow web page reads** separately when you
want the companion to read a selected URL with `web_fetch`. Existing Network
approval rules still apply. Enabling either option makes no search or page request.
An explicit change to a standard access profile clears these optional grants.

Ask for a concrete result, for example: “Find the official Rust release notes,
summarize what changed, and include links.” The model receives result titles,
URLs and bounded excerpts. It can request a page read if it needs more detail;
search does not automatically open every result.

Native search sends the query to Exa's anonymous public MCP service. No API key
or account is needed for this path, but the service can be unavailable or
rate-limited. See the [upstream service documentation](https://github.com/exa-labs/exa-mcp-server).
The separately configured `exa_search` tool still uses the daemon's Exa API key;
native search never silently falls back to that paid path.

Expand the search tool card to see **Search sources**. Those links come from the
normalized successful search result and remain available in saved chat history.
They stay outside the shortened result preview. The assistant can also include
ordinary Markdown links in its answer. Retrieved sources establish where the
search result came from; they do not independently verify every answer claim.

Search accepts a trimmed query up to 512 UTF-8 bytes and one to five results,
defaulting to three. It makes at most one remote search call under one 30-second
operation deadline. Results, excerpts and source lists are bounded; shortened
results are marked. Stop cancels local waiting and initiates transport cleanup.
Already transmitted queries cannot be recalled from the remote service.

Search excerpts are untrusted external text. They cannot grant tools, change the
endpoint or override owner instructions. Source links allow only complete public
HTTP(S) URLs with no embedded credentials or local/private address literals.
This URL check is not DNS verification and does not guarantee a site's contents.
Opening a source link is an ordinary browser navigation.

For a search failure, inspect the tool's error and Network approval state. Check
the two grants independently. History reload does not retry a search. Anonymous
availability and result quality depend on the external service; source cards do
not imply general autonomous research quality or full browser automation.

## Recorded research acceptance

M85 accepted one actual model-driven official Rust2018 research task: one search,
two page reads, a cited report, full native read-back and linked final response.
Independent fact/prose/provenance checks, five quiet same-store seconds and owned
cleanup passed. The captured live/history sources passed60console component
checks. Five subscription model calls reported17,129tokens; no retry, helper or
fallback was used. See the [M85 outcome](../superpowers/plans/2026-10-17-companion-live-research-m85.md).

This is one finite workflow, not general answer-quality, browser-interaction,
coding, always-on or full-parity acceptance. The M84 task remains failed at its
private history-based verifier: one model call, no remote search or report.
M85 used live typed tool/approval events for pending-call verification and saved
full history for final grading on a distinct task. Neither case will be replayed
or regraded.
