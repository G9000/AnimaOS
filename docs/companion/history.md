# Conversation history as evidence

Anima can discover its own earlier conversations and read a small window around
one matching message. In agent Settings, **Allow reading conversation context**
adds the optional `read_conversation_window` tool. It is off by default; changing
to a standard access profile clears the option, which you can explicitly enable
again. Turning it off removes only that grant. Helpers cannot inherit either
owner-history tool.

`search_conversations` requires a nonblank `query` of at most 200 characters.
Optional `limit` defaults to 5 and ranges from 1 to 10. Optional `kind` is `chat`,
`telegram`, `discord`, `checkin`, or `job`. Optional `after_ms` and `before_ms` are
nonnegative JavaScript-safe integer epoch milliseconds. The matching message
must be at or after `after_ms` and before `before_ms`; when both are provided,
`after_ms` must be earlier. These are message timestamps, not session-start dates.

Search excludes the current conversation, helper/delegated sources, hidden
messages and pending deletions. Kind and time filters apply before selecting the
newest match in each session. Search returns structured JSON with `sessionId`,
`matchMessageId`, `matchedAtMs`, `role`, and a bounded snippet. Without time
bounds, a title-only match can have a null anchor and `anchorUnavailable: true`.
A title alone cannot satisfy message-time bounds. `hasMore` and `shortened`
describe omitted results or text; the final serialized response is at most 8KiB.

`read_conversation_window` requires the returned `session_id` and exact
`anchor_message_id`, each at most 256 bytes. Optional `before_count` and
`after_count` each default to 5, range from 0 to 19, and must sum to at most 19.
The anchor counts as one message. The requested own ordinary current session is
also readable. A missing, hidden, deleted or foreign anchor returns a generic
`not_found`; an inconsistent or unavailable store returns an error.

The window retains real message identities, roles and chronological order.
Current hot rows take precedence over mirrored cold copies; a hot retention
watermark alone does not imply durable deletion. One exact anchor lookup and two
bounded neighbor pages suffice. There is no full-history page walking or
cross-session fallback. The final serialized UTF-8 response is at most 16KiB,
with at most 20 messages and 2KiB of text per message before further byte
trimming. Attachment presence, shortened text, incomplete tool pairing and
omissions are flagged. Selected recorded run, step and tool-call identifiers and
status flags survive; binary attachments, raw tool arguments and duplicated
result envelopes do not.

Both tools require their exact configured descriptor-name grant. They follow
the existing Read category, which does not ask for separate per-call approval.
Lookup adds no model call, retry, mutation or grant. Each lookup has one
five-second returned-result deadline; underlying blocking SQLite work can finish
later. Search scopes are bounded to 500 eligible sessions and 5,000 captured hot
message identities. Exceeding those bounds is an explicit error, not evidence
that no matching conversation exists. A time filter does not reduce the initial
eligible-session bound by itself.

Past replies and tool results are quoted evidence. They do not establish current
truth, successful external effects, or authority to change memory or act. This
milestone reuses Anima's native stores, ordering and owner policy, informed by
the pinned public MIT Hermes search/window shapes documented in the
[M41 plan](../superpowers/plans/2026-10-13-companion-history-m41.md). It does not
claim full upstream session-lineage behavior or general recall quality.

M41's local gates pass: 2,525 Rust tests with nine intentional ignored tests,
Rust formatting, and 1,245 web tests plus typecheck/build. One fresh synthetic
task searched an archived chat amid time/kind distractors, read its real anchor,
and correctly cited the adjacent cutoff and holiday-exception messages. It used
three model steps, two history calls and 5,420 reported tokens, without a repair
prompt or retry. One quiet same-store restart preserved the exact run, transcript,
answer and sources. All 85 prior canonical memories, 139 earlier history messages,
25 earlier sessions and protected approval/skill/view state remained unchanged;
two ordinary runtime journals and the declared fixture were added. Original
grants/settings were restored and all 14 captured process identities are gone.
This is one bounded case, not a general answer-quality or upstream parity claim.
